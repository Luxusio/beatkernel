//! Original-coordinate retained Source specialization of the output transaction.
use super::*;
use crate::{
    audio_authority::AudioAuthorityCoordinateBasis,
    gameplay::output::ports::{
        ConditionalSourceRetirementNativeOutputBackend, NativePlaybackObservation,
        SourcePlaybackFacts, SourcePlaybackIdentity, SourceRetirementStatus,
    },
    native_audio_presentation::PreparedCoordinateSourceNativeAudioEpoch,
    native_end::NativeEnd,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
    playback_pause::PreparedCoordinateSourcePauseOutput,
};
use beatkernel_platform::audio::{presentation::validation::NativeFrameBasis, NativeOutputState};

#[cfg(any(target_os = "macos", test))]
#[path = "source_frame_observations.rs"]
pub(crate) mod source_frame_observations;

type ConditionalSourceWaiting<O, R, Ticket> = ConditionalWaiting<O, R, Ticket, OutputFrameBasis>;

pub(super) struct CoordinateSourceWaitingTiming {
    pause: NativePause,
    pub(super) basis: OutputFrameBasis,
    descriptor: AudioAuthorityCoordinateBasis,
    next: AudioAuthorityEpoch,
    cold: PreparedCoordinateSourcePauseOutput,
    identity: SourcePlaybackIdentity,
    validator: NativePresentationValidator,
    snapshots: [Option<CoordinateNativeAudioSnapshot>; 2],
    pairs: [Option<beatkernel::time::ClockPair>; 2],
    last_publication: Option<(CoordinateNativeAudioSnapshot, SourcePlaybackFacts)>,
}

