use super::*;
use beatkernel::time::Timestamp;
use beatkernel_bms_runtime::practice_control::*;
fn t(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn fixture() -> (Desktop, player::PlayerPublisher) {
    let mut app = tests::lifecycle_fixture();
    let next = app
        .prepare_route(ScreenRoute::Play { replay: false })
        .unwrap();
    app.commit_route(next);
    let (publisher, viewer) = player::channel();
    let _ = viewer.take_latest();
    publisher
        .advertise_practice(Some(PracticeCapability {
            generation: 1,
            min_target: t(0),
            max_target: t(1000),
        }))
        .unwrap();
    app.game = Some(Game {
        viewer,
        worker: None,
        snapshot: Some(player::PlayerSnapshot {
            status: player::PlayerStatus::Playing,
            song_time: Some(t(10)),
            pause: player::PauseState::Running,
            ..Default::default()
        }),
        completed_results: None,
        completed_results_error: None,
        cancelling: false,
        joined: false,
        local_page: 0,
        local_comparisons: false,
        replay: false,
        launch: SessionLaunch::new(vec!["--chart".into(), "pinned.bms".into()]).unwrap(),
        prepared_retry: None,
        practice_bookmark: None,
        practice_loop: None,
        loop_enabled: false,
    });
    (app, publisher)
}
fn ack(publisher: &player::PlayerPublisher, request: PracticeRequest, generation: u64) {
    let target = match request.action {
        PracticeAction::Loop { start, .. } => start,
        PracticeAction::Scrub { target } => target,
        PracticeAction::DisableLoop => t(20),
    };
    publisher
        .commit_practice_reply(&PracticeReply {
            id: request.id,
            generation: request.generation,
            result: Ok(PracticeApplied {
                generation,
                physical_frame: 100,
                playback_frame: 80,
                requested_target: target,
                applied_target: target,
            }),
        })
        .unwrap();
}
#[test]
fn desktop_practice_control_f8_f11_actual_dispatch_retains_owner_until_ack() {
    let (mut app, publisher) = fixture();
    app.key(KeyCode::F7, false);
    app.game
        .as_mut()
        .unwrap()
        .snapshot
        .as_mut()
        .unwrap()
        .song_time = Some(t(20));
    app.key(KeyCode::F10, false);
    app.key(KeyCode::F11, false);
    let request = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(
        request.action,
        PracticeAction::Loop {
            start: t(10),
            end: t(20)
        }
    );
    let game = app.game.as_ref().unwrap();
    assert!(!game.cancelling && !game.joined && game.prepared_retry.is_none());
    assert!(!game.loop_enabled); // Admission is not applied boundary.
    assert!(game.pause_target().is_none());
    app.key(KeyCode::F8, false);
    assert!(publisher.take_practice_request().unwrap().is_none());
    ack(&publisher, request, 2);
    app.collect_game();
    assert!(app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F11, false);
    let request = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(request.action, PracticeAction::DisableLoop);
    ack(&publisher, request, 2);
    app.collect_game();
    assert!(!app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F8, false);
    let request = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(request.action, PracticeAction::Scrub { target: t(10) });
    publisher
        .reply_practice(&PracticeReply {
            id: request.id,
            generation: request.generation,
            result: Err("queue full".into()),
        })
        .unwrap();
    app.collect_game();
    assert!(app.failure.as_ref().unwrap().contains("queue full"));
    assert!(!app.game.as_ref().unwrap().cancelling);
    app.key(KeyCode::F5, false); // Pinned retry remains fresh-owner cancellation.
    assert!(app.game.as_ref().unwrap().cancelling);
    assert!(app.game.as_ref().unwrap().prepared_retry.is_some());
}
#[test]
fn desktop_practice_control_scrub_clears_loop_only_after_successful_ack() {
    let (mut app, publisher) = fixture();
    app.key(KeyCode::F7, false);
    app.game
        .as_mut()
        .unwrap()
        .snapshot
        .as_mut()
        .unwrap()
        .song_time = Some(t(20));
    app.key(KeyCode::F10, false);
    app.key(KeyCode::F11, false);
    let request = publisher.take_practice_request().unwrap().unwrap();
    ack(&publisher, request, 2);
    app.collect_game();
    assert!(app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F8, false);
    let refused = publisher.take_practice_request().unwrap().unwrap();
    assert!(app.game.as_ref().unwrap().loop_enabled);
    publisher
        .reply_practice(&PracticeReply {
            id: refused.id,
            generation: refused.generation,
            result: Err("cold scrub refusal".into()),
        })
        .unwrap();
    app.collect_game();
    assert!(app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F8, false);
    let applied = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(applied.action, PracticeAction::Scrub { target: t(10) });
    assert!(app.game.as_ref().unwrap().loop_enabled);
    ack(&publisher, applied, 3);
    app.collect_game();
    assert!(!app.game.as_ref().unwrap().loop_enabled);
    assert!(!app.game.as_ref().unwrap().cancelling);
    app.key(KeyCode::F11, false);
    let next = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(
        next.action,
        PracticeAction::Loop {
            start: t(10),
            end: t(20)
        }
    );
}

#[test]
fn desktop_practice_control_watch_network_pause_and_revocation_never_legacy_restart() {
    let (mut app, publisher) = fixture();
    app.key(KeyCode::F7, false);
    app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause = player::PauseState::Pausing;
    app.key(KeyCode::F8, false);
    assert!(publisher.take_practice_request().unwrap().is_none());
    assert!(!app.game.as_ref().unwrap().cancelling);
    app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause = player::PauseState::Running;
    app.game.as_mut().unwrap().launch = SessionLaunch::new(vec![
        "--chart".into(),
        "pinned.bms".into(),
        "--mp-host".into(),
        "localhost:8000".into(),
    ])
    .unwrap();
    app.key(KeyCode::F8, false);
    assert!(publisher.take_practice_request().unwrap().is_none());
    app.game.as_mut().unwrap().launch =
        SessionLaunch::new(vec!["--chart".into(), "pinned.bms".into()]).unwrap();
    publisher.advertise_practice(None).unwrap();
    app.key(KeyCode::F8, false);
    let game = app.game.as_ref().unwrap();
    assert!(!game.cancelling && game.prepared_retry.is_none());
    assert!(!game.practice_restart_available());
    app.game.as_mut().unwrap().replay = true;
    assert!(!app.game.as_ref().unwrap().practice_restart_available());
}

#[test]
fn desktop_practice_control_f5_and_joined_retry_follow_actual_boundary_lineage() {
    use beatkernel_bms_runtime::{
        native_judge::NativeJudgeConfig,
        play_policy::{GaugeSelection, OriginalGaugeContext},
        practice_session::{prepare_attempt, PracticeAttemptConfig},
    };
    let source =
        beatkernel_bms::parse("#BPM 60\n#WAV01 key.wav\n#00011:0101\n", Default::default())
            .unwrap();
    let domain = beatkernel::time::ClockDomainId(1);
    let policy = NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: domain,
        end: None,
    }
    .resolve_play_policy(
        &OriginalGaugeContext::from_source(&source),
        GaugeSelection::BeatKernel,
    )
    .unwrap();
    let initial = SessionLaunch::new(vec![
        "--chart".into(),
        "pinned.bms".into(),
        "--record-replay".into(),
        "records/game.bkr".into(),
    ])
    .unwrap();
    for dispatch_f5 in [true, false] {
        let (mut app, publisher) = fixture();
        app.game.as_mut().unwrap().launch = initial.clone();
        if !dispatch_f5 {
            // This preflight is selected before the native owner advances. Final
            // joined handling must rebase it onto actual published boundaries.
            app.game.as_mut().unwrap().prepared_retry = Some(initial.retry().unwrap());
        }
        let mut current = initial.clone();
        player::with_publisher(publisher, || {
            player::pin_native_launch(initial.clone()).unwrap();
            player::publish_chart(&source, &source.source.compile().unwrap()).unwrap();
            player::prepare_native_play_policies(&[(
                beatkernel_bms_runtime::local_players::PlayerId(1),
                &policy,
            )])
            .unwrap();
            for generation in 1..=3 {
                let attempt = prepare_attempt(
                    &source,
                    &policy,
                    &current,
                    PracticeAttemptConfig {
                        start: t(1_000_000_000),
                        end: None,
                        domain,
                        chart_seed: 0,
                        capture_limits: beatkernel_bms_runtime::native_judge::capture_limits(
                            true, 16384, 128,
                        )
                        .unwrap(),
                    },
                )
                .unwrap();
                let staged = player::prepare_practice_presentation(
                    generation,
                    &[(beatkernel_bms_runtime::local_players::PlayerId(1), &attempt)],
                )
                .unwrap();
                player::commit_practice_presentation(staged, generation + 1).unwrap();
                current = attempt.next_launch;
            }
            // No frame collection happened during those real presentation commits.
            if dispatch_f5 {
                app.key(KeyCode::F5, false);
                let game = app.game.as_ref().unwrap();
                assert_eq!(game.launch.attempt(), 3);
                assert_eq!(
                    game.prepared_retry.as_ref().unwrap().args()[3],
                    "records/game.retry4.bkr"
                );
                assert!(game.cancelling);
            }
            Ok(())
        })
        .unwrap();

        // Exercise the same final-join handoff over actual published native launch
        // evidence. This component fixture makes no physical/joined-worker claim.
        let game = app.game.as_mut().unwrap();
        game.sync_native_launch().unwrap();
        assert_eq!(game.launch.attempt(), 3);
        game.rebase_joined_retry().unwrap();
        let retry = game.owner_finished(true).unwrap();
        assert_eq!(retry.attempt(), 4);
        assert_eq!(retry.args()[3], "records/game.retry4.bkr");
        assert_eq!(retry.original_args(), initial.original_args());
    }
}

