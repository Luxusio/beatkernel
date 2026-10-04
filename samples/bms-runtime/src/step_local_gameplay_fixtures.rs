//! Deferred actual shared-step runtime, original PCM, capture and replay fixtures.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::{InputResult, PlayerReport},
    replay_capture::CaptureError,
    replay_playback::{decode_section_setup, reconstruct_section},
    section_start::{prepare_at, prepare_section_replay},
    step_gameplay::{
        StepGameplay, StepGameplayConfig, StepGameplayError, StepLocalGameplay,
        StepLocalGameplayError,
    },
    step_replay::{StepReplay, StepReplayConfig},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        RenderReport, SampleBank, SampleId, command_queue,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, ContactId, DeviceId,
        DeviceSelector, EventMeta, GameControlId, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent, Position2, TouchEvent, TouchPhase, TouchRegion, TouchRouter,
    },
    judge::JudgeOutcome,
    replay::{
        ReplayOperation,
        codec::{ReplayCodecError, ReplayCodecLimits, decode_replay, encode_replay},
    },
    runtime::RuntimeReport,
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_bms::BmsInputMode;
use beatkernel_platform::audio::presentation::discipline::{DisciplineConfig, DisciplineUpdate};

const PLAYERS: [PlayerId; 4] = [PlayerId(7), PlayerId(u32::MAX), PlayerId(2), PlayerId(91)];
const DEVICES: [DeviceId; 4] = [
    DeviceId(0),
    DeviceId(u64::MAX),
    DeviceId(0x8877_6655_4433_2211),
    DeviceId(3),
];
const TWO: &str = "#BPM 60\n#00011:01\n#00012:00010000\n";
const MUSIC: &str = "#BPM 60\n#00011:01\n#00012:00010000\n#00001:02\n";
const SEED: u64 = u64::MAX;
struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(11, 10_000_000_000),
        output_origin: point(22, 604_800_000_000_017),
        preroll: Duration::from_nanos(125_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 32,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(4_000_000_000),
        telemetry_capacity: 8,
    }
}
fn limits(records: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(65_536, records, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn pcm_limits() -> PcmLimits {
    PcmLimits::new(4096, 16_384, 16).unwrap()
}
fn wav(rate: u32, values: &[i16]) -> Vec<u8> {
    let size = (values.len() * 2) as u32;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    for value in [1_u16, 1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [rate, rate * 2] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [2_u16, 16] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}
fn prepared(lines: &str, rate: u32) -> PreparedBms {
    let chart = format!("#WAV01 key.wav\n#WAV02 bg.wav\n{lines}");
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files
        .insert("pack/key.wav", wav(rate, &[2048, 4096, -2048, 0]))
        .unwrap();
    files.insert("pack/bg.wav", wav(rate, &[1024; 16])).unwrap();
    prepare_from_source(
        chart.as_bytes(),
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(rate, 1).unwrap(),
        pcm_limits(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        SEED,
        None,
    )
    .unwrap()
}
fn bindings(device: Option<DeviceId>, fanout: bool) -> BindingMap {
    BindingMap::from_bindings((0..3_u16).map(|index| Binding {
        device: device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
        physical: PhysicalControlId::keyboard(if fanout { 4 } else { 4 + index }),
        game_control: GameControlId(u32::from(0x11 + index)),
    }))
    .unwrap()
}
fn plan(count: usize) -> ResolvedInputPlan {
    ResolvedInputPlan::new(
        (0..count)
            .map(|index| (PLAYERS[index], Some(DEVICES[index])))
            .collect(),
    )
    .unwrap()
}
fn local(
    lines: &str,
    rate: u32,
    count: usize,
    chosen: StepGameplayConfig,
    start: i64,
    end: Option<i64>,
    mode: BmsInputMode,
) -> (StepLocalGameplay, SampleBank) {
    let (prepared, _) = prepare_at(
        prepared(lines, rate),
        Timestamp::from_nanos(start),
        pcm_limits(),
    )
    .unwrap();
    StepLocalGameplay::new_section(
        prepared,
        chosen,
        plan(count),
        (0..count)
            .map(|index| bindings(Some(DEVICES[index]), false))
            .collect(),
        Timestamp::from_nanos(start),
        end.map(Timestamp::from_nanos),
        mode,
    )
    .unwrap()
}
fn host(chosen: StepGameplayConfig, start: i64, song: i64) -> ClockPoint {
    point(
        chosen.host_origin.domain.0,
        chosen.host_origin.timestamp.as_nanos() + chosen.preroll.as_nanos() + song - start,
    )
}
fn output(chosen: StepGameplayConfig, ns: i64) -> ClockPoint {
    point(
        chosen.output_origin.domain.0,
        chosen.output_origin.timestamp.as_nanos() + ns,
    )
}
fn input(
    chosen: StepGameplayConfig,
    index: usize,
    start: i64,
    song: i64,
    key: u16,
    sequence: u64,
) -> PhysicalInputEvent {
    let at = host(chosen, start, song);
    let mut meta = EventMeta::new(DEVICES[index], at, sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(7),
        code: Some(u32::from(key)),
        timestamp: Some(at),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(key),
        state: ButtonState::Down,
    })
}
fn accepted(result: InputResult) -> Vec<PlayerReport> {
    match result {
        InputResult::Processed(reports) => reports,
        InputResult::Ignored { .. } => panic!("owned source ignored"),
    }
}
fn mixer(
    bank: SampleBank,
    chosen: StepGameplayConfig,
    end: Option<u64>,
) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(64).unwrap();
    let mut cfg = MixerConfig::new(
        bank.format(),
        chosen.output_origin.domain,
        chosen.output_origin.timestamp,
        AudioLimits::new(64, 32, 64, 64, 64).unwrap(),
    );
    if let Some(end) = end {
        cfg = cfg.with_playback_end_frame(end);
    }
    (producer, Mixer::new(cfg, bank, consumer).unwrap())
}
fn render(mixer: &mut Mixer, frames: usize) -> (Vec<f32>, RenderReport) {
    let mut pcm = vec![0.0; frames];
    let report = mixer.render(&mut pcm).unwrap();
    (pcm, report)
}
fn deliver(local: &mut StepLocalGameplay, producer: &mut CommandProducer) -> Vec<AudioCommand> {
    let mut commands = Vec::new();
    while let Some(batch) = local.take_commands(32).unwrap() {
        for command in &batch.commands {
            producer.try_push(*command).unwrap();
        }
        local
            .acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        commands.extend(batch.commands);
    }
    commands
}
fn same_report(left: &RuntimeReport, right: &RuntimeReport) {
    assert_eq!(left.input, right.input);
    assert_eq!(left.bound_inputs, right.bound_inputs);
    assert_eq!(left.song_time, right.song_time);
    assert_eq!(left.song_end_reached, right.song_end_reached);
    assert_eq!(left.audio_at, right.audio_at);
    assert_eq!(left.judge_events, right.judge_events);
    assert_eq!(left.input_mapping_quality, right.input_mapping_quality);
    assert_eq!(left.audio_mapping_quality, right.audio_mapping_quality);
    assert_eq!(left.audio_commands, right.audio_commands);
    assert_eq!(left.audio_failures, right.audio_failures);
    assert_eq!(left.judge_error, right.judge_error);
}
fn surface() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(0x5754_4f55),
        code: 0,
    }
}
fn router(index: usize) -> TouchRouter {
    TouchRouter::new(
        vec![TouchRegion {
            device: DeviceSelector::Exact(DEVICES[index]),
            physical: surface(),
            game_control: GameControlId(0x11),
            min: Position2 { x: 0.0, y: 0.0 },
            max: Position2 { x: 1.0, y: 1.0 },
        }],
        8,
    )
    .unwrap()
}
fn touch(
    chosen: StepGameplayConfig,
    index: usize,
    start: i64,
    song: i64,
    phase: TouchPhase,
    sequence: u64,
) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DEVICES[index], host(chosen, start, song), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(0x5754_4f55),
        code: Some(u32::MAX),
        timestamp: Some(point(91, -123)),
    });
    meta.original_clock_point = Some(point(92, 9_007_199_254_740_993));
    PhysicalInputEvent::Touch(TouchEvent {
        meta,
        control: surface(),
        contact: ContactId(u64::MAX - index as u64),
        phase,
        position: Position2 { x: 960.0, y: 720.0 },
        pressure: Some(0.75),
    })
}

