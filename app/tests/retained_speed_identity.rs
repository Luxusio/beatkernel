//! Fresh original-chart owners, real Runtime reports, capture codec and saved comparison.
use beatkernel::{
    audio::{command_queue, SourcePosition},
    input::*,
    replay::codec::{decode_replay, encode_replay, ReplayFile},
    runtime::Runtime,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
    transport::{OutputPhaseBasis, OutputPhaseError},
};
use beatkernel_bms::{BmsChart, BmsGaugeKind};
use beatkernel_bms_runtime::{
    competition::{OpponentKind, ScoreSummary},
    competition_live::{replay_limits, CompetitionOptions, LiveCompetition},
    local_players::PlayerId,
    native_judge::NativeJudgeConfig,
    play_policy::{GaugeSelection, OriginalGaugeContext, ResolvedPlayPolicy},
    practice_session::{prepare_attempt, prepare_attempt_with_speed, PracticeAttemptConfig},
    replay_playback::{decode_section_setup, reconstruct_section, validate_section_setup},
    replay_speed::ReplaySpeed,
    session_launch::SessionLaunch,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn speed(s: &str) -> ReplaySpeed {
    s.parse().unwrap()
}
fn original() -> BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#RANK 2\n#WAV01 key.wav\n#00011:00010001\n",
        Default::default(),
    )
    .unwrap()
}
fn launch(record: bool) -> SessionLaunch {
    launch_with_speed(record, "6/4")
}
fn launch_with_speed(record: bool, fraction: &str) -> SessionLaunch {
    let mut a = vec![
        "--chart".into(),
        "original.bms".into(),
        "--speed".into(),
        fraction.into(),
    ];
    if record {
        a.extend(["--record-replay".into(), "records/take.bkr".into()]);
    }
    SessionLaunch::new(a).unwrap()
}
fn config(start: i64, record: bool) -> PracticeAttemptConfig {
    PracticeAttemptConfig {
        start: ts(start),
        end: Some(ts(3_500_000_000)),
        domain: ClockDomainId(71),
        chart_seed: 0,
        capture_limits: record.then(|| replay_limits().unwrap()),
    }
}
fn policy(source: &BmsChart, bms: bool) -> ResolvedPlayPolicy {
    NativeJudgeConfig {
        early: 10_000_000,
        late: 20_000_000,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(71),
        end: Some(ts(3_500_000_000)),
    }
    .resolve_play_policy(
        &OriginalGaugeContext::from_source(source),
        if bms {
            GaugeSelection::Bms(BmsGaugeKind::Hard)
        } else {
            GaugeSelection::BeatKernel
        },
    )
    .unwrap()
}

#[test]
fn four_fresh_rates_preserve_original_chart_policy_and_canonical_capture_identity() {
    let source = original();
    let objects = source.source.compile().unwrap().objects().to_vec();
    for bms in [false, true] {
        let p = policy(&source, bms);
        for s in ["1/2", "1/1", "2/1", "6/4"] {
            let a = prepare_attempt_with_speed(
                &source,
                &p,
                &launch_with_speed(true, s),
                config(500_000_000, true),
                speed(s),
            )
            .unwrap();
            assert_eq!(a.speed(), speed(s));
            assert_eq!(
                a.transport_at(ts(9_000_000_000))
                    .position_at(ts(9_000_000_000)),
                Ok(ts(500_000_000))
            );
            let expected = match s {
                "1/2" => 1_000_000_000,
                "1/1" => 1_500_000_000,
                "2/1" => 2_500_000_000,
                _ => 2_000_000_000,
            };
            assert_eq!(
                a.transport_at(ts(9_000_000_000))
                    .position_at(ts(10_000_000_000)),
                Ok(ts(expected))
            );
            assert_eq!(a.judge.chart().objects(), objects);
            assert_eq!(a.score, ScoreSummary::default());
            assert_eq!(a.gauge.profile(), p.gauge());
            assert_eq!(a.original_gauge, OriginalGaugeContext::from_source(&source));
            let capture = a.capture.unwrap();
            let setup = decode_section_setup(&capture.header().options).unwrap();
            assert_eq!(setup.start, ts(500_000_000));
            assert_eq!(setup.end, Some(ts(3_500_000_000)));
            assert_eq!(setup.source_speed, speed(s));
            let bytes = capture.into_bytes().unwrap();
            let file = decode_replay(&bytes, replay_limits().unwrap()).unwrap();
            validate_section_setup(&source, &file, replay_limits().unwrap()).unwrap();
            let replay = reconstruct_section(&source, file, replay_limits().unwrap()).unwrap();
            assert_eq!(replay.engine().profile(), p.judge());
        }
    }
    assert_eq!(source.source.compile().unwrap().objects(), objects);
}

