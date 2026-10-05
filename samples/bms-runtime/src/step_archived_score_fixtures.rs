//! Actual portable judgments and Mixer completion before historical score export.
use crate::{
    PreparedBms,
    step_gameplay::{StepGameplay, StepLocalGameplay, StepGameplayConfig},
    local_players::{PlayerId, ResolvedInputPlan},
    result_archive::{decode_archive, ArchivedScore},
    play_result::PlayResultScope,
};
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, CommandProducer,
        command_queue,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    replay::codec::{ReplayCodecLimits, decode_replay},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp, Duration},
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
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(8192, 32, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn setup() -> (PreparedBms, StepGameplayConfig) {
    let source = beatkernel_bms::parse(
        "#BPM 3000\n#WAV01 tap.wav\n#00011:00010000\n",
        Default::default(),
    )
    .unwrap();
    let bank = SampleBank::new(
        AudioFormat::new(1000, 1).unwrap(),
        PcmLimits::new(64, 256, 1).unwrap(),
    )
    .unwrap();
    (
        PreparedBms {
            compiled: source.compile().unwrap(),
            source,
            bank,
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
fn bindings() -> BindingMap {
    BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn input(device: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, 20_000_000), 0),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}
fn output(bank: SampleBank, end: Option<i64>) -> (CommandProducer, Mixer) {
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
#[test]
fn actual_solo_hit_exports_exact_score_only_after_full_or_finite_mixer_completion() {
    for end in [None, Some(30_000_000)] {
        let (prepared, config) = setup();
        let (mut game, bank) = StepGameplay::new_section(
            prepared,
            config,
            bindings(),
            Timestamp::ZERO,
            end.map(Timestamp::from_nanos),
        )
        .unwrap();
        game.configure_capture(limits(), u64::MAX).unwrap();
        assert!(game.completed_archive().unwrap().is_none());
        game.activate(point(1, 0)).unwrap();
        game.process_input(input(1), &Domains, point(2, 0)).unwrap();
        assert_eq!(game.score().hits, 1);
        assert!(game.completed_archive().unwrap().is_none());
        game.advance_to(point(1, 30_000_000), &Domains, point(2, 0))
            .unwrap();
        let (mut producer, mut mixer) = output(bank, end);
        while let Some(batch) = game.take_commands(8).unwrap() {
            for command in &batch.commands {
                producer.try_push(*command).unwrap();
            }
            game.acknowledge(batch.sequence, batch.commands.len(), true)
                .unwrap();
        }
        for index in 1..=2 {
            let report = mixer.render(&mut [0.; 50]).unwrap();
            game.observe_completion(Some(report), Some(point(2, index * 50_000_000)))
                .unwrap();
        }
        assert!(game.completed_result().is_some());
        let archive = decode_archive(&game.completed_archive().unwrap().unwrap()).unwrap();
        let row = &archive.entries()[0];
        assert_eq!(row.player, PlayerId(1));
        assert_eq!(
            row.score,
            Some(ArchivedScore::from_summary(game.score()).unwrap())
        );
        let timing = &row.score.as_ref().unwrap().timing;
        assert_eq!(
            (timing.count, timing.exact, timing.sum, timing.absolute_sum),
            (1, 1, 0, 0)
        );
        assert_eq!(
            row.result.scope,
            match end {
                None => PlayResultScope::FullSong,
                Some(end) => PlayResultScope::PracticeSection {
                    start: Timestamp::ZERO,
                    end: Some(Timestamp::from_nanos(end))
                },
            }
        );
        game.fail();
        let replay = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
        assert_eq!(row.header, replay.header);
        assert!(game.completed_archive().is_err());
    }
}
#[test]
fn actual_local_hit_and_miss_export_whole_original_roster_and_consumed_member_refuses() {
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
        vec![bindings(), bindings()],
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(30_000_000)),
        beatkernel_bms::BmsInputMode::ButtonOnly,
    )
    .unwrap();
    for id in ids {
        game.configure_capture(id, limits(), id.0 as u64).unwrap();
    }
    game.activate(point(1, 0)).unwrap();
    game.process_input(input(1), &Domains, point(2, 0)).unwrap();
    assert!(game.completed_archive().unwrap().is_none());
    game.advance_to(point(1, 30_000_000), &Domains, point(2, 0))
        .unwrap();
    let (mut producer, mut mixer) = output(bank, Some(30_000_000));
    while let Some(batch) = game.take_commands(8).unwrap() {
        for command in &batch.commands {
            producer.try_push(*command).unwrap();
        }
        game.acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
    }
    for index in 1..=2 {
        let report = mixer.render(&mut [0.; 50]).unwrap();
        game.observe_completion(Some(report), Some(point(2, index * 50_000_000)))
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
            row.score,
            Some(ArchivedScore::from_summary(game.score(row.player).unwrap()).unwrap())
        );
        assert_eq!(
            row.result.gauge,
            game.completed_result(row.player).unwrap().unwrap().gauge()
        );
        assert_eq!(
            crate::replay_playback::decode_section_setup(&row.header.options)
                .unwrap()
                .chart_seed,
            row.player.0 as u64
        );
    }
    let first = archive.entries()[0].score.as_ref().unwrap();
    let last = archive.entries()[1].score.as_ref().unwrap();
    assert_eq!((first.hits, first.misses, first.timing.count), (1, 0, 1));
    assert_eq!((last.hits, last.misses, last.timing.count), (0, 1, 0));
    game.fail();
    let replay = decode_replay(&game.take_replay(ids[1]).unwrap().unwrap(), limits()).unwrap();
    assert_eq!(archive.entries()[1].header, replay.header);
    assert!(game.completed_archive().is_err());
}

#[test]
fn actual_hit_prefix_and_cancelled_owner_never_export_historical_completed_scores() {
    for recording in [false, true] {
        let (prepared, config) = setup();
        let (mut game, _) =
            StepGameplay::new_section(prepared, config, bindings(), Timestamp::ZERO, None).unwrap();
        if recording {
            game.configure_capture(limits(), 7).unwrap();
        }
        game.activate(point(1, 0)).unwrap();
        game.process_input(input(1), &Domains, point(2, 0)).unwrap();
        assert_eq!(game.score().hits, 1);
        assert!(game.completed_result().is_none());
        assert!(game.completed_archive().unwrap().is_none());
        game.fail();
        assert!(game.completed_result().is_none());
        assert!(game.completed_archive().unwrap().is_none());
    }
}
