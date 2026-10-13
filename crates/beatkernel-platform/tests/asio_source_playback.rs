//! SDK-free actual renderer histories are source facts, not native delivery ACKs.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::audio::asio::{AsioBlockRenderer, AsioPcmEncoding, AsioRenderError};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct Allocator;
fn count(index: usize) {
    let _ = TRACK.try_with(|enabled| {
        if enabled.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut values = counts.get();
                values[index] += 1;
                counts.set(values);
            });
        }
    });
}
// SAFETY: All System arguments pass unchanged; tracking only touches scalar TLS.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(2);
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn tracked<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|enabled| enabled.set(true));
    let result = operation();
    TRACK.with(|enabled| enabled.set(false));
    (result, COUNTS.with(Cell::get))
}
fn rig(history: Option<usize>, end: Option<u64>, prefix: usize) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(4, 1).unwrap();
    let limits = PcmLimits::new(128, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 32], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(7),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    let mut mixer = Mixer::new(config, bank, consumer).unwrap();
    if prefix != 0 {
        mixer.render(&mut [0.0; 16][..prefix]).unwrap();
    }
    if let Some(capacity) = history {
        mixer.enable_source_playback_history(capacity).unwrap();
    }
    (producer, mixer)
}
fn renderer(mixer: Mixer, frames: u32) -> AsioBlockRenderer {
    AsioBlockRenderer::new(mixer, frames, vec![AsioPcmEncoding::Float32Lsb]).unwrap()
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(
        TargetTime::new(frame / 4, frame % 4, 4).unwrap(),
        0,
        0,
        1,
        0,
        1,
    )
    .unwrap()
}
fn assert_span(
    span: SourcePlaybackSpan,
    physical: u64,
    end: Option<u64>,
    playback: u64,
    state: SourcePlaybackState,
) {
    assert_eq!(
        span,
        SourcePlaybackSpan::new(physical, end, playback, state).unwrap()
    );
}

#[test]
fn cold_empty_history_is_borrowed_and_default_renderer_retains_legacy_behavior() {
    let (_, mixer) = rig(Some(8), None, 0);
    let renderer = renderer(mixer, 2);
    let history = renderer.source_playback_history().unwrap();
    assert!(history.is_empty());
    assert_eq!(
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame()
        ),
        (4, 0, 0, 0)
    );
    assert_eq!(renderer.last_render_report(), None);
    let (_, mixer) = rig(None, None, 0);
    let mut legacy = self::renderer(mixer, 2);
    assert!(legacy.source_playback_history().is_none());
    let mut output = [0xa5; 8];
    assert_eq!(
        legacy.render(&mut [&mut output]).unwrap().playback_frames,
        2
    );
    assert_eq!(output, [0, 0, 0x80, 0x3e, 0, 0, 0x80, 0x3e]);
    assert!(legacy.source_playback_history().is_none());
}

#[test]
fn actual_pause_resume_and_merges_preserve_historical_playback_coordinates() {
    let (mut producer, mixer) = rig(Some(8), None, 0);
    let mut renderer = renderer(mixer, 2);
    let mut output = [0xa5; 8];
    renderer.render(&mut [&mut output]).unwrap();
    producer.request_pause(true);
    assert!(renderer.render(&mut [&mut output]).unwrap().paused);
    renderer.render(&mut [&mut output]).unwrap();
    producer.request_pause(false);
    renderer.render(&mut [&mut output]).unwrap();
    let history = renderer.source_playback_history().unwrap();
    assert_eq!((history.len(), history.pulled_end_frame()), (3, 8));
    assert_span(
        history.span(0).unwrap(),
        0,
        Some(2),
        0,
        SourcePlaybackState::Running,
    );
    assert_span(
        history.span(1).unwrap(),
        2,
        Some(6),
        2,
        SourcePlaybackState::Frozen,
    );
    assert_span(
        history.span(2).unwrap(),
        6,
        None,
        2,
        SourcePlaybackState::Running,
    );
    assert_eq!(
        history.lookup_at_source_time(query(2)).unwrap().state(),
        SourcePlaybackState::Frozen
    );
    assert_eq!(
        history.lookup_at_source_time(query(6)).unwrap().state(),
        SourcePlaybackState::Running
    );
    assert!(history.lookup_at_source_time(query(8)).is_ok());
    let future = InterpolatedTargetTime::new(
        TargetTime::new(2, 0, 1).unwrap(),
        0,
        1,
        u64::MAX,
        1,
        u64::MAX - 1,
    )
    .unwrap();
    assert!(history.lookup_at_source_time(future).is_err());
}

#[test]
fn finite_endpoint_keeps_running_and_frozen_intervals_from_one_real_render() {
    let (_, mixer) = rig(Some(4), Some(1), 0);
    let mut renderer = renderer(mixer, 3);
    let mut output = [0xa5; 12];
    let report = renderer.render(&mut [&mut output]).unwrap();
    assert_eq!(
        (
            report.frames,
            report.playback_frames,
            report.playback_end_physical_frame
        ),
        (3, 1, Some(1))
    );
    let history = renderer.source_playback_history().unwrap();
    assert_eq!((history.len(), history.pulled_end_frame()), (2, 3));
    assert_span(
        history.span(0).unwrap(),
        0,
        Some(1),
        0,
        SourcePlaybackState::Running,
    );
    assert_span(
        history.span(1).unwrap(),
        1,
        None,
        1,
        SourcePlaybackState::Frozen,
    );
}

