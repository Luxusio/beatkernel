//! Retained Source epoch publication with original native units and source history.
use super::*;
use crate::{
    audio_authority::PreparedPrimedCoordinateEpoch, gameplay::output::ports::SourcePlaybackIdentity,
};
use beatkernel::audio::{MixerConfig, RenderReport, SourcePlaybackSpan};
use beatkernel_platform::audio::{NativeOutputLayout, NativeOutputState};

/// Cold semantic association captured while the recovered output is exclusively
/// owned. The controller retains that unique owner across its subsequent open;
/// arithmetic identity alone does not establish that ownership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCoordinateSourceOwnerAssociation {
    previous_epoch: AudioAuthorityEpoch,
    previous_record: Option<NativePresentationRecord>,
    previous_frame_position: Option<(NativeFrameBasis, u64)>,
    previous_basis: OutputFrameBasis,
    previous_coordinate_basis: AudioAuthorityCoordinateBasis,
    previous_source_identity: Option<SourcePlaybackIdentity>,
    practice_mixer_basis: OutputFrameBasis,
    source_identity: SourcePlaybackIdentity,
    basis: OutputFrameBasis,
    pulled_source_end: u64,
    playback_frame: u64,
    history_capacity: usize,
    history_retirement_revision: u64,
    history: Vec<SourcePlaybackSpan>,
    mixer_config: MixerConfig,
    output_layout: Option<NativeOutputLayout>,
    output_matrix: Option<(u16, u16, Vec<u32>)>,
    max_frames: usize,
    pending_report: Option<RenderReport>,
    admitted_frames: usize,
    applied_start_frame: Option<u64>,
    paused: bool,
    pause_requested: bool,
}
impl PreparedCoordinateSourceOwnerAssociation {
    pub const fn previous_epoch(&self) -> u64 {
        self.previous_epoch.id
    }
    pub const fn previous_coordinate_basis(&self) -> AudioAuthorityCoordinateBasis {
        self.previous_coordinate_basis
    }
    pub const fn basis(&self) -> OutputFrameBasis {
        self.basis
    }
    pub const fn practice_mixer_basis(&self) -> OutputFrameBasis {
        self.practice_mixer_basis
    }
    pub const fn source_identity(&self) -> SourcePlaybackIdentity {
        self.source_identity
    }
    pub const fn pulled_source_end(&self) -> u64 {
        self.pulled_source_end
    }
    pub const fn playback_frame(&self) -> u64 {
        self.playback_frame
    }

    /// Recheck the recovered owner before moving it into native open. This does
    /// not sample a native clock, render PCM, mutate history or allocate.
    pub fn validate_recovered_state(&self, state: &NativeOutputState) -> NativeGameplayResult<()> {
        let mixer = state.mixer();
        let config = mixer.config();
        let history = mixer
            .source_playback_history()
            .ok_or("Source replacement lost original playback history")?;
        if state.admission_is_uncertain()
            || config != self.mixer_config
            || state.layout() != self.output_layout.as_ref()
            || match (&self.output_matrix, state.channel_matrix()) {
                (None, None) => false,
                (Some((source, target, coefficients)), Some(matrix)) => {
                    *source != matrix.source_channels()
                        || *target != matrix.target_channels()
                        || coefficients.len() != matrix.coefficients().len()
                        || coefficients
                            .iter()
                            .zip(matrix.coefficients())
                            .any(|(saved, current)| *saved != current.to_bits())
                }
                _ => true,
            }
            || state.max_frames() != self.max_frames
            || config.domain() != self.practice_mixer_basis.origin().domain
            || config.origin() != self.practice_mixer_basis.origin().timestamp
            || config.format().sample_rate() != self.practice_mixer_basis.sample_rate()
            || state.output_frame_basis() != self.basis
            || mixer.frame_cursor() != self.pulled_source_end
            || mixer.playback_frame_cursor() != self.playback_frame
            || mixer.applied_start_frame() != self.applied_start_frame
            || mixer.is_paused() != self.paused
            || mixer.pause_requested() != self.pause_requested
            || state.pending_report() != self.pending_report
            || state.admitted_frames() != self.admitted_frames
            || history.source_rate() != self.source_identity.hz()
            || history.initial_physical_frame() != self.source_identity.initial_physical()
            || history.initial_playback_frame() != self.source_identity.initial_playback()
            || history.pulled_end_frame() != self.pulled_source_end
            || history.capacity() != self.history_capacity
            || history.retirement_revision() != self.history_retirement_revision
            || history.len() != self.history.len()
            || self
                .history
                .iter()
                .enumerate()
                .any(|(index, span)| history.span(index) != Some(*span))
        {
            return Err(
                "Source replacement changed the recovered owner or original history".into(),
            );
        }
        Ok(())
    }