#[test]
fn normal_attempt_preserves_exact_legacy_header_bytes_and_transport() {
    let source = original();
    let p = policy(&source, true);
    let old = prepare_attempt(
        &source,
        &p,
        &launch_with_speed(true, "1/1"),
        config(500_000_000, true),
    )
    .unwrap();
    let new = prepare_attempt_with_speed(
        &source,
        &p,
        &launch_with_speed(true, "1/1"),
        config(500_000_000, true),
        speed("8/8"),
    )
    .unwrap();
    for n in [900, 1000, 1_000_000_900] {
        assert_eq!(
            old.transport_at(ts(900)).position_at(ts(n)),
            new.transport_at(ts(900)).position_at(ts(n))
        );
    }
    assert_eq!(
        old.capture.unwrap().into_bytes().unwrap(),
        new.capture.unwrap().into_bytes().unwrap()
    );
    assert_eq!(old.next_launch.args(), new.next_launch.args());
}

#[test]
fn repeated_backward_selection_restores_original_heads_gauge_and_recording_lineage() {
    let source = original();
    let p = policy(&source, true);
    let launch = launch(true);
    let mut first = prepare_attempt_with_speed(
        &source,
        &p,
        &launch,
        config(2_000_000_000, true),
        speed("6/4"),
    )
    .unwrap();
    assert_eq!(first.excluded_objects, 1);
    let misses = first.judge.advance_to(ts(3_100_000_000)).unwrap();
    first.gauge.observe(&misses, &[]).unwrap();
    first.score.observe(&misses).unwrap();
    assert_eq!(first.score.misses, 1);
    let second = prepare_attempt_with_speed(
        &source,
        &p,
        &first.next_launch,
        config(500_000_000, true),
        speed("3/2"),
    )
    .unwrap();
    assert_eq!(second.judge.chart().objects().len(), 2);
    assert_eq!(
        second.judge.chart().objects()[0].time.start,
        ts(1_000_000_000)
    );
    assert_eq!(second.score, ScoreSummary::default());
    assert!(second.gauge.snapshot().failure.is_none());
    assert_eq!(first.next_launch.attempt(), 1);
    assert_eq!(second.next_launch.attempt(), 2);
    assert_eq!(
        first.recording_path.as_deref(),
        Some(std::path::Path::new("records/take.retry1.bkr"))
    );
    assert_eq!(
        second.recording_path.as_deref(),
        Some(std::path::Path::new("records/take.retry2.bkr"))
    );
    assert_eq!(second.next_launch.original_args(), launch.original_args());
    assert!(second
        .next_launch
        .args()
        .chunks_exact(2)
        .any(|pair| pair == ["--speed", "6/4"]));
    assert_eq!(
        decode_section_setup(&second.capture.unwrap().header().options)
            .unwrap()
            .source_speed,
        speed("3/2")
    );
}

struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn point(n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(71),
        timestamp: ts(n),
    }
}
fn input(n: i64, state: ButtonState, seq: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(8), point(n), seq),
        control: PhysicalControlId::keyboard(4),
        state,
    })
}

