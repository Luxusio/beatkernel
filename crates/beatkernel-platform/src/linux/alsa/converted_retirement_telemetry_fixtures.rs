//! Actual complete-owner retirement under the existing converted publication.
use super::*;
use super::super::converted_fixtures::rig_boundaries;
use beatkernel::audio::*;
use std::sync::Arc;

fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn nearby(time: TargetTime, delta: i64) -> InterpolatedTargetTime {
    // Less than one nanosecond: the original rational coordinate is retained.
    InterpolatedTargetTime::new(time, 0, 1, 1, i128::from(delta), 10_000_000_000).unwrap()
}
fn owner(
    source: u32,
    target: u32,
    capacity: usize,
) -> (CommandProducer, ConvertedNativeOutputState) {
    let (producer, mut state, _) = rig_boundaries(16, source, target, None, None);
    state.enable_phase_history(capacity).unwrap();
    state.enable_source_playback_history(capacity).unwrap();
    (producer, state)
}
fn generate(
    state: &mut ConvertedNativeOutputState,
    telemetry: &ConvertedTelemetry,
    held: bool,
    frames: usize,
) -> ConvertedRenderReport {
    let report = if held {
        state.render_held_pending(frames).unwrap()
    } else {
        state.render_pending(frames).unwrap()
    };
    telemetry.publish_generated(Some(report), state);
    state.admit(frames).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    report
}
fn three(
    state: &mut ConvertedNativeOutputState,
    telemetry: &ConvertedTelemetry,
) -> (ConvertedRenderReport, ConvertedRenderReport) {
    generate(state, telemetry, false, 2);
    let hold = generate(state, telemetry, true, 2);
    let resume = generate(state, telemetry, false, 2);
    assert_eq!(state.phase_history().unwrap().len(), 3);
    (hold, resume)
}
fn header(telemetry: &ConvertedTelemetry) -> [u64; 18] {
    std::array::from_fn(|i| telemetry.phase.as_ref().unwrap().values[i].load(Ordering::SeqCst))
}

#[test]
fn retirement_none_publication_keeps_original_cold_stream_and_positive_submission() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut state) = owner(source, target, 3);
        let telemetry =
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
        let (held, resumed) = three(&mut state, &telemetry);
        let original = header(&telemetry);
        let report = telemetry.read().unwrap();
        let generation = telemetry.generation.load(Ordering::SeqCst);
        assert_eq!(
            state.retire_phase_history_before(held.target_start_time),
            Ok(1)
        );
        telemetry.publish_generated(None, &state);
        assert_eq!(telemetry.generation.load(Ordering::SeqCst), generation + 2);
        let after = header(&telemetry);
        assert_eq!(after[0], 2);
        assert_eq!(after[1..], original[1..]);
        assert_eq!(telemetry.read().unwrap().0, report.0);
        assert_eq!(telemetry.read().unwrap().1, report.1);
        assert!(telemetry
            .phase_snapshot_at_interpolated_time(&nearby(held.target_start_time, -1))
            .is_none());
        for q in [
            exact(held.target_start_time),
            nearby(held.target_start_time, 1),
            exact(held.target_end_time),
            exact(resumed.target_end_time),
        ] {
            let snap = telemetry.phase_snapshot_at_interpolated_time(&q).unwrap();
            assert_eq!(
                snap.stream_start,
                state.phase_history().unwrap().start_time()
            );
            assert_eq!(snap.generated_end, resumed.target_end_time);
            assert_eq!(snap.submitted_end, resumed.target_end_time);
            let expected = if q.compare_target_time(held.target_end_time).is_lt() {
                2_000_000_000_i128 / i128::from(target)
            } else {
                // Active source duration after a two-frame explicit hold.
                let t = if q.compare_target_time(resumed.target_end_time).is_eq() {
                    4
                } else {
                    2
                };
                i128::from(t) * 1_000_000_000 / i128::from(target)
            };
            if q.compare_target_time(held.target_start_time).is_eq()
                || q.compare_target_time(held.target_end_time).is_eq()
                || q.compare_target_time(resumed.target_end_time).is_eq()
            {
                assert_eq!(
                    snap.span
                        .elapsed_nanos_at_interpolated_time(q, snap.segment_end, 0),
                    Ok(expected)
                );
            }
        }
        let revision = telemetry.generation.load(Ordering::SeqCst);
        telemetry.publish_generated(None, &state);
        assert_eq!(telemetry.generation.load(Ordering::SeqCst), revision);
    }
}

