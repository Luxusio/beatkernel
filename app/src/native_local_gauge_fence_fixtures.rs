// Deferred common native cohort-owner fixtures. High source IDs, actual bound
// reports and portable PCM are used without native input/output acquisition.
use super::*;
use crate::gauge::GaugeFailure;

const PLAYERS: [PlayerId; 3] = [PlayerId(7), PlayerId(101), PlayerId(u32::MAX)];
const SOURCES: [u64; 3] = [u64::MAX - 2, u64::MAX - 1, u64::MAX];

pub(super) fn cohort_fixture(
    text: &str,
    lanes: &[u32],
    queue_capacity: usize,
    capture_records: usize,
) -> Fixture {
    let mut fixture = Fixture::new(false, 8);
    let source = beatkernel_bms::parse(text, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let mut configs = Vec::new();
    let mut states = Vec::new();
    for (index, (&player, &source_id)) in PLAYERS.iter().zip(&SOURCES).enumerate() {
        let judge = crate::native_judge::NativeJudgeConfig {
            early: 0,
            late: 0,
            offset: 0,
            preroll: 0,
            output: ClockDomainId(2),
            end: None,
        }
        .judge(&source, compiled.chart.clone())
        .unwrap();
        let capture = crate::native_judge::prepare_capture_for_source(
            &source,
            &judge,
            ClockDomainId(1),
            Timestamp::ZERO,
            0,
            Some(
                ReplayCodecLimits::new(
                    65536,
                    capture_records,
                    4096,
                    CodecLimits::new(4096, 4096).unwrap(),
                )
                .unwrap(),
            ),
        )
        .unwrap();
        states.push(PlayerState {
            player,
            capture,
            competition: None,
            completion: None,
            score: ScoreSummary::default(),
            gauge: BmsGauge::default(),
            last_song: Timestamp::ZERO,
        });
        configs.push(MemberConfig {
            player,
            device: Some(DeviceId(source_id)),
            bindings: BindingMap::from_bindings(lanes.iter().map(|&lane| Binding {
                device: DeviceSelector::Exact(DeviceId(source_id)),
                physical: PhysicalControlId::keyboard(4u16),
                game_control: GameControlId(lane),
            }))
            .unwrap(),
            judge,
            sounds: source
                .notes
                .iter()
                .map(|note| SoundBinding {
                    object: note.object,
                    stage: beatkernel::judge::JudgeStage::Instant,
                    sample: SampleId(1),
                    voice: VoiceId(1 + index as u64 * 2 + u64::from(note.lane.control().0 - 0x11)),
                    gain: 1.0,
                })
                .collect(),
        });
    }
    let (producer, consumer) = command_queue(queue_capacity).unwrap();
    fixture.group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        producer,
        configs,
        0,
        &[],
    )
    .unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
    )
    .unwrap();
    fixture.device.mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(queue_capacity, 8, 16, 128, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    fixture.states = states;
    fixture.merger = InputMerger::new(
        ClockDomainId(1),
        host(0),
        SOURCES.map(DeviceId).to_vec(),
        16,
    )
    .unwrap();
    fixture.source = source;
    fixture
}

fn input_reports(
    fixture: &mut Fixture,
    member: usize,
    ns: i64,
    sequence: u64,
    state: ButtonState,
) -> Vec<PlayerReport> {
    let InputResult::Processed(reports) = fixture
        .group
        .process_input(
            button(SOURCES[member], ns, sequence, state),
            &ExplicitDomains,
            output(ns),
        )
        .unwrap()
    else {
        panic!("exact admitted source must route to its member");
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].player, PLAYERS[member]);
    reports
}

fn observe_actual(fixture: &mut Fixture, reports: &mut [PlayerReport]) -> NativeGameplayResult<()> {
    observe_reports(reports, &mut fixture.states, &mut fixture.group, None)
}

fn latest(viewer: &player::PlayerViewer) -> player::PlayerSnapshot {
    player::publish_pause(PauseState::Paused);
    player::publish_pause(PauseState::Running);
    viewer.take_latest().unwrap()
}

