//! Public captured replay, actual remote admission and Mixer evidence.
//! Presented points are controlled port evidence, not physical-device claims.
#[path = "support/replay_speed.rs"]
mod fixture;

use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioLimits, CommandProducer, Mixer, MixerConfig,
        RenderReport, SampleBank, VoiceId,
    },
    input::ButtonState,
    replay::codec::{decode_replay, encode_replay, ReplayFile},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::Rate,
};
use beatkernel_bms_runtime::{
    gauge::GaugeProfile,
    replay_audio::plan_section_audio,
    section_start::{prepare_at, prepare_section_replay},
    step_gameplay::StepGameplayError,
    step_replay::{StepReplay, StepReplayConfig, StepReplayError},
    PreparedBms,
};
use fixture::*;

const MUSIC: &str = "#BPM 60\n#WAV01 music.wav\n#00001:01\n";
fn music() -> PreparedBms {
    prepared(
        MUSIC,
        4,
        &[(
            1,
            &[0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5],
        )],
    )
}
fn config() -> StepReplayConfig {
    StepReplayConfig {
        output_origin: ClockPoint {
            domain: ClockDomainId(73),
            timestamp: Timestamp::from_nanos(604_800_000_000_017),
        },
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(20_000_000_000),
        max_pending: 32,
    }
}
fn shown(c: StepReplayConfig, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: c.output_origin.domain,
        timestamp: Timestamp::from_nanos(c.output_origin.timestamp.as_nanos() + ns),
    }
}
fn owner(
    p: PreparedBms,
    file: &ReplayFile,
    c: StepReplayConfig,
    rate: Rate,
) -> (StepReplay, SampleBank) {
    // Codec round trip pins real original metadata instead of manufacturing it.
    let bytes = encode_replay(file, limits()).unwrap();
    let file = decode_replay(&bytes, limits()).unwrap();
    let selected = prepare_section_replay(p, &file, limits(), pcm_limits()).unwrap();
    StepReplay::new_with_rate(selected, file, limits(), c, rate).unwrap()
}
fn mixer(
    bank: SampleBank,
    c: StepReplayConfig,
    end: Option<u64>,
    capacity: usize,
) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut cfg = MixerConfig::new(
        bank.format(),
        c.output_origin.domain,
        c.output_origin.timestamp,
        AudioLimits::new(capacity, 16, 64, 64, capacity).unwrap(),
    );
    if let Some(end) = end {
        cfg = cfg.with_playback_end_frame(end);
    }
    (producer, Mixer::new(cfg, bank, consumer).unwrap())
}
fn deliver(owner: &mut StepReplay, producer: &mut CommandProducer) -> Vec<AudioCommand> {
    let mut commands = Vec::new();
    while let Some(batch) = owner.take_commands(32).unwrap() {
        for &command in &batch.commands {
            producer.try_push(command).unwrap();
        }
        owner
            .acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        commands.extend(batch.commands);
    }
    commands
}
fn render(output: &mut Mixer, frames: usize) -> (Vec<f32>, RenderReport) {
    let mut pcm = vec![0.0; frames];
    let report = output.render(&mut pcm).unwrap();
    (pcm, report)
}
fn fenced(owner: &mut StepReplay) {
    assert!(owner.failed());
    assert!(matches!(
        owner.take_commands(1),
        Err(StepReplayError::Failed)
    ));
    assert!(matches!(
        owner.observe_output(None, None),
        Err(StepReplayError::Failed)
    ));
}

