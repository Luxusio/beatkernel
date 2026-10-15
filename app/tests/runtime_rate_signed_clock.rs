//! Actual pre-arm clock associations qualify real retained Worklet boundaries.
//! Supplied presentation pairs are DI evidence, not physical-device acceptance.
#[path = "support/replay_speed.rs"]
mod fixture;
use beatkernel::{
    audio::*,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    replay::{codec::decode_replay, ReplayOperation},
    time::{
        presentation::ObservationAdmission, ClockDomainId, ClockPair, ClockPoint, Duration,
        ExtrapolationPolicy, Timestamp,
    },
    transport::Rate,
};
use beatkernel_bms::BmsInputMode;
use beatkernel_bms_runtime::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
        AudioAuthorityError,
    },
    local_input::InputMerger,
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    native_output_coordinate::NativeOutputCoordinate,
    practice_control::{PracticeTransactionCapability, PracticeTransactionRequest},
    replay_speed_policy::split_schedule,
    section_start::{prepare_retained_at, PreparedRetainedSection},
    step_gameplay::{
        StepGameplay, StepGameplayConfig, StepLocalGameplay, StepRemoteHistorySnapshot,
        StepRetainedAdmissionKind, StepRetainedOptions, StepRetainedSetup,
    },
    worklet_audio::{
        WorkletAudio, WorkletAudioBuilder, WorkletAudioConfig, WorkletRetainedConfig,
        WorkletRetainedEnvelope, WorkletRetainedEvent,
    },
};
const LEFT: i64 = 1_530_920_000;
const RIGHT: i64 = 1_537_685_000;
const FIRST: i64 = 1_536_224_814;
const LATER: i64 = 1_539_225_245;
const PROGRAM: u64 = (1 << 53) + 21;
const CONTEXT: u64 = (1 << 53) + 211;
const SEED: u64 = 31337;
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn host(n: i64) -> ClockPoint {
    point(11, n)
}
fn raw(n: i64) -> ClockPoint {
    point(22, n)
}
fn pair(output: i64, at: i64) -> ClockPair {
    ClockPair {
        source: raw(output),
        target: host(at),
    }
}
fn half() -> Rate {
    Rate::new(1, 2).unwrap()
}
fn authority(basis: Option<AudioAuthorityCoordinateBasis>) -> AudioAuthority {
    let config = AudioAuthorityConfig {
        history_capacity: 16,
        max_observation_age: Duration::from_nanos(100_000_000),
        input_extrapolation: ExtrapolationPolicy::Forbid,
        max_input_ahead: Duration::ZERO,
    };
    let epoch = AudioAuthorityEpoch {
        id: 1,
        stream_origin: raw(0),
        logical_origin: point(33, 5_000_000_000),
        host_domain: host(0).domain,
    };
    match basis {
        Some(basis) => AudioAuthority::new_with_coordinates(config, epoch, basis).unwrap(),
        None => AudioAuthority::new(config, epoch).unwrap(),
    }
}
fn signed_authority() -> AudioAuthority {
    authority(Some(AudioAuthorityCoordinateBasis::SourceTimestamp(
        OutputFrameBasis::new(raw(0), 1000, 0).unwrap(),
    )))
}
fn zero_query() -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::new(0, 0, 1).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn merger(devices: Vec<DeviceId>) -> InputMerger {
    InputMerger::new(host(0).domain, host(LEFT), devices, 16).unwrap()
}

#[test]
fn original_signed_bracket_requires_the_exact_first_host_and_real_acquired_prefix() {
    let mut owner = signed_authority();
    let inputs = merger(vec![DeviceId(1)]);
    assert_eq!(
        owner.observe_source_timestamp(1, pair(-5_303_245, LEFT)),
        Ok(ObservationAdmission::Retained)
    );
    assert_eq!(owner.latest_observation(), Some(pair(-5_303_245, LEFT)));
    owner.record_acquired_prefix(host(LEFT)).unwrap();
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&zero_query(), host(LEFT))
            .unwrap(),
        None
    );
    assert_eq!(
        owner.observe_source_timestamp(1, pair(1_459_755, RIGHT)),
        Ok(ObservationAdmission::Retained)
    );
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&zero_query(), host(RIGHT))
            .unwrap(),
        Some(host(FIRST))
    );
    owner.record_acquired_prefix(host(FIRST - 1)).unwrap();
    assert!(owner
        .prepare_coordinate_control_cutoff(1, zero_query(), host(FIRST), host(RIGHT), &inputs)
        .unwrap()
        .is_none());
    owner.record_acquired_prefix(host(FIRST)).unwrap();
    let token = owner
        .prepare_coordinate_control_cutoff(1, zero_query(), host(FIRST), host(RIGHT), &inputs)
        .unwrap()
        .unwrap();
    owner
        .commit_coordinate_control_cutoff(token, host(RIGHT), &inputs)
        .unwrap();
    assert_eq!(
        owner
            .committed_coordinate_operation()
            .unwrap()
            .compare_interpolated_time(zero_query()),
        std::cmp::Ordering::Equal
    );
    assert_eq!(owner.latest_observation(), Some(pair(1_459_755, RIGHT)));
}

