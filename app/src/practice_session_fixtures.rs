use super::*;
use beatkernel::{
    chart::Beat,
    input::*,
    replay::{codec::encode_replay, ReplaySession},
    time::ClockPoint,
};
use beatkernel_bms::{parse, BmsGaugeKind, ParseOptions};

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn original() -> BmsChart {
    parse(
        "#BPM 120\n#RANK 2\n#TOTAL 240\n#WAV01 key.wav\n#BPM01 240\n#STOP01 48\n#00012:01\n#00051:0101\n#00108:01\n#00109:01\n#00111:0101\n#00211:01\n",
        ParseOptions::default(),
    )
    .unwrap()
}
fn launch(recording: bool) -> SessionLaunch {
    let mut args = vec!["--chart".into(), "original.bms".into()];
    if recording {
        args.extend(["--record-replay".into(), "records/my.take.bkr".into()]);
    }
    SessionLaunch::new(args).unwrap()
}
fn config(start: i64, end: Option<i64>, recording: bool) -> PracticeAttemptConfig {
    PracticeAttemptConfig {
        start: ts(start),
        end: end.map(ts),
        domain: ClockDomainId(17),
        chart_seed: u64::MAX,
        capture_limits: crate::native_judge::capture_limits(recording, 16384, 128).unwrap(),
    }
}
fn input(control: u32, at: i64, state: ButtonState, sequence: u64) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(control),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(3),
                ClockPoint {
                    domain: ClockDomainId(17),
                    timestamp: ts(at),
                },
                sequence,
            ),
            control: PhysicalControlId::keyboard(7),
            state,
        }),
    }
}

#[test]
fn original_bpm_stop_and_crossing_heads_match_the_real_fresh_setup() {
    let original = original();
    let cfg = config(550_000_000, Some(3_500_000_000), true);
    let policy = NativeJudgeConfig {
        early: 11,
        late: 23,
        offset: -19,
        preroll: 0,
        output: cfg.domain,
        end: cfg.end,
    }
    .resolve_play_policy(
        &OriginalGaugeContext::from_source(&original),
        GaugeSelection::Bms(BmsGaugeKind::Hazard),
    )
    .unwrap();
    let original_objects = original.source.compile().unwrap().objects().to_vec();
    let mut attempt = prepare_attempt(&original, &policy, &launch(true), cfg).unwrap();
    let selected = crate::section_start::source_at(&original, cfg.start).unwrap();
    let compiled = selected.source.compile().unwrap();
    let expected = original_objects
        .iter()
        .filter(|o| o.time.start >= cfg.start)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(attempt.judge.chart().objects(), expected);
    // Literals independently pin BPM240 plus the quarter-second STOP at beat4.
    assert_eq!(
        expected
            .iter()
            .map(|o| o.time.start.as_nanos())
            .collect::<Vec<_>>(),
        [2_000_000_000, 2_750_000_000, 3_250_000_000]
    );
    assert_eq!(attempt.excluded_objects, 2);
    assert_eq!(attempt.excluded_crossing_holds, 1);
    assert_eq!(
        attempt.source.source.bpm_changes,
        original.source.bpm_changes
    );
    assert_eq!(attempt.source.source.stops, original.source.stops);
    let native_config = NativeJudgeConfig {
        early: 11,
        late: 23,
        offset: -19,
        preroll: 0,
        output: cfg.domain,
        end: cfg.end,
    };
    let mut baseline = native_config
        .judge_with_policy(&selected, compiled, &policy)
        .unwrap();
    assert_eq!(
        attempt.judge.stable_hash().unwrap(),
        baseline.stable_hash().unwrap()
    );
    let baseline_capture = crate::native_judge::prepare_section_capture_for_policy(
        &selected,
        &baseline,
        &policy,
        cfg.domain,
        cfg.start,
        cfg.chart_seed,
        cfg.end,
        cfg.capture_limits,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        encode_replay(
            &beatkernel::replay::codec::ReplayFile::new(
                attempt.capture.as_ref().unwrap().header().clone(),
                vec![]
            ),
            cfg.capture_limits.unwrap()
        )
        .unwrap(),
        encode_replay(
            &beatkernel::replay::codec::ReplayFile::new(baseline_capture.header().clone(), vec![]),
            cfg.capture_limits.unwrap()
        )
        .unwrap(),
    );
    // A held old crossing LN has no new head/tail stage in the fresh attempt.
    for (at, state, sequence) in [
        (550_000_000, ButtonState::Down, 0),
        (1_000_000_000, ButtonState::Up, 1),
    ] {
        let event = input(0x11, at, state, sequence);
        let a = attempt.judge.push_input(&event, ts(at)).unwrap();
        let b = baseline.push_input(&event, ts(at)).unwrap();
        assert_eq!(a, b);
        assert!(a.is_empty());
    }
    // Current state may fail; the next payload must not inherit that failure.
    let missed = attempt.judge.advance_to(ts(2_100_000_000)).unwrap();
    attempt.gauge.observe(&missed, &[]).unwrap();
    attempt.score.observe(&missed).unwrap();
    assert!(attempt.gauge.snapshot().failure.is_some());
    assert_eq!(attempt.score.misses, 1);
    let second = prepare_attempt(&original, &policy, &attempt.next_launch, cfg).unwrap();
    assert!(second.gauge.snapshot().failure.is_none());
    assert_eq!(second.score, ScoreSummary::default());
    assert_eq!(second.judgments.as_ref(), policy.judgments());
    assert_eq!(second.gauge.profile(), policy.gauge());
    assert_eq!(
        second.original_gauge,
        OriginalGaugeContext::from_source(&original)
    );
    assert_eq!(second.judge.chart().objects(), expected);
    assert_eq!(
        original.source.compile().unwrap().objects(),
        original_objects
    );
}

