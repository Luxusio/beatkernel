//! Deferred native competition fixtures; no transport or worker is constructed.
use super::*;
use crate::multiplayer_group::{decode_prefix, encode_prefix};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::{ReplayFile, encode_replay},
    runtime::Runtime,
    time::{ClockMapper, ClockMappingQuality, ClockPoint},
    transport::{Rate, Transport},
};

fn member(player: u32, song_ns: i64, hits: u64) -> MemberProgress {
    MemberProgress {
        player: PlayerId(player),
        progress: Progress {
            song_ns,
            hits,
            misses: 0,
            combo: hits,
            max_combo: hits,
        },
    }
}

fn prefix(members: Vec<MemberProgress>) -> GroupPrefix {
    GroupPrefix {
        sequence: 0,
        final_prefix: false,
        members,
    }
}

#[test]
fn accepted_ordinal_zero_preserves_full_width_identity_and_independent_frontier() {
    let first = member(u32::MAX, i64::MIN, u64::MAX);
    for count in [1usize, 3, 64] {
        let mut members = vec![first];
        members.extend((1..count).map(|index| member(index as u32, i64::MAX, index as u64)));
        let roster = members.iter().map(|entry| entry.player).collect::<Vec<_>>();
        let snapshot = prefix(members);
        assert_eq!(
            selected_remote_member(Some(&roster), Some(&snapshot)),
            Some(first)
        );
        assert_eq!(snapshot.members.len(), count);
    }

    // The local identity may occur later in an unequal remote roster. It is
    // neither a target selector nor a reason to combine that member's counts.
    let local_player = PlayerId(7);
    let remote = prefix(vec![
        member(91, -604_800_000_000_017, 3),
        member(local_player.0, 17, 8),
    ]);
    assert_eq!(
        selected_remote_member(Some(&[PlayerId(91), local_player]), Some(&remote)),
        Some(member(91, -604_800_000_000_017, 3))
    );
}

#[test]
fn malformed_later_rows_and_rosters_refuse_the_entire_comparison_without_fallback() {
    let accepted = prefix(vec![
        member(91, -10, 4),
        member(7, 20, 9),
        member(u32::MAX, 30, 1),
    ]);
    let roster = [PlayerId(91), PlayerId(7), PlayerId(u32::MAX)];
    let mut invalid = Vec::new();
    let mut changed = accepted.clone();
    changed.members[2].progress.combo = 2;
    invalid.push(changed);
    let mut changed = accepted.clone();
    changed.members[2].progress.max_combo = 2;
    invalid.push(changed);
    let mut changed = accepted.clone();
    changed.members[2].progress.misses = u64::MAX;
    invalid.push(changed);
    let mut changed = accepted.clone();
    changed.members[2].player = PlayerId(0);
    invalid.push(changed);
    let mut changed = accepted.clone();
    changed.members[2].player = roster[1];
    invalid.push(changed);
    let mut changed = accepted.clone();
    changed.members[2].player = PlayerId(8);
    invalid.push(changed);
    let mut changed = accepted.clone();
    changed.members.swap(1, 2);
    invalid.push(changed);
    let mut changed = accepted.clone();
    changed.members.pop();
    invalid.push(changed);
    invalid.push(prefix(Vec::new()));
    invalid.push(prefix((1..=65).map(|id| member(id, 0, 0)).collect()));
    for candidate in invalid {
        let before = candidate.clone();
        assert_eq!(
            selected_remote_member(Some(&roster), Some(&candidate)),
            None
        );
        assert_eq!(candidate, before);
    }
    for malformed in [
        vec![],
        vec![roster[0], PlayerId(0), roster[2]],
        vec![roster[0], roster[1], roster[1]],
        vec![roster[0], roster[2], roster[1]],
        vec![roster[0], roster[1]],
        (1..=65).map(PlayerId).collect(),
    ] {
        assert_eq!(
            selected_remote_member(Some(&malformed), Some(&accepted)),
            None
        );
    }
    assert_eq!(selected_remote_member(None, Some(&accepted)), None);
    assert_eq!(selected_remote_member(Some(&roster), None), None);
    assert_eq!(
        selected_remote_member(Some(&roster), Some(&accepted)),
        Some(accepted.members[0])
    );
}

