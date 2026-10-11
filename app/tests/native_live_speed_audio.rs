//! Contract tests for the common native live-audio owner, without a device.
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId,
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::Rate,
};
use beatkernel_bms_runtime::{
    native_audio::{
        feed_rendered, prepare_audio, prepare_audio_with_speed, NativeAudioConfig,
        LIVE_COMMAND_RESERVE,
    },
    replay_speed::ReplaySpeed,
};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn speed(text: &str) -> ReplaySpeed {
    text.parse().unwrap()
}
fn bank(hz: u32) -> SampleBank {
    let format = AudioFormat::new(hz, 1).unwrap();
    let limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            vec![0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875],
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    bank
}
fn config() -> NativeAudioConfig {
    NativeAudioConfig {
        output_origin: ClockPoint {
            domain: ClockDomainId(72),
            timestamp: Timestamp::ZERO,
        },
        start: Timestamp::ZERO,
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(100_000_000),
        voices: 4,
        max_render_frames: 64,
        playback_end_frame: None,
        gated_start: false,
    }
}
fn play(at: i64, voice: u64) -> AudioCommand {
    AudioCommand::Play {
        at: ts(at),
        voice: VoiceId(voice),
        sample: SampleId(1),
        gain: 1.0,
    }
}
fn near(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (index, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert!((a - e).abs() < 0.000001, "PCM frame {index}: {a} != {e}");
    }
}

#[test]
fn actual_mixer_reads_pcm_at_four_literal_pitch_changing_rates() {
    for (fraction, expected) in [
        ("1/2", [0.0, 0.0625, 0.125, 0.1875]),
        ("1/1", [0.0, 0.125, 0.25, 0.375]),
        ("2/1", [0.0, 0.25, 0.5, 0.75]),
        ("3/2", [0.0, 0.1875, 0.375, 0.5625]),
    ] {
        let selected = speed(fraction);
        let mut audio =
            prepare_audio_with_speed(bank(1000), vec![play(0, 1)], config(), selected).unwrap();
        let admitted = audio.bgm.report().total_admitted;
        assert_eq!(admitted, if fraction == "1/1" { 1 } else { 2 });
        let mut output = [99.0; 4];
        let report = audio.mixer.render(&mut output).unwrap();
        near(&output, &expected);
        assert_eq!(audio.mixer.rate(), selected.rate());
        assert_eq!(report.counters.commands_applied, admitted as u64);
        assert_eq!(report.counters.commands_consumed, admitted as u64);
    }
}

#[test]
fn empty_bgm_still_sets_rate_before_a_raw_seventeen_ms_input_sound() {
    for fraction in ["1/2", "1/1", "2/1", "3/2"] {
        let selected = speed(fraction);
        let mut audio = prepare_audio_with_speed(bank(1000), vec![], config(), selected).unwrap();
        assert_eq!(
            audio.bgm.report().total_admitted,
            usize::from(fraction != "1/1")
        );
        audio.producer.try_push(play(17_000_000, 9)).unwrap();
        let mut output = [99.0; 20];
        let report = audio.mixer.render(&mut output).unwrap();
        assert_eq!(&output[..18], &[0.0; 18]);
        let expected = match fraction {
            "1/2" => 0.0625,
            "1/1" => 0.125,
            "2/1" => 0.25,
            _ => 0.1875,
        };
        assert_eq!(output[18], expected);
        assert_eq!(audio.mixer.rate(), selected.rate());
        assert_eq!(
            report.counters.commands_applied,
            if fraction == "1/1" { 1 } else { 2 }
        );
    }
}

