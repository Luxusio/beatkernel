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
