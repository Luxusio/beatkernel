//! Bare Mixer callbacks provide source playback facts, never native presentation ACKs.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct Allocator;
fn count(index: usize) {
    let _ = TRACK.try_with(|track| {
        if track.get() {
            let _ = ALLOCATIONS.try_with(|counts| {
                let mut value = counts.get();
                value[index] += 1;
                counts.set(value);
            });
        }
    });
}
// SAFETY: The allocator forwards every argument unchanged to System.
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
    ALLOCATIONS.with(|counts| counts.set([0; 3]));
    TRACK.with(|track| track.set(true));
    let result = operation();
    TRACK.with(|track| track.set(false));
    (result, ALLOCATIONS.with(Cell::get))
}
fn format(hz: u32) -> AudioFormat {
    AudioFormat::new(hz, 1).unwrap()
}
fn bank(hz: u32) -> SampleBank {
    let limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format(hz), limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format(hz), vec![0.25; 128], limits).unwrap(),
    )
    .unwrap();
    bank
}
fn rig(gated: bool, end: Option<u64>) -> (CommandProducer, Mixer) {
    let (producer, consumer) = if gated {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    let mut config = MixerConfig::new(
        format(1000),
        ClockDomainId(3),
        Timestamp::ZERO,
        AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank(1000), consumer).unwrap())
}
fn play(producer: &mut CommandProducer) {
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
}
fn assert_span(
    span: SourcePlaybackSpan,
    physical: u64,
    end: Option<u64>,
    playback: u64,
    state: SourcePlaybackState,
) {
    assert_eq!(
        (
            span.physical_start(),
            span.physical_end(),
            span.playback_start(),
            span.state()
        ),
        (physical, end, playback, state)
    );
}
fn history(mixer: &Mixer) -> Vec<SourcePlaybackSpan> {
    let history = mixer.source_playback_history().unwrap();
    (0..history.len())
        .map(|index| history.span(index).unwrap())
        .collect()
}
fn converted(mixer: Mixer) -> ConvertedMixer {
    ConvertedMixer::new(
        mixer,
        format(1000),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap_or_else(|failure| panic!("fixture: {:?}", failure.error()))
}

#[test]
fn default_disabled_and_opted_bare_mixer_produce_identical_pcm_and_reports() {
    let (mut legacy_producer, mut legacy) = rig(true, Some(3));
    let (mut opted_producer, mut opted) = rig(true, Some(3));
    legacy_producer.schedule_start_at(2).unwrap();
    opted_producer.schedule_start_at(2).unwrap();
    play(&mut legacy_producer);
    play(&mut opted_producer);
    opted.enable_source_playback_history(3).unwrap();
    for frames in [0, 3, 5, 4] {
        let mut legacy_pcm = [99.0; 8];
        let mut opted_pcm = [99.0; 8];
        let legacy_report = legacy.render(&mut legacy_pcm[..frames]).unwrap();
        let opted_report = opted.render(&mut opted_pcm[..frames]).unwrap();
        assert_eq!(legacy_report, opted_report);
        assert_eq!(legacy_pcm, opted_pcm);
        assert!(legacy.source_playback_history().is_none());
    }
}

#[test]
fn bare_mixer_cold_opt_in_and_empty_render_do_not_fabricate_history() {
    let (_, mut mixer) = rig(false, None);
    assert!(mixer.source_playback_history().is_none());
    assert!(mixer.enable_source_playback_history(0).is_err());
    assert!(mixer.source_playback_history().is_none());
    let (result, allocations) = tracked(|| mixer.enable_source_playback_history(3));
    result.unwrap();
    assert!(allocations[0] > 0);
    assert_eq!(allocations[1], 0);
    assert!(mixer.enable_source_playback_history(4).is_err());
    let (result, allocations) = tracked(|| mixer.render(&mut []));
    assert_eq!(result.unwrap().frames, 0);
    assert_eq!(allocations, [0; 3]);
    let history = mixer.source_playback_history().unwrap();
    assert_eq!(
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame()
        ),
        (1000, 0, 0, 0)
    );
    assert!(history.is_empty());
    assert!(history.span(0).is_none());
    assert!(history.capacity() >= 3);
}