#[test]
fn automatic_one_member_matches_existing_solo_reports_pcm_and_canonical_capture() {
    let chosen = config();
    let (mut solo, solo_bank) =
        StepGameplay::new(prepared(TWO, 8), chosen, bindings(None, false)).unwrap();
    let (mut local, bank) = StepLocalGameplay::new_section(
        prepared(TWO, 8),
        chosen,
        ResolvedInputPlan::new(vec![(PLAYERS[0], None)]).unwrap(),
        vec![bindings(None, false)],
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    assert_eq!(local.players(), &[PLAYERS[0]]);
    assert_eq!(
        local
            .competition_header(PLAYERS[0], limits(128), SEED)
            .unwrap(),
        solo.competition_header(limits(128), SEED).unwrap()
    );
    assert_eq!(
        local
            .competition_identity(PLAYERS[0], limits(128), SEED)
            .unwrap(),
        solo.competition_identity(limits(128), SEED).unwrap()
    );
    solo.configure_capture(limits(128), SEED).unwrap();
    local
        .configure_capture(PLAYERS[0], limits(128), SEED)
        .unwrap();
    solo.activate(chosen.host_origin).unwrap();
    local.activate(chosen.host_origin).unwrap();
    let first = solo
        .advance_to(chosen.host_origin, &SameDomain, chosen.output_origin)
        .unwrap();
    let rows = local
        .advance_to(chosen.host_origin, &SameDomain, chosen.output_origin)
        .unwrap();
    same_report(&rows[0].report, &first);
    for (song, key, sequence) in [(0, 4, 1), (1_000_000_000, 5, 2)] {
        let original = input(chosen, 0, 0, song, key, sequence);
        let at = output(chosen, song + chosen.preroll.as_nanos());
        let old = solo
            .process_input(original.clone(), &SameDomain, at)
            .unwrap();
        let rows = accepted(local.process_input(original, &SameDomain, at).unwrap());
        same_report(&rows[0].report, &old);
    }
    let at = host(chosen, 0, 2_000_000_000);
    let old = solo
        .advance_to(at, &SameDomain, output(chosen, 2_125_000_000))
        .unwrap();
    same_report(
        &local
            .advance_to(at, &SameDomain, output(chosen, 2_125_000_000))
            .unwrap()[0]
            .report,
        &old,
    );
    assert_eq!(local.score(PLAYERS[0]).unwrap(), solo.score());
    assert_eq!(
        local.judge(PLAYERS[0]).unwrap().stable_hash().unwrap(),
        solo.judge().stable_hash().unwrap()
    );
    let (mut solo_producer, mut solo_mixer) = mixer(solo_bank, chosen, None);
    let (mut producer, mut local_mixer) = mixer(bank, chosen, None);
    let mut expected = Vec::new();
    while let Some(batch) = solo.take_commands(32).unwrap() {
        for command in &batch.commands {
            solo_producer.try_push(*command).unwrap();
        }
        solo.acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        expected.extend(batch.commands);
    }
    assert_eq!(deliver(&mut local, &mut producer), expected);
    // This no-BGM chart naturally has the same voice IDs; local multi-member
    // preparation is allowed to remap them when it reserves shared BGM voices.
    let (pcm, report) = render(&mut local_mixer, 24);
    let (old_pcm, old_report) = render(&mut solo_mixer, 24);
    assert_eq!(pcm, old_pcm);
    assert_eq!(report, old_report);
    assert_eq!(
        local
            .observe_completion(Some(report), Some(output(chosen, 3_000_000_000)))
            .unwrap(),
        solo.observe_completion(Some(old_report), Some(output(chosen, 3_000_000_000)))
            .unwrap()
    );
    let (_, report) = render(&mut local_mixer, 1);
    let (_, old_report) = render(&mut solo_mixer, 1);
    assert_eq!(
        local
            .observe_completion(Some(report), Some(output(chosen, 3_125_000_000)))
            .unwrap(),
        solo.observe_completion(Some(old_report), Some(output(chosen, 3_125_000_000)))
            .unwrap()
    );
    solo.fail();
    local.fail();
    assert_eq!(
        local.take_replay(PLAYERS[0]).unwrap(),
        solo.take_replay().unwrap()
    );
    assert!(local.take_replay(PLAYERS[0]).unwrap().is_none());
}

#[test]
fn three_and_four_members_keep_judging_while_one_shared_audio_batch_awaits_acknowledgement() {
    for count in [3, 4] {
        let chosen = config();
        let (mut owner, bank) = local(MUSIC, 8, count, chosen, 0, None, BmsInputMode::ButtonOnly);
        assert_eq!(owner.players(), &PLAYERS[..count]);
        assert_eq!(bank.len(), 2);
        owner.activate(chosen.host_origin).unwrap();
        let at = output(chosen, chosen.preroll.as_nanos());
        let first = accepted(
            owner
                .process_input(input(chosen, 0, 0, 0, 4, 1), &SameDomain, at)
                .unwrap(),
        );
        assert_eq!(first[0].player, PLAYERS[0]);
        let held = owner.take_commands(32).unwrap().unwrap();
        assert_eq!(
            held.commands.len(),
            2,
            "one BGM publication and one real member hit"
        );
        for index in 1..count {
            let original = input(chosen, index, 0, 0, 4, 1);
            let rows = accepted(
                owner
                    .process_input(original.clone(), &SameDomain, at)
                    .unwrap(),
            );
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].player, PLAYERS[index]);
            assert_eq!(rows[0].report.bound_inputs[0].physical, original);
            assert_eq!(owner.score(PLAYERS[index]).unwrap().hits, 1);
        }
        assert!(
            matches!(owner.take_commands(32), Err(StepLocalGameplayError::Control(StepGameplayError::OutstandingBatch { sequence })) if sequence == held.sequence)
        );
        assert!(!owner.failed());
        let (mut producer, mut output_mixer) = mixer(bank, chosen, None);
        for command in &held.commands {
            producer.try_push(*command).unwrap();
        }
        owner
            .acknowledge(held.sequence, held.commands.len(), true)
            .unwrap();
        let mut commands = held.commands;
        commands.extend(deliver(&mut owner, &mut producer));
        let voices: std::collections::BTreeSet<_> = commands
            .iter()
            .filter_map(|command| {
                if let AudioCommand::Play { voice, .. } = command {
                    Some(*voice)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(voices.len(), count + 1);
        assert_eq!(commands.len(), count + 1);
        assert_eq!(
            commands
                .iter()
                .filter(|command| matches!(
                    command,
                    AudioCommand::Play {
                        sample: SampleId(2),
                        ..
                    }
                ))
                .count(),
            1
        );
        let (pcm, report) = render(&mut output_mixer, 24);
        assert_eq!(pcm[0], 0.0);
        assert_eq!(pcm[1], count as f32 * 0.0625 + 0.03125);
        assert_eq!(report.counters.commands_applied, count as u64 + 1);
        owner.feed_audio(24, 32).unwrap();
        assert!(
            !owner
                .observe_completion(Some(report), Some(output(chosen, 3_000_000_000)))
                .unwrap(),
            "audio drain cannot finish members with unjudged notes"
        );
        let rows = owner
            .advance_to(
                host(chosen, 0, 2_000_000_000),
                &SameDomain,
                output(chosen, 3_000_000_000),
            )
            .unwrap();
        assert_eq!(
            rows.iter().map(|row| row.player).collect::<Vec<_>>(),
            PLAYERS[..count]
        );
        for row in &rows {
            assert_eq!(row.report.song_time, Timestamp::from_nanos(2_000_000_000));
            assert!(matches!(
                row.report.judge_events[0].outcome,
                JudgeOutcome::Miss { .. }
            ));
            assert_eq!(owner.score(row.player).unwrap().misses, 1);
        }
        assert!(
            !owner
                .observe_completion(Some(report), Some(output(chosen, 3_000_000_000)))
                .unwrap()
        );
        let (_, idle) = render(&mut output_mixer, 1);
        assert!(
            owner
                .observe_completion(Some(idle), Some(output(chosen, 3_125_000_000)))
                .unwrap()
        );
    }
}

#[test]
fn member_contact_sections_capture_original_provenance_and_replay_the_same_shared_pcm_sum() {
    let chart = "#BPM 60\n#00011:00010000\n#00012:00000100\n";
    let chosen = config();
    let start = 1_000_000_000;
    let end = 1_500_000_000;
    let (mut owner, bank) = local(
        chart,
        8,
        2,
        chosen,
        start,
        Some(end),
        BmsInputMode::ButtonOrContact,
    );
    assert_eq!(owner.playback_end_frame(), Some(5));
    assert_eq!(owner.end_ns(), Some(end));
    for index in 0..2 {
        owner
            .configure_touch_router(PLAYERS[index], router(index))
            .unwrap();
        owner
            .configure_capture(PLAYERS[index], limits(128), SEED)
            .unwrap();
        let header = owner
            .competition_header(PLAYERS[index], limits(128), SEED)
            .unwrap();
        let setup = decode_section_setup(&header.options).unwrap();
        assert_eq!(setup.start.as_nanos(), start);
        assert_eq!(setup.end, None);
        assert_eq!(setup.input_mode, BmsInputMode::ButtonOrContact);
    }
    owner.activate(chosen.host_origin).unwrap();
    owner
        .advance_to(chosen.host_origin, &SameDomain, chosen.output_origin)
        .unwrap();
    let mut events = vec![Vec::new(), Vec::new()];
    for index in 0..2 {
        let original = touch(chosen, index, start, start, TouchPhase::Down, 1);
        let rows = accepted(
            owner
                .process_input_at(
                    original.clone(),
                    Position2 { x: 0.5, y: 0.5 },
                    &SameDomain,
                    output(chosen, chosen.preroll.as_nanos()),
                )
                .unwrap(),
        );
        assert_eq!(rows[0].player, PLAYERS[index]);
        assert_eq!(rows[0].report.bound_inputs[0].physical, original);
        assert_eq!(
            rows[0].report.bound_inputs[0].game_control,
            GameControlId(0x11)
        );
        assert_eq!(rows[0].report.judge_events[0].input, Some(*original.meta()));
        events[index].extend(rows[0].report.judge_events.clone());
    }
    for index in 0..2 {
        let original = touch(chosen, index, start, 1_250_000_000, TouchPhase::Cancel, 2);
        let rows = accepted(
            owner
                .process_input_at(
                    original.clone(),
                    Position2 { x: -1.0, y: -1.0 },
                    &SameDomain,
                    output(chosen, 375_000_000),
                )
                .unwrap(),
        );
        assert_eq!(rows[0].report.bound_inputs[0].physical, original);
        assert!(
            rows[0].report.judge_events.is_empty(),
            "routing release does not manufacture another instant judgment"
        );
    }
    let last = owner
        .advance_to(
            host(chosen, start, 2_500_000_000),
            &SameDomain,
            output(chosen, 1_625_000_000),
        )
        .unwrap();
    for row in last {
        assert_eq!(row.report.song_time.as_nanos(), end);
        assert!(row.report.song_end_reached);
        assert!(row.report.judge_events.is_empty());
    }
    let (mut producer, mut mixed) = mixer(bank, chosen, Some(5));
    assert_eq!(deliver(&mut owner, &mut producer).len(), 2);
    let (live_pcm, report) = render(&mut mixed, 8);
    assert!(
        owner
            .observe_completion(Some(report), Some(output(chosen, 1_000_000_000)))
            .unwrap()
    );
    let scores = PLAYERS[..2]
        .iter()
        .map(|player| owner.score(*player).unwrap().clone())
        .collect::<Vec<_>>();
    let hashes = PLAYERS[..2]
        .iter()
        .map(|player| owner.judge(*player).unwrap().stable_hash().unwrap())
        .collect::<Vec<_>>();
    owner.fail();
    let mut replay_sum = vec![0.0_f32; 8];
    for index in 0..2 {
        let bytes = owner.take_replay(PLAYERS[index]).unwrap().unwrap();
        assert!(owner.take_replay(PLAYERS[index]).unwrap().is_none());
        let file = decode_replay(&bytes, limits(128)).unwrap();
        assert_eq!(encode_replay(&file, limits(128)).unwrap(), bytes);
        let setup = decode_section_setup(&file.header.options).unwrap();
        assert_eq!(
            (
                setup.start.as_nanos(),
                setup.end.unwrap().as_nanos(),
                setup.chart_seed
            ),
            (start, end, SEED)
        );
        assert_eq!(setup.input_mode, BmsInputMode::ButtonOrContact);
        assert_eq!(
            file.records
                .iter()
                .map(|record| record.song_time.as_nanos())
                .collect::<Vec<_>>(),
            [875_000_000, start, 1_250_000_000, end]
        );
        let ReplayOperation::Input(recorded) = &file.records[1].operation else {
            panic!("actual contact operation required")
        };
        assert_eq!(
            recorded.physical,
            touch(chosen, index, start, start, TouchPhase::Down, 1)
        );
        let original = prepared(chart, 8);
        let rebuilt = reconstruct_section(&original.source, file.clone(), limits(128)).unwrap();
        assert_eq!(rebuilt.results(), events[index]);
        assert_eq!(rebuilt.engine().stable_hash().unwrap(), hashes[index]);
        let selected = prepare_section_replay(original, &file, limits(128), pcm_limits()).unwrap();
        let (mut replay, bank) = StepReplay::new(
            selected,
            file,
            limits(128),
            StepReplayConfig {
                output_origin: chosen.output_origin,
                preroll: chosen.preroll,
                lookahead: chosen.bgm_lookahead,
                max_pending: 32,
            },
        )
        .unwrap();
        let (mut producer, mut replay_mixer) = mixer(bank, chosen, replay.playback_end_frame());
        let batch = replay.take_commands(32).unwrap().unwrap();
        assert_eq!(batch.commands.len(), 1);
        producer.try_push(batch.commands[0]).unwrap();
        replay.acknowledge(batch.sequence, 1, true).unwrap();
        let (pcm, report) = render(&mut replay_mixer, 8);
        assert!(
            !replay
                .observe_output(Some(report), Some(output(chosen, 1_000_000_000)))
                .unwrap()
        );
        assert!(replay.take_commands(32).unwrap().is_none());
        assert!(replay.observe_output(None, None).unwrap());
        assert_eq!(replay.score(), &scores[index]);
        assert_eq!(replay.drain_events(), events[index]);
        for (sum, value) in replay_sum.iter_mut().zip(pcm) {
            *sum += value;
        }
    }
    assert_eq!(replay_sum, live_pcm);
}

#[test]
fn finite_shared_completion_requires_every_member_logical_end_and_genuine_execution_receipts() {
    let chosen = StepGameplayConfig {
        preroll: Duration::from_nanos(100_000_000),
        ..config()
    };
    let chart = "#BPM 60\n#00011:01\n#00112:01\n";
    for late_admission in [false, true] {
        let (mut owner, bank) = local(
            chart,
            3,
            2,
            chosen,
            0,
            Some(750_000_000),
            BmsInputMode::ButtonOnly,
        );
        owner.activate(chosen.host_origin).unwrap();
        for index in 0..2 {
            owner
                .process_input(
                    input(chosen, index, 0, 0, 4, 1),
                    &SameDomain,
                    output(chosen, 100_000_000),
                )
                .unwrap();
        }
        let batch = owner.take_commands(32).unwrap().unwrap();
        assert_eq!(batch.commands.len(), 2);
        let (mut producer, mut output_mixer) = mixer(bank, chosen, Some(3));
        if !late_admission {
            for command in &batch.commands {
                producer.try_push(*command).unwrap();
            }
        }
        let marker = if late_admission {
            render(&mut output_mixer, 4).1
        } else {
            let (silence, before) = render(&mut output_mixer, 1);
            assert_eq!(silence, [0.0]);
            assert_eq!(before.playback_end_physical_frame, None);
            assert!(!owner.observe_completion(Some(before), None).unwrap());
            render(&mut output_mixer, 3).1
        };
        assert_eq!(marker.playback_end_physical_frame, Some(3));
        assert!(!owner.observe_completion(Some(marker), None).unwrap());
        let one = accepted(
            owner
                .process_input(
                    input(chosen, 0, 0, 1_000_000_000, 5, 2),
                    &SameDomain,
                    output(chosen, 1_100_000_000),
                )
                .unwrap(),
        );
        assert!(one[0].report.song_end_reached);
        assert!(one[0].report.bound_inputs.is_empty());
        assert_eq!(
            owner.member_song_time(PLAYERS[0]).unwrap().as_nanos(),
            750_000_000
        );
        assert_eq!(owner.member_song_time(PLAYERS[1]).unwrap().as_nanos(), 0);
        assert!(
            !owner
                .observe_completion(None, Some(output(chosen, 1_000_000_000)))
                .unwrap()
        );
        let rows = owner
            .advance_to(
                host(chosen, 0, 1_000_000_000),
                &SameDomain,
                output(chosen, 1_100_000_000),
            )
            .unwrap();
        assert!(rows.iter().all(
            |row| row.report.song_end_reached && row.report.song_time.as_nanos() == 750_000_000
        ));
        assert!(
            !owner.observe_completion(None, None).unwrap(),
            "real marker and every logical end still cannot stand in for the outstanding ACK"
        );
        if late_admission {
            for command in &batch.commands {
                producer.try_push(*command).unwrap();
            }
        }
        owner.acknowledge(batch.sequence, 2, true).unwrap();
        assert!(owner.take_commands(32).unwrap().is_none());
        if late_admission {
            assert!(
                matches!(owner.observe_completion(None, None), Err(StepLocalGameplayError::Control(
                StepGameplayError::Completion { rendered: Some(actual), .. })) if actual == marker)
            );
            assert!(owner.failed());
            assert_eq!(marker.counters.commands_applied, 0);
        } else {
            assert!(
                marker.active_voices > 0,
                "finite completion may retain genuine crossing PCM voices"
            );
            assert_eq!(marker.counters.commands_consumed, 2);
            assert_eq!(marker.counters.commands_applied, 2);
            assert!(owner.observe_completion(None, None).unwrap());
        }
        for player in &PLAYERS[..2] {
            assert_eq!(owner.score(*player).unwrap().hits, 1);
            assert_eq!(owner.score(*player).unwrap().misses, 0);
        }
    }
}

#[test]
fn core_audio_failure_and_member_capture_failure_retain_every_already_committed_report() {
    let chart = "#BPM 60\n#00011:01\n#00012:01\n";
    let chosen = StepGameplayConfig {
        command_capacity: 2,
        bgm_pending: 1,
        ..config()
    };
    let (mut owner, _) = StepLocalGameplay::new_section(
        prepared(chart, 8),
        chosen,
        plan(3),
        (0..3)
            .map(|index| bindings(Some(DEVICES[index]), true))
            .collect(),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    for player in &PLAYERS[..3] {
        owner.configure_capture(*player, limits(128), SEED).unwrap();
    }
    owner.activate(chosen.host_origin).unwrap();
    let first = accepted(
        owner
            .process_input(
                input(chosen, 0, 0, 0, 4, 1),
                &SameDomain,
                output(chosen, 125_000_000),
            )
            .unwrap(),
    );
    assert_eq!(first[0].report.judge_events.len(), 2);
    assert!(first[0].report.audio_failures.is_empty());
    let error = owner
        .process_input(
            input(chosen, 1, 0, 0, 4, 1),
            &SameDomain,
            output(chosen, 125_000_000),
        )
        .unwrap_err();
    let StepLocalGameplayError::Operation {
        group_error: Some(core),
        reports,
        member_errors,
    } = error
    else {
        panic!("actual core committed prefix required")
    };
    assert_eq!(core.failed_player, Some(PLAYERS[1]));
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].player, PLAYERS[1]);
    assert_eq!(core.completed_reports.len(), 1);
    same_report(&reports[0].report, &core.completed_reports[0].report);
    assert_eq!(reports[0].report.judge_events.len(), 2);
    assert_eq!(reports[0].report.audio_failures.len(), 2);
    assert_eq!(reports[0].report.bound_inputs.len(), 2);
    assert!(member_errors.is_empty());
    assert!(owner.failed());
    assert_eq!(owner.score(PLAYERS[0]).unwrap().hits, 2);
    assert_eq!(owner.score(PLAYERS[1]).unwrap().hits, 2);
    assert_eq!(owner.score(PLAYERS[2]).unwrap().hits, 0);
    assert!(matches!(
        owner.process_input(
            input(chosen, 2, 0, 0, 4, 1),
            &SameDomain,
            chosen.output_origin
        ),
        Err(StepLocalGameplayError::Control(StepGameplayError::Failed))
    ));
    for (index, expected) in [2, 2, 0].into_iter().enumerate() {
        let file = decode_replay(
            &owner.take_replay(PLAYERS[index]).unwrap().unwrap(),
            limits(128),
        )
        .unwrap();
        assert_eq!(file.records.len(), expected);
        let replay = reconstruct_section(&prepared(chart, 8).source, file, limits(128)).unwrap();
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            owner.judge(PLAYERS[index]).unwrap().stable_hash().unwrap()
        );
    }

    let chosen = config();
    let (mut owner, _) = local(chart, 8, 3, chosen, 0, None, BmsInputMode::ButtonOnly);
    owner
        .configure_capture(PLAYERS[0], limits(1), SEED)
        .unwrap();
    for player in &PLAYERS[1..3] {
        owner.configure_capture(*player, limits(128), SEED).unwrap();
    }
    owner.activate(chosen.host_origin).unwrap();
    owner
        .advance_to(chosen.host_origin, &SameDomain, chosen.output_origin)
        .unwrap();
    let error = owner
        .advance_to(
            host(chosen, 0, 2_000_000_000),
            &SameDomain,
            output(chosen, 2_125_000_000),
        )
        .unwrap_err();
    let StepLocalGameplayError::Operation {
        group_error: None,
        reports,
        member_errors,
    } = error
    else {
        panic!("postprocessing failure retains common advance reports")
    };
    assert_eq!(
        reports.iter().map(|row| row.player).collect::<Vec<_>>(),
        PLAYERS[..3]
    );
    assert_eq!(member_errors.len(), 1);
    assert_eq!(member_errors[0].player, PLAYERS[0]);
    assert!(member_errors[0].score_error.is_none());
    assert!(matches!(
        &member_errors[0].capture_error,
        Some(CaptureError::Codec(ReplayCodecError::TooManyRecords))
    ));
    assert!(owner.failed());
    for (index, row) in reports.iter().enumerate() {
        assert_eq!(row.report.judge_events.len(), 2);
        assert_eq!(owner.score(row.player).unwrap().misses, 2);
        assert_eq!(
            owner.member_song_time(row.player),
            Some(row.report.song_time)
        );
        let file = decode_replay(
            &owner.take_replay(row.player).unwrap().unwrap(),
            limits(128),
        )
        .unwrap();
        assert_eq!(file.records.len(), if index == 0 { 1 } else { 2 });
        let replay = reconstruct_section(&prepared(chart, 8).source, file, limits(128)).unwrap();
        if index == 0 {
            assert!(
                replay.results().is_empty(),
                "refused capture does not invent the missed common advance"
            );
        } else {
            assert_eq!(replay.results(), row.report.judge_events);
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                owner.judge(row.player).unwrap().stable_hash().unwrap()
            );
        }
        assert!(owner.take_replay(row.player).unwrap().is_none());
    }
}

