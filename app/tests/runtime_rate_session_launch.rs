//! Runtime retry arguments change the returned invocation, never its F5 lineage.
use beatkernel_bms_runtime::{
    practice::PracticeStart, practice_loop::PracticeLoop, replay_speed::ReplaySpeed,
    session_launch::SessionLaunch,
};

fn speed(value: &str) -> ReplaySpeed {
    value.parse().unwrap()
}
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).into()).collect()
}
fn option<'a>(launch: &'a SessionLaunch, flag: &str) -> Option<&'a str> {
    let mut found = launch.args().chunks_exact(2).filter(|p| p[0] == flag);
    let first = found.next().map(|p| p[1].as_str());
    assert!(found.next().is_none(), "duplicate {flag}");
    first
}
fn original() -> SessionLaunch {
    SessionLaunch::new(args(&[
        "--chart",
        "song.bms",
        "--speed",
        "6/4",
        "--start-ns",
        "100",
        "--end-ns",
        "900",
        "--preroll-ns",
        "3000000000",
        "--backend",
        "asio",
        "--buffer-frames",
        "64",
        "--local-player",
        "7:keyboard-a",
        "--local-player",
        "4294967295:keyboard-b",
        "--record-replay",
        "records/song.take.bkr",
    ]))
    .unwrap()
}

#[test]
fn different_runtime_speed_is_canonical_and_f5_restores_original_spelling_and_section() {
    let pinned = original();
    let before = pinned.args().to_vec();
    let first = pinned.retry_with_speed(speed("10/4")).unwrap();
    assert_eq!(first.attempt(), 1);
    assert_eq!(option(&first, "--speed"), Some("5/2"));
    assert_eq!(
        option(&first, "--record-replay"),
        Some("records/song.take.retry1.bkr")
    );
    assert_eq!(option(&first, "--preroll-ns"), Some("3000000000"));
    assert_eq!(first.original_args(), before);
    let second = first.retry_with_speed(speed("1/2")).unwrap();
    assert_eq!(second.attempt(), 2);
    assert_eq!(option(&second, "--speed"), Some("1/2"));
    assert_eq!(
        option(&second, "--record-replay"),
        Some("records/song.take.retry2.bkr")
    );
    let f5 = second.retry().unwrap();
    assert_eq!(f5.attempt(), 3);
    assert_eq!(option(&f5, "--speed"), Some("6/4"));
    assert_eq!(option(&f5, "--start-ns"), Some("100"));
    assert_eq!(option(&f5, "--end-ns"), Some("900"));
    assert_eq!(
        option(&f5, "--record-replay"),
        Some("records/song.take.retry3.bkr")
    );
    assert_eq!(pinned.args(), before);
    assert_eq!(pinned.attempt(), 0);
}

#[test]
fn equal_reduced_original_rate_preserves_argument_bytes_exactly() {
    let pinned = original();
    assert_eq!(
        pinned.retry_with_speed(speed("3/2")).unwrap().args(),
        pinned.retry().unwrap().args()
    );
    let changed = pinned.retry_with_speed(speed("2/1")).unwrap();
    assert_eq!(
        changed.retry_with_speed(speed("12/8")).unwrap().args(),
        changed.retry().unwrap().args()
    );
}

#[test]
fn implicit_normal_stays_absent_while_different_rate_is_added_once() {
    let pinned = SessionLaunch::new(args(&["--chart", "song.bms"])).unwrap();
    assert_eq!(
        pinned.retry_with_speed(speed("2/2")).unwrap().args(),
        pinned.args()
    );
    let one = pinned.retry_with_speed(speed("3/2")).unwrap();
    assert_eq!(option(&one, "--speed"), Some("3/2"));
    let two = one.retry_with_speed(speed("5/3")).unwrap();
    assert_eq!(option(&two, "--speed"), Some("5/3"));
    assert_eq!(two.args().len(), pinned.args().len() + 2);
    assert_eq!(two.retry().unwrap().args(), pinned.args());
    assert_eq!(two.original_args(), pinned.args());
}

