//! Original-coordinate retained Target specialization of the output transaction.
use super::*;
use crate::{
    audio_authority::AudioAuthorityCoordinateBasis,
    gameplay::output::ports::{
        ConditionalTargetRetirementNativeOutputBackend, NativeTargetPlaybackObservation,
        SourcePlaybackIdentity, TargetOutputTelemetry, TargetRetirementStatus,
    },
    native_audio_presentation::PreparedCoordinateTargetNativeAudioEpoch,
    native_end::NativeEnd,
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::audio::{InterpolatedTargetTime, SourcePlaybackState, TargetFrameBasis};
use beatkernel_platform::audio::{
    ConvertedNativeOutputState, presentation::validation::NativeFrameBasis,
};

pub(super) struct ConditionalTargetWaiting<O, R, Ticket> {
    pub(super) output: O,
    request: R,
    ticket: Ticket,
    status: Option<TargetRetirementStatus>,
    pub(super) hold: PauseHold,
    epoch: AudioAuthorityEpoch,
    next_epoch: u64,
    basis: TargetFrameBasis,
    mixer_basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    wait_ns: u64,
    first_poll: Option<ClockPoint>,
    last_poll: Option<ClockPoint>,
}
impl<O, R, Ticket> ConditionalTargetWaiting<O, R, Ticket> {
    pub(super) fn retirement_authorized(&self) -> bool {
        self.status == Some(TargetRetirementStatus::Authorized)
    }
}

pub(super) struct CoordinateTargetWaitingTiming {
    pause: NativePause,
    basis: TargetFrameBasis,
    next: AudioAuthorityEpoch,
    mixer_basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    validator: NativePresentationValidator,
    snapshots: [Option<CoordinateTargetNativeAudioSnapshot>; 2],
    last_revision: Option<u64>,
    last_phase_revision: Option<u64>,
}

/// Conditional refusal retains the original live owner in the controller.
pub enum CoordinateTargetPoll<O, E> {
    Pending,
    LiveRefused(&'static str),
    Ready(ReadyCoordinateTargetAudioOutput<O, E>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateTargetCancellation {
    None,
    Pending,
    LiveRestored,
    Retired,
}

/// Opaque complete native owner, original compound evidence and exclusive hold.
pub struct ReadyCoordinateTargetAudioOutput<O, E> {
    output: O,
    pause: NativePause,
    basis: TargetFrameBasis,
    prepared: PreparedCoordinateTargetNativeAudioEpoch,
    observation: NativeTargetPlaybackObservation<E>,
    hold: PauseHold,
    identity: SourcePlaybackIdentity,
}
impl<O, E> ReadyCoordinateTargetAudioOutput<O, E> {
    pub fn output(&self) -> &O {
        &self.output
    }
    pub fn pause(&self) -> &NativePause {
        &self.pause
    }
    pub const fn basis(&self) -> TargetFrameBasis {
        self.basis
    }
    pub fn prepared(&self) -> &PreparedCoordinateTargetNativeAudioEpoch {
        &self.prepared
    }
    pub fn snapshots(&self) -> [CoordinateTargetNativeAudioSnapshot; 2] {
        self.prepared.snapshots()
    }
    pub fn practice_mixer_basis(&self) -> OutputFrameBasis {
        self.prepared.practice_mixer_basis()
    }
    pub fn observation(&self) -> &NativeTargetPlaybackObservation<E> {
        &self.observation
    }
    pub const fn source_identity(&self) -> SourcePlaybackIdentity {
        self.identity
    }
    pub fn telemetry(&self) -> TargetOutputTelemetry {
        let facts = self.observation.playback().facts();
        TargetOutputTelemetry {
            source: facts.source,
            converted: facts.converted,
            facts: facts.boundaries,
        }
    }
    /// Explicit cleanup transfers both unique owner and its producer lease.
    pub fn into_output_and_hold(self) -> (O, PauseHold) {
        (self.output, self.hold)
    }
}

fn coordinate_target_identity(
    current: &NativeAudioPresentation,
    pause: &NativePause,
    merger: &InputMerger,
    identity: SourcePlaybackIdentity,
) -> Result<(AudioAuthorityEpoch, TargetFrameBasis, OutputFrameBasis), Box<dyn std::error::Error>> {
    current.validate_coordinate_seed()?;
    let epoch = current.authority().epoch();
    let basis = current
        .target_basis()
        .ok_or("coordinate replacement lacks Target basis")?;
    let pin = current
        .practice_mixer_basis()
        .ok_or("coordinate replacement lacks retained Mixer pin")?;
    if pause.phase() != PausePhase::Paused
        || pause.epoch() != epoch.id
        || pause.target_basis() != Some(basis)
        || pause.host_domain() != epoch.host_domain
        || pause.coordinate_source_identity() != Some(identity)
        || current.authority().coordinate_basis()
            != Some(AudioAuthorityCoordinateBasis::TargetFrame(basis))
        || basis.point_at_stream_frame(0)? != epoch.stream_origin
        || pin.origin() != basis.origin()
        || pin.sample_rate() != identity.hz()
        || pin.start_physical_frame() < identity.initial_physical()
        || merger.domain() != epoch.host_domain
    {
        return Err("coordinate replacement requires acknowledged original retained pause".into());
    }
    Ok((epoch, basis, pin))
}

fn validate_recovered_source(
    owner: &ConvertedNativeOutputState,
    pin: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
) -> Result<(), Box<dyn std::error::Error>> {
    let mixer = owner.mixer();
    let config = mixer.config();
    let history = mixer
        .source_playback_history()
        .ok_or("replacement lost original source history")?;
    if config.domain() != pin.origin().domain
        || config.origin() != pin.origin().timestamp
        || config.format().sample_rate() != pin.sample_rate()
        || pin.sample_rate() != identity.hz()
        || mixer.frame_cursor() < pin.start_physical_frame()
        || history.source_rate() != identity.hz()
        || history.initial_physical_frame() != identity.initial_physical()
        || history.initial_playback_frame() != identity.initial_playback()
        || history.pulled_end_frame() != mixer.frame_cursor()
    {
        return Err("replacement changed retained Mixer source identity or creation pin".into());
    }
    Ok(())
}

impl<B: ConditionalTargetRetirementNativeOutputBackend<ConvertedNativeOutputState>>
    OutputReplacement<B, ConvertedNativeOutputState, TargetFrameBasis, B::Ticket>
{
    /// Ask the sole worker for permission while retaining its live native clock.
    pub fn begin_coordinate_target_audio(
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
            coordinate_target_identity(current, pause, merger, identity)
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
                return self.begin_recovered_coordinate_target(
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
        let ticket = match self
            .backend
            .submit_target_conditional_retirement(&mut output, &request)
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
        self.slot = Slot::Conditional(ConditionalTargetWaiting {
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

    fn begin_recovered_coordinate_target(
        &mut self,
        request: B::Request,
        next_epoch: u64,
        owner: ConvertedNativeOutputState,
        hold: PauseHold,
        wait_ns: u64,
        current: &NativeAudioPresentation,
        pause: &NativePause,
        pin: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        self.begin_recovered_lifecycle_with(
            request,
            next_epoch,
            owner,
            hold,
            wait_ns,
            |backend, request, id, owner| {
                validate_recovered_source(owner, pin, identity)
                    .map_err(ReplacementFailure::timing)?;
                let basis = backend
                    .planned_target_basis(request, owner)
                    .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Open, error))?;
                let staged = (|| -> Result<_, Box<dyn std::error::Error>> {
                    let mut candidate = pause.clone();
                    candidate.rebind_coordinate_target_output_with_basis(id, owner, basis)?;
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
                    Ok(CoordinateTargetWaitingTiming {
                        pause: candidate,
                        basis,
                        next,
                        mixer_basis: pin,
                        identity,
                        validator: NativePresentationValidator::new(
                            id,
                            stream_origin,
                            next.host_domain,
                        ),
                        snapshots: [None; 2],
                        last_revision: None,
                        last_phase_revision: None,
                    })
                })()
                .map_err(ReplacementFailure::timing)?;
                Ok((WaitingTiming::CoordinateTarget(staged), basis))
            },
        )
    }

    fn conditional_failure(
        &mut self,
        waiting: ConditionalTargetWaiting<B::Output, B::Request, B::Ticket>,
        mut failure: ReplacementFailure<B::Error>,
    ) -> ReplacementFailure<B::Error> {
        let (cleanup, recovery) = self.cleanup_output(waiting.output);
        self.retain_retirement_hold(waiting.hold);
        failure.cleanup = cleanup;
        failure.recovery = recovery;
        failure
    }

    fn conditional_target_status(
        &mut self,
        waiting: &mut ConditionalTargetWaiting<B::Output, B::Request, B::Ticket>,
    ) -> Result<TargetRetirementStatus, B::Error> {
        if let Some(status) = waiting.status {
            return Ok(status);
        }
        let status = self
            .backend
            .poll_target_conditional_retirement(&mut waiting.output, &waiting.ticket)?;
        if status != TargetRetirementStatus::Pending {
            // Refused/Cancelled were consumed by this exact native poll. Keep
            // the decision until the original request receives its one outcome.
            waiting.status = Some(status);
        }
        Ok(status)
    }

    /// Discover terminal permission before observing the old native stream.
    /// Re-probe after an observation error to cover authorization during IO.
    pub fn probe_coordinate_target_retirement(
        &mut self,
    ) -> Result<Option<TargetRetirementStatus>, ReplacementFailure<B::Error>> {
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
        match self.conditional_target_status(&mut waiting) {
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

    fn cancel_conditional_target(
        &mut self,
        mut waiting: ConditionalTargetWaiting<B::Output, B::Request, B::Ticket>,
    ) -> Result<CoordinateTargetCancellation, ReplacementFailure<B::Error>> {
        let status = match waiting.status {
            Some(status) => Ok(status),
            None => match self
                .backend
                .cancel_target_conditional_retirement(&mut waiting.output, &waiting.ticket)
            {
                Ok(TargetRetirementStatus::Refused | TargetRetirementStatus::Cancelled) => {
                    // Cancellation leaves its terminal response unconsumed. Clear
                    // only this ticket before returning the live stream to its owner.
                    self.backend
                        .poll_target_conditional_retirement(&mut waiting.output, &waiting.ticket)
                }
                other => other,
            },
        };
        match status {
            Ok(TargetRetirementStatus::Refused | TargetRetirementStatus::Cancelled) => {
                self.slot = Slot::Attached(waiting.output);
                Ok(CoordinateTargetCancellation::LiveRestored)
            }
            Ok(TargetRetirementStatus::Pending) => {
                self.slot = Slot::Conditional(waiting);
                Ok(CoordinateTargetCancellation::Pending)
            }
            Ok(TargetRetirementStatus::Authorized) => {
                self.retire_owned_output_held(waiting.output, waiting.hold)?;
                Ok(CoordinateTargetCancellation::Retired)
            }
            Err(error) => {
                let failure = ReplacementFailure::backend(ReplacementPhase::Retire, error);
                Err(self.conditional_failure(waiting, failure))
            }
        }
    }

    pub fn cancel_coordinate_target_audio(
        &mut self,
    ) -> Result<CoordinateTargetCancellation, ReplacementFailure<B::Error>> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        match slot {
            Slot::Conditional(waiting) => self.cancel_conditional_target(waiting),
            Slot::Waiting(waiting)
                if matches!(&waiting.timing, WaitingTiming::CoordinateTarget(_)) =>
            {
                self.slot = Slot::Waiting(waiting);
                self.cancel().map(|_| CoordinateTargetCancellation::Retired)
            }
            other => {
                self.slot = other;
                Ok(CoordinateTargetCancellation::None)
            }
        }
    }

    /// Permission is followed by the same retire/recover/prepare/open/start tail.
    pub fn poll_coordinate_target_audio(
        &mut self,
        now: ClockPoint,
        current: &NativeAudioPresentation,
        pause: &NativePause,
        merger: &InputMerger,
    ) -> Result<CoordinateTargetPoll<B::Output, B::NativeEvidence>, ReplacementFailure<B::Error>>
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
            let identity = coordinate_target_identity(current, pause, merger, waiting.identity);
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
                return match self.cancel_conditional_target(waiting)? {
                    CoordinateTargetCancellation::LiveRestored => {
                        Ok(CoordinateTargetPoll::LiveRefused(if timed_out {
                            "conditional output replacement timed out"
                        } else {
                            "conditional output replacement identity changed"
                        }))
                    }
                    CoordinateTargetCancellation::Pending => Ok(CoordinateTargetPoll::Pending),
                    _ => Err(ReplacementFailure::policy(
                        "conditional cancellation lost to native retirement authorization",
                    )),
                };
            }
            waiting.last_poll = Some(now);
            let status = self.conditional_target_status(&mut waiting);
            match status {
                Ok(TargetRetirementStatus::Pending) => {
                    self.slot = Slot::Conditional(waiting);
                    return Ok(CoordinateTargetPoll::Pending);
                }
                Ok(
                    status @ (TargetRetirementStatus::Refused | TargetRetirementStatus::Cancelled),
                ) => {
                    let reason = if status == TargetRetirementStatus::Refused {
                        "native output retained an incompatible pending PCM suffix"
                    } else {
                        "native conditional output retirement was cancelled"
                    };
                    self.slot = Slot::Attached(waiting.output);
                    return Ok(CoordinateTargetPoll::LiveRefused(reason));
                }
                Ok(TargetRetirementStatus::Authorized) => {
                    let next = waiting.next_epoch;
                    let (owner, hold) =
                        self.recover_replacement_output(waiting.output, waiting.hold)?;
                    self.begin_recovered_coordinate_target(
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
                    return Ok(CoordinateTargetPoll::Pending);
                }
                Err(error) => {
                    let failure = ReplacementFailure::backend(ReplacementPhase::Retire, error);
                    return Err(self.conditional_failure(waiting, failure));
                }
            }
        }
        self.poll_coordinate_target_candidate(now, current, merger)
    }

    fn poll_coordinate_target_candidate(
        &mut self,
        now: ClockPoint,
        current: &NativeAudioPresentation,
        merger: &InputMerger,
    ) -> Result<CoordinateTargetPoll<B::Output, B::NativeEvidence>, ReplacementFailure<B::Error>>
    {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        let Slot::Waiting(mut waiting) = slot else {
            self.slot = slot;
            return Err(ReplacementFailure::policy(
                "coordinate Target output replacement is not waiting",
            ));
        };
        let WaitingTiming::CoordinateTarget(timing) = &mut waiting.timing else {
            self.slot = Slot::Waiting(waiting);
            return Err(ReplacementFailure::policy(
                "coordinate polling cannot consume other timing modes",
            ));
        };
        let result = (|| -> Result<Option<(PreparedCoordinateTargetNativeAudioEpoch, NativeTargetPlaybackObservation<B::NativeEvidence>)>, ReplacementFailure<B::Error>> {
            if now.domain != timing.next.host_domain
                || waiting.last_poll.is_some_and(|old| now.timestamp < old.timestamp)
            {
                return Err(ReplacementFailure::policy("coordinate replacement poll clock changed or regressed"));
            }
            let first = waiting.first_poll.get_or_insert(now);
            if i128::from(now.timestamp.as_nanos()) - i128::from(first.timestamp.as_nanos()) >= i128::from(waiting.wait_ns) {
                return Err(ReplacementFailure::policy("coordinate replacement observation timed out"));
            }
            waiting.last_poll = Some(now);
            if self.backend.epoch(&waiting.output) != timing.next.id || self.backend.basis(&waiting.output) != timing.basis {
                return Err(ReplacementFailure::policy("coordinate output changed its original creation epoch or basis"));
            }
            let Some(observation) = self.backend.observe_native_target_playback(&mut waiting.output, timing.identity)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))? else {
                return Ok(None);
            };
            let packet = observation.playback();
            let original = observation.coordinate();
            let snapshot = original.snapshot();
            let NativeOutputCoordinate::StreamFrame { frame } = original.coordinate() else {
                return Err(ReplacementFailure::policy("coordinate Target requires original stream frames"));
            };
            let OriginalNativePresentationEvidence::SuppliedPair(pair) = snapshot.evidence else {
                return Err(ReplacementFailure::policy("coordinate Target requires original frame evidence"));
            };
            let facts = packet.facts();
            if self.backend.epoch(&waiting.output) != timing.next.id
                || self.backend.basis(&waiting.output) != timing.basis
                || packet.epoch() != timing.next.id || packet.basis() != timing.basis
                || packet.identity() != timing.identity
                || snapshot.epoch != timing.next.id || snapshot.basis != timing.basis
                || facts.boundaries.origin != Some(timing.mixer_basis.origin())
                || facts.boundaries.source_rate != timing.identity.hz()
                || timing.last_revision.is_some_and(|revision| facts.revision < revision)
                || timing.last_phase_revision.is_some_and(|revision| facts.playback.revision < revision)
            {
                return Err(ReplacementFailure::policy("coordinate packet changed retained source or native publication identity"));
            }
            if pair.target.timestamp > now.timestamp
                || i128::from(now.timestamp.as_nanos()) - i128::from(pair.target.timestamp.as_nanos())
                    > i128::from(current.authority().config().max_observation_age.as_nanos())
            {
                return Err(ReplacementFailure::policy("coordinate replacement native evidence is future or stale"));
            }
            let prepared = timing.validator.prepare_frame_pair(
                timing.next.id, pair, NativeFrameBasis::Target(timing.basis), frame,
            ).map_err(|error| ReplacementFailure::timing(error.into()))?;
            let progressing = prepared.correlation_pair().is_some();
            timing.validator.commit(prepared).map_err(|error| ReplacementFailure::timing(error.into()))?;
            timing.last_revision = Some(facts.revision);
            timing.last_phase_revision = Some(facts.playback.revision);
            let telemetry = TargetOutputTelemetry {
                source: facts.source, converted: facts.converted, facts: facts.boundaries,
            };
            let Some(report) = facts.converted else { return Ok(None); };
            let words = facts.playback.phase.to_words();
            if words[7] != 1
                || facts.playback.playback.state() != SourcePlaybackState::Frozen
                || words[1] != report.source_position.frame
                || words[2] != report.source_position.numerator
                || words[3] != report.source_position.denominator
            {
                return Err(ReplacementFailure::policy("coordinate native packet lacks actual submitted held source phase"));
            }
            if !timing.pause.coordinate_target_replacement_observation_ready(original, telemetry)
                .map_err(|error| ReplacementFailure::timing(error.into()))?
                || !progressing
            {
                return Ok(None);
            }
            let mut candidate_pause = timing.pause.clone();
            candidate_pause.observe_coordinate_target_replacement(original, telemetry)
                .map_err(|error| ReplacementFailure::timing(error.into()))?;
            timing.pause = candidate_pause;
            if timing.snapshots[0].is_none() {
                timing.snapshots[0] = Some(original);
            } else {
                if timing.snapshots[1].is_some() {
                    timing.snapshots[0] = timing.snapshots[1];
                }
                timing.snapshots[1] = Some(original);
            }
            let [Some(first), Some(second)] = timing.snapshots else { return Ok(None); };
            let OriginalNativePresentationEvidence::SuppliedPair(first_pair) = first.snapshot().evidence else {
                unreachable!("retained original frame evidence checked");
            };
            if merger.pending() != 0
                || i128::from(now.timestamp.as_nanos()) - i128::from(first_pair.target.timestamp.as_nanos())
                    > i128::from(current.authority().config().max_observation_age.as_nanos())
                || [current.authority().acquired_prefix(), current.authority().closed_host_prefix(),
                    current.authority().committed_input_host(), current.authority().latest_observation().map(|pair| pair.target)]
                    .into_iter().flatten().any(|host| pair.target.timestamp < host.timestamp)
            {
                return Ok(None);
            }
            let NativeOutputCoordinate::StreamFrame { frame: first_frame } = first.coordinate() else {
                unreachable!("retained stream frame checked");
            };
            let query = InterpolatedTargetTime::new(timing.basis.start_time(), first_frame, first_frame,
                u64::from(timing.basis.sample_rate()), 0, 1)
                .map_err(|error| ReplacementFailure::timing(error.into()))?;
            let logical = timing.next.logical_output(first_pair.source)
                .map_err(|error| ReplacementFailure::timing(error.into()))?;
            if [current.authority().committed_operation(), current.authority().committed_presentation()]
                .into_iter().flatten().any(|old| logical.timestamp < old.timestamp)
                || [current.authority().committed_coordinate_operation(), current.authority().committed_coordinate_presentation()]
                    .into_iter().flatten().any(|old| query.compare_interpolated_time(old).is_lt())
            {
                return Ok(None);
            }
            let token = current.prepare_coordinate_target_output_epoch(timing.next, timing.basis,
                [first, second], timing.mixer_basis, now, merger)
                .map_err(ReplacementFailure::timing)?;
            Ok(Some((token, observation)))
        })();
        match result {
            Ok(None) => {
                self.slot = Slot::Waiting(waiting);
                Ok(CoordinateTargetPoll::Pending)
            }
            Ok(Some((prepared, observation))) => {
                let WaitingTiming::CoordinateTarget(timing) = waiting.timing else {
                    unreachable!("coordinate timing checked");
                };
                Ok(CoordinateTargetPoll::Ready(
                    ReadyCoordinateTargetAudioOutput {
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

pub struct ReadyCoordinateTargetAudioPublicationFailure<O, E> {
    pub error: Box<dyn std::error::Error>,
    pub ready: ReadyCoordinateTargetAudioOutput<O, E>,
}

/// Stage the actual applied capability before publishing the held native owner.
pub fn publish_ready_coordinate_target_audio_output_held<O, E, A>(
    ready: ReadyCoordinateTargetAudioOutput<O, E>,
    output: &mut Option<O>,
    context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
    now: ClockPoint,
    stage: impl FnOnce(&ReadyCoordinateTargetAudioOutput<O, E>) -> Result<A, Box<dyn std::error::Error>>,
) -> Result<(PauseHold, A), ReadyCoordinateTargetAudioPublicationFailure<O, E>> {
    let validated = (|| -> Result<(), Box<dyn std::error::Error>> {
        let old_epoch = context.presentation.authority().epoch();
        let pin = ready.prepared.practice_mixer_basis();
        let packet = ready.observation.playback();
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
            || ready.identity.hz() != context.config.sample_rate
            || pin.sample_rate() != context.config.sample_rate
            || context.presentation.practice_mixer_basis() != Some(pin)
            || pin.origin() != ready.basis.origin()
            || pin.origin().domain != context.config.playback_origin.domain
            || context.config.stream_origin != old_epoch.stream_origin
            || context.config.playback_origin != old_epoch.stream_origin
            || ready.pause.host_domain() != context.config.origin.domain
            || old_epoch.host_domain != context.config.origin.domain
            || ready.basis.point_at_stream_frame(0)?
                != ready.prepared.authority_token().next_epoch().stream_origin
            || !context.config.logical_schedule
        {
            return Err(
                "coordinate replacement changed publication or retained source identity".into(),
            );
        }
        context.merger.peek_ready(now)?;
        context
            .presentation
            .validate_coordinate_target_output_epoch(&ready.prepared, now, context.merger)?;
        context.pause.validate_coordinate_target_replacement(
            &ready.pause,
            original,
            ready.telemetry(),
        )?;
        Ok(())
    })();
    if let Err(error) = validated {
        return Err(ReadyCoordinateTargetAudioPublicationFailure { error, ready });
    }
    let applied = match stage(&ready) {
        Ok(applied) => applied,
        Err(error) => return Err(ReadyCoordinateTargetAudioPublicationFailure { error, ready }),
    };
    if let Err(error) = context
        .presentation
        .commit_coordinate_target_output_epoch_borrowed(&ready.prepared, now, context.merger)
    {
        return Err(ReadyCoordinateTargetAudioPublicationFailure { error, ready });
    }
    let origin = ready.prepared.authority_token().next_epoch().stream_origin;
    let ReadyCoordinateTargetAudioOutput {
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

pub fn publish_ready_coordinate_target_audio_output<O, E, A>(
    ready: ReadyCoordinateTargetAudioOutput<O, E>,
    output: &mut Option<O>,
    context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
    now: ClockPoint,
    stage: impl FnOnce(&ReadyCoordinateTargetAudioOutput<O, E>) -> Result<A, Box<dyn std::error::Error>>,
) -> Result<A, ReadyCoordinateTargetAudioPublicationFailure<O, E>> {
    publish_ready_coordinate_target_audio_output_held(ready, output, context, now, stage).map(
        |(hold, applied)| {
            drop(hold);
            applied
        },
    )
}

#[cfg(test)]
#[path = "coordinate_target_replacement_fixtures.rs"]
pub(crate) mod coordinate_target_replacement_fixtures;
