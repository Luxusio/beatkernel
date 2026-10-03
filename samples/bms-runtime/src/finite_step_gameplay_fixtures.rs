//! Actual portable live owner, section PCM, core input reports and finite output.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    bgm::BgmFeedError,
    prepare_from_source,
    replay_playback::{decode_section_setup, reconstruct_section},
    section_start::{prepare_at, prepare_section_replay},
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError},
    step_replay::{StepReplay, StepReplayConfig},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        RenderReport, SampleBank, SampleId, command_queue,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId,
        DeviceSelector, EventMeta, GameControlId, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent,
    },
    interaction::InteractionState,
    judge::JudgeOutcome,
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, ReplayFile, decode_replay, encode_replay},
    },
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
};

const SIMPLE: &str = "#BPM 60\n#00011:01\n";
const WITH_MUSIC: &str = "#BPM 60\n#00011:01\n#00112:01\n#00001:02\n#00101:02\n";
const SECTION: &str = "#BPM 60\n#00011:00010000\n#00012:00010000\n#00013:00000100\n#00054:01000100\n#00001:02020000\n#00001:0000000200000000\n";

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
        preroll: Duration::from_nanos(250_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 16,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(2_000_000_000),
        telemetry_capacity: 8,
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65_536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn pcm_limits() -> PcmLimits {
    PcmLimits::new(4096, 16_384, 16).unwrap()
}
fn bindings() -> BindingMap {
    BindingMap::from_bindings((0u16..3).map(|index| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4 + index),
        game_control: GameControlId(u32::from(0x11 + index)),
    }))
    .unwrap()
}
fn wav(rate: u32, values: &[i16]) -> Vec<u8> {
    let size = u32::try_from(values.len() * 2).unwrap();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}