#[test]
fn actual_selected_runtime_reports_roundtrip_capture_and_rejudge_original_song_points() {
    let source = original();
    let p = policy(&source, true);
    for (s, first_at, last_at) in [
        ("1/2", 11_000_000_000, 15_000_000_000),
        ("1/1", 10_500_000_000, 12_500_000_000),
        ("2/1", 10_250_000_000, 11_250_000_000),
        ("3/2", 10_333_333_334, 11_666_666_667),
    ] {
        let mut a = prepare_attempt_with_speed(
            &source,
            &p,
            &launch_with_speed(true, s),
            config(500_000_000, true),
            speed(s),
        )
        .unwrap();
        let binding = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Exact(DeviceId(8)),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let transport = a.transport_at(ts(10_000_000_000));
        let (producer, _consumer) = command_queue(16).unwrap();
        let mut runtime = Runtime::new(
            ClockDomainId(71),
            ClockDomainId(71),
            transport,
            binding,
            a.judge,
            producer,
            vec![],
            0,
        )
        .unwrap();
        let mut expected = vec![];
        for (at, state, seq) in [
            (first_at, ButtonState::Down, 1),
            (first_at + 1, ButtonState::Up, 2),
            (last_at, ButtonState::Down, 3),
        ] {
            let report = runtime
                .process_input(input(at, state, seq), &Identity, point(at))
                .unwrap();
            if state == ButtonState::Down {
                assert_eq!(
                    report.song_time,
                    ts(if seq == 1 {
                        if s == "3/2" {
                            1_000_000_001
                        } else {
                            1_000_000_000
                        }
                    } else {
                        3_000_000_000
                    })
                );
            }
            a.capture.as_mut().unwrap().record_report(&report).unwrap();
            expected.extend(report.judge_events);
        }
        assert_eq!(expected.len(), 2);
        let bytes = a.capture.unwrap().into_bytes().unwrap();
        let file = decode_replay(&bytes, replay_limits().unwrap()).unwrap();
        assert_eq!(file.records.len(), 3);
        let mut replay = reconstruct_section(&source, file, replay_limits().unwrap()).unwrap();
        replay.seek_cursor(3).unwrap();
        assert_eq!(replay.results(), expected);
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            runtime.judge().stable_hash().unwrap()
        );
    }
}

// These mathematical phase fixtures deliberately do not claim native admission.
// Physical presentation, active-frame qualification and converter history are
// verified by the separate native bridge acceptance criteria.
fn phase_basis(hz: u32) -> OutputPhaseBasis {
    OutputPhaseBasis::new(ClockDomainId(71), 43, 7, 2, hz).unwrap()
}
fn source_position(frame: u64, numerator: u64, denominator: u64) -> SourcePosition {
    SourcePosition {
        frame,
        numerator,
        denominator,
    }
}

#[test]
fn prepared_phase_transport_keeps_one_relative_floor_for_four_selected_rates() {
    let source = original();
    let p = policy(&source, true);
    let b = phase_basis(44_100);
    let phase = b.point(point(1001), source_position(3, 0, 1)).unwrap();
    assert_eq!(phase.elapsed_nanos(), 22_675);
    for (fraction, increment) in [
        ("1/2", 11_337),
        ("1/1", 22_675),
        ("2/1", 45_350),
        ("6/4", 34_012),
    ] {
        let a = prepare_attempt_with_speed(
            &source,
            &p,
            &launch_with_speed(true, fraction),
            config(500_000_000, true),
            speed(fraction),
        )
        .unwrap();
        let t = a.transport_at_output_phase(b, point(1000)).unwrap();
        assert_eq!(t.output_phase_basis(), Some(b));
        assert_eq!(
            t.position_at_output_phase(phase),
            Ok(ts(500_000_000 + increment))
        );
        assert_eq!(
            decode_section_setup(&a.capture.as_ref().unwrap().header().options)
                .unwrap()
                .source_speed,
            speed(fraction)
        );
    }
}

#[test]
fn prepared_phase_transport_preserves_both_converter_fraction_literals() {
    let source = original();
    let p = policy(&source, true);
    let a = prepare_attempt_with_speed(
        &source,
        &p,
        &launch(true),
        config(500_000_000, true),
        speed("3/2"),
    )
    .unwrap();
    for (hz, numerator, denominator, elapsed, increment) in [
        (44_100, 121, 160, 17_148, 25_722),
        (48_000, 26, 147, 3_684, 5_526),
    ] {
        let b = phase_basis(hz);
        let phase = b
            .point(point(1001), source_position(2, numerator, denominator))
            .unwrap();
        assert_eq!(phase.elapsed_nanos(), elapsed);
        let t = a.transport_at_output_phase(b, point(1000)).unwrap();
        assert_eq!(
            t.position_at_output_phase(phase),
            Ok(ts(500_000_000 + increment))
        );
    }
}

