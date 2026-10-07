//! Independent bounded offline composition fixtures, authored without execution.
use beatkernel::{audio::*, judge::JudgeStage, runtime::SoundBinding};
use beatkernel_bms::{parse, ParseOptions};
use beatkernel_bms_runtime::{
    offline::{render_offline, OfflineOptions},
    PreparedBms,
};
use std::{
    collections::BTreeMap,
    io::{self, Write},
};

fn prepared(text: &str, rate: u32, assets: &[(u64, &[f32])]) -> PreparedBms {
    let source = parse(text, ParseOptions::default()).unwrap();
    let compiled = source.compile().unwrap();
    let format = AudioFormat::new(rate, 1).unwrap();
    let limits = PcmLimits::new(4096, 16384, 8).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    for &(id, pcm) in assets {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, pcm.to_vec(), limits).unwrap(),
        )
        .unwrap();
    }
    let holds: BTreeMap<_, _> = compiled
        .chart
        .objects()
        .iter()
        .map(|object| (object.id, object.time.end.is_some()))
        .collect();
    let sounds = source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: if holds[&note.object] {
                JudgeStage::HoldHead
            } else {
                JudgeStage::Instant
            },
            sample: note.sample,
            voice: VoiceId(note.object.0),
            gain: 1.0,
        })
        .collect();
    let first_bgm = compiled
        .chart
        .objects()
        .iter()
        .map(|object| object.id.0)
        .max()
        .unwrap_or(0)
        + 1;
    let bgm_commands = compiled
        .bgm
        .iter()
        .enumerate()
        .map(|(index, event)| AudioCommand::Play {
            voice: VoiceId(first_bgm + index as u64),
            sample: event.sample,
            at: event.at,
            gain: 1.0,
        })
        .collect();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands,
    }
}
fn options(frames: u64, block_frames: usize) -> OfflineOptions {
    OfflineOptions {
        frames,
        block_frames,
        command_capacity: 4,
        max_voices: 2,
    }
}
fn floats(bytes: &[u8]) -> Vec<f32> {
    assert!(bytes.len().is_multiple_of(4));
    bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}
const TIMING: &str = "#BPM 60\n#BPM01 120\n#STOP01 48\n#LNTYPE 1\n\
    #WAV01 instant.wav\n#WAV02 hold.wav\n#WAV03 layer-a.wav\n#WAV04 layer-b.wav\n\
    #00008:00010000\n#00009:00010000\n#00011:01000000\n#00052:00020005\n\
    #00001:00000300\n#00001:00000400\n#00111:01\n";
fn timing() -> PreparedBms {
    prepared(
        TIMING,
        4,
        &[
            (1, &[1.0, 0.5]),
            (2, &[0.25, 0.25]),
            (3, &[-0.5, -0.5]),
            (4, &[0.125, 0.125]),
        ],
    )
}

#[test]
fn literal_pcm_has_actual_bpm_stop_hold_head_and_layered_bgm_timing() {
    let mut bytes = Vec::new();
    let report = render_offline(timing(), options(16, 5), &mut bytes).unwrap();
    assert_eq!(
        floats(&bytes),
        vec![
            1.0, 0.5, 0.0, 0.0, 0.25, 0.25, 0.0, 0.0, -0.375, -0.375, 0.0, 0.0, 1.0, 0.5, 0.0, 0.0
        ]
    );
    assert_eq!(report.frames, 16);
    assert_eq!(report.format, AudioFormat::new(4, 1).unwrap());
    assert_eq!(report.hits, 4); // Two instants and both hold stages.
    assert_eq!(report.judge_results, 4);
    let render = report.last_render.unwrap();
    assert_eq!(render.counters.rendered_frames, 16);
    assert_eq!(render.counters.commands_applied, 5); // Three heads plus two BGM layers.
    assert_eq!(render.counters.late_commands, 0);
    assert_eq!(render.counters.voice_full, 0);
}

#[test]
fn chunk_partition_preserves_literal_pcm_and_judging() {
    let expected: Vec<_> = [
        1.0f32, 0.5, 0.0, 0.0, 0.25, 0.25, 0.0, 0.0, -0.375, -0.375, 0.0, 0.0, 1.0, 0.5, 0.0, 0.0,
    ]
    .into_iter()
    .flat_map(f32::to_le_bytes)
    .collect();
    for block in [1, 2, 3, 4, 7, 16, 64] {
        let mut output = Vec::new();
        let report = render_offline(timing(), options(16, block), &mut output).unwrap();
        assert_eq!(output, expected, "block {block}");
        assert_eq!((report.hits, report.judge_results), (4, 4));
        assert_eq!(report.last_render.unwrap().counters.rendered_frames, 16);
    }
}

#[test]
fn output_cutoff_does_not_judge_unrendered_head_or_tail_and_zero_is_valid() {
    for (frames, expected_hits) in [(0, 0), (4, 1), (5, 2), (10, 2), (11, 3), (12, 3), (13, 4)] {
        let mut output = Vec::new();
        let report = render_offline(timing(), options(frames, 3), &mut output).unwrap();
        assert_eq!(output.len(), frames as usize * 4);
        assert_eq!(report.frames, frames);
        assert_eq!(report.hits, expected_hits);
        assert_eq!(report.judge_results, expected_hits);
        assert_eq!(report.last_render.is_none(), frames == 0);
    }
}

