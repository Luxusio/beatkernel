//! Fresh practice preparation from original chart targets and decoded PCM.
use crate::PreparedBms;
use beatkernel::{
    audio::{AudioCommand, AudioLimits, PcmLimits, SampleBank, SampleId, VoiceId},
    runtime::restart::{FrameRounding, RestartPlan},
    time::Timestamp,
};
use std::{collections::BTreeSet, error::Error};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Original-source selection evidence; native presentation remains uncertain.
#[derive(Debug)]
pub struct TailSelection {
    pub voice: VoiceId,
    pub source: SampleId,
    pub suffix: SampleId,
    pub frame: usize,
    pub applied_song_time: Timestamp,
    pub correction_ns: i128,
}
#[derive(Debug)]
pub struct SectionReport {
    pub start: Timestamp,
    pub excluded_objects: usize,
    pub excluded_crossing_holds: usize,
    pub retired_bgm: usize,
    pub tails: Vec<TailSelection>,
}

/// Prepares an independent practice chart and bank before opening native output.
/// Objects with earlier heads (including crossing holds) are excluded. Targets,
/// BGM commands and all timing markers retain original absolute song times.
/// Each overlapping cue selects directly from its original PCM, never a suffix.
pub fn prepare_at(
    mut prepared: PreparedBms,
    start: Timestamp,
    limits: PcmLimits,
) -> Result<(PreparedBms, SectionReport)> {
    if start.as_nanos() < 0 {
        return Err("practice start must be nonnegative".into());
    }
    let mut report = SectionReport {
        start,
        excluded_objects: 0,
        excluded_crossing_holds: 0,
        retired_bgm: 0,
        tails: Vec::new(),
    };
    if start == Timestamp::ZERO {
        return Ok((prepared, report));
    }
    let retained: BTreeSet<_> = prepared
        .compiled
        .chart
        .objects()
        .iter()
        .filter(|object| object.time.start >= start)
        .map(|object| object.id)
        .collect();
    report.excluded_objects = prepared.compiled.chart.objects().len() - retained.len();
    report.excluded_crossing_holds = prepared
        .compiled
        .chart
        .objects()
        .iter()
        .filter(|object| {
            object.time.start < start && object.time.end.is_some_and(|end| end >= start)
        })
        .count();
    // Keep the original referenced bank available to replay/asset consumers.
    let originals: BTreeSet<_> = prepared
        .sounds
        .iter()
        .map(|sound| sound.sample)
        .chain(
            prepared
                .bgm_commands
                .iter()
                .filter_map(|command| match command {
                    AudioCommand::Play { sample, .. } => Some(*sample),
                    _ => None,
                }),
        )
        .collect();
    let max_samples = limits
        .max_samples()
        .checked_add(AudioLimits::MAX_VOICES)
        .ok_or("section sample capacity overflow")?;
    let section_limits = PcmLimits::new(
        limits.max_asset_bytes(),
        limits.max_total_bytes(),
        max_samples.min(PcmLimits::MAX_SAMPLES),
    )?;
    let mut suffixes = Vec::new();
    let mut total_bytes = prepared.bank.total_bytes();
    let mut next_sample = originals.iter().map(|sample| sample.0).max().unwrap_or(0);
    let mut commands = Vec::new();
    commands.try_reserve_exact(prepared.bgm_commands.len())?;
    for command in &prepared.bgm_commands {
        let AudioCommand::Play {
            voice,
            sample,
            at,
            gain,
        } = *command
        else {
            return Err("section BGM requires Play commands".into());
        };
        if at >= start {
            commands.push(*command);
            continue;
        }
        let source = prepared
            .bank
            .get(sample)
            .ok_or("section BGM asset missing")?;
        let delta = i128::from(start.as_nanos()) - i128::from(at.as_nanos());
        if delta * i128::from(source.format().sample_rate())
            >= (source.frames() as i128) * 1_000_000_000
        {
            report.retired_bgm += 1;
            continue;
        }
        let plan = RestartPlan::select(source, at, start, FrameRounding::Ceil)?;
        if plan.source_frame() == source.frames() {
            report.retired_bgm += 1;
            continue;
        }
        if report.tails.len() >= AudioLimits::MAX_VOICES {
            return Err("section crossing BGM exceeds4096 cues".into());
        }
        next_sample = next_sample
            .checked_add(1)
            .ok_or("section sample identity overflow")?;
        let suffix = SampleId(next_sample);
        let suffix_bytes = (source.frames() - plan.source_frame())
            .checked_mul(usize::from(source.format().channels()))
            .and_then(|samples| samples.checked_mul(std::mem::size_of::<f32>()))
            .ok_or("section PCM byte count overflow")?;
        total_bytes = total_bytes
            .checked_add(suffix_bytes)
            .ok_or("section PCM total overflow")?;
        if total_bytes > section_limits.max_total_bytes() {
            return Err("section PCM exceeds total byte capacity".into());
        }
        suffixes.try_reserve(1)?;
        suffixes.push((suffix, plan.copy_pcm(section_limits)?));
        commands.push(AudioCommand::Play {
            voice,
            sample: suffix,
            at: plan.applied_song_time(),
            gain,
        });
        report.tails.push(TailSelection {
            voice,
            source: sample,
            suffix,
            frame: plan.source_frame(),
            applied_song_time: plan.applied_song_time(),
            correction_ns: plan.correction_nanos(),
        });
    }
    if retained.is_empty() && commands.is_empty() {
        return Err("practice start has no remaining objects or BGM".into());
    }
    commands.sort_by_key(|command| match command {
        AudioCommand::Play { at, .. } => *at,
        _ => Timestamp::ZERO,
    });
    prepared
        .source
        .source
        .objects
        .retain(|object| retained.contains(&object.id));
    prepared
        .source
        .notes
        .retain(|note| retained.contains(&note.object));
    prepared
        .sounds
        .retain(|sound| retained.contains(&sound.object));
    prepared.compiled.chart = prepared.source.source.compile()?;
    prepared.bgm_commands = commands;
    let mut bank = SampleBank::new(prepared.bank.format(), section_limits)?;
    for (id, pcm) in prepared.bank.into_samples() {
        bank.insert(id, pcm)?;
    }
    for (id, pcm) in suffixes {
        bank.insert(id, pcm)?;
    }
    prepared.bank = bank;
    Ok((prepared, report))
}