#[test]
fn pristine_unknown_sources_member_setup_shared_clock_and_partial_remote_ack_keep_their_boundaries()
{
    let chosen = StepGameplayConfig {
        preroll: Duration::ZERO,
        ..config()
    };
    let chart = "#BPM 60\n#00011:01\n#00112:01\n";
    assert!(
        StepLocalGameplay::new_section(
            prepared(chart, 8),
            chosen,
            plan(2),
            vec![bindings(Some(DEVICES[0]), false)],
            Timestamp::ZERO,
            None,
            BmsInputMode::ButtonOnly
        )
        .is_err()
    );
    let (mut owner, bank) = local(chart, 8, 3, chosen, 0, None, BmsInputMode::ButtonOnly);
    let mut unknown = input(chosen, 0, 0, 0, 4, 1);
    let PhysicalInputEvent::Button(button) = &mut unknown else {
        unreachable!()
    };
    button.meta.source = DeviceId(90);
    assert!(matches!(
        owner
            .process_input(unknown, &SameDomain, chosen.output_origin)
            .unwrap(),
        InputResult::Ignored {
            device: DeviceId(90)
        }
    ));
    assert!(owner.input_setup_available());
    assert!(!owner.failed());
    assert!(matches!(
        owner.configure_capture(PlayerId(99), limits(128), SEED),
        Err(StepLocalGameplayError::UnknownPlayer(PlayerId(99)))
    ));
    assert!(owner.score(PlayerId(99)).is_none());
    assert!(owner.judge(PlayerId(99)).is_none());
    assert!(matches!(
        owner.configure_touch_router(PLAYERS[0], router(0)),
        Err(StepLocalGameplayError::Control(
            StepGameplayError::InvalidConfiguration(_)
        ))
    ));
    owner
        .configure_output_clock(DisciplineConfig {
            capacity: 4,
            retention_interval: Duration::from_nanos(1_000_000_000),
            max_observation_age: Duration::from_nanos(1_000_000_000),
            ..DisciplineConfig::default()
        })
        .unwrap();
    for player in &PLAYERS[..3] {
        owner.configure_capture(*player, limits(128), SEED).unwrap();
    }
    assert!(matches!(
        owner.configure_capture(PLAYERS[0], limits(128), SEED),
        Err(StepLocalGameplayError::Control(
            StepGameplayError::InvalidConfiguration(_)
        ))
    ));
    assert!(matches!(
        owner.take_replay(PLAYERS[0]),
        Err(StepLocalGameplayError::Control(
            StepGameplayError::InvalidConfiguration(_)
        ))
    ));
    let identity = owner
        .competition_identity(PLAYERS[0], limits(128), SEED)
        .unwrap();
    assert_eq!(
        owner
            .competition_identity(PLAYERS[1], limits(128), SEED)
            .unwrap(),
        identity
    );
    owner.activate(chosen.host_origin).unwrap();
    assert!(!owner.input_setup_available());
    for index in 0..3 {
        owner
            .process_input(
                input(chosen, index, 0, 0, 4, 1),
                &SameDomain,
                chosen.output_origin,
            )
            .unwrap();
    }
    assert!(matches!(
        owner.competition_header(PLAYERS[0], limits(128), SEED),
        Err(StepLocalGameplayError::Control(
            StepGameplayError::InvalidConfiguration(_)
        ))
    ));
    owner
        .observe_output_clock(ClockPair {
            source: output(chosen, 1_000_100_000),
            target: host(chosen, 0, 1_000_000_000),
        })
        .unwrap();
    let first = host(chosen, 0, 1_000_000_000);
    let rows = owner
        .advance_to(first, &SameDomain, output(chosen, 1_000_000_000))
        .unwrap();
    assert!(
        rows.iter()
            .all(|row| row.report.song_time.as_nanos() == 1_000_000_000)
    );
    assert_eq!(
        owner.update_output_clock(first).unwrap(),
        Some(DisciplineUpdate::Warmup { span_ns: 0 })
    );
    owner
        .observe_output_clock(ClockPair {
            source: output(chosen, 2_000_200_000),
            target: host(chosen, 0, 2_000_000_000),
        })
        .unwrap();
    let second = host(chosen, 0, 2_000_000_000);
    let boundary = owner
        .advance_to(second, &SameDomain, output(chosen, 2_000_000_000))
        .unwrap();
    assert_eq!(
        owner.update_output_clock(second).unwrap(),
        Some(DisciplineUpdate::Applied {
            base_rate_ppm: 100,
            correction_ppm: 20,
            applied_rate_ppm: 120,
            phase_error_ns: 200_000,
            limited: false
        })
    );
    for row in boundary {
        assert_eq!(
            owner.member_song_time(row.player),
            Some(row.report.song_time)
        );
    }
    let rows = owner
        .advance_to(
            host(chosen, 0, 3_000_000_000),
            &SameDomain,
            output(chosen, 3_000_000_000),
        )
        .unwrap();
    assert!(
        rows.iter()
            .all(|row| row.report.song_time.as_nanos() == 3_000_120_000)
    );
    let batch = owner.take_commands(32).unwrap().unwrap();
    assert_eq!(batch.commands.len(), 3);
    let (mut producer, mut actual_mixer) = mixer(bank, chosen, None);
    producer.try_push(batch.commands[0]).unwrap();
    let error = owner.acknowledge(batch.sequence, 1, false).unwrap_err();
    assert!(
        matches!(error, StepLocalGameplayError::Control(StepGameplayError::AudioRejected { batch: actual, admitted: 1 }) if actual == batch)
    );
    let (_, actual) = render(&mut actual_mixer, 1);
    assert_eq!(actual.counters.commands_applied, 1);
    assert!(matches!(
        owner.acknowledge(batch.sequence, 1, false),
        Err(StepLocalGameplayError::Control(StepGameplayError::Failed))
    ));
    assert!(owner.failed());
    for player in &PLAYERS[..3] {
        assert_eq!(owner.score(*player).unwrap().hits, 1);
        let file =
            decode_replay(&owner.take_replay(*player).unwrap().unwrap(), limits(128)).unwrap();
        assert_eq!(
            file.records.last().unwrap().song_time.as_nanos(),
            3_000_120_000
        );
        let replay = reconstruct_section(&prepared(chart, 8).source, file, limits(128)).unwrap();
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            owner.judge(*player).unwrap().stable_hash().unwrap()
        );
        assert!(owner.take_replay(*player).unwrap().is_none());
    }
    let (finite_a, _) = local(
        chart,
        8,
        2,
        config(),
        0,
        Some(750_000_000),
        BmsInputMode::ButtonOnly,
    );
    let (finite_b, _) = local(
        chart,
        8,
        2,
        config(),
        0,
        Some(1_000_000_000),
        BmsInputMode::ButtonOnly,
    );
    assert_eq!(
        finite_a
            .competition_header(PLAYERS[0], limits(128), SEED)
            .unwrap(),
        finite_b
            .competition_header(PLAYERS[0], limits(128), SEED)
            .unwrap()
    );
    assert_ne!(
        finite_a
            .competition_identity(PLAYERS[0], limits(128), SEED)
            .unwrap(),
        finite_b
            .competition_identity(PLAYERS[0], limits(128), SEED)
            .unwrap()
    );
}
