//! Deferred real mixer pause/replacement state; no backend retirement is implied.
use super::*;
use beatkernel::audio::*;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn ns(frame: u64, rate: u32) -> i64 {
    i64::try_from(u128::from(frame) * 1_000_000_000 / u128::from(rate)).unwrap()
}
fn pair(frame: u64, rate: u32) -> ClockPair {
    ClockPair {
        source: point(1, ns(frame, rate)),
        target: point(2, ns(frame, rate) + 100),
    }
}
struct Rig {
    producer: CommandProducer,
    mixer: Mixer,
    pause: NativePause,
    old_report: RenderReport,
}
fn rig(
    rate: u32,
    origin: i64,
    domain: u32,
    frozen: usize,
    extra: usize,
    end: Option<u64>,
    start: Option<u64>,
) -> Rig {
    let format = AudioFormat::new(rate, 1).unwrap();
    let limits = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = if start.is_some() {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::from_nanos(origin),
            gain: 0.5,
        })
        .unwrap();
    if let Some(start) = start {
        producer.schedule_start_at(start).unwrap();
    }
    let config = MixerConfig::new(
        format,
        ClockDomainId(domain),
        Timestamp::from_nanos(origin),
        AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
    );
    let mut mixer = Mixer::new(
        if let Some(end) = end {
            config.with_playback_end_frame(end)
        } else {
            config
        },
        bank,
        consumer,
    )
    .unwrap();
    assert_eq!(mixer.start_gate_frame(), start.map(Some));
    assert_eq!(mixer.applied_start_frame(), None);
    let mut pause = NativePause::new(point(domain, origin), ClockDomainId(2), rate).unwrap();
    if let Some(end) = end {
        pause = pause.with_playback_end_frame(end).unwrap();
    }
    if let Some(start) = start {
        pause = pause.with_start_frame(start).unwrap();
    }
    mixer
        .render(&mut vec![0.; frozen + start.unwrap_or(0) as usize])
        .unwrap();
    assert_eq!(mixer.applied_start_frame(), start);
    let frame = mixer.frame_cursor();
    let reference = ClockPair {
        source: point(domain, origin + ns(frame, rate)),
        target: point(2, ns(frame, rate) + 100),
    };
    pause.request(true, reference).unwrap();
    producer.request_pause(true);
    let old_report = mixer.render(&mut [0.; 1]).unwrap();
    let frame = mixer.frame_cursor();
    let observed = ClockPair {
        source: point(domain, origin + ns(frame, rate)),
        target: point(2, ns(frame, rate) + 100),
    };
    let boundary = pause.observe(Some(old_report), observed).unwrap().unwrap();
    assert!(boundary.paused);
    assert_eq!(boundary.playback_frame, frozen as u64);
    mixer.render(&mut vec![0.; extra]).unwrap();
    Rig {
        producer,
        mixer,
        pause,
        old_report,
    }
}
fn standard() -> Rig {
    rig(3, 0, 1, 2, 2, None, None)
}

