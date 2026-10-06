//! Actual Mixer composition fixtures without SDK buffers or native callbacks.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::audio::asio::{AsioBlockRenderer, AsioPcmEncoding::*, AsioRenderError};
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
    let _ = TRACK.try_with(|track| {
        if track.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut values = counts.get();
                values[index] += 1;
                counts.set(values);
            });
        }
    });
}
// SAFETY: Unmodified System allocation arguments are forwarded. Instrumentation
// only accesses allocation-free thread-local scalar cells.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        // SAFETY: Caller supplies a live System allocation, matching layout,
        // and valid nonzero replacement size under GlobalAlloc's contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(2);
        // SAFETY: Caller supplies the live original pointer and its layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn tracked<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|track| track.set(true));
    let result = operation();
    TRACK.with(|track| track.set(false));
    (result, COUNTS.with(Cell::get))
}

fn rig(samples: &[(u64, &[f32])], max_frames: usize) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(4, 2).unwrap();
    let pcm_limits = PcmLimits::new(256, 1024, 4).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    for &(id, data) in samples {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, data.to_vec(), pcm_limits).unwrap(),
        )
        .unwrap();
    }
    let (producer, consumer) = command_queue(4).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(4, 2, 4, max_frames, 4).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn play(voice: u64, sample: u64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(sample),
        at: Timestamp::ZERO,
        gain,
    }
}

#[test]
fn actual_two_voice_mix_is_written_as_literal_heterogeneous_planar_bytes() {
    let (mut producer, mixer) = rig(
        &[
            (1, &[0.5, -0.5, 0.25, 0.75, -1.0, 1.0]),
            (2, &[0.25, 0.25, -0.25, 0.25]),
        ],
        8,
    );
    producer.try_push(play(1, 1, 1.0)).unwrap();
    producer.try_push(play(2, 2, 1.0)).unwrap();
    let mut renderer = AsioBlockRenderer::new(mixer, 3, vec![Int16Lsb, Float32Msb]).unwrap();
    assert_eq!(renderer.frames(), 3);
    assert_eq!(renderer.format(), AudioFormat::new(4, 2).unwrap());
    assert_eq!(renderer.encodings(), &[Int16Lsb, Float32Msb]);
    assert_eq!(renderer.last_render_report(), None);
    let mut left = [0xa5; 6];
    let mut right = [0xa5; 12];
    let report = renderer.render(&mut [&mut left, &mut right]).unwrap();
    assert_eq!(left, [0, 0x60, 0, 0, 0, 0x80]);
    assert_eq!(
        right,
        [0xbe, 0x80, 0, 0, 0x3f, 0x80, 0, 0, 0x3f, 0x80, 0, 0]
    );
    assert_eq!(
        (
            report.start_frame,
            report.frames,
            report.counters.commands_applied
        ),
        (0, 3, 2)
    );
    assert_eq!(renderer.last_render_report(), Some(report));
}

#[test]
fn every_output_is_validated_before_consuming_queue_or_advancing_frame_zero() {
    let (mut producer, mixer) = rig(&[(1, &[0.5, -0.5])], 8);
    producer.try_push(play(1, 1, 1.0)).unwrap();
    let mut renderer = AsioBlockRenderer::new(mixer, 1, vec![Int16Lsb, Float32Msb]).unwrap();
    let mut left = [0xa5; 2];
    assert!(matches!(
        renderer.render(&mut [&mut left]),
        Err(AsioRenderError::InvalidBuffers)
    ));
    assert_eq!(left, [0xa5; 2]);
    let mut wrong_right = [0xa5; 3];
    assert!(matches!(
        renderer.render(&mut [&mut left, &mut wrong_right]),
        Err(AsioRenderError::InvalidBuffers)
    ));
    assert_eq!(left, [0xa5; 2]);
    assert_eq!(wrong_right, [0xa5; 3]);
    assert!(matches!(
        renderer.render(&mut []),
        Err(AsioRenderError::InvalidBuffers)
    ));
    assert_eq!(renderer.last_render_report(), None);
    let mut right = [0xa5; 4];
    let first = renderer.render(&mut [&mut left, &mut right]).unwrap();
    assert_eq!(
        (
            first.start_frame,
            first.frames,
            first.counters.commands_consumed
        ),
        (0, 1, 1)
    );
    assert_eq!(left, [0, 0x40]);
    assert_eq!(right, [0xbf, 0, 0, 0]);
}

#[test]
fn extreme_finite_gain_is_clamped_by_actual_mixer_before_asio_encoding() {
    let (mut producer, mixer) = rig(&[(1, &[0.25, f32::MAX])], 8);
    producer.try_push(play(1, 1, 2.0)).unwrap();
    let mut renderer = AsioBlockRenderer::new(mixer, 1, vec![Int16Lsb, Float32Msb]).unwrap();
    let mut left = [0xa5; 2];
    let mut right = [0xa5; 4];
    let mixed = renderer.render(&mut [&mut left, &mut right]).unwrap();
    assert_eq!(left, [0, 0x40]); // 0.5 in Int16Lsb.
    assert_eq!(right, [0x3f, 0x80, 0, 0]); // Clamped 1.0 in Float32Msb.
    assert_eq!(renderer.last_render_report(), Some(mixed));
    assert_eq!(
        (
            mixed.start_frame,
            mixed.frames,
            mixed.counters.commands_applied
        ),
        (0, 1, 1)
    );
    assert_eq!(mixed.counters.invalid_gains, 0);
    let next = renderer.render(&mut [&mut left, &mut right]).unwrap();
    assert_eq!((next.start_frame, next.frames), (1, 1));
    assert_eq!(left, [0; 2]);
    assert_eq!(right, [0; 4]);
}