#[test]
fn inverse_section_mapping_has_literal_nanoseconds_and_one_origin_preroll() {
    for (fraction, mapped) in [
        ("1/2", 11_000_000),
        ("1/1", 7_000_000),
        ("2/1", 5_000_000),
        ("3/2", 5_666_666),
    ] {
        let mut cfg = config();
        cfg.start = ts(20_000_000);
        cfg.preroll = Duration::from_nanos(3_000_000);
        cfg.output_origin.timestamp = ts(-100_000_000);
        cfg.lookahead = Duration::from_nanos(1);
        let mut audio = prepare_audio_with_speed(
            bank(1_000_000_000),
            vec![play(24_000_000, 1)],
            cfg,
            speed(fraction),
        )
        .unwrap();
        let mut commands = vec![];
        audio
            .bgm
            .feed(mapped as u64 - 1, 8, |command| {
                commands.push(command);
                Ok(())
            })
            .unwrap();
        assert_eq!(commands, vec![play(-100_000_000 + mapped, 1)]);
        assert_eq!(
            audio.bgm.report().total_admitted,
            if fraction == "1/1" { 1 } else { 2 }
        );
        assert_eq!(
            audio.bgm.config().max_pending,
            AudioLimits::MAX_COMMANDS - LIVE_COMMAND_RESERVE
        );
    }
}

#[test]
fn equal_time_original_order_survives_inverse_mapping() {
    let mut cfg = config();
    cfg.lookahead = Duration::from_nanos(1);
    let mut audio = prepare_audio_with_speed(
        bank(1000),
        vec![play(4_000_000, 3), play(4_000_000, 1), play(4_000_000, 2)],
        cfg,
        speed("2/1"),
    )
    .unwrap();
    let mut admitted = vec![];
    audio
        .bgm
        .feed(1, 8, |command| {
            admitted.push(command);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        admitted,
        vec![play(2_000_000, 3), play(2_000_000, 1), play(2_000_000, 2)]
    );
}

#[test]
fn finite_literal_endpoints_are_exclusive_and_retire_real_prefix_credits() {
    for (fraction, endpoint) in [("1/2", 11), ("1/1", 7), ("2/1", 5), ("3/2", 6)] {
        let mut cfg = config();
        cfg.start = ts(20_000_000);
        cfg.preroll = Duration::from_nanos(3_000_000);
        cfg.playback_end_frame = Some(endpoint);
        let mut audio = prepare_audio_with_speed(
            bank(1000),
            vec![play(20_000_000, 1), play(24_000_000, 2)],
            cfg,
            speed(fraction),
        )
        .unwrap();
        // The original end cue maps to the endpoint frame (ceil on this grid).
        let admitted = if fraction == "1/1" { 1 } else { 2 };
        assert_eq!(audio.bgm.report().total_admitted, admitted);
        let mut output = [99.0; 16];
        let report = audio.mixer.render(&mut output).unwrap();
        assert_eq!(report.playback_end_physical_frame, Some(endpoint));
        assert_eq!(report.playback_frames, endpoint as usize);
        assert!(report.paused);
        assert_eq!(
            &output[endpoint as usize..],
            &vec![0.0; 16 - endpoint as usize]
        );
        assert_eq!(report.counters.commands_consumed, admitted as u64);
        assert_eq!(report.counters.commands_applied, admitted as u64);
        feed_rendered(&mut audio.bgm, Some(report), |command| {
            audio.producer.try_push(command)
        })
        .unwrap();
        assert_eq!(audio.bgm.report().remaining, 0);
        assert_eq!(audio.bgm.report().outstanding, 0);
    }
}

#[test]
fn zero_frame_fence_admits_neither_rate_nor_valid_bgm() {
    let mut cfg = config();
    cfg.playback_end_frame = Some(0);
    let mut audio =
        prepare_audio_with_speed(bank(1000), vec![play(0, 1)], cfg, speed("2/1")).unwrap();
    assert_eq!(audio.bgm.report().total_admitted, 0);
    let mut output = [99.0; 4];
    let report = audio.mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0; 4]);
    assert_eq!(report.counters.commands_consumed, 0);
    assert_eq!(report.playback_end_physical_frame, Some(0));
    assert_eq!(audio.mixer.rate(), Rate::NORMAL);
}