#[test]
fn selected_stage_policy_and_replay_identity_survive_fresh_repeated_attempts() {
    let original = original();
    let selection = crate::play_policy::TimingPresetSelection::parse(
        "beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1",
        "rank-first",
    )
    .unwrap();
    let policy = ResolvedPlayPolicy::with_timing(
        &original,
        GaugeSelection::Bms(BmsGaugeKind::Hard),
        selection,
        7,
    )
    .unwrap();
    let cfg = config(550_000_000, Some(3_000_000_000), true);
    let first = prepare_attempt(&original, &policy, &launch(true), cfg).unwrap();
    let second = prepare_attempt(&original, &policy, &first.next_launch, cfg).unwrap();
    policy
        .validate_timing(&second.judge, beatkernel_bms::BmsInputMode::ButtonOnly)
        .unwrap();
    assert_eq!(second.timing.as_ref(), policy.timing());
    assert_eq!(first.next_launch.attempt(), 1);
    assert_eq!(second.next_launch.attempt(), 2);
    assert_eq!(
        first.recording_path.as_deref(),
        Some(std::path::Path::new("records/my.take.retry1.bkr"))
    );
    assert_eq!(
        second.recording_path.as_deref(),
        Some(std::path::Path::new("records/my.take.retry2.bkr"))
    );
    let first_capture = first.capture.unwrap();
    let second_capture = second.capture.unwrap();
    assert!(first_capture.records().is_empty());
    assert!(second_capture.records().is_empty());
    assert_eq!(first_capture.header(), second_capture.header());
    let setup =
        crate::replay_playback::decode_section_setup(&second_capture.header().options).unwrap();
    assert_eq!(setup.start, cfg.start);
    assert_eq!(setup.end, cfg.end);
    let file = second_capture.into_file();
    crate::replay_playback::validate_section_setup(&original, &file, cfg.capture_limits.unwrap())
        .unwrap();
    let replay = ReplaySession::from_records(file.header, second.judge, file.records).unwrap();
    assert!(replay.results().is_empty());
}

#[test]
fn week_and_twenty_hour_repeated_targets_preserve_literal_ids_and_transport_anchor() {
    for (horizon, measure_length) in [
        (72_000_000_000_001i64, "300"),
        (604_800_000_000_001, "2520"),
    ] {
        // Genuine BMS timing: BPM1 gives measure0 exactly 20h or one week.
        // At measure1 BPM120000 gives its two-beat midpoint exactly 1ms later.
        // Keep a 1ns off-grid start; do not round it to a source beat or frame.
        let text = format!(
            "#BPM 1\n#BPM01 120000\n#WAV01 k.wav\n#00002:{measure_length}\n#00011:01\n#00108:01\n#00111:0001\n"
        );
        let original = parse(&text, ParseOptions::default()).unwrap();
        let policy = ResolvedPlayPolicy::builtin(11, 23, 0).unwrap();
        let cfg = config(horizon, Some(horizon + 2_000_000), false);
        let first = prepare_attempt(&original, &policy, &launch(false), cfg).unwrap();
        let second = prepare_attempt(&original, &policy, &first.next_launch, cfg).unwrap();
        assert_eq!(
            first.judge.chart().objects(),
            second.judge.chart().objects()
        );
        let object = &second.judge.chart().objects()[0];
        assert_eq!(object.id, original.source.objects[1].id);
        assert_eq!(object.time.start, ts(horizon + 999_999));
        let transport = second.transport_at(ts(900_000_000));
        assert_eq!(transport.position_at(ts(900_000_000)).unwrap(), ts(horizon));
        assert_eq!(
            transport.position_at(ts(900_999_999)).unwrap(),
            ts(horizon + 999_999)
        );
        assert_eq!(second.next_launch.attempt(), 2);
        assert!(second.capture.is_none());
        assert_eq!(original.source.objects[0].start, Beat::new(0).unwrap());
    }
}