    fn validate_presentation(&self, current: &NativeAudioPresentation) -> NativeGameplayResult<()> {
        current.validate_coordinate_seed()?;
        if current.authority.epoch() != self.previous_epoch
            || current.validator.latest_record() != self.previous_record
            || current.validator.latest_frame_position() != self.previous_frame_position
            || current.basis != Some(self.previous_basis)
            || current.target_basis.is_some()
            || current.authority.coordinate_basis() != Some(self.previous_coordinate_basis)
            || current.practice_mixer_basis != Some(self.practice_mixer_basis)
            || current.source_owner_identity != self.previous_source_identity
        {
            return Err("coordinate Source native owner changed after cold association".into());
        }
        Ok(())
    }
}

/// Original native evidence staged against the complete prior presentation and
/// its recovered Source association. Created descriptors grant no progress.
#[derive(Clone, Debug)]
pub struct PreparedCoordinateSourceNativeAudioEpoch {
    association: PreparedCoordinateSourceOwnerAssociation,
    validator: NativePresentationValidator,
    coordinate_basis: AudioAuthorityCoordinateBasis,
    snapshots: [CoordinateNativeAudioSnapshot; 2],
    correlation: PreparedPrimedCoordinateEpoch,
}
impl PreparedCoordinateSourceNativeAudioEpoch {
    pub const fn basis(&self) -> OutputFrameBasis {
        self.association.basis
    }
    pub const fn coordinate_basis(&self) -> AudioAuthorityCoordinateBasis {
        self.coordinate_basis
    }
    pub const fn snapshots(&self) -> [CoordinateNativeAudioSnapshot; 2] {
        self.snapshots
    }
    pub fn epoch(&self) -> u64 {
        self.validator.epoch()
    }
    pub fn latest_record(&self) -> NativePresentationRecord {
        self.validator
            .latest_record()
            .expect("two original Source observations staged")
    }
    pub const fn practice_mixer_basis(&self) -> OutputFrameBasis {
        self.association.practice_mixer_basis
    }
    pub const fn source_identity(&self) -> SourcePlaybackIdentity {
        self.association.source_identity
    }
    pub fn authority_token(&self) -> &PreparedPrimedCoordinateEpoch {
        &self.correlation
    }
}

