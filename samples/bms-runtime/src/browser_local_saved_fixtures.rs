//! Deferred native composition of the genuine local member and saved-prefix owners.
//! Browser-only serialization and aggregate admission remain separate WASM boundaries.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    competition::OpponentKind,
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    saved_opponent_hud::SavedOpponentHud,
    saved_opponents::SavedOpponents,
    step_gameplay::{StepGameplayConfig, StepLocalGameplay},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits},
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    replay::codec::{ReplayCodecLimits, decode_replay},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::BmsInputMode;

const PLAYERS: [PlayerId; 3] = [PlayerId(7), PlayerId(u32::MAX), PlayerId(91)];
const SOURCES: [DeviceId; 3] = [DeviceId(0), DeviceId(u64::MAX), DeviceId(3)];
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
struct Exact;
impl ClockMapper for Exact {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn prepared() -> PreparedBms {
    let chart = b"#BPM 60\n#WAV01 key.wav\n#00011:01010000\n";
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&40_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    for value in [1_u16, 1] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [8_u32, 16] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [2_u16, 16] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4_u32.to_le_bytes());
    for value in [8192_i16, -4096] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files.insert("pack/chart.bms", chart.to_vec()).unwrap();
    files.insert("pack/key.wav", wav).unwrap();
    prepare_from_source(
        chart,
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(8, 1).unwrap(),
        PcmLimits::new(256, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        u64::MAX,
        None,
    )
    .unwrap()
}
fn owner(offset: i64) -> StepLocalGameplay {
    owner_with_members(offset, &PLAYERS, &SOURCES)
}
fn owner_with_members(
    offset: i64,
    players: &[PlayerId],
    sources: &[DeviceId],
) -> StepLocalGameplay {
    let config = StepGameplayConfig {
        host_origin: point(11, 10_000_000_000),
        output_origin: point(22, 0),
        preroll: Duration::from_nanos(100_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: offset,
        command_capacity: 32,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 8,
    };
    let plan = ResolvedInputPlan::new(
        players
            .iter()
            .copied()
            .zip(sources.iter().copied().map(Some))
            .collect(),
    )
    .unwrap();
    let bindings = sources
        .iter()
        .map(|source| {
            BindingMap::from_bindings([Binding {
                device: DeviceSelector::Exact(*source),
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(0x11),
            }])
            .unwrap()
        })
        .collect();
    let (game, bank) = StepLocalGameplay::new_section(
        prepared(),
        config,
        plan,
        bindings,
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    assert_eq!(bank.len(), 1);
    game
}
fn input(
    game: &mut StepLocalGameplay,
    member: usize,
    song: i64,
    sequence: u64,
    state: ButtonState,
) {
    let original = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(SOURCES[member], point(11, 10_100_000_000 + song), sequence),
        control: PhysicalControlId::keyboard(4),
        state,
    });
    let result = game
        .process_input(original.clone(), &Exact, point(22, 100_000_000 + song))
        .unwrap();
    let InputResult::Processed(rows) = result else {
        panic!("assigned source was ignored")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].player, PLAYERS[member]);
    assert_eq!(rows[0].report.input.as_ref(), Some(&original));
}
fn recorded_members() -> [Vec<u8>; 2] {
    let mut game = owner(0);
    for player in PLAYERS {
        game.configure_capture(player, limits(), u64::MAX).unwrap();
    }
    game.activate(point(11, 10_000_000_000)).unwrap();
    input(&mut game, 0, 0, 1, ButtonState::Down);
    input(&mut game, 1, 0, 1, ButtonState::Down);
    input(&mut game, 1, 1, 2, ButtonState::Up);
    input(&mut game, 1, 1_000_000_000, 3, ButtonState::Down);
    assert_eq!(game.score(PLAYERS[0]).unwrap().hits, 1);
    assert_eq!(game.score(PLAYERS[1]).unwrap().hits, 2);
    game.fail();
    [
        game.take_replay(PLAYERS[0]).unwrap().unwrap(),
        game.take_replay(PLAYERS[1]).unwrap().unwrap(),
    ]
}

