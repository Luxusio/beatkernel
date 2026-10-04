//! Deferred actual recorded-prefix composition; the HUD has no clock or judge authority.
use crate::{
    competition::OpponentKind, replay_capture::LiveReplayCapture,
    saved_opponent_hud::SavedOpponentHud, saved_opponents::SavedOpponents,
};
#[cfg(feature = "graphics")]
use crate::{competition::ScoreSummary, scene::Scene, ui::organisms::competition_scoreboard};
use beatkernel::{
    input::{
        ButtonEvent, ButtonState, CodecLimits, DeviceId, EventMeta, GameInputEvent,
        PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::{
        ReplayHeader, ReplaySession,
        codec::{ReplayCodecLimits, ReplayFile, encode_replay},
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{BmsChart, parse_seeded};
use crate::{
    multiplayer_protocol::{Progress, validate_progress},
    player::NetworkStatus,
};

const HIT: i64 = 999_999_990;
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(1 << 20, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn recording(complete: bool) -> (BmsChart, ReplayHeader, Vec<u8>) {
    let source = parse_seeded(
        "#BPM 60\n#WAV01 key.wav\n#00011:00010001\n",
        Default::default(),
        u64::MAX,
    )
    .unwrap();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::from_nanos(10),
    )
    .unwrap();
    let judge = JudgeEngine::new(source.compile().unwrap().chart, source.rules(), profile).unwrap();
    let header = LiveReplayCapture::new_at_with_chart_seed(
        &judge,
        ClockDomainId(17),
        limits(),
        Timestamp::ZERO,
        u64::MAX,
    )
    .unwrap()
    .header()
    .clone();
    let mut replay = ReplaySession::new(header.clone(), judge).unwrap();
    replay.advance_to(ts(-100_000_000)).unwrap();
    replay
        .push_input(
            GameInputEvent {
                game_control: source.notes[0].lane.control(),
                physical: PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(
                        DeviceId(u64::MAX),
                        ClockPoint {
                            domain: ClockDomainId(17),
                            timestamp: ts(99),
                        },
                        u64::MAX,
                    ),
                    control: PhysicalControlId::keyboard(4),
                    state: ButtonState::Down,
                }),
            },
            ts(HIT),
        )
        .unwrap();
    if complete {
        replay.advance_to(ts(3_000_000_000)).unwrap();
    }
    let bytes = encode_replay(
        &ReplayFile::new(header.clone(), replay.records().to_vec()),
        limits(),
    )
    .unwrap();
    (source, header, bytes)
}
fn saved(header: ReplayHeader) -> SavedOpponents {
    SavedOpponents::new(header, limits(), 8, 64 << 20).unwrap()
}

fn peer_words(progress: Progress) -> [u32; 10] {
    let values = [
        progress.song_ns as u64,
        progress.hits,
        progress.misses,
        progress.combo,
        progress.max_combo,
    ];
    let mut words = [0; 10];
    for (index, value) in values.into_iter().enumerate() {
        words[index * 2] = value as u32;
        words[index * 2 + 1] = (value >> 32) as u32;
    }
    words
}

#[test]
fn peer_hud_preserves_signed_song_and_full_width_counts_without_advancing_any_clock() {
    let mut hud = SavedOpponentHud::default();
    hud.update_peer(0, &[]).unwrap();
    let waiting = hud.snapshot().unwrap().network.as_ref().unwrap();
    assert_eq!(waiting.status, NetworkStatus::Waiting);
    assert!(waiting.progress.is_none());
    let first = Progress {
        song_ns: i64::MIN,
        hits: u64::MAX,
        misses: 0,
        combo: 0,
        max_combo: 1,
    };
    validate_progress(None, first).unwrap();
    // Independent numeric boundary: ten low/high words, no Number-sized intermediate.
    hud.update_peer(1, &[0, 0x8000_0000, u32::MAX, u32::MAX, 0, 0, 0, 0, 1, 0])
        .unwrap();
    assert_eq!(
        hud.snapshot().unwrap().network.as_ref().unwrap().progress,
        Some(first)
    );
    let week = Progress {
        song_ns: 604_800_000_000_001,
        ..first
    };
    validate_progress(Some(first), week).unwrap();
    hud.update_peer(1, &peer_words(week)).unwrap();
    let retained = hud.snapshot().unwrap().clone();
    for _ in 0..4 {
        assert_eq!(hud.snapshot(), Some(&retained));
    }
    hud.update_peer(2, &[]).unwrap();
    let disconnected = hud.snapshot().unwrap().network.as_ref().unwrap();
    assert_eq!(disconnected.status, NetworkStatus::Disconnected);
    assert_eq!(disconnected.progress, Some(week));
    hud.update_peer(3, &[]).unwrap();
    assert_eq!(
        hud.snapshot().unwrap().network.as_ref().unwrap().status,
        NetworkStatus::Stopped
    );
    assert_eq!(
        hud.snapshot().unwrap().network.as_ref().unwrap().progress,
        Some(week)
    );
    let mut maximum_misses = SavedOpponentHud::default();
    let missed = Progress {
        song_ns: -1,
        hits: 0,
        misses: u64::MAX,
        combo: 0,
        max_combo: 0,
    };
    maximum_misses.update_peer(1, &peer_words(missed)).unwrap();
    assert_eq!(
        maximum_misses
            .snapshot()
            .unwrap()
            .network
            .as_ref()
            .unwrap()
            .progress,
        Some(missed)
    );
}

#[test]
fn peer_hud_refusals_preserve_prefix_and_lifecycle_until_explicit_presentation_failure() {
    let first = Progress {
        song_ns: -10,
        hits: 5,
        misses: 1,
        combo: 3,
        max_combo: 4,
    };
    let mut hud = SavedOpponentHud::default();
    assert!(hud.update_peer(0, &peer_words(first)).is_err());
    assert!(hud.snapshot().is_none());
    hud.update_peer(1, &peer_words(first)).unwrap();
    let retained = hud.snapshot().unwrap().clone();
    for candidate in [
        Progress {
            song_ns: -11,
            ..first
        },
        Progress { hits: 4, ..first },
        Progress { misses: 0, ..first },
        Progress {
            max_combo: 3,
            ..first
        },
        Progress { combo: 4, ..first },
        Progress {
            hits: u64::MAX,
            ..first
        },
        Progress { combo: 5, ..first },
    ] {
        assert!(validate_progress(Some(first), candidate).is_err());
        assert!(hud.update_peer(2, &peer_words(candidate)).is_err());
        assert_eq!(hud.snapshot(), Some(&retained));
        assert!(!hud.peer_failed());
    }
    for (status, words) in [(4, vec![]), (1, vec![0; 9]), (1, vec![0; 11]), (0, vec![])] {
        assert!(hud.update_peer(status, &words).is_err());
        assert_eq!(hud.snapshot(), Some(&retained));
    }
    let next = Progress {
        song_ns: 10,
        hits: 7,
        misses: 2,
        combo: 1,
        max_combo: 4,
    };
    validate_progress(Some(first), next).unwrap();
    hud.update_peer(1, &peer_words(next)).unwrap();
    hud.update_peer(2, &[]).unwrap();
    let closed = hud.snapshot().unwrap().clone();
    assert!(hud.update_peer(1, &peer_words(next)).is_err());
    assert_eq!(hud.snapshot(), Some(&closed));
    hud.update_peer(3, &[]).unwrap();
    let stopped = hud.snapshot().unwrap().clone();
    assert!(hud.update_peer(2, &[]).is_err());
    assert_eq!(hud.snapshot(), Some(&stopped));
    hud.mark_peer_failed();
    assert!(hud.peer_failed());
    assert!(!hud.failed());
    assert!(hud.snapshot().is_none());
    assert!(hud.update_peer(3, &[]).is_err());
}

#[test]
fn saved_prefix_and_peer_hud_share_the_common_view_but_neither_failure_can_erase_the_other() {
    let (source, header, bytes) = recording(false);
    let mut opponents = saved(header);
    opponents
        .add(&source, &bytes, OpponentKind::Own, "own prefix")
        .unwrap();
    opponents.advance_to(ts(HIT)).unwrap();
    let peer = Progress {
        song_ns: -1,
        hits: 9007199254740993,
        misses: 7,
        combo: 1,
        max_combo: 42,
    };
    let mut hud = SavedOpponentHud::default();
    hud.update(&opponents).unwrap();
    hud.update_peer(1, &peer_words(peer)).unwrap();
    let combined = hud.snapshot().unwrap().clone();
    assert_eq!(combined.ghosts[0].hits, 1);
    assert_eq!(combined.network.as_ref().unwrap().progress, Some(peer));
    #[cfg(feature = "graphics")]
    {
        let mut scene = Scene::new(960, 720);
        competition_scoreboard(
            &mut scene,
            &ScoreSummary::default(),
            hud.snapshot().unwrap(),
        )
        .unwrap();
        assert!(!scene.rectangles().is_empty());
        assert_eq!(hud.snapshot(), Some(&combined));
    }
    opponents.reset();
    hud.update(&opponents).unwrap();
    assert_eq!(hud.snapshot().unwrap().network, combined.network);
    hud.mark_failed();
    assert!(hud.failed());
    assert!(!hud.peer_failed());
    assert!(hud.snapshot().unwrap().ghosts.is_empty());
    assert_eq!(hud.snapshot().unwrap().network, combined.network);
    assert!(hud.update(&opponents).is_err());
    hud.update_peer(2, &[]).unwrap();
    assert_eq!(
        hud.snapshot().unwrap().network.as_ref().unwrap().progress,
        Some(peer)
    );
    let mut peer_failed = SavedOpponentHud::default();
    opponents.advance_to(ts(HIT)).unwrap();
    peer_failed.update(&opponents).unwrap();
    peer_failed.update_peer(1, &peer_words(peer)).unwrap();
    peer_failed.mark_peer_failed();
    assert!(peer_failed.peer_failed());
    assert!(!peer_failed.failed());
    assert!(peer_failed.snapshot().unwrap().network.is_none());
    assert_eq!(peer_failed.snapshot().unwrap().ghosts, combined.ghosts);
    assert!(peer_failed.update_peer(3, &[]).is_err());
    opponents.reset();
    peer_failed.update(&opponents).unwrap();
    assert_eq!(peer_failed.snapshot().unwrap().ghosts[0].hits, 0);
    peer_failed.mark_failed();
    assert!(peer_failed.snapshot().is_none());
    assert_eq!(opponents.encoded_bytes(), bytes.len());
    assert_eq!(opponents.song_time(), None);
}

#[test]
fn actual_recorded_prefixes_retain_identity_and_update_only_when_the_game_owner_refreshes_the_cache()
 {
    let (source, header, prefix) = recording(false);
    let (_, _, complete) = recording(true);
    let mut expected = header.clone();
    expected.normalized_clock = ClockDomainId(999);
    let mut owner = saved(expected.clone());
    let mut hud = SavedOpponentHud::default();
    hud.update(&owner).unwrap();
    assert!(hud.snapshot().is_none());
    assert!(!hud.failed());
    owner
        .add(&source, &prefix, OpponentKind::Own, "以前の自分")
        .unwrap();
    owner
        .add(&source, &complete, OpponentKind::Other, " Other <record> ")
        .unwrap();
    owner.advance_to(ts(HIT - 1)).unwrap();
    hud.update(&owner).unwrap();
    let initial = hud.snapshot().unwrap().clone();
    assert!(initial.network.is_none());
    assert_eq!(initial.ghosts.len(), 2);
    assert_eq!(initial.ghosts[0].kind, OpponentKind::Own);
    assert_eq!(initial.ghosts[0].label, "以前の自分");
    assert_eq!(initial.ghosts[1].kind, OpponentKind::Other);
    assert_eq!(initial.ghosts[1].label, " Other <record> ");
    assert_eq!(initial.ghosts[0].recorded_until, Some(ts(HIT)));
    assert_eq!(initial.ghosts[1].recorded_until, Some(ts(3_000_000_000)));
    assert!(
        initial
            .ghosts
            .iter()
            .all(|row| row.hits == 0 && row.misses == 0)
    );
    owner.advance_to(ts(HIT)).unwrap();
    assert_eq!(
        hud.snapshot(),
        Some(&initial),
        "readers cannot pull the replay past the retained display prefix"
    );
    hud.update(&owner).unwrap();
    let hits = hud.snapshot().unwrap().clone();
    assert!(
        hits.ghosts
            .iter()
            .all(|row| row.hits == 1 && row.misses == 0 && row.combo == 1 && row.max_combo == 1)
    );
    owner.advance_to(ts(3_000_000_000)).unwrap();
    hud.update(&owner).unwrap();
    let final_rows = &hud.snapshot().unwrap().ghosts;
    assert_eq!((final_rows[0].hits, final_rows[0].misses), (1, 0));
    assert_eq!((final_rows[1].hits, final_rows[1].misses), (1, 1));
    assert_eq!(owner.expected_header(), &expected);
    assert_eq!(owner.encoded_bytes(), prefix.len() + complete.len());
    assert_eq!(
        hits.ghosts[1].misses, 0,
        "prior snapshots remain independent owned values"
    );
}

#[test]
fn repeated_common_scoreboard_reads_never_fill_an_unrecorded_tail_or_advance_a_saved_owner() {
    let (source, header, prefix) = recording(false);
    let empty = encode_replay(&ReplayFile::new(header.clone(), Vec::new()), limits()).unwrap();
    let mut owner = saved(header);
    owner
        .add(&source, &empty, OpponentKind::Own, "empty")
        .unwrap();
    owner
        .add(&source, &prefix, OpponentKind::Other, &"é".repeat(128))
        .unwrap();
    owner.advance_to(ts(i64::MAX)).unwrap();
    let mut hud = SavedOpponentHud::default();
    hud.update(&owner).unwrap();
    let expected = hud.snapshot().unwrap().clone();
    assert_eq!(expected.ghosts[0].recorded_until, None);
    assert_eq!((expected.ghosts[0].hits, expected.ghosts[0].misses), (0, 0));
    assert_eq!((expected.ghosts[1].hits, expected.ghosts[1].misses), (1, 0));
    assert_eq!(expected.ghosts[1].label.as_bytes().len(), 256);
    #[cfg(feature = "graphics")]
    {
        let score = ScoreSummary::default();
        let mut scene = Scene::new(960, 720);
        competition_scoreboard(&mut scene, &score, hud.snapshot().unwrap()).unwrap();
        let geometry = |scene: &Scene| {
            scene
                .rectangles()
                .iter()
                .map(|r| (r.bounds, r.color, r.uv))
                .collect::<Vec<_>>()
        };
        let rectangles = geometry(&scene);
        assert!(!rectangles.is_empty());
        for _ in 0..4 {
            scene.clear();
            competition_scoreboard(&mut scene, &score, hud.snapshot().unwrap()).unwrap();
            assert_eq!(geometry(&scene), rectangles);
        }
    }
    for _ in 0..4 {
        assert_eq!(hud.snapshot(), Some(&expected));
        assert_eq!(owner.song_time(), Some(ts(i64::MAX)));
        assert_eq!(owner.opponents()[1].recorded_until(), Some(ts(HIT)));
        assert_eq!(owner.opponents()[1].score().misses, 0);
    }
    assert_eq!(owner.encoded_bytes(), empty.len() + prefix.len());
}

#[test]
fn resetting_real_recordings_refreshes_the_prefix_but_terminal_hud_failure_cannot_be_resurrected() {
    let (source, header, bytes) = recording(false);
    let mut owner = saved(header.clone());
    owner
        .add(&source, &bytes, OpponentKind::Other, "retained")
        .unwrap();
    owner.advance_to(ts(HIT)).unwrap();
    let mut hud = SavedOpponentHud::default();
    hud.update(&owner).unwrap();
    assert_eq!(hud.snapshot().unwrap().ghosts[0].hits, 1);
    owner.reset();
    hud.update(&owner).unwrap();
    assert_eq!(hud.snapshot().unwrap().ghosts[0].hits, 0);
    assert_eq!(
        hud.snapshot().unwrap().ghosts[0].recorded_until,
        Some(ts(HIT))
    );
    assert_eq!(owner.encoded_bytes(), bytes.len());
    assert_eq!(owner.expected_header(), &header);
    hud.mark_failed();
    assert!(hud.failed());
    assert!(hud.snapshot().is_none());
    owner.advance_to(ts(HIT)).unwrap();
    assert!(hud.update(&owner).is_err());
    assert!(hud.snapshot().is_none());
    hud.mark_failed();
    owner.reset();
    assert!(hud.update(&owner).is_err());
    let mut replacement = SavedOpponentHud::default();
    replacement.update(&owner).unwrap();
    assert_eq!(replacement.snapshot().unwrap().ghosts[0].hits, 0);
    assert!(!replacement.failed());
    let mut initially_failed = SavedOpponentHud::default();
    initially_failed.mark_failed();
    assert!(initially_failed.update(&saved(header)).is_err());
    assert!(initially_failed.snapshot().is_none());
}

#[test]
fn independent_member_huds_keep_exact_peer_frontiers_and_failure_latches_separate() {
    use crate::local_players::PlayerId;
    let (source, header, bytes) = recording(false);
    let mut opponents = saved(header);
    opponents
        .add(&source, &bytes, OpponentKind::Own, "actual saved prefix")
        .unwrap();
    opponents.advance_to(ts(HIT)).unwrap();
    let players = [PlayerId(7), PlayerId(u32::MAX), PlayerId(91), PlayerId(15)];
    let mut members: Vec<_> = players
        .into_iter()
        .map(|player| (player, SavedOpponentHud::default()))
        .collect();
    for (_, hud) in &mut members {
        hud.update(&opponents).unwrap();
        hud.update_peer(0, &[]).unwrap();
    }
    let peers = [
        Progress {
            song_ns: i64::MIN,
            hits: u64::MAX,
            misses: 0,
            combo: u64::MAX,
            max_combo: u64::MAX,
        },
        Progress {
            song_ns: -1,
            hits: 0,
            misses: u64::MAX,
            combo: 0,
            max_combo: 0,
        },
        Progress {
            song_ns: 604_800_000_000_001,
            hits: 9_007_199_254_740_993,
            misses: 0,
            combo: 7,
            max_combo: 9_007_199_254_740_993,
        },
    ];
    // Values belong to separate common owners; the browser's explicit admission
    // and PlayerId lookup remain a distinct, unexecuted WASM boundary.
    for (index, peer) in peers.into_iter().enumerate() {
        validate_progress(None, peer).unwrap();
        members[index].1.update_peer(1, &peer_words(peer)).unwrap();
    }
    let before: Vec<_> = members
        .iter()
        .map(|(_, hud)| hud.snapshot().unwrap().clone())
        .collect();
    assert_eq!(
        members
            .iter()
            .map(|(player, _)| *player)
            .collect::<Vec<_>>(),
        players
    );
    for index in 0..3 {
        assert_eq!(
            before[index].network.as_ref().unwrap().progress,
            Some(peers[index])
        );
    }
    assert_eq!(
        before[3].network.as_ref().unwrap().status,
        NetworkStatus::Waiting
    );
    assert!(before[3].network.as_ref().unwrap().progress.is_none());
    members[0].1.mark_peer_failed();
    members[1].1.mark_failed();
    assert!(members[0].1.snapshot().unwrap().network.is_none());
    assert_eq!(members[0].1.snapshot().unwrap().ghosts, before[0].ghosts);
    assert!(members[1].1.snapshot().unwrap().ghosts.is_empty());
    assert_eq!(members[1].1.snapshot().unwrap().network, before[1].network);
    assert_eq!(members[2].1.snapshot(), Some(&before[2]));
    assert_eq!(members[3].1.snapshot(), Some(&before[3]));
    assert!(members[0].1.update_peer(3, &[]).is_err());
    assert!(members[1].1.update(&opponents).is_err());
    let regressed = Progress {
        song_ns: peers[2].song_ns - 1,
        ..peers[2]
    };
    assert!(members[2].1.update_peer(2, &peer_words(regressed)).is_err());
    assert_eq!(members[2].1.snapshot(), Some(&before[2]));
    members[1].1.update_peer(2, &[]).unwrap();
    members[2].1.update_peer(3, &[]).unwrap();
    assert_eq!(
        members[1]
            .1
            .snapshot()
            .unwrap()
            .network
            .as_ref()
            .unwrap()
            .progress,
        Some(peers[1])
    );
    assert_eq!(
        members[2]
            .1
            .snapshot()
            .unwrap()
            .network
            .as_ref()
            .unwrap()
            .progress,
        Some(peers[2])
    );
    assert!(members[2].1.update_peer(1, &peer_words(peers[2])).is_err());
    let late = Progress {
        song_ns: i64::MAX,
        hits: 1,
        misses: 0,
        combo: 1,
        max_combo: 1,
    };
    members[3].1.update_peer(1, &peer_words(late)).unwrap();
    opponents.advance_to(ts(5_000_000_000)).unwrap();
    for index in [0, 2, 3] {
        members[index].1.update(&opponents).unwrap();
        assert_eq!(
            (
                members[index].1.snapshot().unwrap().ghosts[0].hits,
                members[index].1.snapshot().unwrap().ghosts[0].misses
            ),
            (1, 0)
        );
    }
    assert_eq!(
        members[3]
            .1
            .snapshot()
            .unwrap()
            .network
            .as_ref()
            .unwrap()
            .progress,
        Some(late)
    );
    assert_eq!(opponents.encoded_bytes(), bytes.len());
    assert_eq!(opponents.opponents()[0].recorded_until(), Some(ts(HIT)));
}