#[test]
fn actual_rational_music_pcm_and_original_progress_are_partition_independent() {
    // Literal source-head interpolation at 4 Hz, including the held final sample.
    let cases = [
        (
            1,
            2,
            vec![
                0.0625, 0.09375, 0.125, 0.15625, 0.1875, 0.21875, 0.25, 0.28125, 0.3125, 0.34375,
                0.375, 0.40625, 0.4375, 0.46875, 0.5, 0.5,
            ],
            3_000_000_000,
        ),
        (
            1,
            1,
            vec![0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5],
            6_000_000_000,
        ),
        (2, 1, vec![0.0625, 0.1875, 0.3125, 0.4375], 12_000_000_000),
        (
            3,
            2,
            vec![0.0625, 0.15625, 0.25, 0.34375, 0.4375, 0.5],
            9_000_000_000,
        ),
    ];
    let file = captured(&music(), 0, Some(2_000_000_000));
    let bytes = encode_replay(&file, limits()).unwrap();
    for (num, den, mut expected, song_ns) in cases {
        expected.resize(24, 0.0);
        for block in [1, 2, 3, 7, 24] {
            let rate = Rate::new(num, den).unwrap();
            let c = config();
            let (mut replay, bank) = owner(music(), &file, c, rate);
            assert_eq!(replay.playback_rate(), rate);
            let (mut producer, mut output) = mixer(bank, c, None, 32);
            let commands = deliver(&mut replay, &mut producer);
            assert_eq!(commands.len(), if rate == Rate::NORMAL { 1 } else { 2 });
            if rate != Rate::NORMAL {
                assert!(
                    matches!(commands[0], AudioCommand::SetRate { rate: r, at } if r == rate && at == c.output_origin.timestamp)
                );
            }
            let mut pcm = Vec::new();
            while pcm.len() < 24 {
                let n = block.min(24 - pcm.len());
                let (part, report) = render(&mut output, n);
                pcm.extend(part);
                replay
                    .observe_output(Some(report), Some(shown(c, pcm.len() as i64 * 250_000_000)))
                    .unwrap();
                assert!(deliver(&mut replay, &mut producer).is_empty());
            }
            assert_eq!(pcm, expected, "{num}/{den} block {block}");
            assert_eq!(replay.song_time().as_nanos(), song_ns);
            assert_eq!(
                replay.recorded_until(),
                Some(Timestamp::from_nanos(2_000_000_000))
            );
            assert_eq!(replay.score().hits, 0);
        }
    }
    assert_eq!(encode_replay(&file, limits()).unwrap(), bytes);
}

