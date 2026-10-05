//! Deferred actual portable completion extraction, with no platform owners.
use super::*;
use crate::{
    PreparedBms,
    local_players::ResolvedInputPlan,
    step_gameplay::StepGameplayConfig,
    play_result::PlayResultScope,
    competition_presentation::{GhostSnapshot, CompetitionSnapshot},
    competition::OpponentKind,
};
use beatkernel::{
    audio::{AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue},
    input::{BindingMap, DeviceId},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::BmsInputMode;
struct Domains;
impl ClockMapper for Domains {
    fn map(&self, p: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (p.domain == target).then_some(p.timestamp)
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
fn setup() -> (PreparedBms, StepGameplayConfig) {
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    (
        PreparedBms {
            compiled: source.compile().unwrap(),
            source,
            bank: SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap(),
            sounds: vec![],
            bgm_commands: vec![],
        },
        StepGameplayConfig {
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
        },
    )
}
fn mixer(bank: SampleBank, end: Option<i64>) -> Mixer {
    let (_, consumer) = command_queue(8).unwrap();
    let config = MixerConfig::new(
        AudioFormat::new(1000, 1).unwrap(),
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
    );
    let config = match end {
        Some(end) => config.with_playback_end_frame((end as u64 + 999_999) / 1_000_000),
        None => config,
    };
    Mixer::new(config, bank, consumer).unwrap()
}
fn solo(end: Option<i64>) -> StepGameplay {
    let (prepared, config) = setup();
    let (mut game, bank) = StepGameplay::new_section(
        prepared,
        config,
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        end.map(Timestamp::from_nanos),
    )
    .unwrap();
    game.activate(point(1, 0)).unwrap();
    game.advance_to(point(1, end.unwrap_or(1_000_000)), &Domains, point(2, 0))
        .unwrap();
    let mut output = mixer(bank, end);
    let first = output.render(&mut [0.0; 10]).unwrap();
    game.observe_completion(Some(first), Some(point(2, 10_000_000)))
        .unwrap();
    let second = output.render(&mut [0.0; 10]).unwrap();
    assert!(
        game.observe_completion(Some(second), Some(point(2, 20_000_000)))
            .unwrap()
    );
    game
}
#[test]
fn unproven_live_owner_cannot_create_results() {
    let (prepared, config) = setup();
    let (game, _) = StepGameplay::new_section(
        prepared,
        config,
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        None,
    )
    .unwrap();
    let mut retained = CompletedResultsPresentation::default();
    assert!(!retained.capture_solo(&game, None).unwrap());
    assert!(retained.results().is_none());
    assert!(retained.view().is_none());
}
#[test]
fn real_mixer_proof_freezes_full_or_practice_and_first_prefix() {
    for end in [None, Some(5_000_000)] {
        let game = solo(end);
        let mut prefix = CompetitionSnapshot {
            ghosts: vec![GhostSnapshot {
                kind: OpponentKind::Own,
                label: "historical".into(),
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
                recorded_until: Some(Timestamp::from_nanos(604_800_000_000_001)),
            }],
            network: None,
        };
        let mut retained = CompletedResultsPresentation::default();
        assert!(retained.capture_solo(&game, Some(&prefix)).unwrap());
        prefix.ghosts[0].hits = 0;
        prefix.ghosts.clear();
        assert!(retained.capture_solo(&game, Some(&prefix)).unwrap());
        let view = retained.view().unwrap();
        assert_eq!(view.rows()[0].player, PlayerId(1));
        assert_eq!(view.rows()[0].gauge_label, "GAUGE 20.000000%");
        assert_eq!(view.details()[0].score, game.score().clone());
        let frozen = view.details()[0].competition.as_ref().unwrap();
        assert_eq!(frozen.ghosts[0].hits, u64::MAX);
        assert_eq!(
            frozen.ghosts[0].recorded_until,
            Some(Timestamp::from_nanos(604_800_000_000_001))
        );
        assert_eq!(
            view.rows()[0].result.scope(),
            match end {
                None => PlayResultScope::FullSong,
                Some(end) => PlayResultScope::PracticeSection {
                    start: Timestamp::ZERO,
                    end: Some(Timestamp::from_nanos(end))
                },
            }
        );
        assert!(!view.rows()[0].result.whole_song_clear());
        drop(game);
        assert!(retained.view().unwrap().has_comparisons());
    }
}
#[test]
fn invalid_display_prefix_retains_actual_proof_without_replacing_it_on_retry() {
    let game = solo(None);
    let bad = CompetitionSnapshot {
        ghosts: vec![GhostSnapshot {
            kind: OpponentKind::Other,
            label: "bad\nlabel".into(),
            hits: 1,
            misses: 0,
            combo: 1,
            max_combo: 1,
            recorded_until: None,
        }],
        network: None,
    };
    let mut retained = CompletedResultsPresentation::default();
    assert!(retained.capture_solo(&game, Some(&bad)).is_err());
    assert!(retained.view().is_none());
    assert!(retained.error().is_some());
    let original = retained.results().unwrap().to_vec();
    assert_eq!(
        original,
        vec![(PlayerId(1), *game.completed_result().unwrap())]
    );
    assert!(retained.capture_solo(&game, None).unwrap());
    assert_eq!(retained.results().unwrap(), original);
    assert!(retained.view().is_none());
    assert!(retained.error().is_some());
}
#[test]
fn actual_local_completion_requires_entire_original_roster_and_rejects_bad_later_row() {
    let ids = [PlayerId(u32::MAX), PlayerId(7)];
    let plan = ResolvedInputPlan::new(vec![
        (ids[0], Some(DeviceId(u64::MAX))),
        (ids[1], Some(DeviceId(0))),
    ])
    .unwrap();
    let (prepared, config) = setup();
    let (mut game, bank) = StepLocalGameplay::new_section(
        prepared,
        config,
        plan,
        vec![
            BindingMap::from_bindings([]).unwrap(),
            BindingMap::from_bindings([]).unwrap(),
        ],
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    let mut retained = CompletedResultsPresentation::default();
    assert!(!retained.capture_local(&game, &[]).unwrap());
    game.activate(point(1, 0)).unwrap();
    game.advance_to(point(1, 1_000_000), &Domains, point(2, 0))
        .unwrap();
    let mut output = mixer(bank, None);
    for block in 1..=2 {
        let report = output.render(&mut [0.0; 10]).unwrap();
        game.observe_completion(Some(report), Some(point(2, block * 10_000_000)))
            .unwrap();
    }
    assert!(
        retained
            .capture_local(&game, &[(ids[0], None), (PlayerId(8), None)])
            .is_err()
    );
    assert!(retained.view().is_none());
    assert!(retained.error().is_some());
    assert_eq!(
        retained
            .results()
            .unwrap()
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        ids
    );
    assert!(retained.capture_local(&game, &[]).unwrap());
    assert!(retained.view().is_none());
    let mut retained = CompletedResultsPresentation::default();
    assert!(
        retained
            .capture_local(&game, &[(ids[1], None), (ids[0], None)])
            .unwrap()
    );
    assert_eq!(
        retained
            .view()
            .unwrap()
            .rows()
            .iter()
            .map(|row| row.player)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(
        retained.view().unwrap().details()[1].score,
        game.score(ids[1]).unwrap().clone()
    );
}
