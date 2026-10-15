//! Pure source/converter playback mapping. These values grant no native admission.
use crate::{
    audio_authority::AudioAuthorityCoordinateBasis,
    gameplay::output::ports::{
        NativeSourcePlaybackQuery, NativeTargetPlaybackQuery, SourcePlaybackFacts,
        SourcePlaybackIdentity,
    },
    native_audio_presentation::NativeAudioPresentation,
};
use beatkernel::{
    audio::{
        ConvertedPhaseSpan, InterpolatedTargetTime, OutputFrameBasis, PracticeRateEvent,
        SourcePlaybackSpan, SourcePlaybackState, TargetFrameBasis, TargetTime,
    },
    time::ClockPoint,
    transport::{OutputPhaseBasis, OutputPhasePoint},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePlaybackPhaseError {
    Identity,
    Descriptor,
    Revision,
    Range,
    Query,
    LogicalPoint,
    Arithmetic,
    Phase,
}
impl std::fmt::Display for NativePlaybackPhaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native playback phase: {self:?}")
    }
}
impl std::error::Error for NativePlaybackPhaseError {}

/// Caller-qualified converter facts, without a native platform type or clock grant.
#[derive(Clone, Copy, Debug)]
pub struct ConvertedPlaybackFacts {
    pub phase: ConvertedPhaseSpan,
    pub segment_end: TargetTime,
    pub generated_target_end: TargetTime,
    pub submitted_target_end: TargetTime,
    pub playback: SourcePlaybackSpan,
    pub pulled_source_end: u64,
    pub revision: u64,
}
#[derive(Clone, Copy, Debug)]
enum Mapping {
    Source {
        basis: OutputFrameBasis,
        facts: SourcePlaybackFacts,
    },
    Converted {
        basis: TargetFrameBasis,
        facts: ConvertedPlaybackFacts,
    },
}