fn survivor_case(viewer: Option<&player::PlayerViewer>) {
    let mut fixture = cohort_fixture(
        "#BPM 3000\n#WAV01 head.wav\n#00011:01000100\n#000D1:00ZZ0001\n",
        &[0x11],
        16,
        128,
    );
    if viewer.is_some() {
        player::publish_local_chart(
            &fixture.source,
            &fixture.source.compile().unwrap().chart,
            &PLAYERS,
        )
        .unwrap();
    }
    for member in 0..3 {
        let mut reports = input_reports(&mut fixture, member, 0, 1, ButtonState::Down);
        observe_actual(&mut fixture, &mut reports).unwrap();
    }
    for member in 1..3 {
        let mut reports = input_reports(&mut fixture, member, 10_000_000, 2, ButtonState::Up);
        observe_actual(&mut fixture, &mut reports).unwrap();
    }
    let mut reports = fixture
        .group
        .advance_to(host(20_000_000), &ExplicitDomains, output(20_000_000))
        .unwrap();
    assert_eq!(
        reports
            .iter()
            .map(|report| report.player)
            .collect::<Vec<_>>(),
        PLAYERS
    );
    assert!(
        reports
            .iter()
            .all(|report| report.report.hazard_events.len() == 1)
    );
    assert_eq!(
        reports[0].report.hazard_events[0].outcome,
        beatkernel::judge::HazardOutcome::Triggered
    );
    assert!(
        reports[1..]
            .iter()
            .all(|report| report.report.hazard_events[0].outcome
                == beatkernel::judge::HazardOutcome::Avoided)
    );
    observe_actual(&mut fixture, &mut reports).unwrap();
    assert_eq!(
        reports[0].report.audio_commands,
        [AudioCommand::Stop {
            voice: VoiceId(1),
            at: Timestamp::from_nanos(20_000_000),
        }]
    );
    assert!(
        reports
            .iter()
            .all(|report| report.report.audio_failures.is_empty())
    );
    assert!(
        reports[1..]
            .iter()
            .all(|report| report.report.audio_commands.is_empty())
    );
    assert!(!fixture.group.poisoned());
    assert_eq!(
        fixture.group.player_gameplay_fence(PLAYERS[0]),
        Some(Timestamp::from_nanos(20_000_000))
    );
    assert_eq!(
        fixture.states[0].gauge.snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    for state in &fixture.states[1..] {
        assert_eq!(fixture.group.player_gameplay_fence(state.player), None);
        assert_eq!(state.gauge.snapshot().level_units, 21_000_000);
        assert_eq!(state.last_song, Timestamp::from_nanos(20_000_000));
    }
    let failed_hash = fixture
        .group
        .member_judge(PLAYERS[0])
        .unwrap()
        .stable_hash()
        .unwrap();
    let failed_capture = fixture.states[0]
        .capture
        .as_ref()
        .unwrap()
        .records()
        .to_vec();
    assert_eq!(failed_capture.len(), 2); // genuine Down(0), Advance(20ms).
    for member in 1..3 {
        let mut reports = input_reports(&mut fixture, member, 40_000_000, 3, ButtonState::Down);
        assert_eq!(reports[0].report.judge_events.len(), 1);
        observe_actual(&mut fixture, &mut reports).unwrap();
    }
    let progress_before = viewer.map(latest);
    let mut frozen = input_reports(&mut fixture, 0, 40_000_000, 2, ButtonState::Up);
    assert_eq!(
        frozen[0].report.song_time,
        Timestamp::from_nanos(20_000_000)
    );
    assert!(frozen[0].report.bound_inputs.is_empty());
    assert!(frozen[0].report.judge_events.is_empty());
    assert!(frozen[0].report.hazard_events.is_empty());
    assert!(frozen[0].report.audio_commands.is_empty());
    observe_actual(&mut fixture, &mut frozen).unwrap();
    if let (Some(viewer), Some(before)) = (viewer, progress_before) {
        let after = latest(viewer);
        assert_eq!(after.song_time, before.song_time);
        assert_eq!(after.song_time, None); // The multi-member legacy view is deliberately empty.
        for (old, new) in before.players[1..].iter().zip(&after.players[1..]) {
            assert_eq!(new.song_time, old.song_time);
            assert_eq!(new.song_time, Some(Timestamp::from_nanos(40_000_000)));
        }
    }
    let mut reports = fixture
        .group
        .advance_to(host(80_000_000), &ExplicitDomains, output(80_000_000))
        .unwrap();
    observe_actual(&mut fixture, &mut reports).unwrap();
    assert_eq!(
        fixture.states[0].capture.as_ref().unwrap().records(),
        failed_capture
    );
    assert_eq!(
        fixture
            .group
            .member_judge(PLAYERS[0])
            .unwrap()
            .stable_hash()
            .unwrap(),
        failed_hash
    );
    assert_eq!(fixture.states[0].score.hits, 1);
    assert_eq!(
        fixture.states[0].last_song,
        Timestamp::from_nanos(20_000_000)
    );
    assert_eq!(
        fixture
            .group
            .member_judge(PLAYERS[0])
            .unwrap()
            .remaining_hazards(),
        1
    );
    for state in &fixture.states[1..] {
        assert_eq!(state.score.hits, 2);
        assert_eq!(state.gauge.snapshot().level_units, 21_500_000);
        assert_eq!(state.gauge.snapshot().failure, None);
        assert_eq!(state.last_song, Timestamp::from_nanos(80_000_000));
    }
    // Scheduled per-member Stops preserve the admitted shared queue and survivor PCM.
    let mut pcm = [0.0; 48];
    let rendered = fixture.device.mixer.render(&mut pcm).unwrap();
    assert_eq!(&pcm[..3], &[0.75, 1.0, 0.0]); // Real mixer clamps the three-voice sum.
    assert_eq!(&pcm[40..43], &[0.5, 1.0, 0.0]);
    assert_eq!(rendered.counters.commands_applied, 6);
    assert_eq!(rendered.counters.unknown_stops, 1); // The stopped two-frame sample already ended.
    if let Some(viewer) = viewer {
        let snapshot = latest(viewer);
        assert_eq!(snapshot.gauge, BmsGauge::default()); // Multi-player has no combined gauge.
        for (member, state) in snapshot.players.iter().zip(&fixture.states) {
            assert_eq!(member.player, state.player);
            assert_eq!(member.gauge, state.gauge);
            assert_eq!(member.score, state.score);
        }
    }
    for state in &mut fixture.states {
        let hash = fixture
            .group
            .member_judge(state.player)
            .unwrap()
            .stable_hash()
            .unwrap();
        let capture = state.capture.take().unwrap();
        let count = capture.records().len();
        let mut replay =
            crate::replay_playback::reconstruct(&fixture.source, capture.into_file(), limits())
                .unwrap();
        replay.seek_cursor(count).unwrap();
        assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    }
}

#[test]
fn native_cohort_failure_fences_one_actual_owner_and_retains_survivors_with_or_without_ui() {
    survivor_case(None);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        survivor_case(Some(&viewer));
        Ok(())
    })
    .unwrap();
}

