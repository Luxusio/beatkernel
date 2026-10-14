//! Cold retained Source pause proof and fresh original-coordinate publication.
use super::*;
use crate::{
    audio_authority::AudioAuthorityCoordinateBasis,
    gameplay::output::ports::NativePlaybackObservation,
    native_audio_presentation::PreparedCoordinateSourceOwnerAssociation,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::audio::{OutputFrameBasis, SourcePlaybackState};
use beatkernel_platform::audio::{
    presentation::validation::{
        NativeFrameBasis, NativePresentationValidator, OriginalNativePresentationEvidence,
    },
    NativeOutputState,
};

/// Prepared while the complete recovered owner is still borrowed. The original
/// native association also retains the immutable creation pin and source history.
#[derive(Clone, Debug)]
pub struct PreparedCoordinateSourcePauseOutput {
    prior: NativePause,
    association: PreparedCoordinateSourceOwnerAssociation,
}
impl PreparedCoordinateSourcePauseOutput {
    pub fn owner_association(&self) -> &PreparedCoordinateSourceOwnerAssociation {
        &self.association
    }
    pub fn basis(&self) -> OutputFrameBasis {
        self.association.basis()
    }
}

impl NativePause {
    pub fn prepare_coordinate_source_output(
        &self,
        state: &NativeOutputState,
        association: &PreparedCoordinateSourceOwnerAssociation,
    ) -> Result<PreparedCoordinateSourcePauseOutput, PauseError> {
        let binding = self.coordinate_binding.ok_or(PauseError(
            "Source replacement lacks coordinate pause binding",
        ))?;
        let previous_basis = source_basis(binding.basis)?;
        let mixer = state.mixer();
        let config = mixer.config();
        association
            .validate_recovered_state(state)
            .map_err(|_| PauseError("Source replacement recovered owner differs"))?;
        if self.phase != PausePhase::Paused
            || self.target.is_some()
            || binding.epoch != self.epoch
            || association.previous_epoch() != self.epoch
            || association.previous_coordinate_basis() != binding.basis
            || association.source_identity() != binding.source_identity
            || association.playback_frame() != self.frozen
            || state.admission_is_uncertain()
            || !mixer.is_paused()
            || !mixer.pause_requested()
            || !state.paused_tail_admissible()
            || config.domain() != self.origin.domain
            || config.origin() != self.origin.timestamp
            || config.format().sample_rate() != self.rate
            || config.playback_end_frame() != self.playback_end
            || association.basis().origin() != self.origin
            || association.basis().sample_rate() != self.rate
            || association.basis().start_physical_frame() < previous_basis.start_physical_frame()
            || association.basis().start_physical_frame() > mixer.frame_cursor()
            || mixer.frame_cursor() < self.min_physical_frame
            || self
                .coordinate_source_pull_floor
                .is_some_and(|floor| mixer.frame_cursor() < floor)
            || self.end_marker.is_some()
            || self.playback_end.is_some_and(|end| self.frozen >= end)
        {
            return Err(PauseError("Source replacement changed acknowledged pause"));
        }
        match mixer.start_gate_frame() {
            None if self.start_frame == 0 => {}
            Some(Some(frame))
                if frame == self.start_frame && mixer.applied_start_frame() == Some(frame) => {}
            _ => return Err(PauseError("Source replacement changed adopted startup")),
        }
        let history = mixer.source_playback_history().ok_or(PauseError(
            "Source replacement lacks original playback history",
        ))?;
        let frozen_span = history
            .len()
            .checked_sub(1)
            .and_then(|index| history.span(index))
            .ok_or(PauseError("Source replacement lacks actual paused history"))?;
        if history.source_rate() != binding.source_identity.hz()
            || history.initial_physical_frame() != binding.source_identity.initial_physical()
            || history.initial_playback_frame() != binding.source_identity.initial_playback()
            || history.pulled_end_frame() != mixer.frame_cursor()
            || frozen_span.state() != SourcePlaybackState::Frozen
            || frozen_span.playback_start() != self.frozen
            || self.last_report.is_some_and(|report| {
                report
                    .start_frame
                    .checked_add(report.frames as u64)
                    .is_none_or(|end| mixer.frame_cursor() < end)
            })
        {
            return Err(PauseError(
                "Source replacement changed frozen source history",
            ));
        }
        self.point(mixer.frame_cursor())?;
        Ok(PreparedCoordinateSourcePauseOutput {
            prior: self.clone(),
            association: association.clone(),
        })
    }

    /// Complete the cold proof using the descriptor returned by the actual open.
    /// A Counter frequency belongs to that new native clock, independently of Hz.
    pub fn rebind_coordinate_source_output(
        &mut self,
        prepared: &PreparedCoordinateSourcePauseOutput,
        epoch: u64,
        descriptor: AudioAuthorityCoordinateBasis,
    ) -> Result<(), PauseError> {
        let binding = self.coordinate_binding.ok_or(PauseError(
            "Source replacement lacks original coordinate binding",
        ))?;
        let basis = source_basis(descriptor)?;
        if self != &prepared.prior
            || epoch <= self.epoch
            || basis != prepared.association.basis()
            || !matches!(
                (binding.basis, descriptor),
                (
                    AudioAuthorityCoordinateBasis::SourceFrame(_),
                    AudioAuthorityCoordinateBasis::SourceFrame(_)
                ) | (
                    AudioAuthorityCoordinateBasis::SourceCounter { .. },
                    AudioAuthorityCoordinateBasis::SourceCounter { frequency: 1.., .. }
                )
            )
        {
            return Err(PauseError(
                "Source replacement cold proof or creation descriptor differs",
            ));
        }
        let mut next = self.clone();
        next.epoch = epoch;
        next.coordinate_binding = Some(CoordinatePauseBinding {
            epoch,
            basis: descriptor,
            ..binding
        });
        next.min_physical_frame = prepared.association.pulled_source_end();
        next.coordinate_source_generation_floor = Some(next.min_physical_frame);
        next.coordinate_source_pull_floor = Some(next.min_physical_frame);
        next.rebound_basis = Some(basis);
        next.min_host = [
            self.min_host,
            self.last_pair.map(|pair| pair.target),
            self.last_now,
        ]
        .into_iter()
        .flatten()
        .max_by_key(|host| host.timestamp);
        next.last_pair = None;
        next.reference = None;
        next.coordinate_reference = None;
        next.coordinate_replacement_observation = None;
        next.coordinate_source_replacement_observation = None;
        next.boundary = None;
        next.last_transition_physical = None;
        next.evidence_kind = None;
        next.interval_reference = None;
        next.last_interval = None;
        next.interval_window = None;
        next.setup_locked = true;
        *self = next;
        Ok(())
    }

    /// Old held suffixes can be admitted by the new stream, but publication
    /// requires a later paused render, positive submission and exact native cut.
    pub fn coordinate_source_replacement_observation_ready<E>(
        &self,
        observation: &NativePlaybackObservation<E>,
    ) -> Result<bool, PauseError> {
        let binding = self
            .coordinate_binding
            .ok_or(PauseError("Source replacement lacks coordinate binding"))?;
        let basis = source_basis(binding.basis)?;
        let original = observation.coordinate();
        let packet = observation.source();
        let facts = packet.facts();
        if self.phase != PausePhase::Paused
            || self.target.is_some()
            || binding.epoch != self.epoch
            || original.snapshot().epoch != self.epoch
            || original.snapshot().basis != basis
            || packet.epoch() != self.epoch
            || packet.basis() != basis
            || packet.identity() != binding.source_identity
            || binding.source_identity.hz() != self.rate
            || basis.origin() != self.origin
            || facts.revision == 0
            || facts.revision & 1 != 0
            || facts.submitted_source_end > facts.pulled_source_end
            || facts.playback.state() != SourcePlaybackState::Frozen
            || facts.playback.playback_start() != self.frozen
            || !matches!(
                (binding.basis, original.coordinate()),
                (
                    AudioAuthorityCoordinateBasis::SourceFrame(_),
                    NativeOutputCoordinate::StreamFrame { .. }
                )
            ) && !matches!(
                (binding.basis, original.coordinate()),
                (
                    AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. },
                    NativeOutputCoordinate::Counter { frequency: actual, .. }
                ) if frequency != 0 && frequency == actual
            )
        {
            return Err(PauseError(
                "Source replacement publication identity differs",
            ));
        }
        let query = original
            .source_query()
            .map_err(|_| PauseError("Source replacement native cut overflow"))?;
        if !query.compare_interpolated_time(packet.query()).is_eq() {
            return Err(PauseError(
                "Source replacement packet differs from native cut",
            ));
        }
        if let Some((previous, prior_facts)) = self.coordinate_source_replacement_observation {
            if facts.revision < prior_facts.revision
                || facts.pulled_source_end < prior_facts.pulled_source_end
                || facts.submitted_source_end < prior_facts.submitted_source_end
                || (facts.revision == prior_facts.revision
                    && (facts != prior_facts || original != previous))
            {
                return Err(PauseError(
                    "Source replacement publication regressed or tore",
                ));
            }
        }
        let Some(pair) = self.source_replacement_native_pair(original)? else {
            return Ok(false);
        };
        let floor = self
            .coordinate_source_generation_floor
            .ok_or(PauseError("Source replacement lacks cold pull frontier"))?;
        if facts.pulled_source_end < floor
            || self
                .coordinate_source_pull_floor
                .is_some_and(|previous| facts.pulled_source_end < previous)
        {
            return Err(PauseError(
                "Source replacement source pull frontier regressed",
            ));
        }
        let Some(report) = facts.render.filter(|report| report.frames != 0) else {
            return Ok(false);
        };
        if !report.paused
            || report.playback_frames != 0
            || report.playback_start_frame != self.frozen
            || report.playback_end_physical_frame.is_some()
        {
            return Err(PauseError("Source replacement changed frozen rendering"));
        }
        if report.start_frame < floor
            || facts.submitted_source_end <= floor
            || !query
                .compare_source_time_frame(floor, self.rate)
                .map_err(|_| PauseError("Source replacement exact native cut overflow"))?
                .is_gt()
        {
            return Ok(false);
        }
        self.check_pair(pair)?;
        let (physical, playback, _) = self.check_report(report)?;
        if physical > facts.pulled_source_end || playback != self.frozen {
            return Err(PauseError(
                "Source replacement render exceeds actual source pull",
            ));
        }
        Ok(true)
    }

    pub fn observe_coordinate_source_replacement<E>(
        &mut self,
        observation: &NativePlaybackObservation<E>,
    ) -> Result<bool, PauseError> {
        if !self.coordinate_source_replacement_observation_ready(observation)? {
            return Ok(false);
        }
        let original = observation.coordinate();
        let facts = observation.source().facts();
        let pair = self
            .source_replacement_native_pair(original)?
            .ok_or(PauseError("Source replacement lacks native progress"))?;
        let mut next = self.clone();
        next.bind(EvidenceKind::Point)?;
        next.adopt_report(facts.render.expect("ready actual Source render"))?;
        next.last_pair = Some(pair);
        next.coordinate_source_pull_floor = Some(facts.pulled_source_end);
        next.coordinate_source_replacement_observation = Some((original, facts));
        *self = next;
        Ok(true)
    }

    /// Validate the accepted original publication before a borrowed atomic
    /// authority/native/pause/output commit. A different packet cannot replace it.
    pub fn validate_coordinate_source_replacement<E>(
        &self,
        candidate: &Self,
        observation: &NativePlaybackObservation<E>,
    ) -> Result<(), PauseError> {
        let binding = self
            .coordinate_binding
            .ok_or(PauseError("Original Source pause lacks coordinate binding"))?;
        let next_binding = candidate.coordinate_binding.ok_or(PauseError(
            "Candidate Source pause lacks coordinate binding",
        ))?;
        let expected_floor = [
            self.min_host,
            self.last_pair.map(|pair| pair.target),
            self.last_now,
        ]
        .into_iter()
        .flatten()
        .max_by_key(|host| host.timestamp);
        if self.phase != PausePhase::Paused
            || candidate.phase != PausePhase::Paused
            || self.target.is_some()
            || candidate.target.is_some()
            || candidate.epoch <= self.epoch
            || binding.epoch != self.epoch
            || next_binding.epoch != candidate.epoch
            || binding.source_identity != next_binding.source_identity
            || self.origin != candidate.origin
            || self.host != candidate.host
            || self.rate != candidate.rate
            || self.frozen != candidate.frozen
            || self.gap != candidate.gap
            || self.start_frame != candidate.start_frame
            || self.start_configured != candidate.start_configured
            || self.playback_end != candidate.playback_end
            || candidate.min_host != expected_floor
            || self.last_now != candidate.last_now
            || self.end_marker.is_some()
            || candidate.end_marker.is_some()
            || candidate.reference.is_some()
            || candidate.coordinate_reference.is_some()
            || candidate.boundary.is_some()
            || candidate.coordinate_source_replacement_observation
                != Some((observation.coordinate(), observation.source().facts()))
            || candidate.last_report != observation.source().facts().render
            || !candidate.coordinate_source_replacement_observation_ready(observation)?
        {
            return Err(PauseError(
                "Source replacement changed acknowledged pause identity",
            ));
        }
        let pair = candidate.last_pair.ok_or(PauseError(
            "Source replacement lacks accepted original native pair",
        ))?;
        self.check_pair(pair)?;
        Ok(())
    }

    fn source_replacement_native_pair(
        &self,
        original: CoordinateNativeAudioSnapshot,
    ) -> Result<Option<ClockPair>, PauseError> {
        let snapshot = original.snapshot();
        let mut validator = NativePresentationValidator::new(
            self.epoch,
            snapshot
                .basis
                .point_at_stream_frame(0)
                .map_err(|_| PauseError("Source replacement creation point overflow"))?,
            self.host,
        );
        if let Some((previous, _)) = self.coordinate_source_replacement_observation {
            let prepared = prepare_source_native(&validator, previous)?;
            validator
                .commit(prepared)
                .map_err(|_| PauseError("Source replacement prior native proof changed"))?;
            if previous == original {
                return Ok(validator.latest_record().map(|record| record.pair()));
            }
        }
        let prepared = prepare_source_native(&validator, original)?;
        Ok(prepared.correlation_pair())
    }
}