#[test]
fn advanced_seed_and_complete_owner_transfer_keep_history_without_reconstruction() {
    let (mut producer, mixer) = rig(Some(8), None, 3);
    let mut renderer = renderer(mixer, 2);
    let mut output = [0xa5; 8];
    renderer.render(&mut [&mut output]).unwrap();
    producer.request_pause(true);
    renderer.render(&mut [&mut output]).unwrap();
    let before = renderer.source_playback_history().unwrap();
    assert_eq!(
        (
            before.initial_physical_frame(),
            before.initial_playback_frame(),
            before.pulled_end_frame()
        ),
        (3, 3, 7)
    );
    let spans = [before.span(0), before.span(1)];
    let mixer = renderer.take_mixer().unwrap();
    assert!(renderer.source_playback_history().is_none());
    assert!(renderer.take_mixer().is_none());
    output.fill(0xa5);
    assert_eq!(
        renderer.render(&mut [&mut output]),
        Err(AsioRenderError::MixerUnavailable)
    );
    assert_eq!(output, [0xa5; 8]);
    let moved = self::renderer(mixer, 2);
    let history = moved.source_playback_history().unwrap();
    assert_eq!([history.span(0), history.span(1)], spans);
    assert_eq!(
        (
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame()
        ),
        (3, 3, 7)
    );
}

#[test]
fn invalid_planes_and_capacity_refusal_preserve_actual_history_and_owner() {
    let (mut producer, mixer) = rig(Some(1), None, 0);
    let mut renderer = renderer(mixer, 2);
    let mut output = [0xa5; 8];
    assert_eq!(
        renderer.render(&mut [&mut output[..7]]),
        Err(AsioRenderError::InvalidBuffers)
    );
    assert!(renderer.source_playback_history().unwrap().is_empty());
    let first = renderer.render(&mut [&mut output]).unwrap();
    producer.request_pause(true);
    output.fill(0xa5);
    assert_eq!(
        renderer.render(&mut [&mut output]),
        Err(AsioRenderError::Core(AudioError::RenderCapacity))
    );
    assert_eq!(output, [0xa5; 8]);
    assert_eq!(renderer.last_render_report(), Some(first));
    assert_eq!(
        renderer
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        2
    );
    producer.request_pause(false);
    assert_eq!(renderer.render(&mut [&mut output]).unwrap().start_frame, 2);
    assert_eq!(renderer.source_playback_history().unwrap().len(), 1);
}

#[test]
fn extreme_finite_gain_is_valid_clipped_delivery_with_real_source_history() {
    let (mut producer, mixer) = rig(Some(8), None, 0);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: f32::MAX,
        })
        .unwrap();
    let mut renderer = renderer(mixer, 2);
    let mut output = [0xa5; 8];
    let report = renderer.render(&mut [&mut output]).unwrap();
    assert_eq!(output, [0, 0, 0x80, 0x3f, 0, 0, 0x80, 0x3f]);
    assert_eq!(report.counters.commands_applied, 2);
    assert_eq!(renderer.last_render_report(), Some(report));
    assert_eq!(
        renderer
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        2
    );
}

#[test]
fn cold_setup_capacity_refusal_returns_complete_advanced_owner_and_history() {
    let (_, mut mixer) = rig(Some(8), None, 3);
    mixer.render(&mut [0.0; 2]).unwrap();
    let expected = mixer.source_playback_history().unwrap().span(0);
    let failure = AsioBlockRenderer::new_recoverable(mixer, 17, vec![AsioPcmEncoding::Float32Lsb])
        .err()
        .unwrap();
    assert_eq!(*failure.error(), AsioRenderError::Capacity);
    let mut recovered = failure.into_parts().1.unwrap();
    assert_eq!(
        (recovered.frame_cursor(), recovered.playback_frame_cursor()),
        (5, 5)
    );
    let history = recovered.source_playback_history().unwrap();
    assert_eq!(
        (
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame()
        ),
        (3, 3, 5)
    );
    assert_eq!(history.span(0), expected);
    assert_eq!(recovered.render(&mut [0.0; 1]).unwrap().start_frame, 5);
}

#[test]
fn hot_actual_render_borrow_lookup_pause_resume_and_refusal_do_not_allocate() {
    let (mut producer, mixer) = rig(Some(8), None, 0);
    let mut renderer = renderer(mixer, 2);
    let mut output = [0xa5; 8];
    let (result, counts) = tracked(|| renderer.render(&mut [&mut output]));
    result.unwrap();
    assert_eq!(counts, [0; 3]);
    producer.request_pause(true);
    let (result, counts) = tracked(|| renderer.render(&mut [&mut output]));
    result.unwrap();
    assert_eq!(counts, [0; 3]);
    producer.request_pause(false);
    let (result, counts) = tracked(|| renderer.render(&mut [&mut output]));
    result.unwrap();
    assert_eq!(counts, [0; 3]);
    let (result, counts) = tracked(|| {
        renderer
            .source_playback_history()
            .unwrap()
            .lookup_at_source_time(query(4))
    });
    assert_eq!(result.unwrap().state(), SourcePlaybackState::Running);
    assert_eq!(counts, [0; 3]);
    let (result, counts) = tracked(|| renderer.render(&mut [&mut output[..7]]));
    assert_eq!(result, Err(AsioRenderError::InvalidBuffers));
    assert_eq!(counts, [0; 3]);
}

#[test]
fn allocation_instrumentation_observes_allocate_reallocate_and_deallocate() {
    let (_, counts) = tracked(|| {
        let mut bytes = Vec::with_capacity(1);
        bytes.push(7_u8);
        bytes.reserve_exact(512);
        std::hint::black_box(&bytes);
        drop(bytes);
    });
    assert!(counts.into_iter().all(|count| count > 0));
}
