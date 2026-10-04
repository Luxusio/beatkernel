//! Shared native chart preparation; publication and native acquisition stay separate.
use crate::{
    ChannelPolicy, PreparedBms, load_prepared_with_seed,
    native_gameplay::NativeGameplayResult,
    section_start::{SectionReport, prepare_at},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits},
    time::Timestamp,
};
use std::{collections::BTreeMap, path::Path};

pub struct NativeChartConfig<'a> {
    pub path: &'a Path,
    pub format: AudioFormat,
    pub limits: PcmLimits,
    pub channels: ChannelPolicy,
    pub chart_seed: u64,
    pub start: Timestamp,
    pub bindings: &'a BTreeMap<u8, u16>,
}
/// Load the selected branch in the exact supplied format, then validate retained lanes.
pub fn prepare_chart(
    config: NativeChartConfig<'_>,
) -> NativeGameplayResult<(PreparedBms, SectionReport)> {
    prepare_with(config, |config| {
        load_prepared_with_seed(
            config.path,
            config.format,
            config.limits,
            config.channels,
            config.chart_seed,
        )
    })
}
pub(crate) fn prepare_with(
    config: NativeChartConfig<'_>,
    load: impl FnOnce(&NativeChartConfig<'_>) -> NativeGameplayResult<PreparedBms>,
) -> NativeGameplayResult<(PreparedBms, SectionReport)> {
    if config.start.as_nanos() < 0 {
        return Err("practice start must be nonnegative".into());
    }
    let (prepared, section) = prepare_at(load(&config)?, config.start, config.limits)?;
    for lane in prepared
        .source
        .notes
        .iter()
        .map(|note| note.lane)
        .chain(prepared.source.invisible.iter().map(|event| event.lane))
    {
        if !config.bindings.contains_key(&lane.channel()) {
            return Err(format!("missing --bind for BMS channel {:02X}", lane.channel()).into());
        }
    }
    Ok((prepared, section))
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        audio::{AudioCommand, PcmSample, SampleBank, SampleId, VoiceId},
        judge::JudgeStage,
        runtime::SoundBinding,
    };
    use beatkernel_bms::{ParseOptions, parse_seeded};
    const TEXT: &str = "#BPM 120\n#WAV01 original.wav\n#00012:01\n#00053:0101\n#00111:01\n#00001:0101\n#00201:01\n#BGA legacy\n";
    fn limits() -> PcmLimits {
        PcmLimits::new(1024, 4096, 4).unwrap()
    }
    fn config<'a>(bindings: &'a BTreeMap<u8, u16>, start: i64) -> NativeChartConfig<'a> {
        NativeChartConfig {
            path: Path::new("unchanged/chart.bms"),
            format: AudioFormat::new(8, 1).unwrap(),
            limits: limits(),
            channels: ChannelPolicy::Exact,
            chart_seed: 0,
            start: Timestamp::from_nanos(start),
            bindings,
        }
    }
    fn parsed(config: &NativeChartConfig<'_>, text: &str) -> NativeGameplayResult<PreparedBms> {
        let source = parse_seeded(text, ParseOptions::default(), config.chart_seed)?;
        let compiled = source.compile()?;
        let mut bank = SampleBank::new(config.format, config.limits)?;
        bank.insert(
            SampleId(1),
            PcmSample::new(
                AudioFormat::new(4, config.format.channels())?,
                (0..12)
                    .flat_map(|n| {
                        std::iter::repeat(n as f32).take(usize::from(config.format.channels()))
                    })
                    .collect(),
                config.limits,
            )?,
        )?;
        let sounds = source
            .notes
            .iter()
            .map(|note| SoundBinding {
                object: note.object,
                stage: if compiled
                    .chart
                    .objects()
                    .iter()
                    .find(|o| o.id == note.object)
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
                voice: VoiceId(note.object.0),
                gain: 1.0,
            })
            .collect();
        let first = compiled
            .chart
            .objects()
            .iter()
            .map(|o| o.id.0)
            .max()
            .unwrap_or(0)
            + 1;
        let bgm_commands = compiled
            .bgm
            .iter()
            .enumerate()
            .map(|(index, cue)| AudioCommand::Play {
                voice: VoiceId(first + index as u64),
                sample: cue.sample,
                at: cue.at,
                gain: 1.0,
            })
            .collect();
        Ok(PreparedBms {
            source,
            compiled,
            bank,
            sounds,
            bgm_commands,
        })
    }
    #[test]
    fn forwards_exact_request_and_seed_without_publication() {
        let bindings = BTreeMap::from([(0x11, 4), (0x12, 5)]);
        let mut cfg = config(&bindings, 0);
        cfg.format = AudioFormat::new(48000, 2).unwrap();
        cfg.channels = ChannelPolicy::MonoToStereo;
        cfg.chart_seed = 3;
        let text = "#BPM 120\n#WAV01 original.wav\n#RANDOM 2\n#IF 1\n#00011:01\n#ELSE\n#00012:01\n#ENDIF\n#ENDRANDOM\n";
        let (prepared, section) = prepare_with(cfg, |actual| {
            assert_eq!(actual.path, Path::new("unchanged/chart.bms"));
            assert_eq!(actual.format, AudioFormat::new(48000, 2).unwrap());
            assert_eq!(actual.channels, ChannelPolicy::MonoToStereo);
            assert_eq!(actual.chart_seed, 3);
            assert_eq!(actual.limits.max_total_bytes(), 4096);
            parsed(actual, text)
        })
        .unwrap();
        assert_eq!(prepared.source.notes[0].lane.channel(), 0x11);
        assert_eq!(prepared.bank.format(), AudioFormat::new(48000, 2).unwrap());
        assert_eq!(section.start, Timestamp::ZERO);
    }
    #[test]
    fn zero_section_preserves_source_objects_commands_and_warning_lines() {
        let bindings = BTreeMap::from([(0x11, 4), (0x12, 5), (0x13, 6), (0x29, 0)]); // unused entries are not validated here.
        let cfg = config(&bindings, 0);
        let original = parsed(&cfg, TEXT).unwrap();
        let notes = original.source.notes.clone();
        let objects = original.compiled.chart.objects().to_vec();
        let commands = original.bgm_commands.clone();
        let warnings = original.source.warnings.clone();
        let pointer = original.bank.get(SampleId(1)).unwrap().samples().as_ptr();
        let (prepared, section) = prepare_with(cfg, |_| Ok(original)).unwrap();
        assert_eq!(prepared.source.notes, notes);
        assert_eq!(prepared.compiled.chart.objects(), objects);
        assert_eq!(prepared.bgm_commands, commands);
        assert_eq!(prepared.source.warnings, warnings);
        assert_eq!(prepared.source.warnings[0].line, 8);
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
            pointer
        );
        assert_eq!(section.excluded_objects, 0);
        assert!(section.tails.is_empty());
    }
    #[test]
    fn sliced_coverage_ignores_old_heads_and_crossing_holds_but_requires_future_notes() {
        let bindings = BTreeMap::from([(0x11, 4)]);
        let cfg = config(&bindings, 550_000_000);
        let original = parsed(&cfg, TEXT).unwrap();
        let retained: Vec<_> = original
            .compiled
            .chart
            .objects()
            .iter()
            .filter(|o| o.time.start >= cfg.start)
            .cloned()
            .collect();
        let (prepared, section) = prepare_with(cfg, |_| Ok(original)).unwrap();
        assert_eq!(prepared.compiled.chart.objects(), retained);
        assert_eq!(section.excluded_crossing_holds, 1);
        assert_eq!(section.excluded_objects, 2);
        assert_eq!(
            prepared.compiled.chart.objects()[0].time.start,
            Timestamp::from_nanos(2_000_000_000)
        );
        assert!(
            prepared
                .source
                .notes
                .iter()
                .all(|n| n.lane.channel() == 0x11)
        );
        assert_eq!(section.tails.len(), 1);
        assert_eq!(section.tails[0].frame, 3);
        assert_eq!(section.tails[0].correction_ns, 200_000_000);
        assert_eq!(
            prepared
                .bank
                .get(section.tails[0].suffix)
                .unwrap()
                .samples(),
            &[3., 4., 5., 6., 7., 8., 9., 10., 11.]
        );
        assert!(
            prepared
                .bgm_commands
                .iter()
                .all(|command| command.at() >= section.start)
        );
        let empty = BTreeMap::new();
        let cfg = config(&empty, 550_000_000);
        let error = prepare_with(cfg, |actual| parsed(actual, TEXT))
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "missing --bind for BMS channel 11");
        let cfg = config(&empty, 5_000_000_000);
        let (prepared, _) = prepare_with(cfg, |actual| parsed(actual, TEXT)).unwrap();
        assert!(prepared.source.notes.is_empty());
    }
    #[test]
    fn negative_start_never_calls_loader_and_loader_errors_stay_exact() {
        let bindings = BTreeMap::new();
        let error = prepare_with(config(&bindings, -1), |_| {
            panic!("negative start before IO")
        })
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "practice start must be nonnegative");
        let error = prepare_with(config(&bindings, 0), |_| {
            Err("original decoder failure".into())
        })
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "original decoder failure");
    }
    #[test]
    fn same_pcm_budget_rejects_suffix_growth_and_section_errors_propagate() {
        let bindings = BTreeMap::from([(0x11, 4), (0x12, 5)]);
        let mut cfg = config(&bindings, 550_000_000);
        cfg.limits = PcmLimits::new(48, 48, 4).unwrap();
        let error = prepare_with(cfg, |actual| parsed(actual, TEXT))
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "section PCM exceeds total byte capacity");
        let cfg = config(&bindings, 550_000_000);
        let mut original = parsed(&cfg, TEXT).unwrap();
        original.bgm_commands[0] = AudioCommand::Stop {
            voice: VoiceId(1),
            at: Timestamp::ZERO,
        };
        let error = prepare_with(cfg, |_| Ok(original)).err().unwrap();
        assert_eq!(error.to_string(), "section BGM requires Play commands");
    }
}