#[test]
fn real_start_gate_holds_rate_and_commands_until_scheduled_physical_frame() {
    let mut cfg = config();
    cfg.gated_start = true;
    let mut audio =
        prepare_audio_with_speed(bank(1000), vec![play(0, 1)], cfg, speed("2/1")).unwrap();
    let mut held = [99.0; 3];
    let report = audio.mixer.render(&mut held).unwrap();
    assert_eq!(held, [0.0; 3]);
    assert_eq!(report.counters.commands_consumed, 0);
    assert_eq!(audio.mixer.rate(), Rate::NORMAL);
    audio.producer.schedule_start_at(5).unwrap();
    let mut output = [99.0; 6];
    let report = audio.mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0, 0.0, 0.0, 0.25, 0.5, 0.75]);
    assert_eq!(report.counters.commands_applied, 2);
    assert_eq!(audio.producer.applied_start_frame(), Some(5));
}

#[test]
fn real_stop_is_unscaled_and_does_not_receive_rate_prefix_stop_credit() {
    let mut audio =
        prepare_audio_with_speed(bank(1000), vec![play(0, 1)], config(), speed("1/2")).unwrap();
    assert_eq!(audio.bgm.admitted_stops(), 0);
    audio
        .producer
        .try_push(AudioCommand::Stop {
            voice: VoiceId(1),
            at: ts(3_000_000),
        })
        .unwrap();
    let mut output = [99.0; 6];
    let report = audio.mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0, 0.0625, 0.125, 0.0, 0.0, 0.0]);
    assert_eq!(report.counters.commands_applied, 3);
    assert_eq!(report.counters.unknown_stops, 0);
    assert_eq!(audio.bgm.admitted_stops(), 0);
}

#[test]
fn normalized_normal_speed_preserves_legacy_pcm_credits_and_config() {
    let mut cfg = config();
    cfg.start = ts(20_000_000);
    cfg.preroll = Duration::from_nanos(3_000_000);
    cfg.output_origin.timestamp = ts(-100_000_000);
    let commands = vec![play(20_000_000, 1), play(24_000_000, 2)];
    let mut legacy = prepare_audio(bank(1000), commands.clone(), cfg).unwrap();
    let mut selected = prepare_audio_with_speed(bank(1000), commands, cfg, speed("2/2")).unwrap();
    assert_eq!(legacy.bgm.config(), selected.bgm.config());
    assert_eq!(legacy.bgm.report(), selected.bgm.report());
    let mut a = [99.0; 16];
    let mut b = [99.0; 16];
    let ra = legacy.mixer.render(&mut a).unwrap();
    let rb = selected.mixer.render(&mut b).unwrap();
    assert_eq!(a, b);
    assert_eq!(ra, rb);
}

#[test]
fn malformed_original_commands_refuse_even_when_fence_is_zero() {
    let mut cfg = config();
    cfg.start = ts(20_000_000);
    cfg.playback_end_frame = Some(0);
    let mut nan = play(20_000_000, 1);
    if let AudioCommand::Play { gain, .. } = &mut nan {
        *gain = f32::NAN;
    }
    for command in [
        play(19_999_999, 1),
        nan,
        AudioCommand::Stop {
            voice: VoiceId(1),
            at: ts(20_000_000),
        },
        AudioCommand::SetRate {
            at: ts(20_000_000),
            rate: Rate::NORMAL,
        },
    ] {
        assert!(prepare_audio_with_speed(bank(1000), vec![command], cfg, speed("2/1")).is_err());
    }
}

#[test]
fn negative_start_preroll_and_unrepresentable_origin_mapping_refuse() {
    let mut negative_start = config();
    negative_start.start = ts(-1);
    assert!(prepare_audio_with_speed(bank(1000), vec![], negative_start, speed("2/1")).is_err());
    let mut negative_preroll = config();
    negative_preroll.preroll = Duration::from_nanos(-1);
    assert!(prepare_audio_with_speed(bank(1000), vec![], negative_preroll, speed("2/1")).is_err());
    let mut overflow = config();
    overflow.output_origin.timestamp = ts(i64::MAX);
    assert!(
        prepare_audio_with_speed(bank(1000), vec![play(2, 1)], overflow, speed("2/1")).is_err()
    );
}
