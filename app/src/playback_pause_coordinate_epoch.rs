//! Retained pause rebinding with unchanged original Target evidence.
use super::*;
use crate::{
    audio_authority::AudioAuthorityCoordinateBasis,
    gameplay::output::ports::TargetOutputTelemetry,
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::audio::{ConvertedOutputState, InterpolatedTargetTime, TargetFrameBasis};
use beatkernel_platform::audio::{
    presentation::validation::OriginalNativePresentationEvidence, ConvertedNativeOutputState,
};

impl NativePause {
    /// Rebind the complete cold owner without changing the original source grid
    /// or granting scalar control permission to the new native coordinates.
    pub fn rebind_coordinate_target_output_with_basis(
        &mut self,
        epoch: u64,
        state: &ConvertedNativeOutputState,
        basis: TargetFrameBasis,
    ) -> Result<(), PauseError> {
        let binding = self.coordinate_binding.ok_or(PauseError(
            "target replacement lacks coordinate pause binding",
        ))?;
        if binding.epoch != self.epoch
            || binding.basis
                != AudioAuthorityCoordinateBasis::TargetFrame(
                    self.target_basis()
                        .ok_or(PauseError("coordinate replacement lacks target basis"))?,
                )
        {
            return Err(PauseError("coordinate target pause binding differs"));
        }
        let mixer = state.mixer();
        let history = mixer.source_playback_history().ok_or(PauseError(
            "coordinate target replacement lacks original source history",
        ))?;
        if history.source_rate() != binding.source_identity.hz()
            || history.initial_physical_frame() != binding.source_identity.initial_physical()
            || history.initial_playback_frame() != binding.source_identity.initial_playback()
            || history.pulled_end_frame() != mixer.frame_cursor()
            || mixer.frame_cursor() < self.min_physical_frame
            || self
                .coordinate_source_pull_floor
                .is_some_and(|floor| mixer.frame_cursor() < floor)
        {
            return Err(PauseError(
                "coordinate target replacement changed original source history",
            ));
        }
        self.check_coordinate_target_replacement_facts(state.boundaries())?;
        let mut next = self.clone();
        next.rebind_target_output_inner(epoch, state, basis)?;
        next.coordinate_binding = Some(CoordinatePauseBinding {
            epoch,
            basis: AudioAuthorityCoordinateBasis::TargetFrame(basis),
            ..binding
        });
        next.coordinate_source_pull_floor = Some(mixer.frame_cursor());
        *self = next;
        Ok(())
    }

    /// Fresh held generation and actual original Target units must both reach
    /// the cold generation floor. Rounded timestamps cannot establish this.
    /// The outer output transaction owns the coherent publication revision and
    /// native submitted-prefix proof; these arithmetic checks do not grant it.
    pub fn coordinate_target_replacement_observation_ready(
        &self,
        original: CoordinateTargetNativeAudioSnapshot,
        telemetry: TargetOutputTelemetry,
    ) -> Result<bool, PauseError> {
        let target = self
            .target
            .ok_or(PauseError("coordinate replacement lacks target pause"))?;
        let binding = self
            .coordinate_binding
            .ok_or(PauseError("coordinate replacement lacks source binding"))?;
        let snapshot = original.snapshot();
        let NativeOutputCoordinate::StreamFrame { frame } = original.coordinate() else {
            return Err(PauseError(
                "target replacement requires original native frames",
            ));
        };
        let OriginalNativePresentationEvidence::SuppliedPair(pair) = snapshot.evidence else {
            return Err(PauseError(
                "target replacement lacks original native frame evidence",
            ));
        };
        if self.phase != PausePhase::Paused
            || binding.epoch != self.epoch
            || snapshot.epoch != self.epoch
            || snapshot.basis != target.basis
            || binding.basis != AudioAuthorityCoordinateBasis::TargetFrame(target.basis)
            || binding.source_identity.hz() != self.rate
            || telemetry.facts.origin != Some(self.origin)
            || telemetry.facts.source_rate != self.rate
            || target
                .basis
                .point_at_stream_frame(frame)
                .map_err(|_| PauseError("native target coordinate overflow"))?
                != pair.source
        {
            return Err(PauseError(
                "target replacement changed original coordinate identity",
            ));
        }
        self.check_pair(pair)?;
        if let Some(previous) = self.coordinate_replacement_observation {
            let NativeOutputCoordinate::StreamFrame {
                frame: previous_frame,
            } = previous.coordinate()
            else {
                return Err(PauseError(
                    "previous target observation lacks original frame",
                ));
            };
            let OriginalNativePresentationEvidence::SuppliedPair(previous_pair) =
                previous.snapshot().evidence
            else {
                return Err(PauseError(
                    "previous target observation lacks original evidence",
                ));
            };
            if frame < previous_frame
                || (frame > previous_frame
                    && pair.target.timestamp <= previous_pair.target.timestamp)
            {
                return Err(PauseError("original target replacement frames regressed"));
            }
        }
        let Some(report) = telemetry.converted else {
            return Ok(false);
        };
        if report.target_frames == 0 {
            return Ok(false);
        }
        let floor = target.generation_floor.ok_or(PauseError(
            "coordinate replacement lacks exact generation floor",
        ))?;
        let source_floor = self
            .coordinate_source_pull_floor
            .ok_or(PauseError("coordinate replacement lacks source pull floor"))?;
        let frames = u64::try_from(report.target_frames)
            .map_err(|_| PauseError("held target extent overflow"))?;
        let expected_end = report
            .target_start_time
            .checked_add_frames(frames, report.target_rate)
            .map_err(|_| PauseError("held target extent overflow"))?;
        let exact_end = InterpolatedTargetTime::new(expected_end, 0, 0, 1, 0, 1)
            .map_err(|_| PauseError("held target duration overflow"))?;
        let position = report.source_position;
        if report.target_rate != target.basis.sample_rate()
            || report.source_rate != self.rate
            || report.state != ConvertedOutputState::Held
            || report.source.is_some()
            || report.source_start_position != position
            || position.denominator == 0
            || position.numerator >= position.denominator
            || position.frame < binding.source_identity.initial_physical()
            || position.frame > report.pulled_source_frame_cursor
            || (position.frame == report.pulled_source_frame_cursor && position.numerator != 0)
            || report.target_frame_cursor < frames
            || report.pulled_source_frame_cursor < source_floor
            || !exact_end
                .compare_target_time(report.target_end_time)
                .is_eq()
        {
            return Err(PauseError(
                "replacement requires coherent fresh target-held rendering",
            ));
        }
        let pause_frame = self.check_coordinate_target_replacement_facts(telemetry.facts)?;
        if report.pause_source_frame != Some(pause_frame)
            || report.resume_source_frame.is_some()
            || report.startup_source_frame
                != telemetry.facts.startup.map(|startup| startup.source_frame)
            || pause_frame > report.pulled_source_frame_cursor
        {
            return Err(PauseError(
                "replacement changed acknowledged source adoption facts",
            ));
        }
        let Some(source) = telemetry.source.filter(|source| source.frames != 0) else {
            return Ok(false);
        };
        let (physical, playback, _) = self.check_report(source)?;
        if !source.paused || playback != self.frozen || physical > report.pulled_source_frame_cursor
        {
            return Err(PauseError(
                "replacement changed actual frozen source rendering",
            ));
        }
        let generated = InterpolatedTargetTime::new(report.target_start_time, 0, 0, 1, 0, 1)
            .map_err(|_| PauseError("held target generation overflow"))?;
        let native = InterpolatedTargetTime::new(
            target.basis.start_time(),
            frame,
            frame,
            u64::from(target.basis.sample_rate()),
            0,
            1,
        )
        .map_err(|_| PauseError("original native target duration overflow"))?;
        Ok(!generated.compare_target_time(floor).is_lt()
            && !native.compare_target_time(floor).is_lt())
    }

    /// Publish only a fully checked held observation; missing or early native
    /// progress leaves every source, pause and native reference unchanged.
    pub fn observe_coordinate_target_replacement(
        &mut self,
        original: CoordinateTargetNativeAudioSnapshot,
        telemetry: TargetOutputTelemetry,
    ) -> Result<bool, PauseError> {
        if !self.coordinate_target_replacement_observation_ready(original, telemetry)? {
            return Ok(false);
        }
        let OriginalNativePresentationEvidence::SuppliedPair(pair) = original.snapshot().evidence
        else {
            return Err(PauseError(
                "target replacement lacks original native evidence",
            ));
        };
        let mut next = self.clone();
        next.bind(EvidenceKind::Point)?;
        if let Some(source) = telemetry.source.filter(|source| source.frames != 0) {
            next.adopt_report(source)?;
        }
        next.last_pair = Some(pair);
        next.coordinate_replacement_observation = Some(original);
        next.coordinate_source_pull_floor = Some(
            telemetry
                .converted
                .expect("ready held report")
                .pulled_source_frame_cursor,
        );
        *self = next;
        Ok(true)
    }

    /// Compare original source identity and exact pause durations before the
    /// synchronously staged output and authority become the current owners.
    /// The caller must retain the exact accepted packet, report, facts and
    /// revision; coherent but different telemetry cannot replace that proof.
    pub fn validate_coordinate_target_replacement(
        &self,
        candidate: &Self,
        original: CoordinateTargetNativeAudioSnapshot,
        telemetry: TargetOutputTelemetry,
    ) -> Result<(), PauseError> {
        let old = self
            .target
            .ok_or(PauseError("original coordinate pause lacks target basis"))?;
        let new = candidate
            .target
            .ok_or(PauseError("candidate coordinate pause lacks target basis"))?;
        let binding = self.coordinate_binding.ok_or(PauseError(
            "original pause lacks coordinate source identity",
        ))?;
        let next_binding = candidate.coordinate_binding.ok_or(PauseError(
            "candidate pause lacks coordinate source identity",
        ))?;
        let OriginalNativePresentationEvidence::SuppliedPair(pair) = original.snapshot().evidence
        else {
            return Err(PauseError(
                "target publication lacks original native frame evidence",
            ));
        };
        let expected_host_floor = [
            self.min_host,
            self.last_pair.map(|pair| pair.target),
            self.last_now,
        ]
        .into_iter()
        .flatten()
        .max_by_key(|host| host.timestamp);
        if self.phase != PausePhase::Paused
            || candidate.phase != PausePhase::Paused
            || candidate.epoch <= self.epoch
            || binding.epoch != self.epoch
            || binding.basis != AudioAuthorityCoordinateBasis::TargetFrame(old.basis)
            || next_binding.epoch != candidate.epoch
            || next_binding.basis != AudioAuthorityCoordinateBasis::TargetFrame(new.basis)
            || binding.source_identity != next_binding.source_identity
            || self.origin != candidate.origin
            || self.host != candidate.host
            || self.rate != candidate.rate
            || self.frozen != candidate.frozen
            || self.gap != candidate.gap
            || old.paused_at != new.paused_at
            || old.total_gap != new.total_gap
            || old.startup != new.startup
            || self.start_frame != candidate.start_frame
            || self.start_configured != candidate.start_configured
            || self.min_physical_frame != candidate.min_physical_frame
            || candidate.min_host != expected_host_floor
            || self.last_now != candidate.last_now
            || self.playback_end != candidate.playback_end
            || self.end_marker.is_some()
            || candidate.end_marker.is_some()
            || candidate.coordinate_reference.is_some()
            || candidate.reference.is_some()
            || candidate.boundary.is_some()
            || candidate.last_pair != Some(pair)
            || candidate.coordinate_replacement_observation != Some(original)
            || !candidate.coordinate_target_replacement_observation_ready(original, telemetry)?
        {
            return Err(PauseError(
                "coordinate replacement changed acknowledged pause identity",
            ));
        }
        self.check_pair(pair)?;
        if let Some(source) = candidate.last_report {
            self.check_report(source)?;
            if telemetry
                .source
                .filter(|source| source.frames != 0)
                .is_some_and(|actual| actual != source)
            {
                return Err(PauseError(
                    "publication differs from accepted original source report",
                ));
            }
        }
        Ok(())
    }

    fn check_coordinate_target_replacement_facts(
        &self,
        facts: beatkernel_platform::audio::ConvertedBoundaryFacts,
    ) -> Result<u64, PauseError> {
        let target = self.target.ok_or(PauseError(
            "coordinate replacement lacks target pause facts",
        ))?;
        let pause_frame = self
            .frozen
            .checked_add(self.gap)
            .ok_or(PauseError("original source pause boundary overflow"))?;
        if facts.origin != Some(self.origin)
            || facts.source_rate != self.rate
            || facts.pause.is_none_or(|pause| {
                Some(pause.target_time) != target.paused_at || pause.source_frame != pause_frame
            })
            || facts.startup.map(|startup| startup.target_time) != target.startup
            || facts
                .startup
                .is_some_and(|startup| startup.source_frame != self.start_frame)
            || facts.resume.is_some()
            || facts.end.is_some()
        {
            return Err(PauseError(
                "replacement changed acknowledged source adoption facts",
            ));
        }
        Ok(pause_frame)
    }
}

#[cfg(test)]
#[path = "playback_pause_coordinate_epoch_fixtures.rs"]
mod fixtures;