#[test]
fn whole_cohort_progress_observes_independent_committed_frontiers_even_after_failure() {
    use crate::{
        multiplayer_group::{
            MemberProgress, decode_prefix, encode_prefix, encode_words, validate_members,
        },
        multiplayer_protocol::Progress,
    };
    let mut observed = owner(0);
    let mut baseline = owner(0);
    let initial = observed.group_progress().unwrap();
    assert_eq!(
        initial.iter().map(|row| row.player).collect::<Vec<_>>(),
        PLAYERS
    );
    assert!(initial.iter().all(|row| row.progress
        == Progress {
            song_ns: -100_000_000,
            hits: 0,
            misses: 0,
            combo: 0,
            max_combo: 0,
        }));
    assert!(observed.input_setup_available());
    for game in [&mut observed, &mut baseline] {
        for player in PLAYERS {
            game.configure_capture(player, limits(), u64::MAX).unwrap();
        }
        game.activate(point(11, 10_000_000_000)).unwrap();
        input(game, 0, 0, 1, ButtonState::Down);
        input(game, 1, 0, 1, ButtonState::Down);
        input(game, 1, 1, 2, ButtonState::Up);
        input(game, 1, 1_000_000_000, 3, ButtonState::Down);
    }
    let expected = [
        MemberProgress {
            player: PLAYERS[0],
            progress: Progress {
                song_ns: 0,
                hits: 1,
                misses: 0,
                combo: 1,
                max_combo: 1,
            },
        },
        MemberProgress {
            player: PLAYERS[1],
            progress: Progress {
                song_ns: 1_000_000_000,
                hits: 2,
                misses: 0,
                combo: 2,
                max_combo: 2,
            },
        },
        MemberProgress {
            player: PLAYERS[2],
            progress: Progress {
                song_ns: -100_000_000,
                hits: 0,
                misses: 0,
                combo: 0,
                max_combo: 0,
            },
        },
    ];
    let hashes: Vec<_> = PLAYERS
        .iter()
        .map(|player| observed.judge(*player).unwrap().stable_hash().unwrap())
        .collect();
    let shared_song = observed.song_time();
    let prefix = observed.group_progress().unwrap();
    assert_eq!(prefix, expected);
    validate_members(Some(&initial), &prefix).unwrap();
    let words = encode_words(&prefix).unwrap();
    assert_eq!(words.len(), 33);
    assert_eq!(
        words.chunks_exact(11).map(|row| row[0]).collect::<Vec<_>>(),
        [7, u32::MAX, 91]
    );
    let mut detached = observed.group_progress().unwrap();
    detached[0].player = PlayerId(999);
    detached[0].progress.hits = 999;
    assert_eq!(observed.group_progress().unwrap(), expected);
    assert_eq!(observed.song_time(), shared_song);

    // A genuine member chronology refusal fences the cohort after distinct
    // committed prefixes. Reading cleanup state must not advance idle members.
    for game in [&mut observed, &mut baseline] {
        let backwards = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(SOURCES[1], point(11, 10_100_000_000), 4),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Up,
        });
        assert!(
            game.process_input(backwards, &Exact, point(22, 1_100_000_000))
                .is_err()
        );
        assert!(game.failed());
    }
    let retained = observed.group_progress().unwrap();
    assert_eq!(retained, expected);
    let encoded = encode_prefix(u64::MAX, true, &retained).unwrap();
    let decoded = decode_prefix(&encoded, u64::MAX, Some(&prefix)).unwrap();
    assert!(decoded.final_prefix);
    assert_eq!(decoded.members, expected);
    assert_eq!(observed.group_progress().unwrap(), expected);
    assert_eq!(
        PLAYERS
            .iter()
            .map(|player| observed.judge(*player).unwrap().stable_hash().unwrap())
            .collect::<Vec<_>>(),
        hashes
    );
    for (index, player) in PLAYERS.into_iter().enumerate() {
        let actual = observed.take_replay(player).unwrap().unwrap();
        let untouched = baseline.take_replay(player).unwrap().unwrap();
        assert_eq!(
            actual, untouched,
            "snapshot reads must not change a member's capture"
        );
        let file = decode_replay(&actual, limits()).unwrap();
        assert_eq!(
            file.records.last().map(|record| record.song_time),
            [Some(Timestamp::ZERO), Some(ts(1_000_000_000)), None][index]
        );
        assert!(observed.take_replay(player).unwrap().is_none());
    }
}

