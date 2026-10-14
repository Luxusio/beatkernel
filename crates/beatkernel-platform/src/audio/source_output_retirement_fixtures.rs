//! Shared retirement authorization against the complete, real Source owner.
use super::*;
use crate::audio::{count_heap_calls, DeviceFormat, SampleEncoding};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::{
    cell::Cell,
    sync::{atomic::AtomicU32, Barrier},
};

fn rig(sample: f32) -> (CommandProducer, NativeOutputState) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(512, 512, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![sample; 128], limits).unwrap(),
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
    let mut mixer = Mixer::new(
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
    mixer.enable_source_playback_history(16).unwrap();
    let device = DeviceFormat::new(1000, 1, SampleEncoding::Float32, None).unwrap();
    let state = NativeOutputState::new(mixer, device, None, 8)
        .unwrap_or_else(|_| panic!("valid complete Source owner"));
    (producer, state)
}

#[derive(Debug, PartialEq)]
struct OwnerObservation {
    report: Option<RenderReport>,
    admitted: usize,
    samples: Vec<f32>,
    samples_address: usize,
    basis: OutputFrameBasis,
    counters: AudioCounters,
    physical: u64,
    playback: u64,
    history_grid: (u32, u64, u64, u64, u64, u64, usize),
    history_spans: Vec<SourcePlaybackSpan>,
}

fn observe_owner(state: &NativeOutputState) -> OwnerObservation {
    let history = state.mixer().source_playback_history().unwrap();
    OwnerObservation {
        report: state.pending_report(),
        admitted: state.admitted_frames(),
        samples: state.pending_samples().to_vec(),
        samples_address: state.pending_samples().as_ptr() as usize,
        basis: state.output_frame_basis(),
        counters: state.mixer().counters(),
        physical: state.mixer().frame_cursor(),
        playback: state.mixer().playback_frame_cursor(),
        history_grid: (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame(),
            history.retained_start_frame(),
            history.retirement_revision(),
            history.capacity(),
        ),
        history_spans: (0..history.len())
            .map(|i| history.span(i).unwrap())
            .collect(),
    }
}

#[test]
fn busy_results_and_stale_or_zero_tickets_preserve_checked_nonces() {
    let (_, mut state) = rig(0.25);
    state.render_pending(4).unwrap();
    let slot = SourceRetirementSlot::new();
    for nonce in [0, 1, u64::MAX] {
        assert_eq!(
            slot.status(nonce),
            Err(SourceRetirementError::InvalidTicket)
        );
        assert_eq!(slot.poll(nonce), Err(SourceRetirementError::InvalidTicket));
        assert_eq!(
            slot.cancel(nonce),
            Err(SourceRetirementError::InvalidTicket)
        );
    }
    let first = slot
        .submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    assert_eq!(first, 1);
    assert_eq!(
        slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
        Ok(None)
    );
    assert_eq!(slot.poll(first), Ok(SourceRetirementStatus::Pending));
    assert_eq!(slot.decision(&state), SourceRetirementDecision::Continue);
    assert_eq!(slot.status(first), Ok(SourceRetirementStatus::Refused));
    assert_eq!(
        slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
        Ok(None)
    );
    assert_eq!(slot.cancel(first), Ok(SourceRetirementStatus::Refused));
    assert_eq!(slot.poll(first), Ok(SourceRetirementStatus::Refused));
    assert_eq!(
        slot.status(first),
        Err(SourceRetirementError::InvalidTicket)
    );
    let second = slot
        .submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .unwrap();
    assert_eq!(second, 2);
    for nonce in [0, first, second + 1, u64::MAX] {
        assert_eq!(slot.poll(nonce), Err(SourceRetirementError::InvalidTicket));
        assert_eq!(
            slot.cancel(nonce),
            Err(SourceRetirementError::InvalidTicket)
        );
    }
    assert_eq!(slot.cancel(second), Ok(SourceRetirementStatus::Cancelled));
    assert_eq!(
        slot.submit(SourceRetirementPolicy::RequireEmptySuffix),
        Ok(None)
    );
    assert_eq!(slot.decision(&state), SourceRetirementDecision::Continue);
    assert_eq!(slot.poll(second), Ok(SourceRetirementStatus::Cancelled));
    assert_eq!(
        slot.cancel(second),
        Err(SourceRetirementError::InvalidTicket)
    );
    assert_eq!(
        slot.submit(SourceRetirementPolicy::RequireEmptySuffix),
        Ok(Some(3))
    );
}