#[test]
fn desktop_practice_control_spawned_worker_keeps_tls_and_joined_f5_lineage_after_cancelled_visual_commit(
) {
    use beatkernel_bms_runtime::{
        native_judge::NativeJudgeConfig,
        play_policy::{GaugeSelection, OriginalGaugeContext},
        practice_session::{prepare_attempt, PracticeAttemptConfig},
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    const WAIT: Duration = Duration::from_secs(5);
    static JOINED_VALIDATIONS: AtomicUsize = AtomicUsize::new(0);
    fn validate_joined(args: &[String]) -> Result<(), Box<dyn Error>> {
        assert!(args
            .chunks_exact(2)
            .any(|pair| pair == ["--record-replay", "records/thread.retry4.bkr"]));
        JOINED_VALIDATIONS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    enum Command {
        ApplyRequest,
        StageBeforeCancel,
        ApplyCancelled,
    }
    #[derive(Debug)]
    struct Step {
        worker: thread::ThreadId,
        attempt: u32,
        request: Option<u64>,
        visual_refused: bool,
    }
    let (commands, owner_commands) = mpsc::sync_channel(1);
    let (owner_steps, steps) = mpsc::sync_channel(1);
    let initial = SessionLaunch::new(vec![
        "--chart".into(),
        "pinned-thread.bms".into(),
        "--record-replay".into(),
        "records/thread.bkr".into(),
    ])
    .unwrap();
    let expected_initial = initial.clone();
    let game = spawn_game_with(
        move |args| {
            let worker = thread::current().id();
            assert_eq!(thread::current().name(), Some("bms-game"));
            assert!(player::attached());
            let mut current = player::native_launch(args)?;
            assert_eq!(current.args(), expected_initial.args());
            assert_eq!(current.original_args(), expected_initial.original_args());
            assert_eq!(current.attempt(), 0);
            let source =
                beatkernel_bms::parse("#BPM 60\n#WAV01 key.wav\n#00011:0101\n", Default::default())
                    .unwrap();
            let domain = beatkernel::time::ClockDomainId(1);
            let policy = NativeJudgeConfig {
                early: 0,
                late: 0,
                offset: 0,
                preroll: 0,
                output: domain,
                end: None,
            }
            .resolve_play_policy(
                &OriginalGaugeContext::from_source(&source),
                GaugeSelection::BeatKernel,
            )
            .unwrap();
            let member = beatkernel_bms_runtime::local_players::PlayerId(1);
            player::publish_chart(&source, &source.source.compile().unwrap()).unwrap();
            player::prepare_native_play_policies(&[(member, &policy)]).unwrap();
            player::publish_pause(player::PauseState::Running);
            let prepare = |launch: &SessionLaunch, generation: u64| {
                let attempt = prepare_attempt(
                    &source,
                    &policy,
                    launch,
                    PracticeAttemptConfig {
                        start: t(1_000_000_000),
                        end: None,
                        domain,
                        chart_seed: 0,
                        capture_limits: beatkernel_bms_runtime::native_judge::capture_limits(
                            true, 16384, 128,
                        )
                        .unwrap(),
                    },
                )
                .unwrap();
                let presentation =
                    player::prepare_practice_presentation(generation, &[(member, &attempt)])
                        .unwrap();
                (attempt, presentation)
            };
            // These are genuine TLS presentation/identity effects. This injected
            // owner does not claim to qualify an audio or native output boundary.
            let (attempt, presentation) = prepare(&current, 1);
            player::commit_practice_presentation(presentation, 2).unwrap();
            current = attempt.next_launch;
            player::advertise_practice(Some(PracticeCapability {
                generation: 2,
                min_target: t(0),
                max_target: t(4_000_000_000),
            }))
            .unwrap();
            owner_steps
                .send(Step {
                    worker,
                    attempt: 1,
                    request: None,
                    visual_refused: false,
                })
                .unwrap();
            assert!(matches!(
                owner_commands.recv_timeout(WAIT).unwrap(),
                Command::ApplyRequest
            ));
            let request = player::take_practice_request().unwrap().unwrap();
            assert_eq!(request.generation, 2);
            assert_eq!(
                request.action,
                PracticeAction::Scrub {
                    target: t(1_000_000_000)
                }
            );
            assert!(!player::cancelled());
            let (attempt, presentation) = prepare(&current, 2);
            player::commit_practice_presentation(presentation, 3).unwrap();
            current = attempt.next_launch;
            player::commit_practice_reply(&PracticeReply {
                id: request.id,
                generation: request.generation,
                result: Ok(PracticeApplied {
                    generation: 3,
                    physical_frame: 100,
                    playback_frame: 80,
                    requested_target: t(1_000_000_000),
                    applied_target: t(1_000_000_000),
                }),
            })
            .unwrap();
            owner_steps
                .send(Step {
                    worker,
                    attempt: 2,
                    request: Some(request.id),
                    visual_refused: false,
                })
                .unwrap();
            assert!(matches!(
                owner_commands.recv_timeout(WAIT).unwrap(),
                Command::StageBeforeCancel
            ));
            let (attempt, mut presentation) = prepare(&current, 3);
            owner_steps
                .send(Step {
                    worker,
                    attempt: current.attempt(),
                    request: None,
                    visual_refused: false,
                })
                .unwrap();
            assert!(matches!(
                owner_commands.recv_timeout(WAIT).unwrap(),
                Command::ApplyCancelled
            ));
            assert!(player::cancelled());
            // The native pump would supply actual boundary qualification here.
            // Applied identity must survive observer refusal after cancellation.
            player::apply_practice_identity(&mut presentation, 4);
            assert!(player::commit_practice_presentation(presentation, 4).is_err());
            current = attempt.next_launch;
            assert_eq!(player::native_launch(current.args()).unwrap().attempt(), 3);
            owner_steps
                .send(Step {
                    worker,
                    attempt: 3,
                    request: None,
                    visual_refused: true,
                })
                .unwrap();
            Ok(())
        },
        initial.clone(),
        0,
        false,
        false,
    )
    .unwrap();
    let mut app = tests::lifecycle_fixture();
    let next = app
        .prepare_route(ScreenRoute::Play { replay: false })
        .unwrap();
    app.commit_route(next);
    app.game = Some(game);
    let ready = steps.recv_timeout(WAIT).unwrap();
    assert_ne!(ready.worker, thread::current().id());
    assert_eq!(ready.attempt, 1);
    app.collect_game();
    assert_eq!(app.game.as_ref().unwrap().launch.attempt(), 1);
    assert_eq!(
        app.game.as_ref().unwrap().snapshot.as_ref().unwrap().status,
        player::PlayerStatus::Playing
    );
    app.key(KeyCode::F7, false);
    app.key(KeyCode::F8, false);
    let request = app
        .game
        .as_ref()
        .unwrap()
        .viewer
        .pending_practice_request()
        .unwrap()
        .unwrap();
    assert!(!app.game.as_ref().unwrap().cancelling);
    assert!(app.game.as_ref().unwrap().worker.is_some());
    commands.send(Command::ApplyRequest).unwrap();
    let applied = steps.recv_timeout(WAIT).unwrap();
    assert_eq!(applied.worker, ready.worker);
    assert_eq!(applied.request, Some(request.id));
    assert_eq!(applied.attempt, 2);
    app.collect_game();
    let game = app.game.as_ref().unwrap();
    assert_eq!(game.launch.attempt(), 2);
    assert!(!game.viewer.practice_pending());
    assert!(!game.cancelling && !game.joined && game.prepared_retry.is_none());
    commands.send(Command::StageBeforeCancel).unwrap();
    assert_eq!(steps.recv_timeout(WAIT).unwrap().worker, ready.worker);
    app.key(KeyCode::F5, false);
    let game = app.game.as_ref().unwrap();
    assert!(game.cancelling);
    assert_eq!(game.prepared_retry.as_ref().unwrap().attempt(), 3);
    assert_eq!(
        game.prepared_retry.as_ref().unwrap().args()[3],
        "records/thread.retry3.bkr"
    );
    // Keep the joined owner for inspection; avoid creating the next native owner.
    app.occluded = true;
    app.validate = validate_joined;
    let before = JOINED_VALIDATIONS.load(Ordering::SeqCst);
    commands.send(Command::ApplyCancelled).unwrap();
    let final_step = steps.recv_timeout(WAIT).unwrap();
    assert_eq!(final_step.worker, ready.worker);
    assert_eq!(final_step.attempt, 3);
    assert!(final_step.visual_refused);
    let deadline = Instant::now() + WAIT;
    while !app
        .game
        .as_ref()
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
        .is_finished()
    {
        assert!(
            Instant::now() < deadline,
            "spawned game worker failed to finish"
        );
        thread::yield_now();
    }
    app.collect_game(); // Actual production join, canonical rebase and preflight.
    let game = app.game.as_ref().unwrap();
    assert!(game.joined && game.worker.is_none());
    assert_eq!(game.launch.attempt(), 3);
    assert_eq!(game.launch.original_args(), initial.original_args());
    assert_eq!(JOINED_VALIDATIONS.load(Ordering::SeqCst), before + 1);
    assert!(game.viewer.practice_capability().unwrap().is_none());
}
