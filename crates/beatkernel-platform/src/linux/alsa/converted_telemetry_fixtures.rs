use super::super::converted_fixtures::rig_boundaries;
use super::*;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockMappingQuality, ClockPoint, Timestamp},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn phase_time(frame: u64) -> TargetTime {
    TargetTime::from_frames(frame, 48_000).unwrap()
}

fn interpolated_exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}

fn playback_publish(
    telemetry: &ConvertedTelemetry,
    state: &mut crate::audio::ConvertedNativeOutputState,
    frames: usize,
) -> ConvertedRenderReport {
    let report = state.render_pending(frames).unwrap();
    telemetry.publish_generated(Some(report), state);
    state.admit(frames).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    report
}

#[test]
fn compound_actual_callback_publishes_startup_running_and_finite_tail_without_heap() {
    let (_producer, mut state, _) = rig_boundaries(16, 1000, 1000, Some(2), Some(3));
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(3).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let (report, calls) =
        crate::audio::count_heap_calls(|| playback_publish(&telemetry, &mut state, 8));
    assert_eq!(calls, 0);
    assert!(report.source.unwrap().paused);
    assert_eq!(
        state
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .len(),
        3
    );
    for (frame, physical, playback, expected_state, nanos) in [
        (1, 0, 0, SourcePlaybackState::Frozen, 0),
        (2, 2, 0, SourcePlaybackState::Running, 0),
        (4, 2, 0, SourcePlaybackState::Running, 2_000_000),
        (5, 5, 3, SourcePlaybackState::Frozen, 3_000_000),
        (8, 5, 3, SourcePlaybackState::Frozen, 3_000_000),
    ] {
        let query = interpolated_exact(TargetTime::from_frames(frame, 1000).unwrap());
        let (snapshot, calls) = crate::audio::count_heap_calls(|| {
            telemetry
                .playback_phase_snapshot_at_interpolated_time(&query)
                .unwrap()
        });
        assert_eq!(calls, 0);
        assert_eq!(
            (
                snapshot.playback.physical_start(),
                snapshot.playback.playback_start(),
                snapshot.playback.state()
            ),
            (physical, playback, expected_state)
        );
        assert_eq!(
            snapshot.pulled_source_end,
            report.pulled_source_frame_cursor
        );
        assert_eq!(
            snapshot
                .phase
                .span
                .playback_elapsed_nanos_at_interpolated_time(
                    query,
                    snapshot.phase.segment_end,
                    snapshot.playback,
                    0
                ),
            Ok(nanos)
        );
    }
}