#[test]
fn impossible_cold_allocation_preserves_mixer_and_allows_valid_retry() {
    let (mut producer, mut mixer) = rig(false, None);
    play(&mut producer);
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
    );
    assert!(mixer.enable_source_playback_history(usize::MAX).is_err());
    assert!(mixer.source_playback_history().is_none());
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            producer.counters()
        ),
        before
    );
    mixer.enable_source_playback_history(1).unwrap();
    let mut output = [99.0; 4];
    assert_eq!(
        mixer
            .render(&mut output)
            .unwrap()
            .counters
            .commands_consumed,
        1
    );
    assert_eq!(output, [0.25; 4]);
}

#[test]
fn automatic_bare_render_records_three_real_intervals_in_one_callback() {
    let (mut producer, mut mixer) = rig(true, Some(3));
    producer.schedule_start_at(2).unwrap();
    play(&mut producer);
    mixer.enable_source_playback_history(3).unwrap();
    let mut output = [99.0; 8];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0, 0.0, 0.25, 0.25, 0.25, 0.0, 0.0, 0.0]);
    assert_eq!(
        (report.frames, report.playback_frames, report.paused),
        (8, 3, true)
    );
    assert_eq!(producer.applied_start_frame(), Some(2));
    let history = mixer.source_playback_history().unwrap();
    assert_eq!((history.len(), history.pulled_end_frame()), (3, 8));
    assert_span(
        history.span(0).unwrap(),
        0,
        Some(2),
        0,
        SourcePlaybackState::Frozen,
    );
    assert_span(
        history.span(1).unwrap(),
        2,
        Some(5),
        0,
        SourcePlaybackState::Running,
    );
    assert_span(
        history.span(2).unwrap(),
        5,
        None,
        3,
        SourcePlaybackState::Frozen,
    );
}

#[test]
fn bare_pause_resume_records_actual_cursors_and_merges_frozen_callbacks() {
    let (mut producer, mut mixer) = rig(false, None);
    mixer.enable_source_playback_history(3).unwrap();
    mixer.render(&mut [0.0; 4]).unwrap();
    producer.request_pause(true);
    assert!(mixer.render(&mut [0.0; 3]).unwrap().paused);
    mixer.render(&mut [0.0; 2]).unwrap();
    producer.request_pause(false);
    assert!(!mixer.render(&mut [0.0; 4]).unwrap().paused);
    let history = mixer.source_playback_history().unwrap();
    assert_eq!((history.len(), history.pulled_end_frame()), (3, 13));
    assert_span(
        history.span(0).unwrap(),
        0,
        Some(4),
        0,
        SourcePlaybackState::Running,
    );
    assert_span(
        history.span(1).unwrap(),
        4,
        Some(9),
        4,
        SourcePlaybackState::Frozen,
    );
    assert_span(
        history.span(2).unwrap(),
        9,
        None,
        4,
        SourcePlaybackState::Running,
    );
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (13, 8)
    );
}

#[test]
fn cold_enable_at_nonzero_paused_cursor_captures_only_new_history() {
    let (mut producer, mut mixer) = rig(false, None);
    mixer.render(&mut [0.0; 5]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 3]).unwrap();
    mixer.enable_source_playback_history(2).unwrap();
    let history = mixer.source_playback_history().unwrap();
    assert_eq!(
        (
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame()
        ),
        (8, 5, 8)
    );
    assert!(history.is_empty());
    mixer.render(&mut [0.0; 2]).unwrap();
    producer.request_pause(false);
    mixer.render(&mut [0.0; 4]).unwrap();
    let history = mixer.source_playback_history().unwrap();
    assert_span(
        history.span(0).unwrap(),
        8,
        Some(10),
        5,
        SourcePlaybackState::Frozen,
    );
    assert_span(
        history.span(1).unwrap(),
        10,
        None,
        5,
        SourcePlaybackState::Running,
    );
    assert_eq!(history.pulled_end_frame(), 14);
}