#[test]
fn maximum_61_bit_nonce_is_usable_once_then_exhaustion_is_immutable() {
    let slot = SourceRetirementSlot::new();
    slot.word
        .store((MAX_NONCE - 1) << NONCE_SHIFT, Ordering::Release);
    assert_eq!(
        slot.submit(SourceRetirementPolicy::RequireEmptySuffix),
        Ok(Some(MAX_NONCE))
    );
    assert_eq!(
        slot.cancel(MAX_NONCE),
        Ok(SourceRetirementStatus::Cancelled)
    );
    assert_eq!(slot.poll(MAX_NONCE), Ok(SourceRetirementStatus::Cancelled));
    let exhausted_word = MAX_NONCE << NONCE_SHIFT;
    assert_eq!(slot.word.load(Ordering::Acquire), exhausted_word);
    let payload = AtomicU32::new(17);
    for policy in [
        SourceRetirementPolicy::PreservePausedSuffix,
        SourceRetirementPolicy::RequireEmptySuffix,
    ] {
        assert_eq!(
            slot.submit(policy),
            Err(SourceRetirementError::NoncesExhausted)
        );
        assert_eq!(
            slot.submit_prepared(policy, || payload.store(99, Ordering::Relaxed)),
            Err(SourceRetirementError::NoncesExhausted)
        );
        assert_eq!(payload.load(Ordering::Relaxed), 17);
        assert_eq!(slot.word.load(Ordering::Acquire), exhausted_word);
    }
    assert_eq!(
        slot.status(MAX_NONCE),
        Err(SourceRetirementError::InvalidTicket)
    );
}

#[test]
fn empty_and_actual_paused_zero_suffix_retire_without_source_changes() {
    for policy in [
        SourceRetirementPolicy::PreservePausedSuffix,
        SourceRetirementPolicy::RequireEmptySuffix,
    ] {
        for tail in 0..4 {
            let (mut producer, mut state) = rig(if tail == 3 { 0. } else { 0.25 });
            if tail == 1 {
                producer.request_pause(true);
            }
            if tail != 0 {
                state.render_pending(4).unwrap();
                state.admit(1).unwrap();
                assert_eq!(state.pending_frames(), 3);
                assert_eq!(state.pending_report().unwrap().paused, tail == 1);
                assert_eq!(
                    state.pending_samples().iter().all(|sample| *sample == 0.),
                    tail != 2
                );
            }
            let before = observe_owner(&state);
            let slot = SourceRetirementSlot::new();
            let nonce = slot.submit(policy).unwrap().unwrap();
            let permitted =
                tail == 0 || (tail == 1 && policy == SourceRetirementPolicy::PreservePausedSuffix);
            assert_eq!(
                slot.decision(&state),
                if permitted {
                    SourceRetirementDecision::Retire
                } else {
                    SourceRetirementDecision::Continue
                }
            );
            assert_eq!(
                slot.status(nonce),
                Ok(if permitted {
                    SourceRetirementStatus::Authorized
                } else {
                    SourceRetirementStatus::Refused
                })
            );
            assert_eq!(observe_owner(&state), before);
        }
    }
}