#[test]
fn signed_observation_preserves_epoch_domains_order_and_native_legacy_mode_guards() {
    let mut owner = signed_authority();
    owner
        .observe_source_timestamp(1, pair(-5_303_245, LEFT))
        .unwrap();
    for (epoch, observation, expected) in [
        (
            2,
            pair(-5_303_244, LEFT + 1),
            AudioAuthorityError::WrongEpoch,
        ),
        (
            1,
            ClockPair {
                source: point(99, -5_303_244),
                target: host(LEFT + 1),
            },
            AudioAuthorityError::DomainMismatch,
        ),
        (
            1,
            ClockPair {
                source: raw(-5_303_244),
                target: point(99, LEFT + 1),
            },
            AudioAuthorityError::DomainMismatch,
        ),
        (
            1,
            pair(-5_303_246, LEFT + 1),
            AudioAuthorityError::ObservationRegression,
        ),
        (
            1,
            pair(-5_303_244, LEFT - 1),
            AudioAuthorityError::ObservationRegression,
        ),
    ] {
        assert_eq!(
            owner.observe_source_timestamp(epoch, observation),
            Err(expected)
        );
        assert_eq!(owner.latest_observation(), Some(pair(-5_303_245, LEFT)));
        assert_eq!(owner.history_len(), 1);
    }
    assert_eq!(
        owner.observe_source_timestamp(1, pair(-5_303_245, LEFT + 1)),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(
        owner.observe_coordinate(
            1,
            pair(0, RIGHT),
            NativeOutputCoordinate::StreamFrame { frame: 0 }
        ),
        Err(AudioAuthorityError::CoordinateMismatch)
    );
    let mut legacy = authority(None);
    assert_eq!(
        legacy.observe_source_timestamp(1, pair(-1, LEFT)),
        Err(AudioAuthorityError::CoordinateModeMismatch)
    );
    let mut native = authority(Some(AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(raw(0), 1000, 0).unwrap(),
        frequency: 1_000_000_000,
    }));
    assert_eq!(
        native.observe_source_timestamp(1, pair(-1, LEFT)),
        Err(AudioAuthorityError::CoordinateModeMismatch)
    );
    assert_eq!(
        native.observe_coordinate(
            1,
            pair(0, LEFT),
            NativeOutputCoordinate::Counter {
                position: 0,
                frequency: 1_000_000_000
            }
        ),
        Ok(ObservationAdmission::Retained)
    );
    let mut missing_prearm = signed_authority();
    missing_prearm
        .observe_source_timestamp(1, pair(1_459_755, RIGHT))
        .unwrap();
    missing_prearm
        .observe_source_timestamp(1, pair(3_000_000, LATER))
        .unwrap();
    assert_eq!(
        missing_prearm.presented_coordinate_boundary_host(&zero_query(), host(LATER)),
        Err(AudioAuthorityError::HistoryExpired)
    );
}

fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: host(LEFT),
        output_origin: raw(0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 32,
        bgm_pending: 8,
        bgm_lookahead: Duration::from_nanos(100_000_000),
        telemetry_capacity: 16,
    }
}
fn options() -> StepRetainedOptions {
    StepRetainedOptions {
        worklet: WorkletRetainedConfig {
            program_id: PROGRAM,
            initial_region: PracticeRegion::new(ts(0), ts(20_000_000), false).unwrap(),
            initial_rate: half(),
            output_preroll: Duration::ZERO,
            open_end: false,
            limits: PracticeLimits::new(8, 4, 128, 8, 16).unwrap(),
            source_history_capacity: 16,
        },
        chart_seed: SEED,
        identity_limits: fixture::limits(),
    }
}
fn prepared() -> PreparedRetainedSection {
    let pcm: Vec<_> = (0..32).map(|i| i as f32 / 32.0).collect();
    prepare_retained_at(
        fixture::prepared(
            "#BPM 60000\n#WAV01 key.wav\n#00011:01\n#00001:01\n",
            1000,
            &[(1, &pcm)],
        ),
        ts(0),
    )
    .unwrap()
}
fn bindings(device: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device,
        physical: PhysicalControlId::keyboard(7),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn output(bank: SampleBank, setup: &StepRetainedSetup) -> WorkletAudio {
    let mut audio = output_before_render(bank, setup, 64);
    audio.render(CONTEXT, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.0, 0.015625]);
    audio
}
fn output_before_render(
    bank: SampleBank,
    setup: &StepRetainedSetup,
    total_voices: usize,
) -> WorkletAudio {
    let mut builder = WorkletAudioBuilder::new(WorkletAudioConfig {
        format: bank.format(),
        pcm_limits: fixture::pcm_limits(),
        audio_limits: AudioLimits::new(32, total_voices, 32, 32, 32).unwrap(),
    })
    .unwrap();
    for (id, pcm) in bank.into_samples() {
        builder
            .insert_sample(id, pcm.format(), pcm.samples().to_vec())
            .unwrap();
    }
    let mut audio = builder
        .finish_retained(setup.cues.clone(), setup.config)
        .unwrap();
    audio.arm(CONTEXT, CONTEXT).unwrap();
    audio
}
fn history(audio: &WorkletAudio) -> StepRemoteHistorySnapshot {
    let h = audio.source_playback_history().unwrap();
    StepRemoteHistorySnapshot {
        program_id: PROGRAM,
        context_start: CONTEXT,
        source_hz: h.source_rate(),
        initial_physical: h.initial_physical_frame(),
        initial_playback: h.initial_playback_frame(),
        retained_start: h.retained_start_frame(),
        pulled_end: h.pulled_end_frame(),
        retirement_revision: h.retirement_revision(),
        spans: (0..h.len()).map(|i| h.span(i).unwrap()).collect(),
    }
}
fn input(device: u64, at: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), host(at), 0),
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    })
}
fn request(cap: PracticeTransactionCapability) -> PracticeTransactionRequest {
    PracticeTransactionRequest::Rate {
        id: 1,
        generation: cap.practice.generation,
        expected: cap.rate,
        requested_rate: Rate::new(2, 1).unwrap(),
    }
}
fn assert_rate(event: WorkletRetainedEnvelope) {
    assert!(
        matches!(event.event, WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
        event: PracticeEvent::Rate(e), .. }) if e.physical_frame == 2 && e.active_elapsed_nanos == 2_000_000
            && e.outcome == PracticeRateOutcome::Applied)
    );
}

