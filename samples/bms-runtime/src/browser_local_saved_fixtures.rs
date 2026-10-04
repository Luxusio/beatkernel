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
        PLAYERS
            .iter()
            .copied()
            .zip(SOURCES.iter().copied().map(Some))
            .collect(),
    )
    .unwrap();
    let bindings = SOURCES
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
    assert!(local_touch_bounds_with_comparison_space(&[0x11], 3, 1, 113).is_err());
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
