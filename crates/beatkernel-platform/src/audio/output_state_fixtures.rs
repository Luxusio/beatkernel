//! Software admission tests: native presentation is deliberately not inferred.
use super::*;
use crate::audio::{asio::AsioPcmEncoding, DeviceFormat, SampleEncoding};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static FAIL_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
    static TRACK_HEAP: Cell<bool> = const { Cell::new(false) };
    static HEAP_CALLS: Cell<usize> = const { Cell::new(0) };
}
struct ObservedAllocator;
#[global_allocator]
static ALLOCATOR: ObservedAllocator = ObservedAllocator;
fn heap_call() -> bool {
    let _ = TRACK_HEAP.try_with(|tracked| {
        if tracked.get() {
            let _ = HEAP_CALLS.try_with(|n| n.set(n.get() + 1));
        }
    });
    FAIL_AFTER
        .try_with(|remaining| match remaining.get() {
            Some(0) => {
                remaining.set(None);
                true
            }
            Some(n) => {
                remaining.set(Some(n - 1));
                false
            }
            None => false,
        })
        .unwrap_or(false)
}
// SAFETY: every successful allocation and matching destruction delegates to
// System with the original pointer/layout; only armed allocation returns null.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for ObservedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if heap_call() {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc(layout) }
        }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if heap_call() {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc_zeroed(layout) }
        }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if heap_call() {
            std::ptr::null_mut()
        } else {
            unsafe { System.realloc(pointer, layout, size) }
        }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let _ = TRACK_HEAP.try_with(|tracked| {
            if tracked.get() {
                let _ = HEAP_CALLS.try_with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.dealloc(pointer, layout) }
    }
}
struct HeapReset;
impl Drop for HeapReset {
    fn drop(&mut self) {
        FAIL_AFTER.set(None);
        TRACK_HEAP.set(false);
    }
}
pub(crate) fn fail_allocation<T>(index: usize, work: impl FnOnce() -> T) -> T {
    FAIL_AFTER.set(Some(index));
    let reset = HeapReset;
    let result = work();
    drop(reset);
    result
}

#[test]
fn actual_cold_allocation_refusals_return_original_mixer_and_retry_growth_without_loss() {
    for index in 0..2 {
        let (mut producer, mixer) = rig();
        let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
        let result = fail_allocation(index, || {
            NativeOutputState::new(mixer, target(2), Some(matrix), 4)
        });
        let failure = match result {
            Err(f) => f,
            Ok(_) => panic!("armed cold allocation must refuse"),
        };
        assert_eq!(failure.error(), &AudioError::AllocationFailed);
        assert_eq!(failure.mixer().unwrap().frame_cursor(), 0);
        let (_, mixer) = failure.into_parts();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 0.5,
            })
            .unwrap();
        let mut state = prepared(
            mixer.unwrap(),
            target(2),
            Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
            4,
        );
        state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        let report = state.pending_report();
        let basis = state.output_frame_basis();
        let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
        let refusal = fail_allocation(0, || state.reconfigure(target(2), Some(matrix), 16));
        assert_eq!(refusal, Err(AudioError::AllocationFailed));
        assert_eq!(state.max_frames(), 4);
        assert_eq!(state.pending_report(), report);
        assert_eq!(state.output_frame_basis(), basis);
        assert_eq!(
            state.pending_samples(),
            [-0.375, 0.1875, 0.5625, -0.28125, -0.75, 0.375]
        );
        state
            .reconfigure(
                target(2),
                Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
                16,
            )
            .unwrap();
        state.admit(3).unwrap();
        state.render_pending(2).unwrap();
        // The two voices sum to -9/8; Mixer saturates to -1 before remix.
        assert_eq!(state.pending_samples(), [0.9375, -0.46875, -1., 0.5]);
    }
}

#[test]
fn prepared_render_and_suffix_replay_allocate_reallocate_and_destroy_nothing() {
    let (_, mixer) = rig();
    let mut state = prepared(
        mixer,
        target(2),
        Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
        8,
    );
    HEAP_CALLS.set(0);
    TRACK_HEAP.set(true);
    let reset = HeapReset;
    let rendered = state.render_pending(4);
    let first = state.admit(1);
    let basis = state.output_frame_basis();
    let suffix = state.pending_samples().len();
    let refused = state.render_pending(1);
    let last = state.admit(3);
    let next = state.render_pending(2);
    drop(reset);
    let heap_calls = HEAP_CALLS.get();
    assert!(rendered.is_ok() && first.is_ok() && last.is_ok() && next.is_ok());
    assert_eq!(refused, Err(AudioError::InvalidBuffer));
    assert_eq!(basis.start_physical_frame(), 1);
    assert_eq!(suffix, 6);
    assert_eq!(heap_calls, 0);
    assert_eq!(state.pending_samples(), [0.625, -0.3125, -0.75, 0.375]);
}

// Shared by actual native pump fixtures; the crate has one global allocator.
pub(crate) fn count_heap_calls<R>(work: impl FnOnce() -> R) -> (R, usize) {
    HEAP_CALLS.set(0);
    TRACK_HEAP.set(true);
    let reset = HeapReset;
    let result = work();
    drop(reset);
    (result, HEAP_CALLS.get())
}

#[test]
fn native_acquisition_preflight_does_not_commit_a_new_render_capacity() {
    let (_, mixer) = rig();
    let mut state = prepared(mixer, target(1), None, 4);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let report = state.pending_report();
    let basis = state.output_frame_basis();
    let pcm = state.pending_samples().to_vec();
    let (result, calls) = count_heap_calls(|| state.validate_reconfigure(target(1), None, 16));
    assert_eq!(result, Ok(()));
    assert_eq!(calls, 0);
    assert_eq!(state.max_frames(), 4);
    assert_eq!(state.pending_report(), report);
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(state.admitted_frames(), 1);
    state.reconfigure(target(1), None, 16).unwrap();
    assert_eq!(state.max_frames(), 16);
    assert_eq!(state.pending_samples(), pcm);
}

