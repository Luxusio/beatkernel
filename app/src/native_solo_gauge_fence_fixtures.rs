// Deferred native common-owner fixtures. The parent Device renders portable
// PCM; these tests neither open a native device nor admit a mine chart file.
use super::*;
use crate::gauge::GaugeFailure;
use beatkernel::interaction::InputOwner;

const FAILURE_AT: i64 = 20_000_000;

pub(super) fn fatal_fixture(queue_capacity: usize, capture_records: usize) -> Fixture {
    let mut fixture = Fixture::new(false, false);
    let source = beatkernel_bms::parse(
        "#BPM 3000\n#LNTYPE 1\n#WAV01 head.wav\n#00051:00010001\n#00012:00010001\n#000D1:00ZZ0001\n",
        Default::default(),
    )
    .unwrap();
    let compiled = source.compile().unwrap();
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
    fixture.capture = crate::native_judge::prepare_capture_for_source(
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
    let bindings = BindingMap::from_bindings([0x11, 0x12].map(|lane| Binding {
        device: DeviceSelector::Exact(DeviceId(1)),
        physical: PhysicalControlId::keyboard(4u16),
        game_control: GameControlId(lane),
    }))
    .unwrap();
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
                    beatkernel::judge::JudgeStage::HoldHead
                } else {
                    beatkernel::judge::JudgeStage::Instant
                },
                sample: SampleId(1),
                voice: VoiceId(u64::from(note.lane.control().0)),
                gain: 1.0,
            }
        })
        .collect();
    let (producer, consumer) = command_queue(queue_capacity).unwrap();
    fixture.runtime = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        sounds,
        0,
    )
    .unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], pcm_limits).unwrap(),
    )
    .unwrap();
    fixture.device.mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(queue_capacity, 2, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    fixture.source = source;
    fixture
}

fn publish_actual(fixture: &mut Fixture, report: RuntimeReport) -> NativeGameplayResult<()> {
    publish(
        &mut NativeGameplaySession {
            runtime: &mut fixture.runtime,
            bgm: &mut fixture.bgm,
            discipline: &mut fixture.discipline,
            pause: &mut fixture.pause,
            end: &mut fixture.end,
            completion: &mut fixture.completion,
            capture: &mut fixture.capture,
            competition: &mut fixture.competition,
            gauge: &mut fixture.gauge,
            delivery: &mut fixture.delivery,
            pre_origin_inputs: &mut fixture.pre,
        },
        report,
    )
}

fn latest(viewer: &player::PlayerViewer) -> player::PlayerSnapshot {
    player::publish_pause(PauseState::Paused);
    player::publish_pause(PauseState::Running);
    viewer.take_latest().unwrap()
}