#[test]
fn compound_actual_repeated_pause_resume_retains_historical_mapping_and_exact_transition() {
    let (mut producer, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(5).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    playback_publish(&telemetry, &mut state, 4);
    producer.request_pause(true);
    playback_publish(&telemetry, &mut state, 4);
    playback_publish(&telemetry, &mut state, 4);
    producer.request_pause(false);
    playback_publish(&telemetry, &mut state, 4);
    producer.request_pause(true);
    playback_publish(&telemetry, &mut state, 4);
    producer.request_pause(false);
    playback_publish(&telemetry, &mut state, 4);
    for (frame, nanos) in [
        (4, 4_000_000),
        (5, 5_000_000),
        (12, 5_000_000),
        (13, 5_000_000),
        (16, 8_000_000),
        (17, 9_000_000),
        (21, 9_000_000),
        (24, 12_000_000),
    ] {
        let query = interpolated_exact(TargetTime::from_frames(frame, 1000).unwrap());
        let snapshot = telemetry
            .playback_phase_snapshot_at_interpolated_time(&query)
            .unwrap();
        assert_eq!(
            snapshot
                .phase
                .span
                .playback_elapsed_nanos_at_interpolated_time(
                    query,
                    snapshot.phase.segment_end,
                    snapshot.playback,
                    0
                ),
            Ok(nanos)
        );
    }
    for (offset, expected) in [
        (-1, SourcePlaybackState::Running),
        (0, SourcePlaybackState::Frozen),
        (1, SourcePlaybackState::Frozen),
    ] {
        let query = InterpolatedTargetTime::new(
            TargetTime::from_frames(5, 1000).unwrap(),
            0,
            1,
            u64::MAX,
            offset,
            u64::MAX - 1,
        )
        .unwrap();
        assert_eq!(
            telemetry
                .playback_phase_snapshot_at_interpolated_time(&query)
                .unwrap()
                .playback
                .state(),
            expected
        );
    }
}

#[test]
fn compound_requires_both_opt_ins_actual_source_and_positive_submission() {
    for (phase_enabled, source_enabled) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let (_, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
        if phase_enabled {
            state.enable_phase_history(8).unwrap();
        }
        if source_enabled {
            state.enable_source_playback_history(8).unwrap();
        }
        let telemetry =
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
        let zero = interpolated_exact(TargetTime::from_frames(0, 1000).unwrap());
        assert!(telemetry
            .playback_phase_snapshot_at_interpolated_time(&zero)
            .is_none());
        let report = state.render_pending(8).unwrap();
        telemetry.publish_generated(Some(report), &state);
        assert!(telemetry
            .playback_phase_snapshot_at_interpolated_time(&zero)
            .is_none());
        state.admit(2).unwrap();
        telemetry.publish_submitted(state.admitted_target_time().unwrap());
        for frame in [0, 2, 3, 8, 9] {
            let query = interpolated_exact(TargetTime::from_frames(frame, 1000).unwrap());
            let (snapshot, calls) = crate::audio::count_heap_calls(|| {
                telemetry.playback_phase_snapshot_at_interpolated_time(&query)
            });
            assert_eq!(calls, 0);
            assert_eq!(
                snapshot.is_some(),
                phase_enabled && source_enabled && frame <= 2
            );
            assert_eq!(
                telemetry
                    .phase_snapshot_at_interpolated_time(&query)
                    .is_some(),
                phase_enabled && frame <= 2
            );
        }
    }
}

#[test]
fn compound_new_stream_needs_fresh_submission_and_refuses_prior_lower_range() {
    let (_, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let query = interpolated_exact(TargetTime::from_frames(2, 1000).unwrap());
    assert!(telemetry
        .playback_phase_snapshot_at_interpolated_time(&query)
        .is_none());
    state.admit(2).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    for frame in [1, 2, 4, 5] {
        let q = interpolated_exact(TargetTime::from_frames(frame, 1000).unwrap());
        assert_eq!(
            telemetry
                .playback_phase_snapshot_at_interpolated_time(&q)
                .is_some(),
            (2..=4).contains(&frame)
        );
    }
}

#[test]
fn compound_target_only_held_grows_target_frontier_without_source_adoption() {
    let (mut producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    playback_publish(&telemetry, &mut state, 4);
    let before = state
        .converter_owner()
        .source_playback_history()
        .unwrap()
        .pulled_end_frame();
    producer.request_pause(true);
    let held = state.render_held_pending(4).unwrap();
    telemetry.publish_generated(Some(held), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let q = interpolated_exact(phase_time(7));
    let snap = telemetry
        .playback_phase_snapshot_at_interpolated_time(&q)
        .unwrap();
    assert_eq!(snap.phase.span.to_words()[7], 1);
    assert_eq!(snap.playback.state(), SourcePlaybackState::Running);
    assert_eq!(snap.pulled_source_end, before);
    assert_eq!(
        state
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        snap.phase.span.playback_elapsed_nanos_at_interpolated_time(
            q,
            snap.phase.segment_end,
            snap.playback,
            0
        ),
        Ok(83_333)
    );
}

#[test]
fn compound_source_corruption_fails_closed_without_disabling_old_phase_query() {
    let (_, mut state, _) = rig_boundaries(16, 1000, 1000, Some(2), Some(3));
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(3).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    playback_publish(&telemetry, &mut state, 8);
    let q = interpolated_exact(TargetTime::from_frames(3, 1000).unwrap());
    let source = telemetry.playback.as_ref().unwrap();
    let fields = [
        (&source.test_header()[0], 0),
        (&source.test_header()[0], u64::MAX),
        (&source.test_header()[1], 0),
        (&source.test_header()[1], 999),
        (&source.test_header()[2], 1),
        (&source.test_header()[3], 1),
        (&source.test_header()[4], 0),
        (&source.test_header()[4], 4),
        (&source.test_slots()[0][0], 1),
        (&source.test_slots()[0][1], 1),
        (&source.test_slots()[0][2], 2),
        (&source.test_slots()[1][0], 0),
        (&source.test_slots()[1][1], 1),
        (&source.test_slots()[1][2], 2),
        (&source.test_slots()[2][0], 2),
        (&source.test_slots()[2][0], u64::MAX),
        (&source.test_slots()[2][1], 2),
        (&source.test_slots()[2][2], 2),
    ];
    for (field, malformed) in fields {
        let old = field.load(Ordering::SeqCst);
        field.store(malformed, Ordering::SeqCst);
        let (snapshot, calls) = crate::audio::count_heap_calls(|| {
            telemetry.playback_phase_snapshot_at_interpolated_time(&q)
        });
        assert!(snapshot.is_none());
        assert_eq!(calls, 0);
        assert!(telemetry.phase_snapshot_at_interpolated_time(&q).is_some());
        field.store(old, Ordering::SeqCst);
        assert!(telemetry
            .playback_phase_snapshot_at_interpolated_time(&q)
            .is_some());
    }
}

#[test]
fn compound_odd_and_exhausted_generations_refuse_without_heap() {
    let (_, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    playback_publish(&telemetry, &mut state, 4);
    let q = interpolated_exact(TargetTime::from_frames(2, 1000).unwrap());
    let generation = telemetry.generation.load(Ordering::SeqCst);
    telemetry.generation.store(generation + 1, Ordering::SeqCst);
    let (snapshot, calls) = crate::audio::count_heap_calls(|| {
        telemetry.playback_phase_snapshot_at_interpolated_time(&q)
    });
    assert!(snapshot.is_none());
    assert_eq!(calls, 0);
    telemetry.generation.store(u64::MAX - 1, Ordering::SeqCst);
    telemetry.publish_submitted(TargetTime::from_frames(4, 1000).unwrap());
    assert!(telemetry
        .playback_phase_snapshot_at_interpolated_time(&q)
        .is_none());
}

#[test]
fn compound_shared_reader_checks_generation_after_finish_and_retries_three_times() {
    let (_, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    playback_publish(&telemetry, &mut state, 4);
    let q = interpolated_exact(TargetTime::from_frames(2, 1000).unwrap());
    let attempts = std::cell::Cell::new(0);
    let (snapshot, calls) = crate::audio::count_heap_calls(|| {
        telemetry.phase_snapshot_with_comparator(
            |bound| q.compare_target_time(bound),
            |phase| {
                attempts.set(attempts.get() + 1);
                let generation = telemetry.generation.load(Ordering::SeqCst);
                telemetry.generation.store(generation + 2, Ordering::SeqCst);
                Some(phase)
            },
        )
    });
    assert!(snapshot.is_none());
    assert_eq!(calls, 0);
    assert_eq!(attempts.get(), 3);
}

#[test]
fn compound_concurrent_publication_never_mixes_report_phase_and_source_frontiers() {
    let (_, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(1).unwrap();
    let telemetry = Arc::new(
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap(),
    );
    let done = Arc::new(AtomicBool::new(false));
    let writer = telemetry.clone();
    let finished = done.clone();
    let worker = std::thread::spawn(move || {
        for _ in 0..64 {
            playback_publish(&writer, &mut state, 1);
        }
        finished.store(true, Ordering::Release);
    });
    let q = interpolated_exact(TargetTime::from_frames(0, 1000).unwrap());
    let mut reads = 0;
    while !done.load(Ordering::Acquire) || reads < 100 {
        if let Some(snapshot) = telemetry.playback_phase_snapshot_at_interpolated_time(&q) {
            let report = snapshot.phase.report.unwrap();
            assert_eq!(
                snapshot.pulled_source_end,
                report.pulled_source_frame_cursor
            );
            assert_eq!(snapshot.phase.generated_end, report.target_end_time);
            assert!(
                snapshot.phase.submitted_end == report.target_start_time
                    || snapshot.phase.submitted_end == report.target_end_time
            );
            assert_eq!(
                snapshot.playback,
                SourcePlaybackSpan::new(0, None, 0, SourcePlaybackState::Running).unwrap()
            );
            assert_eq!(snapshot.phase.revision % 2, 0);
            reads += 1;
        }
    }
    worker.join().unwrap();
}

#[test]
fn compound_zero_numerator_nonunit_cold_denominator_is_valid_source_association() {
    let (_, mut state, _) = rig_boundaries(8, 3, 7, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let initial = state.phase_history().unwrap().initial_source_position();
    assert_eq!((initial.numerator, initial.denominator), (0, 1));
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let (snapshot, calls) = crate::audio::count_heap_calls(|| {
        let report = playback_publish(&telemetry, &mut state, 4);
        let query = interpolated_exact(report.target_end_time);
        // The actual converter normalizes zero to 0/1. This equivalent
        // structural encoding keeps its exact actual coordinate unchanged
        // while checking the publication guard accepts zero with any valid
        // denominator. It fabricates no RenderReport or native evidence.
        let phase = telemetry.phase.as_ref().unwrap();
        phase.values[7].store(7, Ordering::SeqCst);
        phase.slots[0][3].store(7, Ordering::SeqCst);
        telemetry
            .playback_phase_snapshot_at_interpolated_time(&query)
            .unwrap()
    });
    assert_eq!(calls, 0);
    assert_eq!(snapshot.playback.physical_start(), 0);
    assert_eq!(snapshot.playback.playback_start(), 0);
    assert_eq!(snapshot.playback.state(), SourcePlaybackState::Running);
    assert_eq!(snapshot.phase.span.to_words()[2..4], [0, 7]);
    assert_eq!(
        snapshot.pulled_source_end,
        snapshot.phase.report.unwrap().pulled_source_frame_cursor
    );
}

#[test]
fn compound_concurrent_actual_pause_resume_appends_keep_literal_historical_mapping() {
    let (mut producer, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(64).unwrap();
    let telemetry = Arc::new(
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap(),
    );
    let done = Arc::new(AtomicBool::new(false));
    let writer = telemetry.clone();
    let finished = done.clone();
    let worker = std::thread::spawn(move || {
        for iteration in 0..64 {
            producer.request_pause(iteration % 2 == 1);
            playback_publish(&writer, &mut state, 1);
            std::thread::yield_now();
        }
        assert_eq!(
            state
                .converter_owner()
                .source_playback_history()
                .unwrap()
                .len(),
            64
        );
        finished.store(true, Ordering::Release);
    });
    // Literal actual-callback timeline: the first callback pulls [0,2)
    // Running, then each callback pulls one physical frame. Pauses adopt at
    // even frames >= 2; resumes adopt at odd frames >= 3. No native ACK is
    // inferred from these source transitions.
    let check = |frame: u64, snapshot: ConvertedPlaybackPhaseSnapshot| {
        let report = snapshot.phase.report.unwrap();
        assert_eq!(
            snapshot.pulled_source_end,
            report.pulled_source_frame_cursor
        );
        assert_eq!(snapshot.pulled_source_end, report.target_frame_cursor + 1);
        assert_eq!(snapshot.phase.generated_end, report.target_end_time);
        assert!(
            snapshot.phase.submitted_end == report.target_start_time
                || snapshot.phase.submitted_end == report.target_end_time
        );
        assert_eq!(snapshot.phase.revision % 2, 0);
        let expected_start = if frame == 1 { 0 } else { frame };
        let expected_playback = if frame == 1 { 0 } else { frame / 2 + 1 };
        let expected_state = if frame >= 2 && frame % 2 == 0 {
            SourcePlaybackState::Frozen
        } else {
            SourcePlaybackState::Running
        };
        assert_eq!(
            (
                snapshot.playback.physical_start(),
                snapshot.playback.playback_start(),
                snapshot.playback.state()
            ),
            (expected_start, expected_playback, expected_state)
        );
        let next = if frame == 1 { 2 } else { frame + 1 };
        assert_eq!(
            snapshot.playback.physical_end(),
            (next < snapshot.pulled_source_end).then_some(next)
        );
        let query = interpolated_exact(TargetTime::from_frames(frame, 1000).unwrap());
        let elapsed = if frame == 1 { 1 } else { frame / 2 + 1 };
        assert_eq!(
            snapshot
                .phase
                .span
                .playback_elapsed_nanos_at_interpolated_time(
                    query,
                    snapshot.phase.segment_end,
                    snapshot.playback,
                    0
                ),
            Ok(i128::from(elapsed) * 1_000_000)
        );
    };
    let mut reads = 0;
    let mut frame = 1;
    while !done.load(Ordering::Acquire) || reads < 100 {
        let query = interpolated_exact(TargetTime::from_frames(frame, 1000).unwrap());
        if let Some(snapshot) = telemetry.playback_phase_snapshot_at_interpolated_time(&query) {
            check(frame, snapshot);
            reads += 1;
        }
        frame = frame % 64 + 1;
    }
    worker.join().unwrap();
    for frame in 1..=64 {
        let query = interpolated_exact(TargetTime::from_frames(frame, 1000).unwrap());
        let (snapshot, calls) = crate::audio::count_heap_calls(|| {
            telemetry
                .playback_phase_snapshot_at_interpolated_time(&query)
                .unwrap()
        });
        assert_eq!(calls, 0);
        check(frame, snapshot);
    }
}

#[test]
fn compound_cold_allocation_refusal_retains_owner_and_retry_seeds_without_submission() {
    let (_, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let report = state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let before = state
        .converter_owner()
        .source_playback_history()
        .unwrap()
        .pulled_end_frame();
    let pcm = state.pending_samples().to_vec();
    // Phase allocation precedes optional source allocation; exercise every cold allocation refusal.
    for failure in 0..2 {
        let result = crate::audio::fail_allocation(failure, || {
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis())
        });
        assert!(matches!(result, Err(AudioError::AllocationFailed)));
        assert_eq!(state.pending_report(), Some(report));
        assert_eq!(state.pending_samples(), pcm);
        assert_eq!(
            state
                .converter_owner()
                .source_playback_history()
                .unwrap()
                .pulled_end_frame(),
            before
        );
    }
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let q = interpolated_exact(TargetTime::from_frames(2, 1000).unwrap());
    assert!(telemetry
        .playback_phase_snapshot_at_interpolated_time(&q)
        .is_none());
    state.admit(1).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    assert_eq!(
        telemetry
            .playback_phase_snapshot_at_interpolated_time(&q)
            .unwrap()
            .pulled_source_end,
        before
    );
}

#[test]
fn compound_none_helper_publishes_actual_source_only_lag_without_replacing_report_or_submission() {
    let (_, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let first = playback_publish(&telemetry, &mut state, 4);
    let source = telemetry.playback.as_ref().unwrap();
    let old_header: [u64; 5] =
        std::array::from_fn(|i| source.test_header()[i].load(Ordering::SeqCst));
    let second = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(second), &state);
    let before = telemetry.generation.load(Ordering::SeqCst);
    // Synthetic publication lag containing a genuine earlier source prefix.
    // The converter remains current. This proves the None helper contract,
    // not execution of a native postgeneration error branch.
    for (field, value) in source.test_header().iter().zip(old_header) {
        field.store(value, Ordering::SeqCst);
    }
    telemetry.publish_generated(None, &state);
    assert_eq!(telemetry.generation.load(Ordering::SeqCst), before + 2);
    let q = interpolated_exact(TargetTime::from_frames(2, 1000).unwrap());
    let snapshot = telemetry
        .playback_phase_snapshot_at_interpolated_time(&q)
        .unwrap();
    assert_eq!(
        snapshot.pulled_source_end,
        second.pulled_source_frame_cursor
    );
    assert_eq!(snapshot.phase.report, Some(second));
    assert_eq!(snapshot.phase.submitted_end, first.target_end_time);
    assert_eq!(snapshot.phase.generated_end, second.target_end_time);
    let after = telemetry.generation.load(Ordering::SeqCst);
    telemetry.publish_generated(None, &state);
    assert_eq!(telemetry.generation.load(Ordering::SeqCst), after);
}

#[test]
fn compound_cached_empty_source_callback_does_not_adopt_pending_pause() {
    let (mut producer, mut state, _) = rig_boundaries(8, 1000, 2000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(1).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let first = playback_publish(&telemetry, &mut state, 1);
    producer.request_pause(true);
    let second = playback_publish(&telemetry, &mut state, 1);
    assert_eq!(second.source.unwrap().frames, 0);
    assert_eq!(
        second.pulled_source_frame_cursor,
        first.pulled_source_frame_cursor
    );
    let q = interpolated_exact(TargetTime::from_frames(2, 2000).unwrap());
    let snapshot = telemetry
        .playback_phase_snapshot_at_interpolated_time(&q)
        .unwrap();
    assert_eq!(snapshot.playback.state(), SourcePlaybackState::Running);
    assert_eq!(snapshot.pulled_source_end, first.pulled_source_frame_cursor);
    assert_eq!(
        state
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn compound_refuses_fractional_source_above_pulled_frontier_and_noninteger_initial_association() {
    let (_, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    playback_publish(&telemetry, &mut state, 4);
    let source = telemetry.playback.as_ref().unwrap();
    let end = source.test_header()[4].load(Ordering::SeqCst);
    source.test_header()[4].store(2, Ordering::SeqCst);
    // Remove report association to isolate exact source frontier validation.
    let report_present = telemetry.values[0].load(Ordering::SeqCst);
    telemetry.values[0].store(0, Ordering::SeqCst);
    let exact = interpolated_exact(TargetTime::from_frames(2, 1000).unwrap());
    let above = InterpolatedTargetTime::new(
        TargetTime::from_frames(2, 1000).unwrap(),
        0,
        1,
        u64::MAX,
        1,
        u64::MAX - 1,
    )
    .unwrap();
    assert!(telemetry
        .playback_phase_snapshot_at_interpolated_time(&exact)
        .is_some());
    assert!(telemetry
        .playback_phase_snapshot_at_interpolated_time(&above)
        .is_none());
    assert!(telemetry
        .phase_snapshot_at_interpolated_time(&above)
        .is_some());
    source.test_header()[4].store(end, Ordering::SeqCst);
    telemetry.values[0].store(report_present, Ordering::SeqCst);
    let phase = telemetry.phase.as_ref().unwrap();
    let old_numerator = phase.values[6].load(Ordering::SeqCst);
    let old_denominator = phase.values[7].load(Ordering::SeqCst);
    let old_span_numerator = phase.slots[0][2].load(Ordering::SeqCst);
    let old_span_denominator = phase.slots[0][3].load(Ordering::SeqCst);
    phase.values[6].store(1, Ordering::SeqCst);
    phase.values[7].store(2, Ordering::SeqCst);
    phase.slots[0][2].store(1, Ordering::SeqCst);
    phase.slots[0][3].store(2, Ordering::SeqCst);
    assert!(telemetry
        .phase_snapshot_at_interpolated_time(&exact)
        .is_some());
    assert!(telemetry
        .playback_phase_snapshot_at_interpolated_time(&exact)
        .is_none());
    phase.values[6].store(old_numerator, Ordering::SeqCst);
    phase.values[7].store(old_denominator, Ordering::SeqCst);
    phase.slots[0][2].store(old_span_numerator, Ordering::SeqCst);
    phase.slots[0][3].store(old_span_denominator, Ordering::SeqCst);
}

#[test]
fn interpolated_submitted_lookup_refuses_subnanosecond_future_without_heap() {
    let (_producer, mut state, _) = rig_boundaries(8, 1, 1, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(1).unwrap();
    telemetry.publish_generated(Some(report), &state);
    state.admit(1).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let f = u64::MAX;
    // Mathematical extrapolation fixture, not evidence of a native F-Hz clock.
    let future = InterpolatedTargetTime::new(
        TargetTime::new(0, 0, 1).unwrap(),
        f - 1,
        f,
        f,
        i128::from(f),
        f - 1,
    )
    .unwrap();
    let origin = ClockPoint {
        domain: ClockDomainId(7),
        timestamp: Timestamp::ZERO,
    };
    assert_eq!(
        future.point(origin).unwrap().timestamp.as_nanos(),
        1_000_000_000
    );
    let end = TargetTime::from_frames(1, 1).unwrap();
    assert!(future.compare_target_time(end).is_gt());
    let (result, calls) =
        crate::audio::count_heap_calls(|| telemetry.phase_snapshot_at_interpolated_time(&future));
    assert!(result.is_none());
    assert_eq!(calls, 0);
    assert!(telemetry.phase_snapshot_at(end).is_some());
    assert!(telemetry
        .phase_snapshot_at_interpolated_time(&interpolated_exact(end))
        .is_some());
}

#[test]
fn interpolated_subnanosecond_transition_selects_actual_held_span() {
    let (_producer, mut state, _) = rig_boundaries(8, 1, 3, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let active = state.render_pending(1).unwrap();
    telemetry.publish_generated(Some(active), &state);
    state.admit(1).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let held = state.render_held_pending(1).unwrap();
    telemetry.publish_generated(Some(held), &state);
    state.admit(1).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let f = u64::MAX;
    let query = InterpolatedTargetTime::new(
        TargetTime::new(0, 0, 1).unwrap(),
        f / 3,
        f / 3 + 1,
        f,
        1,
        f - 1,
    )
    .unwrap();
    assert_eq!(
        query
            .point(ClockPoint {
                domain: ClockDomainId(7),
                timestamp: Timestamp::ZERO
            })
            .unwrap()
            .timestamp
            .as_nanos(),
        333_333_333
    );
    let boundary = TargetTime::from_frames(1, 3).unwrap();
    assert!(query.compare_target_time(boundary).is_gt());
    let (held_snapshot, calls) = crate::audio::count_heap_calls(|| {
        telemetry
            .phase_snapshot_at_interpolated_time(&query)
            .unwrap()
    });
    assert_eq!(calls, 0);
    assert_eq!(held_snapshot.span.to_words()[7], 1);
    assert_eq!(
        held_snapshot
            .span
            .elapsed_nanos_at_interpolated_time(query, held_snapshot.segment_end, 0),
        Ok(333_333_333)
    );
    assert_eq!(
        telemetry
            .phase_snapshot_at(TargetTime::new(0, 333_333_333, 1_000_000_000).unwrap())
            .unwrap()
            .span
            .to_words()[7],
        0
    );
    assert_eq!(
        telemetry
            .phase_snapshot_at_interpolated_time(&interpolated_exact(boundary))
            .unwrap()
            .span
            .to_words()[7],
        1
    );
}

#[test]
fn interpolated_signed_wide_queries_keep_actual_retarget_span_and_literal_elapsed() {
    // Real device formats retain their supported sample-rate bounds. Large
    // denominators belong to the mathematical HOST query, not a fake device.
    let (_producer, mut state, _) = rig_boundaries(16, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(1).unwrap();
    telemetry.publish_generated(Some(report), &state);
    state.admit(1).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    state
        .reconfigure(
            DeviceFormat::new(32_000, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            16,
        )
        .unwrap();
    for held in [false, true, false] {
        let report = if held {
            state.render_held_pending(1).unwrap()
        } else {
            state
                .render_pending(if state.phase_history().unwrap().len() == 2 {
                    16
                } else {
                    1
                })
                .unwrap()
        };
        telemetry.publish_generated(Some(report), &state);
        state.admit(report.target_frames).unwrap();
        telemetry.publish_submitted(state.admitted_target_time().unwrap());
    }
    let base = TargetTime::new(0, 1, 12_000).unwrap();
    for (offset, expected) in [
        (
            i128::from(u64::MAX),
            [52_083, 52_083, 52_084, 52_084, 52_084, 52_084, 52_084],
        ),
        (
            -i128::from(u64::MAX),
            [52_083, 52_083, 52_083, 52_083, 52_083, 52_083, 52_084],
        ),
    ] {
        for (index, nanos) in expected.into_iter().enumerate() {
            let left = 12_297_829_382 + index as u64 - 3;
            let query = InterpolatedTargetTime::new(
                base,
                left,
                left + 1,
                18_446_744_073_709_551_557,
                offset,
                9_223_372_036_854_775_815,
            )
            .unwrap();
            let (snapshot, calls) = crate::audio::count_heap_calls(|| {
                telemetry
                    .phase_snapshot_at_interpolated_time(&query)
                    .unwrap()
            });
            assert_eq!(calls, 0);
            assert_eq!(
                snapshot.span.to_words(),
                [44_100, 2, 19, 64, 0, 1, 12_000, 0]
            );
            assert_eq!(
                snapshot
                    .span
                    .elapsed_nanos_at_interpolated_time(query, snapshot.segment_end, 0),
                Ok(nanos)
            );
            assert_eq!(
                snapshot
                    .span
                    .elapsed_nanos_at_interpolated_time(query, snapshot.segment_end, 2),
                Ok(6_732)
            );
        }
    }
}

#[test]
fn interpolated_recovery_refuses_negative_and_prior_stream_ranges() {
    let (_producer, mut state, _) = rig_boundaries(8, 1, 1, None, None);
    state.enable_phase_history(8).unwrap();
    state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let negative =
        InterpolatedTargetTime::new(TargetTime::new(0, 0, 1).unwrap(), 0, 1, 1, -1, 1).unwrap();
    let before = interpolated_exact(TargetTime::from_frames(1, 1).unwrap());
    let boundary = interpolated_exact(TargetTime::from_frames(2, 1).unwrap());
    assert!(telemetry
        .phase_snapshot_at_interpolated_time(&boundary)
        .is_none());
    state.admit(2).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    assert!(telemetry
        .phase_snapshot_at_interpolated_time(&negative)
        .is_none());
    assert!(telemetry
        .phase_snapshot_at_interpolated_time(&before)
        .is_none());
    let snapshot = telemetry
        .phase_snapshot_at_interpolated_time(&boundary)
        .unwrap();
    assert_eq!(
        snapshot.stream_start,
        TargetTime::from_frames(2, 1).unwrap()
    );
    assert_eq!(
        snapshot.submitted_end,
        TargetTime::from_frames(4, 1).unwrap()
    );
}

#[test]
fn interpolated_partial_prefix_refuses_generated_future_and_empty_or_disabled_history() {
    for enabled in [false, true] {
        let (_producer, mut state, _) = rig_boundaries(8, 1, 1, None, None);
        if enabled {
            state.enable_phase_history(8).unwrap();
        }
        let telemetry =
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
        let zero = interpolated_exact(TargetTime::from_frames(0, 1).unwrap());
        assert!(telemetry
            .phase_snapshot_at_interpolated_time(&zero)
            .is_none());
        let report = state.render_pending(8).unwrap();
        telemetry.publish_generated(Some(report), &state);
        assert!(telemetry
            .phase_snapshot_at_interpolated_time(&zero)
            .is_none());
        state.admit(2).unwrap();
        telemetry.publish_submitted(state.admitted_target_time().unwrap());
        for frame in 0..=9 {
            let time = TargetTime::from_frames(frame, 1).unwrap();
            let query = interpolated_exact(time);
            let (snapshot, calls) = crate::audio::count_heap_calls(|| {
                telemetry.phase_snapshot_at_interpolated_time(&query)
            });
            assert_eq!(snapshot.is_some(), enabled && frame <= 2);
            assert_eq!(calls, 0);
            assert_eq!(
                snapshot.is_some(),
                telemetry.phase_snapshot_at(time).is_some()
            );
        }
    }
}

#[test]
fn interpolated_malformed_report_header_and_descriptor_matrix_fails_closed() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(report), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let query = interpolated_exact(phase_time(2));
    let phase = telemetry.phase.as_ref().unwrap();
    let fields = [
        (&telemetry.values[5], 0),
        (&telemetry.values[6], 0),
        (&telemetry.values[4], 2),
        (&telemetry.values[15], 0),
        (&telemetry.values[18], 0),
        (&telemetry.values[14], u64::MAX),
        (&telemetry.values[17], u64::MAX),
        (&telemetry.values[6], 24_000),
        (&phase.slots[0][0], 0),
        (&phase.slots[0][3], 0),
        (&phase.slots[0][7], 2),
        (&phase.values[0], u64::MAX),
        (&phase.values[10], 0),
    ];
    for (field, malformed) in fields {
        let original = field.load(Ordering::SeqCst);
        field.store(malformed, Ordering::SeqCst);
        let (snapshot, calls) = crate::audio::count_heap_calls(|| {
            telemetry.phase_snapshot_at_interpolated_time(&query)
        });
        assert!(snapshot.is_none());
        assert_eq!(calls, 0);
        field.store(original, Ordering::SeqCst);
        assert!(telemetry
            .phase_snapshot_at_interpolated_time(&query)
            .is_some());
    }
}

#[test]
fn interpolated_odd_and_exhausted_generation_are_unavailable_without_heap() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(report), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let query = interpolated_exact(phase_time(2));
    let generation = telemetry.generation.load(Ordering::SeqCst);
    telemetry.generation.store(generation + 1, Ordering::SeqCst);
    let (snapshot, calls) =
        crate::audio::count_heap_calls(|| telemetry.phase_snapshot_at_interpolated_time(&query));
    assert!(snapshot.is_none());
    assert_eq!(calls, 0);
    telemetry.generation.store(u64::MAX - 1, Ordering::SeqCst);
    telemetry.publish_submitted(phase_time(4));
    assert!(telemetry
        .phase_snapshot_at_interpolated_time(&query)
        .is_none());
    assert_eq!(telemetry.generation.load(Ordering::SeqCst), u64::MAX);
}

#[test]
fn interpolated_shared_reader_retries_only_three_changing_generations_without_heap() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(report), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    // An out-of-range query ends each attempt at the first comparison, so
    // one real generation change per attempt is deterministic.
    let query =
        InterpolatedTargetTime::new(TargetTime::new(0, 0, 1).unwrap(), 0, 1, 1, -1, 1).unwrap();
    let initial = telemetry.generation.load(Ordering::SeqCst);
    let attempts = std::cell::Cell::new(0);
    let (snapshot, calls) = crate::audio::count_heap_calls(|| {
        telemetry.phase_snapshot_with_comparator(
            |bound| {
                let generation = telemetry.generation.load(Ordering::SeqCst);
                attempts.set(attempts.get() + 1);
                telemetry.generation.store(generation + 2, Ordering::SeqCst);
                query.compare_target_time(bound)
            },
            Some,
        )
    });
    assert!(snapshot.is_none());
    assert_eq!(attempts.get(), 3);
    assert_eq!(telemetry.generation.load(Ordering::SeqCst), initial + 6);
    assert_eq!(calls, 0);
    assert!(telemetry
        .phase_snapshot_at_interpolated_time(&interpolated_exact(phase_time(2)))
        .is_some());
}

#[test]
fn interpolated_concurrent_reader_keeps_report_submitted_prefix_and_revision_coherent() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry = Arc::new(
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap(),
    );
    let done = Arc::new(AtomicBool::new(false));
    let writer = telemetry.clone();
    let finished = done.clone();
    let worker = std::thread::spawn(move || {
        for _ in 0..64 {
            let report = state.render_pending(1).unwrap();
            writer.publish_generated(Some(report), &state);
            state.admit(1).unwrap();
            writer.publish_submitted(state.admitted_target_time().unwrap());
        }
        finished.store(true, Ordering::Release);
    });
    let query = interpolated_exact(phase_time(0));
    let mut reads = 0;
    while !done.load(Ordering::Acquire) || reads < 100 {
        if let Some(snapshot) = telemetry.phase_snapshot_at_interpolated_time(&query) {
            let report = snapshot.report.unwrap();
            let end = report
                .target_start_time
                .checked_add_frames(1, 48_000)
                .unwrap();
            assert_eq!(snapshot.generated_end, end);
            assert_eq!(snapshot.segment_end, end);
            assert!(
                snapshot.submitted_end == report.target_start_time || snapshot.submitted_end == end
            );
            assert_eq!(snapshot.span.to_words(), [44_100, 0, 0, 1, 0, 0, 1, 0]);
            assert_eq!(snapshot.boundaries.source_rate, 44_100);
            assert!(snapshot.last_real_source.unwrap().frames > 0);
            assert_eq!(snapshot.revision % 2, 0);
            reads += 1;
        }
    }
    worker.join().unwrap();
    assert_eq!(
        telemetry
            .phase_snapshot_at_interpolated_time(&interpolated_exact(phase_time(64)))
            .unwrap()
            .submitted_end,
        phase_time(64)
    );
}

#[test]
fn opted_snapshot_refuses_malformed_associated_report_atomically() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(report), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    for (index, malformed) in [
        (5, 0),
        (6, 0),
        (4, 2),
        (15, 0),
        (18, 0),
        (14, u64::MAX),
        (17, u64::MAX),
        (6, 24_000),
    ] {
        let original = telemetry.values[index].load(Ordering::SeqCst);
        telemetry.values[index].store(malformed, Ordering::SeqCst);
        let (snapshot, allocations) =
            crate::audio::count_heap_calls(|| telemetry.phase_snapshot_at(phase_time(2)));
        assert!(
            snapshot.is_none(),
            "malformed associated report field {index} must fail closed"
        );
        assert_eq!(allocations, 0);
        telemetry.values[index].store(original, Ordering::SeqCst);
        assert!(telemetry.phase_snapshot_at(phase_time(2)).is_some());
    }
}

#[test]
fn omitted_report_publication_preserves_prior_facts_but_tracks_actual_advanced_history() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let prior = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(prior), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let before = telemetry.phase_snapshot_at(phase_time(2)).unwrap();
    let actual_next = state.render_pending(4).unwrap();
    assert_eq!(actual_next.target_start_time, phase_time(4));
    // This exercises the publication helper's None contract with actual
    // generated history. It does not fabricate a postgeneration pump error.
    telemetry.publish_generated(None, &state);
    let after = telemetry.phase_snapshot_at(phase_time(2)).unwrap();
    assert_eq!(after.generated_end, phase_time(8));
    assert_eq!(after.segment_end, phase_time(8));
    assert_eq!(after.submitted_end, phase_time(4));
    assert_eq!(after.report, Some(prior));
    assert_eq!(after.boundaries, before.boundaries);
    assert_eq!(after.last_real_source, before.last_real_source);
    assert_eq!(after.span.to_words(), before.span.to_words());
    assert!(telemetry.phase_snapshot_at(phase_time(5)).is_none());
}

#[test]
fn cold_phase_publication_allocation_refusal_preserves_complete_owner_and_allows_retry() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let report = state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let basis = state.target_frame_basis();
    let pcm = state.pending_samples().to_vec();
    let counters = state.mixer().counters();
    let cursor = state.mixer().frame_cursor();
    let phase = state.converter_owner().source_position();
    let history_end = state.phase_history().unwrap().end_time();
    let descriptor = state.phase_history().unwrap().span(0).unwrap().to_words();
    let facts = state.boundaries();
    let source_report = state.last_real_source_report();
    let refused =
        crate::audio::fail_allocation(0, || ConvertedTelemetry::with_phase_history(&state, basis));
    assert!(matches!(refused, Err(AudioError::AllocationFailed)));
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.pending_frames(), 6);
    assert_eq!(state.admitted_target_time(), Some(phase_time(2)));
    assert_eq!(state.target_frame_basis(), basis);
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(state.mixer().frame_cursor(), cursor);
    assert_eq!(state.converter_owner().source_position(), phase);
    assert_eq!(state.phase_history().unwrap().end_time(), history_end);
    assert_eq!(
        state.phase_history().unwrap().span(0).unwrap().to_words(),
        descriptor
    );
    assert_eq!(state.boundaries(), facts);
    assert_eq!(state.last_real_source_report(), source_report);
    let retry = ConvertedTelemetry::with_phase_history(&state, basis).unwrap();
    assert!(retry.phase_snapshot_at(phase_time(2)).is_none());
    state.admit(1).unwrap();
    retry.publish_submitted(state.admitted_target_time().unwrap());
    let snapshot = retry.phase_snapshot_at(phase_time(3)).unwrap();
    assert_eq!(snapshot.generated_end, history_end);
    assert_eq!(snapshot.submitted_end, phase_time(3));
    assert_eq!(snapshot.span.to_words(), descriptor);
}

#[test]
fn optional_phase_seed_is_disabled_by_default_and_empty_seed_has_no_fake_span() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    let disabled =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(8).unwrap();
    disabled.publish_generated(Some(report), &state);
    state.admit(8).unwrap();
    disabled.publish_submitted(state.admitted_target_time().unwrap());
    assert!(disabled.phase_snapshot_at(phase_time(1)).is_none());
    assert_eq!(disabled.read().unwrap().0, report);

    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let empty = ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    assert!(empty.phase_snapshot_at(phase_time(0)).is_none());
    assert_eq!(empty.read(), None);
    assert_eq!(state.phase_history().unwrap().len(), 0);
}

#[test]
fn phase_generated_frontier_does_not_authenticate_unsubmitted_target_frames() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(8).unwrap();
    telemetry.publish_generated(Some(report), &state);
    assert!(telemetry.phase_snapshot_at(phase_time(0)).is_none());
    assert!(telemetry.phase_snapshot_at(phase_time(8)).is_none());
    state.admit(2).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    for frame in 0..=2 {
        let snapshot = telemetry.phase_snapshot_at(phase_time(frame)).unwrap();
        assert_eq!(snapshot.generated_end, phase_time(8));
        assert_eq!(snapshot.segment_end, phase_time(8));
        assert_eq!(snapshot.submitted_end, phase_time(2));
        assert_eq!(
            snapshot
                .span
                .point(phase_time(frame), snapshot.segment_end)
                .unwrap()
                .elapsed_nanos_since(0)
                .unwrap(),
            i128::from(frame) * 1_000_000_000 / 48_000
        );
    }
    assert!(telemetry.phase_snapshot_at(phase_time(3)).is_none());
    assert!(telemetry.phase_snapshot_at(phase_time(9)).is_none());
}

#[test]
fn recovered_history_is_seeded_but_prior_owner_submission_is_not_new_stream_proof() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let basis = state.target_frame_basis();
    let telemetry = ConvertedTelemetry::with_phase_history(&state, basis).unwrap();
    assert_eq!(state.admitted_target_time(), Some(phase_time(2)));
    for frame in 0..=8 {
        assert!(telemetry.phase_snapshot_at(phase_time(frame)).is_none());
    }
    state.admit(2).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    assert!(telemetry.phase_snapshot_at(phase_time(1)).is_none());
    let snapshot = telemetry.phase_snapshot_at(phase_time(2)).unwrap();
    assert_eq!(snapshot.stream_start, phase_time(2));
    assert_eq!(snapshot.submitted_end, phase_time(4));
    assert_eq!(snapshot.generated_end, phase_time(8));
    assert!(telemetry.phase_snapshot_at(phase_time(5)).is_none());
}

#[test]
fn phase_exact_fraction_survives_held_output_and_target_rate_change_without_heap() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let active = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(active), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let held = state.render_held_pending(2).unwrap();
    telemetry.publish_generated(Some(held), &state);
    state.admit(2).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let at_hold = telemetry.phase_snapshot_at(phase_time(5)).unwrap();
    assert_eq!(at_hold.span.to_words()[1..4], [3, 27, 40]);
    assert_eq!(at_hold.span.to_words()[7], 1);
    assert_eq!(
        at_hold
            .span
            .point(phase_time(5), at_hold.segment_end)
            .unwrap()
            .elapsed_nanos_since(3)
            .unwrap(),
        15_306
    );
    let format = DeviceFormat::new(32_000, 1, SampleEncoding::Float32, None).unwrap();
    state
        .reconfigure(format, ChannelMatrix::default_mix(1, 1).unwrap(), 8)
        .unwrap();
    let (snapshot, allocations) = crate::audio::count_heap_calls(|| {
        let active = state.render_pending(4).unwrap();
        telemetry.publish_generated(Some(active), &state);
        state.admit(4).unwrap();
        telemetry.publish_submitted(state.admitted_target_time().unwrap());
        let end = TargetTime::from_frames(1, 4_000).unwrap();
        telemetry.phase_snapshot_at(end).unwrap()
    });
    assert_eq!(allocations, 0);
    assert_eq!(snapshot.span.to_words()[7], 0);
    assert_eq!(
        snapshot
            .span
            .point(snapshot.submitted_end, snapshot.segment_end)
            .unwrap()
            .elapsed_nanos_since(0)
            .unwrap(),
        208_333
    );
    assert_eq!(state.phase_history().unwrap().len(), 3);
}

#[test]
fn phase_capacity_refusal_preserves_source_pcm_and_last_published_frontiers() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(1).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let active = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(active), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let before = telemetry.phase_snapshot_at(phase_time(4)).unwrap();
    let source = state.mixer().frame_cursor();
    assert!(state.render_held_pending(1).is_err());
    telemetry.publish_generated(None, &state);
    let after = telemetry.phase_snapshot_at(phase_time(4)).unwrap();
    assert_eq!(after.generated_end, before.generated_end);
    assert_eq!(after.submitted_end, before.submitted_end);
    assert_eq!(after.span.to_words(), before.span.to_words());
    assert_eq!(after.report, before.report);
    assert_eq!(state.mixer().frame_cursor(), source);
    assert_eq!(state.pending_frames(), 0);
}

#[test]
fn phase_odd_and_exhausted_generation_are_bounded_and_unavailable() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(report), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let generation = telemetry.generation.load(Ordering::SeqCst);
    telemetry.generation.store(generation + 1, Ordering::SeqCst);
    let (snapshot, allocations) =
        crate::audio::count_heap_calls(|| telemetry.phase_snapshot_at(phase_time(2)));
    assert!(snapshot.is_none());
    assert_eq!(allocations, 0);
    telemetry.generation.store(u64::MAX - 1, Ordering::SeqCst);
    telemetry.publish_submitted(phase_time(4));
    assert!(telemetry.phase_snapshot_at(phase_time(2)).is_none());
    assert_eq!(telemetry.generation.load(Ordering::SeqCst), u64::MAX);
}

#[test]
fn ordinary_merged_generation_updates_frontier_without_recopying_prior_slots() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let report = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(report), &state);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    let phase = telemetry.phase.as_ref().unwrap();
    let original = phase.slots[0][1].load(Ordering::SeqCst);
    phase.slots[0][1].store(original + 99, Ordering::SeqCst);
    let report = state.render_pending(4).unwrap();
    telemetry.publish_generated(Some(report), &state);
    assert_eq!(
        phase.slots[0][1].load(Ordering::SeqCst),
        original + 99,
        "merged ordinary render must not recopy the complete history"
    );
    phase.slots[0][1].store(original, Ordering::SeqCst);
    assert_eq!(state.phase_history().unwrap().len(), 1);
    state.admit(4).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    assert_eq!(
        telemetry
            .phase_snapshot_at(phase_time(8))
            .unwrap()
            .generated_end,
        phase_time(8)
    );
}

#[test]
fn phase_malformed_atomic_descriptor_header_and_bounds_fail_closed() {
    for malformed in 0..5 {
        let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
        state.enable_phase_history(8).unwrap();
        let telemetry =
            ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
        let report = state.render_pending(4).unwrap();
        telemetry.publish_generated(Some(report), &state);
        state.admit(4).unwrap();
        telemetry.publish_submitted(state.admitted_target_time().unwrap());
        let phase = telemetry.phase.as_ref().unwrap();
        match malformed {
            0 => phase.slots[0][0].store(0, Ordering::SeqCst),
            1 => phase.slots[0][3].store(0, Ordering::SeqCst),
            2 => phase.slots[0][7].store(2, Ordering::SeqCst),
            3 => phase.values[0].store(u64::MAX, Ordering::SeqCst),
            _ => phase.values[10].store(0, Ordering::SeqCst),
        }
        let (snapshot, allocations) =
            crate::audio::count_heap_calls(|| telemetry.phase_snapshot_at(phase_time(2)));
        assert!(
            snapshot.is_none(),
            "malformed atomic transfer must not qualify a phase"
        );
        assert_eq!(allocations, 0);
    }
}

#[test]
fn concurrent_phase_readers_never_mix_report_and_submitted_frontiers() {
    let (_producer, mut state, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    state.enable_phase_history(8).unwrap();
    let telemetry = Arc::new(
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap(),
    );
    let done = Arc::new(AtomicBool::new(false));
    let writer = telemetry.clone();
    let finished = done.clone();
    let worker = std::thread::spawn(move || {
        for _ in 0..64 {
            let report = state.render_pending(1).unwrap();
            writer.publish_generated(Some(report), &state);
            state.admit(1).unwrap();
            writer.publish_submitted(state.admitted_target_time().unwrap());
        }
        finished.store(true, Ordering::Release);
    });
    let mut reads = 0;
    while !done.load(Ordering::Acquire) || reads < 100 {
        if let Some(snapshot) = telemetry.phase_snapshot_at(phase_time(0)) {
            let report = snapshot.report.unwrap();
            let end = report
                .target_start_time
                .checked_add_frames(1, 48_000)
                .unwrap();
            assert_eq!(snapshot.generated_end, end);
            assert_eq!(snapshot.segment_end, end);
            assert!(
                snapshot.submitted_end == report.target_start_time || snapshot.submitted_end == end
            );
            assert_eq!(snapshot.boundaries.source_rate, 44_100);
            assert!(snapshot.last_real_source.unwrap().frames > 0);
            assert_eq!(snapshot.span.to_words(), [44_100, 0, 0, 1, 0, 0, 1, 0]);
            assert_eq!(snapshot.revision % 2, 0);
            reads += 1;
        }
    }
    worker.join().unwrap();
    assert_eq!(
        telemetry
            .phase_snapshot_at(phase_time(64))
            .unwrap()
            .submitted_end,
        phase_time(64)
    );
}

#[test]
fn coherent_seed_exposes_actual_facts_without_fabricating_a_converted_or_source_report() {
    let (_producer, mut owner, _) = rig_boundaries(16, 44_100, 48_000, Some(3), Some(7));
    let telemetry = ConvertedTelemetry::new();
    assert_eq!(telemetry.output_telemetry(), None);
    let initial = owner.boundaries();
    assert_eq!(initial.source_rate, 44_100);
    assert!(initial.origin.is_some());
    telemetry.seed(initial, None);
    assert_eq!(telemetry.read(), None);
    assert_eq!(telemetry.output_telemetry(), Some((None, None, initial)));
    owner.render_pending(16).unwrap();
    let facts = owner.boundaries();
    let actual = owner.last_real_source_report();
    assert!(actual.unwrap().frames > 0);
    assert!(facts.startup.is_some() && facts.end.is_some());
    telemetry.seed(facts, actual);
    assert_eq!(telemetry.read(), None);
    assert_eq!(telemetry.output_telemetry(), Some((actual, None, facts)));
}

#[test]
fn coherent_telemetry_odd_generation_returns_unavailable_without_waiting_or_default_facts() {
    let (_producer, mut owner, _) = rig_boundaries(16, 44_100, 48_000, Some(3), Some(7));
    let report = owner.render_pending(16).unwrap();
    let facts = owner.boundaries();
    let source = owner.last_real_source_report();
    let telemetry = ConvertedTelemetry::new();
    telemetry.publish(report, facts, source);
    let version = telemetry.generation.load(Ordering::SeqCst);
    telemetry.generation.store(version + 1, Ordering::SeqCst);
    let (unavailable, calls) = crate::audio::count_heap_calls(|| telemetry.output_telemetry());
    assert_eq!(unavailable, None);
    assert_eq!(calls, 0);
    telemetry.generation.store(version, Ordering::SeqCst);
    assert_eq!(
        telemetry.output_telemetry(),
        Some((source, Some(report), facts))
    );
}

#[test]
fn coherent_atomic_read_never_mixes_actual_end_generation_with_actual_pause_held_generation() {
    let (_producer, mut ended, _) = rig_boundaries(16, 44_100, 48_000, Some(3), Some(7));
    let end_report = ended.render_pending(16).unwrap();
    let end_facts = ended.boundaries();
    let end_source = ended.last_real_source_report();
    assert!(end_facts.end.is_some());
    let (mut producer, mut paused, _) = rig_boundaries(16, 24_000, 32_000, None, None);
    paused.render_pending(1).unwrap();
    paused.admit(1).unwrap();
    producer.request_pause(true);
    let pause_report = paused.render_pending(16).unwrap();
    paused.admit(16).unwrap();
    let held_report = paused.render_held_pending(3).unwrap();
    let pause_facts = paused.boundaries();
    let pause_source = paused.last_real_source_report();
    assert!(pause_facts.pause.is_some());
    assert!(pause_source.unwrap().paused);
    assert_eq!(held_report.source, None);
    let end_tuple = (end_source, Some(end_report), end_facts);
    let pause_tuple = (pause_source, Some(pause_report), pause_facts);
    let held_tuple = (pause_source, Some(held_report), pause_facts);
    assert_ne!(end_source, pause_source);
    assert_ne!(end_facts, pause_facts);
    let telemetry = Arc::new(ConvertedTelemetry::new());
    telemetry.publish(end_report, end_facts, end_source);
    let done = Arc::new(AtomicBool::new(false));
    let writer = telemetry.clone();
    let finished = done.clone();
    let worker = std::thread::spawn(move || {
        for _ in 0..2_000 {
            writer.publish(pause_report, pause_facts, pause_source);
            writer.publish(held_report, pause_facts, pause_source);
            writer.publish(end_report, end_facts, end_source);
        }
        finished.store(true, Ordering::Release);
    });
    let mut reads = 0;
    while !done.load(Ordering::Acquire) || reads < 100 {
        if let Some(tuple) = telemetry.output_telemetry() {
            assert!(
                tuple == end_tuple || tuple == pause_tuple || tuple == held_tuple,
                "source/report/boundary generations mixed"
            );
            reads += 1;
        }
    }
    worker.join().unwrap();
    assert_eq!(telemetry.output_telemetry(), Some(end_tuple));
}

#[test]
fn latest_held_report_retains_actual_mapped_start_and_exclusive_end_and_real_source_report() {
    let (_producer, mut owner, _) = rig_boundaries(16, 44_100, 48_000, Some(3), Some(7));
    let active = owner.render_pending(16).unwrap();
    let facts = owner.boundaries();
    let actual_source = owner.last_real_source_report();
    let startup = facts.startup.unwrap();
    let end = facts.end.unwrap();
    assert_eq!(
        (
            startup.source_frame,
            startup.target_frame_offset,
            startup.target_time
        ),
        (3, 4, TargetTime::from_frames(4, 48_000).unwrap())
    );
    assert_eq!(
        (end.source_frame, end.target_frame_offset, end.target_time),
        (10, 11, TargetTime::from_frames(11, 48_000).unwrap())
    );
    assert_eq!(
        facts.origin,
        Some(ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(-123)
        })
    );
    assert_eq!(facts.source_rate, 44_100);
    owner.admit(16).unwrap();
    let held = owner.render_held_pending(8).unwrap();
    assert_eq!(held.source, None);
    assert_eq!(owner.boundaries(), facts);
    assert_eq!(owner.last_real_source_report(), actual_source);
    let telemetry = ConvertedTelemetry::new();
    assert_eq!(telemetry.read(), None);
    let (_, calls) = crate::audio::count_heap_calls(|| {
        telemetry.publish(active, facts, actual_source);
        telemetry.publish(held, owner.boundaries(), owner.last_real_source_report());
    });
    assert_eq!(calls, 0);
    assert_eq!(telemetry.read(), Some((held, facts)));
    assert_eq!(telemetry.boundaries(), facts);
    assert_eq!(telemetry.last_real_source_report(), actual_source);
}

#[test]
fn cached_empty_and_held_reports_cannot_replace_prior_real_pause_adoption_or_mark_audible_prefix_paused(
) {
    let (mut producer, mut owner, _) = rig_boundaries(8, 24_000, 48_000, None, None);
    owner.render_pending(1).unwrap();
    owner.admit(1).unwrap();
    producer.request_pause(true);
    let paused = owner.render_pending(8).unwrap();
    assert!(paused.source.unwrap().paused);
    assert!(owner.pending_samples()[0] > 0.0);
    let facts = owner.boundaries();
    let pause = facts.pause.unwrap();
    assert_eq!(pause.source_frame, 2);
    assert_eq!(
        pause.target_time,
        TargetTime::from_frames(4, 48_000).unwrap()
    );
    assert!(
        owner
            .target_frame_basis()
            .point_at_stream_frame(0)
            .unwrap()
            .timestamp
            < pause
                .target_time
                .point(owner.target_frame_basis().origin())
                .unwrap()
                .timestamp
    );
    let actual = owner.last_real_source_report();
    assert!(actual.unwrap().frames > 0);
    owner.admit(8).unwrap();
    let held = owner.render_held_pending(3).unwrap();
    let telemetry = ConvertedTelemetry::new();
    telemetry.publish(paused, facts, actual);
    telemetry.publish(held, owner.boundaries(), owner.last_real_source_report());
    assert_eq!(telemetry.boundaries().pause, Some(pause));
    assert_eq!(telemetry.last_real_source_report(), actual);
    assert_eq!(telemetry.read().unwrap().0.source, None);
    owner.admit(3).unwrap();
    let empty = owner.render_pending(1).unwrap();
    if empty.source.unwrap().frames == 0 {
        assert_eq!(owner.last_real_source_report(), actual);
    }
    telemetry.publish(empty, owner.boundaries(), owner.last_real_source_report());
    assert_eq!(telemetry.boundaries().pause, Some(pause));
}

#[test]
fn original_native_counter_uses_target_rate_and_exact_piecewise_duration_not_source_pull_frontier()
{
    let (_producer, mut owner, _) = rig_boundaries(8, 44_100, 48_000, None, None);
    owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    let basis = owner.target_frame_basis();
    let native = ClockPoint {
        domain: ClockDomainId(8),
        timestamp: Timestamp::from_nanos(1_000_000_123),
    };
    let snapshot = super::super::AlsaTimingSnapshot {
        native_state: 3,
        submitted_frames: 7,
        delay_frames: 4,
        available_frames: 99,
        native_htstamp: super::super::AlsaNativeTimestamp {
            seconds: 1,
            nanoseconds: 123,
        },
        native_timestamp: Some(native),
        query_started: ClockPoint {
            domain: ClockDomainId(8),
            timestamp: Timestamp::from_nanos(2_000_000_000),
        },
        query_finished: ClockPoint {
            domain: ClockDomainId(8),
            timestamp: Timestamp::from_nanos(2_000_000_100),
        },
        estimated_played_frames: Some(3),
        quality: ClockMappingQuality::Unknown,
        timestamp_mode: 1,
        timestamp_type: 1,
    };
    let pair = crate::linux::alsa_presentation_pair_with_target_basis(snapshot, basis)
        .unwrap()
        .unwrap();
    assert_eq!(
        pair.source,
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(-123 + 5 * 1_000_000_000 / 48_000)
        }
    );
    assert_eq!(pair.target, native);
    assert_ne!(pair.target, snapshot.query_finished);
    let mut invalid = snapshot;
    invalid.estimated_played_frames = Some(4);
    assert!(crate::linux::alsa_presentation_pair_with_target_basis(invalid, basis).is_err());
    let mut missing = snapshot;
    missing.native_timestamp = None;
    assert_eq!(
        crate::linux::alsa_presentation_pair_with_target_basis(missing, basis).unwrap(),
        None
    );
    owner.admit(6).unwrap();
    owner
        .reconfigure(
            crate::audio::DeviceFormat::new(32_000, 1, crate::audio::SampleEncoding::Float32, None)
                .unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            8,
        )
        .unwrap();
    owner.render_held_pending(3).unwrap();
    owner.admit(1).unwrap();
    let reopened = owner.target_frame_basis();
    assert_eq!(reopened.sample_rate(), 32_000);
    let pair = crate::linux::alsa_presentation_pair_with_target_basis(snapshot, reopened)
        .unwrap()
        .unwrap();
    const DEN: u128 = 14_112_000;
    let ticks = 8 * (DEN / 48_000) + 4 * (DEN / 32_000);
    assert_eq!(
        pair.source.timestamp.as_nanos(),
        -123 + (ticks * 1_000_000_000 / DEN) as i64
    );
    assert_eq!(pair.target, native);
}

