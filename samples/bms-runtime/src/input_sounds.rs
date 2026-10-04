//! Pure preparation of unjudged BMS press sounds and their dedicated voices.

use beatkernel::{
    audio::{AudioCommand, SampleId, VoiceId},
    runtime::{
        input_sound::{InputSoundError, InputSoundMarker, InputSoundTimeline},
        SoundBinding,
    },
};
use beatkernel_bms::BmsChart;
use std::collections::{BTreeMap, BTreeSet};

/// Noncryptographic compatibility identity of validated invisible selections.
/// Source lines, resource paths, output clocks and voice assignments are excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputSoundIdentity(u64);

impl InputSoundIdentity {
    /// Hashes actual compiled selections in semantic control/time order. Empty
    /// sources have no extension and retain their exact legacy setup identity.
    pub fn from_source(source: &BmsChart) -> Result<Option<Self>, String> {
        if source.invisible.is_empty() {
            return Ok(None);
        }
        let mut events = source
            .compile_invisible()
            .map_err(|error| error.to_string())?;
        let gain = source.wav_gain().map_err(|error| error.to_string())?;
        events.sort_unstable_by_key(|event| (event.lane.control().0, event.at));
        for pair in events.windows(2) {
            if pair[0].lane.control() == pair[1].lane.control() && pair[0].at == pair[1].at {
                return Err("duplicate invisible input-sound control/time".into());
            }
        }
        let mut hash = 14_695_981_039_346_656_037u64;
        let mut feed = |bytes: &[u8]| {
            for &byte in bytes {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(1_099_511_628_211);
            }
        };
        feed(b"beatkernel-bms/input-sounds/v1");
        feed(&(events.len() as u64).to_le_bytes());
        feed(&gain.to_bits().to_le_bytes());
        for event in events {
            feed(&event.lane.control().0.to_le_bytes());
            feed(&event.at.as_nanos().to_le_bytes());
            feed(&event.sample.0.to_le_bytes());
        }
        Ok(Some(Self(hash)))
    }

    /// Returns the versioned semantic FNV-1a64 fingerprint, not an asset digest.
    pub const fn fingerprint(self) -> u64 {
        self.0
    }
}

/// An immutable input-sound plan; preparation neither loads PCM nor schedules it.
#[derive(Clone, Debug, PartialEq)]
pub struct InputSoundPlan {
    markers: Vec<InputSoundMarker>,
    samples: Vec<SampleId>,
    timeline: InputSoundTimeline,
}

impl InputSoundPlan {
    /// Compiles the actual invisible timeline and WAV gain, assigning one
    /// reusable voice per lane in ascending logical-control order. New voices
    /// follow every supplied gameplay/BGM voice; exhaustion returns no plan.
    pub fn prepare(
        source: &BmsChart,
        sounds: &[SoundBinding],
        bgm: &[AudioCommand],
        max_markers: usize,
    ) -> Result<Self, String> {
        let empty =
            InputSoundTimeline::new(Vec::new(), max_markers).map_err(|error| error.to_string())?;
        if source.invisible.len() > max_markers {
            return Err(InputSoundError::TooManyMarkers.to_string());
        }
        let mut occupied = 0u64;
        for sound in sounds {
            if !sound.gain.is_finite() {
                return Err("occupied gameplay sound has nonfinite gain".into());
            }
            occupied = occupied.max(sound.voice.0);
        }
        for command in bgm {
            match command {
                AudioCommand::Play { voice, gain, .. } => {
                    if !gain.is_finite() {
                        return Err("occupied BGM sound has nonfinite gain".into());
                    }
                    occupied = occupied.max(voice.0);
                }
                _ => return Err("occupied BGM command is not Play".into()),
            }
        }
        let gain = source.wav_gain().map_err(|error| error.to_string())?;
        let events = source
            .compile_invisible()
            .map_err(|error| error.to_string())?;
        if events.is_empty() {
            return Ok(Self {
                markers: Vec::new(),
                samples: Vec::new(),
                timeline: empty,
            });
        }

        let controls: BTreeSet<_> = events.iter().map(|event| event.lane.control().0).collect();
        let mut voices = BTreeMap::new();
        let mut next = occupied.checked_add(1);
        for control in controls {
            let voice = next.ok_or("input sound voice namespace exhausted")?;
            voices.insert(control, VoiceId(voice));
            next = voice.checked_add(1);
        }
        let mut markers = Vec::new();
        markers
            .try_reserve_exact(events.len())
            .map_err(|_| "input sound marker allocation failed")?;
        let mut samples = BTreeSet::new();
        for event in events {
            let control = event.lane.control();
            markers.push(InputSoundMarker {
                control,
                at: event.at,
                sample: event.sample,
                voice: voices[&control.0],
                gain,
            });
            samples.insert(event.sample);
        }
        let timeline = InputSoundTimeline::new(markers.clone(), max_markers)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            markers,
            samples: samples.into_iter().collect(),
            timeline,
        })
    }

    /// Original timed selections in compiled time/ordinal order.
    pub fn markers(&self) -> &[InputSoundMarker] {
        &self.markers
    }

    /// Original PCM identities required by this plan, sorted and deduplicated.
    pub fn samples(&self) -> &[SampleId] {
        &self.samples
    }

    /// Clones the validated immutable core index for an actual runtime owner.
    pub fn timeline(&self) -> InputSoundTimeline {
        self.timeline.clone()
    }
}
