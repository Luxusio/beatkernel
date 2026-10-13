//! Retained Target epoch publication with original native frames and Mixer pin.
use super::*;
use crate::audio_authority::PreparedPrimedCoordinateEpoch;

/// Opaque native preparation bound to the complete prior presentation owner.
/// The original captured Mixer pin is associated with the recovered output
/// owner; matching it does not assert a new frame-zero relation.
#[derive(Clone, Debug)]
pub struct PreparedCoordinateTargetNativeAudioEpoch {
    previous_epoch: AudioAuthorityEpoch,
    previous_record: Option<NativePresentationRecord>,
    previous_frame_position: Option<(NativeFrameBasis, u64)>,
    previous_basis: Option<OutputFrameBasis>,
    previous_target_basis: Option<TargetFrameBasis>,
    previous_coordinate_basis: Option<AudioAuthorityCoordinateBasis>,
    practice_mixer_basis: OutputFrameBasis,
    validator: NativePresentationValidator,
    basis: TargetFrameBasis,
    snapshots: [CoordinateTargetNativeAudioSnapshot; 2],
    correlation: PreparedPrimedCoordinateEpoch,
}
impl PreparedCoordinateTargetNativeAudioEpoch {
    pub const fn basis(&self) -> TargetFrameBasis {
        self.basis
    }
    pub const fn snapshots(&self) -> [CoordinateTargetNativeAudioSnapshot; 2] {
        self.snapshots
    }
    pub fn epoch(&self) -> u64 {
        self.validator.epoch()
    }
    pub fn latest_record(&self) -> NativePresentationRecord {
        self.validator
            .latest_record()
            .expect("two original Target frame observations staged")
    }
    pub const fn practice_mixer_basis(&self) -> OutputFrameBasis {
        self.practice_mixer_basis
    }
    pub fn authority_token(&self) -> &PreparedPrimedCoordinateEpoch {
        &self.correlation
    }
}

impl NativeAudioPresentation {
    /// Stage two original Target frame observations while retaining the original
    /// Mixer's creation grid. The caller retains the original pin and checks
    /// the complete recovered owner, without substituting its next-frame basis.
    pub fn prepare_coordinate_target_output_epoch(
        &self,
        next: AudioAuthorityEpoch,
        basis: TargetFrameBasis,
        snapshots: [CoordinateTargetNativeAudioSnapshot; 2],
        preserved_mixer_basis: OutputFrameBasis,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<PreparedCoordinateTargetNativeAudioEpoch> {
        self.validate_coordinate_seed()?;
        if !matches!(
            self.authority.coordinate_basis(),
            Some(AudioAuthorityCoordinateBasis::TargetFrame(_))
        ) || self.target_basis.is_none()
            || self.practice_mixer_basis != Some(preserved_mixer_basis)
        {
            return Err("coordinate Target replacement differs from retained Mixer pin".into());
        }
        let staged = self.authority.prepare_coordinate_epoch(
            next,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            merger,
        )?;
        let mut validator =
            NativePresentationValidator::new(next.id, next.stream_origin, next.host_domain);
        let mut observations = [None; 2];
        for (index, original) in snapshots.into_iter().enumerate() {
            let snapshot = original.snapshot();
            if snapshot.epoch != next.id || snapshot.basis != basis {
                return Err("coordinate Target snapshot creation epoch/basis differs".into());
            }
            let NativeOutputCoordinate::StreamFrame { frame } = original.coordinate() else {
                return Err("coordinate Target replacement requires original stream frames".into());
            };
            let OriginalNativePresentationEvidence::SuppliedPair(pair) = snapshot.evidence else {
                return Err(
                    "coordinate Target replacement requires original frame evidence".into(),
                );
            };
            let prepared = validator.prepare_frame_pair(
                snapshot.epoch,
                pair,
                NativeFrameBasis::Target(basis),
                frame,
            )?;
            let pair = prepared
                .correlation_pair()
                .ok_or("coordinate Target replacement requires two progressing frames")?;
            observations[index] = Some((pair, original.coordinate()));
            validator.commit(prepared)?;
        }
        let correlation = self.authority.prepare_primed_coordinate_epoch(
            staged,
            [
                observations[0].expect("first original Target frame"),
                observations[1].expect("second original Target frame"),
            ],
            now,
            merger,
        )?;
        Ok(PreparedCoordinateTargetNativeAudioEpoch {
            previous_epoch: self.authority.epoch(),
            previous_record: self.validator.latest_record(),
            previous_frame_position: self.validator.latest_frame_position(),
            previous_basis: self.basis,
            previous_target_basis: self.target_basis,
            previous_coordinate_basis: self.authority.coordinate_basis(),
            practice_mixer_basis: preserved_mixer_basis,
            validator,
            basis,
            snapshots,
            correlation,
        })
    }

    /// Revalidate native sidecars, the original retained pin, authority and input
    /// chronology before any output owner can be published.
    pub fn validate_coordinate_target_output_epoch(
        &self,
        prepared: &PreparedCoordinateTargetNativeAudioEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        self.validate_coordinate_seed()?;
        if prepared.previous_epoch != self.authority.epoch()
            || prepared.previous_record != self.validator.latest_record()
            || prepared.previous_frame_position != self.validator.latest_frame_position()
            || prepared.previous_basis != self.basis
            || prepared.previous_target_basis != self.target_basis
            || prepared.previous_coordinate_basis != self.authority.coordinate_basis()
            || Some(prepared.practice_mixer_basis) != self.practice_mixer_basis
        {
            return Err("coordinate Target native owner changed after preparation".into());
        }
        self.authority
            .validate_primed_coordinate_epoch(&prepared.correlation, now, merger)?;
        Ok(())
    }

    /// Commit the already reserved authority and native frame metadata together.
    /// All fallible checks precede field publication; the original practice pin
    /// remains associated with the same retained Mixer across target-rate changes.
    pub fn commit_coordinate_target_output_epoch(
        &mut self,
        prepared: PreparedCoordinateTargetNativeAudioEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        self.validate_coordinate_target_output_epoch(&prepared, now, merger)?;
        self.authority
            .commit_primed_coordinate_epoch(prepared.correlation, now, merger)?;
        self.validator = prepared.validator;
        self.basis = None;
        self.target_basis = Some(prepared.basis);
        self.practice_mixer_basis = Some(prepared.practice_mixer_basis);
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_audio_presentation_coordinate_epoch_fixtures.rs"]
mod fixtures;