#[test]
fn canonical_latest_and_final_prefixes_keep_their_own_member_times_and_presence() {
    let roster = [PlayerId(17), PlayerId(u32::MAX)];
    let ordinary = [member(17, -1, 0), member(u32::MAX, 604_800_000_000_001, 5)];
    let latest = decode_prefix(
        &encode_prefix(u64::MAX - 1, false, &ordinary).unwrap(),
        u64::MAX - 1,
        None,
    )
    .unwrap();
    assert!(!latest.final_prefix);
    assert_eq!(
        selected_remote_member(Some(&roster), Some(&latest)),
        Some(ordinary[0])
    );
    assert_eq!(selected_remote_member(Some(&roster), None), None);

    let terminal = [
        member(17, i64::MAX, u64::MAX),
        member(u32::MAX, 604_800_000_000_001, 5),
    ];
    let final_prefix = decode_prefix(
        &encode_prefix(u64::MAX, true, &terminal).unwrap(),
        u64::MAX,
        Some(&latest.members),
    )
    .unwrap();
    assert!(final_prefix.final_prefix);
    assert_eq!(final_prefix.sequence, u64::MAX);
    assert_eq!(
        selected_remote_member(Some(&roster), Some(&final_prefix)),
        Some(terminal[0])
    );
    assert_eq!(
        selected_remote_member(Some(&roster), Some(&latest)),
        Some(ordinary[0])
    );
    assert!(!latest.final_prefix);
}

struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

#[test]
fn offline_terminal_progress_uses_actual_reports_and_remote_selection_cannot_change_capture() {
    const HOST_ORIGIN: i64 = 604_800_000_000_017;
    let domain = ClockDomainId(17);
    let point = |song_ns| ClockPoint {
        domain,
        timestamp: Timestamp::from_nanos(HOST_ORIGIN + song_ns),
    };
    let source = parse_seeded(
        "#BPM 60\n#WAV01 key.wav\n#00011:0101",
        ParseOptions::default(),
        0,
    )
    .unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: beatkernel::time::Duration::ZERO,
                late: beatkernel::time::Duration::ZERO,
            }],
            beatkernel::time::Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let limits = replay_limits().unwrap();
    let mut capture = LiveReplayCapture::new(&judge, domain, limits).unwrap();
    let local_player = PlayerId(u32::MAX - 5);
    let mut owner = LiveCompetition {
        player: local_player,
        competition: Competition::new(capture.header().clone(), 0).unwrap(),
        network: None,
        last_publish: Some(2_000_000_000),
        last_display: None,
        network_failed: false,
        network_status: None,
        last_presentation: None,
        network_setup_timeout: Duration::from_secs(10),
    };
    assert_eq!(owner.terminal_prefix(), None);
    assert!(
        owner
            .await_network_ready(|| panic!("offline service must not run"))
            .unwrap()
    );
    assert!(
        owner
            .await_network_commit(|| panic!("offline service must not run"))
            .unwrap()
    );
    assert_eq!(owner.committed_start_schedule(), None);

    let (producer, _consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        domain,
        domain,
        Transport::new(point(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
        BindingMap::from_bindings([Binding {
            device: DeviceSelector::Exact(DeviceId(u64::MAX)),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(0x11),
        }])
        .unwrap(),
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    for (at, sequence, state) in [(0, 41, ButtonState::Down), (1, 42, ButtonState::Up)] {
        let report = runtime
            .process_input(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(u64::MAX), point(at), sequence),
                    control: PhysicalControlId::keyboard(4),
                    state,
                }),
                &Identity,
                point(at),
            )
            .unwrap();
        capture.record_report(&report).unwrap();
        owner
            .competition
            .observe(&report.judge_events, report.song_time)
            .unwrap();
    }
    assert_eq!(owner.terminal_prefix().unwrap().hits, 1);
    let report = runtime
        .advance_to(point(2_000_000_001), &Identity, point(2_000_000_001))
        .unwrap();
    capture.record_report(&report).unwrap();
    owner
        .competition
        .observe(&report.judge_events, report.song_time)
        .unwrap();
    let terminal = Progress {
        song_ns: 2_000_000_001,
        hits: 1,
        misses: 1,
        combo: 0,
        max_combo: 1,
    };
    assert_eq!(owner.terminal_prefix(), Some(terminal));
    assert_eq!(owner.player, local_player);
    assert_eq!(owner.last_publish, Some(2_000_000_000));
    assert_eq!(capture.records().len(), 3);
    let hash = runtime.judge().stable_hash().unwrap();
    let recorded = ReplayFile::new(capture.header().clone(), capture.records().to_vec());
    let bytes = encode_replay(&recorded, limits).unwrap();

    let remote = prefix(vec![
        member(91, -604_800_000_000_001, u64::MAX),
        member(local_player.0, 3, 0),
    ]);
    let roster = [PlayerId(91), local_player];
    assert_eq!(
        selected_remote_member(Some(&roster), Some(&remote)),
        Some(remote.members[0])
    );
    assert_eq!(selected_remote_member(Some(&roster), None), None);
    assert_eq!(owner.terminal_prefix(), Some(terminal));
    assert_eq!(runtime.judge().stable_hash().unwrap(), hash);
    assert_eq!(capture.into_bytes().unwrap(), bytes);
    assert!(owner.network.is_none());
    assert_eq!(owner.network_status, None);
}
