use super::*;
use crate::bgm::{BgmConfig, BgmFeeder};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        PcmSample, SampleBank, SampleId, VoiceId,
    },
    time::{ClockDomainId, Duration},
};
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(22),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn setup(rate: u32, end: u64) -> (beatkernel::audio::CommandProducer, Mixer, BgmFeeder) {
    let format = AudioFormat::new(rate, 1).unwrap();
    let limits = PcmLimits::new(1024, 4096, 4).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 100], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut feeder = BgmFeeder::from_output_commands(
        vec![AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        BgmConfig {
            output_origin: point(0),
            sample_rate: rate,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(1_000_000_000),
            max_pending: 8,
        },
    )
    .unwrap();
    feeder
        .feed(0, 8, |command| producer.try_push(command))
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(22),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        )
        .with_playback_end_frame(end),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer, feeder)
}
#[test]
fn actual_pause_shifted_endpoint_waits_for_native_marker_and_freezes_unfinished_voices() {
    let (mut producer, mut mixer, mut feeder) = setup(10, 3);
    let mut completion = FiniteReplayCompletion::new(point(0), 10, 3).unwrap();
    let first = mixer.render(&mut [0.0; 2]).unwrap();
    assert!(!completion
        .observe(true, &feeder, Some(first), Some(point(100_000_000)))
        .unwrap());
    producer.request_pause(true);
    let paused = mixer.render(&mut [0.0; 3]).unwrap();
    assert!(paused.paused);
    assert_eq!(paused.playback_end_physical_frame, None);
    assert!(!completion
        .observe(true, &feeder, Some(paused), Some(point(500_000_000)))
        .unwrap());
    producer.request_pause(false);
    let mut pcm = [-1.0; 2];
    let terminal = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.0]);
    assert_eq!(terminal.playback_end_physical_frame, Some(6));
    assert_eq!(terminal.active_voices, 1);
    assert!(!completion
        .observe(false, &feeder, Some(terminal), Some(point(599_999_999)))
        .unwrap());
    assert_eq!(feeder.report().outstanding, 1);
    feeder
        .retire_completed(terminal.playback_start_frame + terminal.playback_frames as u64)
        .unwrap();
    assert_eq!(feeder.report().outstanding, 0);
    assert!(!completion
        .observe(true, &feeder, Some(terminal), Some(point(599_999_999)))
        .unwrap());
    assert!(!completion
        .observe(false, &feeder, None, Some(point(600_000_000)))
        .unwrap());
    assert!(completion
        .observe(true, &feeder, None, Some(point(600_000_000)))
        .unwrap());
    let silence = mixer.render(&mut [0.0; 3]).unwrap();
    assert!(completion
        .observe(true, &feeder, Some(silence), Some(point(600_000_000)))
        .unwrap());
    let before = completion.clone();
    for bad in [
        RenderReport {
            playback_end_physical_frame: Some(7),
            ..silence
        },
        RenderReport {
            producer_disconnected: true,
            ..silence
        },
        RenderReport {
            active_voices: 0,
            ..silence
        },
        RenderReport {
            frames: 0,
            ..silence
        },
        RenderReport {
            playback_frames: 1,
            ..silence
        },
    ] {
        assert!(completion
            .observe(true, &feeder, Some(bad), Some(point(600_000_000)))
            .is_err());
        assert_eq!(completion, before);
    }
    assert!(completion
        .observe(true, &feeder, None, Some(point(599_999_999)))
        .is_err());
    assert_eq!(completion, before);
    assert!(completion
        .observe(
            true,
            &feeder,
            None,
            Some(ClockPoint {
                domain: ClockDomainId(99),
                timestamp: Timestamp::ZERO
            })
        )
        .is_err());
    assert_eq!(completion, before);
}

