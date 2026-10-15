//! Original asset custody for the opt-in retained browser preparation path.
//! These portable tests exercise preparation, not a browser job or an output device.
use beatkernel::{
    audio::{AudioCommand, AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId},
    judge::JudgeStage,
    runtime::SoundBinding,
    time::Timestamp,
};
use beatkernel_bms_runtime::{
    play_policy::OriginalGaugeContext,
    section_start::{prepare_at, prepare_retained_at},
    PreparedBms,
};

const MUSIC: [f32; 24] = [
    -0.0, 0.125, -0.25, 0.375, -0.5, 0.625, -0.75, 0.875, 1.0, -1.125, 1.25, -1.375, 1.5, -1.625,
    1.75, -1.875, 2.0, -2.125, 2.25, -2.375, 2.5, -2.625, 2.75, -2.875,
];
const KEY: [f32; 12] = [
    0.5, 0.25, -0.5, -0.25, 1.0, 0.0, -1.0, -0.0, 0.75, -0.75, 0.125, -0.125,
];

fn ts(nanos: i64) -> Timestamp {
    Timestamp::from_nanos(nanos)
}

fn limits() -> PcmLimits {
    PcmLimits::new(256, 1024, 3).unwrap()
}

fn prepared() -> PreparedBms {
    // At BPM120 the hold spans [0,1s], the later instant is at2s and the
    // music cues are at0,1s,4s. Later tempo/stop markers must survive selection.
    let mut source = beatkernel_bms::parse(
        "#TITLE Original retained chart\n#BPM 120\n#RANK 2\n#TOTAL 240\n\
         #WAV01 key.wav\n#WAV02 music.wav\n#00012:01\n#00051:0101\n\
         #00111:01\n#00001:0202\n#00201:02\n#BPM03 60\n#STOP01 48\n\
         #00308:03\n#00309:01\n",
        Default::default(),
    )
    .unwrap();
    source
        .metadata
        .insert("X_TEST_OPAQUE".into(), "original-only metadata".into());
    for (index, object) in source.source.objects.iter_mut().enumerate() {
        object.metadata.0 = vec![0xde, 0xad, index as u8];
    }
    let compiled = source.compile().unwrap();
    let mut bank = SampleBank::new(AudioFormat::new(8, 1).unwrap(), limits()).unwrap();
    for (id, hz, data) in [
        (1, 4, KEY.to_vec()),
        (2, 4, MUSIC.to_vec()),
        (77, 8, vec![-0.0, f32::from_bits(1)]),
    ] {
        bank.insert(
            SampleId(id),
            PcmSample::new(AudioFormat::new(hz, 1).unwrap(), data, limits()).unwrap(),
        )
        .unwrap();
    }
    let sounds = source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: if compiled
                .chart
                .objects()
                .iter()
                .find(|object| object.id == note.object)
                .unwrap()
                .time
                .end
                .is_some()
            {
                JudgeStage::HoldHead
            } else {
                JudgeStage::Instant
            },
            sample: note.sample,
            voice: VoiceId(100 + note.object.0),
            gain: 0.25,
        })
        .collect();
    let bgm_commands = [
        (900, 0, 0.25),
        (901, 1_000_000_000, 0.5),
        (902, 4_000_000_000, 0.75),
    ]
    .map(|(voice, at, gain)| AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(2),
        at: ts(at),
        gain,
    })
    .to_vec();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands,
    }
}

#[test]
fn selected_judge_heads_keep_original_ids_lines_metadata_and_absolute_timing() {
    let original = prepared();
    let full_source = original.source.clone();
    let original_gauge = OriginalGaugeContext::from_source(&full_source);
    let future = full_source
        .notes
        .iter()
        .find(|note| note.line == 9)
        .unwrap()
        .clone();
    let mut expected_source = full_source.clone();
    expected_source
        .source
        .objects
        .retain(|object| object.id == future.object);
    expected_source.notes = vec![future.clone()];
    let original_bgm = original.compiled.bgm.clone();
    let selected = prepare_retained_at(original, ts(550_000_000)).unwrap();

    assert_eq!(selected.original_source, full_source);
    assert_eq!(selected.prepared.source, expected_source);
    assert_eq!(selected.prepared.source.notes, [future.clone()]);
    let objects = selected.prepared.compiled.chart.objects();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].id, future.object);
    assert_eq!(objects[0].time.start, ts(2_000_000_000));
    assert_eq!(objects[0].time.end, None);
    assert_eq!(selected.prepared.compiled.bgm, original_bgm);
    assert!(!selected.prepared.source.source.bpm_changes.is_empty());
    assert!(!selected.prepared.source.source.stops.is_empty());
    assert_eq!(selected.section.original_gauge, original_gauge);
    assert_eq!(selected.section.start, ts(550_000_000));
    assert_eq!(selected.section.excluded_objects, 2);
    assert_eq!(selected.section.excluded_crossing_holds, 1);
    assert_eq!(selected.section.retired_bgm, 0);
    assert!(selected.section.tails.is_empty());
}

