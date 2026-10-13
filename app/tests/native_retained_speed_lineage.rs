//! Public retry selections re-enter original-chart preparation and real capture.
//! No endpoint, native presentation acknowledgement or cold preroll is simulated.
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    replay::codec::decode_replay,
    runtime::Runtime,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
    transport::OutputPhaseBasis,
};
use beatkernel_bms::{BmsChart, BmsGaugeKind};
use beatkernel_bms_runtime::{
    competition::ScoreSummary,
    competition_live::replay_limits,
    gauge::BmsGauge,
    native_judge::NativeJudgeConfig,
    play_policy::{GaugeSelection, OriginalGaugeContext, ResolvedPlayPolicy},
    practice::PracticeStart,
    practice_loop::PracticeLoop,
    practice_session::{prepare_attempt_with_speed, PracticeAttemptConfig},
    replay_playback::{decode_section_setup, reconstruct_section, validate_section_setup},
    replay_speed::ReplaySpeed,
    session_launch::SessionLaunch,
};
use std::{collections::BTreeSet, path::PathBuf};

const DOMAIN: ClockDomainId = ClockDomainId(71);
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(n: i64) -> ClockPoint {
    ClockPoint {
        domain: DOMAIN,
        timestamp: ts(n),
    }
}
fn fraction() -> ReplaySpeed {
    "3/2".parse().unwrap()
}
fn source() -> BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#RANK 2\n#WAV01 key.wav\n#00011:00010001\n",
        Default::default(),
    )
    .unwrap()
}
fn value<'a>(launch: &'a SessionLaunch, flag: &str) -> &'a str {
    &launch
        .args()
        .chunks_exact(2)
        .find(|p| p[0] == flag)
        .unwrap()[1]
}
fn launch(local: bool) -> SessionLaunch {
    let mut args = [
        "--chart",
        "original.bms",
        "--speed",
        "6/4",
        "--start-ns",
        "2000000000",
        "--end-ns",
        "3500000000",
        "--preroll-ns",
        "100000000",
        "--record-replay",
        "records/take.bkr",
    ]
    .map(str::to_owned)
    .to_vec();
    if local {
        args.extend(
            [
                "--local-player",
                "7:keyboard-a",
                "--local-player",
                "4294967295:keyboard-b",
            ]
            .map(str::to_owned),
        );
    }
    SessionLaunch::new(args).unwrap()
}
fn policy(original: &BmsChart) -> ResolvedPlayPolicy {
    NativeJudgeConfig {
        early: 10_000_000,
        late: 20_000_000,
        offset: 0,
        preroll: 100_000_000,
        output: DOMAIN,
        end: Some(ts(3_500_000_000)),
    }
    .resolve_play_policy(
        &OriginalGaugeContext::from_source(original),
        GaugeSelection::Bms(BmsGaugeKind::Hard),
    )
    .unwrap()
}
fn config(launch: &SessionLaunch) -> PracticeAttemptConfig {
    PracticeAttemptConfig {
        start: ts(value(launch, "--start-ns").parse().unwrap()),
        end: Some(ts(value(launch, "--end-ns").parse().unwrap())),
        domain: DOMAIN,
        chart_seed: 0,
        capture_limits: Some(replay_limits().unwrap()),
    }
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

/// Re-enter preparation after the externally selected invocation. Its next
/// launch is a further recording ordinal; the selected invocation itself stays
/// immutable. Feed an actual Runtime hit plus finite completion into capture.
fn record_attempt(
    original: &BmsChart,
    policy: &ResolvedPlayPolicy,
    selected: &SessionLaunch,
    expected_ordinal: u32,
    base: &str,
    paths: &mut BTreeSet<PathBuf>,
) -> SessionLaunch {
    let selected_args = selected.args().to_vec();
    let config = config(selected);
    let mut attempt =
        prepare_attempt_with_speed(original, policy, selected, config, fraction()).unwrap();
    assert_eq!(attempt.speed(), "6/4".parse::<ReplaySpeed>().unwrap());
    assert_eq!(attempt.next_launch.attempt(), expected_ordinal);
    assert_eq!(
        attempt.next_launch.original_args(),
        selected.original_args()
    );
    assert_eq!(
        attempt.original_gauge,
        OriginalGaugeContext::from_source(original)
    );
    assert_eq!(attempt.gauge.profile(), policy.gauge());
    assert_eq!(
        attempt.gauge.snapshot(),
        BmsGauge::new(policy.gauge().try_copy().unwrap()).snapshot()
    );
    assert_eq!(attempt.score, ScoreSummary::default());
    assert_eq!(attempt.judge.profile(), policy.judge());
    let original_objects = original.source.compile().unwrap().objects().to_vec();
    let kept = original_objects
        .iter()
        .filter(|o| o.time.start >= config.start)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(attempt.judge.chart().objects(), kept);
    assert_eq!(
        attempt.excluded_objects,
        original_objects.len() - kept.len()
    );
    let path = PathBuf::from(format!("records/{base}.retry{expected_ordinal}.bkr"));
    assert_eq!(attempt.recording_path, Some(path.clone()));
    assert!(paths.insert(path));
    assert_eq!(value(&attempt.next_launch, "--speed"), "6/4");
    assert_eq!(value(&attempt.next_launch, "--chart"), "original.bms");
    assert_eq!(
        attempt
            .next_launch
            .args()
            .chunks_exact(2)
            .filter(|p| p[0] == "--local-player")
            .map(|p| p[1].as_str())
            .collect::<Vec<_>>(),
        selected
            .args()
            .chunks_exact(2)
            .filter(|p| p[0] == "--local-player")
            .map(|p| p[1].as_str())
            .collect::<Vec<_>>()
    );
    // Cold invocations retain explicit preroll; retained attempts start at the
    // selected original-song cut with zero preroll on their phase transport.
    assert_eq!(value(&attempt.next_launch, "--preroll-ns"), "100000000");
    assert_eq!(value(selected, "--preroll-ns"), "100000000");
    let basis = OutputPhaseBasis::new(DOMAIN, 43, u64::from(expected_ordinal), 0, 1000).unwrap();
    let transport = attempt
        .transport_at_output_phase(basis, point(1000))
        .unwrap();
    let origin = basis.point_from_elapsed_nanos(point(1000), 0).unwrap();
    assert_eq!(transport.position_at_output_phase(origin), Ok(config.start));
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DeviceId(8)),
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let first_note = kept.first().unwrap().time.start.as_nanos();
    // Independent ceil((note - cut) * 2 / 3) for the selected 3/2 rate.
    let elapsed = ((first_note - config.start.as_nanos()) * 2 + 2) / 3;
    let expected_song = ts(config.start.as_nanos() + elapsed * 3 / 2);
    let next_launch = attempt.next_launch.clone();
    let (producer, _consumer) = command_queue(16).unwrap();
    let mut runtime = Runtime::new(
        DOMAIN,
        DOMAIN,
        transport,
        bindings,
        attempt.judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_song_end(config.end.unwrap()).unwrap();
    let mut events = vec![];
    for (sequence, state) in [(1, ButtonState::Down), (2, ButtonState::Up)] {
        let at = point(1000 + sequence as i64);
        let physical = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(8), at, sequence),
            control: PhysicalControlId::keyboard(4),
            state,
        });
        let report = runtime
            .process_input_output_phase(
                physical,
                &Identity,
                point(2000 + sequence as i64),
                basis
                    .point_from_elapsed_nanos(at, i128::from(elapsed))
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(report.song_time, expected_song);
        assert!(report.judge_error.is_none());
        if state == ButtonState::Down {
            assert_eq!(report.judge_events.len(), 1);
            let event = report.judge_events[0];
            assert_eq!(event.object, kept[0].id);
            assert_eq!(event.stage, beatkernel::judge::JudgeStage::Instant);
            assert!(matches!(
                event.outcome,
                beatkernel::judge::JudgeOutcome::Hit { delta, .. }
                    if delta.as_nanos() == expected_song.as_nanos() - first_note
            ));
            assert_eq!(event.at, expected_song);
            assert_eq!(event.input, Some(EventMeta::new(DeviceId(8), at, sequence)));
        }
        attempt
            .capture
            .as_mut()
            .unwrap()
            .record_report(&report)
            .unwrap();
        attempt.gauge.observe(&report.judge_events, &[]).unwrap();
        attempt.score.observe(&report.judge_events).unwrap();
        events.extend(report.judge_events);
    }
    assert_ne!(attempt.score, ScoreSummary::default());
    let end_report = runtime
        .advance_to_output_phase(
            point(1003),
            &Identity,
            point(2003),
            basis
                .point_from_elapsed_nanos(point(1003), 10_000_000_000)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(end_report.song_time, config.end.unwrap());
    assert!(end_report.song_end_reached);
    attempt
        .capture
        .as_mut()
        .unwrap()
        .record_report(&end_report)
        .unwrap();
    events.extend(end_report.judge_events);
    let bytes = attempt.capture.unwrap().into_bytes().unwrap();
    let file = decode_replay(&bytes, replay_limits().unwrap()).unwrap();
    assert_eq!(file.records.len(), 3);
    assert_eq!(file.records[0].song_time, expected_song);
    assert_eq!(file.records[2].song_time, config.end.unwrap());
    let setup = decode_section_setup(&file.header.options).unwrap();
    assert_eq!(setup.source_speed, fraction());
    assert_eq!(setup.start, config.start);
    assert_eq!(setup.end, config.end);
    assert_eq!(&setup.profile, policy.judge());
    assert_eq!(&setup.gauge, policy.gauge());
    validate_section_setup(original, &file, replay_limits().unwrap()).unwrap();
    let mut replay = reconstruct_section(original, file, replay_limits().unwrap()).unwrap();
    replay.seek_cursor(3).unwrap();
    assert_eq!(replay.results(), events);
    assert_eq!(
        replay.engine().stable_hash().unwrap(),
        runtime.judge().stable_hash().unwrap()
    );
    assert_eq!(selected.args(), selected_args);
    next_launch
}

