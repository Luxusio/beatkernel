//! Optional original WAV00 bindings, without asset loading or gauge policy.

use crate::mine_plan::MinePlan;
use beatkernel::{
    audio::{AudioCommand, SampleId, VoiceId},
    judge::HazardId,
    runtime::{
        hazard_sound::{HazardSoundBinding, HazardSoundTimeline},
        input_sound::InputSoundTimeline,
        SoundBinding,
    },
};
use beatkernel_bms::BmsChart;
use std::collections::{BTreeMap, BTreeSet};

/// Validated optional explosion bindings and their required original sample.
#[derive(Clone, Debug, PartialEq)]
pub struct MineSoundPlan {
    timeline: Option<HazardSoundTimeline>,
    samples: Vec<SampleId>,
}

impl MineSoundPlan {
    /// Validates original mines before selecting audible nonfatal markers.
    /// One voice per control follows every supplied occupied voice.
    pub fn prepare(
        source: &BmsChart,
        sounds: &[SoundBinding],
        bgm: &[AudioCommand],
        input_sounds: Option<&InputSoundTimeline>,
        max_markers: usize,
    ) -> Result<Self, String> {
        let mines = MinePlan::prepare(source, max_markers)?;
        let mut occupied = 0u64;
        for sound in sounds {
            if !sound.gain.is_finite() {
                return Err("occupied gameplay sound has nonfinite gain".into());
            }
            occupied = occupied.max(sound.voice.0);
        }
        for command in bgm {
            let AudioCommand::Play { voice, gain, .. } = command else {
                return Err("occupied BGM command is not Play".into());
            };
            if !gain.is_finite() {
                return Err("occupied BGM sound has nonfinite gain".into());
            }
            occupied = occupied.max(voice.0);
        }
        if let Some(input_sounds) = input_sounds {
            for marker in input_sounds.markers() {
                occupied = occupied.max(marker.voice.0);
            }
        }
        if !source.samples.contains_key(&0)
            || !mines.markers().iter().any(|mine| !mine.damage.is_fatal())
        {
            return Ok(Self {
                timeline: None,
                samples: Vec::new(),
            });
        }
        let gain = source.wav_gain().map_err(|error| error.to_string())?;
        let controls: BTreeSet<_> = mines
            .markers()
            .iter()
            .filter(|mine| !mine.damage.is_fatal())
            .map(|mine| mine.lane.control().0)
            .collect();
        let mut voices = BTreeMap::new();
        let mut next = occupied.checked_add(1);
        for control in controls {
            let voice = next.ok_or("mine sound voice namespace exhausted")?;
            voices.insert(control, VoiceId(voice));
            next = voice.checked_add(1);
        }
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(mines.markers().len())
            .map_err(|_| "mine sound binding allocation failed")?;
        bindings.extend(
            mines
                .markers()
                .iter()
                .filter(|mine| !mine.damage.is_fatal())
                .map(|mine| HazardSoundBinding {
                    hazard: HazardId(mine.ordinal),
                    sample: SampleId(0),
                    voice: voices[&mine.lane.control().0],
                    gain,
                }),
        );
        let timeline =
            HazardSoundTimeline::new(bindings, max_markers).map_err(|error| error.to_string())?;
        Ok(Self {
            timeline: Some(timeline),
            samples: vec![SampleId(0)],
        })
    }

    /// Borrows exact original hazard bindings in ascending identity order.
    pub fn bindings(&self) -> &[HazardSoundBinding] {
        self.timeline
            .as_ref()
            .map_or(&[], HazardSoundTimeline::bindings)
    }
    /// Original PCM requirements: empty or the single optional WAV00 identity.
    pub fn samples(&self) -> &[SampleId] {
        &self.samples
    }
    /// Clones validated core bindings only when there are audible markers.
    pub fn timeline(&self) -> Option<HazardSoundTimeline> {
        self.timeline.clone()
    }
}

/// Noncryptographic identity of actual audible mine selections, excluding paths
/// and output voice remapping. Hazard judgment identity separately owns damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MineSoundIdentity(u64);
impl MineSoundIdentity {
    /// Hashes nonfatal ordinal/control/time, finite gain and SampleId(0), excluding
    /// resource paths and output voices. Unused WAV00 creates no extension.
    pub fn from_source(source: &BmsChart) -> Result<Option<Self>, String> {
        let mines = MinePlan::prepare(source, beatkernel_bms::ParseOptions::default().max_objects)?;
        if !source.samples.contains_key(&0)
            || !mines.markers().iter().any(|mine| !mine.damage.is_fatal())
        {
            return Ok(None);
        }
        let gain = source.wav_gain().map_err(|error| error.to_string())?;
        let mut audible: Vec<&beatkernel_bms::ScheduledMine> = Vec::new();
        audible
            .try_reserve_exact(mines.markers().len())
            .map_err(|_| "mine sound identity allocation failed")?;
        audible.extend(
            mines
                .markers()
                .iter()
                .filter(|mine| !mine.damage.is_fatal()),
        );
        audible.sort_unstable_by_key(|mine| mine.ordinal);
        let mut hash = 14_695_981_039_346_656_037u64;
        let mut feed = |bytes: &[u8]| {
            for &byte in bytes {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(1_099_511_628_211);
            }
        };
        feed(b"beatkernel-bms/mine-sounds/v1");
        feed(&(audible.len() as u64).to_le_bytes());
        feed(&gain.to_bits().to_le_bytes());
        feed(&SampleId(0).0.to_le_bytes());
        for mine in audible {
            feed(&mine.ordinal.to_le_bytes());
            feed(&mine.lane.control().0.to_le_bytes());
            feed(&mine.at.as_nanos().to_le_bytes());
        }
        Ok(Some(Self(hash)))
    }
    /// Returns the versioned FNV-1a64 compatibility value, not an asset digest.
    pub const fn fingerprint(self) -> u64 {
        self.0
    }
}
