//! Generated source facts only; these fixtures make no native delivery or ACK claim.
use super::*;
use crate::audio::{count_heap_calls, fail_allocation};
use beatkernel::audio::*;
use std::sync::{atomic::AtomicBool, Arc, Barrier};

fn bare(hz: u32, gate: Option<u64>, end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(hz, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
    let (mut producer, consumer) = if gate.is_some() {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    if let Some(frame) = gate {
        producer.schedule_start_at(frame).unwrap();
    }
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(7),
        Timestamp::ZERO,
        AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
    );
    if let Some(frame) = end {
        config = config.with_playback_end_frame(frame);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn rig(capacity: usize, gate: Option<u64>, end: Option<u64>) -> (CommandProducer, Mixer) {
    let (producer, mut mixer) = bare(1000, gate, end);
    mixer.enable_source_playback_history(capacity).unwrap();
    (producer, mixer)
}
fn history(mixer: &Mixer) -> &SourcePlaybackHistory {
    mixer.source_playback_history().unwrap()
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 1000).unwrap(), 0, 0, 1, 0, 1)
        .unwrap()
}
fn read(
    telemetry: &Telemetry,
    mixer: &Mixer,
    query: &InterpolatedTargetTime,
) -> Option<SourcePlaybackSnapshot> {
    let h = history(mixer);
    telemetry.read_source_playback_at(
        query,
        h.source_rate(),
        h.initial_physical_frame(),
        h.initial_playback_frame(),
    )
}
fn scalar(render: RenderReport) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters {
            submitted_frames: render.start_frame + render.frames as u64,
            buffer_fills: 17,
            ..StreamCounters::default()
        },
        clock: None,
        render: Some(render),
    }
}
fn publish(telemetry: &Telemetry, mixer: &Mixer, render: RenderReport, version: &mut u64) {
    telemetry.publish_with_source_history(Some(scalar(render)), Some(history(mixer)), version);
}

#[test]
fn cold_empty_seed_allocation_failure_and_disabled_legacy_are_distinct() {
    let (_, mut mixer) = rig(3, None, None);
    assert!(matches!(
        fail_allocation(0, || Telemetry::with_source_history(history(&mixer))),
        Err(AudioError::AllocationFailed)
    ));
    let opted = Telemetry::with_source_history(history(&mixer)).unwrap();
    assert!(read(&opted, &mixer, &query(0)).is_none());
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    let legacy = Telemetry::new();
    let mut version = 0;
    legacy.publish(scalar(report), &mut version);
    assert_eq!(legacy.read(), scalar(report));
    assert!(read(&legacy, &mixer, &query(0)).is_none());
    assert_eq!(version, 2);
}

#[test]
fn actual_three_interval_callback_selects_new_boundaries_and_inclusive_pull_endpoint_without_heap()
{
    let (_, mut mixer) = rig(3, Some(2), Some(3));
    let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
    let mut version = 0;
    let report = mixer.render(&mut [0.0; 8]).unwrap();
    let ((), calls) = count_heap_calls(|| publish(&telemetry, &mixer, report, &mut version));
    assert_eq!(calls, 0);
    for (frame, start, playback, state, end) in [
        (0, 0, 0, SourcePlaybackState::Frozen, Some(2)),
        (2, 2, 0, SourcePlaybackState::Running, Some(5)),
        (4, 2, 0, SourcePlaybackState::Running, Some(5)),
        (5, 5, 3, SourcePlaybackState::Frozen, None),
        (8, 5, 3, SourcePlaybackState::Frozen, None),
    ] {
        let q = query(frame);
        let (selected, calls) = count_heap_calls(|| read(&telemetry, &mixer, &q));
        assert_eq!(calls, 0);
        let selected = selected.unwrap();
        assert_eq!(
            (
                selected.playback.physical_start(),
                selected.playback.playback_start(),
                selected.playback.state(),
                selected.playback.physical_end()
            ),
            (start, playback, state, end)
        );
        assert_eq!(selected.snapshot, scalar(report));
        assert_eq!((selected.pulled_source_end, selected.revision), (8, 2));
    }
    let future = InterpolatedTargetTime::new(
        TargetTime::new(0, 16_000_001, 2_000_000_000).unwrap(),
        0,
        0,
        1,
        0,
        1,
    )
    .unwrap();
    assert!(read(&telemetry, &mixer, &future).is_none());
    assert!(read(&telemetry, &mixer, &query(9)).is_none());
}