#[test]
fn retire_then_restore_count_cannot_take_unchanged_frontier_fast_path() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut state) = owner(source, target, 3);
        let telemetry =
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
        let (_, resume) = three(&mut state, &telemetry);
        assert_eq!(
            state.retire_phase_history_before(resume.target_start_time),
            Ok(2)
        );
        let held = state.render_held_pending(2).unwrap();
        state.admit(2).unwrap();
        state.render_pending(2).unwrap();
        state.admit(2).unwrap();
        assert_eq!(state.phase_history().unwrap().len(), 3);
        telemetry.publish_generated(None, &state);
        telemetry.publish_submitted(state.admitted_target_time().unwrap());
        assert!(telemetry
            .phase
            .as_ref()
            .unwrap()
            .available
            .load(Ordering::SeqCst));
        assert!(telemetry
            .phase_snapshot_at(resume.target_start_time)
            .is_some());
        let snapshot = telemetry.phase_snapshot_at(held.target_start_time).unwrap();
        assert_eq!(snapshot.span.to_words()[7], 1);
        assert_eq!(
            snapshot.generated_end,
            state.phase_history().unwrap().end_time()
        );
        assert!(telemetry
            .phase_snapshot_at(TargetTime::from_frames(0, target).unwrap())
            .is_none());
    }
}

#[test]
fn already_retired_seed_needs_this_streams_fresh_positive_admission() {
    let (_, mut state) = owner(3, 7, 3);
    let previous =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let (_, resumed) = three(&mut state, &previous);
    assert_eq!(
        state.retire_phase_history_before(resumed.target_start_time),
        Ok(2)
    );
    let basis = state.target_frame_basis();
    let telemetry = ConvertedTelemetry::with_phase_history(&state, basis).unwrap();
    assert_eq!(
        telemetry
            .phase
            .as_ref()
            .unwrap()
            .retirement_revision
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(header(&telemetry)[HAS_SUBMITTED], 0);
    assert!(telemetry.phase_snapshot_at(basis.start_time()).is_none());
    let next = state.render_pending(2).unwrap();
    telemetry.publish_generated(Some(next), &state);
    assert!(telemetry
        .phase_snapshot_at(next.target_start_time)
        .is_none());
    state.admit(1).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    assert!(telemetry
        .phase_snapshot_at(resumed.target_start_time)
        .is_none());
    assert!(telemetry
        .phase_snapshot_at(next.target_start_time)
        .is_some());
    assert!(telemetry.phase_snapshot_at(next.target_end_time).is_none());
    assert_eq!(
        telemetry
            .phase_snapshot_at(next.target_start_time)
            .unwrap()
            .stream_start,
        basis.start_time()
    );
}

#[test]
fn actual_source_and_target_retire_together_under_one_existing_generation() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut producer, mut state) = owner(source, target, 3);
        let telemetry =
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
        generate(&mut state, &telemetry, false, target as usize);
        producer.request_pause(true);
        generate(&mut state, &telemetry, false, target as usize);
        producer.request_pause(false);
        let resume = generate(&mut state, &telemetry, false, target as usize);
        let h = state.converter_owner().source_playback_history().unwrap();
        assert_eq!(h.len(), 3);
        let source_pin = h.span(2).unwrap().physical_start();
        let lower = resume.resume_boundary().unwrap().unwrap().target_time;
        let original = telemetry
            .playback_phase_snapshot_at_interpolated_time_with_identity(&exact(lower), source, 0, 0)
            .unwrap();
        assert_eq!(state.retire_source_playback_before(source_pin), Ok(2));
        // Phase active correspondence merges across actual Source pause changes;
        // its remaining descriptor must still project the original Source spans.
        state.retire_phase_history_before(lower).unwrap();
        let before = telemetry.generation.load(Ordering::SeqCst);
        telemetry.publish_generated(None, &state);
        assert_eq!(telemetry.generation.load(Ordering::SeqCst), before + 2);
        let actual = telemetry
            .playback_phase_snapshot_at_interpolated_time_with_identity(&exact(lower), source, 0, 0)
            .unwrap();
        assert_eq!(actual.phase.revision, before + 2);
        assert_eq!(
            actual.playback.physical_start(),
            original.playback.physical_start()
        );
        assert_eq!(
            actual.playback.playback_start(),
            original.playback.playback_start()
        );
        assert_eq!(actual.playback.state(), original.playback.state());
        assert_eq!(actual.pulled_source_end, original.pulled_source_end);
        assert!(telemetry
            .playback_phase_snapshot_at_interpolated_time_with_identity(&exact(lower), source, 1, 0)
            .is_none());
        assert!(telemetry
            .playback_phase_snapshot_at_interpolated_time_with_identity(
                &exact(lower),
                source + 1,
                0,
                0
            )
            .is_none());
        assert!(telemetry
            .playback_phase_snapshot_at_interpolated_time_with_identity(
                &exact(TargetTime::from_frames(0, target).unwrap()),
                source,
                0,
                0
            )
            .is_none());
    }
}