fn prepared(chart: &str, rate: u32, seed: u64) -> PreparedBms {
    let chart = format!("#VOLWAV 50\n#LNTYPE 1\n#WAV01 key.wav\n#WAV02 music.wav\n{chart}");
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files
        .insert("pack/key.wav", wav(rate, &[8192, -4096, 2048, 1024]))
        .unwrap();
    let music = (1i16..=32).map(|value| value * 512).collect::<Vec<_>>();
    files.insert("pack/music.wav", wav(rate, &music)).unwrap();
    prepare_from_source(
        chart.as_bytes(),
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(rate, 1).unwrap(),
        pcm_limits(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        seed,
        None,
    )
    .unwrap()
}
fn game(
    chart: &str,
    rate: u32,
    seed: u64,
    chosen: StepGameplayConfig,
    start: i64,
    end: i64,
) -> (StepGameplay, SampleBank) {
    let (selected, _) = prepare_at(
        prepared(chart, rate, seed),
        Timestamp::from_nanos(start),
        pcm_limits(),
    )
    .unwrap();
    StepGameplay::new_section(
        selected,
        chosen,
        bindings(),
        Timestamp::from_nanos(start),
        Some(Timestamp::from_nanos(end)),
    )
    .unwrap()
}
fn clocks(chosen: StepGameplayConfig) -> AffineClockMapper {
    AffineClockMapper::exact_offset(
        ClockPair {
            source: chosen.host_origin,
            target: chosen.output_origin,
        },
        ClockInterval {
            start: chosen.host_origin.timestamp,
            end: chosen
                .host_origin
                .timestamp
                .checked_add(Duration::from_nanos(20_000_000_000))
                .unwrap(),
        },
    )
    .unwrap()
}
fn host(chosen: StepGameplayConfig, start: i64, song: i64) -> ClockPoint {
    point(
        chosen.host_origin.domain.0,
        chosen.host_origin.timestamp.as_nanos() + chosen.preroll.as_nanos() + song - start,
    )
}
fn output(chosen: StepGameplayConfig, elapsed: i64) -> ClockPoint {
    point(
        chosen.output_origin.domain.0,
        chosen.output_origin.timestamp.as_nanos() + elapsed,
    )
}
fn input(
    chosen: StepGameplayConfig,
    start: i64,
    song: i64,
    key: u16,
    sequence: u64,
) -> PhysicalInputEvent {
    let at = host(chosen, start, song);
    let mut meta = EventMeta::new(DeviceId(77), at, sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(7),
        code: Some(42),
        timestamp: Some(at),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(key),
        state: ButtonState::Down,
    })
}
fn advance(
    game: &mut StepGameplay,
    chosen: StepGameplayConfig,
    start: i64,
    song: i64,
) -> beatkernel::runtime::RuntimeReport {
    game.advance_to(
        host(chosen, start, song),
        &clocks(chosen),
        output(chosen, song - start + chosen.preroll.as_nanos()),
    )
    .unwrap()
}
fn hit(
    game: &mut StepGameplay,
    chosen: StepGameplayConfig,
    start: i64,
    song: i64,
    key: u16,
    sequence: u64,
) -> beatkernel::runtime::RuntimeReport {
    game.process_input(
        input(chosen, start, song, key, sequence),
        &clocks(chosen),
        output(chosen, song - start + chosen.preroll.as_nanos()),
    )
    .unwrap()
}
fn mixer(
    bank: SampleBank,
    chosen: StepGameplayConfig,
    end: Option<u64>,
) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(64).unwrap();
    let mut config = MixerConfig::new(
        bank.format(),
        chosen.output_origin.domain,
        chosen.output_origin.timestamp,
        AudioLimits::new(64, 16, 64, 64, 64).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn render(mixer: &mut Mixer, frames: usize) -> (Vec<f32>, RenderReport) {
    let mut pcm = vec![0.0; frames];
    let report = mixer.render(&mut pcm).unwrap();
    (pcm, report)
}
fn deliver(game: &mut StepGameplay, producer: &mut CommandProducer) -> Vec<AudioCommand> {
    let mut commands = Vec::new();
    while let Some(batch) = game.take_commands(16).unwrap() {
        for command in &batch.commands {
            producer.try_push(*command).unwrap();
        }
        game.acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        commands.extend(batch.commands);
    }
    commands
}
fn recording(game: &mut StepGameplay) -> ReplayFile {
    game.fail();
    let bytes = game.take_replay().unwrap().unwrap();
    let file = decode_replay(&bytes, limits()).unwrap();
    assert_eq!(encode_replay(&file, limits()).unwrap(), bytes);
    assert!(game.take_replay().unwrap().is_none());
    file
}
fn fenced(game: &mut StepGameplay) {
    let score = game.score().clone();
    let song = game.song_time();
    let hash = game.judge().stable_hash().unwrap();
    assert!(game.failed());
    assert!(matches!(
        game.take_commands(1),
        Err(StepGameplayError::Failed)
    ));
    assert!(matches!(
        game.observe_completion(None, None),
        Err(StepGameplayError::Failed)
    ));
    assert_eq!(game.score(), &score);
    assert_eq!(game.song_time(), song);
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
}

#[test]
fn finite_live_section_preserves_original_pcm_judgments_and_recorded_replay_across_callback_partitions()
 {
    let chosen = config();
    let seed = u64::MAX;
    let start = 1_000_000_000;
    let end = 1_500_000_000;
    for blocks in [vec![1, 1, 3, 4], vec![6, 3], vec![9]] {
        let original = prepared(SECTION, 8, seed);
        let (selected, selection) =
            prepare_at(original, Timestamp::from_nanos(start), pcm_limits()).unwrap();
        assert_eq!(selection.excluded_crossing_holds, 1);
        assert_eq!(
            selected.bank.get(SampleId(3)).unwrap().samples(),
            &selected.bank.get(SampleId(2)).unwrap().samples()[8..]
        );
        let future = selected
            .compiled
            .chart
            .objects()
            .iter()
            .find(|object| object.time.start.as_nanos() == 2_000_000_000)
            .unwrap()
            .id;
        let (mut owner, bank) = StepGameplay::new_section(
            selected,
            chosen,
            bindings(),
            Timestamp::from_nanos(start),
            Some(Timestamp::from_nanos(end)),
        )
        .unwrap();
        assert_eq!(owner.end_ns(), Some(end));
        assert_eq!(owner.playback_end_frame(), Some(6));
        owner.configure_capture(limits(), seed).unwrap();
        owner.activate(chosen.host_origin).unwrap();
        let mut events = advance(&mut owner, chosen, start, 750_000_000).judge_events;
        for (key, seq) in [(4, 1), (5, 2)] {
            events.extend(hit(&mut owner, chosen, start, start, key, seq).judge_events);
        }
        let terminal = advance(&mut owner, chosen, start, 2_000_000_000);
        assert!(terminal.song_end_reached);
        assert_eq!(terminal.song_time.as_nanos(), end);
        events.extend(terminal.judge_events);
        assert_eq!(events.len(), 2);
        assert_eq!(owner.judge().state(future), Some(InteractionState::Pending));
        let (mut producer, mut output_mixer) = mixer(bank, chosen, Some(6));
        let commands = deliver(&mut owner, &mut producer);
        assert_eq!(commands.len(), 4);
        let samples = commands
            .iter()
            .map(|command| match command {
                AudioCommand::Play { sample, at, .. } => {
                    assert_eq!(*at, output(chosen, 250_000_000).timestamp);
                    *sample
                }
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            samples,
            [SampleId(3), SampleId(2), SampleId(1), SampleId(1)]
        );
        let mut pcm = Vec::new();
        let mut frames = 0;
        for block in blocks {
            let (values, report) = render(&mut output_mixer, block);
            frames += block;
            owner
                .observe_completion(
                    Some(report),
                    Some(output(chosen, frames as i64 * 125_000_000)),
                )
                .unwrap();
            owner.feed_audio(frames as u64, 16).unwrap();
            assert!(deliver(&mut owner, &mut producer).is_empty());
            if frames >= 6 {
                assert!(report.active_voices > 0);
            }
            pcm.extend(values);
        }
        assert!(owner.observe_completion(None, None).unwrap());
        let expected = [
            0.0,
            0.0,
            (9 * 512 + 512 + 16384) as f32 / 65536.0,
            (10 * 512 + 2 * 512 - 8192) as f32 / 65536.0,
            (11 * 512 + 3 * 512 + 4096) as f32 / 65536.0,
            (12 * 512 + 4 * 512 + 2048) as f32 / 65536.0,
            0.0,
            0.0,
            0.0,
        ];
        assert_eq!(pcm, expected);
        let hash = owner.judge().stable_hash().unwrap();
        let score = owner.score().clone();
        let file = recording(&mut owner);
        let setup = decode_section_setup(&file.header.options).unwrap();
        assert_eq!(
            (
                setup.start.as_nanos(),
                setup.end.unwrap().as_nanos(),
                setup.chart_seed
            ),
            (start, end, seed)
        );
        assert_eq!(
            file.records
                .iter()
                .map(|record| record.song_time.as_nanos())
                .collect::<Vec<_>>(),
            [750_000_000, start, start, end]
        );
        let original = prepared(SECTION, 8, seed);
        let rebuilt = reconstruct_section(&original.source, file.clone(), limits()).unwrap();
        assert_eq!(rebuilt.results(), events);
        assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
        let selected = prepare_section_replay(original, &file, limits(), pcm_limits()).unwrap();
        let replay_config = StepReplayConfig {
            output_origin: chosen.output_origin,
            preroll: chosen.preroll,
            lookahead: chosen.bgm_lookahead,
            max_pending: 16,
        };
        let (mut replay, bank) = StepReplay::new(selected, file, limits(), replay_config).unwrap();
        let (mut producer, mut output_mixer) = mixer(bank, chosen, replay.playback_end_frame());
        let batch = replay.take_commands(16).unwrap().unwrap();
        assert_eq!(batch.commands, commands);
        for command in &batch.commands {
            producer.try_push(*command).unwrap();
        }
        replay
            .acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        let (replayed_pcm, report) = render(&mut output_mixer, 9);
        assert_eq!(replayed_pcm, pcm);
        assert!(
            !replay
                .observe_output(Some(report), Some(output(chosen, 1_125_000_000)))
                .unwrap()
        );
        assert!(replay.take_commands(16).unwrap().is_none());
        assert!(replay.observe_output(None, None).unwrap());
        assert_eq!(replay.drain_events(), events);
        assert_eq!(replay.score(), &score);
    }
}

#[test]
fn endpoint_inputs_remain_acquisition_checked_while_offset_hits_and_filtered_audio_stay_in_the_capture()
 {
    let chart = "#BPM 80\n#00011:00010000\n#00012:00000100\n#00001:00020000\n";
    let chosen = StepGameplayConfig {
        preroll: Duration::from_nanos(100_000_000),
        offset_ns: 1,
        ..config()
    };
    let (mut owner, bank) = game(chart, 3, 9, chosen, 0, 750_000_000);
    owner.configure_capture(limits(), 9).unwrap();
    advance(&mut owner, chosen, 0, -100_000_000);
    let event = input(chosen, 0, 749_999_999, 4, 1);
    let accepted = owner
        .process_input(event.clone(), &clocks(chosen), output(chosen, 849_999_999))
        .unwrap();
    assert_eq!(accepted.input, Some(event.clone()));
    assert_eq!(accepted.bound_inputs[0].physical, event);
    assert!(!accepted.song_end_reached);
    assert_eq!(accepted.audio_commands.len(), 1);
    assert!(matches!(
        accepted.judge_events[0].outcome,
        JudgeOutcome::Hit {
            delta: Duration::ZERO,
            ..
        }
    ));
    assert!(
        owner.take_commands(1).unwrap().is_none(),
        "the actual hit remains recorded while its rounded frame equals the exclusive fence"
    );
    assert_eq!(
        owner.bgm_report().total_admitted,
        0,
        "BGM at the logical endpoint is not published"
    );
    for (song, sequence) in [(750_000_000, 2), (2_000_000_000, 3)] {
        let report = hit(&mut owner, chosen, 0, song, 5, sequence);
        assert!(report.song_end_reached);
        assert_eq!(report.song_time.as_nanos(), 750_000_000);
        assert!(report.input.is_none());
        assert!(report.bound_inputs.is_empty());
        assert!(report.judge_events.is_empty());
        assert!(report.audio_commands.is_empty());
    }
    assert_eq!(owner.score().hits, 1);
    assert_eq!(owner.score().misses, 0);
    let (_producer, mut output_mixer) = mixer(bank, chosen, Some(3));
    let (pcm, marker) = render(&mut output_mixer, 4);
    assert_eq!(pcm, [0.0; 4]);
    assert!(
        !owner
            .observe_completion(Some(marker), Some(output(chosen, 850_000_000)))
            .unwrap()
    );
    assert!(
        owner
            .observe_completion(None, Some(output(chosen, 1_000_000_000)))
            .unwrap()
    );
    let hash = owner.judge().stable_hash().unwrap();
    let rejected = owner.process_input(
        input(chosen, 0, 3_000_000_000, 5, 0),
        &clocks(chosen),
        output(chosen, 3_100_000_000),
    );
    assert!(
        matches!(rejected, Err(StepGameplayError::Runtime(_))),
        "sequence validation remains active after the logical cap"
    );
    fenced(&mut owner);
    let file = recording(&mut owner);
    assert_eq!(file.records.len(), 4);
    assert!(
        matches!(&file.records[1].operation, ReplayOperation::Input(input) if input.physical == event)
    );
    assert!(
        file.records[2..]
            .iter()
            .all(|record| record.song_time.as_nanos() == 750_000_000
                && matches!(record.operation, ReplayOperation::Advance))
    );
    let replay = reconstruct_section(&prepared(chart, 3, 9).source, file, limits()).unwrap();
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    assert_eq!(replay.results(), accepted.judge_events);
}

#[test]
fn completion_waits_for_logical_end_presentation_ack_bgm_credits_and_every_filtered_queue_entry() {
    let chosen = config();
    let (mut owner, bank) = game(WITH_MUSIC, 8, 0, chosen, 0, 1_000_000_000);
    hit(&mut owner, chosen, 0, 0, 4, 1);
    let batch = owner.take_commands(16).unwrap().unwrap();
    assert_eq!(batch.commands.len(), 2);
    let (mut producer, mut output_mixer) = mixer(bank, chosen, Some(10));
    for command in &batch.commands {
        producer.try_push(*command).unwrap();
    }
    let (_, marker) = render(&mut output_mixer, 12);
    assert!(!owner.observe_completion(Some(marker), None).unwrap());
    assert_eq!(
        owner.song_time(),
        Timestamp::ZERO,
        "render evidence does not advance the live judge"
    );
    advance(&mut owner, chosen, 0, 2_000_000_000);
    assert!(!owner.observe_completion(None, None).unwrap());
    assert!(
        !owner
            .observe_completion(Some(marker), Some(output(chosen, 1_500_000_000)))
            .unwrap()
    );
    owner.acknowledge(batch.sequence, 2, true).unwrap();
    assert!(!owner.observe_completion(None, None).unwrap());
    assert_eq!(owner.bgm_report().outstanding, 1);
    owner.feed_audio(12, 16).unwrap();
    assert!(owner.observe_completion(None, None).unwrap());
    let chart = "#BPM 80\n#00011:00010000\n#00012:00010000\n";
    let chosen = StepGameplayConfig {
        preroll: Duration::from_nanos(100_000_000),
        offset_ns: 1,
        ..chosen
    };
    let (mut owner, bank) = game(chart, 3, 0, chosen, 0, 750_000_000);
    hit(&mut owner, chosen, 0, 749_999_999, 4, 1);
    hit(&mut owner, chosen, 0, 749_999_999, 5, 2);
    advance(&mut owner, chosen, 0, 750_000_000);
    let (_producer, mut output_mixer) = mixer(bank, chosen, Some(3));
    let (_, marker) = render(&mut output_mixer, 4);
    assert!(
        owner.take_commands(1).unwrap().is_none(),
        "an all-filtered snapshot creates no empty batch or ACK"
    );
    assert!(
        owner
            .observe_completion(Some(marker), Some(output(chosen, 1_000_000_000)))
            .unwrap(),
        "the bounded queue scan removes both filtered entries even when the eligible batch limit is one"
    );
    assert!(owner.take_commands(1).unwrap().is_none());
    assert_eq!(owner.score().hits, 2);

    let (mut owner, bank) = game(chart, 3, 0, chosen, 0, 750_000_000);
    let filtered = hit(&mut owner, chosen, 0, 749_999_999, 4, 1);
    // Scheduling evidence is supplied independently from acquisition/song time.
    let eligible = owner
        .process_input(
            input(chosen, 0, 749_999_999, 5, 2),
            &clocks(chosen),
            output(chosen, 600_000_000),
        )
        .unwrap();
    assert_eq!(filtered.song_time, eligible.song_time);
    assert_eq!(eligible.song_time.as_nanos(), 749_999_999);
    assert_eq!(filtered.audio_commands.len(), 1);
    assert_eq!(eligible.audio_commands.len(), 1);
    assert_eq!(
        filtered.audio_commands[0].at(),
        output(chosen, 849_999_999).timestamp
    );
    assert_eq!(
        eligible.audio_commands[0].at(),
        output(chosen, 600_000_000).timestamp
    );
    advance(&mut owner, chosen, 0, 750_000_000);
    let batch = owner.take_commands(1).unwrap().unwrap();
    assert_eq!(
        batch.commands, eligible.audio_commands,
        "the eligible tail must survive an excluded prefix without an empty batch or retimestamping"
    );
    let (mut producer, mut output_mixer) = mixer(bank, chosen, Some(3));
    producer.try_push(batch.commands[0]).unwrap();
    let (pcm, marker) = render(&mut output_mixer, 4);
    assert_eq!(pcm, [0.0, 0.0, 0.125, 0.0]);
    assert_eq!(marker.counters.commands_consumed, 1);
    assert_eq!(marker.counters.commands_applied, 1);
    assert_eq!(marker.counters.late_commands, 0);
    assert!(
        !owner
            .observe_completion(Some(marker), Some(output(chosen, 1_000_000_000)))
            .unwrap()
    );
    owner.acknowledge(batch.sequence, 1, true).unwrap();
    assert!(owner.take_commands(1).unwrap().is_none());
    assert!(owner.observe_completion(None, None).unwrap());
    assert_eq!(
        owner.score().hits,
        2,
        "audio filtering does not discard either actual judgment"
    );
}

#[test]
fn failed_admission_and_invalid_execution_evidence_fence_without_erasing_committed_judgments() {
    let chosen = config();
    for late_ack in [false, true] {
        let (mut owner, bank) = game(WITH_MUSIC, 8, 0, chosen, 0, 1_000_000_000);
        owner.configure_capture(limits(), 0).unwrap();
        let hit_report = hit(&mut owner, chosen, 0, 0, 4, 1);
        advance(&mut owner, chosen, 0, 1_000_000_000);
        let batch = owner.take_commands(16).unwrap().unwrap();
        let expected = batch.clone();
        let (mut producer, mut output_mixer) = mixer(bank, chosen, Some(10));
        if late_ack {
            let (_, marker) = render(&mut output_mixer, 12);
            assert!(
                !owner
                    .observe_completion(Some(marker), Some(output(chosen, 1_500_000_000)))
                    .unwrap()
            );
            for command in &batch.commands {
                producer.try_push(*command).unwrap();
            }
            owner
                .acknowledge(batch.sequence, batch.commands.len(), true)
                .unwrap();
            owner.feed_audio(12, 16).unwrap();
            let (_, frozen) = render(&mut output_mixer, 1);
            assert_eq!(frozen.counters.commands_consumed, 0);
            assert!(
                matches!(owner.observe_completion(Some(frozen), None), Err(StepGameplayError::Completion { rendered: Some(actual), .. }) if actual == frozen)
            );
        } else {
            producer.try_push(batch.commands[0]).unwrap();
            assert!(
                matches!(owner.acknowledge(batch.sequence, 1, false), Err(StepGameplayError::AudioRejected { batch, admitted: 1 }) if batch == expected)
            );
            let (_, report) = render(&mut output_mixer, 12);
            assert_eq!(report.counters.commands_applied, 1);
        }
        assert_eq!(owner.score().hits, 1);
        fenced(&mut owner);
        let file = recording(&mut owner);
        let rebuilt =
            reconstruct_section(&prepared(WITH_MUSIC, 8, 0).source, file, limits()).unwrap();
        assert_eq!(rebuilt.results(), hit_report.judge_events);
    }
    for corruption in 0..4 {
        let (mut owner, bank) = game(SIMPLE, 8, 0, chosen, 0, 1_000_000_000);
        hit(&mut owner, chosen, 0, 0, 4, 1);
        advance(&mut owner, chosen, 0, 1_000_000_000);
        let (mut producer, mut output_mixer) = mixer(bank, chosen, Some(10));
        deliver(&mut owner, &mut producer);
        let (_, first) = render(&mut output_mixer, 4);
        owner
            .observe_completion(Some(first), Some(output(chosen, 500_000_000)))
            .unwrap();
        let (_, mut bad) = render(&mut output_mixer, 8);
        let mut presentation = output(chosen, 1_500_000_000);
        match corruption {
            0 => bad.playback_end_physical_frame = Some(11),
            1 => presentation.domain = ClockDomainId(99),
            2 => {
                bad.counters.late_commands += 1;
            }
            3 => {
                bad = first;
                bad.active_voices += 1;
            }
            _ => unreachable!(),
        }
        let hash = owner.judge().stable_hash().unwrap();
        let score = owner.score().clone();
        assert!(matches!(
            owner.observe_completion(Some(bad), Some(presentation)),
            Err(StepGameplayError::Completion { .. })
        ));
        assert_eq!(owner.score(), &score);
        assert_eq!(owner.judge().stable_hash().unwrap(), hash);
        fenced(&mut owner);
    }
}

#[test]
fn finite_construction_ignores_unreachable_full_song_extents_but_checks_section_and_output_bounds()
{
    let distant = "#BPM 0.0001\n#00011:01\n#99912:01\n#99901:02\n";
    let chosen = StepGameplayConfig {
        host_origin: point(11, 1),
        output_origin: point(22, 5),
        preroll: Duration::from_nanos(7_000_000_000_000_000_000),
        ..config()
    };
    assert!(
        StepGameplay::new(prepared(distant, 1, 0), chosen, bindings()).is_err(),
        "the ordinary full-song calibration exceeds signed time"
    );
    let (mut finite, _) = StepGameplay::new_section(
        prepared(distant, 1, 0),
        chosen,
        bindings(),
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(1)),
    )
    .unwrap();
    assert_eq!(finite.playback_end_frame(), Some(7_000_000_001));
    assert_eq!(finite.bgm_report().remaining, 0);
    assert_eq!(finite.bgm_report().total_admitted, 0);
    assert!(
        finite.take_commands(16).unwrap().is_none(),
        "unreachable future BGM is removed before its output mapping could overflow"
    );
    assert!(
        finite
            .judge()
            .chart()
            .objects()
            .iter()
            .all(|object| finite.judge().state(object.id) == Some(InteractionState::Pending))
    );
    for (start, end) in [(0, 0), (0, -1), (1, 1), (2, 1), (-1, 1)] {
        assert!(
            StepGameplay::new_section(
                prepared(SIMPLE, 8, 0),
                config(),
                bindings(),
                Timestamp::from_nanos(start),
                Some(Timestamp::from_nanos(end))
            )
            .is_err()
        );
    }
    assert!(
        StepGameplay::new_section(
            prepared(SECTION, 8, 0),
            config(),
            bindings(),
            Timestamp::from_nanos(1_000_000_000),
            Some(Timestamp::from_nanos(1_500_000_000))
        )
        .is_err(),
        "a positive section must already exclude crossing heads and select original PCM"
    );
    let overflow = StepGameplayConfig {
        output_origin: point(22, i64::MAX - 1),
        preroll: Duration::ZERO,
        ..config()
    };
    assert!(matches!(
        StepGameplay::new_section(
            prepared(SIMPLE, 8, 0),
            overflow,
            bindings(),
            Timestamp::ZERO,
            Some(Timestamp::from_nanos(1_000_000_000))
        ),
        Err(StepGameplayError::Completion {
            rendered: None,
            presented: None,
            ..
        })
    ));
    for invalid_gain in [
        None,
        Some(f32::NAN),
        Some(f32::INFINITY),
        Some(f32::NEG_INFINITY),
    ] {
        let mut original = prepared(WITH_MUSIC, 8, 0);
        let end = Timestamp::from_nanos(1_000_000_000);
        let index = original
            .bgm_commands
            .iter()
            .position(|command| command.at() >= end)
            .unwrap();
        let AudioCommand::Play {
            voice, sample, at, ..
        } = original.bgm_commands[index]
        else {
            unreachable!()
        };
        let invalid = match invalid_gain {
            Some(gain) => AudioCommand::Play {
                voice,
                sample,
                at,
                gain,
            },
            None => AudioCommand::Stop { voice, at },
        };
        original.bgm_commands[index] = invalid;
        let count = original.bgm_commands.len();
        let result =
            StepGameplay::new_section(original, config(), bindings(), Timestamp::ZERO, Some(end));
        let Err(StepGameplayError::Bgm {
            error: BgmFeedError::InvalidCommand(actual),
            report,
        }) = result
        else {
            panic!("malformed future BGM must be rejected before finite target exclusion");
        };
        assert_eq!(report.total_admitted, 0);
        assert_eq!(report.admitted, 0);
        assert_eq!(report.remaining, count);
        assert_eq!(actual.at(), at);
        match (actual, invalid_gain) {
            (AudioCommand::Stop { voice: found, .. }, None) => assert_eq!(found, voice),
            (
                AudioCommand::Play {
                    voice: found,
                    sample: found_sample,
                    gain,
                    ..
                },
                Some(expected),
            ) => {
                assert_eq!(found, voice);
                assert_eq!(found_sample, sample);
                assert_eq!(
                    gain.to_bits(),
                    expected.to_bits(),
                    "the rejected nonfinite payload is retained exactly"
                );
            }
            _ => panic!("BGM refusal changed the caller's original command variant"),
        }
    }
}

#[test]
fn absent_end_keeps_legacy_capture_and_completion_while_finite_competition_identity_retains_its_endpoint()
 {
    let chosen = config();
    let seed = u64::MAX;
    let mut bytes = Vec::new();
    let mut identities = Vec::new();
    for explicit in [false, true] {
        let original = prepared(SIMPLE, 8, seed);
        let (mut owner, bank) = if explicit {
            StepGameplay::new_section(original, chosen, bindings(), Timestamp::ZERO, None).unwrap()
        } else {
            StepGameplay::new(original, chosen, bindings()).unwrap()
        };
        assert_eq!(owner.end_ns(), None);
        assert_eq!(owner.playback_end_frame(), None);
        identities.push(owner.competition_identity(limits(), seed).unwrap());
        owner.configure_capture(limits(), seed).unwrap();
        hit(&mut owner, chosen, 0, 0, 4, 1);
        advance(&mut owner, chosen, 0, 1_000_000_000);
        let (mut producer, mut output_mixer) = mixer(bank, chosen, None);
        deliver(&mut owner, &mut producer);
        let (_, first) = render(&mut output_mixer, 12);
        assert!(
            !owner
                .observe_completion(Some(first), Some(output(chosen, 1_500_000_000)))
                .unwrap()
        );
        let (_, idle) = render(&mut output_mixer, 1);
        assert!(
            owner
                .observe_completion(Some(idle), Some(output(chosen, 1_625_000_000)))
                .unwrap()
        );
        let file = recording(&mut owner);
        assert_eq!(
            decode_section_setup(&file.header.options).unwrap().end,
            None
        );
        bytes.push(encode_replay(&file, limits()).unwrap());
    }
    assert_eq!(bytes[0], bytes[1]);
    assert_eq!(identities[0], identities[1]);
    let mut finite_identities = Vec::new();
    for end in [1_000_000_000, 2_000_000_000] {
        let (mut owner, _) = game(SIMPLE, 8, seed, chosen, 0, end);
        let header = owner.competition_header(limits(), seed).unwrap();
        assert_eq!(
            decode_section_setup(&header.options).unwrap().end,
            None,
            "the existing normalized identity basis remains legacy metadata"
        );
        let identity = owner.competition_identity(limits(), seed).unwrap();
        assert_eq!(
            identity,
            crate::multiplayer::competition_identity_for_section(
                &header,
                env!("CARGO_PKG_VERSION"),
                limits(),
                Some(Timestamp::from_nanos(end))
            )
            .unwrap()
        );
        owner.configure_capture(limits(), seed).unwrap();
        assert_eq!(
            owner.competition_identity(limits(), seed).unwrap(),
            identity
        );
        let file = recording(&mut owner);
        assert_eq!(
            decode_section_setup(&file.header.options).unwrap().end,
            Some(Timestamp::from_nanos(end))
        );
        finite_identities.push(identity);
    }
    assert_ne!(finite_identities[0], finite_identities[1]);
    assert_ne!(finite_identities[0], identities[0]);
}
