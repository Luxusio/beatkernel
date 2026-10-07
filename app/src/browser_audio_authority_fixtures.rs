//! Portable browser queue joined to actual Step/Runtime owners, never JsValue bindings.
use crate::{
    PreparedBms,
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    browser_hid_input::BrowserHidSetup,
    browser_input::{BrowserInputQueue, project_touch_on_surface},
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepLocalGameplay},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
        SampleBank, SampleId, VoiceId, command_queue,
    },
    input::*,
    judge::{JudgeOutcome, JudgeStage},
    runtime::SoundBinding,
    time::{
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, ExtrapolationPolicy,
        Timestamp,
    },
};
use beatkernel_bms::BmsInputMode;
const H: i64 = 10_000_000_000;
const R: i64 = 1_000_000_000;
const L: i64 = 5_000_000_000;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(delta: i64) -> ClockPoint {
    point(11, H + delta)
}
fn raw(delta: i64) -> ClockPoint {
    point(22, R + delta)
}
fn logical(delta: i64) -> ClockPoint {
    point(33, L + delta)
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: host(0),
        output_origin: raw(0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 16,
        bgm_pending: 2,
        bgm_lookahead: Duration::from_nanos(100_000_000),
        telemetry_capacity: 8,
    }
}
fn authority() -> AudioAuthority {
    AudioAuthority::new(
        AudioAuthorityConfig {
            history_capacity: 8,
            max_observation_age: Duration::from_nanos(1_000_000_000),
            input_extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: 1,
            stream_origin: raw(0),
            logical_origin: logical(0),
            host_domain: ClockDomainId(11),
        },
    )
    .unwrap()
}
fn prepared() -> PreparedBms {
    let source = beatkernel_bms::parse(
        "#BPM 3000\n#WAV01 key.wav\n#00011:00010000",
        Default::default(),
    )
    .unwrap();
    let compiled = source.compile().unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, -0.25], limits).unwrap(),
    )
    .unwrap();
    let sounds = compiled
        .chart
        .objects()
        .iter()
        .map(|object| SoundBinding {
            object: object.id,
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(object.id.0),
            gain: 1.0,
        })
        .collect();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands: vec![],
    }
}
fn binding(device: Option<DeviceId>, control: PhysicalControlId) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device: device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
        physical: control,
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn event(delta: i64, device: u64, seq: u64) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(device), host(delta), seq);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(7),
        code: Some(4),
        timestamp: Some(host(delta)),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}