#[test]
fn actual_pause_resume_and_frozen_merge_keep_historical_intervals() {
    let (mut producer, mut mixer) = rig(5, None, None);
    let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
    let mut version = 0;
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    publish(&telemetry, &mixer, report, &mut version);
    producer.request_pause(true);
    let report = mixer.render(&mut [0.0; 3]).unwrap();
    publish(&telemetry, &mixer, report, &mut version);
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    publish(&telemetry, &mixer, report, &mut version);
    assert_eq!(history(&mixer).len(), 2);
    producer.request_pause(false);
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    publish(&telemetry, &mixer, report, &mut version);
    for (frame, start, playback, state) in [
        (3, 0, 0, SourcePlaybackState::Running),
        (4, 4, 4, SourcePlaybackState::Frozen),
        (8, 4, 4, SourcePlaybackState::Frozen),
        (9, 9, 4, SourcePlaybackState::Running),
        (13, 9, 4, SourcePlaybackState::Running),
    ] {
        let selected = read(&telemetry, &mixer, &query(frame)).unwrap();
        assert_eq!(
            (
                selected.playback.physical_start(),
                selected.playback.playback_start(),
                selected.playback.state()
            ),
            (start, playback, state)
        );
    }
}

#[test]
fn cold_advanced_then_moved_owner_uses_real_initial_identity() {
    let (_, mut mixer) = bare(1000, None, None);
    mixer.render(&mut [0.0; 4]).unwrap();
    mixer.enable_source_playback_history(3).unwrap();
    let mut moved = mixer;
    let report = moved.render(&mut [0.0; 3]).unwrap();
    let telemetry = Telemetry::with_source_history(history(&moved)).unwrap();
    let selected = read(&telemetry, &moved, &query(4)).unwrap();
    assert_eq!(
        (
            selected.playback.physical_start(),
            selected.playback.playback_start(),
            selected.pulled_source_end
        ),
        (4, 4, 7)
    );
    assert!(read(&telemetry, &moved, &query(3)).is_none());
    assert!(telemetry
        .read_source_playback_at(&query(4), 1000, 0, 0)
        .is_none());
    let mut version = 0;
    publish(&telemetry, &moved, report, &mut version);
    assert_eq!(read(&telemetry, &moved, &query(7)).unwrap().revision, 2);
}

#[test]
fn history_only_publication_preserves_literal_scalar_and_independent_status() {
    let (_, mut mixer) = rig(3, None, None);
    let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
    let mut version = 0;
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    publish(&telemetry, &mixer, report, &mut version);
    telemetry.status.store(
        status_code(AudioStreamStatus::WorkerPanicked),
        Ordering::SeqCst,
    );
    mixer.render(&mut [0.0; 2]).unwrap();
    let ((), calls) = count_heap_calls(|| {
        telemetry.publish_with_source_history(None, Some(history(&mixer)), &mut version)
    });
    assert_eq!(calls, 0);
    let selected = read(&telemetry, &mixer, &query(6)).unwrap();
    let mut expected = scalar(report);
    expected.status = AudioStreamStatus::WorkerPanicked;
    assert_eq!(selected.snapshot, expected);
    assert_eq!((selected.pulled_source_end, selected.revision), (6, 4));
    assert_eq!(telemetry.read(), expected);
}

