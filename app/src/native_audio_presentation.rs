//! Original native evidence admitted into one audio-authoritative gameplay owner.
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityEpoch, PreparedPrimedCorrelation},
    local_input::InputMerger,
    native_gameplay::NativeGameplayResult,
};
use beatkernel::audio::{
    OutputFrameBasis, PracticeReceipt, ProjectedPracticeReceipt, TargetFrameBasis,
};
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

/// Original native association on an exact target-duration creation basis.
/// SuppliedPair adapters must retain and validate their original native metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetNativeAudioSnapshot {
    pub epoch: u64,
    pub basis: TargetFrameBasis,
    pub evidence: OriginalNativePresentationEvidence,
}
/// Cold candidate for a target-rate output epoch, using the same audio authority.
#[derive(Clone, Debug)]
pub struct PreparedTargetNativeAudioEpoch {
    previous_epoch: AudioAuthorityEpoch,
    previous_record: Option<NativePresentationRecord>,
    previous_basis: Option<OutputFrameBasis>,
    previous_target_basis: Option<TargetFrameBasis>,
    validator: NativePresentationValidator,
    basis: TargetFrameBasis,
    snapshots: [TargetNativeAudioSnapshot; 2],
    correlation: PreparedPrimedCorrelation,
}
impl PreparedTargetNativeAudioEpoch {
    pub fn basis(&self) -> TargetFrameBasis {
        self.basis
    }
    pub fn snapshots(&self) -> [TargetNativeAudioSnapshot; 2] {
        self.snapshots
    }
    pub fn epoch(&self) -> u64 {
        self.validator.epoch()
    }
    pub fn latest_record(&self) -> NativePresentationRecord {
        self.validator
            .latest_record()
            .expect("two actual observations staged")
    }
}

/// Opaque original-evidence candidate bound to the current live owners.
#[derive(Clone, Debug)]
pub struct PreparedNativeAudioEpoch {
    previous_epoch: AudioAuthorityEpoch,
    previous_record: Option<NativePresentationRecord>,
    previous_basis: Option<OutputFrameBasis>,
    previous_target_basis: Option<TargetFrameBasis>,
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
    target_basis: Option<TargetFrameBasis>,
    practice_mixer_basis: Option<OutputFrameBasis>,
}