#[test]
fn phase_domain_refusals_preserve_prepared_owners_capture_and_launch() {
    let source = original();
    let p = policy(&source, true);
    let invocation = launch(true);
    let a = prepare_attempt_with_speed(
        &source,
        &p,
        &invocation,
        config(500_000_000, true),
        speed("3/2"),
    )
    .unwrap();
    let hash = a.judge.stable_hash().unwrap();
    let gauge = *a.gauge.snapshot();
    let header = a.capture.as_ref().unwrap().header().clone();
    let args = a.next_launch.args().to_vec();
    let path = a.recording_path.clone();
    let wrong_basis = OutputPhaseBasis::new(ClockDomainId(72), 43, 7, 2, 44_100).unwrap();
    let wrong_point = ClockPoint {
        domain: ClockDomainId(72),
        timestamp: ts(1000),
    };
    for (basis, logical) in [
        (wrong_basis, point(1000)),
        (phase_basis(44_100), wrong_point),
        (wrong_basis, wrong_point),
    ] {
        assert!(matches!(
            a.transport_at_output_phase(basis, logical),
            Err(OutputPhaseError::WrongDomain)
        ));
        assert_eq!(a.judge.stable_hash().unwrap(), hash);
        assert_eq!(*a.gauge.snapshot(), gauge);
        assert_eq!(a.capture.as_ref().unwrap().header(), &header);
        assert_eq!(a.next_launch.args(), args);
        assert_eq!(a.recording_path, path);
        assert_eq!(a.score, ScoreSummary::default());
        assert_eq!(a.speed(), speed("3/2"));
    }
    assert_eq!(invocation.attempt(), 0);
    assert_eq!(a.next_launch.attempt(), 1);
    assert!(a
        .transport_at_output_phase(phase_basis(44_100), point(1000))
        .is_ok());
}

#[test]
fn prepared_phase_transport_caps_actual_finite_end_and_refuses_scalar_fallback() {
    let source = original();
    let p = policy(&source, true);
    let a = prepare_attempt_with_speed(
        &source,
        &p,
        &launch(true),
        config(500_000_000, true),
        speed("3/2"),
    )
    .unwrap();
    let b = phase_basis(44_100);
    let t = a.transport_at_output_phase(b, point(1000)).unwrap();
    let before_end = b
        .point_from_elapsed_nanos(point(1001), 1_999_999_999)
        .unwrap();
    let end = b
        .point_from_elapsed_nanos(point(1002), 2_000_000_000)
        .unwrap();
    let after = b
        .point_from_elapsed_nanos(point(1003), 6_000_000_000)
        .unwrap();
    assert_eq!(
        t.position_at_output_phase(before_end),
        Ok(ts(3_499_999_998))
    );
    assert_eq!(t.position_at_output_phase(end), Ok(ts(3_500_000_000)));
    assert_eq!(t.position_at_output_phase(after), Ok(ts(3_500_000_000)));
    assert!(t.position_at(ts(1001)).is_err());
    assert_eq!(
        t.position_at_output_phase(before_end),
        Ok(ts(3_499_999_998))
    );
    // The preserved legacy scalar helper remains an independent legacy owner.
    assert_eq!(
        a.transport_at(ts(1000)).position_at(ts(1001)),
        Ok(ts(500_000_001))
    );
}