#[test]
fn fixed_capacity_real_cycles_preserve_reference_pcm_and_reports_without_heap() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut state) = owner(source, target, 3);
        let (_, mut reference) = owner(source, target, 80);
        let telemetry =
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
        generate(&mut state, &telemetry, false, 2);
        reference.render_pending(2).unwrap();
        reference.admit(2).unwrap();
        for cycle in 0..24 {
            for held in [true, false] {
                let (report, allocations) = crate::audio::count_heap_calls(|| {
                    if held {
                        state.render_held_pending(2).unwrap()
                    } else {
                        state.render_pending(2).unwrap()
                    }
                });
                assert_eq!(allocations, 0);
                let expected = if held {
                    reference.render_held_pending(2).unwrap()
                } else {
                    reference.render_pending(2).unwrap()
                };
                assert_eq!(report, expected);
                assert_eq!(state.pending_samples(), reference.pending_samples());
                telemetry.publish_generated(Some(report), &state);
                state.admit(2).unwrap();
                reference.admit(2).unwrap();
                telemetry.publish_submitted(state.admitted_target_time().unwrap());
            }
            let pin = span_start(state.phase_history().unwrap().span(2).unwrap()).unwrap();
            let (removed, allocations) = crate::audio::count_heap_calls(|| {
                let removed = state.retire_phase_history_before(pin).unwrap();
                telemetry.publish_generated(None, &state);
                let snap = telemetry.phase_snapshot_at(pin).unwrap();
                assert_eq!(
                    snap.generated_end,
                    state.phase_history().unwrap().end_time()
                );
                removed
            });
            assert_eq!(removed, 2);
            assert_eq!(allocations, 0);
            assert_eq!(state.phase_history().unwrap().capacity(), 3);
            assert_eq!(state.phase_history().unwrap().len(), 1);
            assert_eq!(
                state.phase_history().unwrap().retirement_revision(),
                cycle + 1
            );
        }
    }
}