fn rig() -> (CommandProducer, Mixer) {
    rig_with_render_limit(32)
}
fn rig_with_render_limit(max_render_frames: usize) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(8, 1).unwrap();
    let limits = PcmLimits::new(128, 512, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            vec![0.125, -0.25, 0.375, -0.5, 0.625, -0.75, 0.875, -1.],
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(71),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, max_render_frames, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn target(channels: u16) -> DeviceFormat {
    DeviceFormat::new(8, channels, SampleEncoding::Float32, None).unwrap()
}
fn prepared(
    mixer: Mixer,
    format: DeviceFormat,
    matrix: Option<ChannelMatrix>,
    frames: usize,
) -> NativeOutputState {
    match NativeOutputState::new(mixer, format, matrix, frames) {
        Ok(state) => state,
        Err(_) => panic!("valid cold preparation must succeed"),
    }
}

fn prepared_storage(state: &NativeOutputState) -> (usize, Option<(usize, usize, usize)>) {
    (
        state.samples.len(),
        state.remix.as_ref().map(|converter| {
            (
                converter.max_output_frames(),
                converter.max_source_frames(),
                converter.source_lookahead_frames(),
            )
        }),
    )
}

#[test]
fn coreaudio_actual_max_storage_grows_and_rebounds_without_losing_complete_source_state() {
    for remixed in [false, true] {
        let (mut producer, mixer) = rig_with_render_limit(AudioLimits::MAX_RENDER_FRAMES);
        let configuration = mixer.configuration();
        let channels = if remixed { 2 } else { 1 };
        let format = target(channels);
        let matrix = remixed.then(|| ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap());
        let mut state = NativeOutputState::from_mixer(mixer);
        state
            .reconfigure_for_coreaudio(format, matrix.clone(), 64)
            .unwrap();
        assert_eq!(
            prepared_storage(&state),
            (64 * usize::from(channels), remixed.then_some((64, 64, 0)))
        );
        state.prepare_growing_native_source_history(64, 4).unwrap();
        state.render_pending(2).unwrap();
        state.admit(2).unwrap();
        producer.request_pause(true);
        assert!(state.render_pending(2).unwrap().paused);
        state.admit(2).unwrap();
        producer.request_pause(false);
        assert!(!state.render_pending(2).unwrap().paused);
        state.admit(2).unwrap();
        let mut completed = 0;
        assert_eq!(
            state.apply_source_history_retirement(
                crate::audio::source_history_retirement_mailbox::SourceHistoryRetirementRequest {
                    revision: 2,
                    source_pin: 4,
                },
                &mut completed,
            ),
            Ok(2)
        );
        assert_eq!(completed, 2);
        let report = state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        assert_eq!(state.output_frame_basis().start_physical_frame(), 7);
        assert_eq!(state.mixer().frame_cursor(), 10);
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::from_nanos(1_000_000_000),
                gain: 0.5,
            })
            .unwrap();
        producer.request_pause(true);
        let producer_counters = producer.counters();
        let mut expected = cold_output_snapshot(&state);
        assert_eq!(expected.history.as_ref().unwrap().retirement_revision, 1);
        assert_eq!(expected.history.as_ref().unwrap().retained_start, 4);
        assert_eq!(expected.history.as_ref().unwrap().identity, (8, 0, 0));

        state
            .reconfigure_for_coreaudio(format, matrix.clone(), 128)
            .unwrap();

        expected.max_frames = 128;
        assert_eq!(cold_output_snapshot(&state), expected);
        assert_eq!(state.pending_report(), Some(report));
        assert_eq!(producer.counters(), producer_counters);
        assert_eq!(
            prepared_storage(&state),
            (
                128 * usize::from(channels),
                remixed.then_some((128, 128, 0))
            )
        );
        state.prepare_growing_native_source_history(128, 4).unwrap();
        let capacity = state.mixer().source_playback_history().unwrap().capacity();
        assert!(capacity >= 132);
        expected.history.as_mut().unwrap().capacity = capacity;
        assert_eq!(cold_output_snapshot(&state), expected);
        state.admit(3).unwrap();
        let paused = state.render_pending(128).unwrap();
        assert!(paused.paused);
        assert_eq!(
            (
                paused.start_frame,
                paused.playback_start_frame,
                paused.playback_frames
            ),
            (10, 8, 0)
        );
        assert!(state.pending_samples().iter().all(|sample| *sample == 0.));
        state.admit(64).unwrap();
        let mut expected = cold_output_snapshot(&state);

        state
            .reconfigure_for_coreaudio(format, matrix.clone(), 64)
            .unwrap();

        expected.max_frames = 64;
        assert_eq!(cold_output_snapshot(&state), expected);
        assert_eq!(state.pending_report(), Some(paused));
        assert_eq!(state.output_frame_basis().start_physical_frame(), 74);
        assert_eq!(
            prepared_storage(&state),
            (128 * usize::from(channels), remixed.then_some((64, 64, 0)))
        );
        state.admit(64).unwrap();
        producer.request_pause(false);
        let (next, heap) = count_heap_calls(|| state.render_pending(64));
        let next = next.unwrap();
        assert_eq!(heap, 0);
        assert_eq!((next.start_frame, next.playback_start_frame), (138, 8));
        assert_eq!(next.frames, 64);
        if remixed {
            assert_eq!(
                &state.pending_samples()[..4],
                &[0.0625, -0.03125, -0.125, 0.0625]
            );
        } else {
            assert_eq!(&state.pending_samples()[..2], &[0.0625, -0.125]);
        }
        assert_eq!(state.mixer().counters().commands_consumed, 2);
        assert_eq!(state.mixer().configuration(), configuration);
        assert_eq!(
            state.mixer().configuration().limits().max_render_frames(),
            AudioLimits::MAX_RENDER_FRAMES
        );
    }
}

#[test]
fn coreaudio_actual_max_refusals_preserve_storage_interpretation_history_and_pending_suffix() {
    let (_, mixer) = rig_with_render_limit(256);
    let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
    let mut state = NativeOutputState::from_mixer(mixer);
    state
        .reconfigure_for_coreaudio(target(2), Some(matrix.clone()), 64)
        .unwrap();
    state.prepare_growing_native_source_history(64, 4).unwrap();
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let before = cold_output_snapshot(&state);
    let storage = prepared_storage(&state);
    for (format, matrix, frames, error) in [
        (
            target(2),
            Some(matrix.clone()),
            0,
            AudioError::InvalidCapacity,
        ),
        (
            target(2),
            Some(matrix.clone()),
            257,
            AudioError::RenderCapacity,
        ),
        (
            target(2),
            Some(matrix.clone()),
            usize::MAX,
            AudioError::RenderCapacity,
        ),
        (target(2), None, 128, AudioError::InvalidFormat),
        (target(1), None, 128, AudioError::InvalidFormat),
        (
            target(2),
            Some(ChannelMatrix::new(1, 2, &[0.5, 1.]).unwrap()),
            128,
            AudioError::InvalidFormat,
        ),
        (
            target(2),
            Some(ChannelMatrix::default_mix(2, 2).unwrap()),
            128,
            AudioError::InvalidFormat,
        ),
        (
            DeviceFormat::new(16, 2, SampleEncoding::Float32, None).unwrap(),
            Some(matrix.clone()),
            128,
            AudioError::InvalidFormat,
        ),
        (
            DeviceFormat::new(
                8,
                2,
                SampleEncoding::Pcm {
                    container_bits: 16,
                    valid_bits: 16,
                },
                None,
            )
            .unwrap(),
            Some(matrix.clone()),
            128,
            AudioError::InvalidFormat,
        ),
        (
            DeviceFormat::new(8, 2, SampleEncoding::Float32, Some(3)).unwrap(),
            Some(matrix.clone()),
            128,
            AudioError::InvalidFormat,
        ),
    ] {
        let result = state.reconfigure_for_coreaudio(format, matrix, frames);
        assert_eq!(result, Err(error));
        assert_eq!(cold_output_snapshot(&state), before);
        assert_eq!(prepared_storage(&state), storage);
    }
    state.mark_admission_uncertain();
    let quarantined = cold_output_snapshot(&state);
    for frames in [0, 128, 257] {
        let matrix = matrix.clone();
        let result = state.reconfigure_for_coreaudio(target(2), Some(matrix), frames);
        assert_eq!(result, Err(AudioError::InvalidBuffer));
        assert_eq!(cold_output_snapshot(&state), quarantined);
        assert_eq!(prepared_storage(&state), storage);
    }
}

#[test]
fn coreaudio_actual_max_allocation_refusals_keep_original_storage_and_allow_exact_retry() {
    for index in 0..2 {
        let (_, mixer) = rig_with_render_limit(256);
        let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
        let mut state = NativeOutputState::from_mixer(mixer);
        state
            .reconfigure_for_coreaudio(target(2), Some(matrix.clone()), 64)
            .unwrap();
        state.prepare_growing_native_source_history(64, 4).unwrap();
        state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        let mut expected = cold_output_snapshot(&state);
        let storage = prepared_storage(&state);
        let retry_matrix = matrix.clone();

        assert_eq!(
            fail_allocation(index, || {
                state.reconfigure_for_coreaudio(target(2), Some(retry_matrix), 128)
            }),
            Err(AudioError::AllocationFailed)
        );

        assert_eq!(cold_output_snapshot(&state), expected);
        assert_eq!(prepared_storage(&state), storage);
        state
            .reconfigure_for_coreaudio(target(2), Some(matrix), 128)
            .unwrap();
        expected.max_frames = 128;
        assert_eq!(cold_output_snapshot(&state), expected);
        assert_eq!(prepared_storage(&state), (256, Some((128, 128, 0))));
    }
}