#[test]
fn prepared_native_defaults_remain_pinned_across_runtime_override() {
    let pinned = SessionLaunch::new(args(&[
        "--chart",
        "song.bms",
        "--record-replay",
        "take.bkr",
    ]))
    .unwrap()
    .prepare_with(|requested| {
        let mut accepted = requested.to_vec();
        accepted.extend(args(&[
            "--backend",
            "wasapi",
            "--buffer-frames",
            "128",
            "--speed",
            "6/4",
        ]));
        Ok(accepted)
    })
    .unwrap();
    let requested = pinned.original_args().to_vec();
    let accepted = pinned.prepared_original_args().unwrap().to_vec();
    let retry = pinned.retry_with_speed(speed("1/2")).unwrap();
    assert!(retry.is_prepared());
    assert_eq!(retry.original_args(), requested);
    assert_eq!(retry.prepared_original_args(), Some(accepted.as_slice()));
    assert_eq!(option(&retry, "--speed"), Some("1/2"));
    assert_eq!(option(&retry, "--backend"), Some("wasapi"));
    assert_eq!(option(&retry.retry().unwrap(), "--speed"), Some("6/4"));
    assert_eq!(pinned.prepared_original_args(), Some(accepted.as_slice()));
}

#[test]
fn member_recording_projection_uses_each_original_base_and_global_ordinal() {
    let global = original().retry_with_speed(speed("2/1")).unwrap();
    for id in [7, u32::MAX] {
        let base = format!("records/song.take.p{id}.bkr");
        let member = global
            .for_recording_path(
                base.clone().into(),
                format!("records/song.take.retry1.p{id}.bkr").into(),
            )
            .unwrap();
        let next = member.retry_with_speed(speed("1/2")).unwrap();
        assert_eq!(next.attempt(), 2);
        assert_eq!(option(&next, "--speed"), Some("1/2"));
        assert_eq!(
            option(&next, "--record-replay"),
            Some(format!("records/song.take.p{id}.retry2.bkr").as_str())
        );
        assert_eq!(option(&next.retry().unwrap(), "--speed"), Some("6/4"));
        assert_eq!(
            next.original_args()
                .chunks_exact(2)
                .find(|p| p[0] == "--record-replay")
                .unwrap()[1],
            base
        );
    }
}

#[test]
fn scrub_and_loop_selection_do_not_repin_original_invocation() {
    let pinned = original();
    let region = PracticeLoop::new(
        PracticeStart::from_nanoseconds(200).unwrap(),
        PracticeStart::from_nanoseconds(400).unwrap(),
    )
    .unwrap();
    let selected = pinned.retry_loop(region).unwrap();
    let runtime = selected.retry_with_speed(speed("2/1")).unwrap();
    // retry_with_speed changes speed on retry()'s original section. Region
    // selection belongs to prepare_attempt_with_runtime_speed, not this helper.
    assert_eq!(option(&runtime, "--start-ns"), Some("100"));
    assert_eq!(option(&runtime, "--end-ns"), Some("900"));
    assert_eq!(runtime.original_args(), pinned.original_args());
    assert_eq!(option(&selected, "--start-ns"), Some("200"));
}

#[test]
fn duplicate_or_invalid_original_speed_fails_without_mutating_launch() {
    for fields in [
        args(&["--chart", "song.bms", "--speed", "1/2", "--speed", "1/1"]),
        args(&["--chart", "song.bms", "--speed", "0/1"]),
        args(&["--chart", "song.bms", "--speed", "bad"]),
    ] {
        let launch = SessionLaunch::new(fields.clone()).unwrap();
        assert!(launch.retry_with_speed(speed("3/2")).is_err());
        assert_eq!(launch.args(), fields);
        assert_eq!(launch.original_args(), fields);
        assert_eq!(launch.attempt(), 0);
    }
}
