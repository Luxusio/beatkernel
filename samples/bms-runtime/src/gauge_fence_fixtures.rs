//! Deferred actual stepped owners and their canonical captures. Typed PCM setup
//! does not admit mine files or establish native/browser playback completion.
use crate::{
    PreparedBms,
    competition::ScoreSummary,
    gauge::{BmsGauge, GaugeFailure},
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    replay_capture::CaptureError,
    replay_playback::reconstruct,
    replay_visual::ReplayVisual,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError, StepLocalGameplay},
    step_replay::{StepReplay, StepReplayConfig},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, PcmLimits, PcmSample, QueuePushError, SampleBank, SampleId,
        VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, NativeEventMeta, BackendId, PhysicalControlId,
        PhysicalInputEvent,
    },
    interaction::{InputOwner, InteractionState},
    judge::{HazardOutcome, JudgeStage},
    replay::{
        ReplayOperation,
        codec::{decode_replay, ReplayCodecError, ReplayCodecLimits},
    },
    runtime::{RuntimeError, RuntimeReport, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{parse, BmsChart, BmsInputMode};

const SECOND: i64 = 1_000_000_000;
const HOST: i64 = 10 * SECOND;
const OUTPUT: i64 = 20 * SECOND;
const FATAL: &str = "#BPM 60\n#LNTYPE 1\n#WAV00 mine\n#WAV01 head\n#WAV02 press\n#00051:01000001\n#00012:00000100\n#00032:02\n#000D1:ZZ000001";
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(1, HOST),
        output_origin: point(2, OUTPUT),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 16,
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(SECOND),
        telemetry_capacity: 0,
    }
}
fn limits(records: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, records, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
struct NoMapping;
impl ClockMapper for NoMapping {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn bindings(device: DeviceSelector, lanes: &[u32]) -> BindingMap {
    BindingMap::from_bindings(lanes.iter().map(|&lane| Binding {
        device,
        physical: PhysicalControlId::keyboard(91u16),
        game_control: GameControlId(lane),
    }))
    .unwrap()
}
fn button(device: u64, song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(device), point(1, HOST + song), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(0x5749_4e),
        code: Some(u32::MAX),
        timestamp: Some(point(7, -4)),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(91u16),
        state,
    })
}
fn prepared(source: BmsChart) -> PreparedBms {
    let compiled = source.compile().unwrap();
    assert!(compiled.bgm.is_empty());
    let format = AudioFormat::new(10, 1).unwrap();
    let pcm = PcmLimits::new(64, 256, 4).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    for id in [0, 1, 2] {
        if source.samples.contains_key(&id) {
            bank.insert(
                SampleId(u64::from(id)),
                PcmSample::new(format, vec![0.25, -0.25], pcm).unwrap(),
            )
            .unwrap();
        }
    }
    let sounds = source
        .notes
        .iter()
        .map(|note| {
            let object = compiled
                .chart
                .objects()
                .iter()
                .find(|object| object.id == note.object)
                .unwrap();
            SoundBinding {
                object: note.object,
                stage: if object.time.end.is_some() {
                    JudgeStage::HoldHead
                } else {
                    JudgeStage::Instant
                },
                sample: note.sample,
                voice: VoiceId(100 + u64::from(note.lane.control().0)),
                gain: 1.0,
            }
        })
        .collect();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands: vec![],
    }
}
fn solo(source: &BmsChart, lanes: &[u32], cfg: StepGameplayConfig, records: usize) -> StepGameplay {
    let (mut game, _) = StepGameplay::new(
        prepared(source.clone()),
        cfg,
        bindings(DeviceSelector::Any, lanes),
    )
    .unwrap();
    game.configure_capture(limits(records), 0).unwrap();
    game.activate(cfg.host_origin).unwrap();
    game
}
fn fatal_game() -> (BmsChart, StepGameplay, RuntimeReport) {
    let source = parse(FATAL, Default::default()).unwrap();
    let mut game = solo(&source, &[0x11, 0x12], config(), 64);
    let report = game
        .process_input(
            button(u64::MAX, 0, 5, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    (source, game, report)
}
fn empty(report: &RuntimeReport, frozen: i64) {
    assert_eq!(report.song_time, ts(frozen));
    assert!(
        report.bound_inputs.is_empty()
            && report.judge_events.is_empty()
            && report.hazard_events.is_empty()
    );
    assert!(
        report.audio_commands.is_empty()
            && report.audio_failures.is_empty()
            && report.judge_error.is_none()
    );
}

#[test]
fn fatal_operation_finishes_its_real_fanout_then_freezes_hold_pending_work_and_automatic_audio_only()
 {
    let (_, mut game, first) = fatal_game();
    assert_eq!(
        (
            first.bound_inputs.len(),
            first.judge_events.len(),
            first.hazard_events.len()
        ),
        (2, 1, 1)
    );
    assert_eq!(first.judge_events[0].stage, JudgeStage::HoldHead);
    assert_eq!(first.hazard_events[0].value, 1295);
    assert_eq!(first.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert_eq!(first.audio_commands.len(), 2);
    assert!(
        matches!(first.audio_commands[0], AudioCommand::Play { sample: SampleId(1), at, .. } if at == ts(OUTPUT))
    );
    assert!(
        matches!(first.audio_commands[1], AudioCommand::Play { sample: SampleId(2), at, .. } if at == ts(OUTPUT))
    );
    assert_eq!(game.gameplay_fence(), Some(ts(0)));
    assert!(!game.failed());
    assert_eq!(
        (
            game.gauge().snapshot().level_units,
            game.gauge().snapshot().failure
        ),
        (0, Some(GaugeFailure::InstantDeath))
    );
    assert_eq!(game.score().hits, 1);
    assert!(game.mine_damage().instant_death);
    let hash = game.judge().stable_hash().unwrap();
    let gauge = game.gauge().clone();
    let damage = *game.mine_damage();
    let hold = first.judge_events[0].object;
    assert_eq!(game.judge().state(hold), Some(InteractionState::Active));
    assert_eq!(game.judge().remaining_hazards(), 1);
    assert!(game.judge().is_held(InputOwner {
        source: DeviceId(u64::MAX),
        physical: PhysicalControlId::keyboard(91u16),
        game_control: GameControlId(0x11)
    }));
    for (song, sequence, state) in [
        (SECOND, 6, ButtonState::Up),
        (2 * SECOND, 7, ButtonState::Down),
        (3 * SECOND, 8, ButtonState::Up),
    ] {
        let input = button(u64::MAX, song, sequence, state);
        let report = game
            .process_input(input.clone(), &NoMapping, point(2, OUTPUT + song))
            .unwrap();
        empty(&report, 0);
        assert_eq!(report.input, Some(input));
    }
    for song in [3 * SECOND, 4 * SECOND, 8 * SECOND] {
        let report = game
            .advance_to(point(1, HOST + song), &NoMapping, point(2, OUTPUT + song))
            .unwrap();
        empty(&report, 0);
        assert!(report.input.is_none() && !report.song_end_reached);
    }
    assert_eq!(game.song_time(), ts(0));
    assert_eq!(game.gauge(), &gauge);
    assert_eq!(*game.mine_damage(), damage);
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
    assert_eq!((game.score().hits, game.score().misses), (1, 0));
    let batch = game.take_commands(16).unwrap().unwrap();
    assert_eq!(batch.commands, first.audio_commands);
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    assert!(game.take_commands(16).unwrap().is_none());
    assert!(!game.failed()); // Numeric failure neither flushes PCM nor fabricates output completion.

    for kind in 0..4 {
        let (_, mut owner, _) = fatal_game();
        let hash = owner.judge().stable_hash().unwrap();
        let mut invalid = button(u64::MAX, SECOND, 6, ButtonState::Up);
        let mut output = point(2, OUTPUT);
        match kind {
            0 => invalid.meta_mut().clock_domain = ClockDomainId(99),
            1 => invalid.meta_mut().sequence = 4,
            2 => invalid.meta_mut().timestamp = ts(HOST - 1),
            _ => output.domain = ClockDomainId(99),
        }
        let StepGameplayError::Runtime(error) = owner
            .process_input(invalid, &NoMapping, output)
            .unwrap_err()
        else {
            panic!("actual acquisition refusal")
        };
        assert!(matches!(
            error.kind,
            crate::local_runtime::FailureKind::Core(
                RuntimeError::UnmappedClock { .. }
                    | RuntimeError::SequenceRegression { .. }
                    | RuntimeError::NonMonotonicHost
            )
        ));
        assert_eq!(owner.gameplay_fence(), Some(ts(0)));
        assert_eq!(owner.judge().stable_hash().unwrap(), hash);
        assert!(owner.failed());
    }
    let recoverable = parse(
        "#BPM 60\n#WAV01 head\n#00011:01010000\n#000D1:1E",
        Default::default(),
    )
    .unwrap();
    let mut live = solo(&recoverable, &[0x11], config(), 64);
    live.process_input(
        button(3, 0, 0, ButtonState::Down),
        &NoMapping,
        point(2, OUTPUT),
    )
    .unwrap();
    assert_eq!(live.gauge().snapshot().level_units, 0);
    assert_eq!(live.gauge().snapshot().failure, None);
    assert_eq!(live.gameplay_fence(), None);
    live.process_input(
        button(3, SECOND / 2, 1, ButtonState::Up),
        &NoMapping,
        point(2, OUTPUT),
    )
    .unwrap();
    let resumed = live
        .process_input(
            button(3, SECOND, 2, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT + SECOND),
        )
        .unwrap();
    assert_eq!(resumed.judge_events.len(), 1);
    assert_eq!(live.gauge().snapshot().level_units, 1_000_000);
    assert_eq!(live.gameplay_fence(), None);
}

#[test]
fn sealed_failure_capture_reconstructs_exact_hash_score_gauge_and_held_state_without_later_display_judging()
 {
    let (source, mut game, first) = fatal_game();
    let hash = game.judge().stable_hash().unwrap();
    let score = game.score().clone();
    let gauge = game.gauge().clone();
    let damage = *game.mine_damage();
    let header = {
        // This is the configured capture's existing wire identity, not a new failure-policy format.
        let fresh = solo(&source, &[0x11, 0x12], config(), 64);
        fresh.competition_header(limits(64), 0).unwrap()
    };
    for song in [0, SECOND, 8 * SECOND] {
        empty(
            &game
                .advance_to(point(1, HOST + song), &NoMapping, point(2, OUTPUT + song))
                .unwrap(),
            0,
        );
    }
    empty(
        &game
            .process_input(
                button(u64::MAX, 8 * SECOND, 6, ButtonState::Up),
                &NoMapping,
                point(2, OUTPUT + 8 * SECOND),
            )
            .unwrap(),
        0,
    );
    assert!(!game.failed());
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits(64)).unwrap();
    assert_eq!(file.header, header);
    assert_eq!(file.records.len(), 2);
    assert!(game.take_replay().unwrap().is_none());
    for (record, input) in file.records.iter().zip(&first.bound_inputs) {
        assert_eq!(record.song_time, ts(0));
        let ReplayOperation::Input(recorded) = &record.operation else {
            panic!("only actual admitted bound inputs")
        };
        assert_eq!(recorded, input);
        assert_eq!(recorded.physical.meta().source, DeviceId(u64::MAX));
        assert_eq!(recorded.physical.meta().sequence, 5);
    }
    let rebuilt = reconstruct(&source, file.clone(), limits(64)).unwrap();
    assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
    assert_eq!(rebuilt.engine().remaining_hazards(), 1);
    let mut observed = ScoreSummary::default();
    observed.observe(rebuilt.results()).unwrap();
    assert_eq!(observed, score);
    let mut visual = ReplayVisual::new(&source, &file, limits(64)).unwrap();
    assert!(visual.advance_to(ts(-1)).unwrap().is_empty());
    assert_eq!(visual.gauge(), &BmsGauge::default());
    assert_eq!(visual.advance_to(ts(0)).unwrap(), first.judge_events);
    assert_eq!(visual.gauge(), &gauge);
    assert_eq!(*visual.mine_damage(), damage);
    assert_eq!(visual.recorded_until(), Some(ts(0)));
    assert_eq!(visual.pressed_lanes(), 3);
    for song in [0, SECOND, 9 * SECOND] {
        assert!(visual.advance_to(ts(song)).unwrap().is_empty());
        assert_eq!(visual.gauge(), &gauge);
        assert_eq!(*visual.mine_damage(), damage);
        assert_eq!(visual.pressed_lanes(), 3);
    }
    let (mut replay, _) = StepReplay::new(
        prepared(source),
        file,
        limits(64),
        StepReplayConfig {
            output_origin: config().output_origin,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(SECOND),
            max_pending: 8,
        },
    )
    .unwrap();
    replay.observe_output(None, None).unwrap();
    assert_eq!(replay.gauge(), &BmsGauge::default());
    replay.observe_output(None, Some(point(2, OUTPUT))).unwrap();
    assert_eq!(replay.drain_events(), first.judge_events);
    for output in [
        Some(point(2, OUTPUT)),
        None,
        Some(point(2, OUTPUT + 9 * SECOND)),
    ] {
        replay.observe_output(None, output).unwrap();
        assert!(replay.drain_events().is_empty());
        assert_eq!(replay.gauge(), &gauge);
        assert_eq!(replay.score(), &score);
        assert_eq!(*replay.mine_damage(), damage);
        assert_eq!(replay.pressed_lanes(), 3);
    }
}

#[test]
fn last_roster_members_failure_seals_only_its_capture_and_cannot_regress_surviving_shared_progress()
{
    let source = parse(
        "#BPM 60\n#WAV01 head\n#00011:01000100\n#000D1:00ZZ0001",
        Default::default(),
    )
    .unwrap();
    for count in [2, 3, 4] {
        let players: Vec<_> = (0..count)
            .map(|index| {
                PlayerId(if index + 1 == count {
                    u32::MAX
                } else {
                    9 + index as u32 * 37
                })
            })
            .collect();
        let devices: Vec<_> = (0..count)
            .map(|index| u64::MAX - index as u64 * 19)
            .collect();
        let plan = ResolvedInputPlan::new(
            players
                .iter()
                .zip(&devices)
                .map(|(&player, &device)| (player, Some(DeviceId(device))))
                .collect(),
        )
        .unwrap();
        let maps = devices
            .iter()
            .map(|&device| bindings(DeviceSelector::Exact(DeviceId(device)), &[0x11]))
            .collect();
        let (mut game, bank) = StepLocalGameplay::new_section(
            prepared(source.clone()),
            config(),
            plan,
            maps,
            ts(0),
            None,
            BmsInputMode::ButtonOnly,
        )
        .unwrap();
        assert_eq!(bank.len(), 1);
        assert_eq!(game.gameplay_fence(PlayerId(0)), None);
        for &player in &players {
            game.configure_capture(player, limits(64), 0).unwrap();
        }
        game.activate(config().host_origin).unwrap();
        let mut admitted = Vec::new();
        for (index, &device) in devices.iter().enumerate() {
            let InputResult::Processed(reports) = game
                .process_input(
                    button(device, 0, 1, ButtonState::Down),
                    &NoMapping,
                    point(2, OUTPUT),
                )
                .unwrap()
            else {
                panic!("actual assigned source")
            };
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].player, players[index]);
            assert_eq!(reports[0].report.judge_events.len(), 1);
            admitted.extend(reports[0].report.audio_commands.iter().copied());
        }
        for &device in &devices[..count - 1] {
            game.process_input(
                button(device, SECOND / 2, 2, ButtonState::Up),
                &NoMapping,
                point(2, OUTPUT + SECOND / 2),
            )
            .unwrap();
        }
        let reports = game
            .advance_to(
                point(1, HOST + SECOND),
                &NoMapping,
                point(2, OUTPUT + SECOND),
            )
            .unwrap();
        let dead = players[count - 1];
        for (index, report) in reports.iter().enumerate() {
            assert_eq!(report.player, players[index]);
            assert_eq!(report.report.hazard_events.len(), 1);
            assert_eq!(
                report.report.hazard_events[0].outcome,
                if index + 1 == count {
                    HazardOutcome::Triggered
                } else {
                    HazardOutcome::Avoided
                }
            );
            assert_eq!(
                game.gameplay_fence(players[index]),
                if index + 1 == count {
                    Some(ts(SECOND))
                } else {
                    None
                }
            );
        }
        assert!(!game.failed());
        let dead_hash = game.judge(dead).unwrap().stable_hash().unwrap();
        for (index, &device) in devices[..count - 1].iter().enumerate() {
            let InputResult::Processed(reports) = game
                .process_input(
                    button(device, 2 * SECOND, 3, ButtonState::Down),
                    &NoMapping,
                    point(2, OUTPUT + 2 * SECOND),
                )
                .unwrap()
            else {
                panic!("surviving exact member")
            };
            assert_eq!(reports[0].report.judge_events.len(), 1);
            assert_eq!(game.score(players[index]).unwrap().hits, 2);
            admitted.extend(reports[0].report.audio_commands.iter().copied());
        }
        assert_eq!(game.song_time(), ts(2 * SECOND));
        let input = button(devices[count - 1], 2 * SECOND, 2, ButtonState::Up);
        let InputResult::Processed(frozen) = game
            .process_input(input.clone(), &NoMapping, point(2, OUTPUT + 2 * SECOND))
            .unwrap()
        else {
            panic!("failed source remains acquired")
        };
        empty(&frozen[0].report, SECOND);
        assert_eq!(frozen[0].report.input, Some(input));
        assert_eq!(game.song_time(), ts(2 * SECOND));
        let reports = game
            .advance_to(
                point(1, HOST + 3 * SECOND),
                &NoMapping,
                point(2, OUTPUT + 3 * SECOND),
            )
            .unwrap();
        empty(&reports[count - 1].report, SECOND);
        assert_eq!(game.song_time(), ts(3 * SECOND));
        assert_eq!(game.member_song_time(dead), Some(ts(SECOND)));
        assert_eq!(game.judge(dead).unwrap().stable_hash().unwrap(), dead_hash);
        let progress = game.group_progress().unwrap();
        assert_eq!(progress[count - 1].progress.song_ns, SECOND);
        assert_eq!(progress[0].progress.song_ns, 3 * SECOND);
        assert_eq!(
            game.gauge(dead).unwrap().snapshot().failure,
            Some(GaugeFailure::InstantDeath)
        );
        for &player in &players[..count - 1] {
            assert_eq!(
                game.gauge(player).unwrap().snapshot().level_units,
                21_500_000
            );
            assert_eq!(game.gameplay_fence(player), None);
            assert_eq!(game.mine_damage(player).unwrap().half_percent_damage, 1);
        }
        let batch = game.take_commands(16).unwrap().unwrap();
        assert_eq!(batch.commands, admitted);
        let voices: Vec<_> = admitted[..count]
            .iter()
            .map(|command| match command {
                AudioCommand::Play { voice, .. } => *voice,
                _ => panic!("head audio"),
            })
            .collect();
        for (index, voice) in voices.iter().enumerate() {
            assert!(!voices[..index].contains(voice));
        }
        game.acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        assert!(game.take_commands(16).unwrap().is_none());
        assert!(!game.failed());
        game.fail();
        for (index, &player) in players.iter().enumerate() {
            let file =
                decode_replay(&game.take_replay(player).unwrap().unwrap(), limits(64)).unwrap();
            assert_eq!(file.records.len(), if index + 1 == count { 2 } else { 5 });
            assert_eq!(
                file.records.last().unwrap().song_time,
                if index + 1 == count {
                    ts(SECOND)
                } else {
                    ts(3 * SECOND)
                }
            );
            let rebuilt = reconstruct(&source, file.clone(), limits(64)).unwrap();
            assert_eq!(
                rebuilt.engine().stable_hash().unwrap(),
                game.judge(player).unwrap().stable_hash().unwrap()
            );
            let mut score = ScoreSummary::default();
            score.observe(rebuilt.results()).unwrap();
            assert_eq!(&score, game.score(player).unwrap());
            let mut visual = ReplayVisual::new(&source, &file, limits(64)).unwrap();
            visual.advance_to(ts(9 * SECOND)).unwrap();
            assert_eq!(visual.gauge(), game.gauge(player).unwrap());
            assert_eq!(visual.mine_damage(), game.mine_damage(player).unwrap());
            assert!(game.take_replay(player).unwrap().is_none());
        }
    }
}