#[test]
fn solo_started_waits_for_actual_signed_bracket_then_original_input_and_positive_rate_commit() {
    let (mut game, bank) = StepGameplay::new_retained_audio_section(
        prepared(),
        config(),
        bindings(DeviceSelector::Any),
        Some(ts(20_000_000)),
        BmsInputMode::ButtonOnly,
        signed_authority(),
        None,
        options(),
    )
    .unwrap();
    game.configure_capture(fixture::limits(), SEED).unwrap();
    let mut audio = output(bank, game.retained_setup().unwrap());
    let mut inputs = merger(vec![DeviceId(1)]);
    game.activate_audio().unwrap();
    game.observe_retained_history(history(&audio)).unwrap();
    let started = audio.poll_retained_event().unwrap().unwrap();
    game.retain_output_event(started).unwrap();
    game.observe_audio_source_timestamp(1, pair(-5_303_245, LEFT))
        .unwrap();
    game.record_audio_prefix(host(LEFT)).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(LEFT), raw(2_000_000))
            .unwrap(),
        None
    );
    assert!(game.retained_capability().is_none());
    assert!(game.acknowledge_retained_event(started.serial).is_err());
    assert_eq!(audio.poll_retained_event().unwrap(), Some(started));
    game.observe_audio_source_timestamp(1, pair(1_459_755, RIGHT))
        .unwrap();
    game.record_audio_prefix(host(FIRST - 1)).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(RIGHT), raw(2_000_000))
            .unwrap(),
        None
    );
    assert!(game.retained_capability().is_none());
    game.record_audio_prefix(host(FIRST)).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(RIGHT), raw(2_000_000))
            .unwrap(),
        Some(started.serial)
    );
    audio.acknowledge_retained_event(started.serial).unwrap();
    game.acknowledge_retained_event(started.serial).unwrap();
    let cap = game.retained_capability().unwrap();
    inputs.admit(input(1, FIRST), host(RIGHT)).unwrap();
    game.record_audio_prefix(host(RIGHT)).unwrap();
    let report = game
        .process_next_audio_input(&mut inputs, host(RIGHT), raw(2_000_000), None)
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, ts(0));
    assert_eq!(
        report.input.as_ref().unwrap().meta().original_clock_point,
        Some(host(FIRST))
    );
    assert_eq!(game.score().hits, 1);
    let prepared = game
        .prepare_retained_transaction(request(cap), false)
        .unwrap();
    audio
        .try_set_rate_next(
            1,
            cap.practice.generation,
            cap.rate.revision,
            prepared.preparation.unwrap(),
        )
        .unwrap();
    game.acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 1, Ok(()))
        .unwrap();
    audio.render(CONTEXT + 2, 2).unwrap();
    game.observe_retained_history(history(&audio)).unwrap();
    game.observe_audio_source_timestamp(1, pair(3_000_000, LATER))
        .unwrap();
    let rate = audio.poll_retained_event().unwrap().unwrap();
    assert_rate(rate);
    game.retain_output_event(rate).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(LATER), raw(4_000_000))
            .unwrap(),
        None
    );
    game.record_audio_prefix(host(LATER)).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(LATER), raw(4_000_000))
            .unwrap(),
        Some(rate.serial)
    );
    audio.acknowledge_retained_event(rate.serial).unwrap();
    game.acknowledge_retained_event(rate.serial).unwrap();
    let effective = game.effective_header(PlayerId(1)).unwrap().clone();
    assert_eq!(
        split_schedule(&effective.options).unwrap().1.cuts()[0].whole(),
        1_000_000
    );
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), fixture::limits()).unwrap();
    assert_eq!(file.header, effective);
    assert!(file
        .records
        .iter()
        .any(|r| matches!(&r.operation, ReplayOperation::Input(saved)
        if saved.physical.meta().original_clock_point == Some(host(FIRST)))));
}