#[test]
fn full_owner_authorizes_only_exact_pending_basis_without_lowering_pause_frontier() {
    use beatkernel_platform::audio::{DeviceFormat, NativeOutputState, SampleEncoding};
    let r = standard();
    let mut state = match NativeOutputState::new(
        r.mixer,
        DeviceFormat::new(3, 1, SampleEncoding::Float32, None).unwrap(),
        None,
        4,
    ) {
        Ok(state) => state,
        Err(_) => panic!("valid direct state"),
    };
    let tail = state.render_pending(4).unwrap();
    assert_eq!((tail.start_frame, tail.frames), (5, 4));
    state.admit(1).unwrap();
    assert_eq!(state.pending_samples(), &[0.0; 3]);
    let basis = state.output_frame_basis();
    assert_eq!(basis.start_physical_frame(), 6);
    let mut candidate = r.pause.clone();
    candidate.rebind_output_state(1, &state).unwrap();
    assert_eq!(candidate.min_physical_frame, 9);
    assert_eq!(candidate.frozen, 2);
    assert!(!candidate.replacement_observation_ready(Some(tail), pair(9, 3)));
    assert!(!candidate.replacement_observation_ready(None, pair(9, 3)));
    state.admit(3).unwrap();
    let fresh = state.render_pending(1).unwrap();
    assert!(!candidate.replacement_observation_ready(Some(fresh), pair(8, 3)));
    assert!(candidate.replacement_observation_ready(Some(fresh), pair(10, 3)));
    for invalid in [
        RenderReport { frames: 0, ..fresh },
        RenderReport {
            paused: false,
            ..fresh
        },
        RenderReport {
            playback_frames: 1,
            ..fresh
        },
        RenderReport {
            playback_start_frame: 3,
            ..fresh
        },
    ] {
        assert!(!candidate.replacement_observation_ready(Some(invalid), pair(10, 3)));
    }
    candidate
        .observe_in_epoch(1, Some(fresh), pair(10, 3))
        .unwrap();
    r.pause
        .validate_replacement(&candidate, basis, pair(10, 3))
        .unwrap();
    for frame in [5, 7, 8, 9] {
        let unauthorized = OutputFrameBasis::new(basis.origin(), 3, frame).unwrap();
        assert!(r
            .pause
            .validate_replacement(&candidate, unauthorized, pair(10, 3))
            .is_err());
    }
}