/// Conditional refusal retains the original live owner in the controller.
pub enum CoordinateSourcePoll<O, E> {
    Pending,
    LiveRefused(&'static str),
    Ready(ReadyCoordinateSourceAudioOutput<O, E>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateSourceCancellation {
    None,
    Pending,
    LiveRestored,
    Retired,
}

/// Complete native owner, unmodified Source evidence and its exclusive hold.
pub struct ReadyCoordinateSourceAudioOutput<O, E> {
    output: O,
    pause: NativePause,
    basis: OutputFrameBasis,
    prepared: PreparedCoordinateSourceNativeAudioEpoch,
    observation: NativePlaybackObservation<E>,
    hold: PauseHold,
    identity: SourcePlaybackIdentity,
}
impl<O, E> ReadyCoordinateSourceAudioOutput<O, E> {
    pub fn output(&self) -> &O {
        &self.output
    }
    pub fn pause(&self) -> &NativePause {
        &self.pause
    }
    pub const fn basis(&self) -> OutputFrameBasis {
        self.basis
    }
    pub fn coordinate_basis(&self) -> AudioAuthorityCoordinateBasis {
        self.prepared.coordinate_basis()
    }
    pub fn prepared(&self) -> &PreparedCoordinateSourceNativeAudioEpoch {
        &self.prepared
    }
    pub fn snapshots(&self) -> [CoordinateNativeAudioSnapshot; 2] {
        self.prepared.snapshots()
    }
    pub fn practice_mixer_basis(&self) -> OutputFrameBasis {
        self.prepared.practice_mixer_basis()
    }
    pub fn observation(&self) -> &NativePlaybackObservation<E> {
        &self.observation
    }
    pub const fn source_identity(&self) -> SourcePlaybackIdentity {
        self.identity
    }
    pub fn into_output_and_hold(self) -> (O, PauseHold) {
        (self.output, self.hold)
    }
}

fn source_basis(
    descriptor: AudioAuthorityCoordinateBasis,
) -> Result<OutputFrameBasis, Box<dyn std::error::Error>> {
    match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(basis)
        | AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 1..,
        } => Ok(basis),
        _ => Err("Source replacement requires an actual Source creation descriptor".into()),
    }
}

fn coordinate_source_identity(
    current: &NativeAudioPresentation,
    pause: &NativePause,
    merger: &InputMerger,
    identity: SourcePlaybackIdentity,
) -> Result<(AudioAuthorityEpoch, OutputFrameBasis, OutputFrameBasis), Box<dyn std::error::Error>> {
    current.validate_coordinate_seed()?;
    let epoch = current.authority().epoch();
    let basis = source_basis(
        current
            .authority()
            .coordinate_basis()
            .ok_or("Source replacement lacks original coordinate descriptor")?,
    )?;
    let pin = current
        .practice_mixer_basis()
        .ok_or("Source replacement lacks retained Mixer pin")?;
    if pause.phase() != PausePhase::Paused
        || pause.epoch() != epoch.id
        || pause.target_basis().is_some()
        || pause.host_domain() != epoch.host_domain
        || pause.coordinate_source_identity() != Some(identity)
        || current.basis() != Some(basis)
        || basis.point_at_stream_frame(0)? != epoch.stream_origin
        || pin.origin() != basis.origin()
        || pin.sample_rate() != identity.hz()
        || pin.start_physical_frame() < identity.initial_physical()
        || pin.start_physical_frame() > basis.start_physical_frame()
        || merger.domain() != epoch.host_domain
    {
        return Err("Source replacement requires acknowledged original retained pause".into());
    }
    Ok((epoch, basis, pin))
}

impl<B: ConditionalSourceRetirementNativeOutputBackend<NativeOutputState>>
    OutputReplacement<B, NativeOutputState, OutputFrameBasis, B::Ticket>
{
    /// Ask the sole worker for permission while retaining its live native clock.
    pub fn begin_coordinate_source_audio(
        &mut self,
        request: B::Request,
        current: &NativeAudioPresentation,
        pause: &NativePause,
        end: Option<&NativeEnd>,
        merger: &InputMerger,
        identity: SourcePlaybackIdentity,
        practice_pending: bool,
        wait_ns: u64,
        acquire: impl FnOnce() -> Result<PauseHold, PauseHoldError>,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        if end.is_some() || practice_pending || merger.pending() != 0 || wait_ns == 0 {
            return Err(ReplacementFailure::policy(
                "coordinate replacement requires drained input, no NativeEnd or pending practice and a positive wait",
            ));
        }
        let (epoch, basis, mixer_basis) =
            coordinate_source_identity(current, pause, merger, identity)
                .map_err(ReplacementFailure::timing)?;
        let next_epoch = self
            .last_issued
            .max(epoch.id)
            .checked_add(1)
            .ok_or_else(|| ReplacementFailure::policy("output replacement epoch exhausted"))?;
        match &self.slot {
            Slot::Attached(output)
                if self.backend.epoch(output) == epoch.id
                    && self.backend.basis(output) == basis => {}
            Slot::Recovered(_) => {}
            _ => {
                return Err(ReplacementFailure::policy(
                    "coordinate replacement lacks matching live output ownership",
                ));
            }
        }
        let retained_hold = if matches!(&self.slot, Slot::Recovered(_)) {
            self.retirement_hold.take()
        } else {
            None
        };
        let hold = match retained_hold {
            Some(hold) => hold,
            None => acquire().map_err(|error| ReplacementFailure {
                cause: ReplacementCause::Hold(error),
                cleanup: None,
                recovery: None,
            })?,
        };
        let mut output = match std::mem::replace(&mut self.slot, Slot::Detached) {
            Slot::Attached(output) => output,
            Slot::Recovered(owner) => {
                return self.begin_recovered_coordinate_source(
                    request,
                    next_epoch,
                    owner,
                    hold,
                    wait_ns,
                    current,
                    pause,
                    mixer_basis,
                    identity,
                );
            }
            _ => unreachable!("matching attached or recovered owner checked before acquiring hold"),
        };
        let ticket =
            match self
                .backend
                .submit_source_conditional_retirement(&mut output, &request, None)
            {
                Ok(Some(ticket)) => ticket,
                Ok(None) => {
                    self.slot = Slot::Attached(output);
                    return Err(ReplacementFailure::policy(
                        "native conditional retirement is busy",
                    ));
                }
                Err(error) => {
                    self.slot = Slot::Attached(output);
                    return Err(ReplacementFailure::backend(ReplacementPhase::Retire, error));
                }
            };
        self.slot = Slot::Conditional(ConditionalSourceWaiting {
            output,
            request,
            ticket,
            status: None,
            hold,
            epoch,
            next_epoch,
            basis,
            mixer_basis,
            identity,
            wait_ns,
            first_poll: None,
            last_poll: None,
        });
        Ok(())
    }

    fn begin_recovered_coordinate_source(
        &mut self,
        request: B::Request,
        next_epoch: u64,
        owner: NativeOutputState,
        hold: PauseHold,
        wait_ns: u64,
        current: &NativeAudioPresentation,
        pause: &NativePause,
        pin: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        self.begin_recovered_lifecycle_with_created(
            request,
            next_epoch,
            owner,
            hold,
            wait_ns,
            |_, _, id, owner| {
                if owner.admission_is_uncertain() {
                    return Err(ReplacementFailure::policy(
                        "Source output admission is uncertain and cannot be reopened",
                    ));
                }
                let staged = (|| -> Result<_, Box<dyn std::error::Error>> {
                    let association =
                        current.prepare_coordinate_source_owner_association(owner, identity)?;
                    if association.practice_mixer_basis() != pin {
                        return Err("Source replacement changed original creation pin".into());
                    }
                    let cold = pause.prepare_coordinate_source_output(owner, &association)?;
                    let basis = association.basis();
                    let stream_origin = basis.point_at_stream_frame(0)?;
                    let next = AudioAuthorityEpoch {
                        id,
                        stream_origin,
                        logical_origin: current
                            .authority()
                            .epoch()
                            .logical_output(stream_origin)?,
                        host_domain: current.authority().epoch().host_domain,
                    };
                    Ok(((cold, next), basis))
                })()
                .map_err(ReplacementFailure::timing)?;
                Ok(staged)
            },
            |backend, output, (cold, next)| {
                let descriptor = backend
                    .created_source_coordinate_basis(output)
                    .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Open, error))?;
                let staged = (|| -> Result<_, Box<dyn std::error::Error>> {
                    let basis = source_basis(descriptor)?;
                    let mut candidate = pause.clone();
                    candidate.rebind_coordinate_source_output(&cold, next.id, descriptor)?;
                    Ok(CoordinateSourceWaitingTiming {
                        pause: candidate,
                        basis,
                        descriptor,
                        next,
                        cold,
                        identity,
                        validator: NativePresentationValidator::new(
                            next.id,
                            next.stream_origin,
                            next.host_domain,
                        ),
                        snapshots: [None; 2],
                        pairs: [None; 2],
                        last_publication: None,
                    })
                })()
                .map_err(ReplacementFailure::timing)?;
                Ok(WaitingTiming::CoordinateSource(staged))
            },
        )
    }

    fn conditional_failure(
        &mut self,
        waiting: ConditionalSourceWaiting<B::Output, B::Request, B::Ticket>,
        mut failure: ReplacementFailure<B::Error>,
    ) -> ReplacementFailure<B::Error> {
        let (cleanup, recovery) = self.cleanup_output(waiting.output);
        self.retain_retirement_hold(waiting.hold);
        failure.cleanup = cleanup;
        failure.recovery = recovery;
        failure
    }

    fn conditional_source_status(
        &mut self,
        waiting: &mut ConditionalSourceWaiting<B::Output, B::Request, B::Ticket>,
    ) -> Result<SourceRetirementStatus, B::Error> {
        if let Some(status) = waiting.status {
            return Ok(status);
        }
        let status = self
            .backend
            .poll_source_conditional_retirement(&mut waiting.output, &waiting.ticket)?;
        if status != SourceRetirementStatus::Pending {
            // Refused/Cancelled were consumed by this exact native poll. Keep
            // the decision until the original request receives its one outcome.
            waiting.status = Some(status);
        }
        Ok(status)
    }

    /// Discover terminal permission before observing the old native stream.
    /// Re-probe after an observation error to cover authorization during IO.
    pub fn probe_coordinate_source_retirement(
        &mut self,
    ) -> Result<Option<SourceRetirementStatus>, ReplacementFailure<B::Error>> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        let Slot::Conditional(mut waiting) = slot else {
            self.slot = slot;
            return Ok(None);
        };
        if self.backend.epoch(&waiting.output) != waiting.epoch.id
            || self.backend.basis(&waiting.output) != waiting.basis
        {
            let failure = ReplacementFailure::policy(
                "conditional live output changed its original creation identity",
            );
            return Err(self.conditional_failure(waiting, failure));
        }
        match self.conditional_source_status(&mut waiting) {
            Ok(status) => {
                self.slot = Slot::Conditional(waiting);
                Ok(Some(status))
            }
            Err(error) => {
                let failure = ReplacementFailure::backend(ReplacementPhase::Retire, error);
                Err(self.conditional_failure(waiting, failure))
            }
        }
    }

    fn cancel_conditional_source(
        &mut self,
        mut waiting: ConditionalSourceWaiting<B::Output, B::Request, B::Ticket>,
    ) -> Result<CoordinateSourceCancellation, ReplacementFailure<B::Error>> {
        let status = match waiting.status {
            Some(status) => Ok(status),
            None => match self
                .backend
                .cancel_source_conditional_retirement(&mut waiting.output, &waiting.ticket)
            {
                Ok(SourceRetirementStatus::Refused | SourceRetirementStatus::Cancelled) => {
                    // Cancellation leaves its terminal response unconsumed. Clear
                    // only this ticket before returning the live stream to its owner.
                    self.backend
                        .poll_source_conditional_retirement(&mut waiting.output, &waiting.ticket)
                }
                other => other,
            },
        };
        match status {
            Ok(SourceRetirementStatus::Refused | SourceRetirementStatus::Cancelled) => {
                self.slot = Slot::Attached(waiting.output);
                Ok(CoordinateSourceCancellation::LiveRestored)
            }
            Ok(SourceRetirementStatus::Pending) => {
                self.slot = Slot::Conditional(waiting);
                Ok(CoordinateSourceCancellation::Pending)
            }
            Ok(SourceRetirementStatus::Authorized) => {
                self.retire_owned_output_held(waiting.output, waiting.hold)?;
                Ok(CoordinateSourceCancellation::Retired)
            }
            Err(error) => {
                let failure = ReplacementFailure::backend(ReplacementPhase::Retire, error);
                Err(self.conditional_failure(waiting, failure))
            }
        }
    }

    pub fn cancel_coordinate_source_audio(
        &mut self,
    ) -> Result<CoordinateSourceCancellation, ReplacementFailure<B::Error>> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        match slot {
            Slot::Conditional(waiting) => self.cancel_conditional_source(waiting),
            Slot::Waiting(waiting)
                if matches!(&waiting.timing, WaitingTiming::CoordinateSource(_)) =>
            {
                self.slot = Slot::Waiting(waiting);
                self.cancel().map(|_| CoordinateSourceCancellation::Retired)
            }
            other => {
                self.slot = other;
                Ok(CoordinateSourceCancellation::None)
            }
        }
    }

    /// Permission is followed by the same retire/recover/prepare/open/start tail.
    pub fn poll_coordinate_source_audio(
        &mut self,
        now: ClockPoint,
        current: &NativeAudioPresentation,
        pause: &NativePause,
        merger: &InputMerger,
    ) -> Result<CoordinateSourcePoll<B::Output, B::NativeEvidence>, ReplacementFailure<B::Error>>
    {
        if matches!(&self.slot, Slot::Conditional(_)) {
            let Slot::Conditional(mut waiting) = std::mem::replace(&mut self.slot, Slot::Detached)
            else {
                unreachable!("conditional slot checked");
            };
            if self.backend.epoch(&waiting.output) != waiting.epoch.id
                || self.backend.basis(&waiting.output) != waiting.basis
            {
                let failure = ReplacementFailure::policy(
                    "conditional live output changed its original creation identity",
                );
                return Err(self.conditional_failure(waiting, failure));
            }
            let identity = coordinate_source_identity(current, pause, merger, waiting.identity);
            let invalid = !identity.as_ref().is_ok_and(|(epoch, basis, pin)| {
                *epoch == waiting.epoch && *basis == waiting.basis && *pin == waiting.mixer_basis
            }) || now.domain != waiting.epoch.host_domain
                || waiting
                    .last_poll
                    .is_some_and(|old| now.timestamp < old.timestamp);
            let first = waiting.first_poll.get_or_insert(now);
            let timed_out = i128::from(now.timestamp.as_nanos())
                - i128::from(first.timestamp.as_nanos())
                >= i128::from(waiting.wait_ns);
            if invalid || timed_out {
                return match self.cancel_conditional_source(waiting)? {
                    CoordinateSourceCancellation::LiveRestored => {
                        Ok(CoordinateSourcePoll::LiveRefused(if timed_out {
                            "conditional output replacement timed out"
                        } else {
                            "conditional output replacement identity changed"
                        }))
                    }
                    CoordinateSourceCancellation::Pending => Ok(CoordinateSourcePoll::Pending),
                    _ => Err(ReplacementFailure::policy(
                        "conditional cancellation lost to native retirement authorization",
                    )),
                };
            }
            waiting.last_poll = Some(now);
            let status = self.conditional_source_status(&mut waiting);
            match status {
                Ok(SourceRetirementStatus::Pending) => {
                    self.slot = Slot::Conditional(waiting);
                    return Ok(CoordinateSourcePoll::Pending);
                }
                Ok(
                    status @ (SourceRetirementStatus::Refused | SourceRetirementStatus::Cancelled),
                ) => {
                    let reason = if status == SourceRetirementStatus::Refused {
                        "native output retained an incompatible pending PCM suffix"
                    } else {
                        "native conditional output retirement was cancelled"
                    };
                    self.slot = Slot::Attached(waiting.output);
                    return Ok(CoordinateSourcePoll::LiveRefused(reason));
                }
                Ok(SourceRetirementStatus::Authorized) => {
                    let next = waiting.next_epoch;
                    let (owner, hold) =
                        self.recover_replacement_output(waiting.output, waiting.hold)?;
                    self.begin_recovered_coordinate_source(
                        waiting.request,
                        next,
                        owner,
                        hold,
                        waiting.wait_ns,
                        current,
                        pause,
                        waiting.mixer_basis,
                        waiting.identity,
                    )?;
                    return Ok(CoordinateSourcePoll::Pending);
                }
                Err(error) => {
                    let failure = ReplacementFailure::backend(ReplacementPhase::Retire, error);
                    return Err(self.conditional_failure(waiting, failure));
                }
            }
        }
        self.poll_coordinate_source_candidate(now, current, merger)
    }

    fn poll_coordinate_source_candidate(
        &mut self,
        now: ClockPoint,
        current: &NativeAudioPresentation,
        merger: &InputMerger,
    ) -> Result<CoordinateSourcePoll<B::Output, B::NativeEvidence>, ReplacementFailure<B::Error>>
    {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        let Slot::Waiting(mut waiting) = slot else {
            self.slot = slot;
            return Err(ReplacementFailure::policy(
                "coordinate Source output replacement is not waiting",
            ));
        };
        let WaitingTiming::CoordinateSource(timing) = &mut waiting.timing else {
            self.slot = Slot::Waiting(waiting);
            return Err(ReplacementFailure::policy(
                "Source polling cannot consume other timing modes",
            ));
        };
        let result = (|| -> Result<Option<(PreparedCoordinateSourceNativeAudioEpoch, NativePlaybackObservation<B::NativeEvidence>)>, ReplacementFailure<B::Error>> {
            if now.domain != timing.next.host_domain
                || waiting.last_poll.is_some_and(|old| now.timestamp < old.timestamp)
            {
                return Err(ReplacementFailure::policy("Source replacement poll clock changed or regressed"));
            }
            let first_poll = waiting.first_poll.get_or_insert(now);
            if i128::from(now.timestamp.as_nanos()) - i128::from(first_poll.timestamp.as_nanos()) >= i128::from(waiting.wait_ns) {
                return Err(ReplacementFailure::policy("Source replacement observation timed out"));
            }
            waiting.last_poll = Some(now);
            if self.backend.epoch(&waiting.output) != timing.next.id
                || self.backend.basis(&waiting.output) != timing.basis
            {
                return Err(ReplacementFailure::policy("Source output changed original creation epoch or basis"));
            }
            let descriptor = self.backend.created_source_coordinate_basis(&waiting.output)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))?;
            if descriptor != timing.descriptor {
                return Err(ReplacementFailure::policy("Source output changed actual native creation descriptor"));
            }
            let Some(observation) = self.backend.observe_native_playback_replacement_at(&mut waiting.output, timing.identity, now)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))? else {
                return Ok(None);
            };
            let packet = observation.source();
            let original = observation.coordinate();
            let snapshot = original.snapshot();
            let facts = packet.facts();
            if self.backend.epoch(&waiting.output) != timing.next.id
                || self.backend.basis(&waiting.output) != timing.basis
                || packet.epoch() != timing.next.id
                || packet.basis() != timing.basis
                || packet.identity() != timing.identity
                || snapshot.epoch != timing.next.id || snapshot.basis != timing.basis
                || facts.revision == 0 || facts.revision & 1 != 0
                || timing.last_publication.is_some_and(|(prior, old)|
                    facts.revision < old.revision
                    || facts.pulled_source_end < old.pulled_source_end
                    || facts.submitted_source_end < old.submitted_source_end
                    || (facts.revision == old.revision && (facts != old || original != prior)))
            {
                return Err(ReplacementFailure::policy("Source packet changed native publication or retained source identity"));
            }
            let prepared = match (timing.descriptor, snapshot.evidence, original.coordinate()) {
                (AudioAuthorityCoordinateBasis::SourceFrame(_), OriginalNativePresentationEvidence::SuppliedPair(pair), NativeOutputCoordinate::StreamFrame { frame }) =>
                    timing.validator.prepare_frame_pair(timing.next.id, pair, NativeFrameBasis::Source(timing.basis), frame)
                        .map_err(|error| ReplacementFailure::timing(error.into()))?,
                (AudioAuthorityCoordinateBasis::SourceFrame(_), OriginalNativePresentationEvidence::Asio { .. }, NativeOutputCoordinate::StreamFrame { .. }) =>
                    prepare_native_snapshot(&timing.validator, snapshot).map_err(ReplacementFailure::timing)?,
                (AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. }, OriginalNativePresentationEvidence::Wasapi { .. }, NativeOutputCoordinate::Counter { frequency: actual, .. }) if frequency == actual =>
                    prepare_native_snapshot(&timing.validator, snapshot).map_err(ReplacementFailure::timing)?,
                _ => return Err(ReplacementFailure::policy("Source evidence differs from actual created native descriptor")),
            };
            let progressing = prepared.correlation_pair().is_some();
            timing.validator.commit(prepared).map_err(|error| ReplacementFailure::timing(error.into()))?;
            let Some(pair) = timing.validator.latest_pair() else { return Ok(None); };
            if pair.target.timestamp > now.timestamp
                || i128::from(now.timestamp.as_nanos()) - i128::from(pair.target.timestamp.as_nanos())
                    > i128::from(current.authority().config().max_observation_age.as_nanos())
            {
                return Err(ReplacementFailure::policy("Source replacement native evidence is future or stale"));
            }
            timing.last_publication = Some((original, facts));
            if !timing.pause.coordinate_source_replacement_observation_ready(&observation)
                .map_err(|error| ReplacementFailure::timing(error.into()))? || !progressing
            {
                return Ok(None);
            }
            let mut candidate_pause = timing.pause.clone();
            candidate_pause.observe_coordinate_source_replacement(&observation)
                .map_err(|error| ReplacementFailure::timing(error.into()))?;
            timing.pause = candidate_pause;
            if timing.snapshots[0].is_none() {
                timing.snapshots[0] = Some(original);
                timing.pairs[0] = Some(pair);
            } else {
                if timing.snapshots[1].is_some() {
                    timing.snapshots[0] = timing.snapshots[1];
                    timing.pairs[0] = timing.pairs[1];
                }
                timing.snapshots[1] = Some(original);
                timing.pairs[1] = Some(pair);
            }
            let [Some(first), Some(second)] = timing.snapshots else { return Ok(None); };
            let first_pair = timing.pairs[0].expect("accepted first Source native pair");
            if merger.pending() != 0
                || i128::from(now.timestamp.as_nanos()) - i128::from(first_pair.target.timestamp.as_nanos())
                    > i128::from(current.authority().config().max_observation_age.as_nanos())
                || [current.authority().acquired_prefix(), current.authority().closed_host_prefix(),
                    current.authority().committed_input_host(), current.authority().latest_observation().map(|pair| pair.target)]
                    .into_iter().flatten().any(|host| pair.target.timestamp < host.timestamp)
            {
                return Ok(None);
            }
            let query = first.source_query().map_err(|error| ReplacementFailure::timing(error.into()))?;
            let logical = timing.next.logical_output(first_pair.source).map_err(|error| ReplacementFailure::timing(error.into()))?;
            if [current.authority().committed_operation(), current.authority().committed_presentation()]
                .into_iter().flatten().any(|old| logical.timestamp < old.timestamp)
                || [current.authority().committed_coordinate_operation(), current.authority().committed_coordinate_presentation()]
                    .into_iter().flatten().any(|old| query.compare_interpolated_time(old).is_lt())
            {
                return Ok(None);
            }
            let token = current.prepare_coordinate_source_output_epoch(timing.next, timing.descriptor,
                [first, second], timing.cold.owner_association(), now, merger)
                .map_err(ReplacementFailure::timing)?;
            Ok(Some((token, observation)))
        })();
        match result {
            Ok(None) => {
                self.slot = Slot::Waiting(waiting);
                Ok(CoordinateSourcePoll::Pending)
            }
            Ok(Some((prepared, observation))) => {
                let WaitingTiming::CoordinateSource(timing) = waiting.timing else {
                    unreachable!("Source timing checked");
                };
                Ok(CoordinateSourcePoll::Ready(
                    ReadyCoordinateSourceAudioOutput {
                        output: waiting.output,
                        pause: timing.pause,
                        basis: timing.basis,
                        prepared,
                        observation,
                        hold: waiting.hold,
                        identity: timing.identity,
                    },
                ))
            }
            Err(mut failure) => {
                let (cleanup, recovery) = self.cleanup_output(waiting.output);
                self.retain_retirement_hold(waiting.hold);
                failure.cleanup = cleanup;
                failure.recovery = recovery;
                Err(failure)
            }
        }
    }
}