#[test]
fn legacy_native_preparation_still_prepares_remix_to_the_immutable_mixer_ceiling() {
    let (_, mixer) = rig_with_render_limit(256);
    let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
    let mut state = prepared(mixer, target(2), Some(matrix.clone()), 64);
    assert_eq!(prepared_storage(&state), (128, Some((256, 256, 0))));
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let mut expected = cold_output_snapshot(&state);

    state.reconfigure(target(2), Some(matrix), 128).unwrap();

    expected.max_frames = 128;
    assert_eq!(cold_output_snapshot(&state), expected);
    assert_eq!(prepared_storage(&state), (256, Some((256, 256, 0))));
}

#[derive(Debug, PartialEq)]
struct ColdHistorySnapshot {
    capacity: usize,
    identity: (u32, u64, u64),
    pulled_end: u64,
    retained_start: u64,
    retirement_revision: u64,
    spans: Vec<SourcePlaybackSpan>,
}
fn cold_history_snapshot(mixer: &Mixer) -> Option<ColdHistorySnapshot> {
    mixer
        .source_playback_history()
        .map(|history| ColdHistorySnapshot {
            capacity: history.capacity(),
            identity: (
                history.source_rate(),
                history.initial_physical_frame(),
                history.initial_playback_frame(),
            ),
            pulled_end: history.pulled_end_frame(),
            retained_start: history.retained_start_frame(),
            retirement_revision: history.retirement_revision(),
            spans: (0..history.len())
                .map(|index| history.span(index).unwrap())
                .collect(),
        })
}
#[derive(Debug, PartialEq)]
struct ColdOutputSnapshot {
    format: Option<DeviceFormat>,
    layout: Option<NativeOutputLayout>,
    max_frames: usize,
    report: Option<RenderReport>,
    admitted: usize,
    uncertain: bool,
    basis: OutputFrameBasis,
    cursors: (u64, u64),
    counters: AudioCounters,
    configuration: MixerConfig,
    pcm: Vec<f32>,
    history: Option<ColdHistorySnapshot>,
}
fn cold_output_snapshot(state: &NativeOutputState) -> ColdOutputSnapshot {
    ColdOutputSnapshot {
        format: state.format(),
        layout: state.layout().copied(),
        max_frames: state.max_frames(),
        report: state.pending_report(),
        admitted: state.admitted_frames(),
        uncertain: state.admission_is_uncertain(),
        basis: state.output_frame_basis(),
        cursors: (
            state.mixer().frame_cursor(),
            state.mixer().playback_frame_cursor(),
        ),
        counters: state.mixer().counters(),
        configuration: state.mixer().configuration(),
        pcm: state.pending_samples().to_vec(),
        history: cold_history_snapshot(state.mixer()),
    }
}

#[test]
fn cold_history_allocator_probe_observes_known_allocation_and_refusal() {
    let (probe, heap) = count_heap_calls(|| {
        let mut values = Vec::<u64>::new();
        values.try_reserve_exact(8).unwrap();
        std::hint::black_box(values)
    });
    assert!(
        heap > 0,
        "allocation-free assertions require an active probe"
    );
    assert!(probe.capacity() >= 8);
    drop(probe);
    let refused = fail_allocation(0, || Vec::<u64>::new().try_reserve_exact(8));
    assert!(
        refused.is_err(),
        "the allocation failure probe must be armed"
    );
}

#[test]
fn growing_cold_history_preserves_retired_spans_pending_remix_prefix_and_queued_command() {
    let (mut producer, mut mixer) = rig();
    mixer.enable_source_playback_history(8).unwrap();
    let mut state = prepared(
        mixer,
        target(2),
        Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
        8,
    );
    for paused in [false, true] {
        producer.request_pause(paused);
        state.render_pending(2).unwrap();
        state.admit(2).unwrap();
    }
    producer.request_pause(false);
    let report = state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let mut completed = 0;
    assert_eq!(
        state.apply_source_history_retirement(
            crate::audio::source_history_retirement_mailbox::SourceHistoryRetirementRequest {
                revision: 2,
                source_pin: 4,
            },
            &mut completed,
        ),
        Ok(2)
    );
    assert_eq!(completed, 2);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(750_000_000),
            gain: 0.5,
        })
        .unwrap();
    let producer_counters = producer.counters();
    let mut expected = cold_output_snapshot(&state);
    assert_eq!(expected.history.as_ref().unwrap().retirement_revision, 1);
    assert_eq!(expected.history.as_ref().unwrap().retained_start, 4);
    assert_eq!(expected.history.as_ref().unwrap().spans.len(), 1);
    assert_eq!(expected.pcm, [-0.5, 0.25, 0.625, -0.3125, -0.75, 0.375]);

    let (result, heap) = count_heap_calls(|| state.prepare_growing_native_source_history(16, 4));

    assert_eq!(result, Ok(()));
    assert!(heap > 0);
    let capacity = state.mixer().source_playback_history().unwrap().capacity();
    assert!(capacity >= 20);
    expected.history.as_mut().unwrap().capacity = capacity;
    assert_eq!(cold_output_snapshot(&state), expected);
    assert_eq!(producer.counters(), producer_counters);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 5);
    for (frames, headroom) in [(1, capacity - 1), (1, 0), (2, 1)] {
        let (result, heap) =
            count_heap_calls(|| state.prepare_growing_native_source_history(frames, headroom));
        assert_eq!(result, Ok(()));
        assert_eq!(heap, 0);
        assert_eq!(cold_output_snapshot(&state), expected);
        assert_eq!(producer.counters(), producer_counters);
    }
    state.admit(3).unwrap();
    let next = state.render_pending(2).unwrap();
    assert_eq!((next.start_frame, next.playback_start_frame), (8, 6));
    assert_eq!(state.pending_samples(), [0.9375, -0.46875, -1., 0.5]);
    assert_eq!(state.mixer().counters().commands_consumed, 2);
}

#[test]
fn growing_cold_history_geometry_overflow_allocation_and_quarantine_refuse_atomically() {
    for existing_history in [false, true] {
        let (mut producer, mut mixer) = rig();
        if existing_history {
            mixer.enable_source_playback_history(8).unwrap();
        }
        let mut state = prepared(
            mixer,
            target(2),
            Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
            8,
        );
        state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::from_nanos(500_000_000),
                gain: 0.5,
            })
            .unwrap();
        let before = cold_output_snapshot(&state);
        let producer_counters = producer.counters();
        for (frames, headroom, error) in [
            (0, 4, AudioError::InvalidCapacity),
            (0, usize::MAX, AudioError::InvalidCapacity),
            (33, 4, AudioError::RenderCapacity),
            (33, usize::MAX, AudioError::RenderCapacity),
            (4, usize::MAX, AudioError::Overflow),
        ] {
            let (result, heap) =
                count_heap_calls(|| state.prepare_growing_native_source_history(frames, headroom));
            assert_eq!(result, Err(error));
            assert_eq!(heap, 0);
            assert_eq!(cold_output_snapshot(&state), before);
            assert_eq!(producer.counters(), producer_counters);
        }
        assert_eq!(
            fail_allocation(0, || state.prepare_growing_native_source_history(16, 4)),
            Err(AudioError::AllocationFailed)
        );
        assert_eq!(cold_output_snapshot(&state), before);
        assert_eq!(producer.counters(), producer_counters);
        state.mark_admission_uncertain();
        let quarantined = cold_output_snapshot(&state);
        for (frames, headroom) in [(16, 4), (0, usize::MAX)] {
            let (result, heap) =
                count_heap_calls(|| state.prepare_growing_native_source_history(frames, headroom));
            assert_eq!(result, Err(AudioError::InvalidBuffer));
            assert_eq!(heap, 0);
            assert_eq!(cold_output_snapshot(&state), quarantined);
        }
    }
}

