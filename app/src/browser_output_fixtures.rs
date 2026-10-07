//! Pure report ABI fixtures: no JS values, generated bindings or browser calls.
use super::{
    decode_output, decode_section_output, WorkletAudio, WorkletAudioBuilder, WorkletAudioConfig,
};
use beatkernel::{
    audio::{AudioCommand, AudioFormat, AudioLimits, PcmLimits, SampleId, VoiceId},
    time::Timestamp,
};

fn put(words: &mut [u32; 56], field: usize, value: u64) {
    words[field * 2] = value as u32;
    words[field * 2 + 1] = (value >> 32) as u32;
}

fn rendered(start: u64, cursor: u64, frames: u64) -> [u32; 56] {
    let mut words = [0; 56];
    for (field, value) in [
        (0, 1),
        (1, cursor),
        (2, frames),
        (3, cursor),
        (4, frames),
        (8, 1),
        (9, 2),
        (12, cursor + frames),
        (23, 1),
        (24, start + cursor + frames),
        (25, 1),
        (26, start),
    ] {
        put(&mut words, field, value);
    }
    words
}

fn finite_worklet(end: u64, start: u64, current: u64) -> WorkletAudio {
    let format = AudioFormat::new(1000, 1).unwrap();
    let mut builder = WorkletAudioBuilder::new(WorkletAudioConfig {
        format,
        pcm_limits: PcmLimits::new(32, 32, 1).unwrap(),
        audio_limits: AudioLimits::new(4, 2, 4, 8, 4).unwrap(),
    })
    .unwrap();
    builder
        .insert_sample(SampleId(1), format, vec![0.25; 8])
        .unwrap();
    let mut audio = builder.finish_at(end).unwrap();
    audio
        .enqueue(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    audio.arm(start, current).unwrap();
    audio
}

// Encode actual owner evidence in the documented BrowserAudio::report_word ABI.
// No callback counts are used to invent playback, endpoint or counter values.
fn actual_words(audio: &WorkletAudio) -> [u32; 56] {
    let mut words = [0; 56];
    for (field, value) in [
        (23, u64::from(audio.context_frame().is_some())),
        (24, audio.context_frame().unwrap_or(0)),
        (25, u64::from(audio.start_frame().is_some())),
        (26, audio.start_frame().unwrap_or(0)),
        (27, u64::from(audio.failed())),
    ] {
        put(&mut words, field, value);
    }
    if let Some(report) = audio.report() {
        let c = report.counters;
        let fields = [
            1,
            report.start_frame,
            report.frames as u64,
            report.playback_start_frame,
            report.playback_frames as u64,
            u64::from(report.paused),
            u64::from(report.playback_end_physical_frame.is_some()),
            report.playback_end_physical_frame.unwrap_or(0),
            report.active_voices as u64,
            report.pending_commands as u64,
            report.song_position.as_nanos() as u64,
            u64::from(report.producer_disconnected),
            c.rendered_frames,
            c.commands_consumed,
            c.commands_applied,
            c.late_commands,
            c.pending_full,
            c.voice_full,
            c.unknown_samples,
            c.unknown_stops,
            c.invalid_gains,
            c.invalid_rates,
            c.invalid_times,
        ];
        for (field, value) in fields.into_iter().enumerate() {
            put(&mut words, field, value);
        }
    }
    words
}

#[test]
fn finite_decoder_preserves_actual_partial_worklet_reports_across_exact_crossing_and_zero_ends() {
    let base = (1u64 << 53) + 101;
    for endpoint in [0, 3] {
        for parts in [vec![8], vec![2, 3, 3], vec![1; 8]] {
            let mut audio = finite_worklet(endpoint, base + 2, base);
            let absent = decode_section_output(&actual_words(&audio), Some(endpoint)).unwrap();
            assert!(absent.report.is_none());
            assert_eq!(absent.start, base + 2);
            assert_eq!(absent.context, Some(base));
            let mut current = base;
            for frames in parts {
                audio.render(current, frames).unwrap();
                current += frames as u64;
                let words = actual_words(&audio);
                let decoded = decode_section_output(&words, Some(endpoint)).unwrap();
                assert_eq!(decoded.report, audio.report());
                assert_eq!(decoded.context, Some(current));
                assert_eq!(decoded.start, base + 2);
                if let Some(report) = decoded.report {
                    if report.playback_end_physical_frame.is_some() {
                        assert!(
                            decode_output(&words).is_err(),
                            "unlimited consumers must not silently adopt a finite fence"
                        );
                        assert_eq!(
                            decoded.start + report.playback_end_physical_frame.unwrap(),
                            base + 2 + endpoint
                        );
                    } else {
                        assert_eq!(
                            decode_section_output(&words, None).unwrap().report,
                            decode_output(&words).unwrap().report
                        );
                    }
                }
            }
            assert_eq!(
                audio.report().unwrap().playback_end_physical_frame,
                Some(endpoint)
            );
            assert_eq!(audio.context_frame(), Some(base + 8));
        }
    }
}

#[test]
fn finite_decoder_requires_configured_marker_original_grid_and_checked_absolute_extent() {
    let mut audio = finite_worklet(3, 100, 100);
    audio.render(100, 1).unwrap();
    let before = actual_words(&audio);
    for (field, value) in [(5, 1), (6, 1), (7, 3)] {
        let mut invalid = before;
        put(&mut invalid, field, value);
        assert!(decode_section_output(&invalid, Some(3)).is_err());
    }
    audio.render(101, 4).unwrap();
    let ended = actual_words(&audio);
    assert_eq!(
        decode_section_output(&ended, Some(3)).unwrap().report,
        audio.report()
    );
    for (field, value) in [
        (3, 2),
        (4, 4),
        (5, 0),
        (6, 0),
        (6, 2),
        (7, 4),
        (11, 1),
        (23, 0),
        (24, 103),
        (25, 0),
        (27, 1),
    ] {
        let mut invalid = ended;
        put(&mut invalid, field, value);
        assert!(
            decode_section_output(&invalid, Some(3)).is_err(),
            "field {field}={value}"
        );
    }
    assert!(decode_section_output(&ended, Some(4)).is_err());
    assert!(decode_section_output(&ended[..55], Some(3)).is_err());
    let mut overflow = ended;
    put(&mut overflow, 1, u64::MAX);
    put(&mut overflow, 3, 3);
    assert!(decode_section_output(&overflow, Some(3)).is_err());
    let unavailable = finite_worklet(0, u64::MAX, u64::MAX);
    let words = actual_words(&unavailable);
    assert!(
        decode_section_output(&words, Some(0))
            .unwrap()
            .report
            .is_none()
    );
    assert!(
        decode_section_output(&words, Some(1)).is_err(),
        "configured end overflow rejects even absent render evidence"
    );
    let mut stray = words;
    put(&mut stray, 6, 1);
    assert!(decode_section_output(&stray, Some(0)).is_err());
}

#[test]
fn exact_wide_fields_partial_blocks_and_unavailable_evidence_are_preserved() {
    let start = 9_007_199_254_740_993;
    let cursor = 0x1_ffff_fff0;
    for (frames, song) in [(1, i64::MIN), (3, -1), (257, i64::MAX), (4096, 0)] {
        let mut words = rendered(start, cursor, frames);
        put(&mut words, 10, song as u64);
        put(&mut words, 13, u64::MAX);
        put(&mut words, 14, u64::MAX - 1);
        put(&mut words, 15, 1 << 63);
        let evidence = decode_output(&words).unwrap();
        assert_eq!(evidence.start, start);
        assert_eq!(evidence.context, Some(start + cursor + frames));
        let report = evidence.report.unwrap();
        assert_eq!(report.start_frame, cursor);
        assert_eq!(report.playback_start_frame, cursor);
        assert_eq!(
            (report.frames, report.playback_frames),
            (frames as usize, frames as usize)
        );
        assert_eq!((report.active_voices, report.pending_commands), (1, 2));
        assert_eq!(report.song_position.as_nanos(), song);
        assert_eq!(report.counters.rendered_frames, cursor + frames);
        assert_eq!(report.counters.commands_consumed, u64::MAX);
        assert_eq!(report.counters.commands_applied, u64::MAX - 1);
        assert_eq!(report.counters.late_commands, 1 << 63);
        assert!(!report.paused && !report.producer_disconnected);
        assert_eq!(report.playback_end_physical_frame, None);
    }
    let mut absent = [0; 56];
    put(&mut absent, 25, 1);
    put(&mut absent, 26, u64::MAX);
    assert_eq!(decode_output(&absent).unwrap().context, None);
    put(&mut absent, 23, 1);
    put(&mut absent, 24, u64::MAX);
    let evidence = decode_output(&absent).unwrap();
    assert_eq!(evidence.start, u64::MAX);
    assert_eq!(evidence.context, Some(u64::MAX));
    assert!(evidence.report.is_none());
}

#[test]
fn flags_modes_capacities_and_context_must_describe_one_normal_output_grid() {
    let valid = rendered(1000, 12, 3);
    assert!(decode_output(&valid[..55]).is_err());
    assert!(decode_output(&[0; 57]).is_err());
    for field in [0, 5, 6, 11, 23, 25, 27] {
        for value in [2, 1 << 32] {
            let mut invalid = valid;
            put(&mut invalid, field, value);
            assert!(decode_output(&invalid).is_err(), "flag {field}={value}");
        }
    }
    for (field, value) in [
        (2, 0),
        (2, 4097),
        (8, 4097),
        (9, 4097),
        (3, 13),
        (4, 2),
        (5, 1),
        (6, 1),
        (7, 15),
        (11, 1),
        (23, 0),
        (24, 1014),
        (24, 1016),
        (25, 0),
        (27, 1),
    ] {
        let mut invalid = valid;
        put(&mut invalid, field, value);
        assert!(decode_output(&invalid).is_err(), "field {field}={value}");
    }
    // Error counters are decoded as actual evidence; shared completion rejects
    // them later. The ABI boundary must not silently clear or narrow them.
    let mut failed = valid;
    for field in 16..=22 {
        put(&mut failed, field, (1 << 63) + field as u64);
    }
    let counters = decode_output(&failed).unwrap().report.unwrap().counters;
    assert_eq!(
        [
            counters.pending_full,
            counters.voice_full,
            counters.unknown_samples,
            counters.unknown_stops,
            counters.invalid_gains,
            counters.invalid_rates,
            counters.invalid_times
        ],
        [16, 17, 18, 19, 20, 21, 22].map(|n| (1 << 63) + n)
    );
}

#[test]
fn unavailable_reports_cannot_smuggle_render_data_and_extent_additions_are_checked() {
    let mut absent = [0; 56];
    put(&mut absent, 25, 1);
    put(&mut absent, 26, 100);
    for field in 1..=22 {
        let mut stray = absent;
        put(&mut stray, field, 1);
        assert!(decode_output(&stray).is_err(), "unavailable field {field}");
    }
    put(&mut absent, 24, 1);
    assert!(
        decode_output(&absent).is_err(),
        "absent context has a nonzero cursor"
    );
    put(&mut absent, 23, 1);
    put(&mut absent, 24, 101);
    assert!(
        decode_output(&absent).is_err(),
        "unavailable output passed its armed start"
    );
    let mut relative_overflow = rendered(0, 0, 1);
    put(&mut relative_overflow, 1, u64::MAX);
    put(&mut relative_overflow, 3, u64::MAX);
    assert!(decode_output(&relative_overflow).is_err());
    let mut absolute_overflow = rendered(0, 0, 1);
    put(&mut absolute_overflow, 26, u64::MAX);
    assert!(decode_output(&absolute_overflow).is_err());
}
