//! Deferred portable contact-mode admission, capture and actual replay/audio owners.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    multiplayer::{competition_identity, competition_identity_for_section},
    replay_capture::{CaptureError, LiveReplayCapture, setup_input_header, setup_section_header},
    replay_playback::{
        decode_chart_setup, decode_profile, decode_section_setup, decode_setup, reconstruct,
        reconstruct_section, validate_section_setup, validate_setup,
    },
    section_start::{prepare_section_replay, source_at},
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError},
    step_replay::{StepReplay, StepReplayConfig},
};
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits, SampleBank,
        command_queue,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, ContactId, DeviceId,
        DeviceSelector, EventMeta, GameControlId, GameInputEvent, NativeEventMeta,
        PhysicalControlId, PhysicalInputEvent, Position2, TouchEvent, TouchPhase,
    },
    interaction::{InteractionState, StartEligibility},
    judge::{
        JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow, MissReason,
    },
    replay::{
        ReplayOperation,
        codec::{ReplayCodecError, ReplayCodecLimits, ReplayFile, decode_replay, encode_replay},
    },
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_bms::{BmsChart, BmsInputMode, parse_seeded};

const CHART: &str = "#BPM 60\n#WAV01 key.wav\n#00011:01\n#00052:00010100\n";
const CONTACT: BmsInputMode = BmsInputMode::ButtonOrContact;
const BUTTON: BmsInputMode = BmsInputMode::ButtonOnly;
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
    }
}
fn limits(header: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(65_536, 64, header, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn pcm_limits() -> PcmLimits {
    PcmLimits::new(256, 1024, 8).unwrap()
}
fn source() -> BmsChart {
    parse_seeded(CHART, Default::default(), u64::MAX).unwrap()
}
fn profile() -> JudgeProfile {
    JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(7),
            early: Duration::from_nanos(11),
            late: Duration::from_nanos(23),
        }],
        Duration::from_nanos(-19),
    )
    .unwrap()
}
fn judge(mode: BmsInputMode, start: i64) -> JudgeEngine {
    let selected = source_at(&source(), ts(start)).unwrap();
    JudgeEngine::new(
        selected.compile().unwrap().chart,
        selected.rules_with_input_mode(mode),
        profile(),
    )
    .unwrap()
}
fn body() -> Vec<u8> {
    // Independent wire expectation: offset -19, one grade-7 window [-11,+23].
    let mut bytes = (-19i64).to_le_bytes().to_vec();
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&11i64.to_le_bytes());
    bytes.extend_from_slice(&23i64.to_le_bytes());
    bytes
}
fn v5(seed: u64, start: i64, end: Option<i64>) -> Vec<u8> {
    let mut bytes = b"bms-judge-profile/v5:".to_vec();
    bytes.push(1);
    bytes.extend_from_slice(&seed.to_le_bytes());
    bytes.extend_from_slice(&start.to_le_bytes());
    bytes.push(u8::from(end.is_some()));
    if let Some(end) = end {
        bytes.extend_from_slice(&end.to_le_bytes());
    }
    bytes.extend(body());
    bytes
}
fn config(offset: i64) -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(11, 10_000_000_000),
        output_origin: point(22, 604_800_000_000_017),
        preroll: Duration::from_nanos(250_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: offset,
        command_capacity: 16,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(4_000_000_000),
        telemetry_capacity: 8,
    }
}
fn surface(code: u32) -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(0x544f_5543),
        code,
    }
}
fn bindings() -> BindingMap {
    BindingMap::from_bindings(
        [(1, 0x11), (2, 0x12), (3, 0x12)].map(|(code, lane)| Binding {
            device: DeviceSelector::Any,
            physical: surface(code),
            game_control: GameControlId(lane),
        }),
    )
    .unwrap()
}
fn prepared() -> PreparedBms {
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&40u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    for value in [1u16, 1] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [4u32, 8] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [2u16, 16] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4u32.to_le_bytes());
    for value in [8192i16, -4096] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", CHART.as_bytes().to_vec())
        .unwrap();
    files.insert("pack/key.wav", wav).unwrap();
    prepare_from_source(
        CHART.as_bytes(),
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(4, 1).unwrap(),
        pcm_limits(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        u64::MAX,
        None,
    )
    .unwrap()
}
fn game(
    mode: BmsInputMode,
    chosen: StepGameplayConfig,
    end: Option<i64>,
) -> (StepGameplay, SampleBank) {
    StepGameplay::new_section_with_input_mode(
        prepared(),
        chosen,
        bindings(),
        Timestamp::ZERO,
        end.map(ts),
        mode,
    )
    .unwrap()
}
fn mapper(chosen: StepGameplayConfig) -> AffineClockMapper {
    AffineClockMapper::exact_offset(
        ClockPair {
            source: chosen.host_origin,
            target: chosen.output_origin,
        },
        ClockInterval {
            start: chosen.host_origin.timestamp,
            end: ts(30_000_000_000),
        },
    )
    .unwrap()
}
fn host(chosen: StepGameplayConfig, song: i64) -> ClockPoint {
    point(
        chosen.host_origin.domain.0,
        chosen.host_origin.timestamp.as_nanos() + chosen.preroll.as_nanos() + song,
    )
}
fn output(chosen: StepGameplayConfig, elapsed: i64) -> ClockPoint {
    point(
        chosen.output_origin.domain.0,
        chosen.output_origin.timestamp.as_nanos() + elapsed,
    )
}
fn touch(
    chosen: StepGameplayConfig,
    song: i64,
    seq: u64,
    device: u64,
    control: u32,
    contact: u64,
    phase: TouchPhase,
) -> PhysicalInputEvent {
    let at = host(chosen, song);
    let mut meta = EventMeta::new(DeviceId(device), at, seq);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(0x544f_5543),
        code: Some(control),
        timestamp: Some(point(44, -7)),
    });
    meta.original_clock_point = Some(point(55, 9_007_199_254_740_993));
    PhysicalInputEvent::Touch(TouchEvent {
        meta,
        control: surface(control),
        contact: ContactId(contact),
        phase,
        position: Position2 {
            x: -0.25,
            y: 1234.5,
        },
        pressure: Some(0.75),
    })
}
fn send(
    owner: &mut StepGameplay,
    chosen: StepGameplayConfig,
    song: i64,
    input: PhysicalInputEvent,
) -> beatkernel::runtime::RuntimeReport {
    owner
        .process_input(
            input,
            &mapper(chosen),
            output(chosen, song + chosen.preroll.as_nanos()),
        )
        .unwrap()
}
fn advance(
    owner: &mut StepGameplay,
    chosen: StepGameplayConfig,
    song: i64,
) -> beatkernel::runtime::RuntimeReport {
    owner
        .advance_to(
            host(chosen, song),
            &mapper(chosen),
            output(chosen, song + chosen.preroll.as_nanos()),
        )
        .unwrap()
}
fn recording(owner: &mut StepGameplay) -> ReplayFile {
    owner.fail();
    let bytes = owner.take_replay().unwrap().unwrap();
    let file = decode_replay(&bytes, limits(4096)).unwrap();
    assert_eq!(encode_replay(&file, limits(4096)).unwrap(), bytes);
    assert!(owner.take_replay().unwrap().is_none());
    file
}
fn mixer(bank: SampleBank, chosen: StepGameplayConfig, end: u64) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(16).unwrap();
    let config = MixerConfig::new(
        bank.format(),
        chosen.output_origin.domain,
        chosen.output_origin.timestamp,
        AudioLimits::new(16, 8, 16, 16, 16).unwrap(),
    )
    .with_playback_end_frame(end);
    (producer, Mixer::new(config, bank, consumer).unwrap())
}