/// Maps original song-time BGM once to a fresh output-relative timeline.
/// The existing feeder then adds preroll; no PCM or judgment timestamp changes.
pub fn relative_commands(
    mut commands: Vec<AudioCommand>,
    start: Timestamp,
) -> Result<Vec<AudioCommand>> {
    if start.as_nanos() < 0 {
        return Err("practice start must be nonnegative".into());
    }
    for command in &mut commands {
        let AudioCommand::Play { at, .. } = command else {
            return Err("section BGM requires Play commands".into());
        };
        let relative = i128::from(at.as_nanos()) - i128::from(start.as_nanos());
        if relative < 0 {
            return Err("section BGM precedes selected start".into());
        }
        *at = Timestamp::from_nanos(i64::try_from(relative)?);
    }
    Ok(commands)
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        audio::{AudioFormat, PcmSample},
        judge::JudgeStage,
        runtime::SoundBinding,
    };
    use beatkernel_bms::{ParseOptions, parse};
    fn limits() -> PcmLimits {
        PcmLimits::new(1024, 4096, 4).unwrap()
    }
    fn prepared() -> PreparedBms {
        let source = parse(
            "#BPM 120\n#WAV01 a.wav\n#00012:01\n#00051:0101\n#00111:01\n#00001:0101\n#00201:01\n",
            ParseOptions::default(),
        )
        .unwrap();
        let compiled = source.compile().unwrap();
        let mut bank = SampleBank::new(AudioFormat::new(8, 1).unwrap(), limits()).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(
                AudioFormat::new(4, 1).unwrap(),
                (0..12).map(|n| n as f32).collect(),
                limits(),
            )
            .unwrap(),
        )
        .unwrap();
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
                voice: VoiceId(note.object.0),
                gain: 1.0,
            })
            .collect();
        let first_voice = compiled
            .chart
            .objects()
            .iter()
            .map(|o| o.id.0)
            .max()
            .unwrap()
            + 1;
        let bgm_commands = compiled
            .bgm
            .iter()
            .enumerate()
            .map(|(index, cue)| AudioCommand::Play {
                voice: VoiceId(first_voice + index as u64),
                sample: cue.sample,
                at: cue.at,
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
    #[test]
    fn crossing_cues_have_independent_original_suffixes_and_absolute_targets() {
        let original = prepared();
        let original_pcm = original.bank.get(SampleId(1)).unwrap().samples().as_ptr();
        let future_targets: Vec<_> = original
            .compiled
            .chart
            .objects()
            .iter()
            .filter(|o| o.time.start.as_nanos() >= 550_000_000)
            .cloned()
            .collect();
        let (section, report) =
            prepare_at(original, Timestamp::from_nanos(550_000_000), limits()).unwrap();
        assert_eq!(section.compiled.chart.objects(), future_targets);
        assert_eq!(report.excluded_crossing_holds, 1);
        assert_eq!(report.tails.len(), 1);
        assert_eq!(report.tails[0].frame, 3);
        assert_eq!(report.tails[0].correction_ns, 200_000_000);
        assert_eq!(
            section.bank.get(report.tails[0].suffix).unwrap().samples()[0],
            3.0
        );
        assert_eq!(
            section.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
            original_pcm
        );
        let (section, report) =
            prepare_at(prepared(), Timestamp::from_nanos(1_100_000_000), limits()).unwrap();
        assert_eq!(report.tails.len(), 2);
        assert_ne!(report.tails[0].suffix, report.tails[1].suffix);
        assert_eq!(
            report
                .tails
                .iter()
                .map(|tail| tail.frame)
                .collect::<Vec<_>>(),
            [5, 1]
        );
        let mapped = relative_commands(section.bgm_commands, report.start).unwrap();
        assert!(
            mapped.iter().all(
                |command| matches!(command, AudioCommand::Play { at, .. } if at.as_nanos() >= 0)
            )
        );
        assert!(matches!(mapped[0], AudioCommand::Play { at, .. } if at.as_nanos() == 150_000_000));
    }
    #[test]
    fn repeated_fresh_selection_never_uses_a_previous_rounded_cursor() {
        let (_, first) =
            prepare_at(prepared(), Timestamp::from_nanos(550_000_000), limits()).unwrap();
        let (_, second) =
            prepare_at(prepared(), Timestamp::from_nanos(550_000_000), limits()).unwrap();
        assert_eq!(first.tails[0].frame, second.tails[0].frame);
        assert_eq!(
            first.tails[0].applied_song_time,
            second.tails[0].applied_song_time
        );
        let original = prepared();
        let ptr = original.bank.get(SampleId(1)).unwrap().samples().as_ptr();
        let (full, report) = prepare_at(original, Timestamp::ZERO, limits()).unwrap();
        assert_eq!(full.bank.get(SampleId(1)).unwrap().samples().as_ptr(), ptr);
        assert!(report.tails.is_empty());
        assert_eq!(report.excluded_objects, 0);
    }
    #[test]
    fn expired_cues_capacity_and_invalid_positions_fail_explicitly() {
        let (_, report) =
            prepare_at(prepared(), Timestamp::from_nanos(3_000_000_000), limits()).unwrap();
        assert_eq!(report.retired_bgm, 1); // Cue at0 ends at3; cue at1 still has a tail.
        assert!(prepare_at(prepared(), Timestamp::from_nanos(-1), limits()).is_err());
        assert!(prepare_at(prepared(), Timestamp::from_nanos(i64::MAX), limits()).is_err());
        assert!(
            prepare_at(
                prepared(),
                Timestamp::from_nanos(550_000_000),
                PcmLimits::new(48, 48, 1).unwrap()
            )
            .is_err()
        );
        assert!(
            relative_commands(
                vec![AudioCommand::Play {
                    voice: VoiceId(1),
                    sample: SampleId(1),
                    at: Timestamp::ZERO,
                    gain: 1.0
                }],
                Timestamp::from_nanos(1)
            )
            .is_err()
        );
    }
}