pub struct ReadyCoordinateSourceAudioPublicationFailure<O, E> {
    pub error: Box<dyn std::error::Error>,
    pub ready: ReadyCoordinateSourceAudioOutput<O, E>,
}

/// Validate the applied capability before the borrowed native/authority commit.
pub fn publish_ready_coordinate_source_audio_output_held<O, E, A>(
    ready: ReadyCoordinateSourceAudioOutput<O, E>,
    output: &mut Option<O>,
    context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
    now: ClockPoint,
    stage: impl FnOnce(&ReadyCoordinateSourceAudioOutput<O, E>) -> Result<A, Box<dyn std::error::Error>>,
) -> Result<(PauseHold, A), ReadyCoordinateSourceAudioPublicationFailure<O, E>> {
    let validated = (|| -> Result<(), Box<dyn std::error::Error>> {
        let old_epoch = context.presentation.authority().epoch();
        let pin = ready.prepared.practice_mixer_basis();
        let packet = ready.observation.source();
        let original = ready.observation.coordinate();
        if output.is_some()
            || context.end.is_some()
            || old_epoch.id != context.pause.epoch()
            || ready.pause.epoch() != ready.prepared.epoch()
            || ready.pause.epoch() <= old_epoch.id
            || ready.prepared.basis() != ready.basis
            || ready.prepared.snapshots()[1] != original
            || packet.epoch() != ready.pause.epoch()
            || packet.basis() != ready.basis
            || packet.identity() != ready.identity
            || ready.prepared.source_identity() != ready.identity
            || ready.identity.hz() != context.config.sample_rate
            || pin.sample_rate() != context.config.sample_rate
            || context.presentation.practice_mixer_basis() != Some(pin)
            || pin.origin() != ready.basis.origin()
            || pin.origin().domain != context.config.playback_origin.domain
            || context.config.stream_origin != old_epoch.stream_origin
            || context.config.playback_origin
                != context.pause.coordinate_source_playback_origin()?
            || ready.pause.host_domain() != context.config.origin.domain
            || old_epoch.host_domain != context.config.origin.domain
            || ready.basis.point_at_stream_frame(0)?
                != ready.prepared.authority_token().next_epoch().stream_origin
            || !context.config.logical_schedule
        {
            return Err(
                "Source replacement changed publication or retained source identity".into(),
            );
        }
        context.merger.peek_ready(now)?;
        context
            .presentation
            .validate_coordinate_source_output_epoch(&ready.prepared, now, context.merger)?;
        context
            .pause
            .validate_coordinate_source_replacement(&ready.pause, &ready.observation)?;
        Ok(())
    })();
    if let Err(error) = validated {
        return Err(ReadyCoordinateSourceAudioPublicationFailure { error, ready });
    }
    let applied = match stage(&ready) {
        Ok(applied) => applied,
        Err(error) => return Err(ReadyCoordinateSourceAudioPublicationFailure { error, ready }),
    };
    if let Err(error) = context
        .presentation
        .commit_coordinate_source_output_epoch_borrowed(&ready.prepared, now, context.merger)
    {
        return Err(ReadyCoordinateSourceAudioPublicationFailure { error, ready });
    }
    let origin = ready.prepared.authority_token().next_epoch().stream_origin;
    let ReadyCoordinateSourceAudioOutput {
        output: candidate,
        pause,
        hold,
        ..
    } = ready;
    *output = Some(candidate);
    *context.pause = pause;
    context.config.stream_origin = origin;
    context.config.playback_origin = origin;
    Ok((hold, applied))
}

pub fn publish_ready_coordinate_source_audio_output<O, E, A>(
    ready: ReadyCoordinateSourceAudioOutput<O, E>,
    output: &mut Option<O>,
    context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
    now: ClockPoint,
    stage: impl FnOnce(&ReadyCoordinateSourceAudioOutput<O, E>) -> Result<A, Box<dyn std::error::Error>>,
) -> Result<A, ReadyCoordinateSourceAudioPublicationFailure<O, E>> {
    publish_ready_coordinate_source_audio_output_held(ready, output, context, now, stage).map(
        |(hold, applied)| {
            drop(hold);
            applied
        },
    )
}

#[cfg(test)]
use crate as runtime;
#[cfg(test)]
#[path = "coordinate_source_future_fixtures.rs"]
mod coordinate_source_future_fixtures;
#[cfg(test)]
#[path = "coordinate_source_replacement_fixtures.rs"]
pub(crate) mod coordinate_source_replacement_fixtures;
#[cfg(test)]
#[path = "coordinate_source_replacement_test_support.rs"]
pub(crate) mod test_support;
