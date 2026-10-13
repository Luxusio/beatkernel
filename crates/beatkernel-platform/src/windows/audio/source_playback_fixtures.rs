//! Windows-only pure source-telemetry fixtures: no COM, QPC, or endpoint calls.
//! Failure scenarios below simulate scalar facts in the shared publisher; they
//! do not reproduce a native ReleaseBuffer or IAudioClock failure.
use super::*;
use crate::audio::{count_heap_calls, fail_allocation};
use beatkernel::{audio::*, time::ClockDomainId};

fn bare(gate: Option<u64>, end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 64, 1).unwrap()).unwrap();
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
        ClockDomainId(71),
        Timestamp::ZERO,
        AudioLimits::new(8, 4, 8, 64, 8).unwrap(),
    );
    if let Some(frame) = end {
        config = config.with_playback_end_frame(frame);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn rig(gate: Option<u64>, end: Option<u64>) -> (CommandProducer, Mixer) {
    let (producer, mut mixer) = bare(gate, end);
    mixer.enable_source_playback_history(8).unwrap();
    (producer, mixer)
}
fn history(mixer: &Mixer) -> &SourcePlaybackHistory {
    mixer.source_playback_history().unwrap()
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 1000).unwrap(), 0, 0, 1, 0, 1)
        .unwrap()
}
fn read(telemetry: &Telemetry, mixer: &Mixer, frame: u64) -> Option<SourcePlaybackSnapshot> {
    let h = history(mixer);
    telemetry.read_source_playback_at(
        &query(frame),
        h.source_rate(),
        h.initial_physical_frame(),
        h.initial_playback_frame(),
    )
}
fn scalar(report: Option<RenderReport>, submitted: u64) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters {
            submitted_frames: submitted,
            buffer_fills: 1,
            ..StreamCounters::default()
        },
        clock: None,
        render: report,
    }
}
fn publish(telemetry: &Telemetry, mixer: &Mixer, snapshot: AudioStreamSnapshot, version: &mut u64) {
    telemetry.publish_with_source_history(Some(snapshot), mixer.source_playback_history(), version);
}

#[test]
fn cold_helper_allocation_refusal_keeps_original_owner_basis_cursors_and_history() {
    let (mut producer, mut mixer) = rig(None, None);
    mixer.render(&mut [0.0; 3]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 2]).unwrap();
    let basis = mixer.output_frame_basis();
    let counters = mixer.counters();
    let spans = (history(&mixer).span(0), history(&mixer).span(1));
    assert!(matches!(
        fail_allocation(0, || source_telemetry(&mixer)),
        Err(AudioError::AllocationFailed)
    ));
    assert_eq!(mixer.output_frame_basis(), basis);
    assert_eq!(mixer.counters(), counters);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (5, 3)
    );
    assert_eq!((history(&mixer).span(0), history(&mixer).span(1)), spans);
    assert_eq!(history(&mixer).pulled_end_frame(), 5);
    assert!(source_telemetry(&mixer).is_ok());
}

#[test]
fn cold_helper_default_legacy_needs_no_allocation_or_source_evidence() {
    let (_, mut mixer) = bare(None, None);
    let telemetry = fail_allocation(0, || source_telemetry(&mixer)).unwrap();
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    let expected = scalar(Some(report), 4);
    let mut version = 0;
    publish(&telemetry, &mixer, expected, &mut version);
    assert_eq!(telemetry.read(), expected);
    assert!(telemetry
        .read_source_playback_at(&query(0), 1000, 0, 0)
        .is_none());
    assert!(mixer.source_playback_history().is_none());
}

#[test]
fn cold_seed_has_real_generated_intervals_but_never_fabricates_new_submission() {
    let (_, mut mixer) = rig(None, None);
    mixer.render(&mut [0.0; 4]).unwrap();
    let telemetry = source_telemetry(&mixer).unwrap();
    let selected = read(&telemetry, &mixer, 4).unwrap();
    assert_eq!(selected.pulled_source_end, 4);
    assert_eq!(selected.snapshot.counters.submitted_frames, 0);
    assert_eq!(selected.snapshot.status, AudioStreamStatus::Ready);
    assert!(selected.snapshot.render.is_none());
    assert!(selected.snapshot.clock.is_none());
    assert_eq!(selected.revision, 0);
    assert!(read(&telemetry, &mixer, 5).is_none());
}