#[test]
fn actual_phase_reports_roundtrip_original_replay_wire_for_four_rates_and_fractional_cuts() {
    use beatkernel::judge::JudgeOutcome;
    let source = original();
    let p = policy(&source, true);
    let original_objects = source.source.compile().unwrap().objects().to_vec();
    // Each row supplies independent whole-phase elapsed and rate-scaled literals.
    for (hz, position, elapsed, increments) in [
        (
            44_100,
            source_position(3, 0, 1),
            22_675,
            [11_337, 22_675, 45_350, 34_012],
        ),
        (
            44_100,
            source_position(2, 121, 160),
            17_148,
            [8_574, 17_148, 34_296, 25_722],
        ),
        (
            48_000,
            source_position(2, 26, 147),
            3_684,
            [1_842, 3_684, 7_368, 5_526],
        ),
    ] {
        for (index, fraction) in ["1/2", "1/1", "2/1", "6/4"].into_iter().enumerate() {
            let mut a = prepare_attempt_with_speed(
                &source,
                &p,
                &launch_with_speed(true, fraction),
                config(999_900_000, true),
                speed(fraction),
            )
            .unwrap();
            assert_eq!(a.judge.chart().objects(), original_objects);
            let b = phase_basis(hz);
            let transport = a.transport_at_output_phase(b, point(1000)).unwrap();
            let bindings = BindingMap::from_bindings([Binding {
                device: DeviceSelector::Exact(DeviceId(8)),
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(0x11),
            }])
            .unwrap();
            let (producer, _consumer) = command_queue(16).unwrap();
            let mut runtime = Runtime::new(
                ClockDomainId(71),
                ClockDomainId(71),
                transport,
                bindings,
                a.judge,
                producer,
                vec![],
                0,
            )
            .unwrap();
            runtime.set_song_end(ts(3_500_000_000)).unwrap();
            let expected_song = ts(999_900_000 + increments[index]);
            let mut events = vec![];
            for (at, state, seq) in [(1001, ButtonState::Down, 1), (1002, ButtonState::Up, 2)] {
                let physical = input(at, state, seq);
                let phase = b.point(point(at), position).unwrap();
                assert_eq!(phase.elapsed_nanos(), elapsed);
                let report = runtime
                    .process_input_output_phase(
                        physical.clone(),
                        &Identity,
                        point(7000 + at),
                        phase,
                    )
                    .unwrap();
                assert_eq!(report.song_time, expected_song);
                assert_eq!(report.input.as_ref(), Some(&physical));
                assert_eq!(report.audio_at, point(7000 + at));
                assert!(report.judge_error.is_none());
                if state == ButtonState::Down {
                    assert_eq!(report.judge_events.len(), 1);
                    assert!(matches!(
                        report.judge_events[0].outcome,
                        JudgeOutcome::Hit { .. }
                    ));
                    assert_eq!(report.judge_events[0].at, expected_song);
                }
                a.capture.as_mut().unwrap().record_report(&report).unwrap();
                events.extend(report.judge_events);
            }
            let report = runtime
                .advance_to_output_phase(
                    point(1003),
                    &Identity,
                    point(8000),
                    b.point_from_elapsed_nanos(point(1003), 6_000_000_000)
                        .unwrap(),
                )
                .unwrap();
            assert_eq!(report.song_time, ts(3_500_000_000));
            assert!(report.song_end_reached);
            assert_eq!(report.judge_events.len(), 1);
            assert!(matches!(
                report.judge_events[0].outcome,
                JudgeOutcome::Miss { .. }
            ));
            a.capture.as_mut().unwrap().record_report(&report).unwrap();
            events.extend(report.judge_events);
            assert_eq!(events.len(), 2);
            let bytes = a.capture.unwrap().into_bytes().unwrap();
            let file = decode_replay(&bytes, replay_limits().unwrap()).unwrap();
            let setup = decode_section_setup(&file.header.options).unwrap();
            assert_eq!(setup.source_speed, speed(fraction));
            assert_eq!(setup.start, ts(999_900_000));
            assert_eq!(setup.end, Some(ts(3_500_000_000)));
            assert_eq!(file.records.len(), 3);
            assert_eq!(
                file.records.iter().map(|r| r.song_time).collect::<Vec<_>>(),
                [expected_song, expected_song, ts(3_500_000_000)]
            );
            let mut replay = reconstruct_section(&source, file, replay_limits().unwrap()).unwrap();
            replay.seek_cursor(3).unwrap();
            assert_eq!(replay.results(), events);
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                runtime.judge().stable_hash().unwrap()
            );
        }
    }
    assert_eq!(source.source.compile().unwrap().objects(), original_objects);
}

