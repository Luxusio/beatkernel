//! Original native evidence admitted into one audio-authoritative gameplay owner.
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityEpoch, PreparedPrimedCorrelation},
    local_input::InputMerger,
    native_gameplay::NativeGameplayResult,
};
use beatkernel::audio::OutputFrameBasis;
use beatkernel::time::ClockPoint;
use beatkernel_platform::audio::presentation::validation::{
    NativeObservationAdmission, NativePresentationRecord, NativePresentationValidator,
    OriginalNativePresentationEvidence, PreparedNativePresentation,
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

/// Opaque original-evidence candidate bound to the current live owners.
#[derive(Clone, Debug)]
pub struct PreparedNativeAudioEpoch {
    previous_epoch: AudioAuthorityEpoch,
    previous_record: Option<NativePresentationRecord>,
    previous_basis: Option<OutputFrameBasis>,
    validator: NativePresentationValidator,
    basis: OutputFrameBasis,
    snapshots: [NativeAudioSnapshot; 2],
    correlation: PreparedPrimedCorrelation,
}
impl PreparedNativeAudioEpoch {
    pub(crate) fn latest_record(&self) -> NativePresentationRecord {
        self.validator
            .latest_record()
            .expect("two original observations were staged")
    }
    pub(crate) fn epoch(&self) -> u64 {
        self.validator.epoch()
    }
    pub(crate) fn basis(&self) -> OutputFrameBasis {
        self.basis
    }
    pub(crate) fn snapshots(&self) -> [NativeAudioSnapshot; 2] {
        self.snapshots
    }
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
        let prepared = prepare_native_snapshot(&self.validator, snapshot)?;
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

    pub fn prepare_output_epoch(
        &self,
        next: AudioAuthorityEpoch,
        basis: OutputFrameBasis,
        snapshots: [NativeAudioSnapshot; 2],
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<PreparedNativeAudioEpoch> {
        self.validate_identity()?;
        if basis.point_at_stream_frame(0)? != next.stream_origin {
            return Err("replacement basis differs from raw epoch origin".into());
        }
        let staged = self.authority.prepare_epoch(next, merger)?;
        let mut validator =
            NativePresentationValidator::new(next.id, next.stream_origin, next.host_domain);
        let mut pairs = [None; 2];
        for (index, snapshot) in snapshots.into_iter().enumerate() {
            if snapshot.epoch != next.id || snapshot.basis != basis {
                return Err("replacement snapshots differ from creation epoch/basis".into());
            }
            let prepared = prepare_native_snapshot(&validator, snapshot)?;
            pairs[index] = Some(
                prepared
                    .correlation_pair()
                    .ok_or("replacement requires two progressing original observations")?,
            );
            validator.commit(prepared)?;
        }
        let correlation = self.authority.prepare_primed_epoch(
            staged,
            [
                pairs[0].expect("first original pair"),
                pairs[1].expect("second original pair"),
            ],
            now,
            merger,
        )?;
        Ok(PreparedNativeAudioEpoch {
            previous_epoch: self.authority.epoch(),
            previous_record: self.validator.latest_record(),
            previous_basis: self.basis,
            validator,
            basis,
            snapshots,
            correlation,
        })
    }

    pub fn validate_output_epoch(
        &self,
        prepared: &PreparedNativeAudioEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        self.validate_identity()?;
        if prepared.previous_epoch != self.authority.epoch()
            || prepared.previous_record != self.validator.latest_record()
            || prepared.previous_basis != self.basis
        {
            return Err("replacement native owner changed after preparation".into());
        }
        self.authority
            .validate_primed_correlation(&prepared.correlation, now, merger)?;
        Ok(())
    }

    pub fn commit_output_epoch(
        &mut self,
        prepared: PreparedNativeAudioEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        self.validate_output_epoch(&prepared, now, merger)?;
        self.authority
            .commit_primed_correlation(prepared.correlation, now, merger)?;
        self.validator = prepared.validator;
        self.basis = Some(prepared.basis);
        Ok(())
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

    /// Exact frame grid pinned by the first successfully admitted native progress.
    pub const fn basis(&self) -> Option<OutputFrameBasis> {
        self.basis
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

pub(crate) fn prepare_native_snapshot(
    validator: &NativePresentationValidator,
    snapshot: NativeAudioSnapshot,
) -> NativeGameplayResult<PreparedNativePresentation> {
    if snapshot.epoch != validator.epoch()
        || snapshot.basis.point_at_stream_frame(0)? != validator.output_origin()
    {
        return Err("native snapshot creation epoch or raw basis differs".into());
    }
    Ok(match snapshot.evidence {
        OriginalNativePresentationEvidence::Wasapi {
            snapshot: native,
            basis,
        } => {
            if basis != Some(snapshot.basis) {
                return Err("native WASAPI evidence requires the exact snapshot basis".into());
            }
            validator.prepare_wasapi(snapshot.epoch, native, basis)?
        }
        OriginalNativePresentationEvidence::SuppliedPair(pair) => {
            validator.prepare_pair(snapshot.epoch, pair)?
        }
        OriginalNativePresentationEvidence::Asio { observation, basis } => {
            if basis != Some(snapshot.basis) {
                return Err("native ASIO evidence requires the exact snapshot basis".into());
            }
            validator.prepare_asio(snapshot.epoch, observation, basis)?
        }
    })
}