#[test]
fn ceil_frame_cutoff_preserves_exact_song_input_without_early_audio() {
    // At BPM 120 beat 1 is 0.5 s, mapping to ceil(0.5 * 3) = frame 2.
    let text = "#BPM 120\n#WAV01 note.wav\n#00011:00010000";
    let mut before = Vec::new();
    let report = render_offline(
        prepared(text, 3, &[(1, &[0.75])]),
        options(2, 2),
        &mut before,
    )
    .unwrap();
    assert_eq!(floats(&before), vec![0.0, 0.0]);
    assert_eq!((report.hits, report.judge_results), (0, 0));
    let mut through = Vec::new();
    let report = render_offline(
        prepared(text, 3, &[(1, &[0.75])]),
        options(3, 1),
        &mut through,
    )
    .unwrap();
    assert_eq!(floats(&through), vec![0.0, 0.0, 0.75]);
    assert_eq!((report.hits, report.judge_results), (1, 1));
    assert_eq!(report.last_render.unwrap().counters.late_commands, 0);
}

#[test]
fn five_thousand_sparse_notes_use_one_voice_and_one_outstanding_command() {
    let mut text = String::from("#BPM 60\n#WAV01 note.wav\n");
    for measure in 0..1000 {
        text.push_str(&format!("#{measure:03}11:0101010101\n"));
    }
    let mut output = Vec::new();
    let report = render_offline(
        prepared(&text, 5, &[(1, &[0.25])]),
        OfflineOptions {
            frames: 20_000,
            block_frames: 7,
            command_capacity: 1,
            max_voices: 1,
        },
        &mut output,
    )
    .unwrap();
    assert_eq!((report.hits, report.judge_results), (5000, 5000));
    assert_eq!(report.frames, 20_000);
    let pcm = floats(&output);
    assert_eq!(pcm.len(), 20_000);
    for (frame, sample) in pcm.into_iter().enumerate() {
        assert_eq!(
            sample,
            if frame % 4 == 0 { 0.25 } else { 0.0 },
            "frame {frame}"
        );
    }
    assert_eq!(report.last_render.unwrap().counters.voice_full, 0);
}

#[test]
fn same_frame_queue_capacity_and_actual_voice_rejections_are_explicit() {
    let layered = "#BPM 60\n#WAV01 a.wav\n#00001:01\n#00001:01";
    let small = OfflineOptions {
        frames: 2,
        block_frames: 2,
        command_capacity: 1,
        max_voices: 2,
    };
    let enough = OfflineOptions {
        command_capacity: 2,
        ..small
    };
    assert!(render_offline(
        prepared(layered, 4, &[(1, &[0.25])]),
        small,
        &mut Vec::new()
    )
    .is_err());
    let mut output = Vec::new();
    let report =
        render_offline(prepared(layered, 4, &[(1, &[0.25])]), enough, &mut output).unwrap();
    assert_eq!(floats(&output), vec![0.5, 0.0]);
    assert_eq!(report.hits, 0);
    assert_eq!(report.last_render.unwrap().counters.commands_applied, 2);
    let overlapping = "#BPM 60\n#WAV01 a.wav\n#00011:01\n#00012:01";
    let error = render_offline(
        prepared(overlapping, 4, &[(1, &[0.25, 0.25])]),
        OfflineOptions {
            frames: 2,
            block_frames: 2,
            command_capacity: 2,
            max_voices: 1,
        },
        &mut Vec::new(),
    )
    .err()
    .expect("overlapping playback must expose a voice-capacity failure");
    assert!(
        error.to_string().contains("voice_full"),
        "execution failure must expose actual counters: {error}"
    );
}

#[test]
fn unknown_asset_execution_failure_is_not_a_successful_silent_render() {
    let text = "#BPM 60\n#WAV01 absent.wav\n#00011:01";
    let error = render_offline(prepared(text, 4, &[]), options(2, 2), &mut Vec::new())
        .err()
        .expect("missing asset must produce an execution failure");
    assert!(
        error.to_string().contains("unknown_samples"),
        "execution failure must expose actual counters: {error}"
    );
}

struct RejectWriter {
    calls: usize,
}
impl Write for RejectWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        self.calls += 1;
        Err(io::Error::other("injected offline writer failure"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn invalid_capacities_and_duration_overflow_reject_before_output_and_writer_errors_propagate() {
    let valid = options(16, 4);
    for invalid in [
        OfflineOptions {
            block_frames: 0,
            ..valid
        },
        OfflineOptions {
            block_frames: usize::MAX,
            ..valid
        },
        OfflineOptions {
            command_capacity: 0,
            ..valid
        },
        OfflineOptions {
            command_capacity: AudioLimits::MAX_COMMANDS + 1,
            ..valid
        },
        OfflineOptions {
            max_voices: 0,
            ..valid
        },
        OfflineOptions {
            max_voices: AudioLimits::MAX_VOICES + 1,
            ..valid
        },
        OfflineOptions {
            frames: u64::MAX,
            ..valid
        },
    ] {
        let mut writer = RejectWriter { calls: 0 };
        assert!(render_offline(timing(), invalid, &mut writer).is_err());
        assert_eq!(writer.calls, 0, "configuration must reject before output");
    }
    let mut writer = RejectWriter { calls: 0 };
    let error = render_offline(timing(), valid, &mut writer)
        .err()
        .expect("writer failure must propagate");
    assert!(error
        .to_string()
        .contains("injected offline writer failure"));
    assert_eq!(writer.calls, 1);
}