fn solo() -> StepGameplay {
    let (mut step, _) = StepGameplay::new_audio_section(
        prepared(),
        config(),
        binding(None, PhysicalControlId::keyboard(7u16)),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        authority(),
    )
    .unwrap();
    step.activate_audio().unwrap();
    step
}
fn pair(step: &mut StepGameplay) {
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(0),
            target: host(0),
        },
    )
    .unwrap();
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(40_000_000),
            target: host(20_000_000),
        },
    )
    .unwrap();
}
fn queue() -> BrowserInputQueue {
    BrowserInputQueue::solo(host(0)).unwrap()
}
fn capture_limits() -> beatkernel::replay::codec::ReplayCodecLimits {
    beatkernel::replay::codec::ReplayCodecLimits::new(
        65536,
        128,
        4096,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}
fn state(step: &StepGameplay) -> String {
    format!(
        "{:?}",
        (
            step.song_time(),
            step.score(),
            step.gauge(),
            step.mine_damage(),
            step.failed(),
            step.audio_authority()
        )
    )
}
#[test]
fn real_activation_origin_and_warmup_hold_then_dispatch_original_occurrence_once() {
    let (mut game, _) = StepGameplay::new_audio_section(
        prepared(),
        config(),
        binding(None, PhysicalControlId::keyboard(7u16)),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        authority(),
    )
    .unwrap();
    game.configure_capture(capture_limits(), 0).unwrap();
    game.activate_audio().unwrap();
    let mut queue = queue();
    queue.register_source(DeviceId(u64::MAX)).unwrap();
    assert!(queue.admit(event(-1, u64::MAX, 0), host(0), None).is_err());
    assert_eq!(queue.pending(), 0);
    let original = event(10_000_000, u64::MAX, 0);
    queue
        .admit(original.clone(), host(20_000_000), None)
        .unwrap();
    game.record_audio_prefix(host(20_000_000)).unwrap();
    for count in 0..2 {
        let before = state(&game);
        assert!(
            queue
                .process_next_solo(&mut game, host(20_000_000), raw(50_000_000))
                .unwrap()
                .is_none()
        );
        assert_eq!(queue.pending(), 1);
        assert_eq!(state(&game), before);
        if count == 0 {
            game.observe_audio_output(
                1,
                ClockPair {
                    source: raw(0),
                    target: host(0),
                },
            )
            .unwrap();
        }
    }
    game.observe_audio_output(
        1,
        ClockPair {
            source: raw(40_000_000),
            target: host(20_000_000),
        },
    )
    .unwrap();
    let before = state(&game);
    assert!(
        queue
            .process_next_solo(&mut game, host(1_020_000_001), raw(50_000_000))
            .unwrap()
            .is_none()
    );
    assert_eq!(queue.pending(), 1);
    assert_eq!(state(&game), before);
    let report = queue
        .process_next_solo(&mut game, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(20_000_000));
    assert_eq!(report.audio_at, raw(50_000_000));
    assert_eq!(report.input_mapping_quality, ClockMappingQuality::Unknown);
    let actual = report.input.unwrap();
    assert_eq!(actual.meta().original_clock_point, Some(host(10_000_000)));
    assert_eq!(actual.meta().clock_domain, ClockDomainId(33));
    assert_eq!(actual.meta().timestamp, logical(20_000_000).timestamp);
    assert_eq!(actual.meta().native, original.meta().native);
    assert_eq!(actual.meta().source, DeviceId(u64::MAX));
    assert!(report.audio_commands.iter().any(
        |command| matches!(command,AudioCommand::Play {at,..} if *at==raw(50_000_000).timestamp)
    ));
    assert_eq!(game.score().hits, 1);
    assert_eq!(queue.pending(), 0);
    assert!(
        queue
            .process_next_solo(&mut game, host(20_000_000), raw(50_000_000))
            .unwrap()
            .is_none()
    );
    assert_eq!(game.score().hits, 1);
    game.fail();
    let file = beatkernel::replay::codec::decode_replay(
        &game.take_replay().unwrap().unwrap(),
        capture_limits(),
    )
    .unwrap();
    assert_eq!(file.header.normalized_clock, ClockDomainId(33));
    assert_eq!(file.records.len(), 1);
    let beatkernel::replay::ReplayOperation::Input(saved) = &file.records[0].operation else {
        panic!("expected queued input in capture")
    };
    assert_eq!(
        saved.physical.meta().original_clock_point,
        Some(host(10_000_000))
    );
    assert_eq!(file.records[0].song_time, Timestamp::from_nanos(20_000_000));
}

#[test]
fn delayed_touch_uses_stored_acquisition_projection_and_preserves_raw_position() {
    let surface = PhysicalControlId::Native {
        backend: BackendId(17),
        code: 1,
    };
    let (mut game, _) = StepGameplay::new_audio_section(
        prepared(),
        config(),
        binding(None, surface),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOrContact,
        authority(),
    )
    .unwrap();
    game.configure_touch_router(
        TouchRouter::new(
            vec![TouchRegion {
                device: DeviceSelector::Any,
                physical: surface,
                game_control: GameControlId(0x11),
                min: Position2 { x: 0.0, y: 0.0 },
                max: Position2 { x: 10.0, y: 10.0 },
            }],
            4,
        )
        .unwrap(),
    )
    .unwrap();
    game.activate_audio().unwrap();
    let mut queue = queue();
    queue.register_source(DeviceId(2)).unwrap();
    let touch = PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(2), host(10_000_000), 0),
        control: surface,
        contact: ContactId(77),
        phase: TouchPhase::Down,
        position: Position2 { x: 25.0, y: 25.0 },
        pressure: Some(0.75),
    });
    let projected = project_touch_on_surface(&touch, [100.0, 100.0], [100, 100], [20, 20]).unwrap();
    assert_eq!(projected, Position2 { x: 5.0, y: 5.0 });
    queue
        .admit(touch.clone(), host(20_000_000), Some(projected))
        .unwrap();
    game.record_audio_prefix(host(20_000_000)).unwrap();
    assert!(
        queue
            .process_next_solo(&mut game, host(20_000_000), raw(50_000_000))
            .unwrap()
            .is_none()
    );
    let resized = project_touch_on_surface(&touch, [50.0, 50.0], [100, 100], [20, 20]).unwrap();
    assert_eq!(resized, Position2 { x: 10.0, y: 10.0 });
    pair(&mut game);
    let report = queue
        .process_next_solo(&mut game, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(report.judge_events.len(), 1);
    assert!(matches!(
        report.judge_events[0].outcome,
        JudgeOutcome::Hit { .. }
    ));
    assert_eq!(game.score().hits, 1);
    let PhysicalInputEvent::Touch(retained) = report.input.unwrap() else {
        panic!("expected raw touch retained")
    };
    assert_eq!(retained.position, Position2 { x: 25.0, y: 25.0 });
    assert_eq!(retained.contact, ContactId(77));
    assert_eq!(retained.pressure, Some(0.75));
    assert_eq!(retained.meta.original_clock_point, Some(host(10_000_000)));
    assert_eq!(queue.pending(), 0);
}

