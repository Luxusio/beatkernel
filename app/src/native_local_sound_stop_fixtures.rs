// Deferred shared native report observers and real software queue/Mixer evidence.
use super::*;
use crate::gauge::GaugeFailure;
use beatkernel::input::{ContactId, Position2, TouchEvent, TouchPhase};
const PLAYERS: [PlayerId; 3] = [PlayerId(7), PlayerId(101), PlayerId(u32::MAX)];
const SOURCES: [u64; 3] = [u64::MAX - 2, u64::MAX - 1, u64::MAX];

fn stop(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: Timestamp::from_nanos(at),
    }
}
fn observe_actual(fixture: &mut Fixture, reports: &mut [PlayerReport]) -> NativeGameplayResult<()> {
    observe_reports(reports, &mut fixture.states, &mut fixture.group, None)
}
fn reports(
    fixture: &mut Fixture,
    index: usize,
    at: i64,
    sequence: u64,
    state: ButtonState,
    audio: i64,
) -> Vec<PlayerReport> {
    let InputResult::Processed(reports) = fixture
        .group
        .process_input(
            button(SOURCES[index], at, sequence, state),
            &ExplicitDomains,
            output(audio),
        )
        .unwrap()
    else {
        panic!("exact assigned source")
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].player, PLAYERS[index]);
    reports
}
fn success(viewer: Option<&player::PlayerViewer>) {
    let mut fixture = gauge_fence::cohort_fixture(
        "#BPM 3000\n#WAV01 head.wav\n#00011:01000100\n#000D1:00ZZ0000\n",
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
    // Explicit separately owned BGM remains outside every member's stop union.
    fixture
        .group
        .enqueue_audio(AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(99),
            at: Timestamp::from_nanos(40_000_000),
            gain: 0.5,
        })
        .unwrap();
    for index in 0..3 {
        let mut actual = reports(&mut fixture, index, 0, 1, ButtonState::Down, 40_000_000);
        observe_actual(&mut fixture, &mut actual).unwrap();
        assert!(matches!(actual[0].report.audio_commands.as_slice(),
            [AudioCommand::Play { voice, .. }] if *voice == VoiceId(1 + 2 * index as u64)));
    }
    for index in 1..3 {
        let mut released = reports(
            &mut fixture,
            index,
            10_000_000,
            2,
            ButtonState::Up,
            10_000_000,
        );
        observe_actual(&mut fixture, &mut released).unwrap();
    }
    let mut deadline = fixture
        .group
        .advance_to(host(20_000_000), &ExplicitDomains, output(20_000_000))
        .unwrap();
    observe_actual(&mut fixture, &mut deadline).unwrap();
    assert_eq!(deadline[0].report.audio_at, output(20_000_000));
    assert_eq!(deadline[0].report.audio_commands, [stop(1, 40_000_000)]);
    assert!(
        deadline
            .iter()
            .all(|tagged| tagged.report.audio_failures.is_empty())
    );
    assert!(
        deadline[1..]
            .iter()
            .all(|tagged| tagged.report.audio_commands.is_empty())
    );
    assert_eq!(
        fixture.group.player_gameplay_fence(PLAYERS[0]),
        Some(Timestamp::from_nanos(20_000_000))
    );
    assert_eq!(
        fixture.states[0].gauge.snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    assert!(
        fixture.states[1..]
            .iter()
            .all(|state| state.gauge.snapshot().failure.is_none())
    );
    assert!(!fixture.group.poisoned());
    let failed_hash = fixture
        .group
        .member_judge(PLAYERS[0])
        .unwrap()
        .stable_hash()
        .unwrap();
    let prefix = fixture.states[0]
        .capture
        .as_ref()
        .unwrap()
        .records()
        .to_vec();
    let contact = PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(SOURCES[0]), host(30_000_000), 2),
        control: PhysicalControlId::keyboard(4u16),
        contact: ContactId(u64::MAX),
        phase: TouchPhase::Cancel,
        position: Position2 { x: -5.0, y: 1000.0 },
        pressure: None,
    });
    let InputResult::Processed(mut after) = fixture
        .group
        .process_input(contact.clone(), &ExplicitDomains, output(30_000_000))
        .unwrap()
    else {
        panic!("fenced member still acquires its source")
    };
    assert_eq!(after[0].report.input, Some(contact));
    assert!(after[0].report.bound_inputs.is_empty());
    observe_actual(&mut fixture, &mut after).unwrap();
    assert!(after[0].report.audio_commands.is_empty() && after[0].report.audio_failures.is_empty());
    assert_eq!(
        fixture.states[0].capture.as_ref().unwrap().records(),
        prefix
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
    for index in 1..3 {
        let mut next = reports(
            &mut fixture,
            index,
            40_000_000,
            3,
            ButtonState::Down,
            60_000_000,
        );
        observe_actual(&mut fixture, &mut next).unwrap();
        assert_eq!(fixture.states[index].score.hits, 2);
        assert_eq!(fixture.group.player_gameplay_fence(PLAYERS[index]), None);
    }
    let mut pcm = [0.0; 80];
    let rendered = fixture.device.mixer.render(&mut pcm).unwrap();
    assert_eq!(&pcm[..40], &[0.0; 40]);
    assert_eq!(&pcm[40..43], &[0.625, 1.0, 0.0]); // Two survivors plus BGM, failed voice stopped at its future Play.
    assert_eq!(&pcm[60..63], &[0.5, 1.0, 0.0]);
    assert_eq!(rendered.counters.commands_applied, 7);
    assert_eq!(rendered.counters.unknown_stops, 0);
    assert!(
        fixture
            .group
            .fence_player_sounds(PLAYERS[0], Timestamp::ZERO)
            .unwrap()
            .is_none()
    );
    if let Some(viewer) = viewer {
        player::publish_pause(PauseState::Paused);
        player::publish_pause(PauseState::Running);
        let snapshot = viewer.take_latest().unwrap();
        assert_eq!(snapshot.players[0].pressed_lanes, 0);
        assert_eq!(snapshot.players[0].score.hits, 1);
        assert!(
            snapshot.players[1..]
                .iter()
                .all(|member| member.score.hits == 2 && member.pressed_lanes == 1)
        );
    }
    for state in &mut fixture.states {
        let hash = fixture
            .group
            .member_judge(state.player)
            .unwrap()
            .stable_hash()
            .unwrap();
        let rebuilt = crate::replay_playback::reconstruct(
            &fixture.source,
            state.capture.take().unwrap().into_file(),
            limits(),
        )
        .unwrap();
        assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
    }
}