#[test]
fn adapter_mode_is_explicit_and_old_rules_ignore_touch_while_both_modes_keep_button_judging() {
    assert_eq!(BmsInputMode::default(), BUTTON);
    let chart = source();
    for mode in [BUTTON, CONTACT] {
        let eligibility = if mode == BUTTON {
            StartEligibility::ProfileButtonPress
        } else {
            StartEligibility::ProfilePress
        };
        assert!(
            chart
                .rules_with_input_mode(mode)
                .iter()
                .all(|rule| rule.evaluator.start_eligibility() == eligibility)
        );
    }
    assert!(
        chart
            .rules()
            .iter()
            .all(|rule| rule.evaluator.start_eligibility() == StartEligibility::ProfileButtonPress)
    );
    let old = JudgeEngine::new(chart.compile().unwrap().chart, chart.rules(), profile()).unwrap();
    assert_eq!(
        old.stable_hash().unwrap(),
        judge(BUTTON, 0).stable_hash().unwrap()
    );
    assert_ne!(
        old.stable_hash().unwrap(),
        judge(CONTACT, 0).stable_hash().unwrap()
    );
    for mode in [BUTTON, CONTACT] {
        let mut owner = judge(mode, 0);
        let physical = touch(config(0), 19, 1, 77, 1, u64::MAX, TouchPhase::Down);
        let bound = GameInputEvent {
            game_control: GameControlId(0x11),
            physical: physical.clone(),
        };
        let result = owner.push_input(&bound, ts(19)).unwrap();
        assert_eq!(result.len(), usize::from(mode == CONTACT));
        if mode == CONTACT {
            assert_eq!(result[0].input, Some(*physical.meta()));
        }
        let mut key_owner = judge(mode, 0);
        let key = GameInputEvent {
            game_control: GameControlId(0x11),
            physical: PhysicalInputEvent::Button(ButtonEvent {
                meta: *physical.meta(),
                control: surface(1),
                state: ButtonState::Down,
            }),
        };
        let hits = key_owner.push_input(&key, ts(19)).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].outcome,
            JudgeOutcome::Hit {
                grade: JudgeGrade(7),
                delta: Duration::ZERO
            }
        );
    }
}

