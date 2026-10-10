//! Real desktop worker/TLS/publication composition with injected native discovery.
//! These tests exercise software ownership, not physical device startup.
use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering as AtomicOrdering},
    mpsc,
};

const WAIT: Duration = Duration::from_secs(5);

fn request() -> SessionLaunch {
    SessionLaunch::new(
        [
            "--chart",
            "selected.bms",
            "--record-replay",
            "records/live.bkr",
        ]
        .map(str::to_owned)
        .to_vec(),
    )
    .unwrap()
}

fn resolved(args: &[String]) -> Vec<String> {
    let mut args = args.to_vec();
    args.extend(["--channel-policy", "exact", "--output", "selected-output"].map(str::to_owned));
    args
}

fn joined(game: &mut Game) -> Result<(), String> {
    let deadline = Instant::now() + WAIT;
    while !game.worker.as_ref().unwrap().is_finished() {
        assert!(
            Instant::now() < deadline,
            "native preparation worker did not finish"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let result = game
        .worker
        .take()
        .unwrap()
        .join()
        .expect("native worker panicked");
    game.joined = true;
    result
}

#[test]
fn actual_worker_resolves_once_off_ui_then_pins_exact_native_args_and_retry_bypasses_discovery() {
    let ui = thread::current().id();
    let requested = request();
    let expected_original = requested.original_args().to_vec();
    let expected_args = resolved(requested.args());
    let resolves = Arc::new(AtomicUsize::new(0));
    let count = resolves.clone();
    let (resolved_tx, resolution) = mpsc::sync_channel(1);
    let (called_tx, called) = mpsc::sync_channel(1);
    let native_args = expected_args.clone();
    let native_original = expected_original.clone();
    let mut game = spawn_game_with_native_preparation(
        move |args| {
            assert!(player::attached());
            let pinned = player::native_launch(args)?;
            assert!(pinned.is_prepared());
            assert_eq!(pinned.args(), native_args);
            assert_eq!(pinned.original_args(), native_original);
            assert_eq!(pinned.attempt(), 0);
            assert!(player::native_launch(pinned.original_args()).is_err());
            called_tx.send(thread::current().id()).unwrap();
            Ok(())
        },
        move |args| {
            assert!(player::attached());
            assert_ne!(thread::current().id(), ui);
            count.fetch_add(1, AtomicOrdering::SeqCst);
            resolved_tx.send(thread::current().id()).unwrap();
            Ok(resolved(args))
        },
        requested,
        2,
        true,
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        resolution.recv_timeout(WAIT).unwrap(),
        called.recv_timeout(WAIT).unwrap()
    );
    joined(&mut game).unwrap();
    game.sync_native_launch().unwrap();
    assert!(game.launch.is_prepared());
    assert_eq!(game.launch.args(), expected_args);
    assert_eq!(game.launch.original_args(), expected_original);
    assert_eq!(game.local_page, 2);
    assert!(game.local_comparisons);
    let retry = game.launch.retry().unwrap();
    let retry_args = retry.args().to_vec();
    let mut next = spawn_game_with_native_preparation(
        move |args| {
            assert_eq!(args, retry_args);
            let pinned = player::native_launch(args)?;
            assert!(pinned.is_prepared());
            assert_eq!(pinned.attempt(), 1);
            assert_eq!(pinned.args()[3], "records/live.retry1.bkr");
            Ok(())
        },
        |_| panic!("an accepted retry must not rediscover native devices"),
        retry,
        0,
        false,
        false,
        None,
    )
    .unwrap();
    joined(&mut next).unwrap();
    next.sync_native_launch().unwrap();
    assert_eq!(next.launch.attempt(), 1);
    assert_eq!(resolves.load(AtomicOrdering::SeqCst), 1);
}

#[test]
fn held_resolution_has_no_early_pin_and_cancellation_prevents_native_effects_and_publication() {
    let native_calls = Arc::new(AtomicUsize::new(0));
    let count = native_calls.clone();
    let (entered_tx, entered) = mpsc::sync_channel(1);
    let (release_tx, release) = mpsc::sync_channel(1);
    let mut game = spawn_game_with_native_preparation(
        move |_| {
            count.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        },
        move |args| {
            assert!(player::attached());
            entered_tx.send(()).unwrap();
            release.recv_timeout(WAIT).map_err(|e| e.to_string())?;
            assert!(player::cancelled());
            Ok(resolved(args))
        },
        request(),
        0,
        false,
        false,
        None,
    )
    .unwrap();
    entered.recv_timeout(WAIT).unwrap();
    assert!(game.viewer.take_native_launch().unwrap().is_none());
    assert!(!game.launch.is_prepared());
    game.viewer.cancel();
    release_tx.send(()).unwrap();
    joined(&mut game).unwrap();
    let terminal = game.viewer.take_latest().unwrap();
    assert!(terminal.cancelled);
    assert_eq!(terminal.status, player::PlayerStatus::Finished);
    assert!(game.viewer.take_native_launch().unwrap().is_none());
    assert_eq!(native_calls.load(AtomicOrdering::SeqCst), 0);
    assert!(!game.launch.is_prepared());
}

#[test]
fn failed_discovery_publishes_no_pin_and_raw_f5_retry_can_recover_once() {
    let native_calls = Arc::new(AtomicUsize::new(0));
    let count = native_calls.clone();
    let mut failed = spawn_game_with_native_preparation(
        move |_| {
            count.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        },
        |_| Err("discovery rejected selected output".into()),
        request(),
        0,
        false,
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        joined(&mut failed).unwrap_err(),
        "discovery rejected selected output"
    );
    failed.sync_native_launch().unwrap();
    assert!(!failed.launch.is_prepared());
    assert_eq!(native_calls.load(AtomicOrdering::SeqCst), 0);
    assert!(failed.viewer.take_native_launch().unwrap().is_none());
    let retry = failed.launch.retry().unwrap();
    let retry_original = retry.original_args().to_vec();
    let (called_tx, called) = mpsc::sync_channel(1);
    let mut recovered = spawn_game_with_native_preparation(
        move |args| {
            let pinned = player::native_launch(args)?;
            assert!(pinned.is_prepared());
            assert_eq!(pinned.attempt(), 1);
            assert_eq!(pinned.original_args(), retry_original);
            assert_eq!(pinned.args()[3], "records/live.retry1.bkr");
            called_tx.send(()).unwrap();
            Ok(())
        },
        |args| Ok(resolved(args)),
        retry,
        0,
        false,
        false,
        None,
    )
    .unwrap();
    called.recv_timeout(WAIT).unwrap();
    joined(&mut recovered).unwrap();
    recovered.sync_native_launch().unwrap();
    let next = recovered.launch.retry().unwrap();
    assert_eq!(next.args()[3], "records/live.retry2.bkr");
    assert_eq!(next.args()[5], "exact");
}

#[test]
fn successful_but_foreign_resolution_is_rejected_before_pin_and_native_call() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mut game = spawn_game_with_native_preparation(
        move |_| {
            count.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        },
        |args| {
            let mut args = resolved(args);
            args[1] = "foreign.bms".into();
            Ok(args)
        },
        request(),
        0,
        false,
        false,
        None,
    )
    .unwrap();
    assert!(joined(&mut game).is_err());
    assert!(game.viewer.take_native_launch().unwrap().is_none());
    assert_eq!(calls.load(AtomicOrdering::SeqCst), 0);
    assert!(!game.launch.is_prepared());
}

#[test]
fn same_requested_lineage_cannot_publish_a_different_accepted_native_configuration() {
    let requested = request();
    let accepted = requested
        .clone()
        .prepare_with(|args| Ok(resolved(args)))
        .unwrap();
    let foreign = requested
        .prepare_with(|args| {
            let mut args = resolved(args);
            *args.last_mut().unwrap() = "different-output".into();
            Ok(args)
        })
        .unwrap();
    let mut game = spawn_game_with_native_preparation(
        |_| Ok(()),
        |_| panic!("prepared owner must bypass resolver"),
        foreign,
        0,
        false,
        false,
        None,
    )
    .unwrap();
    joined(&mut game).unwrap();
    // An already accepted UI owner receives the real worker's conflicting mailbox.
    // Requested lineage alone is insufficient authority to replace its template.
    game.launch = accepted;
    assert!(game.sync_native_launch().is_err());
    assert_eq!(game.launch.args().last().unwrap(), "selected-output");
}

fn requested_replay() -> SessionLaunch {
    let mut args = [
        "--chart",
        "selected.bms",
        "--replay",
        "records/selected.bkr",
        "--rate",
        "48000",
        "--channels",
        "2",
        "--buffer-frames",
        "1024",
        "--channel-policy",
        "mono-stereo",
        "--bgm-lookahead-ns",
        "9000000",
        "--replay-max-records",
        "321",
        "--replay-max-bytes",
        "1048576",
    ]
    .map(str::to_owned)
    .to_vec();
    if cfg!(target_os = "windows") {
        args = args
            .chunks_exact(2)
            .filter(|pair| {
                !matches!(
                    pair[0].as_str(),
                    "--rate" | "--channels" | "--buffer-frames"
                )
            })
            .flatten()
            .cloned()
            .collect();
    }
    if cfg!(target_os = "linux") {
        args.extend(["--alsa", "selected-output", "--period-frames", "256"].map(str::to_owned));
    } else {
        args.extend(
            [
                "--device",
                if cfg!(target_os = "macos") {
                    "1"
                } else {
                    "selected-output"
                },
            ]
            .map(str::to_owned),
        );
    }
    SessionLaunch::new(args).unwrap()
}

#[test]
#[cfg(not(target_os = "windows"))]
fn real_replay_projection_worker_and_repeated_retry_validate_the_accepted_configuration() {
    let mut app = super::tests::lifecycle_fixture();
    app.validate_replay = crate::app::validate_replay;
    app.validate_prepared_replay = Some(crate::app::validate_prepared_replay);
    let requested = requested_replay();
    app.validate_launch(&requested, true).unwrap();
    let original = requested.original_args().to_vec();
    let canonical = crate::auto_native::syntax_replay(requested.args()).unwrap();
    // Requested settings and canonical parser arguments deliberately have different
    // namespaces. Sending accepted arguments through the initial projection fails.
    assert!(crate::app::validate_replay(&canonical).is_err());
    crate::app::validate_prepared_replay(&canonical).unwrap();
    let mut launch = requested;
    for attempt in 0..=3 {
        let expected = canonical.clone();
        let mut game = spawn_game_with_native_preparation(
            move |args| {
                assert_eq!(args, expected);
                crate::app::validate_prepared_replay(args).map_err(|error| error.to_string())?;
                assert!(player::native_launch(args)?.is_prepared());
                Ok(())
            },
            |args| crate::auto_native::syntax_replay(args).map_err(|error| error.to_string()),
            launch,
            0,
            false,
            true,
            None,
        )
        .unwrap();
        joined(&mut game).unwrap();
        game.sync_native_launch().unwrap();
        assert_eq!(game.launch.attempt(), attempt);
        assert_eq!(game.launch.original_args(), original);
        assert_eq!(game.launch.args(), canonical);
        for pair in [
            ["--lookahead-ns", "9000000"],
            ["--max-records", "321"],
            ["--max-bytes", "1048576"],
            ["--buffer-frames", "1024"],
            ["--rate", "48000"],
            ["--channels", "2"],
            ["--channel-policy", "mono-stereo"],
        ] {
            assert!(game
                .launch
                .args()
                .chunks_exact(2)
                .any(|value| value == pair));
        }
        assert!(game
            .launch
            .args()
            .chunks_exact(2)
            .any(|value| value[0] == "--device"
                && value[1]
                    == if cfg!(target_os = "macos") {
                        "1"
                    } else {
                        "selected-output"
                    }));
        app.validate_launch(&game.launch, true).unwrap();
        launch = game.launch.retry().unwrap();
        app.validate_launch(&launch, true).unwrap();
    }
}

#[test]
fn replay_validation_selects_current_launch_state_and_preserves_custom_injection() {
    fn reject_canonical(_: &[String]) -> Result<(), Box<dyn Error>> {
        Err("canonical validator selected".into())
    }
    fn reject_requested(_: &[String]) -> Result<(), Box<dyn Error>> {
        Err("custom requested validator selected".into())
    }
    let requested = requested_replay();
    let accepted = requested
        .clone()
        .prepare_with(|args| crate::auto_native::syntax_replay(args).map_err(|e| e.to_string()))
        .unwrap();
    let mut app = super::tests::lifecycle_fixture();
    app.validate_replay = crate::app::validate_replay;
    app.validate_prepared_replay = Some(reject_canonical);
    app.validate_launch(&requested.retry().unwrap(), true)
        .unwrap();
    assert_eq!(
        app.validate_launch(&accepted.retry().unwrap(), true)
            .unwrap_err()
            .to_string(),
        "canonical validator selected"
    );
    app.validate_replay = reject_requested;
    app.validate_prepared_replay = None;
    for launch in [&requested, &accepted] {
        assert_eq!(
            app.validate_launch(launch, true).unwrap_err().to_string(),
            "custom requested validator selected"
        );
    }
}

#[test]
fn rejected_canonical_replay_retry_preserves_the_joined_owner_without_cancellation() {
    // A canonical capacity rejected by the real parser must never be dropped by
    // reprojecting accepted options into settings and supplying parser defaults.
    let requested = SessionLaunch::new(
        [
            "--chart",
            "selected.bms",
            "--replay",
            "records/selected.bkr",
        ]
        .map(str::to_owned)
        .to_vec(),
    )
    .unwrap();
    crate::app::validate_replay(requested.args()).unwrap();
    let invalid = requested
        .prepare_with(|args| {
            let mut canonical =
                crate::auto_native::syntax_replay(args).map_err(|e| e.to_string())?;
            canonical.extend(["--max-records", "0"].map(str::to_owned));
            Ok(canonical)
        })
        .unwrap();
    assert!(crate::app::validate_prepared_replay(invalid.args()).is_err());
    let mut game = spawn_game_with_native_preparation(
        |_| Ok(()),
        |_| panic!("accepted launch cannot rediscover"),
        invalid,
        2,
        true,
        true,
        None,
    )
    .unwrap();
    joined(&mut game).unwrap();
    game.sync_native_launch().unwrap();
    let original = game.launch.args().to_vec();
    let mut app = super::tests::lifecycle_fixture();
    app.validate_replay = crate::app::validate_replay;
    app.validate_prepared_replay = Some(crate::app::validate_prepared_replay);
    let route = app
        .prepare_route(ScreenRoute::Play { replay: true })
        .unwrap();
    app.commit_route(route);
    app.game = Some(game);
    app.request_retry();
    let retained = app.game.as_ref().unwrap();
    assert!(retained.joined);
    assert!(!retained.cancelling);
    assert!(retained.prepared_retry.is_none());
    assert_eq!(retained.launch.args(), original);
    assert_eq!(retained.launch.attempt(), 0);
    assert_eq!(retained.local_page, 2);
    assert!(retained.local_comparisons);
    assert!(app.failure.as_deref().unwrap().contains("retry preflight"));
}

#[test]
fn deferred_requested_replay_retry_rebases_to_the_joined_canonical_publication() {
    fn accepted_native(args: &[String]) -> Result<(), Box<dyn Error>> {
        crate::app::validate_prepared_replay(args)?;
        assert_eq!(player::native_launch(args)?.attempt(), 1);
        Ok(())
    }
    let requested = requested_replay();
    let original = requested.original_args().to_vec();
    let accepted = requested
        .clone()
        .prepare_with(|args| {
            crate::auto_native::syntax_replay(args).map_err(|error| error.to_string())
        })
        .unwrap();
    let canonical = accepted.args().to_vec();
    let (publisher, viewer) = player::channel();
    let (entered_tx, entered) = mpsc::sync_channel(1);
    let (release_tx, release) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        player::with_publisher(publisher, || {
            entered_tx.send(()).unwrap();
            release
                .recv_timeout(WAIT)
                .map_err(|error| error.to_string())?;
            // Model a publication committed by the old owner during final cleanup;
            // it remains authoritative even when UI preflight preceded the mailbox.
            assert!(player::cancelled());
            player::pin_native_launch(accepted)?;
            Ok(())
        })
    });
    entered.recv_timeout(WAIT).unwrap();
    let mut app = super::tests::lifecycle_fixture();
    app.validate_replay = crate::app::validate_replay;
    app.validate_prepared_replay = Some(crate::app::validate_prepared_replay);
    app.replay = accepted_native;
    let route = app
        .prepare_route(ScreenRoute::Play { replay: true })
        .unwrap();
    app.commit_route(route);
    app.game = Some(Game {
        viewer,
        worker: Some(worker),
        snapshot: None,
        completed_results: None,
        completed_results_error: None,
        cancelling: false,
        joined: false,
        local_page: 2,
        local_comparisons: true,
        replay: true,
        launch: requested,
        prepared_retry: None,
        practice_bookmark: None,
        practice_loop: None,
        loop_enabled: false,
    });
    app.request_retry();
    let pending = app.game.as_ref().unwrap();
    assert!(pending.cancelling);
    let preflight = pending.prepared_retry.as_ref().unwrap();
    assert!(!preflight.is_prepared());
    assert_eq!(preflight.attempt(), 1);
    assert_eq!(preflight.original_args(), original);
    release_tx.send(()).unwrap();
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
            "cleanup publication worker did not finish"
        );
        thread::sleep(Duration::from_millis(1));
    }
    app.collect_game();
    assert!(app.failure.is_none(), "{:?}", app.failure);
    let next = app.game.as_mut().unwrap();
    assert!(!next.joined);
    assert!(next.launch.is_prepared());
    assert_eq!(next.launch.attempt(), 1);
    assert_eq!(next.launch.original_args(), original);
    assert_eq!(next.launch.args(), canonical);
    joined(next).unwrap();
    next.sync_native_launch().unwrap();
}
