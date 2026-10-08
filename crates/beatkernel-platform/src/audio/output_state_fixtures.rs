//! Software admission tests: native presentation is deliberately not inferred.
use super::*;
use crate::audio::{DeviceFormat, SampleEncoding};
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
fn fail_allocation<T>(index: usize, work: impl FnOnce() -> T) -> T {
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
            AudioLimits::new(16, 4, 16, 32, 16).unwrap(),
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