#[test]
fn stale_nonce_and_same_nonce_corrupt_count_permanently_disable_publication() {
    for corruption in 0..3 {
        let (_, mut state) = owner(3, 7, 3);
        let telemetry =
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
        let (hold, resume) = three(&mut state, &telemetry);
        let phase = telemetry.phase.as_ref().unwrap();
        match corruption {
            0 => {
                phase.values[0].store(4, Ordering::SeqCst);
            }
            1 => {
                phase.retirement_revision.store(1, Ordering::SeqCst);
            }
            _ => {
                let future = resume.target_end_time.checked_add_frames(1, 7).unwrap();
                let mut words = [0; 3];
                encode_time(future, &mut words);
                for (field, value) in phase.values[GENERATED..GENERATED + 3].iter().zip(words) {
                    field.store(value, Ordering::SeqCst);
                }
            }
        }
        telemetry.publish_generated(Some(resume), &state);
        assert!(!phase.available.load(Ordering::SeqCst));
        assert!(telemetry
            .phase_snapshot_at(hold.target_start_time)
            .is_none());
        // Actual later progress/retirement cannot revive poisoned provenance.
        state
            .retire_phase_history_before(resume.target_start_time)
            .unwrap();
        telemetry.publish_generated(None, &state);
        let next = state.render_pending(2).unwrap();
        telemetry.publish_generated(Some(next), &state);
        assert!(!phase.available.load(Ordering::SeqCst));
        assert!(telemetry
            .phase_snapshot_at(resume.target_start_time)
            .is_none());
    }
}

#[test]
fn retired_reader_still_refuses_odd_and_exhausted_owning_generation_without_heap() {
    let (_, mut state) = owner(7, 3, 3);
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let (_, resume) = three(&mut state, &telemetry);
    state
        .retire_phase_history_before(resume.target_start_time)
        .unwrap();
    telemetry.publish_generated(None, &state);
    let generation = telemetry.generation.load(Ordering::SeqCst);
    for invalid in [generation + 1, u64::MAX] {
        telemetry.generation.store(invalid, Ordering::SeqCst);
        let (snapshot, allocations) = crate::audio::count_heap_calls(|| {
            telemetry.phase_snapshot_at(resume.target_start_time)
        });
        assert!(snapshot.is_none());
        assert_eq!(allocations, 0);
    }
    telemetry.generation.store(generation, Ordering::SeqCst);
    assert!(telemetry
        .phase_snapshot_at(resume.target_start_time)
        .is_some());
}

#[test]
fn concurrent_reader_observes_coherent_retained_spans_during_real_owner_cycles() {
    let (_, mut state) = owner(3, 7, 3);
    let telemetry = Arc::new(
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap(),
    );
    generate(&mut state, &telemetry, false, 2);
    std::thread::scope(|scope| {
        let writer = Arc::clone(&telemetry);
        let handle = scope.spawn(move || {
            for _ in 0..32 {
                generate(&mut state, &writer, true, 2);
                let resumed = generate(&mut state, &writer, false, 2);
                state
                    .retire_phase_history_before(resumed.target_start_time)
                    .unwrap();
                writer.publish_generated(None, &state);
            }
            state
        });
        for frame in 0..130 {
            let q = TargetTime::from_frames(frame, 7).unwrap();
            if let Some(snap) = telemetry.phase_snapshot_at(q) {
                assert_eq!(snap.revision % 2, 0);
                assert!(!compare_time(q, span_start(snap.span).unwrap()).is_lt());
                assert!(!compare_time(q, snap.segment_end).is_gt());
                assert!(!compare_time(q, snap.submitted_end).is_gt());
                assert!(!compare_time(snap.submitted_end, snap.generated_end).is_gt());
                assert_eq!(snap.stream_start, TargetTime::from_frames(0, 7).unwrap());
                assert_eq!(snap.report.unwrap().source_rate, 3);
            }
        }
        let state = handle.join().unwrap();
        let snap = telemetry
            .phase_snapshot_at(state.phase_history().unwrap().end_time())
            .unwrap();
        assert_eq!(
            snap.generated_end,
            state.phase_history().unwrap().end_time()
        );
        assert_eq!(header(&telemetry)[0], 1);
    });
}