fn successful_failure_prefix(viewer: Option<&player::PlayerViewer>) {
    let mut fixture = fatal_fixture(8, 128);
    if viewer.is_some() {
        player::publish_chart(&fixture.source, &fixture.source.compile().unwrap().chart).unwrap();
    }
    fixture.run(false, true).unwrap();
    assert_eq!(
        fixture.runtime.gameplay_fence(),
        Some(Timestamp::from_nanos(FAILURE_AT))
    );
    assert_eq!(
        fixture.gauge.snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    assert_eq!(fixture.gauge.snapshot().level_units, 0);
    assert_eq!(fixture.runtime.judge().remaining_hazards(), 1);
    // The genuine Play prefix is followed by equal-frame Stops. The hold tail
    // and later mine remain unconsumed; zero PCM does not prove completion.
    assert_eq!(&fixture.device.pcm[20..23], &[0.0, 0.0, 0.0]);
    assert_eq!(fixture.device.mixer.counters().commands_applied, 4);
    assert_eq!(fixture.device.mixer.counters().unknown_stops, 0);
    let held = InputOwner {
        source: DeviceId(1),
        physical: PhysicalControlId::keyboard(4u16),
        game_control: GameControlId(0x11),
    };
    assert!(fixture.runtime.judge().is_held(held));
    let hash = fixture.runtime.judge().stable_hash().unwrap();
    let captured = fixture.capture.as_ref().unwrap().records().to_vec();
    assert_eq!(captured.len(), 3); // advance(0), first bound head, second bound head.
    assert_eq!(
        captured
            .iter()
            .map(|record| record.song_time.as_nanos())
            .collect::<Vec<_>>(),
        vec![0, FAILURE_AT, FAILURE_AT]
    );
    let before = viewer.map(latest);
    let release = fixture
        .runtime
        .process_input(
            input(60_000_000, 2, ButtonState::Up),
            &ExplicitDomains,
            point(2, 60_000_000),
        )
        .unwrap();
    assert!(release.input.is_some());
    assert!(release.bound_inputs.is_empty());
    assert!(release.judge_events.is_empty());
    assert!(release.hazard_events.is_empty());
    assert!(release.audio_commands.is_empty());
    assert_eq!(release.song_time, Timestamp::from_nanos(FAILURE_AT));
    publish_actual(&mut fixture, release).unwrap();
    let later = fixture
        .runtime
        .advance_to(point(1, 80_000_000), &ExplicitDomains, point(2, 80_000_000))
        .unwrap();
    publish_actual(&mut fixture, later).unwrap();
    assert_eq!(fixture.capture.as_ref().unwrap().records(), captured);
    assert_eq!(fixture.runtime.judge().stable_hash().unwrap(), hash);
    assert!(fixture.runtime.judge().is_held(held));
    assert!(matches!(
        fixture
            .runtime
            .process_input(
                input(90_000_000, 1, ButtonState::Down),
                &ExplicitDomains,
                point(2, 90_000_000),
            )
            .unwrap_err()
            .kind,
        crate::local_runtime::FailureKind::Core(
            beatkernel::runtime::RuntimeError::SequenceRegression {
                last: 2,
                received: 1,
                ..
            }
        )
    )); // Acquisition sequence validation remains live.
    if let (Some(viewer), Some(before)) = (viewer, before) {
        let after = latest(viewer);
        assert_eq!(before.score.hits, 2);
        assert_eq!(before.mine_damage.triggered, 1);
        assert_eq!(before.gauge, fixture.gauge);
        assert_eq!(after.score, before.score);
        assert_eq!(after.gauge, before.gauge);
        assert_eq!(after.song_time, before.song_time);
        assert_eq!(after.pressed_lanes, before.pressed_lanes);
    }
    let file = fixture.capture.take().unwrap().into_file();
    let mut replay = crate::replay_playback::reconstruct(&fixture.source, file, limits()).unwrap();
    replay.seek_cursor(captured.len()).unwrap();
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    assert_eq!(replay.results().len(), 2);
}

#[test]
fn native_solo_failure_keeps_actual_fanout_pcm_and_capture_with_or_without_ui() {
    successful_failure_prefix(None);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        successful_failure_prefix(Some(&viewer));
        Ok(())
    })
    .unwrap();
}

