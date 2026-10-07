//! Deferred actual Step/Mixer completion with borrowed display-only comparisons.
use super::{StepGameplay, StepLocalGameplay, StepGameplayConfig};
use crate::{
    PreparedBms,
    local_players::{PlayerId, ResolvedInputPlan},
    result_archive::{decode_archive, ArchivedScore},
    competition::OpponentKind,
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
    multiplayer::Progress,
};
use beatkernel::{
    audio::{AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue},
    input::{BindingMap, CodecLimits, DeviceId},
    replay::codec::{ReplayCodecLimits, decode_replay},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp, Duration},
};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
struct Domains;
impl ClockMapper for Domains {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(8192, 32, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn setup() -> (PreparedBms, StepGameplayConfig) {
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    (
        PreparedBms {
            compiled: source.compile().unwrap(),
            source,
            bank: SampleBank::new(
                AudioFormat::new(1000, 1).unwrap(),
                PcmLimits::new(64, 256, 1).unwrap(),
            )
            .unwrap(),
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
fn output(bank: SampleBank, end: Option<i64>) -> (beatkernel::audio::CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(8).unwrap();
    let config = MixerConfig::new(
        AudioFormat::new(1000, 1).unwrap(),
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
    );
    (
        producer,
        Mixer::new(
            match end {
                Some(end) => config.with_playback_end_frame(end as u64 / 1_000_000),
                None => config,
            },
            bank,
            consumer,
        )
        .unwrap(),
    )
}
fn selected() -> CompetitionSnapshot {
    // Borrowed port data; these prefix statistics convey no local completion.
    CompetitionSnapshot {
        ghosts: vec![GhostSnapshot {
            kind: OpponentKind::Own,
            label: "saved.bkr".into(),
            hits: 5,
            misses: 1,
            combo: 3,
            max_combo: 5,
            recorded_until: Some(Timestamp::from_nanos(604_800_000_000_000)),
        }],
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Disconnected,
            progress: Some(Progress {
                song_ns: 9_007_199_254_740_993,
                hits: 3,
                misses: 1,
                combo: 0,
                max_combo: 3,
            }),
        }),
    }
}
#[test]
fn actual_solo_full_and_finite_completion_attach_borrowed_comparisons_before_one_shot_capture_take()
{
    for end in [None, Some(5_000_000)] {
        let (prepared, config) = setup();
        let (mut game, bank) = StepGameplay::new_section(
            prepared,
            config,
            BindingMap::from_bindings([]).unwrap(),
            Timestamp::ZERO,
            end.map(Timestamp::from_nanos),
        )
        .unwrap();
        game.configure_capture(limits(), u64::MAX).unwrap();
        let selected = selected();
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(0), Some(&selected))])
                .unwrap()
                .is_none()
        );
        game.activate(point(1, 0)).unwrap();
        game.advance_to(point(1, end.unwrap_or(1_000_000)), &Domains, point(2, 0))
            .unwrap();
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(1), Some(&selected))])
                .unwrap()
                .is_none()
        );
        let (_producer, mut mixer) = output(bank, end);
        for index in 1..=2 {
            let report = mixer.render(&mut [0.; 10]).unwrap();
            game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
                .unwrap();
        }
        assert!(game.completed_result().is_some());
        let old = game.completed_archive().unwrap().unwrap();
        assert_eq!(&old[8..12], &2u32.to_le_bytes());
        assert!(decode_archive(&old).unwrap().comparisons().is_none());
        let bytes = game
            .completed_archive_with_comparisons(&[(PlayerId(1), Some(&selected))])
            .unwrap()
            .unwrap();
        let archive = decode_archive(&bytes).unwrap();
        assert_eq!(&bytes[8..12], &3u32.to_le_bytes());
        assert_eq!(archive.comparison(PlayerId(1)), Some(&selected));
        assert_eq!(
            archive.entries()[0].score,
            Some(ArchivedScore::from_summary(game.score()).unwrap())
        );
        assert_eq!(
            archive.entries()[0].result.gauge,
            game.completed_result().unwrap().gauge()
        );
        game.fail();
        let replay = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
        assert_eq!(archive.entries()[0].header, replay.header);
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(1), Some(&selected))])
                .is_err()
        );
    }
}
#[test]
fn actual_local_completion_exports_whole_original_roster_and_validates_shuffled_none_or_selected_rows()
 {
    for count in [1u32, 2, 64] {
        let ids: Vec<_> = (0..count)
            .map(|index| PlayerId(u32::MAX - index * 17))
            .collect();
        let plan = ResolvedInputPlan::new(
            ids.iter()
                .enumerate()
                .map(|(index, id)| (*id, Some(DeviceId(index as u64))))
                .collect(),
        )
        .unwrap();
        let (prepared, config) = setup();
        let (mut game, bank) = StepLocalGameplay::new_section(
            prepared,
            config,
            plan,
            ids.iter()
                .map(|_| BindingMap::from_bindings([]).unwrap())
                .collect(),
            Timestamp::ZERO,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
        )
        .unwrap();
        for id in &ids {
            game.configure_capture(*id, limits(), id.0 as u64).unwrap();
        }
        let selected = selected();
        let rows: Vec<_> = ids
            .iter()
            .enumerate()
            .rev()
            .map(|(index, id)| {
                (
                    *id,
                    if index % 2 == 0 {
                        Some(&selected)
                    } else {
                        None
                    },
                )
            })
            .collect();
        assert!(
            game.completed_archive_with_comparisons(&rows)
                .unwrap()
                .is_none()
        );
        game.activate(point(1, 0)).unwrap();
        game.advance_to(point(1, 1_000_000), &Domains, point(2, 0))
            .unwrap();
        let (_producer, mut mixer) = output(bank, None);
        for index in 1..=2 {
            let report = mixer.render(&mut [0.; 10]).unwrap();
            game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
                .unwrap();
        }
        let bytes = game
            .completed_archive_with_comparisons(&rows)
            .unwrap()
            .unwrap();
        let archive = decode_archive(&bytes).unwrap();
        assert_eq!(
            archive
                .comparisons()
                .unwrap()
                .iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            ids
        );
        for (index, row) in archive.entries().iter().enumerate() {
            assert_eq!(
                archive.comparison(row.player),
                if index % 2 == 0 {
                    Some(&selected)
                } else {
                    None
                }
            );
            assert_eq!(
                row.score,
                Some(ArchivedScore::from_summary(game.score(row.player).unwrap()).unwrap())
            );
            assert_eq!(
                row.result.gauge,
                game.completed_result(row.player).unwrap().unwrap().gauge()
            );
        }
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(0), None)])
                .is_err()
        );
        game.fail();
        game.take_replay(*ids.last().unwrap()).unwrap().unwrap();
        assert!(game.completed_archive_with_comparisons(&rows).is_err());
    }
}
#[test]
fn cancelled_or_disabled_owner_cannot_promote_comparison_data_and_invalid_completed_attachment_keeps_proof()
 {
    let selected = selected();
    for recording in [false, true] {
        let (prepared, config) = setup();
        let (mut game, _) = StepGameplay::new_section(
            prepared,
            config,
            BindingMap::from_bindings([]).unwrap(),
            Timestamp::ZERO,
            None,
        )
        .unwrap();
        if recording {
            game.configure_capture(limits(), 7).unwrap();
        }
        game.fail();
        assert!(game.completed_result().is_none());
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(1), Some(&selected))])
                .unwrap()
                .is_none()
        );
    }
    {
        let (prepared, config) = setup();
        let (mut disabled, bank) = StepGameplay::new_section(
            prepared,
            config,
            BindingMap::from_bindings([]).unwrap(),
            Timestamp::ZERO,
            None,
        )
        .unwrap();
        disabled.activate(point(1, 0)).unwrap();
        disabled
            .advance_to(point(1, 1_000_000), &Domains, point(2, 0))
            .unwrap();
        let (_producer, mut mixer) = output(bank, None);
        for index in 1..=2 {
            let report = mixer.render(&mut [0.; 10]).unwrap();
            disabled
                .observe_completion(Some(report), Some(point(2, index * 10_000_000)))
                .unwrap();
        }
        assert!(disabled.completed_result().is_some());
        assert!(
            disabled
                .completed_archive_with_comparisons(&[(PlayerId(0), Some(&selected))])
                .unwrap()
                .is_none()
        );
    }
    let (prepared, config) = setup();
    let (mut game, bank) = StepGameplay::new_section(
        prepared,
        config,
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        None,
    )
    .unwrap();
    game.configure_capture(limits(), 7).unwrap();
    game.activate(point(1, 0)).unwrap();
    game.advance_to(point(1, 1_000_000), &Domains, point(2, 0))
        .unwrap();
    let (_producer, mut mixer) = output(bank, None);
    for index in 1..=2 {
        let report = mixer.render(&mut [0.; 10]).unwrap();
        game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
            .unwrap();
    }
    let proof = *game.completed_result().unwrap();
    let score = game.score().clone();
    let mut bad = selected.clone();
    bad.ghosts[0].label = "invalid\nlabel".into();
    assert!(
        game.completed_archive_with_comparisons(&[(PlayerId(1), Some(&bad))])
            .is_err()
    );
    assert_eq!(game.completed_result(), Some(&proof));
    assert_eq!(game.score(), &score);
    assert!(
        decode_archive(
            &game
                .completed_archive_with_comparisons(&[(PlayerId(1), None)])
                .unwrap()
                .unwrap()
        )
        .unwrap()
        .comparisons()
        .unwrap()[0]
            .1
            .is_none()
    );
}