#[test]
fn local_started_and_shared_rate_require_the_same_genuine_signed_prefix_for_all_members() {
    let members = [(PlayerId(3), DeviceId(1)), (PlayerId(7), DeviceId(2))];
    let (mut game, bank) = StepLocalGameplay::new_retained_audio_section(
        prepared(),
        config(),
        ResolvedInputPlan::new(members.map(|(id, device)| (id, Some(device))).to_vec()).unwrap(),
        vec![
            bindings(DeviceSelector::Exact(members[0].1)),
            bindings(DeviceSelector::Exact(members[1].1)),
        ],
        Some(ts(20_000_000)),
        BmsInputMode::ButtonOnly,
        signed_authority(),
        None,
        options(),
    )
    .unwrap();
    for (id, _) in members {
        game.configure_capture(id, fixture::limits(), SEED).unwrap();
    }
    let mut audio = output(bank, game.retained_setup().unwrap());
    let mut inputs = merger(members.map(|(_, device)| device).to_vec());
    game.activate_audio().unwrap();
    game.observe_retained_history(history(&audio)).unwrap();
    let started = audio.poll_retained_event().unwrap().unwrap();
    game.retain_output_event(started).unwrap();
    game.observe_audio_source_timestamp(1, pair(-5_303_245, LEFT))
        .unwrap();
    game.record_audio_prefix(host(LEFT)).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(LEFT), raw(2_000_000))
            .unwrap(),
        None
    );
    assert!(game.retained_capability().is_none());
    assert!(game.acknowledge_retained_event(started.serial).is_err());
    game.observe_audio_source_timestamp(1, pair(1_459_755, RIGHT))
        .unwrap();
    game.record_audio_prefix(host(FIRST - 1)).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(RIGHT), raw(2_000_000))
            .unwrap(),
        None
    );
    game.record_audio_prefix(host(FIRST)).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(RIGHT), raw(2_000_000))
            .unwrap(),
        Some(started.serial)
    );
    audio.acknowledge_retained_event(started.serial).unwrap();
    game.acknowledge_retained_event(started.serial).unwrap();
    game.record_audio_prefix(host(RIGHT)).unwrap();
    for (id, device) in members {
        inputs.admit(input(device.0, FIRST), host(RIGHT)).unwrap();
        let Some(InputResult::Processed(reports)) = game
            .process_next_audio_input(&mut inputs, host(RIGHT), raw(2_000_000), None)
            .unwrap()
        else {
            panic!("actual local input missing");
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, id);
        assert_eq!(reports[0].report.song_time, ts(0));
        assert_eq!(
            reports[0]
                .report
                .input
                .as_ref()
                .unwrap()
                .meta()
                .original_clock_point,
            Some(host(FIRST))
        );
    }
    let cap = game.retained_capability().unwrap();
    let prepared = game
        .prepare_retained_transaction(request(cap), false)
        .unwrap();
    audio
        .try_set_rate_next(
            1,
            cap.practice.generation,
            cap.rate.revision,
            prepared.preparation.unwrap(),
        )
        .unwrap();
    game.acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 1, Ok(()))
        .unwrap();
    audio.render(CONTEXT + 2, 2).unwrap();
    game.observe_retained_history(history(&audio)).unwrap();
    game.observe_audio_source_timestamp(1, pair(3_000_000, LATER))
        .unwrap();
    let rate = audio.poll_retained_event().unwrap().unwrap();
    assert_rate(rate);
    game.retain_output_event(rate).unwrap();
    game.record_audio_prefix(host(LATER)).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(LATER), raw(4_000_000))
            .unwrap(),
        Some(rate.serial)
    );
    audio.acknowledge_retained_event(rate.serial).unwrap();
    game.acknowledge_retained_event(rate.serial).unwrap();
    for (id, _) in members {
        assert_eq!(game.score(id).unwrap().hits, 1);
        assert_eq!(
            split_schedule(&game.effective_header(id).unwrap().options)
                .unwrap()
                .1
                .cuts()[0]
                .whole(),
            1_000_000
        );
    }
    game.fail();
    for (id, _) in members {
        let file =
            decode_replay(&game.take_replay(id).unwrap().unwrap(), fixture::limits()).unwrap();
        assert!(file
            .records
            .iter()
            .any(|r| matches!(&r.operation, ReplayOperation::Input(saved)
            if saved.physical.meta().original_clock_point == Some(host(FIRST)))));
    }
}

fn budget_solo(prepared: PreparedRetainedSection) -> (StepGameplay, SampleBank) {
    StepGameplay::new_retained_audio_section(
        prepared,
        config(),
        bindings(DeviceSelector::Any),
        Some(ts(20_000_000)),
        BmsInputMode::ButtonOnly,
        signed_authority(),
        None,
        options(),
    )
    .unwrap()
}
fn start_budget_solo(game: &mut StepGameplay, audio: &mut WorkletAudio, inputs: &mut InputMerger) {
    game.activate_audio().unwrap();
    game.observe_retained_history(history(audio)).unwrap();
    game.observe_audio_source_timestamp(1, pair(-5_303_245, LEFT))
        .unwrap();
    game.observe_audio_source_timestamp(1, pair(1_459_755, RIGHT))
        .unwrap();
    game.record_audio_prefix(host(RIGHT)).unwrap();
    let started = audio.poll_retained_event().unwrap().unwrap();
    game.retain_output_event(started).unwrap();
    assert_eq!(
        game.service_retained_event(inputs, host(RIGHT), raw(2_000_000))
            .unwrap(),
        Some(started.serial)
    );
    audio.acknowledge_retained_event(started.serial).unwrap();
    game.acknowledge_retained_event(started.serial).unwrap();
}
fn cold_fields(setup: &StepRetainedSetup) -> [usize; 6] {
    let limits = setup.config.limits;
    [
        limits.max_cues,
        limits.max_overlap,
        limits.max_events_per_render,
        limits.control_capacity,
        limits.receipt_capacity,
        setup.config.source_history_capacity,
    ]
}
fn budget_prepared(chart: &str, pcm: &[f32]) -> PreparedRetainedSection {
    prepare_retained_at(fixture::prepared(chart, 1000, &[(1, pcm)]), ts(0)).unwrap()
}