#[test]
fn decision_uses_current_suffix_and_additional_geometry_can_only_restrict() {
    let (_, mut state) = rig(0.25);
    let slot = SourceRetirementSlot::new();
    let first = slot
        .submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .unwrap();
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    assert_eq!(
        slot.decision_with(&state, || true),
        SourceRetirementDecision::Continue
    );
    assert_eq!(slot.poll(first), Ok(SourceRetirementStatus::Refused));

    let (mut producer, mut paused) = rig(0.25);
    producer.request_pause(true);
    paused.render_pending(4).unwrap();
    paused.admit(1).unwrap();
    let before = observe_owner(&paused);
    for (frames, expected) in [
        (2, SourceRetirementDecision::Continue),
        (3, SourceRetirementDecision::Retire),
    ] {
        let slot = SourceRetirementSlot::new();
        let nonce = slot
            .submit(SourceRetirementPolicy::PreservePausedSuffix)
            .unwrap()
            .unwrap();
        let calls = Cell::new(0);
        assert_eq!(
            slot.decision_with(&paused, || {
                calls.set(calls.get() + 1);
                paused.pending_frames() % frames == 0
            }),
            expected
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(
            slot.status(nonce),
            Ok(if frames == 2 {
                SourceRetirementStatus::Refused
            } else {
                SourceRetirementStatus::Authorized
            })
        );
        assert_eq!(observe_owner(&paused), before);
    }
    let (_, empty) = rig(0.25);
    let slot = SourceRetirementSlot::new();
    let nonce = slot
        .submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    assert_eq!(
        slot.decision_with(&empty, || false),
        SourceRetirementDecision::Continue
    );
    assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Refused));
}

#[test]
fn authorization_is_sticky_and_quarantine_never_creates_permission() {
    let (_, mut state) = rig(0.25);
    let slot = SourceRetirementSlot::new();
    let calls = Cell::new(0);
    assert_eq!(
        slot.decision_with(&state, || {
            calls.set(1);
            true
        }),
        SourceRetirementDecision::Continue
    );
    assert_eq!(calls.get(), 0);
    let nonce = slot
        .submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    assert_eq!(slot.decision(&state), SourceRetirementDecision::Retire);
    state.mark_admission_uncertain();
    for _ in 0..2 {
        assert_eq!(
            slot.decision_with(&state, || {
                calls.set(1);
                false
            }),
            SourceRetirementDecision::Retire
        );
        assert_eq!(slot.cancel(nonce), Ok(SourceRetirementStatus::Authorized));
        assert_eq!(slot.poll(nonce), Ok(SourceRetirementStatus::Authorized));
        assert_eq!(
            slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
            Ok(None)
        );
    }
    assert_eq!(calls.get(), 0);

    let quarantined = SourceRetirementSlot::new();
    assert_eq!(
        quarantined.decision(&state),
        SourceRetirementDecision::Quarantined
    );
    let ticket = quarantined
        .submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .unwrap();
    let before = observe_owner(&state);
    assert_eq!(
        quarantined.decision_with(&state, || {
            calls.set(1);
            true
        }),
        SourceRetirementDecision::Quarantined
    );
    assert_eq!(
        quarantined.status(ticket),
        Ok(SourceRetirementStatus::Pending)
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(observe_owner(&state), before);
    assert_eq!(
        quarantined.cancel(ticket),
        Ok(SourceRetirementStatus::Cancelled)
    );
    assert_eq!(
        quarantined.poll(ticket),
        Ok(SourceRetirementStatus::Cancelled)
    );
}

#[test]
fn prepared_payload_is_synchronous_and_busy_states_do_not_mutate_it() {
    let (_, mut state) = rig(0.25);
    let slot = SourceRetirementSlot::new();
    let payload = AtomicU32::new(17);
    let first = slot
        .submit_prepared(SourceRetirementPolicy::RequireEmptySuffix, || {
            payload.store(4, Ordering::Relaxed);
        })
        .unwrap()
        .unwrap();
    assert_eq!(payload.load(Ordering::Relaxed), 4);
    for stage in 0..4 {
        match stage {
            0 => assert_eq!(slot.status(first), Ok(SourceRetirementStatus::Pending)),
            1 => {
                state.render_pending(4).unwrap();
                assert_eq!(slot.decision(&state), SourceRetirementDecision::Continue);
                assert_eq!(slot.status(first), Ok(SourceRetirementStatus::Refused));
            }
            2 => {
                assert_eq!(slot.poll(first), Ok(SourceRetirementStatus::Refused));
                let nonce = slot
                    .submit(SourceRetirementPolicy::RequireEmptySuffix)
                    .unwrap()
                    .unwrap();
                assert_eq!(slot.cancel(nonce), Ok(SourceRetirementStatus::Cancelled));
            }
            3 => {
                assert_eq!(slot.poll(2), Ok(SourceRetirementStatus::Cancelled));
                state.admit(4).unwrap();
                assert_eq!(
                    slot.submit(SourceRetirementPolicy::RequireEmptySuffix),
                    Ok(Some(3))
                );
                assert_eq!(slot.decision(&state), SourceRetirementDecision::Retire);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            slot.submit_prepared(SourceRetirementPolicy::PreservePausedSuffix, || payload
                .store(99, Ordering::Relaxed)),
            Ok(None)
        );
        assert_eq!(payload.load(Ordering::Relaxed), 4);
    }
}

#[test]
fn prepared_atomic_payload_is_visible_to_the_deciding_consumer() {
    let (_, state) = rig(0.25);
    let slot = SourceRetirementSlot::new();
    let frames = AtomicU32::new(0);
    let marker = AtomicU32::new(0);
    let observed = Cell::new(None);
    let ready = Barrier::new(2);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            // Synchronize before the payload stores: their only publication
            // to the consumer below is the retirement request word.
            ready.wait();
            assert_eq!(
                slot.submit_prepared(SourceRetirementPolicy::RequireEmptySuffix, || {
                    frames.store(128, Ordering::Relaxed);
                    marker.store(!128, Ordering::Relaxed);
                }),
                Ok(Some(1))
            );
        });
        ready.wait();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            assert_eq!(
                slot.decision_with(&state, || {
                    observed.set(Some((
                        frames.load(Ordering::Relaxed),
                        marker.load(Ordering::Relaxed),
                    )));
                    false
                }),
                SourceRetirementDecision::Continue
            );
            if observed.get().is_some() {
                break;
            }
            std::thread::yield_now();
        }
    });
    assert_eq!(observed.get(), Some((128, !128)));
    assert_eq!(slot.status(1), Ok(SourceRetirementStatus::Refused));
}