#[test]
fn invalid_frame_or_channel_configuration_is_explicit_before_renderer_creation() {
    for encodings in [vec![], vec![Int16Lsb], vec![Int16Lsb, Int16Lsb, Int16Lsb]] {
        let (_producer, mixer) = rig(&[], 8);
        assert!(matches!(
            AsioBlockRenderer::new(mixer, 1, encodings),
            Err(AsioRenderError::InvalidConfiguration)
        ));
    }
    let (_producer, mixer) = rig(&[], 8);
    assert!(matches!(
        AsioBlockRenderer::new(mixer, 0, vec![Int16Lsb, Int16Lsb]),
        Err(AsioRenderError::InvalidConfiguration)
    ));
    let (_producer, mixer) = rig(&[], 2);
    assert!(matches!(
        AsioBlockRenderer::new(mixer, 3, vec![Int16Lsb, Int16Lsb]),
        Err(AsioRenderError::Capacity)
    ));
    let (_producer, mixer) = rig(&[], 2);
    assert!(matches!(
        AsioBlockRenderer::new(mixer, u32::MAX, vec![Int16Lsb, Int16Lsb]),
        Err(AsioRenderError::Capacity)
    ));
}

#[test]
fn repeated_reports_are_contiguous_and_invalid_buffers_keep_last_successful_report() {
    let (mut producer, mixer) = rig(&[(1, &[0.5, -0.5, 0.25, 0.25, 1.0, -1.0])], 8);
    producer.try_push(play(1, 1, 1.0)).unwrap();
    let mut renderer = AsioBlockRenderer::new(mixer, 2, vec![Int16Lsb, Int16Msb]).unwrap();
    let mut left = [0xa5; 4];
    let mut right = [0xa5; 4];
    let first = renderer.render(&mut [&mut left, &mut right]).unwrap();
    assert_eq!(left, [0, 0x40, 0, 0x20]);
    assert_eq!(right, [0xc0, 0, 0x20, 0]);
    assert!(matches!(
        renderer.render(&mut [&mut left]),
        Err(AsioRenderError::InvalidBuffers)
    ));
    assert_eq!(renderer.last_render_report(), Some(first));
    let second = renderer.render(&mut [&mut left, &mut right]).unwrap();
    assert_eq!(left, [0xff, 0x7f, 0, 0]);
    assert_eq!(right, [0x80, 0, 0, 0]);
    assert_eq!(
        (
            first.start_frame,
            first.frames,
            second.start_frame,
            second.frames
        ),
        (0, 2, 2, 2)
    );
    assert_eq!(second.counters.rendered_frames, 4);
    assert_eq!(second.counters.commands_applied, 1);
    assert_eq!(renderer.last_render_report(), Some(second));
}

#[test]
fn allocator_calibration_observes_each_operation() {
    let (_, counts) = tracked(|| {
        let mut bytes = Vec::with_capacity(1);
        bytes.push(3_u8);
        bytes.reserve_exact(1024);
        std::hint::black_box(&bytes);
        drop(bytes);
    });
    assert!(counts.into_iter().all(|count| count > 0));
}

#[test]
fn rendering_and_invalid_buffer_preflight_allocate_nothing() {
    let (mut producer, mixer) = rig(&[(1, &[0.5, -0.5])], 8);
    producer.try_push(play(1, 1, 1.0)).unwrap();
    let mut renderer = AsioBlockRenderer::new(mixer, 1, vec![Int32Lsb18, Float64Msb]).unwrap();
    let mut left = [0xa5; 4];
    let mut right = [0xa5; 8];
    let (result, counts) = tracked(|| renderer.render(&mut [&mut left, &mut right]));
    assert_eq!(counts, [0, 0, 0]);
    assert_eq!(result.unwrap().start_frame, 0);
    let (result, counts) = tracked(|| renderer.render(&mut [&mut left]));
    assert_eq!(counts, [0, 0, 0]);
    assert!(matches!(result, Err(AsioRenderError::InvalidBuffers)));
    let (result, counts) = tracked(|| renderer.render(&mut [&mut left, &mut right]));
    assert_eq!(counts, [0, 0, 0]);
    assert_eq!(result.unwrap().start_frame, 1);
}

#[test]
fn explicit_stereo_to_mono_remix_allocates_nothing_and_preserves_frame_reports() {
    let (mut producer, mixer) = rig(&[(1, &[0.5, -0.5, 0.25, 0.75])], 8);
    producer.try_push(play(1, 1, 1.0)).unwrap();
    let mut renderer = AsioBlockRenderer::new_remixed_recoverable(
        mixer,
        2,
        AudioFormat::new(4, 1).unwrap(),
        vec![Float32Lsb],
        beatkernel::audio::ChannelMatrix::default_mix(2, 1).unwrap(),
    )
    .unwrap_or_else(|f| panic!("{}", f.error()));
    let mut plane = [0xa5; 8];
    let (result, counts) = tracked(|| renderer.render(&mut [&mut plane]));
    assert_eq!(counts, [0; 3]);
    let report = result.unwrap();
    assert_eq!((report.start_frame, report.frames), (0, 2));
    assert_eq!(plane, [0, 0, 0, 0, 0, 0, 0, 0x3f]);
}
