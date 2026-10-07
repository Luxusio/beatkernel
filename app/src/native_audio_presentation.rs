//! Original native evidence admitted into one audio-authoritative gameplay owner.
use crate::{audio_authority::AudioAuthority, native_gameplay::NativeGameplayResult};
use beatkernel::audio::OutputFrameBasis;
use beatkernel_platform::audio::presentation::validation::{
    NativeObservationAdmission, NativePresentationRecord, NativePresentationValidator,
    OriginalNativePresentationEvidence,
};

/// One caller-supplied native observation on its actual creation epoch and frame basis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeAudioSnapshot {
    /// Original stream creation token, rather than a receipt-time replacement token.
    pub epoch: u64,
    /// Captured physical mixer grid; it becomes immutable after first admitted progress.
    pub basis: OutputFrameBasis,
    /// Complete native source metadata, including ASIO's original HOST bracket.
    pub evidence: OriginalNativePresentationEvidence,
}

/// Exclusive native validation and finite gameplay correlation; no estimator or rate loop.
#[derive(Debug)]
pub struct NativeAudioPresentation {
    authority: AudioAuthority,
    validator: NativePresentationValidator,
    basis: Option<OutputFrameBasis>,
}
impl NativeAudioPresentation {
    /// Construct matching cold owners without stitching independently admitted histories.
    pub fn new(
        authority: AudioAuthority,
        validator: NativePresentationValidator,
    ) -> NativeGameplayResult<Self> {
        if validator.latest_record().is_some()
            || authority.history_len() != 0
            || authority.committed_input_host().is_some()
            || authority.committed_operation().is_some()
            || authority.committed_presentation().is_some()
            || authority.closed_host_prefix().is_some()
        {
            return Err("native audio presentation requires cold, uncommitted owners".into());
        }
        let owner = Self {
            authority,
            validator,
            basis: None,
        };
        owner.validate_identity()?;
        Ok(owner)
    }

    /// Validate native metadata, admit its usable pair, then commit native state.
    /// Failed authority admission leaves native evidence and published basis unchanged.
    pub fn admit(
        &mut self,
        snapshot: NativeAudioSnapshot,
    ) -> NativeGameplayResult<NativeObservationAdmission> {
        self.validate_identity()?;
        if snapshot.epoch != self.authority.epoch().id {
            return Err("native audio snapshot belongs to another output epoch".into());
        }
        if snapshot.basis.point_at_stream_frame(0)? != self.validator.output_origin()
            || self.basis.is_some_and(|basis| basis != snapshot.basis)
        {
            return Err("native audio snapshot differs from its original frame basis".into());
        }
        let prepared = match snapshot.evidence {
            OriginalNativePresentationEvidence::Wasapi {
                snapshot: native,
                basis,
            } => {
                if basis != Some(snapshot.basis) {
                    return Err("native WASAPI evidence requires the exact snapshot basis".into());
                }
                self.validator
                    .prepare_wasapi(snapshot.epoch, native, basis)?
            }
            OriginalNativePresentationEvidence::SuppliedPair(pair) => {
                self.validator.prepare_pair(snapshot.epoch, pair)?
            }
            OriginalNativePresentationEvidence::Asio { observation, basis } => {
                if basis != Some(snapshot.basis) {
                    return Err("native ASIO evidence requires the exact snapshot basis".into());
                }
                self.validator
                    .prepare_asio(snapshot.epoch, observation, basis)?
            }
        };
        if let Some(pair) = prepared.correlation_pair() {
            self.authority.observe(snapshot.epoch, pair)?;
        }
        // Both owners are private and exclusively borrowed; authority admission
        // cannot change the validator state bound by this exact native token.
        let admission = self.validator.commit(prepared)?;
        if admission == NativeObservationAdmission::Progress {
            self.basis = Some(snapshot.basis);
        }
        Ok(admission)
    }

    /// Read actual correlation and committed gameplay/presentation watermarks.
    pub fn authority(&self) -> &AudioAuthority {
        &self.authority
    }

    /// Borrow for gameplay prefix, input and frontier operations.
    /// Lifecycle changes must coordinate both owner identities; the next native
    /// admission rechecks epoch/HOST/raw origin rather than silently resynchronizing.
    pub fn authority_mut(&mut self) -> &mut AudioAuthority {
        &mut self.authority
    }

    /// Last progressing original native record, including the full ASIO bracket.
    pub fn latest_record(&self) -> Option<NativePresentationRecord> {
        self.validator.latest_record()
    }

    fn validate_identity(&self) -> NativeGameplayResult<()> {
        let epoch = self.authority.epoch();
        if self.validator.epoch() != epoch.id
            || self.validator.host_domain() != epoch.host_domain
            || self.validator.output_origin() != epoch.stream_origin
        {
            return Err(
                "native presentation owners differ in epoch, HOST domain or raw origin".into(),
            );
        }
        Ok(())
    }
}