#[test]
fn native_solo_observation_errors_keep_committed_failure_and_reject_custom_entry() {
    let mut fixture = fatal_fixture(1, 1);
    let initial = fixture
        .runtime
        .advance_to(point(1, 0), &ExplicitDomains, point(2, 0))
        .unwrap();
    publish_actual(&mut fixture, initial).unwrap();
    // The comparison observer has independently seen a later genuine report.
    // Its chronology error must not short-circuit capture, UI or failure state.
    let comparison =
        crate::competition::Competition::new(fixture.capture.as_ref().unwrap().header().clone(), 0)
            .unwrap();
    let mut comparison = LiveCompetition::from_prepared(
        crate::local_players::PlayerId(1),
        comparison,
        None,
        WallDuration::from_secs(1),
    )
    .unwrap();
    let mut ahead = fatal_fixture(8, 128);
    let future = ahead
        .runtime
        .advance_to(point(1, 80_000_000), &ExplicitDomains, point(2, 80_000_000))
        .unwrap();
    comparison.observe(&future).unwrap();
    fixture.competition = Some(comparison);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        // Deliberately register a different owner, through the real public
        // channel, so native solo publication must report its independent error.
        player::publish_local_chart(
            &fixture.source,
            &fixture.source.compile().unwrap().chart,
            &[
                crate::local_players::PlayerId(7),
                crate::local_players::PlayerId(9),
            ],
        )
        .unwrap();
        let report = fixture
            .runtime
            .process_input(
                input(FAILURE_AT, 1, ButtonState::Down),
                &ExplicitDomains,
                point(2, FAILURE_AT),
            )
            .unwrap();
        let error = publish_actual(&mut fixture, report).unwrap_err();
        let error = error
            .downcast_ref::<NativeReportObservationError>()
            .unwrap();
        assert!(error.gauge_error.is_none());
        assert!(error.capture_error.is_some());
        assert!(error.presentation_error.is_some());
        assert!(error.competition_error.is_some());
        assert_eq!(error.report.song_time, Timestamp::from_nanos(FAILURE_AT));
        assert_eq!(error.report.bound_inputs.len(), 2);
        assert_eq!(error.report.judge_events.len(), 2);
        assert_eq!(error.report.hazard_events.len(), 1);
        assert_eq!(error.report.audio_commands.len(), 1);
        assert_eq!(error.report.audio_failures.len(), 3);
        assert!(matches!(
            error.report.audio_commands.as_slice(),
            [AudioCommand::Play {
                voice: VoiceId(17),
                ..
            }]
        ));
        assert!(matches!(
            error.report.audio_failures[0].command,
            AudioCommand::Play {
                voice: VoiceId(18),
                ..
            }
        ));
        assert_eq!(
            error.report.audio_failures[1..]
                .iter()
                .map(|error| error.command)
                .collect::<Vec<_>>(),
            [
                AudioCommand::Stop {
                    voice: VoiceId(17),
                    at: Timestamp::from_nanos(FAILURE_AT)
                },
                AudioCommand::Stop {
                    voice: VoiceId(18),
                    at: Timestamp::from_nanos(FAILURE_AT)
                },
            ]
        );
        assert!(
            error
                .report
                .audio_failures
                .iter()
                .all(|error| error.reason == QueuePushError::Full)
        );
        assert_eq!(
            fixture.gauge.snapshot().failure,
            Some(GaugeFailure::InstantDeath)
        );
        assert_eq!(
            fixture.runtime.gameplay_fence(),
            Some(Timestamp::from_nanos(FAILURE_AT))
        );
        assert_eq!(fixture.capture.as_ref().unwrap().records().len(), 1);
        let mut tail = [0.0; 24];
        fixture.device.mixer.render(&mut tail).unwrap();
        assert_eq!(&tail[20..23], &[0.25, 0.5, 0.0]);
        let snapshot = latest(&viewer);
        assert!(snapshot.players.iter().all(|member| member.score.hits == 0));
        let file = fixture.capture.take().unwrap().into_file();
        let mut prefix =
            crate::replay_playback::reconstruct(&fixture.source, file, limits()).unwrap();
        prefix.seek_cursor(1).unwrap();
        assert_ne!(
            prefix.engine().stable_hash().unwrap(),
            fixture.runtime.judge().stable_hash().unwrap()
        );
        Ok(())
    })
    .unwrap();

    let mut fixture = fatal_fixture(8, 128);
    fixture.gauge = BmsGauge::new(GaugeProfile::new(0, 0, 0, 0, true, vec![]).unwrap());
    let before = fixture.runtime.judge().stable_hash().unwrap();
    assert!(fixture.run(false, true).is_err());
    assert_eq!(fixture.device.step, 0);
    assert!(fixture.device.pcm.is_empty());
    assert!(fixture.capture.as_ref().unwrap().records().is_empty());
    assert_eq!(fixture.runtime.judge().stable_hash().unwrap(), before);
    assert_eq!(fixture.runtime.gameplay_fence(), None);

    let mut seed = Fixture::new(false, false);
    let accepted = seed
        .runtime
        .process_input(
            input(FAILURE_AT, 1, ButtonState::Down),
            &ExplicitDomains,
            point(2, FAILURE_AT),
        )
        .unwrap();
    seed.gauge
        .observe(&accepted.judge_events, &accepted.hazard_events)
        .unwrap();
    let mut resumed = Fixture::new(false, false);
    resumed.gauge = seed.gauge;
    resumed.run(false, true).unwrap();
    assert_eq!(resumed.gauge.snapshot().level_units, 22_000_000);
    assert_eq!(resumed.runtime.gameplay_fence(), None);
}
