//! Real Source identities with mathematical requests, never native acknowledgments.
use super::*;
use crate::audio::count_heap_calls;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::sync::{Arc, Barrier};

fn rig() -> (CommandProducer, Mixer, OutputFrameBasis) {
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(128, 128, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(17),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 32, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 3]).unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    producer.request_pause(false);
    mixer.render(&mut [0.0; 2]).unwrap();
    let basis = mixer.output_frame_basis();
    (producer, mixer, basis)
}

fn identity(mixer: &Mixer) -> (u32, u64, u64) {
    let history = mixer.source_playback_history().unwrap();
    (
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
}

fn mailbox(mixer: &Mixer, basis: OutputFrameBasis) -> SourceHistoryRetirementMailbox {
    SourceHistoryRetirementMailbox::new(basis, mixer.source_playback_history().unwrap()).unwrap()
}

fn payload(m: &SourceHistoryRetirementMailbox) -> (u64, u64) {
    (
        m.sequence.load(Ordering::SeqCst),
        m.source_pin.load(Ordering::SeqCst),
    )
}

#[test]
fn later_stream_creation_keeps_original_nonzero_physical_and_playback_identity() {
    let (_, mixer, basis) = rig();
    assert_eq!(identity(&mixer), (3, 5, 2));
    assert_eq!(basis.start_physical_frame(), 7);
    let m = mailbox(&mixer, basis);
    assert!(m.latest().is_none());
    assert_eq!(m.validate_identity(basis, (3, 5, 2)), Ok(()));
    // The history pin can precede stream creation; it is not rebased to seven.
    assert_eq!(m.request(basis, (3, 5, 2), 5), Ok(Some(2)));
    assert_eq!(
        m.latest(),
        Some(SourceHistoryRetirementRequest {
            revision: 2,
            source_pin: 5
        })
    );
}

#[test]
fn constructor_refuses_wrong_source_hz_and_creation_before_history() {
    let (_, mixer, basis) = rig();
    for wrong in [
        OutputFrameBasis::new(basis.origin(), 4, 7).unwrap(),
        OutputFrameBasis::new(basis.origin(), 3, 4).unwrap(),
    ] {
        assert!(matches!(
            SourceHistoryRetirementMailbox::new(wrong, mixer.source_playback_history().unwrap(),),
            Err(AudioError::InvalidFormat)
        ));
    }
    let exact_creation = OutputFrameBasis::new(basis.origin(), 3, 5).unwrap();
    assert!(SourceHistoryRetirementMailbox::new(
        exact_creation,
        mixer.source_playback_history().unwrap()
    )
    .is_ok());
}

#[test]
fn every_identity_and_basis_component_refuses_without_mutation() {
    let (_, mixer, basis) = rig();
    let m = mailbox(&mixer, basis);
    let id = identity(&mixer);
    m.request(basis, id, 6).unwrap().unwrap();
    let before = payload(&m);
    for wrong in [(4, 5, 2), (3, 6, 2), (3, 5, 3)] {
        assert_eq!(
            m.validate_identity(basis, wrong),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(m.request(basis, wrong, 7), Err(AudioError::InvalidFormat));
        assert_eq!(payload(&m), before);
    }
    let origin = basis.origin();
    for wrong in [
        OutputFrameBasis::new(
            ClockPoint {
                domain: ClockDomainId(18),
                timestamp: origin.timestamp,
            },
            3,
            7,
        )
        .unwrap(),
        OutputFrameBasis::new(
            ClockPoint {
                domain: origin.domain,
                timestamp: Timestamp::from_nanos(1),
            },
            3,
            7,
        )
        .unwrap(),
        OutputFrameBasis::new(origin, 4, 7).unwrap(),
        OutputFrameBasis::new(origin, 3, 8).unwrap(),
    ] {
        assert_eq!(
            m.validate_identity(wrong, id),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(m.request(wrong, id, 7), Err(AudioError::InvalidFormat));
        assert_eq!(payload(&m), before);
    }
}

#[test]
fn monotonic_coalescing_duplicate_and_regression_are_exact_in_source_frames() {
    let (_, mixer, basis) = rig();
    let m = mailbox(&mixer, basis);
    let id = identity(&mixer);
    assert_eq!(m.request(basis, id, 4), Err(AudioError::InvalidFormat));
    assert_eq!(payload(&m), (0, 0));
    assert_eq!(m.request(basis, id, 5), Ok(Some(2)));
    assert_eq!(m.request(basis, id, 6), Ok(Some(4)));
    assert_eq!(m.request(basis, id, 6), Ok(Some(4)));
    let before = payload(&m);
    assert_eq!(m.request(basis, id, 5), Err(AudioError::InvalidFormat));
    assert_eq!(payload(&m), before);
}

#[test]
fn recovered_retired_history_uses_retained_lower_but_original_identity() {
    let (mut producer, mut mixer, basis) = rig();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 2]).unwrap();
    producer.request_pause(false);
    mixer.render(&mut [0.0; 2]).unwrap();
    assert!(mixer.retire_source_playback_before(9).unwrap() > 0);
    assert_eq!(
        mixer
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        9
    );
    let m = mailbox(&mixer, basis);
    assert_eq!(m.validate_identity(basis, (3, 5, 2)), Ok(()));
    assert_eq!(
        m.request(basis, (3, 5, 2), 8),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(payload(&m), (0, 0));
    assert_eq!(m.request(basis, (3, 5, 2), 9), Ok(Some(2)));
}

#[test]
fn maximum_mathematical_pin_is_preserved_without_claiming_generated_bounds() {
    let (_, mixer, basis) = rig();
    let m = mailbox(&mixer, basis);
    let id = identity(&mixer);
    // Mailbox-only boundary: a real native caller must separately authenticate
    // this range. No real Mixer has generated u64::MAX frames here.
    assert_eq!(m.request(basis, id, u64::MAX), Ok(Some(2)));
    assert_eq!(m.latest().unwrap().source_pin, u64::MAX);
    assert_eq!(m.request(basis, id, u64::MAX), Ok(Some(2)));
    assert_eq!(
        m.request(basis, id, u64::MAX - 1),
        Err(AudioError::InvalidFormat)
    );
}

#[test]
fn exhausted_revision_preserves_payload_and_still_accepts_duplicate() {
    let (_, mixer, basis) = rig();
    let m = mailbox(&mixer, basis);
    let id = identity(&mixer);
    m.request(basis, id, 5).unwrap().unwrap();
    m.sequence.store(u64::MAX - 1, Ordering::SeqCst);
    let before = payload(&m);
    assert_eq!(m.request(basis, id, 6), Err(AudioError::Overflow));
    assert_eq!(payload(&m), before);
    assert_eq!(m.request(basis, id, 5), Ok(Some(u64::MAX - 1)));
    assert_eq!(payload(&m), before);
}

#[test]
fn odd_writer_is_bounded_busy_and_partial_payload_is_not_visible() {
    let (_, mixer, basis) = rig();
    let m = mailbox(&mixer, basis);
    m.sequence.store(3, Ordering::SeqCst);
    m.source_pin.store(u64::MAX, Ordering::SeqCst);
    let before = payload(&m);
    for _ in 0..16 {
        assert!(m.latest().is_none());
        assert_eq!(m.request(basis, identity(&mixer), 6), Ok(None));
    }
    assert_eq!(payload(&m), before);
}

#[test]
fn malformed_even_payload_cannot_be_read_or_overwritten() {
    let (_, mixer, basis) = rig();
    let m = mailbox(&mixer, basis);
    m.sequence.store(2, Ordering::SeqCst);
    m.source_pin.store(4, Ordering::SeqCst);
    let before = payload(&m);
    assert!(m.latest().is_none());
    assert_eq!(
        m.request(basis, identity(&mixer), 6),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(payload(&m), before);
}

#[test]
fn cold_constructor_request_and_read_have_zero_calibrated_heap_calls() {
    let (_, calibration) = count_heap_calls(|| {
        std::hint::black_box(Vec::<u64>::with_capacity(8));
    });
    assert!(calibration > 0);
    let (_, mixer, basis) = rig();
    let id = identity(&mixer);
    let ((), calls) = count_heap_calls(|| {
        let m = mailbox(&mixer, basis);
        assert!(m.latest().is_none());
        for pin in 5..133 {
            let revision = m.request(basis, id, pin).unwrap().unwrap();
            assert_eq!(
                m.latest().unwrap(),
                SourceHistoryRetirementRequest {
                    revision,
                    source_pin: pin
                }
            );
            assert_eq!(m.request(basis, id, pin), Ok(Some(revision)));
        }
        assert_eq!(
            m.request(basis, (4, 5, 2), 133),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(m.request(basis, id, 5), Err(AudioError::InvalidFormat));
    });
    assert_eq!(calls, 0);
}

#[test]
fn concurrent_reader_observes_matching_revision_and_pin() {
    let (_, mixer, basis) = rig();
    let id = identity(&mixer);
    let m = Arc::new(mailbox(&mixer, basis));
    let writer = Arc::clone(&m);
    let thread = std::thread::spawn(move || {
        for offset in 0..4096u64 {
            assert_eq!(
                writer.request(basis, id, 5 + offset),
                Ok(Some(2 * (offset + 1)))
            );
        }
    });
    for _ in 0..16384 {
        if let Some(request) = m.latest() {
            assert_eq!(request.revision, 2 * (request.source_pin - 4));
        }
    }
    thread.join().unwrap();
    assert_eq!(
        m.latest(),
        Some(SourceHistoryRetirementRequest {
            revision: 8192,
            source_pin: 4100
        })
    );
}

#[test]
fn concurrent_identical_producers_publish_only_one_revision() {
    let (_, mixer, basis) = rig();
    let id = identity(&mixer);
    let m = Arc::new(mailbox(&mixer, basis));
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let m = Arc::clone(&m);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..1024 {
                    if let Some(revision) = m.request(basis, id, 6).unwrap() {
                        assert_eq!(revision, 2);
                    }
                }
            })
        })
        .collect();
    barrier.wait();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(
        m.latest(),
        Some(SourceHistoryRetirementRequest {
            revision: 2,
            source_pin: 6
        })
    );
}