#[test]
fn concurrent_atomic_publication_keeps_complete_converted_report_boundary_and_source_snapshots() {
    let (_producer, mut owner, _) = rig_boundaries(16, 44_100, 48_000, Some(3), Some(7));
    let active = owner.render_pending(16).unwrap();
    let facts = owner.boundaries();
    let source = owner.last_real_source_report();
    owner.admit(16).unwrap();
    let held = owner.render_held_pending(8).unwrap();
    let telemetry = Arc::new(ConvertedTelemetry::new());
    telemetry.publish(active, facts, source);
    let done = Arc::new(AtomicBool::new(false));
    let writer = telemetry.clone();
    let finished = done.clone();
    let worker = std::thread::spawn(move || {
        for _ in 0..2_000 {
            writer.publish(held, facts, source);
            writer.publish(active, facts, source);
        }
        finished.store(true, Ordering::Release);
    });
    let mut reads = 0;
    while !done.load(Ordering::Acquire) || reads < 100 {
        if let Some((report, actual_facts)) = telemetry.read() {
            assert!(
                report == active || report == held,
                "torn target/source report"
            );
            assert_eq!(actual_facts, facts);
            reads += 1;
        }
    }
    worker.join().unwrap();
    assert_eq!(telemetry.last_real_source_report(), source);
}

