//! Deferred actual Step/Mixer completion and cold archive extraction.
use crate::{
    PreparedBms,
    step_gameplay::{StepGameplay, StepLocalGameplay, StepGameplayConfig},
    local_players::{PlayerId, ResolvedInputPlan},
    result_archive::decode_archive,
    play_result::PlayResultScope,
};
use beatkernel::{
    audio::{AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue},
    input::{BindingMap, DeviceId},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp, Duration},
    replay::codec::decode_replay,
};
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
                Some(end) => config.with_playback_end_frame((end as u64 + 999_999) / 1_000_000),
                None => config,
            },
            bank,
            consumer,
        )
        .unwrap(),
    )
}
#[test]
fn actual_solo_completion_preserves_capture_header_and_scope_before_one_shot_replay_take() {
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
        let limits = crate::native_judge::capture_limits(true, 8192, 8)
            .unwrap()
            .unwrap();
        game.configure_capture(limits, u64::MAX).unwrap();
        assert!(game.completed_archive().unwrap().is_none());
        game.activate(point(1, 0)).unwrap();
        game.advance_to(point(1, end.unwrap_or(1_000_000)), &Domains, point(2, 0))
            .unwrap();
        assert!(game.completed_archive().unwrap().is_none());
        let (_producer, mut mixer) = output(bank, end);
        for index in 1..=2 {
            let report = mixer.render(&mut [0.; 10]).unwrap();
            game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
                .unwrap();
        }
        let bytes = game.completed_archive().unwrap().unwrap();
        let archived = decode_archive(&bytes).unwrap();
        assert_eq!(archived.entries()[0].player, PlayerId(1));
        assert_eq!(
            archived.entries()[0].result.gauge,
            game.completed_result().unwrap().gauge()
        );
        assert_eq!(
            archived.entries()[0].result.scope,
            match end {
                None => PlayResultScope::FullSong,
                Some(end) => PlayResultScope::PracticeSection {
                    start: Timestamp::ZERO,
                    end: Some(Timestamp::from_nanos(end))
                },
            }
        );
        assert_eq!(archived.entries()[0].profile, *game.gauge().profile());
        game.fail();
        let replay = decode_replay(&game.take_replay().unwrap().unwrap(), limits).unwrap();
        assert_eq!(archived.entries()[0].header, replay.header);
        assert!(game.completed_archive().is_err());
    }
}

#[test]
fn actual_step_completion_copies_dynamic_gauge_policy_to_archive() {
    use crate::gauge::{GaugeProfile, GaugeDynamics};
    let profile = GaugeProfile::new(100_000_000, 0, 1, -10_000_000, true, vec![])
        .unwrap()
        .with_dynamics(GaugeDynamics {
            minimum_alive: 0,
            failure_below: 2_000_000,
            damage_reduction_below: 32_000_000,
        })
        .unwrap();
    let (prepared, config) = setup();
    let (mut game, bank) = StepGameplay::new_section(
        prepared,
        config,
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(5_000_000)),
    )
    .unwrap();
    game.configure_gauge(profile.clone()).unwrap();
    let limits = crate::native_judge::capture_limits(true, 8192, 8)
        .unwrap()
        .unwrap();
    game.configure_capture(limits, 1).unwrap();
    assert!(game.configure_gauge(profile.clone()).is_err());
    game.activate(point(1, 0)).unwrap();
    game.advance_to(point(1, 5_000_000), &Domains, point(2, 0))
        .unwrap();
    let (_producer, mut mixer) = output(bank, Some(5_000_000));
    for index in 0..3 {
        let mut pcm = [1.; 10];
        let report = mixer.render(&mut pcm).unwrap();
        game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
            .unwrap();
    }
    let bytes = game.completed_archive().unwrap().unwrap();
    assert_eq!(
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        crate::result_archive::DYNAMIC_VERSION
    );
    assert_eq!(
        decode_archive(&bytes).unwrap().entries()[0].profile,
        profile
    );
}