#[test]
fn growing_cold_history_allocation_refusal_preserves_retired_history_and_retry_capacity() {
    let (mut producer, mut mixer) = rig();
    mixer.enable_source_playback_history(8).unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(false);
    mixer.render(&mut [0.; 2]).unwrap();
    assert_eq!(mixer.retire_source_playback_before(4), Ok(2));
    let mut state = prepared(
        mixer,
        target(2),
        Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
        8,
    );
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let mut expected = cold_output_snapshot(&state);
    assert_eq!(expected.history.as_ref().unwrap().capacity, 8);
    assert_eq!(expected.history.as_ref().unwrap().spans.len(), 1);
    assert_eq!(
        fail_allocation(0, || state.prepare_growing_native_source_history(16, 4)),
        Err(AudioError::AllocationFailed)
    );
    assert_eq!(cold_output_snapshot(&state), expected);
    let (result, heap) = count_heap_calls(|| state.prepare_growing_native_source_history(16, 4));
    assert_eq!(result, Ok(()));
    assert!(
        heap > 0,
        "retry must still allocate after a refused reservation"
    );
    let capacity = state.mixer().source_playback_history().unwrap().capacity();
    assert!(capacity >= 20);
    expected.history.as_mut().unwrap().capacity = capacity;
    assert_eq!(cold_output_snapshot(&state), expected);
}

#[test]
fn mixer_cold_history_allocation_refusal_preserves_retired_state_and_pending_command() {
    let (mut producer, mut mixer) = rig();
    mixer.enable_source_playback_history(8).unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(false);
    mixer.render(&mut [0.; 2]).unwrap();
    assert_eq!(mixer.retire_source_playback_before(4), Ok(2));
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(500_000_000),
            gain: 0.5,
        })
        .unwrap();
    let before = (
        mixer.configuration(),
        mixer.output_frame_basis(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
        cold_history_snapshot(&mixer),
    );
    assert_eq!(
        fail_allocation(0, || mixer.reserve_source_playback_history(20)),
        Err(AudioError::AllocationFailed)
    );
    assert_eq!(
        (
            mixer.configuration(),
            mixer.output_frame_basis(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            producer.counters(),
            cold_history_snapshot(&mixer),
        ),
        before
    );
    let (result, heap) = count_heap_calls(|| mixer.reserve_source_playback_history(20));
    assert_eq!(result, Ok(()));
    assert!(heap > 0);
    let mut pcm = [99.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.6875, -0.875]);
    assert_eq!(mixer.counters().commands_consumed, 2);
}

#[test]
fn growing_cold_history_supports_64_to_128_within_immutable_mixer_limit() {
    let (_, mixer) = rig_with_render_limit(256);
    let configuration = mixer.configuration();
    let mut state = prepared(mixer, target(1), None, 64);
    state.prepare_growing_native_source_history(64, 4).unwrap();
    assert_eq!(
        state.mixer().source_playback_history().unwrap().capacity(),
        68
    );
    let original = state.render_pending(64).unwrap();
    state.admit(1).unwrap();
    let mut expected = cold_output_snapshot(&state);

    state.prepare_growing_native_source_history(128, 4).unwrap();

    let capacity = state.mixer().source_playback_history().unwrap().capacity();
    assert!(capacity >= 132);
    expected.history.as_mut().unwrap().capacity = capacity;
    assert_eq!(cold_output_snapshot(&state), expected);
    state.reconfigure(target(1), None, 128).unwrap();
    assert_eq!(state.max_frames(), 128);
    assert_eq!(state.pending_report(), Some(original));
    assert_eq!(state.pending_samples(), expected.pcm);
    state.admit(63).unwrap();
    let next = state.render_pending(128).unwrap();
    assert_eq!((next.start_frame, next.frames), (64, 128));
    assert_eq!(state.mixer().configuration(), configuration);
    assert_eq!(
        state.mixer().configuration().limits().max_render_frames(),
        256
    );
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        192
    );
    let before = cold_output_snapshot(&state);
    let (result, heap) = count_heap_calls(|| state.prepare_growing_native_source_history(257, 4));
    assert_eq!(result, Err(AudioError::RenderCapacity));
    assert_eq!(heap, 0);
    assert_eq!(cold_output_snapshot(&state), before);
}

#[test]
fn short_prefix_recovery_uses_first_unsent_basis_and_smaller_period_without_rerender() {
    let (mut producer, mixer) = rig();
    let mut state = prepared(mixer, target(1), None, 8);
    let report = state.render_pending(6).unwrap();
    assert_eq!(
        (report.start_frame, report.frames, report.playback_frames),
        (0, 6, 6)
    );
    assert_eq!(
        state.pending_samples(),
        [0.125, -0.25, 0.375, -0.5, 0.625, -0.75]
    );
    state.admit(2).unwrap();
    assert_eq!(state.mixer().frame_cursor(), 6);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 2);
    assert_eq!(state.pending_samples(), [0.375, -0.5, 0.625, -0.75]);
    let counters = state.mixer().counters();
    let mut state = match state.into_mixer() {
        Err(retained) => retained,
        Ok(_) => panic!("a pending suffix cannot be flattened"),
    };
    state.reconfigure(target(1), None, 2).unwrap();
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.admitted_frames(), 2);
    assert!(state.render_pending(2).is_err());
    assert_eq!(state.mixer().counters(), counters);
    state.admit(2).unwrap();
    assert_eq!(state.pending_samples(), [0.625, -0.75]);
    state.admit(2).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(750_000_000),
            gain: 0.5,
        })
        .unwrap();
    let fresh = state.render_pending(2).unwrap();
    assert_eq!(fresh.start_frame, 6);
    // At frame seven the old -1 and newly started voice's -1/8 saturate to -1.
    assert_eq!(state.pending_samples(), [0.9375, -1.]);
    assert_eq!(state.mixer().counters().commands_consumed, 2);
}

#[test]
fn incompatible_interpretations_and_invalid_admission_preserve_exact_suffix_then_retry() {
    let (_, mixer) = rig();
    let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
    let mut state = prepared(mixer, target(2), Some(matrix.clone()), 8);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let expected = [-0.25, 0.125, 0.375, -0.1875, -0.5, 0.25];
    let report = state.pending_report();
    let basis = state.output_frame_basis();
    let counters = state.mixer().counters();
    for (format, mix, capacity) in [
        (
            DeviceFormat::new(16, 2, SampleEncoding::Float32, None).unwrap(),
            Some(matrix.clone()),
            8,
        ),
        (target(1), None, 8),
        (
            DeviceFormat::new(
                8,
                2,
                SampleEncoding::Pcm {
                    container_bits: 16,
                    valid_bits: 16,
                },
                None,
            )
            .unwrap(),
            Some(matrix.clone()),
            8,
        ),
        (
            DeviceFormat::new(8, 2, SampleEncoding::Float32, Some(3)).unwrap(),
            Some(matrix.clone()),
            8,
        ),
        (
            target(2),
            Some(ChannelMatrix::new(1, 2, &[0.5, 1.]).unwrap()),
            8,
        ),
        (target(2), Some(matrix.clone()), 0),
    ] {
        assert!(state.reconfigure(format, mix, capacity).is_err());
        assert_eq!(state.pending_samples(), expected);
        assert_eq!(state.pending_report(), report);
        assert_eq!(state.output_frame_basis(), basis);
        assert_eq!(state.mixer().counters(), counters);
        assert_eq!(state.admitted_frames(), 1);
        assert_eq!(state.max_frames(), 8);
    }
    for n in [0, 4, usize::MAX] {
        assert!(state.admit(n).is_err());
        assert_eq!(state.pending_samples(), expected);
        assert_eq!(state.output_frame_basis(), basis);
    }
    state.reconfigure(target(2), Some(matrix), 16).unwrap();
    state.admit(3).unwrap();
    state.render_pending(2).unwrap();
    assert_eq!(state.pending_samples(), [0.625, -0.3125, -0.75, 0.375]);
    assert_eq!(state.mixer().frame_cursor(), 6);
    assert!(
        matches!(state.into_mixer(), Err(_)),
        "prepared converter remains owned even after its output is admitted"
    );
}

