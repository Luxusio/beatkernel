//! Deferred actual Desktop retained score/comparison mode and owner freezing.
use super::*;
use beatkernel_bms_runtime::{
    competition::ScoreSummary,
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
    gauge::BmsGauge,
    play_result::CompletedPlayResult,
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
        Some(end) => mixer_config.with_playback_end_frame((end as u64 + 999_999) / 1_000_000),
        None => mixer_config,
    };
    let mut mixer = Mixer::new(mixer_config, bank, consumer).unwrap();
    let first = mixer.render(&mut [0.0; 10]).unwrap();
    let finished = owner
        .observe_completion(Some(first), Some(point(2, 10_000_000)))
        .unwrap();
    if end.is_none() {
        assert!(!finished);
    }
    let second = mixer.render(&mut [0.0; 10]).unwrap();
    assert!(
        owner
            .observe_completion(Some(second), Some(point(2, 20_000_000)))
            .unwrap()
    );
    (*owner.completed_result().unwrap(), owner.gauge().clone())
}
fn comparison() -> CompetitionSnapshot {
    CompetitionSnapshot {
        ghosts: (0..8)
            .map(|index| GhostSnapshot {
                kind: if index % 2 == 0 {
                    OpponentKind::Own
                } else {
                    OpponentKind::Other
                },
                label: format!("prefix-{index}.bkr"),
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
                recorded_until: Some(Timestamp::from_nanos(604_800_000_000_001)),
            })
            .collect(),
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Disconnected,
            progress: Some(beatkernel_bms_runtime::multiplayer::Progress {
                song_ns: i64::MIN,
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
            }),
        }),
    }
}
fn snapshot(
    ids: &[PlayerId],
    result: CompletedPlayResult,
    gauge: &BmsGauge,
    comparisons: bool,
) -> player::PlayerSnapshot {
    player::PlayerSnapshot {
        players: ids
            .iter()
            .map(|&player| player::LocalPlayerSnapshot {
                bms_score: None,
                player,
                chart: None,
                song_time: Some(Timestamp::ZERO),
                score: ScoreSummary {
                    hits: u64::MAX,
                    combo: u64::MAX,
                    max_combo: u64::MAX,
                    ..Default::default()
                },
                mine_damage: Default::default(),
                gauge: gauge.clone(),
                last_judge: None,
                recent_results: vec![],
                pressed_lanes: 0,
                note_progress: None,
                competition: comparisons.then(comparison),
            })
            .collect(),
        completed_results: Some(ids.iter().map(|&id| (id, result)).collect()),
        status: player::PlayerStatus::Playing,
        ..Default::default()
    }
}
fn game() -> Game {
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
fn first_completed_game_freezes_scores_and_original_comparison_prefixes_through_cleanup_error() {
    let (result, gauge) = completed(None);
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let original = snapshot(&ids, result, &gauge, true);
    let mut game = game();
    game.accept_snapshot(original.clone());
    game.owner_finished(false);
    let mut later = original.clone();
    for member in &mut later.players {
        member.score = Default::default();
        member.competition = None;
    }
    later.completed_results = None;
    later.status = player::PlayerStatus::Failed("cleanup join failed".into());
    game.accept_snapshot(later);
    let frozen = game.completed_results.as_ref().unwrap().details();
    for (index, detail) in frozen.iter().enumerate() {
        assert_eq!(detail.player, ids[index]);
        assert_eq!(detail.score, original.players[index].score);
        assert_eq!(detail.competition, original.players[index].competition);
        assert_eq!(
            game.snapshot.as_ref().unwrap().players[index].score,
            original.players[index].score
        );
    }
    assert_eq!(
        game.snapshot.as_ref().unwrap().status,
        player::PlayerStatus::Failed("cleanup join failed".into())
    );
}

#[test]
fn actual_c_key_mode_and_page_controls_reach_all_comparisons_then_clamp_to_details() {
    for end in [None, Some(2_000_000)] {
        let (result, gauge) = completed(end);
        let mut game = game();
        game.accept_snapshot(snapshot(&[PlayerId(u32::MAX)], result, &gauge, true));
        assert!(!game.comparisons_available()); // The live solo path has no comparison control.
        game.owner_finished(true);
        let mut app = super::tests::lifecycle_fixture();
        app.game = Some(game);
        app.navigate(ScreenRoute::Play { replay: false }).unwrap();
        app.navigate(ScreenRoute::Results { replay: false })
            .unwrap();
        app.draw().unwrap();
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(8)));
        assert_eq!(app.game.as_ref().unwrap().presentation_page_count(), 1);
        app.key(KeyCode::KeyC, false);
        assert!(app.game.as_ref().unwrap().local_comparisons);
        assert_eq!(app.game.as_ref().unwrap().presentation_page_count(), 3);
        for _ in 0..8 {
            app.activate(ControlId(7));
        }
        assert_eq!(app.game.as_ref().unwrap().local_page, 2);
        app.draw().unwrap();
        let mut later = app
            .game
            .as_ref()
            .unwrap()
            .snapshot
            .as_ref()
            .unwrap()
            .clone();
        later.players[0].competition = None;
        app.game.as_mut().unwrap().accept_snapshot(later);
        assert_eq!(app.game.as_ref().unwrap().local_page, 2);
        app.key(KeyCode::KeyC, false);
        assert!(!app.game.as_ref().unwrap().local_comparisons);
        assert_eq!(app.game.as_ref().unwrap().local_page, 0);
        app.draw().unwrap();
        app.activate(ControlId(8));
        assert!(app.game.as_ref().unwrap().local_comparisons);
        assert_eq!(app.game.as_ref().unwrap().local_page, 0);
    }
}

