//! Stream-bound mathematical requests; these sequences never acknowledge native audio.
use super::*;
use crate::audio::{count_heap_calls, ConvertedNativeOutputState, DeviceFormat, SampleEncoding};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::sync::{atomic::Ordering, Arc};

fn rig() -> (
    CommandProducer,
    ConvertedNativeOutputState,
    TargetFrameBasis,
) {
    let format = AudioFormat::new(3, 1).unwrap();
    let limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 1024], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(17),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 128, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0]).unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    mixer.render(&mut [0.0]).unwrap();
    let device = DeviceFormat::new(7, 1, SampleEncoding::Float32, None).unwrap();
    let mut state = ConvertedNativeOutputState::new(
        mixer,
        device,
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|e| panic!("owner: {:?}", e.error()));
    state.enable_phase_history(8).unwrap();
    let basis = state.target_frame_basis();
    (producer, state, basis)
}
fn mailbox(
    state: &ConvertedNativeOutputState,
    basis: TargetFrameBasis,
) -> ConvertedHistoryRetirementMailbox {
    ConvertedHistoryRetirementMailbox::new(
        basis,
        state.mixer().source_playback_history().unwrap(),
        state.phase_history().unwrap(),
    )
    .unwrap()
}
fn identity(state: &ConvertedNativeOutputState) -> (u32, u64, u64) {
    let source = state.mixer().source_playback_history().unwrap();
    (
        source.source_rate(),
        source.initial_physical_frame(),
        source.initial_playback_frame(),
    )
}
fn payload(m: &ConvertedHistoryRetirementMailbox) -> (u64, [u64; 4]) {
    (
        m.sequence.load(Ordering::SeqCst),
        std::array::from_fn(|i| m.words[i].load(Ordering::SeqCst)),
    )
}
fn complete(state: &mut ConvertedNativeOutputState, held: bool) {
    if held {
        state.render_held_pending(7).unwrap();
    } else {
        state.render_pending(7).unwrap();
    }
    state.admit(7).unwrap();
}

#[test]
fn cold_binding_uses_original_source_creation_not_converter_creation() {
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    assert_eq!(identity(&state), (3, 1, 1));
    assert_eq!(
        state
            .phase_history()
            .unwrap()
            .initial_source_position()
            .frame,
        2
    );
    assert_ne!(basis.start_time(), TargetTime::from_frames(1, 3).unwrap());
    assert!(m.latest().is_none());
    assert_eq!(payload(&m).0, 0);
    assert_eq!(m.validate_identity(basis, identity(&state)), Ok(()));
    let revision = m
        .request(basis, identity(&state), 1, basis.start_time())
        .unwrap()
        .unwrap();
    assert_ne!(revision, 0);
    let accepted = m.latest().unwrap();
    assert_eq!(
        (accepted.revision, accepted.source_pin, accepted.target_pin),
        (revision, 1, basis.start_time())
    );
}