fn source_basis(descriptor: AudioAuthorityCoordinateBasis) -> Result<OutputFrameBasis, PauseError> {
    match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(basis)
        | AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 1..,
        } => Ok(basis),
        _ => Err(PauseError(
            "Source replacement requires actual Source native coordinates",
        )),
    }
}

fn prepare_source_native(
    validator: &NativePresentationValidator,
    original: CoordinateNativeAudioSnapshot,
) -> Result<
    beatkernel_platform::audio::presentation::validation::PreparedNativePresentation,
    PauseError,
> {
    let snapshot = original.snapshot();
    match (snapshot.evidence, original.coordinate()) {
        (
            OriginalNativePresentationEvidence::SuppliedPair(pair),
            NativeOutputCoordinate::StreamFrame { frame },
        ) => validator
            .prepare_frame_pair(
                snapshot.epoch,
                pair,
                NativeFrameBasis::Source(snapshot.basis),
                frame,
            )
            .map_err(|_| PauseError("Source replacement native frame evidence is invalid")),
        _ => crate::native_audio_presentation::prepare_native_snapshot(validator, snapshot)
            .map_err(|_| PauseError("Source replacement original native evidence is invalid")),
    }
}

#[cfg(test)]
#[path = "playback_pause_source_coordinate_epoch_fixtures.rs"]
mod fixtures;