#[test]
fn total_two_reserves_one_bgm_voice_and_real_judged_key_continues_across_rate_cut() {
    let (mut game, bank) = budget_solo(prepared());
    game.configure_retained_audio_limits(2, 128, 8, 16, 16)
        .unwrap();
    assert_eq!(game.retained_setup().unwrap().config.limits.max_overlap, 1);
    game.configure_capture(fixture::limits(), SEED).unwrap();
    let mut audio = output_before_render(bank, game.retained_setup().unwrap(), 2);
    audio.render(CONTEXT, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.0, 0.015625]);
    let mut inputs = merger(vec![DeviceId(1)]);
    start_budget_solo(&mut game, &mut audio, &mut inputs);
    inputs.admit(input(1, FIRST), host(RIGHT)).unwrap();
    let report = game
        .process_next_audio_input(&mut inputs, host(RIGHT), raw(2_000_000), None)
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, ts(0));
    assert_eq!(game.score().hits, 1);
    let batch = game.take_commands(32).unwrap().unwrap();
    assert_eq!(batch.commands.len(), 1);
    assert!(
        matches!(batch.commands[0], AudioCommand::Play { sample: SampleId(1), at, .. } if at == ts(2_000_000))
    );
    for &command in &batch.commands {
        audio.enqueue(command).unwrap();
    }
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    audio.render(CONTEXT + 2, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.03125, 0.0625]);
    assert_eq!(audio.report().unwrap().counters.voice_full, 0);
    assert_eq!(audio.report().unwrap().counters.commands_applied, 1);
    let cap = game.retained_capability().unwrap();
    let prepared = game
        .prepare_retained_transaction(request(cap), false)
        .unwrap();
    audio
        .try_set_rate_next(
            1,
            cap.practice.generation,
            cap.rate.revision,
            prepared.preparation.unwrap(),
        )
        .unwrap();
    game.acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 1, Ok(()))
        .unwrap();
    audio.render(CONTEXT + 4, 4).unwrap();
    assert_eq!(&audio.output()[..4], &[0.09375, 0.21875, 0.34375, 0.46875]);
    assert_eq!(audio.report().unwrap().counters.voice_full, 0);
    game.observe_retained_history(history(&audio)).unwrap();
    game.observe_audio_source_timestamp(1, pair(7_000_000, LATER + 4_000_000))
        .unwrap();
    game.record_audio_prefix(host(LATER + 4_000_000)).unwrap();
    let event = audio.poll_retained_event().unwrap().unwrap();
    assert!(
        matches!(event.event, WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
        event: PracticeEvent::Rate(rate), .. }) if rate.physical_frame == 4 && rate.active_elapsed_nanos == 4_000_000
            && rate.outcome == PracticeRateOutcome::Applied)
    );
    game.retain_output_event(event).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(LATER + 4_000_000), raw(8_000_000))
            .unwrap(),
        Some(event.serial)
    );
    audio.acknowledge_retained_event(event.serial).unwrap();
    game.acknowledge_retained_event(event.serial).unwrap();
    assert_eq!(game.score().hits, 1);
    assert_eq!(
        game.retained_capability().unwrap().practice.generation,
        cap.practice.generation
    );
    let effective = game.effective_header(PlayerId(1)).unwrap().clone();
    assert_eq!(
        split_schedule(&effective.options).unwrap().1.cuts()[0].whole(),
        2_000_000
    );
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), fixture::limits()).unwrap();
    assert_eq!(file.header, effective);
    assert!(file
        .records
        .iter()
        .any(|r| matches!(&r.operation, ReplayOperation::Input(saved)
        if saved.physical.meta().original_clock_point == Some(host(FIRST)))));
}