#[test]
fn paused_zero_tail_requires_actual_paused_report_and_preserves_frozen_playback() {
    let (mut producer, mixer) = rig();
    let mut state = prepared(mixer, target(1), None, 8);
    state.render_pending(2).unwrap();
    producer.request_pause(true);
    assert!(
        !state.paused_tail_admissible(),
        "queued pause is not rendered pause evidence"
    );
    state.admit(2).unwrap();
    let report = state.render_pending(3).unwrap();
    assert!(report.paused);
    assert_eq!((report.start_frame, report.playback_frames), (2, 0));
    assert_eq!(state.pending_samples(), [0.; 3]);
    assert!(state.paused_tail_admissible());
    state.admit(1).unwrap();
    assert_eq!(state.output_frame_basis().start_physical_frame(), 3);
    assert_eq!(state.mixer().frame_cursor(), 5);
    assert_eq!(state.mixer().playback_frame_cursor(), 2);
    state.reconfigure(target(1), None, 1).unwrap();
    assert_eq!(state.pending_report(), Some(report));
    assert!(state.paused_tail_admissible());
    state.admit(2).unwrap();
    producer.request_pause(false);
    state.render_pending(1).unwrap();
    assert_eq!(state.pending_samples(), [0.375]);
    assert!(!state.paused_tail_admissible());
}

#[test]
fn constructor_refusal_returns_original_queue_and_voices_without_source_advance() {
    for (format, matrix, capacity) in [(target(2), None, 8), (target(1), None, 0)] {
        let (mut producer, mixer) = rig();
        let failure = match NativeOutputState::new(mixer, format, matrix, capacity) {
            Err(failure) => failure,
            Ok(_) => panic!("invalid preparation must refuse"),
        };
        assert_eq!(failure.mixer().unwrap().frame_cursor(), 0);
        let (_, mixer) = failure.into_parts();
        let mut mixer = mixer.unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 0.5,
            })
            .unwrap();
        let mut pcm = [0.; 2];
        mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.1875, -0.375]);
        assert_eq!(mixer.counters().commands_consumed, 2);
    }
}

#[test]
fn direct_owner_can_only_flatten_when_no_generated_tail_is_lost() {
    let (_, mixer) = rig();
    let bare = NativeOutputState::from_mixer(mixer);
    assert_eq!(bare.format(), None);
    assert_eq!(bare.pending_frames(), 0);
    let mixer = match bare.into_mixer() {
        Ok(m) => m,
        Err(_) => panic!("bare state must transfer"),
    };
    let mut state = prepared(mixer, target(1), None, 4);
    assert!(state.render_pending(0).is_err());
    assert!(state.render_pending(5).is_err());
    assert_eq!(state.mixer().frame_cursor(), 0);
    state.render_pending(4).unwrap();
    state.admit(4).unwrap();
    assert_eq!(state.pending_frames(), 0);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 4);
    let mut mixer = match state.into_mixer() {
        Ok(m) => m,
        Err(_) => panic!("fully admitted direct state must transfer"),
    };
    let mut pcm = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.625, -0.75]);
}

#[test]
fn explicit_identity_remix_is_not_silently_substituted_for_pending_direct_output() {
    let (_, mixer) = rig();
    let mut state = prepared(mixer, target(1), None, 4);
    state.render_pending(2).unwrap();
    state.admit(1).unwrap();
    assert_eq!(
        state.reconfigure(
            target(1),
            Some(ChannelMatrix::default_mix(1, 1).unwrap()),
            4
        ),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(state.pending_samples(), [-0.25]);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
    state.admit(1).unwrap();
    state
        .reconfigure(
            target(1),
            Some(ChannelMatrix::default_mix(1, 1).unwrap()),
            4,
        )
        .unwrap();
    state.render_pending(2).unwrap();
    assert_eq!(state.pending_samples(), [0.375, -0.5]);
}

#[test]
fn uncertain_admission_survives_complete_recovery_and_explicit_preparation_retry() {
    for remixed in [false, true] {
        let (mut producer, mut mixer) = rig();
        mixer.enable_source_playback_history(8).unwrap();
        let format = target(if remixed { 2 } else { 1 });
        let matrix = remixed.then(|| ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap());
        let mut state = prepared(mixer, format, matrix.clone(), 8);
        let report = state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::from_nanos(500_000_000),
                gain: 0.5,
            })
            .unwrap();
        let pcm = state.pending_samples().to_vec();
        assert!(pcm.iter().any(|sample| *sample != 0.));
        let basis = state.output_frame_basis();
        let config = state.mixer().configuration();
        let counters = state.mixer().counters();
        let history = state.mixer().source_playback_history().unwrap();
        let span = history.span(0);
        let identity = (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
        );
        let mut completed = 0;
        let (results, heap) = count_heap_calls(|| {
            state.mark_admission_uncertain();
            state.mark_admission_uncertain();
            (
                state.render_pending(1),
                state.admit(1),
                state.validate_reconfigure(format, matrix.as_ref(), 16),
                state.apply_source_history_retirement(
                    crate::audio::source_history_retirement_mailbox::SourceHistoryRetirementRequest {
                        revision: 2,
                        source_pin: 4,
                    },
                    &mut completed,
                ),
            )
        });
        assert_eq!(heap, 0);
        assert_eq!(results.0, Err(AudioError::InvalidBuffer));
        assert_eq!(results.1, Err(AudioError::InvalidBuffer));
        assert_eq!(results.2, Err(AudioError::InvalidBuffer));
        assert_eq!(results.3, Err(AudioError::InvalidBuffer));
        assert_eq!(
            state.reconfigure(format, matrix, 16),
            Err(AudioError::InvalidBuffer)
        );
        assert_eq!(completed, 0);
        assert!(state.admission_is_uncertain());
        assert!(!state.paused_tail_admissible());
        let mut recovered = match state.into_mixer() {
            Err(retained) => retained,
            Ok(_) => panic!("uncertain admission must retain the complete owner"),
        };
        // Explicit retry after moving the recovered owner cannot clear quarantine.
        let matrix = remixed.then(|| ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap());
        assert_eq!(
            recovered.reconfigure(format, matrix, 1),
            Err(AudioError::InvalidBuffer)
        );
        assert_eq!(recovered.admit(3), Err(AudioError::InvalidBuffer));
        assert!(recovered.admission_is_uncertain());
        assert_eq!(recovered.pending_samples(), pcm);
        assert_eq!(recovered.pending_report(), Some(report));
        assert_eq!(recovered.admitted_frames(), 1);
        assert_eq!(recovered.max_frames(), 8);
        assert_eq!(recovered.output_frame_basis(), basis);
        assert_eq!(recovered.mixer().configuration(), config);
        assert_eq!(recovered.mixer().counters(), counters);
        assert_eq!(recovered.mixer().frame_cursor(), 4);
        assert_eq!(recovered.mixer().playback_frame_cursor(), 4);
        let history = recovered.mixer().source_playback_history().unwrap();
        assert_eq!(history.span(0), span);
        assert_eq!(history.pulled_end_frame(), 4);
        assert_eq!(history.retirement_revision(), 0);
        assert_eq!(
            (
                history.source_rate(),
                history.initial_physical_frame(),
                history.initial_playback_frame(),
            ),
            identity
        );
    }
}

