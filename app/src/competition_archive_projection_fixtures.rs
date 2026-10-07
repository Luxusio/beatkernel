//! Deferred retained projection without a presentation host or clock port.
use super::*;
use crate::replay_capture::LiveReplayCapture;
use beatkernel::{
    input::{
        ButtonEvent, ButtonState, DeviceId, EventMeta, GameInputEvent, PhysicalControlId,
        PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeProfile, JudgeWindow, JudgeGrade},
    replay::{
        ReplaySession,
        codec::{ReplayCodecLimits, ReplayFile},
    },
    input::CodecLimits,
    time::{ClockDomainId, ClockPoint, Duration},
};
fn competition(label: &str) -> Competition {
    let source =
        beatkernel_bms::parse("#BPM 60\n#WAV01 tap.wav\n#00011:01\n", Default::default()).unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let limits =
        ReplayCodecLimits::new(65536, 32, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    let header = LiveReplayCapture::new(&judge, ClockDomainId(9), limits)
        .unwrap()
        .header()
        .clone();
    let mut session = ReplaySession::new(header.clone(), judge).unwrap();
    session
        .push_input(
            GameInputEvent {
                game_control: source.notes[0].lane.control(),
                physical: PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(
                        DeviceId(u64::MAX),
                        ClockPoint {
                            domain: ClockDomainId(9),
                            timestamp: Timestamp::ZERO,
                        },
                        u64::MAX,
                    ),
                    control: PhysicalControlId::keyboard(4),
                    state: ButtonState::Down,
                }),
            },
            Timestamp::ZERO,
        )
        .unwrap();
    let mut competition = Competition::new(header.clone(), 1).unwrap();
    competition
        .add_replay(
            &source,
            ReplayFile::new(header, session.records().to_vec()),
            limits,
            OpponentKind::Own,
            label,
        )
        .unwrap();
    competition
        .observe(&[], Timestamp::from_nanos(9_007_199_254_740_993))
        .unwrap();
    competition
}
fn prefix(ids: &[PlayerId]) -> GroupPrefix {
    GroupPrefix {
        sequence: u64::MAX,
        final_prefix: true,
        members: ids
            .iter()
            .enumerate()
            .map(|(index, id)| MemberProgress {
                player: *id,
                progress: crate::multiplayer::Progress {
                    song_ns: 9_007_199_254_740_993 + index as i64,
                    hits: u64::MAX - index as u64,
                    misses: 0,
                    combo: 0,
                    max_combo: 0,
                },
            })
            .collect(),
    }
}
#[test]
fn pure_projection_preserves_actual_saved_prefix_bounded_basename_and_owner_without_time_or_ui_mutation()
 {
    let competition = competition("directory\\own.bkr");
    let before = competition.score().clone();
    let time = competition.song_time();
    let projected = project_archive_snapshot(PlayerId(u32::MAX), &competition, None).unwrap();
    assert_eq!(
        projected.ghosts,
        [GhostSnapshot {
            kind: OpponentKind::Own,
            label: "own.bkr".into(),
            hits: 1,
            misses: 0,
            combo: 1,
            max_combo: 1,
            recorded_until: Some(Timestamp::ZERO)
        }]
    );
    assert!(projected.network.is_none());
    assert_eq!(competition.score(), &before);
    assert_eq!(competition.song_time(), time);
    assert!(project_archive_snapshot(PlayerId(0), &competition, None).is_err());
    for (source, expected) in [
        (
            format!("{}/name\n.bkr", "long-directory".repeat(30)),
            "name.bkr".to_owned(),
        ),
        ("\n\0".into(), "RECORD".into()),
        ("a".repeat(65), "a".repeat(64)),
    ] {
        assert_eq!(
            project_archive_snapshot(PlayerId(7), &self::competition(&source), None)
                .unwrap()
                .ghosts[0]
                .label,
            expected
        );
    }
}
#[test]
fn exact_original_roster_mapping_retains_disconnected_prefix_and_explicit_unaffected_members() {
    let local = [PlayerId(u32::MAX), PlayerId(7), PlayerId(91)];
    let remote = [PlayerId(2), PlayerId(9)];
    let prefix = prefix(&remote);
    let rows = project_archive_network(
        false,
        &local,
        NetworkStatus::Disconnected,
        Some(&remote),
        Some(&prefix),
    )
    .unwrap();
    assert_eq!(rows.iter().map(|row| row.0).collect::<Vec<_>>(), local);
    assert_eq!(rows[0].1.progress, Some(prefix.members[0].progress));
    assert_eq!(rows[1].1.progress, Some(prefix.members[1].progress));
    assert_eq!(rows[2].1.progress, None);
    assert!(
        rows.iter()
            .all(|row| row.1.status == NetworkStatus::Disconnected)
    );
    let saved = competition("own.bkr");
    let solo = project_archive_snapshot(
        local[0],
        &saved,
        Some(SoloNetworkPresentation {
            status: Some(NetworkStatus::Stopped),
            roster: Some(&remote),
            prefix: Some(&prefix),
        }),
    )
    .unwrap();
    assert_eq!(
        solo.network.unwrap().progress,
        Some(prefix.members[0].progress)
    );
}
#[test]
fn later_bad_peer_row_changed_order_missing_roster_and_duplicate_ids_refuse_before_partial_projection()
 {
    let local = [PlayerId(u32::MAX)];
    let remote = [PlayerId(7), PlayerId(91)];
    let valid = prefix(&remote);
    let mut bad = valid.clone();
    bad.members[1].progress.combo = 1;
    assert!(
        project_archive_network(
            false,
            &local,
            NetworkStatus::Connected,
            Some(&remote),
            Some(&bad)
        )
        .is_err()
    );
    let mut reversed = valid.clone();
    reversed.members.reverse();
    assert!(
        project_archive_network(
            false,
            &local,
            NetworkStatus::Connected,
            Some(&remote),
            Some(&reversed)
        )
        .is_err()
    );
    assert!(
        project_archive_network(false, &local, NetworkStatus::Connected, None, Some(&valid))
            .is_err()
    );
    assert!(
        project_archive_network(
            false,
            &[PlayerId(7), PlayerId(7)],
            NetworkStatus::Connected,
            Some(&remote),
            Some(&valid)
        )
        .is_err()
    );
    let saved = competition("own.bkr");
    assert!(
        project_archive_snapshot(
            local[0],
            &saved,
            Some(SoloNetworkPresentation {
                status: Some(NetworkStatus::Connected),
                roster: Some(&remote),
                prefix: Some(&bad)
            })
        )
        .is_err()
    );
}
#[test]
fn room_projection_skips_unknown_peer_mapping_and_waiting_without_progress_remains_explicit() {
    let local = [PlayerId(u32::MAX), PlayerId(7)];
    let remote = [PlayerId(91)];
    let prefix = prefix(&remote);
    assert!(
        project_archive_network(
            true,
            &local,
            NetworkStatus::Stopped,
            Some(&remote),
            Some(&prefix)
        )
        .unwrap()
        .is_empty()
    );
    let rows = project_archive_network(false, &local, NetworkStatus::Waiting, None, None).unwrap();
    assert!(rows.iter().all(|row| row.1
        == NetworkSnapshot {
            status: NetworkStatus::Waiting,
            progress: None
        }));
    assert!(
        project_archive_network(true, &[PlayerId(0)], NetworkStatus::Stopped, None, None).is_err()
    );
}
