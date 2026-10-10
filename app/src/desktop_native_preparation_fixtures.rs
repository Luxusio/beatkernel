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