#[test]
fn actual_game_rejects_later_invalid_detail_then_recovers_without_caching_partial_comparisons() {
    let (result, gauge) = completed(None);
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let valid = snapshot(&ids, result, &gauge, true);
    let mut invalid = valid.clone();
    invalid.players[1]
        .competition
        .as_mut()
        .unwrap()
        .ghosts
        .push(comparison().ghosts[0].clone());
    let mut game = game();
    game.accept_snapshot(invalid);
    assert!(game.completed_results.is_none());
    assert!(game.completed_results_error.is_some());
    game.accept_snapshot(valid.clone());
    assert!(game.completed_results_error.is_none());
    assert_eq!(game.completed_results.as_ref().unwrap().details().len(), 2);
    assert_eq!(
        game.completed_results.as_ref().unwrap().details()[1].competition,
        valid.players[1].competition
    );
    game.owner_finished(true);
    assert!(game.comparisons_available());
    assert_eq!(
        game.completed_results
            .as_ref()
            .unwrap()
            .page_count_for(true),
        5
    );
}

#[test]
fn completed_control_requires_frozen_data_and_replay_or_unproven_prefix_keeps_original_path() {
    let (result, gauge) = completed(None);
    for (replay, completed, comparisons) in [
        (false, true, false),
        (true, false, true),
        (false, false, true),
    ] {
        let mut game = game();
        game.replay = replay;
        let mut data = snapshot(&[PlayerId(u32::MAX)], result, &gauge, comparisons);
        if !completed {
            data.completed_results = None;
        }
        game.accept_snapshot(data);
        game.owner_finished(true);
        let mut app = super::tests::lifecycle_fixture();
        app.game = Some(game);
        app.navigate(ScreenRoute::Play { replay }).unwrap();
        app.navigate(ScreenRoute::Results { replay }).unwrap();
        app.draw().unwrap();
        assert!(!app.hits.iter().any(|(id, _)| *id == ControlId(8)));
        app.key(KeyCode::KeyC, false);
        assert!(!app.game.as_ref().unwrap().local_comparisons);
        if !completed {
            assert!(app.game.as_ref().unwrap().completed_results.is_none());
        }
    }
}