#[test]
fn contact_headers_have_literal_v5_extents_and_exact_budgets_while_button_headers_keep_v1_through_v4()
 {
    for (seed, start, end) in [
        (0, 0, None),
        (u64::MAX, 0, Some(1)),
        (0, 604_800_000_000_001, Some(604_800_000_000_002)),
        (u64::MAX, i64::MAX - 1, Some(i64::MAX)),
    ] {
        let judge = judge(CONTACT, start);
        let header = setup_input_header(
            &judge,
            ClockDomainId(11),
            limits(4096),
            ts(start),
            seed,
            end.map(ts),
            CONTACT,
        )
        .unwrap();
        assert_eq!(header.options, v5(seed, start, end));
        assert_eq!(header.rules_identity, b"beatkernel-bms/press-judge/v1");
        assert_eq!(header.seed, 0);
        let decoded = decode_section_setup(&header.options).unwrap();
        assert_eq!(
            (
                decoded.start,
                decoded.end,
                decoded.chart_seed,
                decoded.input_mode
            ),
            (ts(start), end.map(ts), seed, CONTACT)
        );
        assert_eq!(decoded.profile, profile());
        let cap = header.chart_identity.len()
            + header.rules_identity.len()
            + header.options.len()
            + env!("CARGO_PKG_VERSION").len();
        assert_eq!(
            setup_input_header(
                &judge,
                ClockDomainId(11),
                limits(cap),
                ts(start),
                seed,
                end.map(ts),
                CONTACT
            )
            .unwrap(),
            header
        );
        assert!(matches!(
            setup_input_header(
                &judge,
                ClockDomainId(11),
                limits(cap - 1),
                ts(start),
                seed,
                end.map(ts),
                CONTACT
            ),
            Err(CaptureError::Codec(ReplayCodecError::HeaderTooLarge))
        ));
        let capture = LiveReplayCapture::new_with_input_mode(
            &judge,
            ClockDomainId(11),
            limits(4096),
            ts(start),
            seed,
            end.map(ts),
            CONTACT,
        )
        .unwrap();
        assert_eq!(capture.header(), &header);
        let bytes = capture.into_bytes().unwrap();
        assert_eq!(
            encode_replay(&decode_replay(&bytes, limits(4096)).unwrap(), limits(4096)).unwrap(),
            bytes
        );
    }
    for (seed, start, end, version) in [
        (0, 0, None, 1),
        (0, 1, None, 2),
        (u64::MAX, 1, None, 3),
        (0, 1, Some(2), 4),
    ] {
        let judge = judge(BUTTON, start);
        let old = setup_section_header(
            &judge,
            ClockDomainId(11),
            limits(4096),
            ts(start),
            seed,
            end.map(ts),
        )
        .unwrap();
        let header = setup_input_header(
            &judge,
            ClockDomainId(11),
            limits(4096),
            ts(start),
            seed,
            end.map(ts),
            BUTTON,
        )
        .unwrap();
        let mut literal = format!("bms-judge-profile/v{version}:").into_bytes();
        if version >= 3 {
            literal.extend_from_slice(&seed.to_le_bytes());
        }
        if version >= 2 {
            literal.extend_from_slice(&start.to_le_bytes());
        }
        if let Some(end) = end {
            literal.extend_from_slice(&end.to_le_bytes());
        }
        literal.extend(body());
        assert_eq!(header, old);
        assert_eq!(header.options, literal);
        assert_eq!(decode_section_setup(&literal).unwrap().input_mode, BUTTON);
        let old = LiveReplayCapture::new_section(
            &judge,
            ClockDomainId(11),
            limits(4096),
            ts(start),
            seed,
            end.map(ts),
        )
        .unwrap();
        let explicit = LiveReplayCapture::new_with_input_mode(
            &judge,
            ClockDomainId(11),
            limits(4096),
            ts(start),
            seed,
            end.map(ts),
            BUTTON,
        )
        .unwrap();
        assert_eq!(old.into_bytes().unwrap(), explicit.into_bytes().unwrap());
    }
}

