//! Prepared gameplay-voice stops shared by execution and off-thread planning.

use crate::{
    audio::{AudioCommand, CommandPushError, VoiceId},
    time::Timestamp,
};

/// One scheduled gameplay-voice stop attempt, preserving callback evidence.
#[derive(Clone, Debug)]
pub struct RuntimeSoundStopReport {
    /// Effective output-domain time, no earlier than accepted gameplay audio.
    pub at: Timestamp,
    /// Stop commands accepted by the callback in prepared voice order.
    pub commands: Vec<AudioCommand>,
    /// Original rejected commands and reasons; no automatic retry is permitted.
    pub failures: Vec<CommandPushError>,
}

/// Caller-bounded unique voices, accepted output watermark and one-attempt latch.
///
/// The caller decides when stopping is authorized. This state owns no judge,
/// queue or transport. Callback success proves only the caller's admission;
/// planning success does not prove queue admission, execution or silence.
#[derive(Debug)]
pub struct GameplaySoundStop {
    voices: Vec<VoiceId>,
    watermark: Option<Timestamp>,
    attempted: bool,
}

impl GameplaySoundStop {
    /// Sorts and deduplicates the caller's already bounded gameplay voice list.
    pub fn new(mut voices: Vec<VoiceId>) -> Self {
        voices.sort_unstable();
        voices.dedup();
        Self {
            voices,
            watermark: None,
            attempted: false,
        }
    }

    /// Prepared ascending unique gameplay voices, excluding caller-owned BGM.
    pub fn voices(&self) -> &[VoiceId] {
        &self.voices
    }

    /// Records a successfully admitted or planned gameplay command's output time.
    pub fn observe_admitted(&mut self, at: Timestamp) {
        self.watermark = Some(self.watermark.map_or(at, |previous| previous.max(at)));
    }

    /// Attempts every prepared voice once at the later of request and watermark.
    ///
    /// All voices are attempted even after callback refusal. A prior attempt,
    /// including an empty or partially rejected attempt, returns `None` until reset.
    pub fn attempt(
        &mut self,
        requested_at: Timestamp,
        mut admit: impl FnMut(AudioCommand) -> Result<(), CommandPushError>,
    ) -> Option<RuntimeSoundStopReport> {
        if self.attempted {
            return None;
        }
        let at = self
            .watermark
            .map_or(requested_at, |latest| requested_at.max(latest));
        let mut report = RuntimeSoundStopReport {
            at,
            commands: Vec::with_capacity(self.voices.len()),
            failures: Vec::with_capacity(self.voices.len()),
        };
        self.attempted = true;
        for &voice in &self.voices {
            let command = AudioCommand::Stop { voice, at };
            match admit(command) {
                Ok(()) => report.commands.push(command),
                Err(error) => report.failures.push(error),
            }
        }
        Some(report)
    }

    /// Clears the watermark and attempt latch during explicit owner restoration.
    /// Prepared voices remain unchanged; no existing output commands are removed.
    pub fn reset(&mut self) {
        self.watermark = None;
        self.attempted = false;
    }
}