#[test]
fn real_pending_terminal_expiry_obeys_shared_output_conservation_without_fabricated_ack() {
    let (mut producer, mut mixer, _feeder) = setup(1000, 3);
    // This independent direct producer owns a real future command. Public
    // cold Step batches filter such targets; this is report-validator evidence,
    // not a claim that Step acknowledged a command outside its endpoint.
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(8_000_000),
            gain: 1.0,
        })
        .unwrap();
    let marker = mixer.render(&mut [0.0; 5]).unwrap();
    assert_eq!(marker.playback_end_physical_frame, Some(3));
    assert_eq!(marker.pending_commands, 1);
    assert_eq!(marker.counters.commands_consumed, 2);
    assert_eq!(marker.counters.commands_applied, 1);
    assert_eq!(marker.counters.commands_expired, 0);
    let stops = crate::offline::OwnedStopEvidence::default();
    crate::step_gameplay::validate_section_output_evidence_with_stops(
        point(0),
        1000,
        None,
        None,
        Some(marker),
        None,
        Some(3),
        &stops,
    )
    .unwrap();
    let mut pcm = [1.0; 1];
    let settled = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0]);
    assert_eq!(settled.pending_commands, 0);
    assert_eq!(settled.counters.commands_consumed, 2);
    assert_eq!(settled.counters.commands_applied, 1);
    assert_eq!(settled.counters.commands_expired, 1);
    assert_eq!(settled.song_position, marker.song_position);
    assert_eq!(settled.active_voices, marker.active_voices);
    assert!(crate::step_gameplay::finite_settlement_progression(
        marker, settled
    ));
    crate::step_gameplay::validate_section_output_evidence_with_stops(
        point(0),
        1000,
        Some(marker),
        None,
        Some(settled),
        None,
        Some(3),
        &stops,
    )
    .unwrap();
    for case in 0..9 {
        let mut forged = settled;
        match case {
            0 => forged.counters.commands_expired = 0,
            1 => forged.counters.commands_consumed += 1,
            2 => forged.pending_commands = 2,
            3 => forged.counters.commands_applied += 1,
            4 => forged.counters.late_commands += 1,
            5 => forged.playback_end_physical_frame = None,
            6 => forged.song_position = Timestamp::from_nanos(forged.song_position.as_nanos() + 1),
            7 => forged.active_voices += 1,
            8 => forged.counters.commands_expired = u64::MAX,
            _ => unreachable!(),
        }
        assert!(
            crate::step_gameplay::validate_section_output_evidence_with_stops(
                point(0),
                1000,
                Some(marker),
                None,
                Some(forged),
                None,
                Some(3),
                &stops,
            )
            .is_err(),
            "case {case}"
        );
    }
}
#[test]
fn fractional_endpoint_requires_exact_admission_render_and_presentation_evidence() {
    let (_producer, mut mixer, mut feeder) = setup(3, 1);
    let terminal = mixer.render(&mut [0.0; 2]).unwrap();
    let mut completion = FiniteReplayCompletion::new(point(0), 3, 1).unwrap();
    assert!(!completion.observe(true, &feeder, None, None).unwrap());
    let mut bad = terminal;
    bad.counters.commands_consumed = 2;
    bad.counters.commands_applied = 2;
    assert!(completion.observe(true, &feeder, Some(bad), None).is_err());
    assert_eq!(completion.last_render, None);
    feeder
        .retire_completed(terminal.playback_start_frame + terminal.playback_frames as u64)
        .unwrap();
    assert!(!completion
        .observe(true, &feeder, Some(terminal), Some(point(333_333_333)))
        .unwrap());
    assert!(completion
        .observe(true, &feeder, None, Some(point(333_333_334)))
        .unwrap());
    assert!(completion
        .observe(true, &feeder, None, Some(point(666_666_668)))
        .is_err());
    assert!(FiniteReplayCompletion::new(point(0), 0, 1).is_err());
    assert!(FiniteReplayCompletion::new(point(i64::MAX), 3, 1).is_err());
    assert!(FiniteReplayCompletion::new(point(0), 1, u64::MAX).is_err());
}