#[test]
fn v5_rejects_noncanonical_modes_tags_extents_windows_and_legacy_consumers_cannot_drop_the_mode() {
    let valid = v5(u64::MAX, 1, Some(i64::MAX));
    let prefix = b"bms-judge-profile/v5:".len();
    for length in 0..valid.len() {
        assert!(
            decode_section_setup(&valid[..length]).is_err(),
            "truncation {length}"
        );
    }
    for (offset, replacement) in [(0, 0), (0, 2), (17, 2), (17, 255)] {
        let mut bad = valid.clone();
        bad[prefix + offset] = replacement;
        assert!(decode_section_setup(&bad).is_err());
    }
    for (offset, value) in [
        (9, (-1i64).to_le_bytes()),
        (18, 1i64.to_le_bytes()),
        (18, (-1i64).to_le_bytes()),
        (34, 0u64.to_le_bytes()),
        (34, u64::MAX.to_le_bytes()),
        (46, (-1i64).to_le_bytes()),
    ] {
        let mut bad = valid.clone();
        bad[prefix + offset..prefix + offset + 8].copy_from_slice(&value);
        assert!(decode_section_setup(&bad).is_err());
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(decode_section_setup(&trailing).is_err());
    let pristine = judge(CONTACT, 0);
    for end in [None, Some(ts(3_000_000_000))] {
        let file = LiveReplayCapture::new_with_input_mode(
            &pristine,
            ClockDomainId(11),
            limits(4096),
            Timestamp::ZERO,
            u64::MAX,
            end,
            CONTACT,
        )
        .unwrap()
        .into_file();
        assert!(decode_chart_setup(&file.header.options).is_err());
        assert!(decode_setup(&file.header.options).is_err());
        assert!(decode_profile(&file.header.options).is_err());
        assert!(validate_setup(&source(), &file, limits(4096)).is_err());
        assert!(reconstruct(&source(), file.clone(), limits(4096)).is_err());
        assert!(validate_section_setup(&source(), &file, limits(4096)).is_ok());
    }
    for end in [-1, 0] {
        assert!(matches!(
            setup_input_header(
                &pristine,
                ClockDomainId(11),
                limits(4096),
                Timestamp::ZERO,
                0,
                Some(ts(end)),
                CONTACT
            ),
            Err(CaptureError::InvalidEnd)
        ));
    }
    assert!(matches!(
        setup_input_header(
            &pristine,
            ClockDomainId(11),
            limits(4096),
            ts(-1),
            0,
            None,
            CONTACT
        ),
        Err(CaptureError::InvalidStart)
    ));
    assert!(pristine.effective_song_time().is_none());
    assert!(
        setup_input_header(
            &pristine,
            ClockDomainId(11),
            limits(4096),
            Timestamp::ZERO,
            0,
            None,
            CONTACT
        )
        .is_ok()
    );
}

#[test]
fn actual_contact_live_capture_reconstructs_identical_judgments_and_finite_mixer_pcm_through_step_replay()
 {
    let chosen = config(0);
    let (mut owner, bank) = game(CONTACT, chosen, Some(3_000_000_000));
    owner.configure_capture(limits(4096), u64::MAX).unwrap();
    owner.activate(chosen.host_origin).unwrap();
    let mut events = advance(&mut owner, chosen, -250_000_000).judge_events;
    for (song, control, phase, seq) in [
        (0, 1, TouchPhase::Down, 1),
        (1_000_000_000, 2, TouchPhase::Down, 2),
        (2_000_000_000, 2, TouchPhase::Up, 3),
    ] {
        events.extend(
            send(
                &mut owner,
                chosen,
                song,
                touch(chosen, song, seq, 77, control, u64::MAX, phase),
            )
            .judge_events,
        );
    }
    let terminal = advance(&mut owner, chosen, 3_000_000_000);
    assert!(terminal.song_end_reached);
    events.extend(terminal.judge_events);
    assert_eq!(
        events.iter().map(|event| event.stage).collect::<Vec<_>>(),
        [
            JudgeStage::Instant,
            JudgeStage::HoldHead,
            JudgeStage::HoldTail
        ]
    );
    let hash = owner.judge().stable_hash().unwrap();
    let score = owner.score().clone();
    assert_eq!((score.hits, score.misses), (3, 0));
    assert_eq!(owner.playback_end_frame(), Some(13));
    let (mut producer, mut live_mixer) = mixer(bank, chosen, 13);
    let mut commands = Vec::new();
    while let Some(batch) = owner.take_commands(16).unwrap() {
        for command in &batch.commands {
            producer.try_push(*command).unwrap();
        }
        owner
            .acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        commands.extend(batch.commands);
    }
    assert_eq!(commands.len(), 2); // LNTYPE1 tail is judged without another keysound.
    let mut live_pcm = [0.0; 16];
    let live_report = live_mixer.render(&mut live_pcm).unwrap();
    owner
        .feed_audio(live_report.counters.rendered_frames, 16)
        .unwrap();
    assert!(
        owner
            .observe_completion(Some(live_report), Some(output(chosen, 4_000_000_000)))
            .unwrap()
    );
    assert_eq!(
        live_pcm,
        [
            0.0, 0.25, -0.125, 0.0, 0.0, 0.25, -0.125, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0
        ]
    );
    let file = recording(&mut owner);
    assert_eq!(
        decode_section_setup(&file.header.options)
            .unwrap()
            .input_mode,
        CONTACT
    );
    assert_eq!(file.records.first().unwrap().song_time, ts(-250_000_000));
    let reconstructed = reconstruct_section(&source(), file.clone(), limits(4096)).unwrap();
    assert_eq!(reconstructed.results(), events);
    assert_eq!(reconstructed.engine().stable_hash().unwrap(), hash);
    let selected = prepare_section_replay(prepared(), &file, limits(4096), pcm_limits()).unwrap();
    let (mut replay, bank) = StepReplay::new(
        selected,
        file,
        limits(4096),
        StepReplayConfig {
            output_origin: chosen.output_origin,
            preroll: chosen.preroll,
            lookahead: Duration::from_nanos(4_000_000_000),
            max_pending: 16,
        },
    )
    .unwrap();
    let (mut producer, mut replay_mixer) = mixer(bank, chosen, 13);
    let batch = replay.take_commands(16).unwrap().unwrap();
    assert_eq!(batch.commands, commands);
    for command in &batch.commands {
        producer.try_push(*command).unwrap();
    }
    replay
        .acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    let mut replay_pcm = [0.0; 16];
    let report = replay_mixer.render(&mut replay_pcm).unwrap();
    replay
        .observe_output(Some(report), Some(output(chosen, 4_000_000_000)))
        .unwrap();
    assert!(replay.take_commands(16).unwrap().is_none());
    assert!(replay.observe_output(None, None).unwrap());
    assert_eq!(replay_pcm, live_pcm);
    assert_eq!(replay.drain_events(), events);
    assert_eq!(replay.score(), &score);
}

#[test]
fn contact_owner_cancel_offset_and_original_provenance_survive_prefix_recording_without_synthetic_advance()
 {
    let chosen = config(10);
    let (mut owner, _) = game(CONTACT, chosen, None);
    owner.configure_capture(limits(4096), u64::MAX).unwrap();
    let mut expected_inputs = Vec::new();
    let mut events = Vec::new();
    for (song, seq, device, control, contact, phase) in [
        (-10, 1, 77, 1, 8, TouchPhase::Down),
        (999_999_990, 2, 77, 2, u64::MAX, TouchPhase::Down),
        (999_999_990, 3, 77, 2, u64::MAX, TouchPhase::Down),
        (1_499_999_990, 4, 77, 2, u64::MAX, TouchPhase::Move),
        (1_999_999_990, 1, 88, 2, u64::MAX, TouchPhase::Up),
        (1_999_999_990, 5, 77, 3, u64::MAX, TouchPhase::Up),
        (1_999_999_990, 6, 77, 2, 0, TouchPhase::Cancel),
        (1_999_999_990, 7, 77, 2, u64::MAX, TouchPhase::Cancel),
    ] {
        let input = touch(chosen, song, seq, device, control, contact, phase);
        let report = send(&mut owner, chosen, song, input.clone());
        assert_eq!(report.bound_inputs.len(), 1);
        assert_eq!(report.bound_inputs[0].physical, input);
        expected_inputs.push((ts(song), report.bound_inputs[0].clone()));
        events.extend(report.judge_events);
    }
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].at, Timestamp::ZERO);
    assert_eq!(events[1].at, ts(1_000_000_000));
    assert_eq!(events[2].at, ts(2_000_000_000));
    assert_eq!(
        events[2].outcome,
        JudgeOutcome::Miss {
            reason: MissReason::RejectedInput
        }
    );
    assert_eq!((owner.score().hits, owner.score().misses), (2, 1));
    let hash = owner.judge().stable_hash().unwrap();
    let file = recording(&mut owner);
    assert_eq!(file.records.len(), expected_inputs.len());
    for (record, (song, event)) in file.records.iter().zip(&expected_inputs) {
        assert_eq!(record.song_time, *song);
        assert_eq!(record.operation, ReplayOperation::Input(event.clone()));
    }
    assert_eq!(file.records.last().unwrap().song_time, ts(1_999_999_990));
    let replay = reconstruct_section(&source(), file, limits(4096)).unwrap();
    assert_eq!(replay.results(), events);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);

    let (mut prefix, _) = game(CONTACT, chosen, None);
    prefix.configure_capture(limits(4096), 0).unwrap();
    send(
        &mut prefix,
        chosen,
        -10,
        touch(chosen, -10, 1, 77, 1, 1, TouchPhase::Down),
    );
    send(
        &mut prefix,
        chosen,
        999_999_990,
        touch(chosen, 999_999_990, 2, 77, 2, 2, TouchPhase::Down),
    );
    let file = recording(&mut prefix);
    let replay = reconstruct_section(&source(), file, limits(4096)).unwrap();
    assert_eq!(replay.results().len(), 2);
    assert!(
        replay
            .results()
            .iter()
            .all(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
    );
    let hold = source()
        .compile()
        .unwrap()
        .chart
        .objects()
        .iter()
        .find(|object| object.time.end.is_some())
        .unwrap()
        .id;
    assert_eq!(replay.engine().state(hold), Some(InteractionState::Active));
}