fn chain(
    original: &BmsChart,
    policy: &ResolvedPlayPolicy,
    pinned: &SessionLaunch,
    base: &str,
    paths: &mut BTreeSet<PathBuf>,
) {
    let original_args = pinned.args().to_vec();
    let f5 = pinned.retry().unwrap();
    assert_eq!(f5.attempt(), 1);
    let first = record_attempt(original, policy, &f5, 2, base, paths);
    let bookmark = first
        .retry_from(PracticeStart::from_nanoseconds(500_000_000).unwrap())
        .unwrap();
    assert_eq!(value(&bookmark, "--start-ns"), "500000000");
    let second = record_attempt(original, policy, &bookmark, 4, base, paths);
    let region = PracticeLoop::new(
        PracticeStart::from_nanoseconds(750_000_000).unwrap(),
        PracticeStart::from_nanoseconds(1_500_000_000).unwrap(),
    )
    .unwrap();
    let selected_loop = second.retry_loop(region).unwrap();
    let third = record_attempt(original, policy, &selected_loop, 6, base, paths);
    // Ordinary F5 restores pinned endpoints after the bookmark and finite loop.
    let restored = third.retry().unwrap();
    assert_eq!(restored.attempt(), 7);
    assert_eq!(value(&restored, "--start-ns"), "2000000000");
    assert_eq!(value(&restored, "--end-ns"), "3500000000");
    assert_eq!(
        value(&restored, "--record-replay"),
        format!("records/{base}.retry7.bkr")
    );
    assert_eq!(pinned.args(), original_args);
    assert_eq!(pinned.attempt(), 0);
}