#[test]
fn actual_local_completion_keeps_distinct_dynamic_and_legacy_member_policies() {
    use crate::gauge::{GaugeProfile, GaugeDynamics};
    let profile = GaugeProfile::new(100_000_000, 0, 1, -10_000_000, true, vec![])
        .unwrap()
        .with_dynamics(GaugeDynamics {
            minimum_alive: 0,
            failure_below: 2_000_000,
            damage_reduction_below: 32_000_000,
        })
        .unwrap();
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let plan = ResolvedInputPlan::new(vec![
        (ids[0], Some(DeviceId(1))),
        (ids[1], Some(DeviceId(2))),
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
        Some(Timestamp::from_nanos(5_000_000)),
        beatkernel_bms::BmsInputMode::ButtonOnly,
    )
    .unwrap();
    game.configure_gauge(ids[0], profile.clone()).unwrap();
    let limits = crate::native_judge::capture_limits(true, 8192, 8)
        .unwrap()
        .unwrap();
    for id in ids {
        game.configure_capture(id, limits, 1).unwrap();
    }
    assert!(game.configure_gauge(ids[1], profile.clone()).is_err());
    game.activate(point(1, 0)).unwrap();
    game.advance_to(point(1, 5_000_000), &Domains, point(2, 0))
        .unwrap();
    let (_producer, mut mixer) = output(bank, Some(5_000_000));
    for index in 0..3 {
        let mut pcm = [1.; 10];
        let report = mixer.render(&mut pcm).unwrap();
        game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
            .unwrap();
    }
    let decoded = decode_archive(&game.completed_archive().unwrap().unwrap()).unwrap();
    assert_eq!(
        decoded
            .entries()
            .iter()
            .find(|row| row.player == ids[0])
            .unwrap()
            .profile,
        profile
    );
    assert_eq!(
        decoded
            .entries()
            .iter()
            .find(|row| row.player == ids[1])
            .unwrap()
            .profile,
        GaugeProfile::default()
    );
}
#[test]
fn actual_local_whole_roster_preserves_all_original_ids_and_missing_later_capture_refuses() {
    for count in 1u32..=64 {
        let ids = (0..count)
            .map(|index| PlayerId(u32::MAX - index * 17))
            .collect::<Vec<_>>();
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
        let limits = crate::native_judge::capture_limits(true, 8192, 8)
            .unwrap()
            .unwrap();
        for id in &ids {
            game.configure_capture(*id, limits, id.0 as u64).unwrap();
        }
        assert!(game.completed_archive().unwrap().is_none());
        game.activate(point(1, 0)).unwrap();
        game.advance_to(point(1, 1_000_000), &Domains, point(2, 0))
            .unwrap();
        let (_producer, mut mixer) = output(bank, None);
        for index in 1..=2 {
            let report = mixer.render(&mut [0.; 10]).unwrap();
            game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
                .unwrap();
        }
        let archive = decode_archive(&game.completed_archive().unwrap().unwrap()).unwrap();
        assert_eq!(
            archive
                .entries()
                .iter()
                .map(|row| row.player)
                .collect::<Vec<_>>(),
            ids
        );
        for row in archive.entries() {
            assert_eq!(
                crate::replay_playback::decode_section_setup(&row.header.options)
                    .unwrap()
                    .chart_seed,
                row.player.0 as u64
            );
            assert_eq!(
                row.result.gauge,
                game.completed_result(row.player).unwrap().unwrap().gauge()
            );
        }
        game.fail();
        let last = *ids.last().unwrap();
        let replay = decode_replay(&game.take_replay(last).unwrap().unwrap(), limits).unwrap();
        assert_eq!(archive.entries().last().unwrap().header, replay.header);
        assert!(game.completed_archive().is_err());
    }
}
#[test]
fn actual_completed_disabled_solo_and_missing_unconfigured_later_local_capture_refuse_archive() {
    let (prepared, config) = setup();
    let (mut solo, bank) = StepGameplay::new_section(
        prepared,
        config,
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        None,
    )
    .unwrap();
    solo.activate(point(1, 0)).unwrap();
    solo.advance_to(point(1, 1_000_000), &Domains, point(2, 0))
        .unwrap();
    let (_producer, mut mixer) = output(bank, None);
    for index in 1..=2 {
        let report = mixer.render(&mut [0.; 10]).unwrap();
        solo.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
            .unwrap();
    }
    assert!(solo.completed_result().is_some());
    assert!(solo.completed_archive().unwrap().is_none());
    let ids = [PlayerId(u32::MAX), PlayerId(7)];
    let plan = ResolvedInputPlan::new(vec![
        (ids[0], Some(DeviceId(1))),
        (ids[1], Some(DeviceId(2))),
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
        Some(Timestamp::from_nanos(5_000_000)),
        beatkernel_bms::BmsInputMode::ButtonOnly,
    )
    .unwrap();
    game.configure_capture(
        ids[0],
        crate::native_judge::capture_limits(true, 8192, 8)
            .unwrap()
            .unwrap(),
        19,
    )
    .unwrap();
    game.activate(point(1, 0)).unwrap();
    game.advance_to(point(1, 5_000_000), &Domains, point(2, 0))
        .unwrap();
    let (_producer, mut mixer) = output(bank, Some(5_000_000));
    for index in 1..=2 {
        let report = mixer.render(&mut [0.; 10]).unwrap();
        game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
            .unwrap();
    }
    assert!(game.completed_result(ids[1]).unwrap().is_some());
    assert!(game.completed_archive().is_err());
}
#[test]
fn disabled_and_cancelled_prefix_cannot_export_completed_archive() {
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
            game.configure_capture(
                crate::native_judge::capture_limits(true, 8192, 8)
                    .unwrap()
                    .unwrap(),
                0,
            )
            .unwrap();
        }
        game.fail();
        assert!(game.completed_result().is_none());
        assert!(game.completed_archive().unwrap().is_none());
    }
}