#[test]
fn typed_hid_and_zero_emission_raw_reports_reach_real_queue_without_identity_synthesis() {
    let source = DeviceId(0xfedc_ba98_7654_3210);
    let control = PhysicalControlId::HidUsage {
        usage_page: 9,
        usage: 1,
    };
    let bindings = binding(Some(source), control);
    let mut hid = BrowserHidSetup::new(
        &[
            source.0 as u32,
            (source.0 >> 32) as u32,
            1,
            0xffff,
            1,
            0xabcd,
        ],
        &[0, 1, 1, 1, 0, 9, 1, 0, 1, 0, 0, 0, 0],
        &[0.0, 0.0],
        bindings.bindings(),
    )
    .unwrap();
    let (mut game, _) = StepGameplay::new_audio_section(
        prepared(),
        config(),
        bindings,
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        authority(),
    )
    .unwrap();
    game.activate_audio().unwrap();
    let mut queue = queue();
    queue.register_source(source).unwrap();
    let mut meta = EventMeta::new(source, host(10_000_000), 0);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(0x57484944),
        code: Some(1),
        timestamp: Some(host(10_000_000)),
    });
    let first = RawHidReportEvent {
        meta,
        report_id: Some(1),
        data: vec![1],
    };
    let mut typed = Vec::new();
    assert_eq!(hid.decode_report(&first, &mut typed).unwrap(), 1);
    queue
        .admit(typed.remove(0), host(20_000_000), None)
        .unwrap();
    game.record_audio_prefix(host(20_000_000)).unwrap();
    assert!(
        queue
            .process_next_solo(&mut game, host(20_000_000), raw(50_000_000))
            .unwrap()
            .is_none()
    );
    pair(&mut game);
    let report = queue
        .process_next_solo(&mut game, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(game.score().hits, 1);
    assert_eq!(report.input.unwrap().meta().native, first.meta.native);
    let mut repeat = first.clone();
    repeat.meta.timestamp = host(15_000_000).timestamp;
    repeat.meta.sequence = 1;
    assert_eq!(hid.decode_report(&repeat, &mut typed).unwrap(), 0);
    let raw_event = PhysicalInputEvent::RawHidReport(repeat.clone());
    queue.admit(raw_event, host(20_000_000), None).unwrap();
    let report = queue
        .process_next_solo(&mut game, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert!(report.bound_inputs.is_empty());
    assert!(report.judge_events.is_empty());
    assert_eq!(game.score().hits, 1);
    let PhysicalInputEvent::RawHidReport(saved) = report.input.unwrap() else {
        panic!("expected raw HID evidence")
    };
    assert_eq!(saved.data, vec![1]);
    assert_eq!(saved.report_id, Some(1));
    assert_eq!(saved.meta.source, source);
    assert_eq!(saved.meta.original_clock_point, Some(host(15_000_000)));
    assert_eq!(queue.pending(), 0);
}

fn local(automatic: bool) -> StepLocalGameplay {
    let rows = if automatic {
        vec![(PlayerId(91), None)]
    } else {
        vec![
            (PlayerId(7), Some(DeviceId(9))),
            (PlayerId(u32::MAX), Some(DeviceId(u64::MAX))),
        ]
    };
    let maps = rows
        .iter()
        .map(|(_, d)| binding(*d, PhysicalControlId::keyboard(7u16)))
        .collect();
    let (mut game, _) = StepLocalGameplay::new_audio_section(
        prepared(),
        config(),
        ResolvedInputPlan::new(rows).unwrap(),
        maps,
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        authority(),
    )
    .unwrap();
    for id in game.players().to_vec() {
        game.configure_capture(id, capture_limits(), 0).unwrap();
    }
    game.activate_audio().unwrap();
    game
}
fn local_pair(game: &mut StepLocalGameplay) {
    game.observe_audio_output(
        1,
        ClockPair {
            source: raw(0),
            target: host(0),
        },
    )
    .unwrap();
    game.observe_audio_output(
        1,
        ClockPair {
            source: raw(40_000_000),
            target: host(20_000_000),
        },
    )
    .unwrap();
}
#[test]
fn fixed_local_original_ids_and_automatic_single_group_consume_actual_registered_sources() {
    let mut game = local(false);
    let mut queue =
        BrowserInputQueue::local(host(0), vec![DeviceId(9), DeviceId(u64::MAX)]).unwrap();
    assert!(queue.register_source(DeviceId(42)).is_err());
    assert_eq!(queue.pending(), 0);
    for device in [9, u64::MAX] {
        queue
            .admit(event(10_000_000, device, 0), host(20_000_000), None)
            .unwrap();
    }
    game.record_audio_prefix(host(20_000_000)).unwrap();
    local_pair(&mut game);
    assert!(
        queue
            .advance_local(&mut game, host(20_000_000), raw(50_000_000))
            .unwrap()
            .is_none()
    );
    for id in [PlayerId(7), PlayerId(u32::MAX)] {
        let Some(InputResult::Processed(rows)) = queue
            .process_next_local(&mut game, host(20_000_000), raw(50_000_000))
            .unwrap()
        else {
            panic!("expected actual local input")
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].player, id);
        assert_eq!(rows[0].report.song_time, Timestamp::from_nanos(20_000_000));
    }
    let rows = queue
        .advance_local(&mut game, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(rows.len(), 2);
    for id in [PlayerId(7), PlayerId(u32::MAX)] {
        assert_eq!(game.score(id).unwrap().hits, 1);
        assert_eq!(game.score(id).unwrap().misses, 0);
    }
    game.fail();
    for (id, source) in [
        (PlayerId(7), DeviceId(9)),
        (PlayerId(u32::MAX), DeviceId(u64::MAX)),
    ] {
        let file = beatkernel::replay::codec::decode_replay(
            &game.take_replay(id).unwrap().unwrap(),
            capture_limits(),
        )
        .unwrap();
        assert_eq!(file.header.normalized_clock, ClockDomainId(33));
        assert_eq!(file.records.len(), 2);
        let beatkernel::replay::ReplayOperation::Input(saved) = &file.records[0].operation else {
            panic!("expected original member input")
        };
        assert_eq!(saved.physical.meta().source, source);
        assert_eq!(
            saved.physical.meta().original_clock_point,
            Some(host(10_000_000))
        );
    }
    let mut auto = local(true);
    let mut dynamic = BrowserInputQueue::solo(host(0)).unwrap();
    dynamic.register_source(DeviceId(u64::MAX)).unwrap();
    dynamic
        .admit(event(10_000_000, u64::MAX, 0), host(20_000_000), None)
        .unwrap();
    auto.record_audio_prefix(host(20_000_000)).unwrap();
    local_pair(&mut auto);
    let Some(InputResult::Processed(rows)) = dynamic
        .process_next_local(&mut auto, host(20_000_000), raw(50_000_000))
        .unwrap()
    else {
        panic!("expected one automatic group owner")
    };
    assert_eq!(rows[0].player, PlayerId(91));
    assert_eq!(auto.score(PlayerId(91)).unwrap().hits, 1);
}

#[test]
fn malformed_admission_and_dispatch_preserve_pending_payload_then_equal_time_input_precedes_deadline()
 {
    let mut game = solo();
    pair(&mut game);
    let mut queue = queue();
    queue.register_source(DeviceId(9)).unwrap();
    let original = event(10_000_000, 9, 7);
    queue
        .admit(original.clone(), host(20_000_000), None)
        .unwrap();
    game.record_audio_prefix(host(20_000_000)).unwrap();
    for bad in [
        Some(Position2 {
            x: f32::NAN,
            y: 0.0,
        }),
        Some(Position2 {
            x: 0.0,
            y: f32::INFINITY,
        }),
    ] {
        assert!(
            queue
                .admit(event(15_000_000, 9, 8), host(20_000_000), bad)
                .is_err()
        );
        assert_eq!(queue.pending(), 1);
    }
    assert!(
        queue
            .admit(event(9_000_000, 9, 8), host(20_000_000), None)
            .is_err()
    );
    assert_eq!(queue.pending(), 1);
    let before = state(&game);
    assert!(
        queue
            .process_next_solo(&mut game, host(20_000_000), host(20_000_000))
            .is_err()
    );
    assert_eq!(state(&game), before);
    assert_eq!(queue.pending(), 1);
    assert!(
        queue
            .advance_solo(&mut game, host(20_000_000), raw(50_000_000))
            .unwrap()
            .is_none()
    );
    let report = queue
        .process_next_solo(&mut game, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(
        report.input.unwrap().meta().sequence,
        original.meta().sequence
    );
    assert_eq!(game.score().hits, 1);
    queue
        .advance_solo(&mut game, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert!(
        queue
            .admit(event(20_000_000, 9, 9), host(21_000_000), None)
            .is_err()
    );
    assert_eq!(queue.pending(), 0);
}

#[test]
fn actual_generated_mixer_cursor_alone_does_not_judge_or_complete_queued_browser_play() {
    let (mut game, bank) = StepGameplay::new_audio_section(
        prepared(),
        config(),
        binding(None, PhysicalControlId::keyboard(7u16)),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        authority(),
    )
    .unwrap();
    game.activate_audio().unwrap();
    let (_producer, consumer) = command_queue(16).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            AudioFormat::new(1000, 1).unwrap(),
            ClockDomainId(22),
            raw(0).timestamp,
            AudioLimits::new(16, 2, 8, 1000, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let rendered = mixer.render(&mut [0.0; 1000]).unwrap();
    assert_eq!(rendered.frames, 1000);
    let mut queue = queue();
    queue.register_source(DeviceId(9)).unwrap();
    queue
        .admit(event(10_000_000, 9, 0), host(20_000_000), None)
        .unwrap();
    game.record_audio_prefix(host(20_000_000)).unwrap();
    game.feed_audio(1000, 8).unwrap();
    assert!(
        queue
            .advance_solo(&mut game, host(20_000_000), raw(1_000_000_000))
            .unwrap()
            .is_none()
    );
    assert!(!game.observe_completion(Some(rendered), None).unwrap());
    assert_eq!(game.song_time(), Timestamp::ZERO);
    assert_eq!(game.score().misses, 0);
    assert_eq!(game.score().hits, 0);
    assert_eq!(queue.pending(), 1);
    assert!(game.completed_result().is_none());
}

#[test]
fn startup_input_before_first_observation_uses_explicit_backward_permission_once() {
    let backward = ExtrapolationPolicy::Bounded {
        before: Duration::from_nanos(70_000_000_000),
        after: Duration::ZERO,
    };
    let selected = AudioAuthority::new(
        AudioAuthorityConfig {
            history_capacity: 8,
            max_observation_age: Duration::from_nanos(1_000_000_000),
            input_extrapolation: backward,
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: 1,
            stream_origin: raw(0),
            logical_origin: logical(0),
            host_domain: ClockDomainId(11),
        },
    )
    .unwrap();
    let (mut game, _) = StepGameplay::new_audio_section(
        prepared(),
        config(),
        binding(None, PhysicalControlId::keyboard(7u16)),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        selected,
    )
    .unwrap();
    game.configure_capture(capture_limits(), 0).unwrap();
    game.activate_audio().unwrap();
    let mut queue = BrowserInputQueue::solo(host(0)).unwrap();
    queue.register_source(DeviceId(9)).unwrap();
    assert!(queue.admit(event(-1, 9, 0), host(0), None).is_err());
    assert_eq!(queue.pending(), 0);
    let original = event(10_000_000, 9, 0);
    queue
        .admit(original.clone(), host(40_000_000), None)
        .unwrap();
    game.record_audio_prefix(host(40_000_000)).unwrap();
    assert!(
        queue
            .process_next_solo(&mut game, host(40_000_000), raw(90_000_000))
            .unwrap()
            .is_none()
    );
    game.observe_audio_output(
        1,
        ClockPair {
            source: raw(40_000_000),
            target: host(20_000_000),
        },
    )
    .unwrap();
    assert!(
        queue
            .process_next_solo(&mut game, host(40_000_000), raw(90_000_000))
            .unwrap()
            .is_none()
    );
    assert_eq!(queue.pending(), 1);
    assert_eq!(game.score().hits, 0);
    assert_eq!(game.score().misses, 0);
    game.observe_audio_output(
        1,
        ClockPair {
            source: raw(80_000_000),
            target: host(40_000_000),
        },
    )
    .unwrap();
    assert_eq!(
        game.audio_authority().unwrap().config().input_extrapolation,
        backward
    );
    assert_eq!(
        game.audio_authority().unwrap().config().max_input_ahead,
        Duration::ZERO
    );
    let report = queue
        .process_next_solo(&mut game, host(40_000_000), raw(90_000_000))
        .unwrap()
        .unwrap();
    // The measured slope is 2:1. The original HOST10ms therefore denotes
    // logical+20ms even though both real observations occurred later.
    assert_eq!(report.song_time, Timestamp::from_nanos(20_000_000));
    assert_eq!(report.audio_at, raw(90_000_000));
    assert_eq!(report.input_mapping_quality, ClockMappingQuality::Unknown);
    let actual = report.input.unwrap();
    assert_eq!(actual.meta().original_clock_point, Some(host(10_000_000)));
    assert_eq!(actual.meta().timestamp, logical(20_000_000).timestamp);
    assert_eq!(actual.meta().native, original.meta().native);
    assert_eq!(actual.meta().source, DeviceId(9));
    assert_eq!(actual.meta().sequence, 0);
    assert_eq!(game.score().hits, 1);
    assert_eq!(queue.pending(), 0);
    assert!(
        queue
            .process_next_solo(&mut game, host(40_000_000), raw(90_000_000))
            .unwrap()
            .is_none()
    );
    assert_eq!(game.score().hits, 1);
    game.fail();
    let file = beatkernel::replay::codec::decode_replay(
        &game.take_replay().unwrap().unwrap(),
        capture_limits(),
    )
    .unwrap();
    assert_eq!(file.header.normalized_clock, ClockDomainId(33));
    assert_eq!(file.records.len(), 1);
    let beatkernel::replay::ReplayOperation::Input(saved) = &file.records[0].operation else {
        panic!("expected startup input captured")
    };
    assert_eq!(
        saved.physical.meta().original_clock_point,
        Some(host(10_000_000))
    );
    assert_eq!(file.records[0].song_time, Timestamp::from_nanos(20_000_000));
}

#[test]
fn consecutive_output_admission_rejects_chronology_before_pending_input_or_bgm_effects() {
    for fault in ["counters", "extent", "presentation"] {
        let (mut game, bank) = StepGameplay::new_audio_section(
            prepared(),
            config(),
            binding(None, PhysicalControlId::keyboard(7u16)),
            Timestamp::ZERO,
            None,
            BmsInputMode::ButtonOnly,
            authority(),
        )
        .unwrap();
        game.configure_capture(capture_limits(), 0).unwrap();
        game.activate_audio().unwrap();
        let mut queue = BrowserInputQueue::solo(host(0)).unwrap();
        queue.register_source(DeviceId(9)).unwrap();
        queue
            .admit(event(10_000_000, 9, 0), host(20_000_000), None)
            .unwrap();
        game.record_audio_prefix(host(20_000_000)).unwrap();
        let (mut producer, consumer) = command_queue(16).unwrap();
        producer
            .try_push(AudioCommand::Play {
                sample: SampleId(1),
                voice: VoiceId(1),
                at: raw(0).timestamp,
                gain: 1.0,
            })
            .unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                AudioFormat::new(1000, 1).unwrap(),
                ClockDomainId(22),
                raw(0).timestamp,
                AudioLimits::new(16, 2, 8, 20, 8).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        let first = mixer.render(&mut [0.0; 20]).unwrap();
        let mut second = mixer.render(&mut [0.0; 20]).unwrap();
        assert_eq!(first.counters.commands_consumed, 1);
        assert_eq!(first.counters.commands_applied, 1);
        assert_eq!(second.start_frame, 20);
        assert_eq!(second.counters.rendered_frames, 40);
        let before_bgm = format!("{:?}", game.bgm_report());
        let before_authority = format!("{:?}", game.audio_authority());
        game.admit_output_evidence(Some(first), Some(raw(20_000_000)))
            .unwrap();
        assert!(!game.failed());
        assert!(game.completed_result().is_none());
        assert_eq!(queue.pending(), 1);
        assert_eq!(game.song_time(), Timestamp::ZERO);
        assert_eq!(game.score().hits, 0);
        assert_eq!(game.score().misses, 0);
        assert_eq!(format!("{:?}", game.bgm_report()), before_bgm);
        assert_eq!(format!("{:?}", game.audio_authority()), before_authority);
        let presentation = if fault == "presentation" {
            raw(19_000_000)
        } else {
            raw(40_000_000)
        };
        if fault == "counters" {
            second.counters.commands_consumed = 0;
            second.counters.commands_applied = 0;
        }
        if fault == "extent" {
            second.start_frame = 19;
            second.playback_start_frame = 19;
            second.counters.rendered_frames = 39;
        }
        assert!(
            game.admit_output_evidence(Some(second), Some(presentation))
                .is_err(),
            "{fault}"
        );
        assert!(game.failed());
        assert!(game.completed_result().is_none());
        assert_eq!(queue.pending(), 1);
        assert_eq!(game.song_time(), Timestamp::ZERO);
        assert_eq!(game.score().hits, 0);
        assert_eq!(game.score().misses, 0);
        assert_eq!(format!("{:?}", game.bgm_report()), before_bgm);
        assert_eq!(format!("{:?}", game.audio_authority()), before_authority);
        let file = beatkernel::replay::codec::decode_replay(
            &game.take_replay().unwrap().unwrap(),
            capture_limits(),
        )
        .unwrap();
        assert!(file.records.is_empty());
    }
}

#[test]
fn local_output_admission_delegates_chronology_without_completing_or_dispatching_members() {
    let mut game = local(false);
    let mut queue =
        BrowserInputQueue::local(host(0), vec![DeviceId(9), DeviceId(u64::MAX)]).unwrap();
    queue
        .admit(event(10_000_000, 9, 0), host(20_000_000), None)
        .unwrap();
    game.record_audio_prefix(host(20_000_000)).unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(1),
            at: raw(0).timestamp,
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            AudioFormat::new(1000, 1).unwrap(),
            ClockDomainId(22),
            raw(0).timestamp,
            AudioLimits::new(16, 2, 8, 20, 8).unwrap(),
        ),
        prepared().bank,
        consumer,
    )
    .unwrap();
    let first = mixer.render(&mut [0.0; 20]).unwrap();
    let mut second = mixer.render(&mut [0.0; 20]).unwrap();
    let before_bgm = format!("{:?}", game.bgm_report());
    let before_authority = format!("{:?}", game.audio_authority());
    game.admit_output_evidence(Some(first), Some(raw(20_000_000)))
        .unwrap();
    assert!(!game.failed());
    second.counters.commands_consumed = 0;
    second.counters.commands_applied = 0;
    assert!(
        game.admit_output_evidence(Some(second), Some(raw(40_000_000)))
            .is_err()
    );
    assert!(game.failed());
    assert_eq!(queue.pending(), 1);
    assert_eq!(format!("{:?}", game.bgm_report()), before_bgm);
    assert_eq!(format!("{:?}", game.audio_authority()), before_authority);
    for id in [PlayerId(7), PlayerId(u32::MAX)] {
        assert_eq!(game.score(id).unwrap().hits, 0);
        assert_eq!(game.score(id).unwrap().misses, 0);
        assert!(game.completed_result(id).unwrap().is_none());
        let file = beatkernel::replay::codec::decode_replay(
            &game.take_replay(id).unwrap().unwrap(),
            capture_limits(),
        )
        .unwrap();
        assert!(file.records.is_empty());
    }
}