#[test]
fn earlier_actual_source_creation_remains_identity_after_converted_retirement() {
    use beatkernel::time::{ClockDomainId, Timestamp};
    for (source, target) in [(3, 7), (7, 3)] {
        let format = AudioFormat::new(source, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
        let (_, consumer) = command_queue(16).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(7),
                Timestamp::ZERO,
                AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        mixer.render(&mut [0.0]).unwrap();
        mixer.enable_source_playback_history(3).unwrap();
        mixer.render(&mut [0.0]).unwrap();
        let device = DeviceFormat::new(target, 1, SampleEncoding::Float32, None).unwrap();
        let mut state = ConvertedNativeOutputState::new(
            mixer,
            device,
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            16,
        )
        .unwrap_or_else(|_| panic!("actual complete owner"));
        state.enable_phase_history(3).unwrap();
        let telemetry =
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
        let (hold, resume) = three(&mut state, &telemetry);
        let query = exact(hold.target_start_time);
        let original = telemetry
            .playback_phase_snapshot_at_interpolated_time_with_identity(&query, source, 1, 1)
            .unwrap();
        assert!(telemetry
            .playback_phase_snapshot_at_interpolated_time(&query)
            .is_none());
        assert_eq!(
            state
                .phase_history()
                .unwrap()
                .initial_source_position()
                .frame,
            2
        );
        state
            .retire_phase_history_before(hold.target_start_time)
            .unwrap();
        telemetry.publish_generated(None, &state);
        let actual = telemetry
            .playback_phase_snapshot_at_interpolated_time_with_identity(&query, source, 1, 1)
            .unwrap();
        assert_eq!(actual.phase.span.to_words(), original.phase.span.to_words());
        assert_eq!(actual.playback.physical_start(), 1);
        assert_eq!(actual.playback.playback_start(), 1);
        assert_eq!(actual.phase.generated_end, resume.target_end_time);
        assert!(telemetry
            .playback_phase_snapshot_at_interpolated_time_with_identity(&query, source, 2, 2)
            .is_none());
        assert_eq!(
            state
                .converter_owner()
                .source_playback_history()
                .unwrap()
                .initial_physical_frame(),
            1
        );
    }
}

#[test]
fn partial_pending_admission_protects_original_block_while_retirement_is_published() {
    let (_, mut state) = owner(3, 7, 3);
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let (_, resume) = three(&mut state, &telemetry);
    state
        .retire_phase_history_before(resume.target_start_time)
        .unwrap();
    telemetry.publish_generated(None, &state);
    let held = state.render_held_pending(2).unwrap();
    telemetry.publish_generated(Some(held), &state);
    state.admit(1).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let submitted = state.admitted_target_time().unwrap();
    assert_eq!(
        state.retire_phase_history_before(held.target_start_time),
        Ok(1)
    );
    telemetry.publish_generated(None, &state);
    let snapshot = telemetry.phase_snapshot_at(held.target_start_time).unwrap();
    assert_eq!(snapshot.report, Some(held));
    assert_eq!(snapshot.submitted_end, submitted);
    assert_eq!(snapshot.generated_end, held.target_end_time);
    assert_eq!(state.pending_frames(), 1);
    let revision = telemetry.generation.load(Ordering::SeqCst);
    let (result, allocations) =
        crate::audio::count_heap_calls(|| state.retire_phase_history_before(held.target_end_time));
    assert_eq!(result, Err(AudioError::InvalidFormat));
    assert_eq!(allocations, 0);
    telemetry.publish_generated(None, &state);
    assert_eq!(telemetry.generation.load(Ordering::SeqCst), revision);
    assert_eq!(state.pending_report(), Some(held));
    state.admit(1).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    assert!(telemetry.phase_snapshot_at(held.target_end_time).is_some());
}
