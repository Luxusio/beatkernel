//! Deferred actual replay PCM and closed-owner Stop evidence; no native output claim.
use crate::{
    mine_audio_consumers_fixtures::{data, recorded, replay_limits, Action},
    offline::{
        render_block, render_block_with_stops, OfflineError, OfflineOptions, OfflineReport,
        OwnedStopEvidence,
    },
    replay_render::render_replay,
    PreparedBms,
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig,
        PcmLimits, PcmSample, QueuePushError, SampleBank, SampleId, VoiceId,
    },
    time::{ClockDomainId, Duration, Timestamp},
};
use beatkernel_bms::BmsInputMode;
use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};

const SECOND: i64 = 1_000_000_000;
const FATAL: &str = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 note\n#WAV02 music\n#00011:01000101\n#00031:02\n#000D1:00ZZ0000\n#000D2:00000001\n#00001:02000002";
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn options(frames: u64, block_frames: usize, command_capacity: usize) -> OfflineOptions {
    OfflineOptions {
        frames,
        block_frames,
        command_capacity,
        max_voices: 16,
    }
}
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn stop(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(at),
    }
}
fn floats(bytes: &[u8]) -> Vec<f32> {
    assert_eq!(bytes.len() % 4, 0);
    bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}
fn prepared() -> PreparedBms {
    let mut prepared = data(FATAL, true);
    let format = AudioFormat::new(10, 1).unwrap();
    let limits = PcmLimits::new(256, 2048, 8).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    for (id, pcm) in [
        (0, vec![0.5, -0.5]),
        (1, vec![1.0, -1.0]),
        (2, vec![0.25; 40]),
    ] {
        bank.insert(SampleId(id), PcmSample::new(format, pcm, limits).unwrap())
            .unwrap();
    }
    prepared.bank = bank;
    prepared
}
fn actions() -> [Action; 8] {
    [
        Action::Bgm(0),
        Action::Press(0, 91),
        Action::Advance(SECOND),
        Action::Release(SECOND, 91),
        Action::Press(2 * SECOND, 91),
        Action::Release(2 * SECOND, 91),
        Action::Bgm(1),
        Action::Press(3 * SECOND, 91),
    ]
}