#[test]
fn old_nonce_cannot_authorize_after_cancel_poll_and_new_payload_publication() {
    let (_, state) = rig(0.25);
    let slot = SourceRetirementSlot::new();
    let geometry = AtomicU32::new(0);
    let published = Barrier::new(2);
    let captured_old_word = Barrier::new(2);
    let published_replacement = Barrier::new(2);
    let observed = Cell::new(0);
    let decision = std::thread::scope(|scope| {
        let producer = scope.spawn(|| {
            let first = slot
                .submit_prepared(SourceRetirementPolicy::RequireEmptySuffix, || {
                    geometry.store(3, Ordering::Relaxed)
                })
                .unwrap()
                .unwrap();
            assert_eq!(first, 1);
            published.wait();
            captured_old_word.wait();
            assert_eq!(slot.cancel(first), Ok(SourceRetirementStatus::Cancelled));
            assert_eq!(slot.poll(first), Ok(SourceRetirementStatus::Cancelled));
            assert_eq!(
                slot.submit_prepared(SourceRetirementPolicy::RequireEmptySuffix, || geometry
                    .store(4, Ordering::Relaxed)),
                Ok(Some(2))
            );
            published_replacement.wait();
        });
        published.wait();
        let decision = slot.decision_with(&state, || {
            captured_old_word.wait();
            published_replacement.wait();
            let frames = geometry.load(Ordering::Relaxed);
            observed.set(frames);
            frames == 4
        });
        producer.join().unwrap();
        decision
    });
    assert_eq!(
        observed.get(),
        4,
        "the old decision deliberately sees the replacement payload"
    );
    assert_eq!(decision, SourceRetirementDecision::Continue);
    assert_eq!(slot.status(1), Err(SourceRetirementError::InvalidTicket));
    assert_eq!(slot.status(2), Ok(SourceRetirementStatus::Pending));
    assert_eq!(
        slot.decision_with(&state, || geometry.load(Ordering::Relaxed) == 4),
        SourceRetirementDecision::Retire
    );
    assert_eq!(slot.status(2), Ok(SourceRetirementStatus::Authorized));
}