#[test]
fn local_members_share_the_entire_remaining_gameplay_pool_alongside_one_bgm_voice() {
    let members = [(PlayerId(3), DeviceId(1)), (PlayerId(7), DeviceId(2))];
    let (mut game, bank) = StepLocalGameplay::new_retained_audio_section(
        prepared(),
        config(),
        ResolvedInputPlan::new(members.map(|(id, device)| (id, Some(device))).to_vec()).unwrap(),
        vec![
            bindings(DeviceSelector::Exact(members[0].1)),
            bindings(DeviceSelector::Exact(members[1].1)),
        ],
        Some(ts(20_000_000)),
        BmsInputMode::ButtonOnly,
        signed_authority(),
        None,
        options(),
    )
    .unwrap();
    // One shared gameplay slot is a valid budget for two members; it is not a
    // guarantee that two concurrent voices fit. Use two slots for this journey.
    game.configure_retained_audio_limits(2, 128, 8, 16, 16)
        .unwrap();
    assert_eq!(game.retained_setup().unwrap().config.limits.max_overlap, 1);
    game.configure_retained_audio_limits(3, 128, 8, 16, 16)
        .unwrap();
    for (id, _) in members {
        game.configure_capture(id, fixture::limits(), SEED).unwrap();
    }
    let mut audio = output_before_render(bank, game.retained_setup().unwrap(), 3);
    audio.render(CONTEXT, 2).unwrap();
    game.activate_audio().unwrap();
    game.observe_retained_history(history(&audio)).unwrap();
    game.observe_audio_source_timestamp(1, pair(-5_303_245, LEFT))
        .unwrap();
    game.observe_audio_source_timestamp(1, pair(1_459_755, RIGHT))
        .unwrap();
    game.record_audio_prefix(host(RIGHT)).unwrap();
    let mut inputs = merger(members.map(|(_, device)| device).to_vec());
    let started = audio.poll_retained_event().unwrap().unwrap();
    game.retain_output_event(started).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(RIGHT), raw(2_000_000))
            .unwrap(),
        Some(started.serial)
    );
    audio.acknowledge_retained_event(started.serial).unwrap();
    game.acknowledge_retained_event(started.serial).unwrap();
    for (id, device) in members {
        inputs.admit(input(device.0, FIRST), host(RIGHT)).unwrap();
        let Some(InputResult::Processed(reports)) = game
            .process_next_audio_input(&mut inputs, host(RIGHT), raw(2_000_000), None)
            .unwrap()
        else {
            panic!("actual local key input missing");
        };
        assert_eq!(reports[0].player, id);
        assert_eq!(reports[0].report.song_time, ts(0));
    }
    let batch = game.take_commands(32).unwrap().unwrap();
    assert_eq!(batch.commands.len(), 2);
    for &command in &batch.commands {
        audio.enqueue(command).unwrap();
    }
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    audio.render(CONTEXT + 2, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.03125, 0.078125]);
    assert_eq!(audio.report().unwrap().counters.voice_full, 0);
    assert_eq!(audio.report().unwrap().counters.commands_applied, 2);
    let cap = game.retained_capability().unwrap();
    let prepared = game
        .prepare_retained_transaction(request(cap), false)
        .unwrap();
    audio
        .try_set_rate_next(
            1,
            cap.practice.generation,
            cap.rate.revision,
            prepared.preparation.unwrap(),
        )
        .unwrap();
    game.acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 1, Ok(()))
        .unwrap();
    audio.render(CONTEXT + 4, 4).unwrap();
    assert_eq!(&audio.output()[..4], &[0.125, 0.3125, 0.5, 0.6875]);
    assert_eq!(audio.report().unwrap().counters.voice_full, 0);
    game.observe_retained_history(history(&audio)).unwrap();
    game.observe_audio_source_timestamp(1, pair(7_000_000, LATER + 4_000_000))
        .unwrap();
    game.record_audio_prefix(host(LATER + 4_000_000)).unwrap();
    let event = audio.poll_retained_event().unwrap().unwrap();
    game.retain_output_event(event).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(LATER + 4_000_000), raw(8_000_000))
            .unwrap(),
        Some(event.serial)
    );
    audio.acknowledge_retained_event(event.serial).unwrap();
    game.acknowledge_retained_event(event.serial).unwrap();
    for (id, _) in members {
        assert_eq!(game.score(id).unwrap().hits, 1);
        assert_eq!(
            split_schedule(&game.effective_header(id).unwrap().options)
                .unwrap()
                .1
                .cuts()[0]
                .whole(),
            2_000_000
        );
    }
    game.fail();
    for (id, _) in members {
        let file =
            decode_replay(&game.take_replay(id).unwrap().unwrap(), fixture::limits()).unwrap();
        assert_eq!(
            split_schedule(&file.header.options).unwrap().1.cuts()[0].whole(),
            2_000_000
        );
    }
}