#[test]
fn quarantine_is_sticky_even_without_pending_pcm_or_remix() {
    let (_, mixer) = rig();
    let mut state = prepared(mixer, target(1), None, 4);
    state.render_pending(4).unwrap();
    state.admit(4).unwrap();
    assert_eq!(state.pending_report(), None);
    assert!(state.paused_tail_admissible());
    state.mark_admission_uncertain();
    assert!(!state.paused_tail_admissible());
    assert_eq!(state.render_pending(1), Err(AudioError::InvalidBuffer));
    assert_eq!(
        state.reconfigure(target(1), None, 8),
        Err(AudioError::InvalidBuffer)
    );
    let recovered = match state.into_mixer() {
        Err(retained) => retained,
        Ok(_) => panic!("empty output does not discharge uncertain admission"),
    };
    assert!(recovered.admission_is_uncertain());
    assert_eq!(recovered.mixer().frame_cursor(), 4);
    assert_eq!(recovered.admitted_frames(), 4);
}

#[test]
fn definite_local_refusal_preserves_retryable_positive_admission() {
    let (mut producer, mixer) = rig();
    let mut state = prepared(mixer, target(1), None, 4);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    assert_eq!(state.admit(4), Err(AudioError::InvalidBuffer));
    assert_eq!(
        state.reconfigure(target(2), None, 4),
        Err(AudioError::InvalidFormat)
    );
    assert!(!state.admission_is_uncertain());
    assert_eq!(state.pending_samples(), [-0.25, 0.375, -0.5]);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(500_000_000),
            gain: 0.5,
        })
        .unwrap();
    state.reconfigure(target(1), None, 2).unwrap();
    state.admit(3).unwrap();
    assert_eq!(state.render_pending(2).unwrap().start_frame, 4);
    assert_eq!(state.pending_samples(), [0.6875, -0.875]);
    assert_eq!(state.mixer().counters().commands_consumed, 2);
    assert!(!state.admission_is_uncertain());
}

#[test]
fn complete_cold_history_prepares_actual_geometry_without_advancing_the_owner() {
    let (mut producer, mut mixer) = rig();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 1]).unwrap();
    let mut state = prepared(
        mixer,
        target(2),
        Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
        8,
    );
    let counters = state.mixer().counters();
    let producer_counters = producer.counters();
    let config = state.mixer().configuration();
    let basis = state.output_frame_basis();

    state.prepare_native_source_history(3, 4).unwrap();

    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.capacity(), 7);
    assert!(history.is_empty());
    assert_eq!(
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame(),
        ),
        (8, 3, 2, 3)
    );
    assert_eq!(state.mixer().configuration(), config);
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(producer.counters(), producer_counters);
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.max_frames(), 8);
    assert_eq!(state.pending_report(), None);

    producer.request_pause(false);
    let report = state.render_pending(2).unwrap();
    assert_eq!((report.start_frame, report.playback_start_frame), (3, 2));
    assert_eq!(state.pending_samples(), [0.375, -0.1875, -0.5, 0.25]);
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        5
    );
}

#[test]
fn complete_cold_history_preserves_retained_pcm_remix_prefix_and_original_history() {
    let (mut producer, mut mixer) = rig();
    mixer.enable_source_playback_history(8).unwrap();
    let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
    let mut state = prepared(mixer, target(2), Some(matrix), 8);
    for paused in [false, true] {
        producer.request_pause(paused);
        state.render_pending(2).unwrap();
        state.admit(2).unwrap();
    }
    producer.request_pause(false);
    let report = state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let mut completed = 0;
    assert_eq!(
        state.apply_source_history_retirement(
            crate::audio::source_history_retirement_mailbox::SourceHistoryRetirementRequest {
                revision: 2,
                source_pin: 4,
            },
            &mut completed,
        ),
        Ok(2)
    );
    assert_eq!(completed, 2);
    let pcm = state.pending_samples().to_vec();
    let basis = state.output_frame_basis();
    let counters = state.mixer().counters();
    let config = state.mixer().configuration();
    let history = state.mixer().source_playback_history().unwrap();
    let spans = (history.span(0), history.span(1), history.span(2));
    let (result, heap) = count_heap_calls(|| state.prepare_native_source_history(2, usize::MAX));
    assert_eq!(result, Ok(()));
    assert_eq!(heap, 0);
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(basis.start_physical_frame(), 5);
    assert_eq!(state.mixer().frame_cursor(), 8);
    assert_eq!(state.mixer().playback_frame_cursor(), 6);
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(state.mixer().configuration(), config);
    assert_eq!(state.max_frames(), 8);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.capacity(), 8);
    assert_eq!(history.retirement_revision(), 1);
    assert_eq!((history.span(0), history.span(1), history.span(2)), spans);
    assert_eq!(
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame()
        ),
        (8, 0, 0)
    );
    producer.request_pause(true);
    state.admit(3).unwrap();
    assert!(state.render_pending(1).unwrap().paused);
    assert_eq!(state.pending_samples(), [0., 0.]);
}

#[test]
fn complete_cold_history_geometry_allocation_and_quarantine_refuse_atomically() {
    let (mut producer, mixer) = rig();
    let mut state = prepared(
        mixer,
        target(2),
        Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
        8,
    );
    let report = state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let pcm = state.pending_samples().to_vec();
    let basis = state.output_frame_basis();
    let counters = state.mixer().counters();
    for (frames, headroom, expected) in [
        (0, 4, AudioError::InvalidCapacity),
        (33, 4, AudioError::RenderCapacity),
        (4, usize::MAX, AudioError::Overflow),
    ] {
        let (result, heap) =
            count_heap_calls(|| state.prepare_native_source_history(frames, headroom));
        assert_eq!(result, Err(expected));
        assert_eq!(heap, 0);
        assert!(state.mixer().source_playback_history().is_none());
        assert_eq!(state.pending_samples(), pcm);
        assert_eq!(state.pending_report(), Some(report));
        assert_eq!(state.output_frame_basis(), basis);
        assert_eq!(state.admitted_frames(), 1);
        assert_eq!(state.mixer().counters(), counters);
    }
    assert_eq!(
        fail_allocation(0, || state.prepare_native_source_history(4, 4)),
        Err(AudioError::AllocationFailed)
    );
    assert!(state.mixer().source_playback_history().is_none());
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(state.max_frames(), 8);
    producer.request_pause(true);
    state.mark_admission_uncertain();
    let (result, heap) = count_heap_calls(|| state.prepare_native_source_history(4, 4));
    assert_eq!(result, Err(AudioError::InvalidBuffer));
    assert_eq!(heap, 0);
    assert!(state.mixer().source_playback_history().is_none());
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.mixer().counters(), counters);
    assert!(state.admission_is_uncertain());
}

fn asio_layout(rows: &[(u32, AsioPcmEncoding)]) -> AsioPlanarLayout {
    AsioPlanarLayout::new(AudioFormat::new(8, rows.len() as u16).unwrap(), rows).unwrap()
}