#[test]
fn old_constructor_and_reduced_normal_preserve_commands_pcm_and_events() {
    let p = main_prepared();
    let file = captured(&p, 17, Some(4_000_000_001));
    let bytes = encode_replay(&file, limits()).unwrap();
    let baseline = plan_section_audio(
        &p,
        file.clone(),
        limits(),
        config().output_origin,
        Duration::ZERO,
    )
    .unwrap();
    let mut results = Vec::new();
    for mode in 0..3 {
        let selected =
            prepare_section_replay(main_prepared(), &file, limits(), pcm_limits()).unwrap();
        let (mut replay, bank) = if mode == 0 {
            StepReplay::new(selected, file.clone(), limits(), config()).unwrap()
        } else {
            StepReplay::new_with_rate(
                selected,
                file.clone(),
                limits(),
                config(),
                Rate::new(if mode == 1 { 1 } else { 8 }, if mode == 1 { 1 } else { 8 }).unwrap(),
            )
            .unwrap()
        };
        assert_eq!(replay.playback_rate(), Rate::NORMAL);
        let (mut producer, mut output) = mixer(bank, config(), None, 32);
        let commands = deliver(&mut replay, &mut producer);
        assert_eq!(commands, baseline.commands);
        let (pcm, report) = render(&mut output, 40);
        replay
            .observe_output(Some(report), Some(shown(config(), 5_000_000_000)))
            .unwrap();
        let events = replay.drain_events();
        assert_eq!(events, baseline.judge_events);
        assert_eq!(replay.score().hits, 3);
        results.push((pcm, events, replay.score().clone()));
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(results[0], results[2]);
    assert_eq!(encode_replay(&file, limits()).unwrap(), bytes);
}

#[test]
fn nonnormal_judges_keep_original_times_and_recorded_offset() {
    let file = captured(&main_prepared(), 17, Some(4_000_000_001));
    let reference = plan_section_audio(
        &main_prepared(),
        file.clone(),
        limits(),
        config().output_origin,
        Duration::ZERO,
    )
    .unwrap();
    let bytes = encode_replay(&file, limits()).unwrap();
    for rate in [
        Rate::new(1, 2).unwrap(),
        Rate::new(2, 1).unwrap(),
        Rate::new(3, 2).unwrap(),
    ] {
        let (mut replay, bank) = owner(main_prepared(), &file, config(), rate);
        let (mut producer, mut output) = mixer(bank, config(), None, 32);
        deliver(&mut replay, &mut producer);
        // The source is 8 Hz: each legal 40-frame callback presents another
        // five output seconds without exceeding the rig's 64-frame limit.
        for presented_ns in [5_000_000_000, 10_000_000_000] {
            let (_, report) = render(&mut output, 40);
            replay
                .observe_output(Some(report), Some(shown(config(), presented_ns)))
                .unwrap();
            assert!(deliver(&mut replay, &mut producer).is_empty());
        }
        assert_eq!(replay.drain_events(), reference.judge_events);
        assert_eq!(replay.score().hits, 3);
        assert_eq!(replay.recorded_until(), reference.recorded_until);
        assert_eq!(encode_replay(&file, limits()).unwrap(), bytes);
    }
}

#[test]
fn original_section_bgm_suffix_is_selected_once_and_output_preroll_is_unscaled() {
    let start = Timestamp::from_nanos(1_000_000_000);
    let (selected, _) = prepare_at(music(), start, pcm_limits()).unwrap();
    let file = capture_events(
        &selected,
        0,
        start,
        Some(Timestamp::from_nanos(2_000_000_000)),
        &GaugeProfile::default(),
        &[],
        Some(2_000_000_000),
    );
    let c = StepReplayConfig {
        preroll: Duration::from_nanos(500_000_000),
        ..config()
    };
    let (mut replay, bank) = owner(music(), &file, c, Rate::new(2, 1).unwrap());
    assert_eq!(replay.song_time().as_nanos(), 0);
    assert_eq!(replay.playback_end_frame(), Some(4));
    let (mut producer, mut output) = mixer(bank, c, Some(4), 32);
    deliver(&mut replay, &mut producer);
    let (pcm, report) = render(&mut output, 6);
    assert_eq!(pcm, [0.0, 0.0, 0.3125, 0.4375, 0.0, 0.0]);
    assert!(!replay.observe_output(Some(report), None).unwrap());
    assert_eq!(replay.song_time().as_nanos(), 0);
    assert!(deliver(&mut replay, &mut producer).is_empty());
    assert!(replay
        .observe_output(None, Some(shown(c, 1_500_000_000)))
        .unwrap());
    assert_eq!(replay.song_time().as_nanos(), 2_000_000_000);
}

#[test]
fn finite_rate_fences_require_actual_render_presentation_and_retired_credit() {
    let file = captured_at(
        &music(),
        0,
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(1_000_000_000)),
        Some(1_000_000_000),
    );
    for (num, den, endpoint) in [(1, 2, 8), (1, 1, 4), (2, 1, 2), (3, 2, 3)] {
        let c = config();
        let (mut replay, bank) = owner(music(), &file, c, Rate::new(num, den).unwrap());
        assert_eq!(replay.playback_end_frame(), Some(endpoint));
        let (mut producer, mut output) = mixer(bank, c, Some(endpoint), 32);
        deliver(&mut replay, &mut producer);
        let (_, report) = render(&mut output, endpoint as usize + 2);
        assert!(!replay.observe_output(Some(report), None).unwrap());
        assert_eq!(replay.song_time(), Timestamp::ZERO);
        assert!(!replay
            .observe_output(None, Some(shown(c, endpoint as i64 * 250_000_000 - 1)))
            .unwrap());
        assert!(deliver(&mut replay, &mut producer).is_empty());
        assert!(replay
            .observe_output(None, Some(shown(c, endpoint as i64 * 250_000_000)))
            .unwrap());
        assert_eq!(replay.song_time().as_nanos(), 1_000_000_000);
        assert_eq!(replay.bgm_report().outstanding, 0);
    }
}