#[test]
fn competition_and_reconstruction_keep_mode_identity_distinct_and_finite_end_outside_the_base_header()
 {
    let chosen = config(0);
    let (mut contact, _) = game(CONTACT, chosen, Some(3_000_000_000));
    let (button, _) = game(BUTTON, chosen, Some(3_000_000_000));
    let header = contact.competition_header(limits(4096), u64::MAX).unwrap();
    let setup = decode_section_setup(&header.options).unwrap();
    assert_eq!(setup.input_mode, CONTACT);
    assert_eq!(setup.end, None);
    let identity = contact
        .competition_identity(limits(4096), u64::MAX)
        .unwrap();
    assert_eq!(
        identity,
        competition_identity_for_section(
            &header,
            env!("CARGO_PKG_VERSION"),
            limits(4096),
            Some(ts(3_000_000_000))
        )
        .unwrap()
    );
    assert_ne!(
        identity,
        button.competition_identity(limits(4096), u64::MAX).unwrap()
    );
    contact.configure_capture(limits(4096), u64::MAX).unwrap();
    assert_eq!(
        contact
            .competition_identity(limits(4096), u64::MAX)
            .unwrap(),
        identity
    );
    let mut other_clock = header.clone();
    other_clock.normalized_clock = ClockDomainId(999);
    assert_eq!(
        competition_identity_for_section(
            &other_clock,
            env!("CARGO_PKG_VERSION"),
            limits(4096),
            Some(ts(3_000_000_000))
        )
        .unwrap(),
        identity
    );
    assert_ne!(
        competition_identity_for_section(
            &header,
            env!("CARGO_PKG_VERSION"),
            limits(4096),
            Some(ts(3_000_000_001))
        )
        .unwrap(),
        identity
    );
    let file = recording(&mut contact);
    assert_eq!(
        decode_section_setup(&file.header.options).unwrap().end,
        Some(ts(3_000_000_000))
    );
    assert!(
        competition_identity_for_section(
            &file.header,
            env!("CARGO_PKG_VERSION"),
            limits(4096),
            Some(ts(3_000_000_000))
        )
        .is_err()
    );
    assert_eq!(
        competition_identity_for_section(
            &file.header,
            env!("CARGO_PKG_VERSION"),
            limits(4096),
            None
        )
        .unwrap(),
        competition_identity(&file.header, env!("CARGO_PKG_VERSION"), limits(4096)).unwrap()
    );
    assert!(validate_section_setup(&source(), &file, limits(4096)).is_ok());
    for changed in 0..4 {
        let mut bad = file.clone();
        match changed {
            0 => bad.header.rules_identity = b"beatkernel-bms/builtin-judge/v1".to_vec(),
            1 => bad.header.chart_identity[0] ^= 1,
            2 => bad.runtime_version.push_str("-other"),
            _ => bad.header.options[b"bms-judge-profile/v5:".len()] = 0,
        }
        assert!(reconstruct_section(&source(), bad, limits(4096)).is_err());
    }
    let altered = parse_seeded(
        &CHART.replace("#BPM 60", "#BPM 120"),
        Default::default(),
        u64::MAX,
    )
    .unwrap();
    assert!(reconstruct_section(&altered, file, limits(4096)).is_err());
    let (mut started, _) = game(CONTACT, chosen, None);
    advance(&mut started, chosen, -250_000_000);
    assert!(matches!(
        started.competition_header(limits(4096), 0),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert!(!started.failed());
}