#[test]
fn repeated_finite_frozen_report_end_can_precede_current_physical_block() {
    let (_, mut mixer) = rig(3, None, Some(2));
    let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
    let mut version = 0;
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    publish(&telemetry, &mixer, report, &mut version);
    let report = mixer.render(&mut [0.0; 3]).unwrap();
    assert_eq!(report.playback_end_physical_frame, Some(2));
    assert!(report.start_frame > 2);
    publish(&telemetry, &mixer, report, &mut version);
    assert_eq!(
        read(&telemetry, &mixer, &query(7)).unwrap().snapshot,
        scalar(report)
    );
}

#[test]
fn missing_history_and_old_publish_permanently_close_only_opted_reader() {
    for legacy_call in [false, true] {
        let (_, mut mixer) = rig(3, None, None);
        let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
        let mut version = 0;
        let report = mixer.render(&mut [0.0; 4]).unwrap();
        if legacy_call {
            telemetry.publish(scalar(report), &mut version);
        } else {
            telemetry.publish_with_source_history(Some(scalar(report)), None, &mut version);
        }
        assert_eq!(telemetry.read(), scalar(report));
        assert!(read(&telemetry, &mixer, &query(0)).is_none());
        publish(&telemetry, &mixer, report, &mut version);
        assert!(read(&telemetry, &mixer, &query(0)).is_none());
    }
}

#[test]
fn mismatched_rate_initial_identity_capacity_and_regressed_frontier_are_permanent() {
    for case in 0..4 {
        let (_, mut mixer) = rig(3, None, None);
        let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
        let mut version = 0;
        let report = mixer.render(&mut [0.0; 4]).unwrap();
        publish(&telemetry, &mixer, report, &mut version);
        let (_, mut other) = bare(if case == 0 { 2000 } else { 1000 }, None, None);
        if case == 1 {
            other.render(&mut [0.0; 2]).unwrap();
        }
        other
            .enable_source_playback_history(if case == 2 { 4 } else { 3 })
            .unwrap();
        if case != 3 {
            other.render(&mut [0.0; 4]).unwrap();
        }
        telemetry.publish_with_source_history(None, Some(history(&other)), &mut version);
        assert!(read(&telemetry, &mixer, &query(0)).is_none(), "case {case}");
        publish(&telemetry, &mixer, report, &mut version);
        assert!(
            read(&telemetry, &mixer, &query(0)).is_none(),
            "recovery case {case}"
        );
    }
}

#[test]
fn opted_reader_rejects_corrupt_flags_and_checked_report_extents_without_changing_legacy_decode() {
    for (index, value) in [
        (15, 2),
        (21, 2),
        (35, 2),
        (36, 2),
        (16, u64::MAX),
        (17, u64::MAX),
        (33, u64::MAX),
        (34, u64::MAX),
        (37, 5),
    ] {
        let (_, mut mixer) = rig(3, None, None);
        let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
        let mut version = 0;
        let report = mixer.render(&mut [0.0; 4]).unwrap();
        publish(&telemetry, &mixer, report, &mut version);
        if index == 37 {
            telemetry.values[36].store(1, Ordering::SeqCst);
        }
        telemetry.values[index].store(value, Ordering::SeqCst);
        assert!(
            read(&telemetry, &mixer, &query(0)).is_none(),
            "word {index}"
        );
        assert!(telemetry.read().telemetry_available);
    }
}

#[test]
fn clock_metadata_remains_legacy_decoded_and_is_not_native_authentication() {
    let (_, mut mixer) = rig(3, None, None);
    let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
    let mut version = 0;
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    publish(&telemetry, &mixer, report, &mut version);
    telemetry.values[7].store(9, Ordering::SeqCst);
    telemetry.values[9].store(0, Ordering::SeqCst);
    telemetry.values[14].store(2, Ordering::SeqCst);
    let selected = read(&telemetry, &mixer, &query(0)).unwrap();
    assert_eq!(selected.snapshot, telemetry.read());
    let clock = selected.snapshot.clock.unwrap();
    assert_eq!(clock.frequency, 0);
    assert_eq!(clock.reading_quality, AudioClockReadingQuality::Unknown);
}