#[test]
fn solo_f5_backward_bookmark_and_loop_roundtrip_actual_accepted_song_points() {
    let original = source();
    let objects = original.source.compile().unwrap().objects().to_vec();
    chain(
        &original,
        &policy(&original),
        &launch(false),
        "take",
        &mut BTreeSet::new(),
    );
    assert_eq!(original.source.compile().unwrap().objects(), objects);
}

#[test]
fn local_members_keep_roster_and_disjoint_original_base_across_retry_chains() {
    let original = source();
    let p = policy(&original);
    let global = launch(true);
    let global_args = global.args().to_vec();
    let mut paths = BTreeSet::new();
    for member in [7_u32, u32::MAX] {
        let base = format!("take.p{member}");
        let member_launch = global
            .for_recording_path(
                format!("records/{base}.bkr").into(),
                format!("records/{base}.bkr").into(),
            )
            .unwrap();
        chain(&original, &p, &member_launch, &base, &mut paths);
        assert_eq!(
            member_launch
                .args()
                .chunks_exact(2)
                .filter(|p| p[0] == "--local-player")
                .map(|p| p[1].as_str())
                .collect::<Vec<_>>(),
            ["7:keyboard-a", "4294967295:keyboard-b"]
        );
    }
    assert_eq!(paths.len(), 6);
    assert_eq!(global.args(), global_args);
    assert_eq!(global.attempt(), 0);
}