#[test]
fn fatal_report_with_audio_or_capture_failure_preserves_each_observation_and_installs_the_gameplay_fence()
 {
    let source = parse(
        "#BPM 60\n#WAV01 head\n#WAV02 press\n#00011:01\n#00012:01\n#00033:02\n#000D1:ZZ",
        Default::default(),
    )
    .unwrap();
    for (capacity, max_records) in [(2, 1), (8, 1), (2, 64)] {
        let mut cfg = config();
        cfg.command_capacity = capacity;
        let mut game = solo(&source, &[0x11, 0x12, 0x13], cfg, max_records);
        let error = game
            .process_input(
                button(3, 0, 1, ButtonState::Down),
                &NoMapping,
                point(2, OUTPUT),
            )
            .unwrap_err();
        let (report, capture) = match error {
            StepGameplayError::Report {
                report,
                score_error,
                capture_error,
            } => {
                assert_eq!(capacity, 2);
                assert!(score_error.is_none());
                (report, capture_error)
            }
            StepGameplayError::Capture {
                error,
                report: Some(report),
            } => {
                assert_eq!(capacity, 8);
                (report, Some(error))
            }
            other => panic!("actual committed error evidence: {other:?}"),
        };
        assert_eq!(
            (
                report.bound_inputs.len(),
                report.judge_events.len(),
                report.hazard_events.len()
            ),
            (3, 2, 1)
        );
        assert!(report.judge_error.is_none());
        assert_eq!(report.hazard_events[0].value, 1295);
        assert_eq!(
            report.audio_commands.len(),
            if capacity == 2 { 2 } else { 3 }
        );
        assert_eq!(report.audio_failures.len(), usize::from(capacity == 2));
        if capacity == 2 {
            assert_eq!(report.audio_failures[0].reason, QueuePushError::Full);
            assert!(matches!(
                report.audio_failures[0].command,
                AudioCommand::Play {
                    sample: SampleId(2),
                    ..
                }
            ));
        }
        if max_records == 1 {
            assert!(matches!(
                capture,
                Some(CaptureError::Codec(ReplayCodecError::TooManyRecords))
            ));
        } else {
            assert!(capture.is_none());
        }
        assert!(game.failed());
        assert_eq!(game.gameplay_fence(), Some(ts(0)));
        assert_eq!((game.score().hits, game.score().misses), (2, 0));
        assert_eq!(game.mine_damage().triggered, 1);
        assert!(game.mine_damage().instant_death);
        assert_eq!(
            (
                game.gauge().snapshot().level_units,
                game.gauge().snapshot().failure
            ),
            (0, Some(GaugeFailure::InstantDeath))
        );
        let hash = game.judge().stable_hash().unwrap();
        let gauge = game.gauge().clone();
        assert!(matches!(
            game.advance_to(
                point(1, HOST + SECOND),
                &NoMapping,
                point(2, OUTPUT + SECOND)
            ),
            Err(StepGameplayError::Failed)
        ));
        assert_eq!(game.judge().stable_hash().unwrap(), hash);
        assert_eq!(game.gauge(), &gauge);
        let file =
            decode_replay(&game.take_replay().unwrap().unwrap(), limits(max_records)).unwrap();
        assert_eq!(file.records.len(), if max_records == 1 { 0 } else { 3 });
        let rebuilt = reconstruct(&source, file.clone(), limits(max_records)).unwrap();
        if max_records == 1 {
            assert_ne!(rebuilt.engine().stable_hash().unwrap(), hash); // Rejected capture batch is never relabeled as a complete recording.
            assert!(rebuilt.results().is_empty());
        } else {
            assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
            let mut visual = ReplayVisual::new(&source, &file, limits(max_records)).unwrap();
            visual.advance_to(ts(SECOND)).unwrap();
            assert_eq!(visual.gauge(), &gauge);
            assert_eq!(visual.mine_damage(), game.mine_damage());
        }
    }
}