#[test]
fn exactly_three_changed_generation_attempts_are_bounded_and_later_attempt_can_succeed() {
    let (_, mut mixer) = rig(3, None, None);
    let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
    let mut version = 0;
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    publish(&telemetry, &mixer, report, &mut version);
    let q = query(0);
    let mut attempts = 0;
    let selected = telemetry.read_source_playback_with(&q, 1000, 0, 0, |attempt| {
        assert_eq!(attempt, attempts);
        attempts += 1;
        publish(&telemetry, &mixer, report, &mut version);
    });
    assert!(selected.is_none());
    assert_eq!(attempts, 3);
    assert_eq!(version, 8);
    attempts = 0;
    let selected = telemetry
        .read_source_playback_with(&q, 1000, 0, 0, |attempt| {
            assert_eq!(attempt, attempts);
            attempts += 1;
            if attempt < 2 {
                publish(&telemetry, &mixer, report, &mut version);
            }
        })
        .unwrap();
    assert_eq!(attempts, 3);
    assert_eq!(selected.revision, 12);
}

#[test]
fn odd_and_checked_exhausted_revision_never_wrap_or_hide_independent_status() {
    let (_, mut mixer) = rig(3, None, None);
    let telemetry = Telemetry::with_source_history(history(&mixer)).unwrap();
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    let mut version = 0;
    publish(&telemetry, &mixer, report, &mut version);
    telemetry.version.store(3, Ordering::SeqCst);
    assert!(read(&telemetry, &mixer, &query(0)).is_none());
    version = u64::MAX - 1;
    let mut failed = scalar(report);
    failed.status = AudioStreamStatus::Failed { hresult: -7 };
    telemetry.publish_with_source_history(Some(failed), Some(history(&mixer)), &mut version);
    assert_eq!(telemetry.version.load(Ordering::SeqCst), u64::MAX);
    assert_eq!(version, u64::MAX - 1);
    assert!(read(&telemetry, &mixer, &query(0)).is_none());
    assert_eq!(telemetry.read().status, failed.status);
    assert!(!telemetry.read().telemetry_available);
}

#[test]
fn actual_concurrent_scalar_report_and_source_frontier_share_one_publication() {
    let (_, mut mixer) = rig(3, None, None);
    let telemetry = Arc::new(Telemetry::with_source_history(history(&mixer)).unwrap());
    let barrier = Arc::new(Barrier::new(2));
    let done = Arc::new(AtomicBool::new(false));
    let writer_telemetry = Arc::clone(&telemetry);
    let writer_barrier = Arc::clone(&barrier);
    let writer_done = Arc::clone(&done);
    let writer = std::thread::spawn(move || {
        let mut version = 0;
        writer_barrier.wait();
        for _ in 0..128 {
            let report = mixer.render(&mut [0.0; 1]).unwrap();
            publish(&writer_telemetry, &mixer, report, &mut version);
            std::thread::yield_now();
        }
        writer_done.store(true, Ordering::SeqCst);
    });
    barrier.wait();
    let q = query(0);
    while !done.load(Ordering::SeqCst) {
        if let Some(selected) = telemetry.read_source_playback_at(&q, 1000, 0, 0) {
            let report = selected.snapshot.render.unwrap();
            assert_eq!(
                selected.pulled_source_end,
                selected.snapshot.counters.submitted_frames
            );
            assert_eq!(
                selected.pulled_source_end,
                report.start_frame + report.frames as u64
            );
            assert_eq!(selected.revision, 2 * selected.pulled_source_end);
        }
    }
    writer.join().unwrap();
    let selected = telemetry.read_source_playback_at(&q, 1000, 0, 0).unwrap();
    assert_eq!(
        (
            selected.pulled_source_end,
            selected.snapshot.counters.submitted_frames,
            selected.revision
        ),
        (128, 128, 256)
    );
}