struct Saved(PathBuf);
impl Drop for Saved {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn saved(a: &beatkernel_bms_runtime::practice_session::PreparedPracticeAttempt) -> Saved {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "beatkernel-retained-identity-{}-{}.bkr",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let bytes = encode_replay(
        &ReplayFile::new(a.capture.as_ref().unwrap().header().clone(), vec![]),
        replay_limits().unwrap(),
    )
    .unwrap();
    std::fs::write(&path, bytes).unwrap();
    Saved(path)
}

#[test]
fn saved_comparison_reloads_same_canonical_speed_and_refuses_different_fresh_attempt() {
    let source = original();
    for bms in [false, true] {
        let p = policy(&source, bms);
        let a = prepare_attempt_with_speed(
            &source,
            &p,
            &launch(true),
            config(500_000_000, true),
            speed("6/4"),
        )
        .unwrap();
        let saved = saved(&a);
        for kind in [OpponentKind::Own, OpponentKind::Other] {
            let options = CompetitionOptions {
                ghosts: vec![(kind, saved.0.clone())],
                ..Default::default()
            };
            let old = LiveCompetition::prepare_member_section_with_policy_and_speed(
                PlayerId(43),
                &options,
                &a.source,
                &a.judge,
                &p,
                a.config.domain,
                a.config.start,
                0,
                a.config.end,
                speed("3/2"),
            )
            .unwrap()
            .unwrap();
            let fresh = old.prepare_practice(&a, &p).unwrap();
            let snapshot = fresh.archive_snapshot().unwrap();
            assert_eq!(snapshot.ghosts.len(), 1);
            assert_eq!(snapshot.ghosts[0].kind, kind);
            let wrong = prepare_attempt_with_speed(
                &source,
                &p,
                &launch_with_speed(true, "2/1"),
                config(500_000_000, true),
                speed("2/1"),
            )
            .unwrap();
            assert!(old.prepare_practice(&wrong, &p).is_err());
            assert_eq!(old.archive_snapshot().unwrap().ghosts.len(), 1);
        }
    }
}

#[test]
fn cold_refusals_leave_existing_attempt_and_original_launch_untouched() {
    let source = original();
    let p = policy(&source, true);
    let launch = launch(false);
    let a = prepare_attempt_with_speed(
        &source,
        &p,
        &launch,
        config(500_000_000, false),
        speed("3/2"),
    )
    .unwrap();
    let hash = a.judge.stable_hash().unwrap();
    let snapshot = *a.gauge.snapshot();
    for bad in [
        config(-1, false),
        config(3_500_000_000, false),
        config(500_000_000, true),
    ] {
        assert!(
            prepare_attempt_with_speed(&source, &p, &a.next_launch, bad, speed("3/2")).is_err()
        );
        assert_eq!(a.judge.stable_hash().unwrap(), hash);
        assert_eq!(*a.gauge.snapshot(), snapshot);
        assert_eq!(a.next_launch.attempt(), 1);
    }
    assert_eq!(launch.attempt(), 0);
}

#[test]
fn rate_mismatch_cannot_relabel_pinned_invocation_even_without_capture() {
    let source = original();
    let p = policy(&source, true);
    let launch = launch(false);
    for s in ["1/2", "1/1", "2/1"] {
        assert!(prepare_attempt_with_speed(
            &source,
            &p,
            &launch,
            config(500_000_000, false),
            speed(s)
        )
        .is_err());
        assert_eq!(launch.attempt(), 0);
    }
    assert!(prepare_attempt_with_speed(
        &source,
        &p,
        &launch,
        config(500_000_000, false),
        speed("3/2")
    )
    .is_ok());
}

#[test]
fn normal_builtin_saved_comparison_keeps_legacy_endpoint_free_header() {
    use beatkernel_bms_runtime::{
        input_sounds::InputSoundIdentity, replay_capture::LiveReplayCapture,
    };
    let source = original();
    let p = policy(&source, false);
    let a = prepare_attempt_with_speed(
        &source,
        &p,
        &launch_with_speed(true, "1/1"),
        config(500_000_000, true),
        speed("1/1"),
    )
    .unwrap();
    let legacy = LiveReplayCapture::new_with_input_sounds(
        &a.judge,
        a.config.domain,
        replay_limits().unwrap(),
        a.config.start,
        0,
        None,
        beatkernel_bms::BmsInputMode::ButtonOnly,
        InputSoundIdentity::from_source(&a.source).unwrap(),
    )
    .unwrap();
    let selected = a.capture.as_ref().unwrap();
    // Legacy comparison deliberately omits the finite endpoint, while the live
    // admitted policy header describes the finite attempt. Do not collapse them.
    assert_eq!(
        decode_section_setup(&legacy.header().options).unwrap().end,
        None
    );
    assert_eq!(
        decode_section_setup(&selected.header().options)
            .unwrap()
            .end,
        a.config.end
    );
    let legacy_attempt = prepare_attempt(
        &source,
        &p,
        &launch_with_speed(true, "1/1"),
        config(500_000_000, true),
    )
    .unwrap();
    assert_eq!(
        selected.header(),
        legacy_attempt.capture.as_ref().unwrap().header()
    );
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let saved = Saved(std::env::temp_dir().join(format!(
        "beatkernel-retained-normal-{}-{}.bkr",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::write(&saved.0, legacy.into_bytes().unwrap()).unwrap();
    let options = CompetitionOptions {
        ghosts: vec![(OpponentKind::Own, saved.0.clone())],
        ..Default::default()
    };
    let owner = LiveCompetition::prepare_member_section_with_policy(
        PlayerId(43),
        &options,
        &a.source,
        &a.judge,
        &p,
        a.config.domain,
        a.config.start,
        0,
        a.config.end,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        owner
            .prepare_practice(&a, &p)
            .unwrap()
            .archive_snapshot()
            .unwrap()
            .ghosts
            .len(),
        1
    );
}