#[test]
fn coalesced_held_and_cached_reports_retain_full_actual_resume_boundary_and_native_crossing_coordinate(
) {
    let (mut producer, mut owner, _) = rig_boundaries(16, 24_000, 48_000, None, None);
    owner.render_pending(1).unwrap();
    owner.admit(1).unwrap();
    producer.request_pause(true);
    owner.render_pending(8).unwrap();
    owner.admit(8).unwrap();
    producer.request_pause(false);
    let mut crossing = None;
    for _ in 0..12 {
        let report = owner.render_pending(1).unwrap();
        if report.resume_source_frame.is_some()
            && report.resume_boundary().unwrap().is_none()
            && owner.boundaries().resume.is_none()
        {
            assert!(report.resume_source_frame.unwrap() * 2 > report.target_frame_cursor);
        }
        owner.admit(1).unwrap();
        if owner.boundaries().resume.is_some() {
            crossing = Some(report);
            break;
        }
    }
    let crossing = crossing.expect("actual consumed target resume crossing");
    let facts = owner.boundaries();
    let resume = facts.resume.unwrap();
    let real = owner.last_real_source_report();
    assert_eq!(
        resume.target_time,
        TargetTime::from_frames(resume.source_frame * 2, 48_000).unwrap()
    );
    assert!(real.unwrap().frames > 0);
    assert!(!real.unwrap().paused);
    let held = owner.render_held_pending(3).unwrap();
    let telemetry = Arc::new(ConvertedTelemetry::new());
    telemetry.publish(crossing, facts, real);
    telemetry.publish(held, owner.boundaries(), owner.last_real_source_report());
    assert_eq!(telemetry.read(), Some((held, facts)));
    assert_eq!(telemetry.boundaries().resume, Some(resume));
    assert_eq!(telemetry.last_real_source_report(), real);
    // A consumer can wait for this exact target time; neither source adoption
    // nor the latest Held/source-none report substitutes for native crossing.
    let native_basis = TargetFrameBasis::new(
        facts.origin.unwrap(),
        TargetTime::new(0, 0, 1).unwrap(),
        48_000,
    )
    .unwrap();
    assert!(
        native_basis
            .point_at_stream_frame(resume.source_frame * 2 - 1)
            .unwrap()
            .timestamp
            < resume
                .target_time
                .point(facts.origin.unwrap())
                .unwrap()
                .timestamp
    );
    assert_eq!(
        native_basis
            .point_at_stream_frame(resume.source_frame * 2)
            .unwrap(),
        resume.target_time.point(facts.origin.unwrap()).unwrap()
    );
    let publisher = telemetry.clone();
    let thread = std::thread::spawn(move || {
        for _ in 0..200 {
            publisher.publish(crossing, facts, real);
            publisher.publish(held, facts, real);
        }
    });
    for _ in 0..200 {
        if let Some((report, actual)) = telemetry.read() {
            assert!(report == crossing || report == held);
            assert_eq!(actual.resume, Some(resume));
            assert_eq!(actual, facts);
        }
    }
    thread.join().unwrap();
    assert_eq!(telemetry.boundaries().resume, Some(resume));
}
