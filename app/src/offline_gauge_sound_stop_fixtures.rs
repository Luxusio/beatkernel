//! Deferred closed-owner offline fixtures; requested frames do not prove song clearance.
//! The typed bank composes real Runtime/Mixer paths without admitting mine source files.
use crate::{
    mine_audio_consumers_fixtures::data,
    offline::{render_block, render_offline, OfflineError, OfflineOptions, OfflineReport},
    PreparedBms,
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        PcmSample, QueuePushError, SampleBank, SampleId, VoiceId,
    },
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};

const SECOND: i64 = 1_000_000_000;
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
fn floats(bytes: &[u8]) -> Vec<f32> {
    assert_eq!(bytes.len() % 4, 0);
    bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
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
fn long_music(text: &str) -> PreparedBms {
    let mut prepared = data(text, true);
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

#[test]
fn fatal_held_and_same_time_hit_prefix_keep_bgm_and_exact_output_extent_across_blocks() {
    let held = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 head\n#WAV02 music\n#00051:01000001\n#00012:00000100\n#000D1:00ZZ0000\n#000D2:00000001\n#00001:02000002";
    let same_time = "#BPM 60\n#VOLWAV 50\n#WAV00 unused\n#WAV01 head\n#WAV02 music\n#00011:00010001\n#00031:02\n#000D1:00ZZ0000\n#00001:02000002";
    for block in [1usize, 3, 7, 64] {
        for (text, held_before_failure, unknown_stops) in [(held, true, 3), (same_time, false, 2)] {
            let mut bytes = Vec::new();
            let report =
                render_offline(long_music(text), options(37, block, 16), &mut bytes).unwrap();
            let mut expected = vec![0.125; 37];
            expected[30..].fill(0.25); // A second independent BGM starts after failure.
            if held_before_failure {
                expected[0] = 0.625;
                expected[1] = -0.375;
            }
            assert_eq!(floats(&bytes), expected);
            assert_eq!(
                (report.frames, report.hits, report.judge_results),
                (37, 1, 1)
            );
            assert_eq!(bytes.len(), 37 * 4);
            let last = report.last_render.unwrap();
            assert_eq!(last.start_frame + last.frames as u64, 37);
            // Two BGM Plays, the committed head Play and three distinct gameplay Stops.
            assert_eq!(last.counters.commands_applied, 6);
            assert_eq!(last.counters.unknown_stops, unknown_stops);
            assert_eq!(last.counters.late_commands, 0);
            assert_eq!(last.counters.unknown_samples, 0);
            assert_eq!(last.counters.voice_full, 0);
            // The equal-time head was judged and queued, but its following Stop executes
            // before frame 10 is mixed. Later notes and the held tail are never fabricated.
            assert_eq!(&floats(&bytes)[10..12], [0.125, 0.125]);
        }
    }
}

#[test]
fn optional_wav00_avoided_and_recoverable_mines_preserve_silence_legacy_and_zero_extent() {
    for text in [
        "#BPM 60\n#WAV01 head\n#00051:01000100\n#000D1:00ZZ0000",
        "#BPM 60\n#WAV00 unused\n#WAV01 head\n#00051:01000100\n#000D1:00ZZ0000",
    ] {
        let mut bytes = Vec::new();
        let report = render_offline(data(text, false), options(25, 4, 8), &mut bytes).unwrap();
        let mut expected = vec![0.0; 25];
        expected[0] = 1.0;
        expected[1] = -1.0;
        assert_eq!(floats(&bytes), expected);
        assert_eq!((report.hits, report.judge_results), (1, 1));
        let last = report.last_render.unwrap();
        assert_eq!(last.counters.commands_applied, 2);
        assert_eq!(
            last.counters.unknown_stops, 1,
            "the already expired head is still an owned Stop target"
        );
    }
    for (text, has_zero) in [
        ("#BPM 60\n#000D1:ZZ", false),
        ("#BPM 60\n#WAV00 unused\n#000D1:ZZ", false),
        ("#BPM 60\n#WAV00 blast\n#000D1:1E", true),
        ("#BPM 60\n#000D1:1E", false),
    ] {
        let mut bytes = Vec::new();
        let report = render_offline(data(text, has_zero), options(13, 5, 8), &mut bytes).unwrap();
        assert_eq!(floats(&bytes), [0.0; 13]);
        assert_eq!((report.hits, report.judge_results), (0, 0));
        assert_eq!(
            report.last_render.unwrap().counters.commands_applied,
            0,
            "mine-only schedules contain no invented press, Play or Stop"
        );
    }
    let recoverable = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 note\n#00051:01000100\n#00012:00000001\n#000D1:001E0000";
    for mines in [true, false] {
        let text = if mines {
            recoverable.to_owned()
        } else {
            recoverable
                .replace("#000D1:001E0000", "")
                .replace("#WAV00 blast\n", "")
        };
        let mut bytes = Vec::new();
        let report = render_offline(data(&text, mines), options(35, 6, 8), &mut bytes).unwrap();
        let mut expected = vec![0.0; 35];
        expected[0] = 0.5;
        expected[1] = -0.5;
        expected[30] = 0.5;
        expected[31] = -0.5;
        if mines {
            expected[10] = 0.25;
            expected[11] = -0.25;
        }
        assert_eq!(floats(&bytes), expected);
        assert_eq!(
            (report.hits, report.judge_results),
            (3, 3),
            "default recoverable zero still admits the genuine hold tail and later head"
        );
        assert_eq!(
            report.last_render.unwrap().counters.commands_applied,
            if mines { 3 } else { 2 }
        );
        assert_eq!(report.last_render.unwrap().counters.unknown_stops, 0);
    }
    let mut untouched = vec![0x6a];
    let zero = render_offline(
        data("#BPM 60\n#WAV01 note\n#00011:01\n#000D1:ZZ", false),
        options(0, 4, 8),
        &mut untouched,
    )
    .unwrap();
    assert_eq!(untouched, [0x6a]);
    assert_eq!((zero.frames, zero.hits, zero.judge_results), (0, 0, 0));
    assert!(zero.last_render.is_none());
    assert!(
        render_offline(
            data("#BPM 60\n#WAV00 missing\n#000D1:1E", false),
            options(0, 4, 8),
            &mut untouched
        )
        .is_err()
    );
    assert_eq!(
        untouched,
        [0x6a],
        "zero extent preserves actual setup validation"
    );
}

#[test]
fn partial_stop_and_original_play_refusals_keep_exact_written_prefix_and_latest_render() {
    let text = "#BPM 60\n#VOLWAV 50\n#WAV01 note\n#00011:01010000\n#000D1:00ZZ0000";
    for capacity in [1usize, 2, 3] {
        let mut prepared = data(text, false);
        let mut second_sound = prepared.sounds[1].clone();
        second_sound.voice = VoiceId(31);
        prepared.sounds.push(second_sound); // Two real bindings for the failure operation's head.
        let mut bytes = Vec::new();
        let error = render_offline(prepared, options(25, 3, capacity), &mut bytes).unwrap_err();
        let failure = error.downcast_ref::<OfflineError>().unwrap();
        assert_eq!(
            floats(&bytes),
            [0.5, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        let last = failure.last_render.unwrap();
        assert_eq!(last.start_frame + last.frames as u64, 10);
        assert_eq!(last.counters.commands_applied, 1);
        assert_eq!(
            last.counters.unknown_stops, 0,
            "newly admitted Stops have not been rendered on this error path"
        );
        let expected = match capacity {
            1 => vec![
                play(1, 31, SECOND, 0.5),
                stop(11, SECOND),
                stop(12, SECOND),
                stop(31, SECOND),
            ],
            2 => vec![stop(11, SECOND), stop(12, SECOND), stop(31, SECOND)],
            3 => vec![stop(12, SECOND), stop(31, SECOND)],
            _ => unreachable!(),
        };
        assert_eq!(
            failure
                .audio_failures
                .iter()
                .map(|error| error.command)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(
            failure
                .audio_failures
                .iter()
                .all(|error| error.reason == QueuePushError::Full)
        );
        // Capacity 3 accepts Stop(11) after both Plays. Its success is not a license
        // to discard the two refusals, retry, render further or count requested Stops.
        assert!(failure.message.contains("admission"));
    }
}

#[test]
fn generic_render_remains_strict_and_closed_owner_allowance_never_hides_other_diagnostics() {
    let format = AudioFormat::new(10, 1).unwrap();
    let limits = PcmLimits::new(32, 32, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0, -1.0], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(4).unwrap();
    producer.try_push(play(1, 11, 0, 1.0)).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            ts(0),
            AudioLimits::new(4, 2, 4, 2, 4).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let encoded = DeviceFormat::new(10, 1, SampleEncoding::Float32, None).unwrap();
    let mut summary = OfflineReport {
        frames: 0,
        format,
        hits: 0,
        judge_results: 0,
        last_render: None,
    };
    let mut pcm = [0.0; 2];
    let mut encoded_bytes = [0u8; 8];
    let mut output = Vec::new();
    render_block(
        &mut mixer,
        &mut pcm,
        &mut encoded_bytes,
        encoded,
        &mut output,
        &mut summary,
    )
    .unwrap();
    assert_eq!(floats(&output), [1.0, -1.0]);
    producer.try_push(stop(11, 200_000_000)).unwrap();
    let error = render_block(
        &mut mixer,
        &mut pcm,
        &mut encoded_bytes,
        encoded,
        &mut output,
        &mut summary,
    )
    .unwrap_err();
    assert_eq!(summary.frames, 2);
    assert_eq!(floats(&output), [1.0, -1.0]);
    assert!(
        error.audio_failures.is_empty(),
        "the actual queue accepted the Stop"
    );
    let rejected = error.last_render.unwrap();
    assert_eq!(rejected, summary.last_render.unwrap());
    assert_eq!((rejected.start_frame, rejected.frames), (2, 2));
    assert_eq!(rejected.counters.commands_applied, 2);
    assert_eq!(
        rejected.counters.unknown_stops, 1,
        "generic rendering receives no owned-stop allowance"
    );

    let mut broken = data("#BPM 60\n#WAV01 note\n#00011:01\n#000D1:ZZ", false);
    broken.sounds[0].sample = SampleId(99);
    let mut untouched = Vec::new();
    let error = render_offline(broken, options(4, 2, 8), &mut untouched).unwrap_err();
    let failure = error.downcast_ref::<OfflineError>().unwrap();
    let rejected = failure.last_render.unwrap();
    assert!(untouched.is_empty());
    assert!(failure.audio_failures.is_empty());
    assert_eq!(rejected.counters.unknown_samples, 1);
    assert_eq!(
        rejected.counters.unknown_stops, 1,
        "this Stop is actually accepted, but its allowance cannot hide a missing sample"
    );
}