#[test]
fn native_cohort_processing_error_keeps_poisoned_prefix_and_independent_observation_errors() {
    let mut fixture = cohort_fixture(
        "#BPM 3000\n#WAV01 head.wav\n#00011:00010000\n#00012:00010000\n#000D1:00ZZ0000\n",
        &[0x11, 0x12],
        1,
        1,
    );
    let mut initial = fixture
        .group
        .advance_to(host(0), &ExplicitDomains, output(0))
        .unwrap();
    observe_actual(&mut fixture, &mut initial).unwrap();
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_local_chart(
            &fixture.source,
            &fixture.source.compile().unwrap().chart,
            &[PlayerId(555)],
        )
        .unwrap();
        let pause_supported = false;
        let mut session = NativeCohortSession {
            group: &mut fixture.group,
            states: &mut fixture.states,
            network: None,
            merger: &mut fixture.merger,
            bgm: &mut fixture.bgm,
            discipline: &mut fixture.discipline,
            pause: &mut fixture.pause,
            end: &mut fixture.end,
            delivery: &mut fixture.delivery,
            pre_origin_inputs: &mut fixture.pre,
        };
        let error = process(
            &mut fixture.device,
            &mut session,
            NativeGameplayConfig {
                origin: host(0),
                stream_origin: output(0),
                playback_origin: output(0),
                song_origin: Timestamp::ZERO,
                sample_rate: 1000,
                end_song: None,
                advance_lag: Duration::ZERO,
                seconds: None,
                pause_supported,
                logical_schedule: false,
            },
            button(SOURCES[0], 20_000_000, 1, ButtonState::Down),
        )
        .unwrap_err();
        let error = error.downcast_ref::<NativeCohortProcessingError>().unwrap();
        assert_eq!(error.group_error.failed_player, Some(PLAYERS[0]));
        let original = &error.group_error.completed_reports;
        assert_eq!(original.len(), 1);
        assert_eq!(original[0].report.bound_inputs.len(), 2);
        assert_eq!(original[0].report.judge_events.len(), 2);
        assert_eq!(original[0].report.hazard_events.len(), 1);
        assert_eq!(original[0].report.audio_commands.len(), 1);
        assert_eq!(original[0].report.audio_failures.len(), 3);
        assert!(matches!(
            original[0].report.audio_failures[0].command,
            AudioCommand::Play {
                voice: VoiceId(2),
                ..
            }
        ));
        assert_eq!(
            original[0].report.audio_failures[1..]
                .iter()
                .map(|error| error.command)
                .collect::<Vec<_>>(),
            [
                AudioCommand::Stop {
                    voice: VoiceId(1),
                    at: Timestamp::ZERO
                },
                AudioCommand::Stop {
                    voice: VoiceId(2),
                    at: Timestamp::ZERO
                },
            ]
        );
        assert!(
            original[0]
                .report
                .audio_failures
                .iter()
                .all(|error| error.reason == QueuePushError::Full)
        );
        let observation = error
            .observation_error
            .as_ref()
            .unwrap()
            .downcast_ref::<NativeCohortObservationError>()
            .unwrap();
        assert_eq!(observation.reports.len(), 1);
        assert_eq!(
            observation.reports[0].report.audio_commands,
            original[0].report.audio_commands
        );
        assert_eq!(
            observation.reports[0].report.audio_failures,
            original[0].report.audio_failures
        );
        assert_eq!(
            observation.reports[0].report.bound_inputs,
            original[0].report.bound_inputs
        );
        assert_eq!(
            observation.reports[0].report.hazard_events,
            original[0].report.hazard_events
        );
        assert!(
            observation
                .failures
                .iter()
                .any(|error| error.contains("capture"))
        );
        assert!(
            observation
                .failures
                .iter()
                .any(|error| error.contains("presentation"))
        );
        assert!(
            observation
                .failures
                .iter()
                .any(|error| error.contains("partial report"))
        );
        assert!(fixture.group.poisoned());
        assert_eq!(
            fixture.group.player_gameplay_fence(PLAYERS[0]),
            Some(Timestamp::from_nanos(20_000_000))
        );
        assert_eq!(
            fixture.states[0].gauge.snapshot().failure,
            Some(GaugeFailure::InstantDeath)
        );
        assert_eq!(fixture.states[0].score.hits, 2);
        for state in &fixture.states[1..] {
            assert_eq!(state.gauge, BmsGauge::default());
            assert_eq!(state.score, ScoreSummary::default());
            assert_eq!(fixture.group.player_gameplay_fence(state.player), None);
        }
        assert!(
            fixture
                .states
                .iter()
                .all(|state| state.capture.as_ref().unwrap().records().len() == 1)
        );
        assert!(
            fixture
                .group
                .advance_to(host(30_000_000), &ExplicitDomains, output(30_000_000))
                .is_err()
        );
        let mut pcm = [0.0; 3];
        fixture.device.mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.25, 0.5, 0.0]);
        assert_eq!(latest(&viewer).players[0].score.hits, 0);
        Ok(())
    })
    .unwrap();

    let mut fixture = Fixture::new(false, 8);
    fixture.states[1].gauge = BmsGauge::new(GaugeProfile::new(0, 0, 0, 0, true, vec![]).unwrap());
    let hashes = fixture
        .states
        .iter()
        .map(|state| {
            fixture
                .group
                .member_judge(state.player)
                .unwrap()
                .stable_hash()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(fixture.run(false).is_err());
    assert_eq!(fixture.device.step, 0);
    assert!(fixture.device.pcm.is_empty());
    assert!(!fixture.group.poisoned());
    for (state, hash) in fixture.states.iter().zip(hashes) {
        assert_eq!(
            fixture
                .group
                .member_judge(state.player)
                .unwrap()
                .stable_hash()
                .unwrap(),
            hash
        );
        assert_eq!(fixture.group.player_gameplay_fence(state.player), None);
        assert!(state.capture.as_ref().unwrap().records().is_empty());
    }
}