fn prepared_asio(
    mixer: Mixer,
    layout: AsioPlanarLayout,
    matrix: Option<ChannelMatrix>,
    frames: usize,
) -> NativeOutputState {
    let mut state = NativeOutputState::from_mixer(mixer);
    state.reconfigure_asio(layout, matrix, frames).unwrap();
    state
}

#[test]
fn asio_checked_layout_rejects_malformed_native_rows_without_allocating() {
    let stereo = AudioFormat::new(8, 2).unwrap();
    let invalid_rows: &[&[(u32, AsioPcmEncoding)]] = &[
        &[],
        &[(0, AsioPcmEncoding::Int16Lsb)],
        &[
            (0, AsioPcmEncoding::Int16Lsb),
            (1, AsioPcmEncoding::Float64Msb),
            (2, AsioPcmEncoding::Int24Lsb),
        ],
        &[
            (7, AsioPcmEncoding::Int16Lsb),
            (7, AsioPcmEncoding::Float64Msb),
        ],
        &[
            (0, AsioPcmEncoding::Int16Lsb),
            (i32::MAX as u32 + 1, AsioPcmEncoding::Float64Msb),
        ],
    ];
    for rows in invalid_rows {
        let (result, heap) = count_heap_calls(|| AsioPlanarLayout::new(stereo, rows));
        assert_eq!(result, Err(AudioError::InvalidFormat));
        assert_eq!(heap, 0);
    }
    let rows: Vec<_> = (0..33)
        .map(|channel| (channel, AsioPcmEncoding::Int24Lsb))
        .collect();
    let (result, heap) =
        count_heap_calls(|| AsioPlanarLayout::new(AudioFormat::new(8, 32).unwrap(), &rows));
    assert_eq!(result, Err(AudioError::InvalidFormat));
    assert_eq!(heap, 0);
    let (layout, heap) =
        count_heap_calls(|| AsioPlanarLayout::new(AudioFormat::new(8, 32).unwrap(), &rows[..32]));
    assert_eq!(heap, 0);
    assert_eq!(layout.unwrap().channels(), &rows[..32]);
    assert!(AsioPlanarLayout::new(
        AudioFormat::new(8, 1).unwrap(),
        &[(i32::MAX as u32, AsioPcmEncoding::Float64Msb)]
    )
    .is_ok());
}

#[test]
fn asio_all_eighteen_native_encodings_keep_honest_layout_and_actual_remixed_pcm() {
    use AsioPcmEncoding::*;
    let encodings = [
        Int16Msb, Int24Msb, Int32Msb, Float32Msb, Float64Msb, Int32Msb16, Int32Msb18, Int32Msb20,
        Int32Msb24, Int16Lsb, Int24Lsb, Int32Lsb, Float32Lsb, Float64Lsb, Int32Lsb16, Int32Lsb18,
        Int32Lsb20, Int32Lsb24,
    ];
    let rows: Vec<_> = encodings
        .into_iter()
        .enumerate()
        .map(|(index, encoding)| (31 - index as u32, encoding))
        .collect();
    let layout = asio_layout(&rows);
    let coefficients: Vec<_> = (0..18)
        .map(|channel| if channel % 2 == 0 { 1. } else { -0.5 })
        .collect();
    let (_, mixer) = rig();
    let mut state = prepared_asio(
        mixer,
        layout,
        Some(ChannelMatrix::new(1, 18, &coefficients).unwrap()),
        4,
    );
    assert_eq!(
        state.format(),
        None,
        "heterogeneous planes are not a DeviceFormat"
    );
    assert_eq!(state.target_pcm(), Some(AudioFormat::new(8, 18).unwrap()));
    assert_eq!(
        state.layout(),
        Some(&NativeOutputLayout::AsioPlanar(layout))
    );
    assert_eq!(layout.channels(), rows);
    let (report, heap) = count_heap_calls(|| state.render_pending(3));
    let report = report.unwrap();
    assert_eq!(heap, 0);
    assert_eq!((report.start_frame, report.frames), (0, 3));
    for (frame, original) in [0.125, -0.25, 0.375].into_iter().enumerate() {
        for (channel, coefficient) in coefficients.iter().enumerate() {
            assert_eq!(
                state.pending_samples()[frame * 18 + channel],
                original * coefficient
            );
        }
    }
    let (admitted, heap) = count_heap_calls(|| state.admit(1));
    assert_eq!(admitted, Ok(()));
    assert_eq!(heap, 0);
    assert_eq!(state.pending_samples().len(), 36);
    assert_eq!(state.pending_samples()[0], -0.25);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
}

#[test]
fn asio_prefix_reopen_preserves_original_report_history_queue_and_first_unsent_pcm() {
    let layout = asio_layout(&[
        (5, AsioPcmEncoding::Int24Lsb),
        (2, AsioPcmEncoding::Float64Msb),
    ]);
    let (mut producer, mut mixer) = rig();
    mixer.enable_source_playback_history(8).unwrap();
    let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
    let mut state = prepared_asio(mixer, layout, Some(matrix.clone()), 8);
    let report = state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    assert_eq!(
        state.pending_samples(),
        [-0.25, 0.125, 0.375, -0.1875, -0.5, 0.25]
    );
    assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(500_000_000),
            gain: 0.5,
        })
        .unwrap();
    let producer_counters = producer.counters();
    let mut expected = cold_output_snapshot(&state);
    let (validation, heap) =
        count_heap_calls(|| state.validate_reconfigure_asio(&layout, Some(&matrix), 2));
    assert_eq!(validation, Ok(()));
    assert_eq!(heap, 0);
    assert_eq!(cold_output_snapshot(&state), expected);
    state.reconfigure_asio(layout, Some(matrix), 2).unwrap();
    expected.max_frames = 2;
    assert_eq!(cold_output_snapshot(&state), expected);
    assert_eq!(producer.counters(), producer_counters);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(prepared_storage(&state), (16, Some((2, 2, 0))));
    let (result, heap) = count_heap_calls(|| {
        assert_eq!(state.render_pending(1), Err(AudioError::InvalidBuffer));
        state.admit(3)?;
        state.render_pending(2)
    });
    assert_eq!(heap, 0);
    let next = result.unwrap();
    assert_eq!(
        (next.start_frame, next.playback_start_frame, next.frames),
        (4, 4, 2)
    );
    // The newly scheduled voice starts its asset at sample zero at frame four:
    // [.625, -.75] + .5 * [.125, -.25], followed by the unchanged remix.
    assert_eq!(state.pending_samples(), [0.6875, -0.34375, -0.875, 0.4375]);
    assert_eq!(state.mixer().counters().commands_consumed, 2);
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        6
    );
}