#[test]
fn native_shared_deadline_stops_only_failed_future_voices_and_preserves_survivors_and_bgm() {
    success(None);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        success(Some(&viewer));
        Ok(())
    })
    .unwrap();
}

#[test]
fn poisoned_input_and_whole_deadline_prefix_retain_exact_stop_errors_and_every_failed_fence() {
    let mut fixture = gauge_fence::cohort_fixture(
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
    fixture.device.step = 4; // The actual injected fallback scheduler returns output 40ms.
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_local_chart(
            &fixture.source,
            &fixture.source.compile().unwrap().chart,
            &[PlayerId(555)],
        )
        .unwrap();
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
        let failure = process(
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
                pause_supported: false,
                logical_schedule: false,
            },
            button(SOURCES[0], 20_000_000, 1, ButtonState::Down),
        )
        .unwrap_err();
        let failure = failure
            .downcast_ref::<NativeCohortProcessingError>()
            .unwrap();
        assert_eq!(failure.group_error.failed_player, Some(PLAYERS[0]));
        assert!(matches!(
            failure.group_error.kind,
            crate::local_runtime::FailureKind::ReportedFailure
        ));
        let report = &failure.group_error.completed_reports[0].report;
        assert_eq!(
            (
                report.bound_inputs.len(),
                report.judge_events.len(),
                report.hazard_events.len()
            ),
            (2, 2, 1)
        );
        assert_eq!(
            report.audio_commands,
            [AudioCommand::Play {
                sample: SampleId(1),
                voice: VoiceId(1),
                at: Timestamp::from_nanos(40_000_000),
                gain: 1.0
            }]
        );
        assert_eq!(
            report
                .audio_failures
                .iter()
                .map(|error| error.command)
                .collect::<Vec<_>>(),
            [
                AudioCommand::Play {
                    sample: SampleId(1),
                    voice: VoiceId(2),
                    at: Timestamp::from_nanos(40_000_000),
                    gain: 1.0
                },
                stop(1, 40_000_000),
                stop(2, 40_000_000),
            ]
        );
        assert!(
            report
                .audio_failures
                .iter()
                .all(|error| error.reason == QueuePushError::Full)
        );
        let observed = failure
            .observation_error
            .as_ref()
            .unwrap()
            .downcast_ref::<NativeCohortObservationError>()
            .unwrap();
        assert_eq!(
            observed.reports[0].report.audio_commands,
            report.audio_commands
        );
        assert_eq!(
            observed.reports[0].report.audio_failures,
            report.audio_failures
        );
        for name in ["capture", "presentation", "partial report", "sound stops"] {
            assert!(observed.failures.iter().any(|error| error.contains(name)));
        }
        assert!(fixture.group.poisoned());
        assert_eq!(
            fixture.group.player_gameplay_fence(PLAYERS[0]),
            Some(Timestamp::from_nanos(20_000_000))
        );
        assert_eq!(fixture.states[0].score.hits, 2);
        assert!(
            fixture.states[1..]
                .iter()
                .all(|state| state.score.hits == 0 && state.gauge == BmsGauge::default())
        );
        let mut pcm = [0.0; 64];
        fixture.device.mixer.render(&mut pcm).unwrap();
        assert_eq!(&pcm[40..43], &[0.25, 0.5, 0.0]);
        assert!(
            fixture
                .group
                .fence_player_sounds(PLAYERS[0], Timestamp::from_nanos(90_000_000))
                .unwrap()
                .is_none()
        );
        assert!(fixture.group.poisoned());
        player::publish_pause(PauseState::Paused);
        player::publish_pause(PauseState::Running);
        assert_eq!(viewer.take_latest().unwrap().players[0].score.hits, 0);
        Ok(())
    })
    .unwrap();

    let mut all = gauge_fence::cohort_fixture(
        "#BPM 3000\n#WAV01 future.wav\n#00011:00000100\n#000D1:00ZZ0000\n",
        &[0x11],
        2,
        128,
    );
    for index in 0..3 {
        let mut held = reports(&mut all, index, 0, 1, ButtonState::Down, 0);
        assert!(held[0].report.audio_commands.is_empty());
        observe_actual(&mut all, &mut held).unwrap();
    }
    let mut deadline = all
        .group
        .advance_to(host(20_000_000), &ExplicitDomains, output(20_000_000))
        .unwrap();
    let failure = observe_actual(&mut all, &mut deadline).unwrap_err();
    let failure = failure
        .downcast_ref::<NativeCohortObservationError>()
        .unwrap();
    assert_eq!(failure.reports.len(), 3);
    assert_eq!(deadline[0].report.audio_commands, [stop(1, 20_000_000)]);
    assert_eq!(deadline[1].report.audio_commands, [stop(3, 20_000_000)]);
    assert!(deadline[2].report.audio_commands.is_empty());
    assert_eq!(deadline[2].report.audio_failures.len(), 1);
    assert_eq!(
        deadline[2].report.audio_failures[0].command,
        stop(5, 20_000_000)
    );
    assert_eq!(
        deadline[2].report.audio_failures[0].reason,
        QueuePushError::Full
    );
    for (index, &player) in PLAYERS.iter().enumerate() {
        assert_eq!(
            all.group.player_gameplay_fence(player),
            Some(Timestamp::from_nanos(20_000_000))
        );
        assert_eq!(
            all.states[index].gauge.snapshot().failure,
            Some(GaugeFailure::InstantDeath)
        );
        assert_eq!(
            all.states[index].capture.as_ref().unwrap().records().len(),
            2
        );
        assert_eq!(
            failure.reports[index].report.audio_commands,
            deadline[index].report.audio_commands
        );
        assert_eq!(
            failure.reports[index].report.audio_failures,
            deadline[index].report.audio_failures
        );
    }
    let mut pcm = [0.0; 32];
    let rendered = all.device.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0; 32]);
    assert_eq!(rendered.counters.commands_applied, 2);
    assert_eq!(rendered.counters.unknown_stops, 2); // Both future-only configured voices were inactive.
    for &player in &PLAYERS {
        assert!(
            all.group
                .fence_player_sounds(player, Timestamp::from_nanos(30_000_000))
                .unwrap()
                .is_none()
        );
    }
}