#[test]
fn cancel_inside_eligibility_cas_prevents_authorization() {
    let (_, state) = rig(0.25);
    let slot = SourceRetirementSlot::new();
    let nonce = slot
        .submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    assert_eq!(
        slot.decision_with(&state, || {
            assert_eq!(slot.cancel(nonce), Ok(SourceRetirementStatus::Cancelled));
            true
        }),
        SourceRetirementDecision::Continue
    );
    assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Cancelled));
    assert_eq!(slot.decision(&state), SourceRetirementDecision::Continue);
}

#[test]
fn slot_control_and_decision_branches_have_zero_calibrated_heap_calls() {
    let (_, calibration) = count_heap_calls(|| {
        std::hint::black_box(Vec::<u64>::with_capacity(8));
    });
    assert!(calibration > 0);
    let (_, mut state) = rig(0.25);
    state.render_pending(4).unwrap();
    let (_, empty) = rig(0.25);
    let (_, mut uncertain) = rig(0.25);
    uncertain.mark_admission_uncertain();
    let exhausted = SourceRetirementSlot::new();
    exhausted
        .word
        .store(MAX_NONCE << NONCE_SHIFT, Ordering::Release);
    let payload = AtomicU32::new(0);
    let ((), calls) = count_heap_calls(|| {
        let slot = SourceRetirementSlot::new();
        assert_eq!(slot.decision(&empty), SourceRetirementDecision::Continue);
        let first = slot
            .submit_prepared(SourceRetirementPolicy::RequireEmptySuffix, || {
                payload.store(4, Ordering::Relaxed)
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            slot.submit_prepared(SourceRetirementPolicy::RequireEmptySuffix, || payload
                .store(99, Ordering::Relaxed)),
            Ok(None)
        );
        assert_eq!(slot.status(first), Ok(SourceRetirementStatus::Pending));
        assert_eq!(
            slot.decision_with(&state, || true),
            SourceRetirementDecision::Continue
        );
        assert_eq!(slot.poll(first), Ok(SourceRetirementStatus::Refused));
        let second = slot
            .submit(SourceRetirementPolicy::RequireEmptySuffix)
            .unwrap()
            .unwrap();
        assert_eq!(slot.cancel(second), Ok(SourceRetirementStatus::Cancelled));
        assert_eq!(slot.poll(second), Ok(SourceRetirementStatus::Cancelled));
        assert_eq!(slot.status(0), Err(SourceRetirementError::InvalidTicket));
        let third = slot
            .submit(SourceRetirementPolicy::RequireEmptySuffix)
            .unwrap()
            .unwrap();
        assert_eq!(
            slot.decision_with(&empty, || payload.load(Ordering::Relaxed) == 4),
            SourceRetirementDecision::Retire
        );
        assert_eq!(
            slot.decision_with(&empty, || false),
            SourceRetirementDecision::Retire
        );
        assert_eq!(slot.poll(third), Ok(SourceRetirementStatus::Authorized));
        assert_eq!(
            SourceRetirementSlot::new().decision(&uncertain),
            SourceRetirementDecision::Quarantined
        );
        assert_eq!(
            exhausted.submit_prepared(SourceRetirementPolicy::RequireEmptySuffix, || payload
                .store(99, Ordering::Relaxed)),
            Err(SourceRetirementError::NoncesExhausted)
        );
    });
    assert_eq!(calls, 0);
    assert_eq!(payload.load(Ordering::Relaxed), 4);
}