#[test]
fn bare_capacity_refusal_preserves_pcm_queue_gate_and_cursors_for_same_owner_retry() {
    let (mut producer, mut mixer) = rig(true, Some(3));
    producer.schedule_start_at(2).unwrap();
    play(&mut producer);
    mixer.enable_source_playback_history(2).unwrap();
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
    );
    let mut output = [99.0; 8];
    for _ in 0..2 {
        let (result, allocations) = tracked(|| mixer.render(&mut output));
        assert_eq!(result, Err(AudioError::RenderCapacity));
        assert_eq!(allocations, [0; 3]);
        assert_eq!(output, [99.0; 8]);
        assert_eq!(
            (
                mixer.frame_cursor(),
                mixer.playback_frame_cursor(),
                mixer.counters(),
                producer.counters()
            ),
            before
        );
        assert_eq!(producer.applied_start_frame(), None);
        assert!(mixer.source_playback_history().unwrap().is_empty());
    }
    let mut retry = [99.0; 3];
    let report = mixer.render(&mut retry).unwrap();
    assert_eq!(report.counters.commands_consumed, 1);
    assert_eq!(retry, [0.0, 0.0, 0.25]);
    assert_eq!(producer.applied_start_frame(), Some(2));
    assert_eq!(mixer.source_playback_history().unwrap().len(), 2);
}

#[test]
fn full_capacity_running_merge_and_empty_pending_pause_are_allocation_free() {
    let (mut producer, mut mixer) = rig(false, None);
    play(&mut producer);
    mixer.enable_source_playback_history(1).unwrap();
    mixer.render(&mut [0.0; 4]).unwrap();
    for _ in 0..8 {
        let (result, allocations) = tracked(|| mixer.render(&mut [0.0; 4]));
        result.unwrap();
        assert_eq!(allocations, [0; 3]);
    }
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        history(&mixer),
    );
    producer.request_pause(true);
    let (result, allocations) = tracked(|| mixer.render(&mut []));
    assert_eq!(result.unwrap().frames, 0);
    assert_eq!(allocations, [0; 3]);
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            history(&mixer)
        ),
        before
    );
    let mut output = [99.0; 4];
    assert_eq!(mixer.render(&mut output), Err(AudioError::RenderCapacity));
    assert_eq!(output, [99.0; 4]);
    producer.request_pause(false);
    mixer.render(&mut output).unwrap();
    assert_eq!(mixer.source_playback_history().unwrap().len(), 1);
}

#[test]
fn enabled_advanced_bare_owner_moves_to_converter_without_reseeding_or_duplicate_history() {
    let (mut producer, mut mixer) = rig(false, None);
    mixer.enable_source_playback_history(3).unwrap();
    mixer.render(&mut [0.0; 5]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 2]).unwrap();
    let before = history(&mixer);
    let mut owner = converted(mixer);
    assert_eq!(
        (
            owner
                .source_playback_history()
                .unwrap()
                .initial_physical_frame(),
            owner.source_playback_history().unwrap().pulled_end_frame()
        ),
        (0, 7)
    );
    assert_eq!(owner.mixer().source_playback_history().unwrap().len(), 2);
    assert!(owner.enable_source_playback_history(8).is_err());
    assert_eq!(
        (0..owner.source_playback_history().unwrap().len())
            .map(|i| owner.source_playback_history().unwrap().span(i).unwrap())
            .collect::<Vec<_>>(),
        before
    );
    let held = owner.render_held(&mut [0.0; 4]).unwrap();
    assert!(held.source.is_none());
    assert_eq!(
        owner.source_playback_history().unwrap().pulled_end_frame(),
        7
    );
    producer.request_pause(false);
    owner.render(&mut [0.0; 4]).unwrap();
    let history = owner.source_playback_history().unwrap();
    assert_eq!(history.len(), 3);
    assert_span(
        history.span(2).unwrap(),
        7,
        None,
        5,
        SourcePlaybackState::Running,
    );
    assert_eq!(
        owner
            .mixer()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        history.pulled_end_frame()
    );
    owner
        .retarget(format(2000), ChannelMatrix::default_mix(1, 1).unwrap(), 32)
        .unwrap();
    let retained = history_snapshot(&owner);
    let mut moved = owner;
    assert_eq!(history_snapshot(&moved), retained);
    moved.render(&mut [0.0; 4]).unwrap();
    assert_eq!(moved.source_playback_history().unwrap().len(), 3);
}
fn history_snapshot(owner: &ConvertedMixer) -> Vec<SourcePlaybackSpan> {
    let history = owner.source_playback_history().unwrap();
    (0..history.len())
        .map(|i| history.span(i).unwrap())
        .collect()
}