#[test]
fn zero_quantized_fence_finishes_original_records_only_after_genuine_zero_marker() {
    let p = prepared("#BPM 60\n", 4, &[]);
    let file = captured_at(
        &p,
        0,
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(1)),
        Some(1),
    );
    let (mut replay, bank) = owner(p, &file, config(), Rate::new(i64::MAX, 1).unwrap());
    assert_eq!(replay.playback_end_frame(), Some(0));
    assert!(replay.take_commands(32).unwrap().is_none());
    assert!(!replay.observe_output(None, None).unwrap());
    assert_eq!(replay.song_time(), Timestamp::ZERO);
    let (_producer, mut output) = mixer(bank, config(), Some(0), 32);
    let (pcm, report) = render(&mut output, 1);
    assert_eq!(pcm, [0.0]);
    assert_eq!(report.playback_end_physical_frame, Some(0));
    assert_eq!(report.counters.commands_applied, 0);
    assert!(!replay.observe_output(Some(report), None).unwrap());
    assert_eq!(replay.song_time(), Timestamp::ZERO);
    assert!(replay
        .observe_output(None, Some(shown(config(), 0)))
        .unwrap());
    assert_eq!(replay.song_time().as_nanos(), 1);
}

#[test]
fn empty_and_prefix_logs_do_not_fabricate_misses_at_fast_finite_end() {
    let make = || {
        prepared(
            "#BPM 60\n#WAV01 key.wav\n#00011:01000100\n",
            4,
            &[(1, &[0.25, 0.5])],
        )
    };
    for prefix in [false, true] {
        let p = make();
        let events = if prefix {
            vec![(0, 0x11, ButtonState::Down)]
        } else {
            vec![]
        };
        let file = capture_events(
            &p,
            0,
            Timestamp::ZERO,
            Some(Timestamp::from_nanos(3_000_000_000)),
            &GaugeProfile::default(),
            &events,
            None,
        );
        let (mut replay, bank) = owner(make(), &file, config(), Rate::new(2, 1).unwrap());
        let (mut producer, mut output) = mixer(bank, config(), Some(6), 32);
        deliver(&mut replay, &mut producer);
        let (_, report) = render(&mut output, 8);
        replay
            .observe_output(Some(report), Some(shown(config(), 2_000_000_000)))
            .unwrap();
        deliver(&mut replay, &mut producer);
        assert!(replay.observe_output(None, None).unwrap());
        assert_eq!(replay.score().hits, u64::from(prefix));
        assert_eq!(replay.score().misses, 0);
        assert_eq!(replay.recorded_until(), prefix.then_some(Timestamp::ZERO));
    }
}

#[test]
fn wrong_presentation_identity_fences_without_committing_record_progress() {
    let file = captured(&main_prepared(), 0, Some(4_000_000_001));
    let (mut replay, bank) = owner(main_prepared(), &file, config(), Rate::new(2, 1).unwrap());
    let (mut producer, mut output) = mixer(bank, config(), None, 32);
    deliver(&mut replay, &mut producer);
    let (_, report) = render(&mut output, 24);
    let mut wrong = shown(config(), 3_000_000_000);
    wrong.domain = ClockDomainId(74);
    assert!(matches!(
        replay.observe_output(Some(report), Some(wrong)),
        Err(StepReplayError::Output { .. })
    ));
    assert_eq!(replay.score().hits, 0);
    assert_eq!(replay.song_time(), Timestamp::ZERO);
    assert!(replay.drain_events().is_empty());
    fenced(&mut replay);
}

#[test]
fn selected_rate_is_real_queue_credit_and_partial_ack_retains_control_only_prefix() {
    let make = || {
        prepared(
            "#BPM 60\n#WAV01 key.wav\n#00011:01\n",
            4,
            &[(1, &[0.25, 0.5])],
        )
    };
    let file = captured(&make(), 0, None);
    let (mut replay, bank) = owner(make(), &file, config(), Rate::new(2, 1).unwrap());
    let (mut producer, mut output) = mixer(bank, config(), None, 1);
    let batch = replay.take_commands(32).unwrap().unwrap();
    assert_eq!(batch.commands.len(), 2);
    assert!(matches!(batch.commands[0], AudioCommand::SetRate { .. }));
    assert!(matches!(
        replay.take_commands(32),
        Err(StepReplayError::OutstandingBatch { .. })
    ));
    producer.try_push(batch.commands[0]).unwrap();
    let rejected = producer.try_push(batch.commands[1]).unwrap_err();
    assert_eq!(rejected.command, batch.commands[1]);
    assert!(matches!(
        replay.acknowledge(batch.sequence, 1, false),
        Err(StepReplayError::Acknowledgement(
            StepGameplayError::AudioRejected { admitted: 1, .. }
        ))
    ));
    let (pcm, report) = render(&mut output, 4);
    assert_eq!(pcm, [0.0; 4]);
    assert_eq!(report.counters.commands_applied, 1);
    assert_eq!(report.counters.unknown_stops, 0);
    assert_eq!(replay.acknowledged_stop_commands(), 0);
    fenced(&mut replay);
}