#[test]
fn fatal_legacy_log_keeps_full_hash_and_hits_while_pcm_preserves_bgm_across_blocks() {
    let legacy = recorded(
        &prepared(),
        &actions(),
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    assert_eq!(legacy.reports[1].hazard_events[0].value, 1295);
    assert!(legacy.commands.contains(&play(1, 12, 3 * SECOND, 0.5)));
    assert!(
        legacy.commands.contains(&play(1, 13, 4 * SECOND, 0.5)),
        "the actual legacy Runtime continued after the recorded failure"
    );
    for block in [1usize, 3, 7, 64] {
        let mut bytes = Vec::new();
        let report = render_replay(
            prepared(),
            legacy.file.clone(),
            replay_limits(),
            options(37, block, 16),
            Duration::ZERO,
            &mut bytes,
        )
        .unwrap();
        let mut expected = vec![0.125; 37];
        expected[0] = 0.625;
        expected[1] = -0.375;
        expected[30..].fill(0.25);
        assert_eq!(floats(&bytes), expected);
        assert_eq!(
            (report.frames, report.hits, report.judge_results),
            (37, 3, 3)
        );
        assert_eq!(
            report.commands_admitted, 8,
            "two BGM, one gameplay Play and five unique Stops"
        );
        assert_eq!(report.recorded_until, Some(ts(3 * SECOND)));
        assert_eq!(report.final_judge_hash, legacy.hash);
        let last = report.last_render.unwrap();
        assert_eq!(last.start_frame + last.frames as u64, 37);
        assert_eq!(last.counters.commands_applied, 8);
        assert_eq!(
            last.counters.unknown_stops, 5,
            "the short head ended; future heads, press and nonfatal-mine voices never played"
        );
        assert_eq!(last.counters.unknown_samples, 0);
        assert_eq!(last.counters.late_commands, 0);
        assert_eq!(last.counters.voice_full, 0);
    }
}

#[test]
fn partial_stop_queue_refusal_keeps_original_command_and_written_prefix_without_retry() {
    let legacy = recorded(
        &prepared(),
        &actions(),
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    for (capacity, refused) in [(2usize, 13u64), (3, 92), (4, 93)] {
        let mut bytes = Vec::new();
        let error = render_replay(
            prepared(),
            legacy.file.clone(),
            replay_limits(),
            options(37, 3, capacity),
            Duration::ZERO,
            &mut bytes,
        )
        .unwrap_err();
        let failure = error.downcast_ref::<OfflineError>().unwrap();
        assert_eq!(
            floats(&bytes),
            [
                0.625, -0.375, 0.125, 0.125, 0.125, 0.125, 0.125, 0.125, 0.125, 0.125
            ]
        );
        assert_eq!(
            failure.audio_failures.len(),
            1,
            "replay returns the first actual refused command instead of retrying its prefix"
        );
        assert_eq!(failure.audio_failures[0].command, stop(refused, SECOND));
        assert_eq!(failure.audio_failures[0].reason, QueuePushError::Full);
        let last = failure.last_render.unwrap();
        assert_eq!(last.start_frame + last.frames as u64, 10);
        assert_eq!(last.counters.commands_applied, 2);
        assert_eq!(
            last.counters.unknown_stops, 0,
            "the accepted Stop prefix is queued, not yet executed on this failure path"
        );
        assert!(failure.message.contains("admission"));
    }
}

#[test]
fn exclusive_output_cutoffs_and_preroll_leave_full_log_evidence_and_legacy_pcm_intact() {
    let legacy = recorded(
        &prepared(),
        &actions(),
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    // At 10 Hz, .15 seconds of preroll rounds first commands to frame 2 and Stops to 12.
    for (frames, preroll, first, admitted, unknown) in [
        (0u64, 0i64, 0usize, 0usize, 0u64),
        (10, 0, 0, 2, 0),
        (11, 0, 0, 7, 5),
        (11, 100_000_000, 1, 2, 0),
        (12, 100_000_000, 1, 7, 5),
        (12, 150_000_000, 2, 2, 0),
        (13, 150_000_000, 2, 7, 5),
    ] {
        let mut bytes = Vec::new();
        let report = render_replay(
            prepared(),
            legacy.file.clone(),
            replay_limits(),
            options(frames, 4, 16),
            Duration::from_nanos(preroll),
            &mut bytes,
        )
        .unwrap();
        let mut expected = vec![0.0; frames as usize];
        if frames != 0 {
            expected[first..].fill(0.125);
            expected[first] = 0.625;
            expected[first + 1] = -0.375;
        }
        assert_eq!(floats(&bytes), expected);
        assert_eq!(report.commands_admitted, admitted);
        assert_eq!(
            (report.frames, report.hits, report.judge_results),
            (frames, 3, 3)
        );
        assert_eq!(report.recorded_until, Some(ts(3 * SECOND)));
        assert_eq!(report.final_judge_hash, legacy.hash);
        if frames == 0 {
            assert!(report.last_render.is_none());
        } else {
            let last = report.last_render.unwrap();
            assert_eq!(last.counters.commands_applied, admitted as u64);
            assert_eq!(last.counters.unknown_stops, unknown);
        }
    }
    let mut untouched = vec![0x6b];
    let missing = render_replay(
        data(FATAL, false),
        legacy.file,
        replay_limits(),
        options(0, 4, 16),
        Duration::ZERO,
        &mut untouched,
    );
    assert!(
        missing.is_err(),
        "zero output still validates required audible PCM"
    );
    assert_eq!(untouched, [0x6b]);

    let text = "#BPM 60\n#VOLWAV 50\n#WAV01 note\n#00011:01010000";
    let ordinary = recorded(
        &data(text, false),
        &[
            Action::Press(0, 91),
            Action::Release(0, 91),
            Action::Press(SECOND, 91),
            Action::Release(SECOND, 91),
        ],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let mut bytes = Vec::new();
    let report = render_replay(
        data(text, false),
        ordinary.file,
        replay_limits(),
        options(13, 3, 8),
        Duration::from_nanos(100_000_000),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(
        floats(&bytes),
        [
            0.0, 0.5, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, -0.5
        ]
    );
    assert_eq!(
        (
            report.frames,
            report.hits,
            report.judge_results,
            report.commands_admitted
        ),
        (13, 2, 2, 2)
    );
    assert_eq!(report.final_judge_hash, ordinary.hash);
    assert_eq!(report.last_render.unwrap().counters.unknown_stops, 0);
}

fn output_owner() -> (Mixer, CommandProducer, OfflineReport, DeviceFormat) {
    let format = AudioFormat::new(10, 1).unwrap();
    let limits = PcmLimits::new(16, 16, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.5, 0.5], limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(2).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            ts(0),
            AudioLimits::new(2, 2, 2, 2, 2).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (
        mixer,
        producer,
        OfflineReport {
            frames: 0,
            format,
            hits: 0,
            judge_results: 0,
            last_render: None,
        },
        DeviceFormat::new(10, 1, SampleEncoding::Float32, None).unwrap(),
    )
}

#[test]
fn actual_accepted_stops_only_relax_unknown_stop_diagnostics_and_generic_render_stays_strict() {
    let (mut mixer, mut producer, mut summary, format) = output_owner();
    let mut evidence = OwnedStopEvidence::default();
    let accepted = [play(1, 11, 0, 1.0), stop(u64::MAX, 0)];
    for &command in &accepted {
        producer.try_push(command).unwrap();
    }
    evidence.record_admitted(&accepted).unwrap();
    let rejected = producer.try_push(stop(9, 0)).unwrap_err();
    assert_eq!(rejected.reason, QueuePushError::Full); // Never entered the accepted evidence.
    let mut pcm = [0.0; 2];
    let mut encoded = [0u8; 8];
    let mut bytes = Vec::new();
    render_block_with_stops(
        &mut mixer,
        &mut pcm,
        &mut encoded,
        format,
        &mut bytes,
        &mut summary,
        &evidence,
    )
    .unwrap();
    assert_eq!(floats(&bytes), [0.5, 0.5]);
    assert_eq!(summary.last_render.unwrap().counters.unknown_stops, 1);
    let expired = stop(11, 200_000_000);
    producer.try_push(expired).unwrap();
    evidence
        .record_admitted(std::slice::from_ref(&expired))
        .unwrap();
    render_block_with_stops(
        &mut mixer,
        &mut pcm,
        &mut encoded,
        format,
        &mut bytes,
        &mut summary,
        &evidence,
    )
    .unwrap();
    render_block_with_stops(
        &mut mixer,
        &mut pcm,
        &mut encoded,
        format,
        &mut bytes,
        &mut summary,
        &evidence,
    )
    .unwrap();
    assert_eq!(floats(&bytes), [0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(summary.frames, 6);
    assert_eq!(summary.last_render.unwrap().counters.commands_applied, 3);
    assert_eq!(
        summary.last_render.unwrap().counters.unknown_stops,
        2,
        "cumulative raw evidence persists across blocks without being normalized"
    );

    for generic in [false, true] {
        let (mut mixer, mut producer, mut summary, format) = output_owner();
        let mut evidence = OwnedStopEvidence::default();
        producer.try_push(accepted[0]).unwrap();
        evidence.record_admitted(&accepted[..1]).unwrap(); // Play cannot purchase an allowance.
        producer.try_push(accepted[1]).unwrap();
        if generic {
            evidence.record_admitted(&accepted[1..]).unwrap();
        }
        let mut output = Vec::new();
        let error = if generic {
            render_block(
                &mut mixer,
                &mut pcm,
                &mut encoded,
                format,
                &mut output,
                &mut summary,
            )
        } else {
            render_block_with_stops(
                &mut mixer,
                &mut pcm,
                &mut encoded,
                format,
                &mut output,
                &mut summary,
                &evidence,
            )
        }
        .unwrap_err();
        assert!(output.is_empty());
        assert_eq!(summary.frames, 0);
        assert_eq!(error.last_render.unwrap().counters.unknown_stops, 1);
        assert!(error.audio_failures.is_empty());
    }
    for (bad, unknown_sample, invalid_gain) in [
        (play(99, 11, 0, 1.0), 1, 0),
        (play(1, 11, 0, f32::NAN), 0, 1),
    ] {
        let (mut mixer, mut producer, mut summary, format) = output_owner();
        let mut evidence = OwnedStopEvidence::default();
        let accepted = [bad, stop(11, 0)];
        for &command in &accepted {
            producer.try_push(command).unwrap();
        }
        evidence.record_admitted(&accepted).unwrap();
        let mut output = Vec::new();
        let error = render_block_with_stops(
            &mut mixer,
            &mut pcm,
            &mut encoded,
            format,
            &mut output,
            &mut summary,
            &evidence,
        )
        .unwrap_err();
        assert!(output.is_empty());
        assert_eq!(summary.frames, 0);
        let raw = error.last_render.unwrap();
        assert_eq!(summary.last_render, Some(raw));
        assert_eq!(raw.counters.unknown_stops, 1);
        assert_eq!(raw.counters.unknown_samples, unknown_sample);
        assert_eq!(raw.counters.invalid_gains, invalid_gain);
        assert!(
            error.audio_failures.is_empty(),
            "both commands were actually admitted"
        );
    }
}