#[test]
fn cold_bgm_budget_uses_actual_full_geometry_not_total_cues_or_selected_region() {
    let (mut ordinary, _) = budget_solo(prepared());
    ordinary
        .configure_retained_audio_limits(4096, 128, 8, 16, 16)
        .unwrap();
    assert_eq!(
        ordinary.retained_setup().unwrap().config.limits.max_overlap,
        1
    );
    let header = ordinary.effective_header(PlayerId(1)).unwrap().clone();
    let before = cold_fields(ordinary.retained_setup().unwrap());
    for total in [0, 1] {
        assert!(ordinary
            .configure_retained_audio_limits(total, 128, 8, 16, 16)
            .is_err());
        assert_eq!(cold_fields(ordinary.retained_setup().unwrap()), before);
        assert_eq!(ordinary.effective_header(PlayerId(1)).unwrap(), &header);
    }
    ordinary
        .configure_retained_limits(PracticeLimits::new(8, 2, 128, 8, 16).unwrap(), 16)
        .unwrap();
    assert_eq!(
        ordinary.retained_setup().unwrap().config.limits.max_overlap,
        2,
        "explicit PracticeLimits retains its original reservation contract"
    );
    ordinary.activate_audio().unwrap();
    let active = cold_fields(ordinary.retained_setup().unwrap());
    assert!(ordinary
        .configure_retained_audio_limits(4096, 128, 8, 16, 16)
        .is_err());
    assert_eq!(cold_fields(ordinary.retained_setup().unwrap()), active);
    let pcm: Vec<_> = (0..32).map(|i| i as f32 / 32.0).collect();
    let sequential = "#BPM 60\n#WAV01 key.wav\n#00011:01\n#00001:01\n#00101:01\n#00201:01\n";
    let (mut game, _) = budget_solo(budget_prepared(sequential, &pcm));
    assert_eq!(game.retained_setup().unwrap().cues.len(), 3);
    game.configure_retained_audio_limits(2, 128, 8, 16, 16)
        .unwrap();
    assert_eq!(game.retained_setup().unwrap().config.limits.max_overlap, 1);
    let future_overlap = "#BPM 60\n#WAV01 key.wav\n#00011:01\n#00001:01\n#00101:01\n#00101:01\n";
    let (mut game, _) = budget_solo(budget_prepared(future_overlap, &pcm));
    let before = cold_fields(game.retained_setup().unwrap());
    let header = game.effective_header(PlayerId(1)).unwrap().clone();
    assert!(
        game.configure_retained_audio_limits(2, 128, 8, 16, 16)
            .is_err(),
        "two actual future BGM voices plus gameplay cannot fit total2"
    );
    assert_eq!(cold_fields(game.retained_setup().unwrap()), before);
    assert_eq!(game.effective_header(PlayerId(1)).unwrap(), &header);
    game.configure_retained_audio_limits(3, 128, 8, 16, 16)
        .unwrap();
    assert_eq!(game.retained_setup().unwrap().config.limits.max_overlap, 2);
}

#[test]
fn zero_bgm_and_empty_pcm_reserve_zero_while_bgm_only_can_use_the_whole_budget() {
    let pcm: Vec<_> = (0..32).map(|i| i as f32 / 32.0).collect();
    for (chart, samples, overlap, expected) in [
        (
            "#BPM 60\n#WAV01 key.wav\n#00011:01\n",
            pcm.as_slice(),
            0,
            [0.0, 0.0],
        ),
        (
            "#BPM 60\n#WAV01 key.wav\n#00011:01\n#00001:01\n",
            &[][..],
            0,
            [0.0, 0.0],
        ),
        (
            "#BPM 60\n#WAV01 key.wav\n#00001:01\n",
            pcm.as_slice(),
            1,
            [0.0, 0.015625],
        ),
    ] {
        let (mut game, bank) = budget_solo(budget_prepared(chart, samples));
        game.configure_retained_audio_limits(1, 128, 8, 16, 16)
            .unwrap();
        assert_eq!(
            game.retained_setup().unwrap().config.limits.max_overlap,
            overlap
        );
        let mut audio = output_before_render(bank, game.retained_setup().unwrap(), 1);
        audio.render(CONTEXT, 2).unwrap();
        assert_eq!(&audio.output()[..2], &expected);
        assert_eq!(audio.report().unwrap().counters.voice_full, 0);
    }
}