#[test]
fn actual_gate_and_finite_tail_select_exact_boundaries_and_refuse_fractional_future() {
    let (_, mut mixer) = rig(Some(2), Some(3));
    let telemetry = source_telemetry(&mixer).unwrap();
    assert!(read(&telemetry, &mixer, 0).is_none());
    let report = mixer.render(&mut [0.0; 8]).unwrap();
    let expected = scalar(Some(report), 8);
    let mut version = 0;
    let ((), calls) = count_heap_calls(|| publish(&telemetry, &mixer, expected, &mut version));
    assert_eq!(calls, 0);
    for (frame, start, playback, state) in [
        (0, 0, 0, SourcePlaybackState::Frozen),
        (2, 2, 0, SourcePlaybackState::Running),
        (4, 2, 0, SourcePlaybackState::Running),
        (5, 5, 3, SourcePlaybackState::Frozen),
        (8, 5, 3, SourcePlaybackState::Frozen),
    ] {
        let (selected, calls) = count_heap_calls(|| read(&telemetry, &mixer, frame));
        assert_eq!(calls, 0);
        let selected = selected.unwrap();
        assert_eq!(
            (
                selected.playback.physical_start(),
                selected.playback.playback_start(),
                selected.playback.state()
            ),
            (start, playback, state)
        );
        assert_eq!(selected.snapshot, expected);
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
    assert!(telemetry
        .read_source_playback_at(&future, 1000, 0, 0)
        .is_none());
}

#[test]
fn actual_pause_resume_uses_history_identity_instead_of_reopened_frame_basis() {
    let (mut producer, mut mixer) = rig(None, None);
    mixer.render(&mut [0.0; 3]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 2]).unwrap();
    producer.request_pause(false);
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    let reopened_basis = mixer.output_frame_basis();
    assert_eq!(reopened_basis.start_physical_frame(), 9);
    assert_eq!(history(&mixer).initial_physical_frame(), 0);
    let telemetry = source_telemetry(&mixer).unwrap();
    let mut version = 0;
    // Fresh stream submission is relative to the reopened stream, not the old history.
    publish(&telemetry, &mixer, scalar(Some(report), 0), &mut version);
    for (frame, start, playback, state) in [
        (2, 0, 0, SourcePlaybackState::Running),
        (3, 3, 3, SourcePlaybackState::Frozen),
        (4, 3, 3, SourcePlaybackState::Frozen),
        (5, 5, 3, SourcePlaybackState::Running),
        (9, 5, 3, SourcePlaybackState::Running),
    ] {
        let selected = read(&telemetry, &mixer, frame).unwrap();
        assert_eq!(
            (
                selected.playback.physical_start(),
                selected.playback.playback_start(),
                selected.playback.state()
            ),
            (start, playback, state)
        );
    }
    assert!(telemetry
        .read_source_playback_at(&query(9), 1000, 9, 7)
        .is_none());
    assert!(telemetry
        .read_source_playback_at(&query(9), 2000, 0, 0)
        .is_none());
    assert!(telemetry
        .read_source_playback_at(&query(9), 1000, 0, 1)
        .is_none());
}

#[test]
fn simulated_post_generation_failure_preserves_old_report_without_music_submission() {
    for prior_success in [false, true] {
        let (_, mut mixer) = rig(None, None);
        let telemetry = source_telemetry(&mixer).unwrap();
        let mut version = 0;
        let mut snapshot = scalar(None, 0);
        if prior_success {
            let report = mixer.render(&mut [0.0; 3]).unwrap();
            snapshot = scalar(Some(report), 3);
            publish(&telemetry, &mixer, snapshot, &mut version);
        }
        // Actual core pull, followed by simulated failed remix/encoding/delivery facts.
        mixer.render(&mut [0.0; 2]).unwrap();
        snapshot.status = AudioStreamStatus::Failed { hresult: -17 };
        snapshot.counters.native_failures = 1;
        publish(&telemetry, &mixer, snapshot, &mut version);
        let selected = read(&telemetry, &mixer, mixer.frame_cursor()).unwrap();
        assert_eq!(selected.snapshot, snapshot);
        assert_eq!(
            selected.pulled_source_end,
            if prior_success { 5 } else { 2 }
        );
        assert_eq!(
            selected.snapshot.counters.submitted_frames,
            if prior_success { 3 } else { 0 }
        );
        assert_eq!(selected.snapshot.render.is_some(), prior_success);
    }
}

#[test]
fn simulated_post_submission_clock_failure_retains_literal_successful_prefix_and_clock() {
    let (_, mut mixer) = rig(None, None);
    let telemetry = source_telemetry(&mixer).unwrap();
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    let mut snapshot = scalar(Some(report), 4);
    snapshot.clock = Some(AudioClockSnapshot {
        position: 2,
        frequency: 1000,
        qpc_100ns: 37,
        reading_quality: AudioClockReadingQuality::Accurate,
        host_point: None,
        mapping_quality: ClockMappingQuality::Unknown,
    });
    snapshot.status = AudioStreamStatus::Failed { hresult: -23 };
    snapshot.counters.native_failures = 1;
    let mut version = 0;
    publish(&telemetry, &mixer, snapshot, &mut version);
    let selected = read(&telemetry, &mixer, 4).unwrap();
    assert_eq!(selected.snapshot, snapshot);
    assert_eq!((selected.pulled_source_end, selected.revision), (4, 2));
}

#[test]
fn checked_exhaustion_leaves_permanent_odd_source_unavailability_and_visible_failure_status() {
    let (_, mut mixer) = rig(None, None);
    let telemetry = source_telemetry(&mixer).unwrap();
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    let mut version = 0;
    let mut snapshot = scalar(Some(report), 2);
    publish(&telemetry, &mixer, snapshot, &mut version);
    assert!(read(&telemetry, &mixer, 2).is_some());
    version = u64::MAX - 1;
    snapshot.status = AudioStreamStatus::Failed { hresult: -29 };
    publish(&telemetry, &mixer, snapshot, &mut version);
    assert_eq!(version, u64::MAX - 1);
    assert!(read(&telemetry, &mixer, 2).is_none());
    assert!(!telemetry.read().telemetry_available);
    assert_eq!(telemetry.read().status, snapshot.status);
    publish(&telemetry, &mixer, snapshot, &mut version);
    assert!(read(&telemetry, &mixer, 0).is_none());
}