#[test]
fn asio_pending_exact_plane_order_encoding_index_rate_matrix_and_family_refuse_atomically() {
    let rows = [
        (5, AsioPcmEncoding::Int24Lsb),
        (2, AsioPcmEncoding::Float64Msb),
    ];
    let layout = asio_layout(&rows);
    let (_, mut mixer) = rig();
    mixer.enable_source_playback_history(8).unwrap();
    let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
    let mut state = prepared_asio(mixer, layout, Some(matrix.clone()), 8);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let before = cold_output_snapshot(&state);
    let storage = prepared_storage(&state);
    let changed_layouts = [
        asio_layout(&[rows[1], rows[0]]),
        asio_layout(&[(6, rows[0].1), rows[1]]),
        asio_layout(&[(rows[0].0, AsioPcmEncoding::Int24Msb), rows[1]]),
        asio_layout(&[(5, AsioPcmEncoding::Int24Lsb)]),
        AsioPlanarLayout::new(AudioFormat::new(16, 2).unwrap(), &rows).unwrap(),
    ];
    for changed in changed_layouts {
        let (refused, heap) =
            count_heap_calls(|| state.validate_reconfigure_asio(&changed, Some(&matrix), 16));
        assert_eq!(refused, Err(AudioError::InvalidFormat));
        assert_eq!(heap, 0);
        assert_eq!(
            state.reconfigure_asio(changed, Some(matrix.clone()), 16),
            refused
        );
        assert_eq!(cold_output_snapshot(&state), before);
        assert_eq!(prepared_storage(&state), storage);
    }
    for (matrix, frames, expected_error) in [
        (None, 8, AudioError::InvalidFormat),
        (
            Some(ChannelMatrix::new(1, 2, &[0.5, 1.]).unwrap()),
            8,
            AudioError::InvalidFormat,
        ),
        (Some(matrix.clone()), 0, AudioError::InvalidCapacity),
        (Some(matrix.clone()), 33, AudioError::RenderCapacity),
    ] {
        assert_eq!(
            state.reconfigure_asio(layout, matrix, frames),
            Err(expected_error)
        );
        assert_eq!(cold_output_snapshot(&state), before);
        assert_eq!(prepared_storage(&state), storage);
    }
    assert_eq!(
        state.reconfigure(target(2), Some(matrix), 8),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(cold_output_snapshot(&state), before);
    assert_eq!(prepared_storage(&state), storage);
    state.admit(3).unwrap();
    state
        .reconfigure_asio(
            changed_layouts[0],
            Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
            2,
        )
        .unwrap();
    assert_eq!(state.render_pending(2).unwrap().start_frame, 4);
    assert_eq!(state.pending_samples(), [0.625, -0.3125, -0.75, 0.375]);
}

#[test]
fn asio_cold_storage_tracks_actual_frames_and_preserves_retained_pcm_across_growth_and_rebound() {
    for remixed in [false, true] {
        let (_, mixer) = rig_with_render_limit(256);
        let layout = if remixed {
            asio_layout(&[
                (7, AsioPcmEncoding::Int16Lsb),
                (3, AsioPcmEncoding::Float64Msb),
            ])
        } else {
            asio_layout(&[(7, AsioPcmEncoding::Int24Lsb)])
        };
        let matrix = remixed.then(|| ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap());
        let mut state = prepared_asio(mixer, layout, matrix.clone(), 64);
        let channels = usize::from(layout.pcm().channels());
        assert_eq!(
            prepared_storage(&state),
            (64 * channels, remixed.then_some((64, 64, 0)))
        );
        state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        let mut expected = cold_output_snapshot(&state);
        state.reconfigure_asio(layout, matrix.clone(), 128).unwrap();
        expected.max_frames = 128;
        assert_eq!(cold_output_snapshot(&state), expected);
        assert_eq!(
            prepared_storage(&state),
            (128 * channels, remixed.then_some((128, 128, 0)))
        );
        state.reconfigure_asio(layout, matrix, 2).unwrap();
        expected.max_frames = 2;
        assert_eq!(cold_output_snapshot(&state), expected);
        assert_eq!(
            prepared_storage(&state),
            (128 * channels, remixed.then_some((2, 2, 0)))
        );
        assert_eq!(
            state.mixer().configuration().limits().max_render_frames(),
            256
        );
        state.admit(3).unwrap();
        let (report, heap) = count_heap_calls(|| state.render_pending(2));
        assert_eq!(report.unwrap().start_frame, 4);
        assert_eq!(heap, 0);
        assert_eq!(
            &state.pending_samples()[..channels],
            if remixed {
                &[0.625, -0.3125][..]
            } else {
                &[0.625][..]
            }
        );
    }
}

#[test]
fn asio_actual_cold_allocation_refusals_preserve_original_owner_and_allow_retry() {
    let layout = asio_layout(&[
        (5, AsioPcmEncoding::Int24Lsb),
        (2, AsioPcmEncoding::Float64Msb),
    ]);
    for index in 0..2 {
        let (mut producer, mut mixer) = rig_with_render_limit(256);
        mixer.enable_source_playback_history(8).unwrap();
        let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
        let mut state = prepared_asio(mixer, layout, Some(matrix.clone()), 4);
        state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::from_nanos(500_000_000),
                gain: 0.5,
            })
            .unwrap();
        let mut expected = cold_output_snapshot(&state);
        let storage = prepared_storage(&state);
        let counters = producer.counters();
        let retry_matrix = matrix.clone();
        assert_eq!(
            fail_allocation(index, || state.reconfigure_asio(
                layout,
                Some(retry_matrix),
                16
            )),
            Err(AudioError::AllocationFailed)
        );
        assert_eq!(cold_output_snapshot(&state), expected);
        assert_eq!(prepared_storage(&state), storage);
        assert_eq!(producer.counters(), counters);
        state.reconfigure_asio(layout, Some(matrix), 16).unwrap();
        expected.max_frames = 16;
        assert_eq!(cold_output_snapshot(&state), expected);
        state.admit(3).unwrap();
        state.render_pending(2).unwrap();
        // Queued voice starts at sample zero, not at the original voice's phase.
        assert_eq!(state.pending_samples(), [0.6875, -0.34375, -0.875, 0.4375]);
        assert_eq!(state.mixer().counters().commands_consumed, 2);
    }
}

#[test]
fn asio_pending_interleaved_output_cannot_be_silently_reinterpreted_as_planar() {
    let (_, mixer) = rig();
    let mut state = prepared(mixer, target(1), None, 4);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let before = cold_output_snapshot(&state);
    let layout = asio_layout(&[(0, AsioPcmEncoding::Float32Lsb)]);
    assert_eq!(
        state.reconfigure_asio(layout, None, 4),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(cold_output_snapshot(&state), before);
    state.admit(3).unwrap();
    state.reconfigure_asio(layout, None, 2).unwrap();
    assert_eq!(state.format(), None);
    assert_eq!(state.render_pending(2).unwrap().start_frame, 4);
    assert_eq!(state.pending_samples(), [0.625, -0.75]);
}

#[test]
fn asio_quarantine_and_paused_reports_preserve_exact_pcm_and_history_on_retry() {
    let (mut producer, mut mixer) = rig();
    mixer.enable_source_playback_history(8).unwrap();
    let layout = asio_layout(&[
        (5, AsioPcmEncoding::Int24Lsb),
        (2, AsioPcmEncoding::Float64Msb),
    ]);
    let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
    let mut state = prepared_asio(mixer, layout, Some(matrix.clone()), 4);
    state.render_pending(2).unwrap();
    producer.request_pause(true);
    assert!(!state.paused_tail_admissible());
    state.admit(2).unwrap();
    let paused = state.render_pending(3).unwrap();
    assert!(paused.paused);
    assert_eq!(paused.playback_frames, 0);
    assert_eq!(state.pending_samples(), [0.; 6]);
    assert!(state.paused_tail_admissible());
    state.admit(1).unwrap();
    state.mark_admission_uncertain();
    let before = cold_output_snapshot(&state);
    let (result, heap) = count_heap_calls(|| {
        (
            state.validate_reconfigure_asio(&layout, Some(&matrix), 8),
            state.render_pending(1),
            state.admit(1),
            state.prepare_native_source_history(4, 4),
        )
    });
    assert_eq!(heap, 0);
    assert_eq!(
        result,
        (
            Err(AudioError::InvalidBuffer),
            Err(AudioError::InvalidBuffer),
            Err(AudioError::InvalidBuffer),
            Err(AudioError::InvalidBuffer)
        )
    );
    assert_eq!(
        state.reconfigure_asio(layout, Some(matrix), 8),
        Err(AudioError::InvalidBuffer)
    );
    assert_eq!(cold_output_snapshot(&state), before);
    assert!(!state.paused_tail_admissible());
    assert!(matches!(state.into_mixer(), Err(_)));
}