#[test]
fn every_identity_component_refuses_without_payload_mutation() {
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    let id = identity(&state);
    m.request(basis, id, 2, basis.start_time())
        .unwrap()
        .unwrap();
    let before = payload(&m);
    for wrong in [(7, 1, 1), (3, 2, 1), (3, 1, 2)] {
        assert_eq!(
            m.validate_identity(basis, wrong),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(
            m.request(basis, wrong, 3, basis.start_time()),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(payload(&m), before);
    }
}

#[test]
fn every_target_basis_component_refuses_without_payload_mutation() {
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    let id = identity(&state);
    let origin = basis.origin();
    let variants = [
        TargetFrameBasis::new(
            ClockPoint {
                domain: ClockDomainId(18),
                timestamp: origin.timestamp,
            },
            basis.start_time(),
            basis.sample_rate(),
        )
        .unwrap(),
        TargetFrameBasis::new(
            origin,
            basis.start_time().checked_add_frames(1, 7).unwrap(),
            basis.sample_rate(),
        )
        .unwrap(),
        TargetFrameBasis::new(origin, basis.start_time(), 8).unwrap(),
    ];
    let before = payload(&m);
    for wrong in variants {
        assert_eq!(
            m.validate_identity(wrong, id),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(
            m.request(wrong, id, 2, basis.start_time()),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(payload(&m), before);
    }
}

#[test]
fn queued_pair_coalesces_monotonically_and_identical_request_reuses_revision() {
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    let id = identity(&state);
    let first = m
        .request(basis, id, 2, basis.start_time())
        .unwrap()
        .unwrap();
    let t = basis.start_time().checked_add_frames(3, 7).unwrap();
    let second = m.request(basis, id, 4, t).unwrap().unwrap();
    assert!(second > first);
    assert_eq!(
        m.latest(),
        Some(ConvertedHistoryRetirementRequest {
            revision: second,
            source_pin: 4,
            target_pin: t
        })
    );
    let before = payload(&m);
    assert_eq!(m.request(basis, id, 4, t), Ok(Some(second)));
    assert_eq!(payload(&m), before);
}

#[test]
fn either_regressing_pin_or_cold_lower_bound_is_readonly() {
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    let id = identity(&state);
    let later = basis.start_time().checked_add_frames(3, 7).unwrap();
    assert_eq!(
        m.request(basis, id, 0, basis.start_time()),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        m.request(basis, id, 1, TargetTime::from_frames(1, 3).unwrap()),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(payload(&m).0, 0);
    m.request(basis, id, 4, later).unwrap().unwrap();
    let before = payload(&m);
    assert_eq!(
        m.request(basis, id, 3, later.checked_add_frames(1, 7).unwrap()),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        m.request(basis, id, 5, basis.start_time()),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(payload(&m), before);
}

#[test]
fn exact_fraction_alias_reuses_sequence_but_same_nanosecond_floor_regression_refuses() {
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    let id = identity(&state);
    let t = TargetTime::new(1, 1, 3).unwrap();
    let r = m.request(basis, id, 2, t).unwrap().unwrap();
    assert_eq!(
        m.request(basis, id, 2, TargetTime::new(1, 2, 6).unwrap()),
        Ok(Some(r))
    );
    let higher = TargetTime::new(2, 2, 4_000_000_000).unwrap();
    let lower = TargetTime::new(2, 1, 4_000_000_000).unwrap();
    assert_eq!(
        higher.point(basis.origin()).unwrap(),
        lower.point(basis.origin()).unwrap()
    );
    m.request(basis, id, 3, higher).unwrap().unwrap();
    let before = payload(&m);
    assert_eq!(
        m.request(basis, id, 4, lower),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(payload(&m), before);
}

#[test]
fn recovered_retired_histories_bind_actual_lower_bounds_and_original_identity() {
    let (mut producer, mut state, basis) = rig();
    complete(&mut state, false);
    producer.request_pause(true);
    complete(&mut state, false);
    complete(&mut state, true);
    producer.request_pause(false);
    complete(&mut state, false);
    let source_pin = state.converter_owner().source_position().frame;
    let target_pin = state.phase_history().unwrap().end_time();
    let removed = state
        .retire_histories_before(source_pin, target_pin)
        .unwrap();
    assert!(removed.0 > 0 && removed.1 > 0);
    let m = mailbox(&state, basis);
    let source_lower = state
        .mixer()
        .source_playback_history()
        .unwrap()
        .retained_start_frame();
    let target_lower = state.phase_history().unwrap().retained_start_time();
    assert!(source_lower > 1);
    assert_ne!(target_lower, basis.start_time());
    assert_eq!(m.validate_identity(basis, (3, 1, 1)), Ok(()));
    assert_eq!(
        m.request(basis, (3, 1, 1), 1, target_lower),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        m.request(basis, (3, 1, 1), source_lower, basis.start_time()),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(payload(&m).0, 0);
    assert!(m
        .request(basis, (3, 1, 1), source_lower, target_lower)
        .unwrap()
        .is_some());
}

#[test]
fn constructor_refuses_mismatched_actual_source_grid() {
    let (_, state, basis) = rig();
    let format = AudioFormat::new(7, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(128, 128, 1).unwrap()).unwrap();
    let (_, consumer) = command_queue(4).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(17),
            Timestamp::ZERO,
            AudioLimits::new(4, 2, 4, 16, 4).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(3).unwrap();
    assert!(matches!(
        ConvertedHistoryRetirementMailbox::new(
            basis,
            mixer.source_playback_history().unwrap(),
            state.phase_history().unwrap()
        ),
        Err(AudioError::InvalidFormat)
    ));
}

#[test]
fn odd_sequence_is_bounded_busy_and_never_exposes_partial_payload() {
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    m.sequence.store(3, Ordering::SeqCst);
    for _ in 0..16 {
        assert!(m.latest().is_none());
        assert_eq!(
            m.request(basis, identity(&state), 2, basis.start_time()),
            Ok(None)
        );
    }
    assert_eq!(payload(&m).0, 3);
}

#[test]
fn exhausted_even_sequence_refuses_without_writing_pins() {
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    m.request(basis, identity(&state), 2, basis.start_time())
        .unwrap()
        .unwrap();
    m.sequence.store(u64::MAX - 1, Ordering::SeqCst);
    let before = payload(&m);
    assert_eq!(
        m.request(basis, identity(&state), 3, basis.start_time()),
        Err(AudioError::Overflow)
    );
    assert_eq!(payload(&m), before);
}

#[test]
fn malformed_even_payload_is_not_an_accepted_read() {
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    m.sequence.store(2, Ordering::SeqCst);
    m.words[3].store(0, Ordering::SeqCst);
    assert!(m.latest().is_none());
}

#[test]
fn request_and_read_have_zero_heap_calls_after_calibrated_cold_setup() {
    let (_, calibration) = count_heap_calls(|| {
        let mut v = Vec::with_capacity(1);
        v.push(1u64);
        v.reserve(64);
        std::hint::black_box(v);
    });
    assert!(calibration > 0);
    let (_, state, basis) = rig();
    let m = mailbox(&state, basis);
    let id = identity(&state);
    let ((), calls) = count_heap_calls(|| {
        assert!(m.latest().is_none());
        for i in 0..128 {
            let target = basis.start_time().checked_add_frames(i, 7).unwrap();
            let r = m.request(basis, id, 1 + i, target).unwrap().unwrap();
            assert_eq!(m.latest().unwrap().revision, r);
            assert_eq!(m.request(basis, id, 1 + i, target), Ok(Some(r)));
        }
        assert_eq!(
            m.request(basis, (7, 1, 1), 129, basis.start_time()),
            Err(AudioError::InvalidFormat)
        );
    });
    assert_eq!(calls, 0);
}

#[test]
fn concurrent_reads_never_mix_two_accepted_request_pairs() {
    let (_, state, basis) = rig();
    let m = Arc::new(mailbox(&state, basis));
    let id = identity(&state);
    let writer = Arc::clone(&m);
    let thread = std::thread::spawn(move || {
        for i in 1..=4096u64 {
            let t = TargetTime::new(i, 1, 3).unwrap();
            let accepted = writer.request(basis, id, i, t).unwrap();
            assert!(accepted.is_some());
        }
    });
    for _ in 0..16384 {
        if let Some(r) = m.latest() {
            assert_eq!(r.source_pin, r.target_pin.seconds());
            assert_eq!(
                (r.target_pin.numerator(), r.target_pin.denominator()),
                (1, 3)
            );
            assert_eq!(r.revision % 2, 0);
            assert_ne!(r.revision, 0);
        }
    }
    thread.join().unwrap();
    let last = m.latest().unwrap();
    assert_eq!((last.source_pin, last.target_pin.seconds()), (4096, 4096));
}

#[test]
fn concurrent_producers_coalesce_identical_pair_without_unbounded_retry() {
    let (_, state, basis) = rig();
    let m = Arc::new(mailbox(&state, basis));
    let id = identity(&state);
    let t = TargetTime::new(5, 1, 3).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let m = Arc::clone(&m);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..1024 {
                    if let Some(r) = m.request(basis, id, 5, t).unwrap() {
                        assert_ne!(r, 0);
                        assert_eq!(r % 2, 0);
                    }
                }
            })
        })
        .collect();
    barrier.wait();
    for thread in threads {
        thread.join().unwrap();
    }
    let last = m.latest().unwrap();
    assert_eq!((last.source_pin, last.target_pin), (5, t));
    assert_eq!(last.revision, 2);
}