#[test]
fn active_pending_tail_refuses_held_rebind_without_rewriting_frozen_identity() {
    use beatkernel_platform::audio::{DeviceFormat, NativeOutputState, SampleEncoding};
    let mut r = standard();
    r.producer.request_pause(false);
    let mut state = match NativeOutputState::new(
        r.mixer,
        DeviceFormat::new(3, 1, SampleEncoding::Float32, None).unwrap(),
        None,
        2,
    ) {
        Ok(state) => state,
        Err(_) => panic!("valid direct state"),
    };
    let active = state.render_pending(2).unwrap();
    assert!(!active.paused);
    assert_eq!(state.pending_samples(), &[0.375, 0.5]);
    r.producer.request_pause(true);
    let mut candidate = r.pause.clone();
    let before = format!("{candidate:?}");
    assert!(candidate.rebind_output_state(1, &state).is_err());
    unchanged(&candidate, &before);
    assert_eq!(state.pending_samples(), &[0.375, 0.5]);
    state.admit(2).unwrap();
    state.render_pending(1).unwrap();
    // The output state now contains genuine silence, but playback moved while
    // active: it cannot rewrite the already acknowledged frozen identity.
    assert!(candidate.rebind_output_state(1, &state).is_err());
    unchanged(&candidate, &before);
}
fn unchanged(pause: &NativePause, before: &str) {
    assert_eq!(format!("{pause:?}"), before);
}
#[test]
fn acknowledged_pause_rebind_preserves_frozen_gap_and_original_pcm_then_commits_cumulative_gap_only_on_genuine_resume(
) {
    let mut r = standard();
    assert_eq!(r.pause.epoch(), 0);
    assert_eq!(
        (r.mixer.frame_cursor(), r.mixer.playback_frame_cursor()),
        (5, 2)
    );
    r.producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(7),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_000_000_000),
            gain: 0.5,
        })
        .unwrap();
    let counters = r.mixer.counters();
    r.pause.rebind_output(1, &r.mixer).unwrap();
    assert_eq!(r.pause.gap, 0);
    assert_eq!(r.pause.frozen, 2);
    assert_eq!(r.pause.phase(), PausePhase::Paused);
    assert!(r.pause.last_render_report().is_none());
    assert_eq!(r.mixer.counters(), counters);
    assert_eq!(
        r.pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
        Timestamp::ZERO
    );
    assert!(r.pause.request_in_epoch(1, false, pair(5, 3)).unwrap());
    r.producer.request_pause(false);
    let mut pcm = [0.; 1];
    let report = r.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.375]);
    let boundary = r
        .pause
        .observe_in_epoch(1, Some(report), pair(6, 3))
        .unwrap()
        .unwrap();
    assert!(!boundary.paused);
    assert_eq!(boundary.playback_frame, 2);
    assert_eq!(r.pause.gap, 3);
    assert_eq!(r.pause.phase(), PausePhase::Running);
    assert_eq!(
        r.pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
        Timestamp::from_nanos(-1_000_000_000)
    );
    r.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.625]);
    // A later acknowledged pause starts with the previous gap. Rebinding must
    // preserve it until the next genuine resume replaces it cumulatively.
    r.pause.request_in_epoch(1, true, pair(7, 3)).unwrap();
    r.producer.request_pause(true);
    let paused = r.mixer.render(&mut [0.; 1]).unwrap();
    r.pause
        .observe_in_epoch(1, Some(paused), pair(8, 3))
        .unwrap()
        .unwrap();
    r.mixer.render(&mut [0.; 1]).unwrap();
    r.pause.rebind_output(2, &r.mixer).unwrap();
    assert_eq!(r.pause.gap, 3);
    r.pause.request_in_epoch(2, false, pair(9, 3)).unwrap();
    r.producer.request_pause(false);
    let resumed = r.mixer.render(&mut [0.; 1]).unwrap();
    r.pause
        .observe_in_epoch(2, Some(resumed), pair(10, 3))
        .unwrap()
        .unwrap();
    assert_eq!(r.pause.gap, 5);
    assert_eq!(
        r.pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
        Timestamp::from_nanos(-1_666_666_666)
    );
}
#[test]
fn three_real_one_frame_replacements_floor_cumulative_gap_once_instead_of_summing_rounded_pause_durations(
) {
    let mut r = rig(3, 0, 1, 2, 0, None, None);
    for epoch in 1..=3 {
        let old_gap = r.pause.gap;
        r.pause.rebind_output(epoch, &r.mixer).unwrap();
        assert_eq!(r.pause.gap, old_gap);
        let physical = r.mixer.frame_cursor();
        r.pause
            .request_in_epoch(epoch, false, pair(physical, 3))
            .unwrap();
        r.producer.request_pause(false);
        let resumed = r.mixer.render(&mut [0.; 1]).unwrap();
        let physical = r.mixer.frame_cursor();
        r.pause
            .observe_in_epoch(epoch, Some(resumed), pair(physical, 3))
            .unwrap()
            .unwrap();
        assert_eq!(r.pause.gap, epoch);
        assert_eq!(
            r.pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::from_nanos(-ns(epoch, 3))
        );
        if epoch < 3 {
            r.pause
                .request_in_epoch(epoch, true, pair(physical, 3))
                .unwrap();
            r.producer.request_pause(true);
            let paused = r.mixer.render(&mut [0.; 1]).unwrap();
            r.pause
                .observe_in_epoch(epoch, Some(paused), pair(r.mixer.frame_cursor(), 3))
                .unwrap()
                .unwrap();
        }
    }
    assert_eq!(
        r.pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
        Timestamp::from_nanos(-1_000_000_000)
    );
    assert_ne!(3 * ns(1, 3), ns(3, 3));
}
#[test]
fn repeated_paused_replacement_reaches_max_epoch_without_wrap_and_all_stale_tagged_operations_are_atomic(
) {
    let mut r = standard();
    for epoch in [1, 7, u64::MAX] {
        r.pause.rebind_output(epoch, &r.mixer).unwrap();
        assert_eq!(r.pause.epoch(), epoch);
        assert_eq!((r.pause.gap, r.pause.frozen), (0, 2));
    }
    let before = format!("{:?}", r.pause);
    let invalid = ClockPair {
        source: point(99, i64::MIN),
        target: point(99, i64::MIN),
    };
    assert!(r
        .pause
        .request_in_epoch(0, false, invalid)
        .unwrap_err()
        .0
        .contains("epoch"));
    unchanged(&r.pause, &before);
    assert!(r
        .pause
        .observe_in_epoch(0, Some(r.old_report), invalid)
        .unwrap_err()
        .0
        .contains("epoch"));
    unchanged(&r.pause, &before);
    let interval = PauseIntervalObservation {
        output_origin: point(99, 0),
        sample_rate: 0,
        render: r.old_report,
        clock: StartInterval {
            output: point(99, 0),
            before: point(99, 2),
            after: point(99, 1),
        },
    };
    assert!(r
        .pause
        .request_interval_in_epoch(0, false, interval)
        .unwrap_err()
        .0
        .contains("epoch"));
    unchanged(&r.pause, &before);
    assert!(r
        .pause
        .observe_interval_in_epoch(0, Some(interval), point(99, i64::MIN))
        .unwrap_err()
        .0
        .contains("epoch"));
    unchanged(&r.pause, &before);
    for epoch in [0, 7, u64::MAX] {
        assert!(r.pause.rebind_output(epoch, &r.mixer).is_err());
        unchanged(&r.pause, &before);
    }
}
#[test]
fn replacement_switches_point_interval_point_and_preserves_actual_arrival_not_uncertainty_upper_endpoint(
) {
    let mut r = standard();
    r.pause.rebind_output(1, &r.mixer).unwrap();
    let report = r.mixer.render(&mut [0.; 1]).unwrap();
    let actual = point(2, ns(6, 3) + 10);
    let upper = point(2, ns(6, 3) + 1_000_000);
    let interval = PauseIntervalObservation {
        output_origin: point(1, 0),
        sample_rate: 3,
        render: report,
        clock: StartInterval::new(
            point(1, ns(report.start_frame, 3)),
            point(2, ns(5, 3)),
            upper,
        )
        .unwrap(),
    };
    assert_eq!(
        r.pause
            .observe_interval_in_epoch(1, Some(interval), actual)
            .unwrap(),
        None
    );
    assert_eq!(r.pause.evidence_kind, Some(EvidenceKind::Interval));
    r.pause.rebind_output(2, &r.mixer).unwrap();
    assert_eq!(r.pause.min_host, Some(actual));
    assert!(r.pause.evidence_kind.is_none());
    let fresh = ClockPair {
        source: point(1, ns(6, 3)),
        target: point(2, actual.timestamp.as_nanos() + 1),
    };
    assert!(fresh.target.timestamp < upper.timestamp);
    assert!(!r.pause.request_in_epoch(2, true, fresh).unwrap());
    assert_eq!(r.pause.evidence_kind, Some(EvidenceKind::Point));
    assert_eq!((r.pause.gap, r.pause.frozen), (0, 2));
}
#[test]
fn captured_physical_and_greatest_host_floors_refuse_old_reports_and_relations_before_mutating_fresh_kind(
) {
    let mut r = standard();
    r.pause.rebind_output(1, &r.mixer).unwrap();
    let before = format!("{:?}", r.pause);
    assert!(r.pause.request_in_epoch(1, false, pair(4, 3)).is_err());
    unchanged(&r.pause, &before);
    let bad_host = ClockPair {
        source: point(1, ns(5, 3)),
        target: point(2, ns(3, 3) + 99),
    };
    assert!(r.pause.observe_in_epoch(1, None, bad_host).is_err());
    unchanged(&r.pause, &before);
    assert!(r
        .pause
        .observe_in_epoch(1, Some(r.old_report), pair(5, 3))
        .is_err());
    unchanged(&r.pause, &before);
    let older = rig(3, 0, 1, 2, 0, None, None);
    assert!(r.pause.rebind_output(2, &older.mixer).is_err());
    unchanged(&r.pause, &before);
    // Explicit malformed-history fault injection, not a report emitted by Mixer.
    let mut malformed = r.pause.clone();
    let mut report = r.old_report;
    report.start_frame = u64::MAX;
    report.frames = 1;
    malformed.last_report = Some(report);
    let fault = format!("{malformed:?}");
    assert!(malformed
        .rebind_output(2, &r.mixer)
        .unwrap_err()
        .0
        .contains("overflow"));
    unchanged(&malformed, &fault);
    assert!(!r.pause.request_in_epoch(1, true, pair(5, 3)).unwrap());
}
#[test]
fn phase_grid_frozen_cursor_and_unpaused_model_refusals_preserve_complete_acknowledged_pause_state()
{
    let mut r = standard();
    let before = format!("{:?}", r.pause);
    for candidate in [
        rig(4, 0, 1, 2, 2, None, None),
        rig(3, -1, 1, 2, 2, None, None),
        rig(3, 0, 3, 2, 2, None, None),
        rig(3, 0, 1, 1, 4, None, None),
        rig(3, 0, 1, 3, 2, None, None),
    ] {
        assert!(r.pause.rebind_output(1, &candidate.mixer).is_err());
        unchanged(&r.pause, &before);
    }
    let mut active = standard();
    active.producer.request_pause(false);
    active.mixer.render(&mut [0.; 1]).unwrap();
    assert!(r.pause.rebind_output(1, &active.mixer).is_err());
    unchanged(&r.pause, &before);
    let mut running = NativePause::new(point(1, 0), ClockDomainId(2), 3).unwrap();
    let state = format!("{running:?}");
    assert!(running.rebind_output(1, &r.mixer).is_err());
    unchanged(&running, &state);
    running.request(true, pair(0, 3)).unwrap();
    let state = format!("{running:?}");
    assert!(running.rebind_output(1, &r.mixer).is_err());
    unchanged(&running, &state);
    r.pause.request(false, pair(5, 3)).unwrap();
    let state = format!("{:?}", r.pause);
    assert!(r.pause.rebind_output(1, &r.mixer).is_err());
    unchanged(&r.pause, &state);
}
#[test]
fn immutable_start_end_and_reached_endpoint_refuse_while_compatible_finite_replacement_keeps_start_gap(
) {
    let mut r = rig(3, 0, 1, 2, 1, Some(8), Some(4));
    let before = format!("{:?}", r.pause);
    for candidate in [
        rig(3, 0, 1, 2, 7, Some(8), None),
        rig(3, 0, 1, 2, 3, Some(8), Some(5)),
        rig(3, 0, 1, 2, 3, Some(9), Some(4)),
        rig(3, 0, 1, 2, 3, None, Some(4)),
    ] {
        assert!(r.pause.rebind_output(1, &candidate.mixer).is_err());
        unchanged(&r.pause, &before);
    }
    r.pause.rebind_output(1, &r.mixer).unwrap();
    assert_eq!(r.pause.start_frame, 4);
    assert_eq!(r.pause.playback_end, Some(8));
    assert_eq!(r.pause.gap, 4);
    assert_eq!(r.pause.frozen, 2);
    assert_eq!(r.mixer.start_gate_frame(), Some(Some(4)));
    assert_eq!(r.mixer.applied_start_frame(), Some(4));
    let rebound = format!("{:?}", r.pause);
    for (target, frames) in [(None, 1), (Some(4), 1), (Some(4), 4)] {
        let format = AudioFormat::new(3, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(64, 128, 1).unwrap()).unwrap();
        let (mut producer, consumer) = command_queue_with_start_gate(8).unwrap();
        if let Some(target) = target {
            producer.schedule_start_at(target).unwrap();
        }
        let mut unresolved = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(1),
                Timestamp::ZERO,
                AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
            )
            .with_playback_end_frame(8),
            bank,
            consumer,
        )
        .unwrap();
        unresolved.render(&mut vec![0.; frames]).unwrap();
        assert!(unresolved.is_paused());
        assert_eq!(unresolved.start_gate_frame(), Some(target));
        assert_eq!(unresolved.applied_start_frame(), None);
        assert!(r
            .pause
            .rebind_output(2, &unresolved)
            .unwrap_err()
            .0
            .contains("startup"));
        unchanged(&r.pause, &rebound);
    }
    assert!(r.pause.clone().with_start_frame(5).is_err());
    let mut finite = rig(3, 0, 1, 1, 1, Some(2), None);
    let before = format!("{:?}", finite.pause);
    finite.producer.request_pause(false);
    let terminal = finite.mixer.render(&mut [0.; 1]).unwrap();
    assert!(terminal.playback_end_physical_frame.is_some());
    assert!(finite.pause.rebind_output(1, &finite.mixer).is_err());
    unchanged(&finite.pause, &before);
    let initial = NativePause::new(point(1, 0), ClockDomainId(2), 3).unwrap();
    assert!(initial.clone().with_start_frame(u64::MAX).is_err());
    assert!(initial.with_playback_end_frame(u64::MAX).is_err());
}
