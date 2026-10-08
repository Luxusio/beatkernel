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