/// Preserves exact generated mapping and creation identity; native evidence stays external.
#[derive(Clone, Copy, Debug)]
pub struct NativePlaybackMapping {
    epoch: u64,
    identity: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    mapping: Mapping,
}
impl NativePlaybackMapping {
    /// Reuses the exact converted mapping already checked by the packet constructor.
    pub fn from_target<E>(packet: &NativeTargetPlaybackQuery<E>) -> Self {
        Self {
            epoch: packet.epoch(),
            identity: packet.identity(),
            query: packet.query(),
            mapping: Mapping::Converted {
                basis: packet.basis(),
                facts: packet.facts().playback,
            },
        }
    }
    pub fn from_source<E>(packet: &NativeSourcePlaybackQuery<E>) -> Self {
        Self {
            epoch: packet.epoch(),
            identity: packet.identity(),
            query: packet.query(),
            mapping: Mapping::Source {
                basis: packet.basis(),
                facts: packet.facts(),
            },
        }
    }
    pub fn converted(
        epoch: u64,
        basis: TargetFrameBasis,
        identity: SourcePlaybackIdentity,
        query: InterpolatedTargetTime,
        facts: ConvertedPlaybackFacts,
    ) -> Result<Self, NativePlaybackPhaseError> {
        use NativePlaybackPhaseError as Error;
        if facts.revision & 1 != 0 {
            return Err(Error::Revision);
        }
        if facts.phase.to_words()[0] != u64::from(identity.hz()) {
            return Err(Error::Identity);
        }
        let compare = |left, right| {
            InterpolatedTargetTime::new(left, 0, 0, 1, 0, 1)
                .map(|exact| exact.compare_target_time(right))
                .map_err(|_| Error::Arithmetic)
        };
        let earlier_segment = compare(facts.segment_end, facts.generated_target_end)?.is_lt();
        if !compare(facts.submitted_target_end, basis.start_time())?.is_gt()
            || compare(facts.submitted_target_end, facts.generated_target_end)?.is_gt()
            || compare(facts.segment_end, facts.generated_target_end)?.is_gt()
            || query.compare_target_time(basis.start_time()).is_lt()
            || query
                .compare_target_time(facts.submitted_target_end)
                .is_gt()
            || query.compare_target_time(facts.segment_end).is_gt()
            || (earlier_segment && query.compare_target_time(facts.segment_end).is_eq())
        {
            return Err(Error::Range);
        }
        let span = facts.playback;
        if span.physical_start() < identity.initial_physical()
            || span.physical_start() > facts.pulled_source_end
            || span
                .physical_end()
                .is_some_and(|end| end > facts.pulled_source_end)
            || span.playback_start() < identity.initial_playback()
            || span.playback_start() - identity.initial_playback()
                > span.physical_start() - identity.initial_physical()
        {
            return Err(Error::Range);
        }
        if facts
            .phase
            .compare_source_frame_at_interpolated_time(
                query,
                facts.segment_end,
                facts.pulled_source_end,
            )
            .map_err(|_| Error::Range)?
            .is_gt()
        {
            return Err(Error::Range);
        }
        let compare_source = |frame| {
            facts
                .phase
                .compare_source_frame_at_interpolated_time(query, facts.segment_end, frame)
                .map_err(|_| Error::Range)
        };
        if compare_source(span.physical_start())?.is_lt()
            || span
                .physical_end()
                .is_some_and(|end| !compare_source(end).is_ok_and(|order| order.is_lt()))
        {
            return Err(Error::Range);
        }
        Ok(Self {
            epoch,
            identity,
            query,
            mapping: Mapping::Converted { basis, facts },
        })
    }
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }
    pub const fn identity(&self) -> SourcePlaybackIdentity {
        self.identity
    }
    pub const fn query(&self) -> InterpolatedTargetTime {
        self.query
    }

    /// Orders the stored exact physical query against a Source cut. This can
    /// restrict progress but does not qualify presentation or an input prefix.
    pub(crate) fn compare_physical_source_frame(
        &self,
        frame: u64,
    ) -> Result<core::cmp::Ordering, NativePlaybackPhaseError> {
        match self.mapping {
            Mapping::Source { .. } => self
                .query
                .compare_source_time_frame(frame, self.identity.hz()),
            Mapping::Converted { facts, .. } => facts
                .phase
                .compare_source_frame_at_interpolated_time(self.query, facts.segment_end, frame),
        }
        .map_err(|_| NativePlaybackPhaseError::Range)
    }

    /// Derives an exact integer Source cut without changing its first crossing
    /// Target sample. The returned coordinates are arithmetic, not admission.
    pub(crate) fn runtime_rate_cut(
        &self,
        presentation: &NativeAudioPresentation,
        expected_identity: SourcePlaybackIdentity,
        attempt: OutputPhaseBasis,
        projected: (&InterpolatedTargetTime, ClockPoint),
        event: PracticeRateEvent,
    ) -> Result<(InterpolatedTargetTime, OutputPhasePoint), NativePlaybackPhaseError> {
        use NativePlaybackPhaseError as Error;
        // Revalidate the projected correspondence and its original logical
        // point before deriving any earlier mathematical coordinate.
        self.phase(
            presentation,
            expected_identity,
            projected.0,
            attempt,
            projected.1,
        )?;
        if event.generation != attempt.generation()
            || event.physical_frame < self.identity.initial_physical()
            || event.playback_frame < self.identity.initial_playback()
            || event.playback_frame < attempt.active_origin_frame()
        {
            return Err(Error::Identity);
        }
        let (query, origin, playback) = match self.mapping {
            Mapping::Source { basis, facts } => {
                if event.physical_frame < basis.start_physical_frame()
                    || event.physical_frame > facts.submitted_source_end
                    || event.physical_frame > facts.pulled_source_end
                {
                    return Err(Error::Range);
                }
                let time = TargetTime::from_frames(event.physical_frame, self.identity.hz())
                    .map_err(|_| Error::Arithmetic)?;
                let query = InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1)
                    .map_err(|_| Error::Arithmetic)?;
                (query, basis.origin(), facts.playback)
            }
            Mapping::Converted { basis, facts } => {
                let [hz, frame, numerator, denominator, seconds, n, d, state] =
                    facts.phase.to_words();
                if state != 0 || event.physical_frame > facts.pulled_source_end {
                    return Err(Error::Range);
                }
                let delta = event.physical_frame.checked_sub(frame).ok_or(Error::Range)?;
                let base = TargetTime::new(seconds, n, d).map_err(|_| Error::Arithmetic)?;
                // base + delta/hz - numerator/(denominator*hz), retaining
                // both fractions without rounding to a target sample or ns.
                // Split a positive delta before the signed fraction so even
                // delta+numerator outside u64 needs no narrowing or extra term.
                let query = if delta == 0 {
                    InterpolatedTargetTime::new(
                        base,
                        0,
                        1,
                        hz,
                        -i128::from(numerator),
                        denominator,
                    )
                } else {
                    InterpolatedTargetTime::new(
                        base,
                        delta - 1,
                        delta,
                        hz,
                        i128::from(denominator - numerator),
                        denominator,
                    )
                }
                .map_err(|_| Error::Arithmetic)?;
                if query.compare_target_time(basis.start_time()).is_lt()
                    || query.compare_target_time(facts.submitted_target_end).is_gt()
                    || query.compare_target_time(facts.generated_target_end).is_gt()
                    || !facts
                        .phase
                        .compare_source_frame_at_interpolated_time(
                            query,
                            facts.segment_end,
                            event.physical_frame,
                        )
                        .map_err(|_| Error::Range)?
                        .is_eq()
                {
                    return Err(Error::Range);
                }
                (query, basis.origin(), facts.playback)
            }
        };
        if playback.state() != SourcePlaybackState::Running
            || event.physical_frame < playback.physical_start()
            || playback
                .physical_end()
                .is_some_and(|end| event.physical_frame >= end)
            || query.compare_interpolated_time(self.query).is_gt()
        {
            return Err(Error::Range);
        }
        let playback_frame = playback
            .playback_start()
            .checked_add(event.physical_frame - playback.physical_start())
            .ok_or(Error::Arithmetic)?;
        if playback_frame != event.playback_frame {
            return Err(Error::Phase);
        }
        let raw = query.point(origin).map_err(|_| Error::Arithmetic)?;
        let logical = presentation
            .authority()
            .epoch()
            .logical_output(raw)
            .map_err(|_| Error::Arithmetic)?;
        let cut_mapping = Self { query, ..*self };
        let point = cut_mapping.phase(presentation, expected_identity, &query, attempt, logical)?;
        let elapsed = i128::from(event.playback_frame - attempt.active_origin_frame())
            * 1_000_000_000
            / i128::from(self.identity.hz());
        if point.elapsed_nanos() != elapsed || elapsed != event.active_elapsed_nanos {
            return Err(Error::Phase);
        }
        Ok((query, point))
    }

    /// Validate original correspondence even when a closure-only frontier
    /// predates the current attempt's active origin. This grants no advance.
    pub(crate) fn validate_correspondence(
        &self,
        presentation: &NativeAudioPresentation,
        expected_identity: SourcePlaybackIdentity,
        expected_query: &InterpolatedTargetTime,
        attempt: OutputPhaseBasis,
        logical: ClockPoint,
    ) -> Result<(), NativePlaybackPhaseError> {
        use NativePlaybackPhaseError as Error;
        if self.epoch != presentation.authority().epoch().id
            || self.identity != expected_identity
            || attempt.source_hz() != self.identity.hz()
        {
            return Err(Error::Identity);
        }
        if !self
            .query
            .compare_interpolated_time(*expected_query)
            .is_eq()
        {
            return Err(Error::Query);
        }
        let descriptor = presentation.authority().coordinate_basis();
        let origin = match self.mapping {
            Mapping::Source { basis, .. } => {
                let matches = match descriptor {
                    Some(AudioAuthorityCoordinateBasis::SourceFrame(actual)) => actual == basis,
                    Some(AudioAuthorityCoordinateBasis::SourceCounter {
                        basis: actual,
                        frequency,
                    }) => actual == basis && frequency != 0,
                    _ => false,
                };
                if !matches
                    || presentation.target_basis().is_some()
                    || presentation.basis().is_some_and(|actual| actual != basis)
                    || basis
                        .point_at_stream_frame(0)
                        .map_err(|_| Error::Arithmetic)?
                        != presentation.authority().epoch().stream_origin
                {
                    return Err(Error::Descriptor);
                }
                basis.origin()
            }
            Mapping::Converted { basis, .. } => {
                if descriptor != Some(AudioAuthorityCoordinateBasis::TargetFrame(basis))
                    || presentation.basis().is_some()
                    || presentation
                        .target_basis()
                        .is_some_and(|actual| actual != basis)
                    || basis
                        .point_at_stream_frame(0)
                        .map_err(|_| Error::Arithmetic)?
                        != presentation.authority().epoch().stream_origin
                {
                    return Err(Error::Descriptor);
                }
                basis.origin()
            }
        };
        let raw = self.query.point(origin).map_err(|_| Error::Arithmetic)?;
        let normalized = presentation
            .authority()
            .epoch()
            .logical_output(raw)
            .map_err(|_| Error::Arithmetic)?;
        if normalized != logical {
            return Err(Error::LogicalPoint);
        }
        if logical.domain != attempt.domain() {
            return Err(Error::Phase);
        }
        Ok(())
    }

    /// Validates original chronology separately from the once-floored active duration.
    /// The supplied attempt is the caller's actual transport binding, not an epoch ID.
    pub fn phase(
        &self,
        presentation: &NativeAudioPresentation,
        expected_identity: SourcePlaybackIdentity,
        expected_query: &InterpolatedTargetTime,
        attempt: OutputPhaseBasis,
        logical: ClockPoint,
    ) -> Result<OutputPhasePoint, NativePlaybackPhaseError> {
        use NativePlaybackPhaseError as Error;
        self.validate_correspondence(
            presentation,
            expected_identity,
            expected_query,
            attempt,
            logical,
        )?;
        let elapsed = match self.mapping {
            Mapping::Source { facts, .. } => facts.playback.elapsed_nanos_at_source_time(
                self.query,
                self.identity.hz(),
                attempt.active_origin_frame(),
            ),
            Mapping::Converted { facts, .. } => {
                facts.phase.playback_elapsed_nanos_at_interpolated_time(
                    self.query,
                    facts.segment_end,
                    facts.playback,
                    attempt.active_origin_frame(),
                )
            }
        }
        .map_err(|_| Error::Arithmetic)?;
        attempt
            .point_from_elapsed_nanos(logical, elapsed)
            .map_err(|_| Error::Phase)
    }
}

#[cfg(test)]
#[path = "native_playback_phase_fixtures.rs"]
mod fixtures;