#[test]
fn cold_refusals_preserve_existing_owners_and_pinned_identity() {
    let original = original();
    let policy = ResolvedPlayPolicy::builtin(11, 23, 0).unwrap();
    let launch = launch(false);
    let current = prepare_attempt(&original, &policy, &launch, config(0, None, false)).unwrap();
    let hash = current.judge.stable_hash().unwrap();
    let gauge = *current.gauge.snapshot();
    for cfg in [
        config(-1, None, false),
        config(100, Some(100), false),
        config(101, Some(100), false),
        config(0, None, true),
    ] {
        assert!(prepare_attempt(&original, &policy, &current.next_launch, cfg).is_err());
        assert_eq!(current.judge.stable_hash().unwrap(), hash);
        assert_eq!(*current.gauge.snapshot(), gauge);
        assert_eq!(current.next_launch.attempt(), 1);
        assert_eq!(current.score, ScoreSummary::default());
    }
    assert_eq!(launch.attempt(), 0);
    for flag in [
        "--replay",
        "--mp-host",
        "--mp-join",
        "--mp-webtransport",
        "--mp-room",
    ] {
        let launch = SessionLaunch::new(vec![
            "--chart".into(),
            "a.bms".into(),
            flag.into(),
            "value".into(),
        ])
        .unwrap();
        assert!(prepare_attempt(&original, &policy, &launch, config(0, None, false)).is_err());
        assert_eq!(launch.attempt(), 0);
    }
}

struct Identity;
impl beatkernel::time::ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> beatkernel::time::ClockMappingQuality {
        beatkernel::time::ClockMappingQuality::Exact
    }
}

#[test]
fn actual_reused_runtime_clears_old_held_state_and_matches_fresh_attempt_and_replay() {
    use beatkernel::{audio::command_queue, runtime::Runtime};
    let original = original();
    let policy = ResolvedPlayPolicy::builtin(11, 23, 0).unwrap();
    let old = prepare_attempt(&original, &policy, &launch(false), config(0, None, false)).unwrap();
    let binding = || {
        BindingMap::from_bindings([Binding {
            device: DeviceSelector::Exact(DeviceId(3)),
            physical: PhysicalControlId::keyboard(7),
            game_control: GameControlId(0x11),
        }])
        .unwrap()
    };
    let point = |n| ClockPoint {
        domain: ClockDomainId(17),
        timestamp: ts(n),
    };
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut reused = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        old.transport_at(ts(0)),
        binding(),
        old.judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    let head = reused
        .process_input(
            input(0x11, 0, ButtonState::Down, 0).physical,
            &Identity,
            point(0),
        )
        .unwrap();
    assert_eq!(head.judge_events.len(), 1);
    assert_eq!(
        head.judge_events[0].stage,
        beatkernel::judge::JudgeStage::HoldHead
    );
    let cfg = config(550_000_000, Some(3_000_000_000), true);
    let mut next = prepare_attempt(&original, &policy, &launch(true), cfg).unwrap();
    let transport = next.transport_at(ts(10_000_000_000));
    reused.replace_state(next.judge, transport);
    let baseline = prepare_attempt(&original, &policy, &launch(true), cfg).unwrap();
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut fresh = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        baseline.transport_at(ts(10_000_000_000)),
        binding(),
        baseline.judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    let mut expected = Vec::new();
    for (at, state, sequence) in [
        (10_450_000_000, ButtonState::Up, 1),
        (11_450_000_000, ButtonState::Down, 2),
    ] {
        let physical = input(0x11, at, state, sequence).physical;
        let a = reused
            .process_input(physical.clone(), &Identity, point(at))
            .unwrap();
        let b = fresh.process_input(physical, &Identity, point(at)).unwrap();
        assert_eq!(a.song_time, b.song_time);
        assert_eq!(a.judge_events, b.judge_events);
        if state == ButtonState::Up {
            assert!(a.judge_events.is_empty());
        }
        expected.extend(a.judge_events.clone());
        next.capture.as_mut().unwrap().record_report(&a).unwrap();
        next.score.observe(&a.judge_events).unwrap();
        next.gauge
            .observe(&a.judge_events, &a.hazard_events)
            .unwrap();
    }
    assert_eq!(next.score.hits, 1);
    assert_eq!(next.score.misses, 0);
    assert_eq!(
        reused.judge().stable_hash().unwrap(),
        fresh.judge().stable_hash().unwrap()
    );
    let file = next.capture.unwrap().into_file();
    let mut replay =
        crate::replay_playback::reconstruct_section(&original, file, cfg.capture_limits.unwrap())
            .unwrap();
    replay.seek_cursor(2).unwrap();
    assert_eq!(replay.results(), expected);
    assert_eq!(
        replay.engine().stable_hash().unwrap(),
        reused.judge().stable_hash().unwrap()
    );
}
