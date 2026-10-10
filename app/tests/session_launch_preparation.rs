//! Resolved native invocations preserve the original requested retry lineage.
use beatkernel_bms_runtime::{
    player, practice::PracticeStart, practice_loop::PracticeLoop, session_launch::SessionLaunch,
};
use std::cell::Cell;

fn requested() -> Vec<String> {
    [
        "--chart",
        "chart.bms",
        "--record-replay",
        "records/play.bkr",
        "--rate",
        "48000",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn resolve(args: &[String]) -> Result<Vec<String>, String> {
    let mut args = args.to_vec();
    args.extend(["--channel-policy", "exact", "--output", "accepted-output"].map(str::to_owned));
    Ok(args)
}

fn value<'a>(launch: &'a SessionLaunch, flag: &str) -> Option<&'a str> {
    launch
        .args()
        .chunks_exact(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

#[test]
fn resolved_initial_and_retries_keep_exact_configuration_and_original_lineage() {
    let original = requested();
    let raw = SessionLaunch::new(original.clone()).unwrap();
    assert!(!raw.is_prepared());
    let accepted = raw
        .prepare_with(|args| {
            assert_eq!(args, original);
            resolve(args)
        })
        .unwrap();
    assert!(accepted.is_prepared());
    assert_eq!(accepted.original_args(), original);
    assert_eq!(accepted.args(), resolve(&original).unwrap());
    let first = accepted.retry().unwrap();
    let second = first.retry().unwrap();
    for launch in [&accepted, &first, &second] {
        assert!(launch.is_prepared());
        assert_eq!(launch.original_args(), original);
        assert_eq!(value(launch, "--channel-policy"), Some("exact"));
        assert_eq!(value(launch, "--output"), Some("accepted-output"));
        assert_eq!(value(launch, "--rate"), Some("48000"));
    }
    assert_eq!(first.attempt(), 1);
    assert_eq!(second.attempt(), 2);
    assert_eq!(
        value(&first, "--record-replay"),
        Some("records/play.retry1.bkr")
    );
    assert_eq!(
        value(&second, "--record-replay"),
        Some("records/play.retry2.bkr")
    );
    assert_eq!(
        value(&accepted, "--record-replay"),
        Some("records/play.bkr")
    );
}

#[test]
fn bookmark_loop_and_joined_rebase_use_accepted_template_without_suffix_accumulation() {
    let initial = SessionLaunch::new(requested())
        .unwrap()
        .prepare_with(resolve)
        .unwrap();
    let start = PracticeStart::from_nanoseconds(123).unwrap();
    let end = PracticeStart::from_nanoseconds(456).unwrap();
    let selected = initial
        .retry_loop(PracticeLoop::new(start, end).unwrap())
        .unwrap();
    let final_owner = initial.retry().unwrap().retry().unwrap();
    let rebased = final_owner.rebase_prepared_retry(&selected).unwrap();
    assert_eq!(rebased.attempt(), 3);
    assert_eq!(value(&rebased, "--start-ns"), Some("123"));
    assert_eq!(value(&rebased, "--end-ns"), Some("456"));
    assert_eq!(
        value(&rebased, "--record-replay"),
        Some("records/play.retry3.bkr")
    );
    assert_eq!(value(&rebased, "--output"), Some("accepted-output"));
    assert_eq!(rebased.original_args(), initial.original_args());
    let ordinary = rebased.retry().unwrap();
    assert_eq!(value(&ordinary, "--start-ns"), None);
    assert_eq!(value(&ordinary, "--end-ns"), None);
    assert_eq!(
        value(&ordinary, "--record-replay"),
        Some("records/play.retry4.bkr")
    );
    let bookmarked = ordinary.retry_from(start).unwrap();
    assert_eq!(bookmarked.attempt(), 5);
    assert_eq!(value(&bookmarked, "--start-ns"), Some("123"));
    assert_eq!(value(&bookmarked, "--channel-policy"), Some("exact"));
}

#[test]
fn member_recording_projection_preserves_accepted_defaults_and_member_retry_base() {
    let launch = SessionLaunch::new(requested())
        .unwrap()
        .prepare_with(resolve)
        .unwrap()
        .retry()
        .unwrap()
        .retry()
        .unwrap();
    let member = launch
        .for_recording_path(
            "records/play.p2.bkr".into(),
            "records/play.retry2.p2.bkr".into(),
        )
        .unwrap();
    assert!(member.is_prepared());
    assert_eq!(member.attempt(), 2);
    assert_eq!(member.original_args()[3], "records/play.p2.bkr");
    assert_eq!(value(&member, "--output"), Some("accepted-output"));
    let retry = member.retry().unwrap();
    assert_eq!(
        value(&retry, "--record-replay"),
        Some("records/play.p2.retry3.bkr")
    );
    assert_eq!(value(&retry, "--channel-policy"), Some("exact"));
    assert_eq!(
        value(&launch, "--record-replay"),
        Some("records/play.retry2.bkr")
    );
}

#[test]
fn prepared_initial_and_retry_refuse_second_resolution_without_calling_resolver() {
    let initial = SessionLaunch::new(requested())
        .unwrap()
        .prepare_with(resolve)
        .unwrap();
    for launch in [initial.clone(), initial.retry().unwrap()] {
        let called = Cell::new(false);
        assert!(launch
            .prepare_with(|_| {
                called.set(true);
                Ok(requested())
            })
            .is_err());
        assert!(!called.get());
    }
}

#[test]
fn resolution_refuses_identity_explicit_option_and_invalid_argument_drift() {
    for (flag, replacement) in [
        ("--chart", "different.bms"),
        ("--record-replay", "different.bkr"),
        ("--rate", "44100"),
    ] {
        let launch = SessionLaunch::new(requested()).unwrap();
        assert!(launch
            .prepare_with(|args| {
                let mut args = resolve(args)?;
                let pair = args
                    .chunks_exact_mut(2)
                    .find(|pair| pair[0] == flag)
                    .unwrap();
                pair[1] = replacement.into();
                Ok(args)
            })
            .is_err());
    }
    let replay = SessionLaunch::new(
        ["--chart", "chart.bms", "--replay", "capture.bkr"]
            .map(str::to_owned)
            .to_vec(),
    )
    .unwrap();
    assert!(replay
        .prepare_with(|_| Ok(["--chart", "chart.bms", "--replay", "foreign.bkr"]
            .map(str::to_owned)
            .to_vec()))
        .is_err());
    assert!(SessionLaunch::new(requested())
        .unwrap()
        .prepare_with(|_| Ok(vec!["--chart".into()]))
        .is_err());
    assert!(SessionLaunch::new(requested())
        .unwrap()
        .prepare_with(|_| Err("discovery failed".into()))
        .is_err());
}

#[test]
fn pinned_initial_and_retry_require_exact_resolved_native_invocation() {
    let initial = SessionLaunch::new(requested())
        .unwrap()
        .prepare_with(resolve)
        .unwrap();
    for launch in [initial.clone(), initial.retry().unwrap()] {
        let (publisher, viewer) = player::channel();
        player::with_publisher(publisher, || {
            player::pin_native_launch(launch.clone())?;
            let pinned = player::native_launch(launch.args())?;
            assert_eq!(pinned.args(), launch.args());
            assert_eq!(pinned.original_args(), requested());
            assert_eq!(pinned.attempt(), launch.attempt());
            assert!(player::native_launch(&requested()).is_err());
            let mut changed = launch.args().to_vec();
            changed.extend(["--buffer-frames".into(), "64".into()]);
            assert!(player::native_launch(&changed).is_err());
            Ok(())
        })
        .unwrap();
        let published = viewer.take_native_launch().unwrap().unwrap();
        assert_eq!(published.args(), launch.args());
        assert!(published.is_prepared());
    }
}

#[test]
fn first_resolution_after_failed_attempt_preserves_current_override_and_original_retry_base() {
    let original = SessionLaunch::new(requested()).unwrap();
    assert!(original
        .clone()
        .prepare_with(|_| Err("discovery failed".into()))
        .is_err());
    let selected = original
        .retry_from(PracticeStart::from_nanoseconds(123).unwrap())
        .unwrap();
    let recovered = selected.prepare_with(resolve).unwrap();
    assert_eq!(recovered.attempt(), 1);
    assert_eq!(recovered.original_args(), original.original_args());
    assert_eq!(value(&recovered, "--start-ns"), Some("123"));
    assert_eq!(
        value(&recovered, "--record-replay"),
        Some("records/play.retry1.bkr")
    );
    let retry = recovered.retry().unwrap();
    assert_eq!(value(&retry, "--start-ns"), None);
    assert_eq!(
        value(&retry, "--record-replay"),
        Some("records/play.retry2.bkr")
    );
    assert_eq!(value(&retry, "--output"), Some("accepted-output"));
}

#[test]
fn unresolved_selected_retry_rebases_onto_accepted_defaults_but_foreign_templates_refuse() {
    let raw = SessionLaunch::new(requested()).unwrap();
    let selected = raw
        .retry_loop(
            PracticeLoop::new(
                PracticeStart::from_nanoseconds(123).unwrap(),
                PracticeStart::from_nanoseconds(456).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let accepted = raw.clone().prepare_with(resolve).unwrap();
    let rebased = accepted
        .retry()
        .unwrap()
        .rebase_prepared_retry(&selected)
        .unwrap();
    assert!(rebased.is_prepared());
    assert_eq!(rebased.attempt(), 2);
    assert_eq!(value(&rebased, "--start-ns"), Some("123"));
    assert_eq!(value(&rebased, "--end-ns"), Some("456"));
    assert_eq!(value(&rebased, "--output"), Some("accepted-output"));
    assert_eq!(
        value(&rebased, "--record-replay"),
        Some("records/play.retry2.bkr")
    );
    let foreign = raw
        .prepare_with(|args| {
            let mut args = resolve(args)?;
            let pair = args
                .chunks_exact_mut(2)
                .find(|pair| pair[0] == "--output")
                .unwrap();
            pair[1] = "different-output".into();
            Ok(args)
        })
        .unwrap()
        .retry()
        .unwrap();
    assert!(accepted.rebase_prepared_retry(&foreign).is_err());
}

#[test]
fn replay_projection_does_not_reintroduce_live_section_options_on_retry() {
    let requested = [
        "--chart",
        "chart.bms",
        "--replay",
        "capture.bkr",
        "--start-ns",
        "123",
        "--end-ns",
        "456",
    ]
    .map(str::to_owned)
    .to_vec();
    let projected = SessionLaunch::new(requested.clone())
        .unwrap()
        .prepare_with(|_| {
            Ok([
                "--chart",
                "chart.bms",
                "--replay",
                "capture.bkr",
                "--device",
                "accepted-output",
            ]
            .map(str::to_owned)
            .to_vec())
        })
        .unwrap();
    assert_eq!(projected.original_args(), requested);
    for launch in [&projected, &projected.retry().unwrap()] {
        assert!(launch.is_prepared());
        assert_eq!(value(launch, "--start-ns"), None);
        assert_eq!(value(launch, "--end-ns"), None);
        assert_eq!(value(launch, "--replay"), Some("capture.bkr"));
        assert_eq!(value(launch, "--device"), Some("accepted-output"));
    }
}