#[test]
fn rate_control_does_not_authorize_unowned_stop_diagnostics() {
    let file = captured(&music(), 0, Some(1_000_000_000));
    let (mut replay, bank) = owner(music(), &file, config(), Rate::new(2, 1).unwrap());
    let (mut producer, mut output) = mixer(bank, config(), None, 32);
    deliver(&mut replay, &mut producer);
    producer
        .try_push(AudioCommand::Stop {
            voice: VoiceId(999),
            at: config().output_origin.timestamp,
        })
        .unwrap();
    let (_, report) = render(&mut output, 4);
    assert_eq!(report.counters.unknown_stops, 1);
    assert_eq!(replay.acknowledged_stop_commands(), 0);
    assert!(matches!(
        replay.observe_output(Some(report), Some(shown(config(), 1_000_000_000))),
        Err(StepReplayError::Audio { .. }) | Err(StepReplayError::Output { .. })
    ));
    assert_eq!(replay.song_time(), Timestamp::ZERO);
    fenced(&mut replay);
}

#[test]
fn actual_policy_failure_stops_earn_only_the_exact_acknowledged_prefix() {
    let make = || {
        prepared(
            "#BPM 60\n#WAV01 key.wav\n#00011:01000100\n",
            4,
            &[(1, &[0.25, 0.5])],
        )
    };
    let policy = GaugeProfile::new(1, 0, -1, -1, true, vec![]).unwrap();
    let file = capture_events(
        &make(),
        0,
        Timestamp::ZERO,
        None,
        &policy,
        &[(0, 0x11, ButtonState::Down)],
        None,
    );
    let (mut replay, bank) = owner(make(), &file, config(), Rate::new(2, 1).unwrap());
    let (mut producer, mut output) = mixer(bank, config(), None, 32);
    let batch = replay.take_commands(32).unwrap().unwrap();
    let stops = batch
        .commands
        .iter()
        .filter(|c| matches!(c, AudioCommand::Stop { .. }))
        .count() as u64;
    assert!(
        stops > 0,
        "actual recorded depleted gauge produces owned Stops"
    );
    assert_eq!(replay.acknowledged_stop_commands(), 0);
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    replay
        .acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    assert_eq!(replay.acknowledged_stop_commands(), stops);
    let (_, report) = render(&mut output, 4);
    assert!(report.counters.unknown_stops <= stops);
    replay
        .observe_output(Some(report), Some(shown(config(), 1_000_000_000)))
        .unwrap();
    assert_eq!(replay.score().hits, 1);
    assert!(replay.gauge().snapshot().failure.is_some());
}

#[test]
fn invalid_speed_and_bad_ack_are_typed_and_cannot_commit_progress() {
    let file = captured(&music(), 0, None);
    for rate in [Rate::ZERO, Rate::REVERSE] {
        assert!(
            StepReplay::new_with_rate(music(), file.clone(), limits(), config(), rate).is_err()
        );
    }
    let (mut replay, _bank) = owner(music(), &file, config(), Rate::new(3, 2).unwrap());
    let batch = replay.take_commands(32).unwrap().unwrap();
    assert!(matches!(
        replay.acknowledge(batch.sequence + 1, batch.commands.len(), true),
        Err(StepReplayError::Acknowledgement(
            StepGameplayError::InvalidAcknowledgement { .. }
        ))
    ));
    assert_eq!(replay.score().hits, 0);
    assert_eq!(replay.acknowledged_stop_commands(), 0);
    fenced(&mut replay);
}