/// Opaque qualification of a rendered receipt by retained native presentation.
/// This grants no acquisition-prefix or attempt-commit authority by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedPracticeBoundary {
    epoch: AudioAuthorityEpoch,
    mixer_basis: OutputFrameBasis,
    receipt: PracticeReceipt,
    projected: Option<ProjectedPracticeReceipt>,
    raw: ClockPoint,
    logical: ClockPoint,
    host: ClockPoint,
}
impl PreparedPracticeBoundary {
    pub const fn epoch(&self) -> u64 {
        self.epoch.id
    }
    pub const fn receipt(&self) -> PracticeReceipt {
        self.receipt
    }
    pub const fn projected_receipt(&self) -> Option<ProjectedPracticeReceipt> {
        self.projected
    }
    pub const fn raw_output(&self) -> ClockPoint {
        self.raw
    }
    pub const fn output(&self) -> ClockPoint {
        self.logical
    }
    pub const fn host(&self) -> ClockPoint {
        self.host
    }
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
            target_basis: None,
            practice_mixer_basis: None,
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
        if self.target_basis.is_some() {
            return Err("legacy snapshot cannot replace target creation basis".into());
        }
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
            previous_target_basis: self.target_basis,
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
            || prepared.previous_target_basis != self.target_basis
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
        self.target_basis = None;
        self.practice_mixer_basis = None;
        Ok(())
    }

    /// Admits an original target-grid association without substituting source frames.
    pub fn admit_target(
        &mut self,
        snapshot: TargetNativeAudioSnapshot,
    ) -> NativeGameplayResult<NativeObservationAdmission> {
        self.validate_identity()?;
        if self.basis.is_some()
            || snapshot.epoch != self.authority.epoch().id
            || self
                .target_basis
                .is_some_and(|basis| basis != snapshot.basis)
        {
            return Err("target snapshot differs from immutable creation epoch/basis".into());
        }
        let prepared = prepare_target_native_snapshot(&self.validator, snapshot)?;
        if let Some(pair) = prepared.correlation_pair() {
            self.authority.observe(snapshot.epoch, pair)?;
        }
        let admission = self.validator.commit(prepared)?;
        if admission == NativeObservationAdmission::Progress {
            self.target_basis = Some(snapshot.basis);
        }
        Ok(admission)
    }
    pub const fn target_basis(&self) -> Option<TargetFrameBasis> {
        self.target_basis
    }
    /// Prepares two genuine progressing target observations and the same input authority.
    pub fn prepare_target_output_epoch(
        &self,
        next: AudioAuthorityEpoch,
        basis: TargetFrameBasis,
        snapshots: [TargetNativeAudioSnapshot; 2],
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<PreparedTargetNativeAudioEpoch> {
        self.validate_identity()?;
        if basis.point_at_stream_frame(0)? != next.stream_origin {
            return Err("target epoch origin differs from physical creation basis".into());
        }
        let staged = self.authority.prepare_epoch(next, merger)?;
        let mut validator =
            NativePresentationValidator::new(next.id, next.stream_origin, next.host_domain);
        let mut pairs = [None; 2];
        for (index, snapshot) in snapshots.into_iter().enumerate() {
            if snapshot.epoch != next.id || snapshot.basis != basis {
                return Err("target replacement snapshot creation identity differs".into());
            }
            let prepared = prepare_target_native_snapshot(&validator, snapshot)?;
            pairs[index] = Some(
                prepared
                    .correlation_pair()
                    .ok_or("target replacement requires progressing original observations")?,
            );
            validator.commit(prepared)?;
        }
        let correlation = self.authority.prepare_primed_epoch(
            staged,
            [
                pairs[0].expect("first pair"),
                pairs[1].expect("second pair"),
            ],
            now,
            merger,
        )?;
        Ok(PreparedTargetNativeAudioEpoch {
            previous_epoch: self.authority.epoch(),
            previous_record: self.validator.latest_record(),
            previous_basis: self.basis,
            previous_target_basis: self.target_basis,
            validator,
            basis,
            snapshots,
            correlation,
        })
    }
    pub fn validate_target_output_epoch(
        &self,
        prepared: &PreparedTargetNativeAudioEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        self.validate_identity()?;
        if prepared.previous_epoch != self.authority.epoch()
            || prepared.previous_record != self.validator.latest_record()
            || prepared.previous_basis != self.basis
            || prepared.previous_target_basis != self.target_basis
        {
            return Err("target output changed after cold preparation".into());
        }
        self.authority
            .validate_primed_correlation(&prepared.correlation, now, merger)?;
        Ok(())
    }
    pub fn commit_target_output_epoch(
        &mut self,
        prepared: PreparedTargetNativeAudioEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        self.validate_target_output_epoch(&prepared, now, merger)?;
        self.authority
            .commit_primed_correlation(prepared.correlation, now, merger)?;
        self.validator = prepared.validator;
        self.basis = None;
        self.target_basis = Some(prepared.basis);
        self.practice_mixer_basis = None;
        Ok(())
    }

    /// Whether this output authority has a pinned retained-practice owner.
    pub const fn has_retained_practice(&self) -> bool {
        self.practice_mixer_basis.is_some()
    }

    /// Pin the retained Mixer's actual creation grid before practice admission.
    /// A target-rate converter does not change this original source-frame grid.
    /// Epoch replacement revokes this binding and requires explicit rebinding.
    pub fn pin_practice_mixer_basis(
        &mut self,
        basis: OutputFrameBasis,
    ) -> NativeGameplayResult<()> {
        self.validate_identity()?;
        if self.practice_mixer_basis.is_some_and(|old| old != basis) {
            return Err("practice mixer creation basis is already pinned".into());
        }
        if let Some(native) = self.basis {
            if native != basis {
                return Err("practice mixer differs from native mixer creation basis".into());
            }
        } else if let Some(native) = self.target_basis {
            if native.origin() != basis.origin()
                || basis.point_at_stream_frame(0)? != native.point_at_stream_frame(0)?
            {
                return Err("practice mixer and converter creation origins differ".into());
            }
        } else {
            return Err("practice requires an admitted native creation basis".into());
        }
        self.practice_mixer_basis = Some(basis);
        Ok(())
    }

    /// Qualify native presentation before draining earlier acquired inputs.
    /// None is an explicit wait for real presentation evidence, never permission
    /// to use render progress or the control's receive HOST timestamp instead.
    pub fn prepare_practice_boundary(
        &self,
        receipt: PracticeReceipt,
        now: ClockPoint,
    ) -> NativeGameplayResult<Option<PreparedPracticeBoundary>> {
        if self.target_basis.is_some() {
            return Err("converted practice requires an actual projected target boundary".into());
        }
        let basis = self.validate_practice_receipt(receipt)?;
        let frame = receipt.physical_frame - basis.start_physical_frame();
        let raw = basis.point_at_stream_frame(frame)?;
        self.qualify_practice_boundary(receipt, None, basis, raw, now)
    }

    /// Qualify the exact target sample crossed by retained source consumption.
    /// The copied projection must originate in the actual conversion/direct
    /// output owner; it supplies no native presentation acknowledgment itself.
    pub fn prepare_projected_practice_boundary(
        &self,
        projected: ProjectedPracticeReceipt,
        now: ClockPoint,
    ) -> NativeGameplayResult<Option<PreparedPracticeBoundary>> {
        let receipt = projected.receipt;
        let basis = self.validate_practice_receipt(receipt)?;
        if projected.boundary.source_frame != receipt.physical_frame
            || projected.origin != basis.origin()
            || projected.target_rate == 0
            || projected.boundary.target_frame_offset as u128 > u128::from(projected.target_frame)
        {
            return Err("practice projection differs from original receipt/source identity".into());
        }
        let raw = projected.boundary.target_time.point(projected.origin)?;
        if let Some(native) = self.target_basis {
            if projected.origin != native.origin()
                || projected.target_rate != native.sample_rate()
                || raw.timestamp < native.point_at_stream_frame(0)?.timestamp
            {
                return Err("practice projection differs from native target creation grid".into());
            }
        } else if let Some(native) = self.basis {
            let relative = receipt.physical_frame - native.start_physical_frame();
            if projected.target_rate != native.sample_rate()
                || raw != native.point_at_stream_frame(relative)?
                || projected.target_frame != receipt.physical_frame
            {
                return Err("direct practice projection differs from original native grid".into());
            }
        } else {
            return Err("practice projection lacks native creation identity".into());
        }
        self.qualify_practice_boundary(receipt, Some(projected), basis, raw, now)
    }

    fn validate_practice_receipt(
        &self,
        receipt: PracticeReceipt,
    ) -> NativeGameplayResult<OutputFrameBasis> {
        self.validate_identity()?;
        if self.validator.latest_record().map(|record| record.pair())
            != self.authority.latest_observation()
        {
            return Err("practice authority lacks matching original native observation".into());
        }
        let basis = self
            .practice_mixer_basis
            .ok_or("practice mixer basis is not pinned")?;
        if receipt.generation == 0
            || receipt.playback_frame > receipt.physical_frame
            || receipt.physical_frame < basis.start_physical_frame()
            || i128::from(receipt.applied_song_time.as_nanos())
                - i128::from(receipt.requested_song_time.as_nanos())
                != i128::from(receipt.correction_nanos)
        {
            return Err("malformed practice boundary receipt".into());
        }
        Ok(basis)
    }

    fn qualify_practice_boundary(
        &self,
        receipt: PracticeReceipt,
        projected: Option<ProjectedPracticeReceipt>,
        basis: OutputFrameBasis,
        raw: ClockPoint,
        now: ClockPoint,
    ) -> NativeGameplayResult<Option<PreparedPracticeBoundary>> {
        let Some(host) = self.authority.presented_boundary_host(raw, now)? else {
            return Ok(None);
        };
        Ok(Some(PreparedPracticeBoundary {
            epoch: self.authority.epoch(),
            mixer_basis: basis,
            receipt,
            projected,
            raw,
            logical: self.authority.checked_logical_output(raw)?,
            host,
        }))
    }

    /// Check the exact original input cut after all strictly earlier inputs drain.
    /// An input equal to the cut is deliberately retained for the new attempt.
    pub fn practice_boundary_ready(
        &self,
        prepared: &PreparedPracticeBoundary,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<bool> {
        self.validate_practice_boundary(prepared, now)?;
        Ok(self
            .authority
            .prepare_resume_control_cutoff(
                prepared.epoch.id,
                prepared.raw,
                prepared.host,
                now,
                merger,
            )?
            .is_some())
    }

    /// Commit only after the caller has accepted the corresponding attempt state.
    /// Native history, acquired chronology and the output epoch remain intact.
    pub fn commit_practice_boundary(
        &mut self,
        prepared: PreparedPracticeBoundary,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        self.validate_practice_boundary(&prepared, now)?;
        let cutoff = self
            .authority
            .prepare_resume_control_cutoff(
                prepared.epoch.id,
                prepared.raw,
                prepared.host,
                now,
                merger,
            )?
            .ok_or("practice boundary awaits acquired prefix or pre-cut input drain")?;
        self.authority.commit_control_cutoff(cutoff, merger)?;
        Ok(())
    }

    fn validate_practice_boundary(
        &self,
        prepared: &PreparedPracticeBoundary,
        now: ClockPoint,
    ) -> NativeGameplayResult<()> {
        if self.authority.epoch() != prepared.epoch
            || self.practice_mixer_basis != Some(prepared.mixer_basis)
        {
            return Err("practice boundary belongs to a retired output epoch/basis".into());
        }
        let current = match prepared.projected {
            Some(projected) => self.prepare_projected_practice_boundary(projected, now)?,
            None => self.prepare_practice_boundary(prepared.receipt, now)?,
        };
        if current != Some(*prepared) {
            return Err(
                "practice boundary native qualification changed or is not available".into(),
            );
        }
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

    /// Checked logical coordinate of a point on the current raw output epoch.
    pub fn logical_output(
        &self,
        raw: beatkernel::time::ClockPoint,
    ) -> NativeGameplayResult<beatkernel::time::ClockPoint> {
        Ok(self.authority.checked_logical_output(raw)?)
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

pub(crate) fn prepare_target_native_snapshot(
    validator: &NativePresentationValidator,
    snapshot: TargetNativeAudioSnapshot,
) -> NativeGameplayResult<PreparedNativePresentation> {
    if snapshot.epoch != validator.epoch()
        || snapshot.basis.point_at_stream_frame(0)? != validator.output_origin()
    {
        return Err("target snapshot creation epoch or exact basis differs".into());
    }
    match snapshot.evidence {
        OriginalNativePresentationEvidence::SuppliedPair(pair) => {
            Ok(validator.prepare_pair(snapshot.epoch, pair)?)
        }
        _ => Err(
            "target native backend must supply its validated target-grid original association"
                .into(),
        ),
    }
}

#[cfg(test)]
mod practice_boundary_tests {
    use super::*;
    use crate::audio_authority::{AudioAuthorityConfig, AudioAuthorityError};
    use beatkernel::{
        audio::{PracticeBoundaryKind, TargetTime},
        input::{
            ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent,
        },
        time::{ClockDomainId, ClockPair, Duration, Timestamp},
    };
    use beatkernel_platform::audio::{
        AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
        StreamCounters,
    };
    fn point(domain: u32, nanos: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(nanos),
        }
    }
    fn owner(capacity: usize) -> NativeAudioPresentation {
        let epoch = AudioAuthorityEpoch {
            id: 7,
            stream_origin: point(2, 0),
            logical_origin: point(3, 5_000_000_000),
            host_domain: ClockDomainId(1),
        };
        NativeAudioPresentation::new(
            AudioAuthority::new(
                AudioAuthorityConfig {
                    history_capacity: capacity,
                    max_observation_age: Duration::from_nanos(1_000_000_000),
                    ..AudioAuthorityConfig::default()
                },
                epoch,
            )
            .unwrap(),
            NativePresentationValidator::new(7, epoch.stream_origin, epoch.host_domain),
        )
        .unwrap()
    }
    fn native(owner: &mut NativeAudioPresentation, nanos: i64) {
        let basis = OutputFrameBasis::new(point(2, 0), 1000, 0).unwrap();
        owner
            .admit(NativeAudioSnapshot {
                epoch: 7,
                basis,
                evidence: OriginalNativePresentationEvidence::Wasapi {
                    snapshot: AudioStreamSnapshot {
                        telemetry_available: true,
                        status: AudioStreamStatus::Running,
                        counters: StreamCounters::default(),
                        render: None,
                        clock: Some(AudioClockSnapshot {
                            position: nanos as u64,
                            frequency: 1_000_000_000,
                            qpc_100ns: nanos as u64 / 100,
                            reading_quality: AudioClockReadingQuality::Accurate,
                            host_point: Some(point(1, nanos)),
                            mapping_quality: beatkernel::time::ClockMappingQuality::Unknown,
                        }),
                    },
                    basis: Some(basis),
                },
            })
            .unwrap();
    }
    fn receipt(frame: u64) -> PracticeReceipt {
        PracticeReceipt {
            request_id: 12,
            generation: 3,
            iteration: 2,
            physical_frame: frame,
            playback_frame: frame - 10,
            requested_song_time: Timestamp::from_nanos(20_000_000),
            applied_song_time: Timestamp::from_nanos(20_000_000),
            correction_nanos: 0,
            kind: PracticeBoundaryKind::Looped,
        }
    }
    fn merger() -> InputMerger {
        InputMerger::new(ClockDomainId(1), point(1, 0), vec![DeviceId(9)], 8).unwrap()
    }
    fn event(nanos: i64, sequence: u64) -> PhysicalInputEvent {
        PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(9), point(1, nanos), sequence),
            control: PhysicalControlId::keyboard(7u16),
            state: ButtonState::Down,
        })
    }
    #[test]
    fn native_qualification_exposes_cut_before_drain_and_retains_equal_input() {
        let mut owner = owner(4);
        native(&mut owner, 100_000_000);
        native(&mut owner, 200_000_000);
        owner
            .pin_practice_mixer_basis(OutputFrameBasis::new(point(2, 0), 1000, 0).unwrap())
            .unwrap();
        let now = point(1, 200_000_000);
        let cut = owner
            .prepare_practice_boundary(receipt(150), now)
            .unwrap()
            .unwrap();
        assert_eq!(cut.epoch(), 7);
        assert_eq!(cut.host(), point(1, 150_000_000));
        assert_eq!(cut.raw_output(), point(2, 150_000_000));
        assert_eq!(cut.output(), point(3, 5_150_000_000));
        assert_eq!(cut.receipt().playback_frame, 140);
        let mut merger = merger();
        merger.admit(event(149_999_999, 1), now).unwrap();
        merger.admit(event(150_000_000, 2), now).unwrap();
        assert!(!owner.practice_boundary_ready(&cut, now, &merger).unwrap());
        owner.authority_mut().record_acquired_prefix(now).unwrap();
        assert!(!owner.practice_boundary_ready(&cut, now, &merger).unwrap());
        assert!(owner.commit_practice_boundary(cut, now, &merger).is_err());
        assert_eq!(
            merger.pop_ready(now).unwrap().unwrap().meta().timestamp,
            Timestamp::from_nanos(149_999_999)
        );
        assert!(owner.practice_boundary_ready(&cut, now, &merger).unwrap());
        let before_epoch = owner.authority().epoch();
        let before_record = owner.latest_record();
        owner.commit_practice_boundary(cut, now, &merger).unwrap();
        assert_eq!(owner.authority().epoch(), before_epoch);
        assert_eq!(owner.latest_record(), before_record);
        assert_eq!(owner.authority().history_len(), 2);
        assert_eq!(owner.authority().acquired_prefix(), Some(now));
        assert_eq!(
            owner.authority().committed_operation(),
            Some(point(3, 5_150_000_000))
        );
        assert_eq!(
            merger.peek_ready(now).unwrap().unwrap().meta().timestamp,
            Timestamp::from_nanos(150_000_000)
        );
    }
    fn converted_projection(held: bool) -> (ProjectedPracticeReceipt, OutputFrameBasis) {
        use beatkernel::audio::*;
        let source = AudioFormat::new(1000, 1).unwrap();
        let target = AudioFormat::new(1500, 1).unwrap();
        let bank = SampleBank::new(source, PcmLimits::new(16, 64, 1).unwrap()).unwrap();
        let program = PreparedPracticeProgram::new(
            &bank,
            vec![],
            PracticeLimits::new(1, 0, 8, 4, 8).unwrap(),
        )
        .unwrap();
        let (mut controller, endpoint) = practice_queue(&program).unwrap();
        let (_producer, consumer) = command_queue(8).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                source,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), false)
                    .unwrap(),
            )
            .unwrap();
        controller
            .try_request(PracticeRequest {
                id: 12,
                expected_generation: 1,
                at_playback_frame: 1,
                region: PracticeRegion::new(
                    Timestamp::from_nanos(20_000_000),
                    Timestamp::from_nanos(100_000_000),
                    false,
                )
                .unwrap(),
            })
            .unwrap();
        let basis = mixer.output_frame_basis();
        let mut converted = ConvertedMixer::new(
            mixer,
            target,
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            16,
        )
        .unwrap();
        if held {
            converted.render_held(&mut [0.0; 3]).unwrap();
        }
        converted.render(&mut [0.0; 3]).unwrap();
        let first = controller.try_pop_projected_receipt().unwrap();
        assert_eq!(first.receipt.kind, PracticeBoundaryKind::Started);
        let projected = controller.try_pop_projected_receipt().unwrap();
        assert_eq!(projected.receipt.request_id, 12);
        assert_eq!(projected.receipt.physical_frame, 1);
        assert_eq!(projected.receipt.playback_frame, 1);
        (projected, basis)
    }
    #[test]
    fn actual_converted_nonintegral_and_held_boundaries_require_target_projection() {
        for held in [false, true] {
            let (projected, mixer_basis) = converted_projection(held);
            let expected = if held { 3_333_333 } else { 1_333_333 };
            assert_eq!(
                projected.boundary.target_time.point(point(2, 0)).unwrap(),
                point(2, expected)
            );
            assert_eq!(projected.target_frame, if held { 5 } else { 2 });
            assert_ne!(
                projected.boundary.target_time.point(point(2, 0)).unwrap(),
                mixer_basis.point_at_stream_frame(1).unwrap()
            );
            let mut owner = owner(4);
            let target =
                TargetFrameBasis::new(point(2, 0), TargetTime::from_frames(0, 1500).unwrap(), 1500)
                    .unwrap();
            for frame in [0, 12] {
                let raw = target.point_at_stream_frame(frame).unwrap();
                owner
                    .admit_target(TargetNativeAudioSnapshot {
                        epoch: 7,
                        basis: target,
                        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                            source: raw,
                            target: point(1, raw.timestamp.as_nanos()),
                        }),
                    })
                    .unwrap();
            }
            owner.pin_practice_mixer_basis(mixer_basis).unwrap();
            let now = point(1, 8_000_000);
            assert!(owner
                .prepare_practice_boundary(projected.receipt, now)
                .is_err());
            let cut = owner
                .prepare_projected_practice_boundary(projected, now)
                .unwrap()
                .unwrap();
            assert_eq!(cut.projected_receipt(), Some(projected));
            assert_eq!(cut.raw_output(), point(2, expected));
            assert_eq!(cut.host(), point(1, expected));
            assert_eq!(cut.output(), point(3, 5_000_000_000 + expected));
            let merger = merger();
            owner.authority_mut().record_acquired_prefix(now).unwrap();
            assert!(owner.practice_boundary_ready(&cut, now, &merger).unwrap());
            owner.commit_practice_boundary(cut, now, &merger).unwrap();
        }
    }
    #[test]
    fn projection_refuses_wrong_rate_origin_source_boundary_and_missing_native_progress() {
        let (projected, mixer_basis) = converted_projection(false);
        let mut owner = owner(4);
        let target =
            TargetFrameBasis::new(point(2, 0), TargetTime::from_frames(0, 1500).unwrap(), 1500)
                .unwrap();
        owner
            .admit_target(TargetNativeAudioSnapshot {
                epoch: 7,
                basis: target,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: point(2, 0),
                    target: point(1, 0),
                }),
            })
            .unwrap();
        owner.pin_practice_mixer_basis(mixer_basis).unwrap();
        let now = point(1, 8_000_000);
        assert!(owner
            .prepare_projected_practice_boundary(projected, now)
            .unwrap()
            .is_none());
        for index in 0..3 {
            let mut malformed = projected;
            match index {
                0 => malformed.target_rate = 48000,
                1 => malformed.origin = point(99, 0),
                _ => malformed.boundary.source_frame = 2,
            }
            assert!(owner
                .prepare_projected_practice_boundary(malformed, now)
                .is_err());
        }
    }
    #[test]
    fn no_native_progress_or_future_render_or_malformed_clock_never_authorizes() {
        let mut owner = owner(4);
        assert!(owner
            .prepare_practice_boundary(receipt(150), point(1, 200_000_000))
            .is_err());
        native(&mut owner, 100_000_000);
        owner
            .pin_practice_mixer_basis(OutputFrameBasis::new(point(2, 0), 1000, 0).unwrap())
            .unwrap();
        assert!(owner
            .prepare_practice_boundary(receipt(150), point(1, 200_000_000))
            .unwrap()
            .is_none());
        native(&mut owner, 200_000_000);
        assert!(owner
            .prepare_practice_boundary(receipt(250), point(1, 200_000_000))
            .unwrap()
            .is_none());
        assert!(owner
            .prepare_practice_boundary(receipt(150), point(99, 200_000_000))
            .is_err());
        let mut malformed = receipt(150);
        malformed.playback_frame = 151;
        assert!(owner
            .prepare_practice_boundary(malformed, point(1, 200_000_000))
            .is_err());
        malformed = receipt(150);
        malformed.correction_nanos = 1;
        assert!(owner
            .prepare_practice_boundary(malformed, point(1, 200_000_000))
            .is_err());
    }
    #[test]
    fn retired_native_history_is_explicit_and_prepared_token_cannot_hide_expiry() {
        let mut owner = owner(2);
        native(&mut owner, 100_000_000);
        native(&mut owner, 200_000_000);
        owner
            .pin_practice_mixer_basis(OutputFrameBasis::new(point(2, 0), 1000, 0).unwrap())
            .unwrap();
        let cut = owner
            .prepare_practice_boundary(receipt(150), point(1, 200_000_000))
            .unwrap()
            .unwrap();
        let mut merger = merger();
        owner
            .authority_mut()
            .record_acquired_prefix(point(1, 200_000_000))
            .unwrap();
        let frontier = owner
            .authority()
            .prepare_frontier(point(1, 200_000_000), &merger)
            .unwrap()
            .unwrap();
        owner
            .authority_mut()
            .commit_frontier(frontier, &mut merger)
            .unwrap();
        native(&mut owner, 300_000_000);
        let error = owner
            .prepare_practice_boundary(receipt(150), point(1, 300_000_000))
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<AudioAuthorityError>(),
            Some(&AudioAuthorityError::HistoryExpired)
        );
        assert!(owner
            .practice_boundary_ready(&cut, point(1, 300_000_000), &merger)
            .is_err());
    }
}