#[test]
fn four_member_peer_and_saved_geometry_does_not_change_actual_cohort_capture_or_judgment() {
    use crate::{
        browser_input::TouchInputSetup,
        multiplayer_protocol::Progress,
        player::NetworkStatus,
        playfield_layout::{
            local_field_bounds_with_comparison_space, local_touch_bounds_with_comparison_space,
        },
    };
    use beatkernel::input::{BackendId, ContactId, Position2, TouchEvent, TouchPhase, TouchRoute};
    let players = [PLAYERS[0], PLAYERS[1], PLAYERS[2], PlayerId(15)];
    let sources = [SOURCES[0], SOURCES[1], SOURCES[2], DeviceId(4)];
    let recordings = recorded_members();
    let source = prepared().source;
    let mut live = owner_with_members(0, &players, &sources);
    let mut untouched = owner_with_members(0, &players, &sources);
    assert_eq!(live.players(), players);
    assert_eq!(untouched.players(), players);
    let header = live
        .competition_header(players[0], limits(), u64::MAX)
        .unwrap();
    let mut saved = SavedOpponents::new(header, limits(), 8, 64 << 20).unwrap();
    // All eight saved records belong to one member in this actual four-member
    // cohort, which shares one transport/output. The second cohort is the
    // identical-input baseline, not an alternative per-player game owner.
    for index in 0..8 {
        saved
            .add(
                &source,
                &recordings[index % 2],
                if index % 2 == 0 {
                    OpponentKind::Own
                } else {
                    OpponentKind::Other
                },
                &format!("saved {index}"),
            )
            .unwrap();
    }
    for owner in [&mut live, &mut untouched] {
        for player in players {
            owner.configure_capture(player, limits(), u64::MAX).unwrap();
        }
        owner.activate(point(11, 10_000_000_000)).unwrap();
        input(owner, 0, 0, 1, ButtonState::Down);
        input(owner, 1, 0, 1, ButtonState::Down);
    }
    saved
        .advance_to(live.member_song_time(players[0]).unwrap())
        .unwrap();
    let hashes: Vec<_> = players
        .iter()
        .map(|player| live.judge(*player).unwrap().stable_hash().unwrap())
        .collect();
    let scores: Vec<_> = players
        .iter()
        .map(|player| live.score(*player).unwrap().clone())
        .collect();
    let songs: Vec<_> = players
        .iter()
        .map(|player| live.member_song_time(*player))
        .collect();
    let mut hud: Vec<_> = players
        .iter()
        .map(|_| SavedOpponentHud::default())
        .collect();
    hud[0].update(&saved).unwrap();
    for member in [0, 1, 2] {
        hud[member].update_peer(0, &[]).unwrap();
    }
    let words = [
        0,
        0x8000_0000,
        u32::MAX,
        u32::MAX,
        0,
        0,
        u32::MAX,
        u32::MAX,
        u32::MAX,
        u32::MAX,
    ];
    let reported = Progress {
        song_ns: i64::MIN,
        hits: u64::MAX,
        misses: 0,
        combo: u64::MAX,
        max_combo: u64::MAX,
    };
    hud[0].update_peer(1, &words).unwrap();
    let combined = hud[0].snapshot().unwrap().clone();
    assert_eq!(combined.ghosts.len(), 8);
    assert_eq!(combined.network.as_ref().unwrap().progress, Some(reported));
    assert_eq!(
        hud[1].snapshot().unwrap().network.as_ref().unwrap().status,
        NetworkStatus::Waiting
    );
    assert!(hud[3].snapshot().is_none());
    let sibling = hud[1].snapshot().unwrap().clone();
    assert!(hud[0].update_peer(2, &words[..9]).is_err());
    assert_eq!(hud[0].snapshot(), Some(&combined));
    assert_eq!(hud[1].snapshot(), Some(&sibling));
    // Independent failure histories consume the same genuine saved prefix;
    // no fixture substitutes for BrowserLocalGame's admission or touch lock.
    let mut peer_disabled = SavedOpponentHud::default();
    peer_disabled.update(&saved).unwrap();
    peer_disabled.update_peer(1, &words).unwrap();
    peer_disabled.mark_peer_failed();
    hud[0].mark_failed();
    assert_eq!(peer_disabled.snapshot().unwrap().ghosts, combined.ghosts);
    assert!(peer_disabled.snapshot().unwrap().network.is_none());
    assert!(hud[0].snapshot().unwrap().ghosts.is_empty());
    assert_eq!(hud[0].snapshot().unwrap().network, combined.network);
    assert_eq!(hud[1].snapshot(), Some(&sibling));

    let reserved = [140, 28, 28, 0];
    for (slot, expected) in [
        [34, 312, 430, 44],
        [496, 312, 430, 44],
        [34, 588, 430, 44],
        [496, 588, 430, 44],
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            local_field_bounds_with_comparison_space(4, slot, 140).unwrap(),
            expected
        );
        let touch = local_touch_bounds_with_comparison_space(&[0x11], 4, slot, 140).unwrap();
        assert_eq!(
            touch,
            vec![
                expected[0] as f32,
                (expected[1] + 4) as f32,
                (expected[0] + expected[2]) as f32,
                (expected[1] + expected[3]) as f32
            ]
        );
    }
    assert!(local_field_bounds_with_comparison_space(4, 0, 141).is_err());
    let touch = local_touch_bounds_with_comparison_space(&[0x11], 4, 0, reserved[0]).unwrap();
    let mut setup =
        TouchInputSetup::new(&[0x11, 1, 0, 0, 1, 0x5754_4f55, 0], &touch, &[0x11], 256).unwrap();
    let original = PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(0), point(11, 10_100_000_000), u64::MAX),
        control: PhysicalControlId::Native {
            backend: BackendId(0x5754_4f55),
            code: 0,
        },
        contact: ContactId(u64::MAX),
        phase: TouchPhase::Down,
        position: Position2 { x: 1.25, y: 2.5 },
        pressure: Some(0.5),
    });
    let routed = setup
        .router
        .route_at(&original, Position2 { x: 35.0, y: 317.0 })
        .unwrap();
    assert_eq!(
        routed,
        TouchRoute::Bound(beatkernel::input::GameInputEvent {
            game_control: GameControlId(0x11),
            physical: original
        })
    );

    #[cfg(feature = "graphics")]
    {
        use crate::{
            bga_render::BgaFrame,
            player_chart::PlayerChart,
            scene::Scene,
            ui::organisms::{LocalPlayerView, local_player_views_with_reserved_comparison_space},
        };
        let prepared = prepared();
        let chart = PlayerChart::from_compiled(&prepared.source, &prepared.compiled.chart).unwrap();
        let compose = |first| {
            let views: Vec<_> = players
                .iter()
                .enumerate()
                .map(|(index, player)| LocalPlayerView {
                    player: *player,
                    chart: Some(&chart),
                    song_time: live.member_song_time(*player),
                    score: live.score(*player).unwrap(),
                    last_judge: None,
                    recent_results: &[],
                    pressed_lanes: 0,
                    note_progress: None,
                    competition: if index == 0 {
                        first
                    } else {
                        hud[index].snapshot()
                    },
                })
                .collect();
            for (index, view) in views.iter().enumerate() {
                assert!(std::ptr::eq(
                    view.score,
                    live.score(players[index]).unwrap()
                ));
            }
            let mut scene = Scene::new(960, 720);
            local_player_views_with_reserved_comparison_space(
                &mut scene,
                &views,
                2_000_000_000,
                0,
                true,
                &[BgaFrame::default(); 4],
                &reserved,
            )
            .unwrap();
            scene
        };
        let before = compose(Some(&combined));
        let no_peer = compose(peer_disabled.snapshot());
        let no_saved = compose(hud[0].snapshot());
        let band = |scene: &Scene, top: f32, height: f32| {
            scene
                .rectangles()
                .iter()
                .filter(|rectangle| {
                    let [x, y, width, h] = rectangle.bounds;
                    x >= 34.0 && x + width <= 464.0 && y >= top && y + h <= top + height
                })
                .map(|rectangle| {
                    (
                        rectangle.bounds.map(f32::to_bits),
                        rectangle.color.map(f32::to_bits),
                        rectangle.uv.map(f32::to_bits),
                    )
                })
                .collect::<Vec<_>>()
        };
        let saved_rows = band(&before, 172.0, 112.0);
        let peer_rows = band(&before, 284.0, 28.0);
        assert!(!saved_rows.is_empty());
        assert!(!peer_rows.is_empty());
        assert_eq!(band(&no_peer, 172.0, 112.0), saved_rows);
        assert_eq!(
            band(&no_saved, 284.0, 28.0),
            peer_rows,
            "saved failure cannot move the healthy peer into the failed saved reservation"
        );
        for scene in [&before, &no_peer, &no_saved] {
            assert_eq!(scene.playfields().len(), 4);
            for (slot, frame) in scene.playfields().iter().enumerate() {
                let area =
                    local_touch_bounds_with_comparison_space(&[0x11], 4, slot, reserved[slot])
                        .unwrap();
                assert_eq!(frame.top, area[1]);
                assert!(frame.bottom <= area[3]);
                assert!(frame.bottom > frame.top);
            }
            assert_eq!(scene.playfields()[0].top, touch[1]);
        }
    }
    assert_eq!(
        players
            .iter()
            .map(|player| live.judge(*player).unwrap().stable_hash().unwrap())
            .collect::<Vec<_>>(),
        hashes
    );
    assert_eq!(
        players
            .iter()
            .map(|player| live.score(*player).unwrap().clone())
            .collect::<Vec<_>>(),
        scores
    );
    assert_eq!(
        players
            .iter()
            .map(|player| live.member_song_time(*player))
            .collect::<Vec<_>>(),
        songs
    );
    for owner in [&mut live, &mut untouched] {
        input(owner, 0, 1, 2, ButtonState::Up);
        input(owner, 0, 1_000_000_000, 3, ButtonState::Down);
        owner.fail();
    }
    for player in players {
        let observed = live.take_replay(player).unwrap().unwrap();
        let baseline = untouched.take_replay(player).unwrap().unwrap();
        assert_eq!(
            observed, baseline,
            "passive comparison state must not change any member's actual capture bytes"
        );
    }
}