impl NativeAudioPresentation {
    /// Capture original history and the first unsent native basis while the
    /// complete recovered Source output is still exclusively borrowed.
    pub fn prepare_coordinate_source_owner_association(
        &self,
        state: &NativeOutputState,
        source_identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<PreparedCoordinateSourceOwnerAssociation> {
        self.validate_coordinate_seed()?;
        let previous_coordinate_basis = self
            .authority
            .coordinate_basis()
            .ok_or("Source replacement lacks original coordinate descriptor")?;
        let previous_basis = match previous_coordinate_basis {
            AudioAuthorityCoordinateBasis::SourceFrame(basis)
            | AudioAuthorityCoordinateBasis::SourceCounter { basis, .. } => basis,
            _ => return Err("Source replacement requires original Source coordinates".into()),
        };
        let pin = self
            .practice_mixer_basis
            .ok_or("Source replacement lacks original Mixer creation pin")?;
        let mixer = state.mixer();
        let history = mixer
            .source_playback_history()
            .ok_or("Source replacement lacks original playback history")?;
        let basis = state.output_frame_basis();
        if self.basis != Some(previous_basis)
            || (pin != previous_basis && !self.source_owner_preserves_pin(previous_basis, pin))
            || self
                .source_owner_identity
                .is_some_and(|identity| identity != source_identity)
            || pin.origin() != basis.origin()
            || pin.sample_rate() != basis.sample_rate()
            || pin.sample_rate() != source_identity.hz()
            || pin.start_physical_frame() < source_identity.initial_physical()
            || basis.start_physical_frame() < previous_basis.start_physical_frame()
            || basis.start_physical_frame() < pin.start_physical_frame()
            || basis.start_physical_frame() > mixer.frame_cursor()
            || history.pulled_end_frame() != mixer.frame_cursor()
        {
            return Err("Source replacement differs from retained creation pin/history".into());
        }
        let association = PreparedCoordinateSourceOwnerAssociation {
            previous_epoch: self.authority.epoch(),
            previous_record: self.validator.latest_record(),
            previous_frame_position: self.validator.latest_frame_position(),
            previous_basis,
            previous_coordinate_basis,
            previous_source_identity: self.source_owner_identity,
            practice_mixer_basis: pin,
            source_identity,
            basis,
            pulled_source_end: mixer.frame_cursor(),
            playback_frame: mixer.playback_frame_cursor(),
            history_capacity: history.capacity(),
            history_retirement_revision: history.retirement_revision(),
            history: (0..history.len())
                .map(|index| history.span(index).expect("retained Source history span"))
                .collect(),
            mixer_config: mixer.config(),
            output_layout: state.layout().copied(),
            output_matrix: state.channel_matrix().map(|matrix| {
                (
                    matrix.source_channels(),
                    matrix.target_channels(),
                    matrix
                        .coefficients()
                        .iter()
                        .map(|gain| gain.to_bits())
                        .collect(),
                )
            }),
            max_frames: state.max_frames(),
            pending_report: state.pending_report(),
            admitted_frames: state.admitted_frames(),
            applied_start_frame: mixer.applied_start_frame(),
            paused: mixer.is_paused(),
            pause_requested: mixer.pause_requested(),
        };
        association.validate_recovered_state(state)?;
        Ok(association)
    }

    /// Stage genuine native Frame or Counter evidence with the created
    /// descriptor. A Counter frequency belongs to this newly opened stream.
    pub fn prepare_coordinate_source_output_epoch(
        &self,
        next: AudioAuthorityEpoch,
        coordinate_basis: AudioAuthorityCoordinateBasis,
        snapshots: [CoordinateNativeAudioSnapshot; 2],
        association: &PreparedCoordinateSourceOwnerAssociation,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<PreparedCoordinateSourceNativeAudioEpoch> {
        association.validate_presentation(self)?;
        let basis = match coordinate_basis {
            AudioAuthorityCoordinateBasis::SourceFrame(basis)
            | AudioAuthorityCoordinateBasis::SourceCounter { basis, .. } => basis,
            _ => return Err("Source replacement requires an actual Source descriptor".into()),
        };
        if basis != association.basis {
            return Err("created Source basis differs from recovered first unsent frame".into());
        }
        let staged = self
            .authority
            .prepare_coordinate_epoch(next, coordinate_basis, merger)?;
        let mut validator =
            NativePresentationValidator::new(next.id, next.stream_origin, next.host_domain);
        let mut observations = [None; 2];
        for (index, original) in snapshots.into_iter().enumerate() {
            let snapshot = original.snapshot();
            let coordinate = original.coordinate();
            if snapshot.epoch != next.id || snapshot.basis != basis {
                return Err("coordinate Source snapshot creation epoch/basis differs".into());
            }
            let prepared = match (coordinate_basis, snapshot.evidence, coordinate) {
                (
                    AudioAuthorityCoordinateBasis::SourceFrame(_),
                    OriginalNativePresentationEvidence::SuppliedPair(pair),
                    NativeOutputCoordinate::StreamFrame { frame },
                ) => validator.prepare_frame_pair(
                    snapshot.epoch,
                    pair,
                    NativeFrameBasis::Source(basis),
                    frame,
                )?,
                (
                    AudioAuthorityCoordinateBasis::SourceFrame(_),
                    OriginalNativePresentationEvidence::Asio { .. },
                    NativeOutputCoordinate::StreamFrame { .. },
                ) => prepare_native_snapshot(&validator, snapshot)?,
                (
                    AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. },
                    OriginalNativePresentationEvidence::Wasapi { .. },
                    NativeOutputCoordinate::Counter {
                        frequency: actual, ..
                    },
                ) if frequency == actual => prepare_native_snapshot(&validator, snapshot)?,
                _ => return Err("Source native evidence differs from created descriptor".into()),
            };
            let pair = prepared
                .correlation_pair()
                .ok_or("coordinate Source replacement requires two progressing observations")?;
            observations[index] = Some((pair, coordinate));
            validator.commit(prepared)?;
        }
        let correlation = self.authority.prepare_primed_coordinate_epoch(
            staged,
            [
                observations[0].expect("first original Source coordinate"),
                observations[1].expect("second original Source coordinate"),
            ],
            now,
            merger,
        )?;
        Ok(PreparedCoordinateSourceNativeAudioEpoch {
            association: association.clone(),
            validator,
            coordinate_basis,
            snapshots,
            correlation,
        })
    }

    /// Recheck every prior native sidecar, original pin, Source association,
    /// authority frontier and merger semantic state before publication.
    pub fn validate_coordinate_source_output_epoch(
        &self,
        prepared: &PreparedCoordinateSourceNativeAudioEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        prepared.association.validate_presentation(self)?;
        self.authority
            .validate_primed_coordinate_epoch(&prepared.correlation, now, merger)?;
        Ok(())
    }

    pub fn commit_coordinate_source_output_epoch(
        &mut self,
        prepared: PreparedCoordinateSourceNativeAudioEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        self.commit_coordinate_source_output_epoch_borrowed(&prepared, now, merger)
    }

    /// All fallible work precedes publication. Borrowing avoids copying the
    /// cold Source history or merger registry while publishing a Ready output.
    pub(crate) fn commit_coordinate_source_output_epoch_borrowed(
        &mut self,
        prepared: &PreparedCoordinateSourceNativeAudioEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        self.validate_coordinate_source_output_epoch(prepared, now, merger)?;
        let validator = prepared.validator.clone();
        self.authority.commit_primed_coordinate_epoch_borrowed(
            &prepared.correlation,
            now,
            merger,
        )?;
        self.validator = validator;
        self.basis = Some(prepared.association.basis);
        self.target_basis = None;
        self.practice_mixer_basis = Some(prepared.association.practice_mixer_basis);
        self.source_owner_identity = Some(prepared.association.source_identity);
        Ok(())
    }

    pub(super) fn source_owner_preserves_pin(
        &self,
        basis: OutputFrameBasis,
        pin: OutputFrameBasis,
    ) -> bool {
        self.practice_mixer_basis == Some(pin)
            && self.basis == Some(basis)
            && self.source_owner_identity.is_some_and(|identity| {
                identity.hz() == pin.sample_rate()
                    && pin.origin() == basis.origin()
                    && pin.sample_rate() == basis.sample_rate()
                    && identity.initial_physical() <= pin.start_physical_frame()
                    && pin.start_physical_frame() <= basis.start_physical_frame()
            })
    }
}

#[cfg(test)]
#[path = "native_audio_presentation_source_coordinate_epoch_fixtures.rs"]
mod fixtures;
