//! Deferred actual desktop acceptance/navigation/draw without a window or worker.
use super::*;
use beatkernel_bms_runtime::{
    gauge::BmsGauge,
    play_result::{CompletedPlayResult, PlayResultScope},
    step_gameplay::{StepGameplay, StepGameplayConfig},
};
use beatkernel::{
    audio::{AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue},
    input::BindingMap,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
struct Domains;
impl ClockMapper for Domains {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn completed(end: Option<i64>) -> (CompletedPlayResult, BmsGauge) {
    // The binary cannot use the core's private result constructor. Obtain proof
    // from the actual portable stepped owner and two real idle Mixer blocks.
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let prepared = beatkernel_bms_runtime::PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let config = StepGameplayConfig {
        host_origin: point(1, 0),
        output_origin: point(2, 0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 8,
    };
    let (mut owner, bank) = StepGameplay::new_section(
        prepared,
        config,
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        end.map(Timestamp::from_nanos),
    )
    .unwrap();
    assert!(owner.completed_result().is_none());
    owner.activate(point(1, 0)).unwrap();
    owner
        .advance_to(point(1, end.unwrap_or(1_000_000)), &Domains, point(2, 0))
        .unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mixer_config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
    );
    let mixer_config = match end {
        Some(end) => mixer_config
            .with_playback_end_frame((u64::try_from(end).unwrap() + 999_999) / 1_000_000),
        None => mixer_config,
    };
    let mut mixer = Mixer::new(mixer_config, bank, consumer).unwrap();
    let first = mixer.render(&mut [0.0; 10]).unwrap();
    let first_done = owner
        .observe_completion(Some(first), Some(point(2, 10_000_000)))
        .unwrap();
    if end.is_none() {
        assert!(!first_done);
    }
    let second = mixer.render(&mut [0.0; 10]).unwrap();
    assert!(
        owner
            .observe_completion(Some(second), Some(point(2, 20_000_000)))
            .unwrap()
    );
    (*owner.completed_result().unwrap(), owner.gauge().clone())
}
fn member(player: PlayerId, gauge: BmsGauge) -> player::LocalPlayerSnapshot {
    player::LocalPlayerSnapshot {
        player,
        chart: None,
        song_time: Some(Timestamp::ZERO),
        score: Default::default(),
        mine_damage: Default::default(),
        gauge,
        last_judge: None,
        recent_results: vec![],
        pressed_lanes: 0,
        note_progress: None,
        competition: None,
    }
}
fn snapshot(
    ids: &[PlayerId],
    result: CompletedPlayResult,
    gauge: &BmsGauge,
) -> player::PlayerSnapshot {
    player::PlayerSnapshot {
        players: ids.iter().map(|&id| member(id, gauge.clone())).collect(),
        completed_results: Some(ids.iter().map(|&id| (id, result)).collect()),
        status: player::PlayerStatus::Playing,
        ..Default::default()
    }
}
fn fixture_game() -> Game {
    let (_, viewer) = player::channel();
    Game {
        viewer,
        worker: None,
        snapshot: None,
        cancelling: false,
        joined: false,
        local_page: 0,
        local_comparisons: false,
        replay: false,
        launch: SessionLaunch::new(vec!["--chart".into(), "pinned.bms".into()]).unwrap(),
        prepared_retry: None,
        practice_bookmark: None,
        practice_loop: None,
        loop_enabled: false,
        completed_results: None,
        completed_results_error: None,
    }
}

#[test]
fn actual_game_keeps_completion_before_cleanup_but_results_navigation_waits_for_join() {
    let (result, gauge) = completed(None);
    let mut game = fixture_game();
    game.accept_snapshot(snapshot(&[PlayerId(u32::MAX)], result, &gauge));
    assert!(game.completed_results.is_some());
    assert!(!game.joined);
    // A deliberately invalid Results page makes invocation observable without
    // inspecting private renderer buffers: live drawing must ignore it.
    game.local_page = usize::MAX;
    let mut app = super::tests::lifecycle_fixture();
    app.game = Some(game);
    app.navigate(ScreenRoute::Play { replay: false }).unwrap();
    assert!(
        app.navigate(ScreenRoute::Results { replay: false })
            .is_err()
    );
    assert_eq!(app.navigator.route(), ScreenRoute::Play { replay: false });
    app.draw().unwrap();
    app.game.as_mut().unwrap().owner_finished(false);
    app.navigate(ScreenRoute::Results { replay: false })
        .unwrap();
    assert!(app.draw().unwrap_err().contains("out of range"));
    app.game.as_mut().unwrap().local_page = 0;
    app.draw().unwrap();
    let accepted = app
        .game
        .as_ref()
        .unwrap()
        .completed_results
        .as_ref()
        .unwrap();
    assert_eq!(accepted.rows()[0].player, PlayerId(u32::MAX));
    assert_eq!(accepted.rows()[0].result, result);
}

#[test]
fn actual_game_retains_first_table_and_gauge_roster_across_cleanup_failure_and_trailing_snapshots()
{
    let (full, gauge) = completed(None);
    let (practice, _) = completed(Some(2_000_000));
    let ids = [PlayerId(91), PlayerId(u32::MAX)];
    let mut game = fixture_game();
    game.accept_snapshot(snapshot(&ids, full, &gauge));
    game.owner_finished(false);
    let mut changed = snapshot(&[PlayerId(7)], practice, &gauge);
    changed.status = player::PlayerStatus::Failed("cleanup join failed".into());
    game.accept_snapshot(changed);
    assert_eq!(
        game.completed_results
            .as_ref()
            .unwrap()
            .rows()
            .iter()
            .map(|row| (row.player, row.result))
            .collect::<Vec<_>>(),
        [(ids[0], full), (ids[1], full)]
    );
    let current = game.snapshot.as_ref().unwrap();
    assert_eq!(
        current.completed_results,
        Some(vec![(ids[0], full), (ids[1], full)])
    );
    assert_eq!(
        current
            .players
            .iter()
            .map(|member| member.player)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(
        current.status,
        player::PlayerStatus::Failed("cleanup join failed".into())
    );
    game.accept_snapshot(player::PlayerSnapshot {
        status: player::PlayerStatus::Finished,
        ..Default::default()
    });
    assert_eq!(
        game.completed_results.as_ref().unwrap().rows()[0].result,
        full
    );
    assert_eq!(
        game.snapshot.as_ref().unwrap().completed_results,
        Some(vec![(ids[0], full), (ids[1], full)])
    );
    assert_eq!(
        practice.scope(),
        PlayResultScope::PracticeSection {
            start: Timestamp::ZERO,
            end: Some(Timestamp::from_nanos(2_000_000)),
        }
    );
    assert!(!practice.whole_song_clear());
}

#[test]
fn actual_game_rejects_later_bad_row_atomically_recovers_valid_table_and_never_accepts_replay_prefix_as_completion()
 {
    let (full, gauge) = completed(None);
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let mut game = fixture_game();
    let mut invalid = snapshot(&ids, full, &gauge);
    invalid.completed_results.as_mut().unwrap()[1].0 = PlayerId(9);
    game.accept_snapshot(invalid);
    assert!(game.completed_results.is_none());
    assert!(game.completed_results_error.is_some());
    game.accept_snapshot(snapshot(&ids, full, &gauge));
    assert!(game.completed_results.is_some());
    assert!(game.completed_results_error.is_none());
    for replay in [false, true] {
        let mut game = fixture_game();
        game.replay = replay;
        let mut prefix = snapshot(&ids, full, &gauge);
        prefix.completed_results = None;
        prefix.status = player::PlayerStatus::Finished;
        prefix.cancelled = !replay;
        prefix.score.hits = u64::MAX;
        game.accept_snapshot(prefix);
        game.owner_finished(true);
        assert!(game.completed_results.is_none());
        assert_eq!(game.snapshot.as_ref().unwrap().completed_results, None);
    }
}

#[test]
fn actual_results_paging_reaches_sixty_fourth_original_player_and_clamps_both_ends() {
    let (full, gauge) = completed(None);
    let ids = (0..64)
        .map(|index| PlayerId(u32::MAX - index * 17))
        .collect::<Vec<_>>();
    let mut game = fixture_game();
    game.accept_snapshot(snapshot(&ids, full, &gauge));
    game.owner_finished(true);
    let mut app = super::tests::lifecycle_fixture();
    app.game = Some(game);
    app.navigate(ScreenRoute::Play { replay: false }).unwrap();
    app.navigate(ScreenRoute::Results { replay: false })
        .unwrap();
    for _ in 0..20 {
        app.change_local_page(true);
    }
    let game = app.game.as_ref().unwrap();
    assert_eq!(game.local_page, 15);
    let rows = game.completed_results.as_ref().unwrap().rows();
    assert_eq!(rows[63].player, ids[63]);
    assert_eq!(rows[63].result, full);
    app.draw().unwrap();
    for _ in 0..20 {
        app.change_local_page(false);
    }
    assert_eq!(app.game.as_ref().unwrap().local_page, 0);
    app.draw().unwrap();
}