#[test]
fn rejected_selection_and_rate_do_not_mutate_active_lineage_or_capture() {
    let original = source();
    let p = policy(&original);
    let pinned = launch(false);
    let selected = pinned
        .retry_from(PracticeStart::from_nanoseconds(500_000_000).unwrap())
        .unwrap();
    let active =
        prepare_attempt_with_speed(&original, &p, &selected, config(&selected), fraction())
            .unwrap();
    let args = active.next_launch.args().to_vec();
    let original_args = active.next_launch.original_args().to_vec();
    let hash = active.judge.stable_hash().unwrap();
    let gauge = *active.gauge.snapshot();
    let header = active.capture.as_ref().unwrap().header().clone();
    assert!(prepare_attempt_with_speed(
        &original,
        &p,
        &active.next_launch,
        config(&selected),
        "2/1".parse().unwrap()
    )
    .is_err());
    let foreign = SessionLaunch::new(vec!["--chart".into(), "different.bms".into()])
        .unwrap()
        .retry()
        .unwrap();
    assert!(active.next_launch.rebase_prepared_retry(&foreign).is_err());
    let mut bad = config(&selected);
    bad.start = bad.end.unwrap();
    assert!(
        prepare_attempt_with_speed(&original, &p, &active.next_launch, bad, fraction()).is_err()
    );
    assert_eq!(active.next_launch.args(), args);
    assert_eq!(active.next_launch.original_args(), original_args);
    assert_eq!(active.next_launch.attempt(), 2);
    assert_eq!(active.judge.stable_hash().unwrap(), hash);
    assert_eq!(*active.gauge.snapshot(), gauge);
    assert_eq!(active.capture.as_ref().unwrap().header(), &header);
    assert_eq!(active.score, ScoreSummary::default());
    assert_eq!(pinned.attempt(), 0);
    // A loop selected before an intervening retained attempt is rebased against
    // the joined owner's final ordinal, preserving selected cuts and base.
    let region = PracticeLoop::new(
        PracticeStart::from_nanoseconds(750_000_000).unwrap(),
        PracticeStart::from_nanoseconds(1_500_000_000).unwrap(),
    )
    .unwrap();
    let early = active.next_launch.retry_loop(region).unwrap();
    let advanced = active.next_launch.retry().unwrap().retry().unwrap();
    let rebased = advanced.rebase_prepared_retry(&early).unwrap();
    assert_eq!(rebased.attempt(), 5);
    assert_eq!(value(&rebased, "--start-ns"), "750000000");
    assert_eq!(value(&rebased, "--end-ns"), "1500000000");
    assert_eq!(
        value(&rebased, "--record-replay"),
        "records/take.retry5.bkr"
    );
    record_attempt(&original, &p, &rebased, 6, "take", &mut BTreeSet::new());
}