#[test]
fn saved_local_rows_follow_their_actual_member_frontiers_and_never_extend_unrecorded_tails() {
    let recordings = recorded_members();
    let source = prepared().source;
    let mut live = owner(0);
    let mut groups: Vec<_> = PLAYERS
        .iter()
        .map(|player| {
            SavedOpponents::new(
                live.competition_header(*player, limits(), u64::MAX)
                    .unwrap(),
                limits(),
                8,
                64 << 20,
            )
            .unwrap()
        })
        .collect();
    for (index, group) in groups.iter_mut().enumerate().take(2) {
        assert_eq!(
            group
                .add(&source, &recordings[0], OpponentKind::Own, "one actual hit")
                .unwrap(),
            0
        );
        assert_eq!(
            group
                .add(
                    &source,
                    &recordings[1],
                    OpponentKind::Other,
                    "two actual hits"
                )
                .unwrap(),
            1
        );
        assert_eq!(
            group.encoded_bytes(),
            recordings[0].len() + recordings[1].len()
        );
        assert_eq!(
            group.expected_header(),
            &live
                .competition_header(PLAYERS[index], limits(), u64::MAX)
                .unwrap()
        );
    }
    let first_file = decode_replay(&recordings[0], limits()).unwrap();
    let second_file = decode_replay(&recordings[1], limits()).unwrap();
    assert_eq!(
        first_file.records.last().unwrap().song_time,
        Timestamp::ZERO
    );
    assert_eq!(
        second_file.records.last().unwrap().song_time,
        ts(1_000_000_000)
    );
    live.activate(point(11, 10_000_000_000)).unwrap();
    input(&mut live, 0, 0, 1, ButtonState::Down);
    let mut hud: Vec<_> = PLAYERS
        .iter()
        .map(|_| SavedOpponentHud::default())
        .collect();
    for index in 0..3 {
        groups[index]
            .advance_to(live.member_song_time(PLAYERS[index]).unwrap())
            .unwrap();
        hud[index].update(&groups[index]).unwrap();
    }
    assert_eq!(hud[0].snapshot().unwrap().ghosts[0].hits, 1);
    assert_eq!(hud[1].snapshot().unwrap().ghosts[0].hits, 0);
    assert!(hud[2].snapshot().is_none());
    assert_eq!(live.member_song_time(PLAYERS[1]), Some(ts(-100_000_000)));
    input(&mut live, 1, 0, 1, ButtonState::Down);
    input(&mut live, 1, 1, 2, ButtonState::Up);
    input(&mut live, 1, 1_000_000_000, 3, ButtonState::Down);
    groups[1]
        .advance_to(live.member_song_time(PLAYERS[1]).unwrap())
        .unwrap();
    hud[1].update(&groups[1]).unwrap();
    assert_eq!(
        hud[1]
            .snapshot()
            .unwrap()
            .ghosts
            .iter()
            .map(|row| row.hits)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(groups[0].song_time(), Some(Timestamp::ZERO));
    let hashes: Vec<_> = PLAYERS
        .iter()
        .map(|player| live.judge(*player).unwrap().stable_hash().unwrap())
        .collect();
    let live_scores: Vec<_> = PLAYERS
        .iter()
        .map(|player| live.score(*player).unwrap().clone())
        .collect();
    for _ in 0..4 {
        hud[1].update(&groups[1]).unwrap();
    }
    assert_eq!(
        hashes,
        PLAYERS
            .iter()
            .map(|player| live.judge(*player).unwrap().stable_hash().unwrap())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        live_scores,
        PLAYERS
            .iter()
            .map(|player| live.score(*player).unwrap().clone())
            .collect::<Vec<_>>()
    );
    live.advance_to(point(11, 15_100_000_000), &Exact, point(22, 5_100_000_000))
        .unwrap();
    groups[0]
        .advance_to(live.member_song_time(PLAYERS[0]).unwrap())
        .unwrap();
    hud[0].update(&groups[0]).unwrap();
    let saved = &hud[0].snapshot().unwrap().ghosts;
    assert_eq!(
        (saved[0].hits, saved[0].misses, saved[0].recorded_until),
        (1, 0, Some(Timestamp::ZERO))
    );
    assert_eq!((saved[1].hits, saved[1].misses), (2, 0));
    assert_eq!(live.score(PLAYERS[0]).unwrap().misses, 1);
    assert_eq!(live.score(PLAYERS[2]).unwrap().misses, 2);
}

#[test]
fn member_admission_and_presentation_failures_preserve_sibling_prefixes_and_actual_local_capture() {
    let recordings = recorded_members();
    let source = prepared().source;
    let mut live = owner(0);
    let header = live
        .competition_header(PLAYERS[0], limits(), u64::MAX)
        .unwrap();
    let mut first = SavedOpponents::new(header.clone(), limits(), 8, 64 << 20).unwrap();
    let mut second = SavedOpponents::new(
        live.competition_header(PLAYERS[1], limits(), u64::MAX)
            .unwrap(),
        limits(),
        8,
        64 << 20,
    )
    .unwrap();
    first
        .add(&source, &recordings[0], OpponentKind::Own, "member seven")
        .unwrap();
    second
        .add(&source, &recordings[1], OpponentKind::Other, "member max")
        .unwrap();
    let charged = second.encoded_bytes();
    assert!(
        second
            .add(&source, &[0, 1, 2], OpponentKind::Other, "malformed")
            .is_err()
    );
    assert_eq!(second.count(), 1);
    assert_eq!(second.encoded_bytes(), charged);
    let changed = owner(1);
    let mut incompatible = SavedOpponents::new(
        changed
            .competition_header(PLAYERS[1], limits(), u64::MAX)
            .unwrap(),
        limits(),
        8,
        64 << 20,
    )
    .unwrap();
    assert!(
        incompatible
            .add(
                &source,
                &recordings[0],
                OpponentKind::Own,
                "different actual offset"
            )
            .is_err()
    );
    assert_eq!(incompatible.count(), 0);
    assert_eq!(incompatible.encoded_bytes(), 0);
    for player in PLAYERS {
        live.configure_capture(player, limits(), u64::MAX).unwrap();
    }
    live.activate(point(11, 10_000_000_000)).unwrap();
    input(&mut live, 0, 0, 1, ButtonState::Down);
    input(&mut live, 1, 0, 1, ButtonState::Down);
    first
        .advance_to(live.member_song_time(PLAYERS[0]).unwrap())
        .unwrap();
    second
        .advance_to(live.member_song_time(PLAYERS[1]).unwrap())
        .unwrap();
    let mut first_hud = SavedOpponentHud::default();
    let mut second_hud = SavedOpponentHud::default();
    first_hud.update(&first).unwrap();
    second_hud.update(&second).unwrap();
    let healthy = first_hud.snapshot().unwrap().clone();
    let second_display = second_hud.snapshot().unwrap().clone();
    // The reservation is admitted-record state, not the changing HUD row count.
    let reserved = [first.count() as i64 * 14, second.count() as i64 * 14, 0];
    use crate::playfield_layout::{
        local_field_bounds_with_comparison_space, local_touch_bounds_with_comparison_space,
    };
    let field = local_field_bounds_with_comparison_space(3, 1, reserved[1]).unwrap();
    let touch = local_touch_bounds_with_comparison_space(&[0x11], 3, 1, reserved[1]).unwrap();
    assert_eq!(field, [496, 186, 430, 170]);
    assert_eq!(touch, vec![496.0, 190.0, 926.0, 356.0]);
    assert_eq!(
        local_field_bounds_with_comparison_space(4, 3, 112).unwrap(),
        [496, 560, 430, 72]
    );
    assert!(local_field_bounds_with_comparison_space(3, 1, -1).is_err());
    assert!(local_touch_bounds_with_comparison_space(&[0x11], 3, 1, 141).is_err());
    assert!(second.advance_to(ts(-1)).is_err());
    second_hud.mark_failed();
    assert!(second_hud.snapshot().is_none());
    assert!(second_hud.update(&second).is_err());
    assert_eq!(first_hud.snapshot(), Some(&healthy));
    assert!(!first_hud.failed());
    assert_eq!(
        local_field_bounds_with_comparison_space(3, 1, reserved[1]).unwrap(),
        field
    );
    assert_eq!(
        local_touch_bounds_with_comparison_space(&[0x11], 3, 1, reserved[1]).unwrap(),
        touch
    );
    #[cfg(feature = "graphics")]
    {
        use crate::{
            bga_render::BgaFrame,
            player_chart::PlayerChart,
            scene::Scene,
            ui::organisms::{LocalPlayerView, local_player_views_with_reserved_comparison_space},
        };
        let prepared = prepared();
        let chart = PlayerChart::from_compiled(&prepared.source, &prepared.compiled.chart).unwrap();
        let mut views: Vec<_> = PLAYERS
            .iter()
            .enumerate()
            .map(|(index, player)| LocalPlayerView {
                player: *player,
                chart: Some(&chart),
                song_time: live.member_song_time(*player),
                score: live.score(*player).unwrap(),
                last_judge: None,
                recent_results: &[],
                pressed_lanes: 0,
                note_progress: None,
                competition: match index {
                    0 => first_hud.snapshot(),
                    1 => Some(&second_display),
                    _ => None,
                },
            })
            .collect();
        let frames = [BgaFrame::default(); 4];
        let mut before = Scene::new(960, 720);
        local_player_views_with_reserved_comparison_space(
            &mut before,
            &views,
            2_000_000_000,
            0,
            true,
            &frames,
            &reserved,
        )
        .unwrap();
        views[1].competition = second_hud.snapshot();
        let mut after = Scene::new(960, 720);
        local_player_views_with_reserved_comparison_space(
            &mut after,
            &views,
            2_000_000_000,
            0,
            true,
            &frames,
            &reserved,
        )
        .unwrap();
        assert_eq!(before.playfields().len(), 3);
        assert_eq!(after.playfields().len(), 3);
        for (before, after) in before.playfields().iter().zip(after.playfields()) {
            assert_eq!((before.top, before.bottom), (after.top, after.bottom));
        }
        assert_eq!(after.playfields()[1].top, touch[1]);
        assert!(after.playfields()[1].bottom <= touch[3]);
        assert!(std::ptr::eq(
            views[0].score,
            live.score(PLAYERS[0]).unwrap()
        ));
    }
    #[cfg(not(feature = "graphics"))]
    assert_eq!(second_display.ghosts.len(), 1);
    assert!(!live.failed());
    assert_eq!(second.encoded_bytes(), charged);
    input(&mut live, 0, 1, 2, ButtonState::Up);
    input(&mut live, 0, 1_000_000_000, 3, ButtonState::Down);
    first
        .advance_to(live.member_song_time(PLAYERS[0]).unwrap())
        .unwrap();
    first_hud.update(&first).unwrap();
    assert_eq!(live.score(PLAYERS[0]).unwrap().hits, 2);
    assert_eq!(first_hud.snapshot().unwrap().ghosts[0].hits, 1);
    live.fail();
    let file = decode_replay(&live.take_replay(PLAYERS[0]).unwrap().unwrap(), limits()).unwrap();
    assert_eq!(file.header, header);
    assert_eq!(file.records.last().unwrap().song_time, ts(1_000_000_000));
    assert!(live.take_replay(PLAYERS[0]).unwrap().is_none());
    assert_eq!(
        decode_replay(&live.take_replay(PLAYERS[1]).unwrap().unwrap(), limits())
            .unwrap()
            .records
            .len(),
        1
    );
    assert!(
        decode_replay(&live.take_replay(PLAYERS[2]).unwrap().unwrap(), limits())
            .unwrap()
            .records
            .is_empty()
    );
}