#[test]
fn retained_selection_moves_full_sound_and_music_vectors_and_every_original_pcm() {
    let original = prepared();
    let sound_ptr = original.sounds.as_ptr();
    let cue_ptr = original.bgm_commands.as_ptr();
    let sounds = original.sounds.clone();
    let cues = original.bgm_commands.clone();
    let total_bytes = original.bank.total_bytes();
    let assets = [SampleId(1), SampleId(2), SampleId(77)].map(|id| {
        let pcm = original.bank.get(id).unwrap();
        (
            id,
            pcm.samples().as_ptr(),
            pcm.format(),
            pcm.samples()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
        )
    });
    let selected = prepare_retained_at(original, ts(550_000_000)).unwrap();

    assert_eq!(selected.prepared.sounds, sounds);
    assert_eq!(selected.prepared.sounds.as_ptr(), sound_ptr);
    assert_eq!(selected.prepared.bgm_commands, cues);
    assert_eq!(selected.prepared.bgm_commands.as_ptr(), cue_ptr);
    assert_eq!(selected.prepared.bank.len(), 3);
    assert_eq!(selected.prepared.bank.total_bytes(), total_bytes);
    assert_eq!(
        selected.prepared.bank.format(),
        AudioFormat::new(8, 1).unwrap()
    );
    for (id, ptr, format, bits) in assets {
        let pcm = selected.prepared.bank.get(id).unwrap();
        assert_eq!(pcm.samples().as_ptr(), ptr, "PCM{id:?} was copied");
        assert_eq!(pcm.format(), format);
        assert_eq!(
            pcm.samples()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            bits
        );
    }
    assert_eq!(
        selected.prepared.bank.get(SampleId(2)).unwrap().samples(),
        MUSIC
    );
    assert!(selected.prepared.sounds.iter().any(|sound| {
        !selected
            .prepared
            .source
            .notes
            .iter()
            .any(|note| note.object == sound.object)
    }));
    assert_eq!(
        selected
            .prepared
            .bank
            .into_samples()
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        [SampleId(1), SampleId(2), SampleId(77)]
    );
}

#[test]
fn zero_start_preserves_the_entire_initial_chart_and_original_asset_ownership() {
    let original = prepared();
    let source = original.source.clone();
    let compiled = original.compiled.clone();
    let music_ptr = original.bank.get(SampleId(2)).unwrap().samples().as_ptr();
    let selected = prepare_retained_at(original, Timestamp::ZERO).unwrap();

    assert_eq!(selected.original_source, source);
    assert_eq!(selected.prepared.source, source);
    assert_eq!(selected.prepared.compiled, compiled);
    assert_eq!(
        selected
            .prepared
            .bank
            .get(SampleId(2))
            .unwrap()
            .samples()
            .as_ptr(),
        music_ptr
    );
    assert_eq!(selected.section.excluded_objects, 0);
    assert_eq!(selected.section.excluded_crossing_holds, 0);
    assert_eq!(selected.section.retired_bgm, 0);
    assert!(selected.section.tails.is_empty());
}

#[test]
fn nonnegative_empty_selections_preserve_originals_for_future_backward_scrub() {
    for start in [9_000_000_000, i64::MAX] {
        let original = prepared();
        let source = original.source.clone();
        let cues = original.bgm_commands.clone();
        let music_ptr = original.bank.get(SampleId(2)).unwrap().samples().as_ptr();
        let selected = prepare_retained_at(original, ts(start)).unwrap();
        assert!(selected.prepared.compiled.chart.objects().is_empty());
        assert!(selected.prepared.source.notes.is_empty());
        assert_eq!(selected.original_source, source);
        assert_eq!(selected.original_source.notes.len(), 3);
        assert_eq!(selected.prepared.sounds.len(), 3);
        assert_eq!(selected.prepared.bgm_commands, cues);
        assert_eq!(
            selected
                .prepared
                .bank
                .get(SampleId(2))
                .unwrap()
                .samples()
                .as_ptr(),
            music_ptr
        );
        assert_eq!(selected.section.start, ts(start));
        assert_eq!(selected.section.excluded_objects, 3);
        assert_eq!(selected.section.retired_bgm, 0);
        assert!(selected.section.tails.is_empty());
    }
    for start in [-1, i64::MIN] {
        assert!(prepare_retained_at(prepared(), ts(start)).is_err());
    }
}

#[test]
fn legacy_section_preparation_still_slices_music_and_prunes_earlier_bindings() {
    let (legacy, report) = prepare_at(prepared(), ts(550_000_000), limits()).unwrap();
    assert_eq!(report.tails.len(), 1);
    assert_eq!(report.tails[0].source, SampleId(2));
    assert_eq!(report.tails[0].suffix, SampleId(3));
    assert_eq!(report.tails[0].frame, 3);
    assert_eq!(report.tails[0].applied_song_time, ts(750_000_000));
    assert_eq!(report.tails[0].correction_ns, 200_000_000);
    assert_eq!(legacy.bank.get(SampleId(3)).unwrap().samples(), &MUSIC[3..]);
    assert_ne!(
        legacy.bank.get(SampleId(3)).unwrap().samples().as_ptr(),
        legacy.bank.get(SampleId(2)).unwrap().samples().as_ptr()
    );
    assert_eq!(legacy.sounds.len(), 1);
    assert_eq!(legacy.bank.len(), 4); // Three original assets plus one copied suffix.
    assert!(matches!(legacy.bgm_commands[0], AudioCommand::Play {
        sample: SampleId(3), at, gain, ..
    } if at == ts(750_000_000) && gain == 0.25));
    assert!(prepare_at(prepared(), ts(i64::MAX), limits()).is_err());

    let retained = prepare_retained_at(prepared(), ts(550_000_000)).unwrap();
    assert!(retained.section.tails.is_empty());
    assert_eq!(retained.prepared.bank.len(), 3);
    assert_eq!(retained.prepared.sounds.len(), 3);
    assert!(
        matches!(retained.prepared.bgm_commands[0], AudioCommand::Play {
        sample: SampleId(2), at, gain, ..
    } if at == Timestamp::ZERO && gain == 0.25)
    );
}