#[test]
fn converted_enable_delegates_to_common_owner_and_retains_late_target_guard() {
    let (_, mixer) = rig(false, None);
    let mut owner = converted(mixer);
    owner.enable_source_playback_history(2).unwrap();
    assert!(owner.mixer().source_playback_history().is_some());
    owner.render(&mut [0.0; 4]).unwrap();
    assert_eq!(owner.mixer().source_playback_history().unwrap().len(), 1);
    for held in [false, true] {
        let (_, mixer) = rig(false, None);
        let mut owner = converted(mixer);
        if held {
            owner.render_held(&mut [0.0; 1]).unwrap();
        } else {
            owner.render(&mut [0.0; 1]).unwrap();
        }
        let before = (
            owner.target_time(),
            owner.source_position(),
            owner.pulled_source_frame_cursor(),
        );
        assert!(owner.enable_source_playback_history(2).is_err());
        assert!(owner.mixer().source_playback_history().is_none());
        assert_eq!(
            (
                owner.target_time(),
                owner.source_position(),
                owner.pulled_source_frame_cursor()
            ),
            before
        );
    }
}

#[test]
fn bare_history_refusal_preserves_actual_practice_request_until_same_owner_resumes() {
    use beatkernel::{time::Duration, transport::Rate};
    let bank = bank(1000);
    let program = PreparedPracticeProgram::new_with_speed(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        PracticeLimits::new(8, 4, 128, 8, 16).unwrap(),
        Rate::new(3, 2).unwrap(),
        Duration::ZERO,
        false,
    )
    .unwrap();
    let (mut controller, endpoint) = practice_queue(&program).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format(1000),
            ClockDomainId(3),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), true).unwrap(),
        )
        .unwrap();
    mixer.enable_source_playback_history(1).unwrap();
    mixer.render(&mut [0.0; 3]).unwrap();
    assert_eq!(
        controller
            .try_pop_projected_receipt()
            .unwrap()
            .receipt
            .generation,
        1
    );
    controller
        .try_request_next(
            7,
            1,
            PracticeRegion::new(
                Timestamp::from_nanos(1_000_000),
                Timestamp::from_nanos(50_000_000),
                true,
            )
            .unwrap(),
        )
        .unwrap();
    play(&mut producer);
    producer.request_pause(true);
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
        history(&mixer),
    );
    let mut output = [99.0; 4];
    let (result, allocations) = tracked(|| mixer.render(&mut output));
    assert_eq!(result, Err(AudioError::RenderCapacity));
    assert_eq!(allocations, [0; 3]);
    assert_eq!(output, [99.0; 4]);
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            producer.counters(),
            history(&mixer)
        ),
        before
    );
    assert_eq!(controller.generation(), 1);
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    producer.request_pause(false);
    mixer.render(&mut output).unwrap();
    let receipt = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (
            receipt.receipt.request_id,
            receipt.receipt.generation,
            receipt.receipt.kind
        ),
        (7, 2, PracticeBoundaryKind::Requested)
    );
    assert_eq!(
        (
            receipt.receipt.physical_frame,
            receipt.receipt.playback_frame
        ),
        (3, 3)
    );
    assert_eq!(
        receipt.receipt.requested_song_time,
        Timestamp::from_nanos(1_000_000)
    );
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    assert_eq!(mixer.source_playback_history().unwrap().len(), 1);
    assert_eq!(
        mixer.counters().commands_consumed,
        before.2.commands_consumed + 1
    );
}
