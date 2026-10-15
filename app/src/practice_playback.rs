//! Control-thread ownership of ordered retained-output practice transitions.
//! Render receipts are held one at a time until the actual native input cut and
//! every attempt owner have committed. This module never opens an endpoint.
use crate::{
    live_speed::LiveSpeedSetup,
    local_players::PlayerId,
    native_audio_presentation::{
        NativeAudioPresentation, PreparedPracticeBoundary, PreparedPracticeEventBoundary,
        PreparedPracticeRateCut,
    },
    native_gameplay::NativeGameplayResult,
    native_gameplay_host::NativeGameplayHost,
    play_policy::ResolvedPlayPolicy,
    player::PreparedPracticePresentation,
    practice_control::{
        PracticeAction, PracticeApplied, PracticeCapability, PracticeRateState, PracticeReply,
        PracticeTransactionCapability, PracticeTransactionReply, PracticeTransactionRequest,
    },
    practice_session::{
        PracticeAttemptConfig, PreparedPracticeAttempt, prepare_attempt,
        prepare_attempt_with_runtime_speed, prepare_attempt_with_speed,
    },
    replay_capture::{LiveReplayCapture, PreparedSourceRateChange},
    replay_speed::ReplaySpeed,
    replay_speed_policy::{self, SourceRateCut, SourceRateSchedule},
    session_launch::SessionLaunch,
};
use beatkernel::{
    audio::{
        InterpolatedTargetTime, OutputFrameBasis, PracticeBoundaryKind, PracticeController,
        PracticeError, PracticeEvent, PracticeRateEvent, PracticeRateOutcome, PracticeRateRefusal,
        PracticeReceipt, PracticeRegion, ProjectedPracticeEvent, ProjectedPracticeReceipt,
    },
    replay::{ReplayHeader, codec::ReplayCodecLimits},
    time::{ClockPoint, Duration, Timestamp},
    transport::{OutputPhasePoint, PositiveRatePhase, Transport},
};
use beatkernel_bms::BmsChart;
use std::path::Path;
static NEXT_OWNER_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Pinned preparation inputs in the same order as the actual local cohort.
pub struct PracticeMember {
    pub player: PlayerId,
    pub policy: ResolvedPlayPolicy,
    pub launch: SessionLaunch,
    pub chart_seed: u64,
    pub capture_limits: Option<ReplayCodecLimits>,
}

#[derive(Clone)]
pub struct PracticeRuntimeHeader {
    pub player: PlayerId,
    pub header: ReplayHeader,
    pub limits: ReplayCodecLimits,
}

#[derive(Clone)]
struct RuntimeHeader {
    identity: PracticeRuntimeHeader,
    schedule: SourceRateSchedule,
}
struct RuntimePending {
    request: PracticeTransactionRequest,
    expected: PracticeRateState,
    region: PracticeRegion,
    rate: Option<PreparedRuntimeRateRequest>,
}
pub(crate) struct RuntimePractice {
    rate: PracticeRateState,
    phase: PositiveRatePhase,
    headers: Vec<RuntimeHeader>,
    capture_players: Vec<PlayerId>,
    owner_id: u64,
}

#[derive(Clone)]
pub(crate) struct PreparedRuntimeRateRequest {
    request: PracticeTransactionRequest,
    captures: Vec<Option<PreparedSourceRateChange>>,
    headers: Vec<RuntimeHeader>,
    owner_id: u64,
}

pub(crate) struct PreparedRuntimeRateCommit<'live, 'capture>
where
    'capture: 'live,
{
    transport: &'live mut Transport,
    captures: &'live mut [(PlayerId, &'capture mut Option<LiveReplayCapture>)],
    candidate: Option<Transport>,
    request: PreparedRuntimeRateRequest,
    event: PracticeRateEvent,
    phase: PositiveRatePhase,
    previous: PracticeRateState,
}

/// Exclusive live borrows keep every validated capture and phase unchanged
/// between preparation and the final all-member move.
pub struct PreparedPracticeRateCommit<'live, 'capture>
where
    'capture: 'live,
{
    business: PreparedRuntimeRateCommit<'live, 'capture>,
    event: PracticeRateEvent,
    projected: ProjectedPracticeEvent,
    serial: u64,
    owner_id: u64,
}
pub struct AppliedPracticeRateEvent {
    event: PracticeRateEvent,
    reply: PracticeTransactionReply,
    serial: u64,
    owner_id: u64,
}
impl AppliedPracticeRateEvent {
    pub const fn event(&self) -> PracticeRateEvent {
        self.event
    }
}

impl RuntimePractice {
    /// The adapter first validates each supplied header against its actual
    /// chart/section/policy. This shared owner checks roster and rate custody.
    pub(crate) fn new(
        headers: Vec<PracticeRuntimeHeader>,
        capture_players: &[PlayerId],
        phase: PositiveRatePhase,
    ) -> NativeGameplayResult<Self> {
        if headers.is_empty()
            || headers.len() > crate::local_players::MAX_LOCAL_PLAYERS
            || headers.iter().enumerate().any(|(index, header)| {
                header.player.0 == 0
                    || header.header.normalized_clock != headers[0].header.normalized_clock
                    || headers[..index]
                        .iter()
                        .any(|old| old.player == header.player)
            })
            || capture_players.iter().enumerate().any(|(index, player)| {
                capture_players[..index].contains(player)
                    || !headers.iter().any(|header| header.player == *player)
            })
        {
            return Err("runtime rate business roster differs".into());
        }
        let mut actual = Vec::new();
        actual.try_reserve_exact(headers.len())?;
        for identity in headers {
            let (_, schedule) = replay_speed_policy::split_schedule(&identity.header.options)?;
            if schedule.is_variable() || schedule.current_speed().rate() != phase.rate() {
                return Err("runtime rate business initial history differs".into());
            }
            beatkernel::replay::codec::encode_replay(
                &beatkernel::replay::codec::ReplayFile::new(identity.header.clone(), Vec::new()),
                identity.limits,
            )?;
            actual.push(RuntimeHeader { identity, schedule });
        }
        let mut captured = Vec::new();
        captured.try_reserve_exact(capture_players.len())?;
        captured.extend_from_slice(capture_players);
        let owner_id = NEXT_OWNER_ID
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |next| next.checked_add(1),
            )
            .map_err(|_| "runtime practice identity exhausted")?;
        Ok(Self {
            rate: PracticeRateState {
                revision: 0,
                rate: phase.rate(),
            },
            phase,
            headers: actual,
            capture_players: captured,
            owner_id,
        })
    }
    /// Fresh attempts retain the controller's cumulative revision while their
    /// original-song phase and recording schedule start at the selected rate.
    pub(crate) fn new_at_state(
        headers: Vec<PracticeRuntimeHeader>,
        capture_players: &[PlayerId],
        phase: PositiveRatePhase,
        state: PracticeRateState,
    ) -> NativeGameplayResult<Self> {
        if state.rate != phase.rate() {
            return Err("runtime rate business state differs from its fresh phase".into());
        }
        let mut runtime = Self::new(headers, capture_players, phase)?;
        runtime.rate = state;
        Ok(runtime)
    }

    /// Prepare a fresh attempt while preserving custody of an admitted control
    /// whose actual stale-generation refusal may follow an autonomous boundary.
    pub(crate) fn candidate_for_attempt(
        &self,
        headers: Vec<PracticeRuntimeHeader>,
        capture_players: &[PlayerId],
        phase: PositiveRatePhase,
        state: PracticeRateState,
    ) -> NativeGameplayResult<Self> {
        let mut candidate = Self::new_at_state(headers, capture_players, phase, state)?;
        candidate.owner_id = self.owner_id;
        Ok(candidate)
    }

    /// A qualified nonfresh region control changes neither phase nor identity.
    pub(crate) fn commit_control_revision(
        &mut self,
        expected: PracticeRateState,
        applied_revision: u64,
    ) -> NativeGameplayResult<()> {
        if self.rate != expected || expected.revision.checked_add(1) != Some(applied_revision) {
            return Err("runtime control revision differs from committed state".into());
        }
        self.rate.revision = applied_revision;
        Ok(())
    }

    pub(crate) fn rate_state(&self) -> PracticeRateState {
        self.rate
    }
    pub(crate) fn current_speed(&self) -> ReplaySpeed {
        ReplaySpeed::new(self.rate.rate).expect("committed positive runtime rate")
    }
    pub(crate) fn effective_header(&self, player: PlayerId) -> Option<&ReplayHeader> {
        self.headers
            .iter()
            .find(|header| header.identity.player == player)
            .map(|header| &header.identity.header)
    }
    fn validate_logical_domain(
        &self,
        domain: beatkernel::time::ClockDomainId,
    ) -> NativeGameplayResult<()> {
        if self.headers[0].identity.header.normalized_clock != domain {
            return Err("runtime rate header differs from actual logical output domain".into());
        }
        Ok(())
    }
    fn validate_transport(&self, transport: &Transport) -> NativeGameplayResult<()> {
        let basis = transport
            .output_phase_basis()
            .ok_or("runtime rate requires an output-phase Transport")?;
        self.validate_logical_domain(basis.domain())?;
        if transport.runtime_output_rate() != Some(self.rate.rate) {
            return Err("runtime rate Transport differs from committed state".into());
        }
        Ok(())
    }
    fn validate_captures(
        &self,
        captures: &[(PlayerId, &mut Option<LiveReplayCapture>)],
    ) -> NativeGameplayResult<()> {
        if captures.len() != self.headers.len() {
            return Err("runtime capture roster differs".into());
        }
        for (header, (player, capture)) in self.headers.iter().zip(captures) {
            if header.identity.player != *player
                || capture.is_some() != self.capture_players.contains(player)
                || capture.as_ref().is_some_and(|capture| {
                    capture.header() != &header.identity.header
                        || capture.source_rate_schedule() != &header.schedule
                })
            {
                return Err("runtime capture owner or effective header differs".into());
            }
        }
        Ok(())
    }
    pub(crate) fn reserve_rate(
        &self,
        request: PracticeTransactionRequest,
        transport: &mut Transport,
        captures: &mut [(PlayerId, &mut Option<LiveReplayCapture>)],
    ) -> NativeGameplayResult<PreparedRuntimeRateRequest> {
        self.validate_captures(captures)?;
        self.validate_transport(transport)?;
        let PracticeTransactionRequest::Rate {
            id,
            generation,
            expected,
            requested_rate,
        } = request
        else {
            return Err("runtime rate reservation requires a Rate request".into());
        };
        if id == 0 || generation == 0 || expected != self.rate {
            return Err("runtime rate reservation differs from committed state".into());
        }
        let speed = ReplaySpeed::new(requested_rate)?;
        expected
            .revision
            .checked_add(1)
            .ok_or("practice revision exhausted")?;
        transport.try_reserve_output_phase_controls(1)?;
        let mut prepared = PreparedRuntimeRateRequest {
            request,
            captures: Vec::new(),
            headers: Vec::new(),
            owner_id: self.owner_id,
        };
        prepared.captures.try_reserve_exact(captures.len())?;
        prepared.headers.try_reserve_exact(self.headers.len())?;
        for header in &self.headers {
            let mut schedule = header.schedule.try_copy()?;
            let candidate = if speed == schedule.current_speed() {
                header.identity.header.clone()
            } else {
                let last = schedule.cuts().last().copied();
                schedule.try_push(SourceRateCut::new(
                    last.map_or(0, SourceRateCut::whole),
                    last.map_or(0, SourceRateCut::numerator),
                    last.map_or(1, SourceRateCut::denominator),
                    speed,
                )?)?;
                let mut base = header.identity.header.clone();
                base.options = replay_speed_policy::split_schedule(&base.options)?
                    .0
                    .to_vec();
                replay_speed_policy::wrap_header_with_schedule(
                    base,
                    &schedule,
                    header.identity.limits,
                )?
            };
            prepared.headers.push(RuntimeHeader {
                identity: PracticeRuntimeHeader {
                    player: header.identity.player,
                    header: candidate,
                    limits: header.identity.limits,
                },
                schedule,
            });
        }
        for (_, capture) in captures.iter_mut() {
            prepared.captures.push(
                capture
                    .as_mut()
                    .map(|capture| capture.prepare_source_rate_change(speed))
                    .transpose()?,
            );
        }
        Ok(prepared)
    }
    pub(crate) fn prepare_rate<'live, 'capture>(
        &self,
        event: PracticeRateEvent,
        point: OutputPhasePoint,
        transport: &'live mut Transport,
        captures: &'live mut [(PlayerId, &'capture mut Option<LiveReplayCapture>)],
        mut prepared: PreparedRuntimeRateRequest,
    ) -> NativeGameplayResult<PreparedRuntimeRateCommit<'live, 'capture>>
    where
        'capture: 'live,
    {
        self.validate_captures(captures)?;
        self.validate_transport(transport)?;
        self.validate_logical_domain(point.logical().domain)?;
        let PracticeTransactionRequest::Rate {
            id,
            generation,
            expected,
            requested_rate,
        } = prepared.request
        else {
            return Err("runtime rate result lacks correlated request".into());
        };
        if prepared.owner_id != self.owner_id
            || id != event.request_id
            || event.request_revision != expected.revision
            || requested_rate != event.requested_rate
            || event.previous_rate != self.rate.rate
            || event.generation == 0
            || event.playback_frame > event.physical_frame
            || point.elapsed_nanos() != event.active_elapsed_nanos
            || transport.output_phase_basis() != Some(point.basis())
            || prepared.headers.len() != self.headers.len()
            || prepared.captures.len() != captures.len()
        {
            return Err("runtime rate event differs from request, phase or roster".into());
        }
        transport.position_at_output_phase(point)?;
        match event.outcome {
            PracticeRateOutcome::Applied | PracticeRateOutcome::Noop => {
                if generation != event.generation
                    || expected != self.rate
                    || expected.revision.checked_add(1) != Some(event.applied_revision)
                    || event.applied_rate != requested_rate
                    || (event.outcome == PracticeRateOutcome::Noop)
                        != (requested_rate == event.previous_rate)
                {
                    return Err("runtime rate applied outcome differs from reservation".into());
                }
            }
            PracticeRateOutcome::Refused(reason) => {
                if event.applied_revision != self.rate.revision
                    || event.applied_rate != self.rate.rate
                    || (generation != event.generation
                        && reason != PracticeRateRefusal::StaleGeneration)
                {
                    return Err("runtime rate refusal changed semantic state".into());
                }
            }
        }
        let mut phase = self.phase;
        let candidate = if event.outcome == PracticeRateOutcome::Applied {
            phase = phase.with_rate_at_elapsed(event.active_elapsed_nanos, event.applied_rate)?;
            let mut candidate = transport.clone();
            candidate.try_reserve_output_phase_controls(1)?;
            candidate.set_rate_at_output_phase(point, event.applied_rate)?;
            let (whole, numerator, denominator) =
                phase.original_song_phase_at_elapsed(event.active_elapsed_nanos)?;
            let cut = SourceRateCut::new(
                whole,
                numerator,
                denominator,
                ReplaySpeed::new(event.applied_rate)?,
            )?;
            for header in &self.headers {
                header.schedule.validate_change(cut)?;
            }
            for (old, candidate) in self.headers.iter().zip(&mut prepared.headers) {
                let offset = replay_speed_policy::prepared_cut_offset(old.schedule.cuts().len());
                cut.write_bytes(&mut candidate.identity.header.options[offset..offset + 48]);
                candidate.schedule.replace_prepared_cut(cut);
            }
            for ((_, capture), token) in captures.iter_mut().zip(&mut prepared.captures) {
                if let (Some(capture), Some(token)) = (capture.as_mut(), token.as_mut()) {
                    capture.prepare_applied_source_rate_change(token, cut)?;
                }
            }
            Some(candidate)
        } else {
            None
        };
        Ok(PreparedRuntimeRateCommit {
            transport,
            captures,
            candidate,
            request: prepared,
            event,
            phase,
            previous: self.rate,
        })
    }
    pub(crate) fn apply_rate(
        &mut self,
        mut prepared: PreparedRuntimeRateCommit<'_, '_>,
    ) -> NativeGameplayResult<()> {
        if self.rate != prepared.previous || prepared.request.owner_id != self.owner_id {
            return Err("stale runtime rate business commit".into());
        }
        if let Some(candidate) = prepared.candidate.take() {
            // Exclusive borrows prevent report, roster or token mutation after
            // every fill validated its actual header/byte accounting.
            *prepared.transport = candidate;
            for ((_, capture), token) in prepared
                .captures
                .iter_mut()
                .zip(&mut prepared.request.captures)
            {
                if let (Some(capture), Some(token)) = (capture.as_mut(), token.take()) {
                    capture
                        .commit_source_rate_change(token)
                        .expect("exclusive filled capture cannot change before commit");
                }
            }
            self.headers = prepared.request.headers;
            self.phase = prepared.phase;
        }
        self.rate = PracticeRateState {
            revision: prepared.event.applied_revision,
            rate: prepared.event.applied_rate,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatkernel::{audio::*, time::ClockDomainId};
    fn ts(ms: i64) -> Timestamp {
        Timestamp::from_nanos(ms * 1_000_000)
    }
    fn setup(repeat: bool) -> (PracticePlayback, Mixer) {
        setup_completion(repeat, false)
    }
    fn setup_completion(repeat: bool, full: bool) -> (PracticePlayback, Mixer) {
        let original = beatkernel_bms::parse(
            "#BPM 120\n#WAV01 key.wav\n#00011:01\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let policy = crate::native_judge::NativeJudgeConfig {
            early: 1,
            late: 1,
            offset: 0,
            preroll: 0,
            output: ClockDomainId(7),
            end: None,
        }
        .resolve_play_policy(
            &crate::play_policy::OriginalGaugeContext::from_source(&original),
            crate::play_policy::GaugeSelection::BeatKernel,
        )
        .unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(8, 128, 1).unwrap()).unwrap();
        let program = PreparedPracticeProgram::new(
            &bank,
            vec![],
            PracticeLimits::new(8, 1, 64, 8, 16).unwrap(),
        )
        .unwrap();
        let (controller, endpoint) = practice_queue(&program).unwrap();
        let (_, consumer) = command_queue(8).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(7),
                ts(0),
                AudioLimits::new(8, 2, 8, 64, 8).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        let region = PracticeRegion::new(
            ts(0),
            if full {
                Timestamp::from_nanos(i64::MAX)
            } else {
                ts(4)
            },
            repeat,
        )
        .unwrap();
        mixer.install_practice(program, endpoint, region).unwrap();
        let playback = PracticePlayback::new_with_completion(
            controller,
            original,
            vec![PracticeMember {
                player: PlayerId(1),
                policy,
                launch: SessionLaunch::new(vec!["--chart".into(), "chart.bms".into()]).unwrap(),
                chart_seed: 0,
                capture_limits: None,
            }],
            region,
            ts(20),
            OutputFrameBasis::new(
                ClockPoint {
                    domain: ClockDomainId(7),
                    timestamp: ts(0),
                },
                1000,
                0,
            )
            .unwrap(),
            1,
            if full { None } else { Some(ts(4)) },
        )
        .unwrap();
        (playback, mixer)
    }

    #[test]
    fn repeated_peek_retains_first_real_mixer_receipt_and_preserves_later_laps() {
        let (mut owner, mut mixer) = setup(true);
        mixer.render(&mut [0.; 10]).unwrap();
        let first = owner.peek_receipt().unwrap().unwrap();
        let first_projection = owner.held_projected.unwrap();
        assert_eq!(
            (first.kind, first.generation, first.playback_frame),
            (PracticeBoundaryKind::Started, 1, 0)
        );
        assert_eq!(owner.peek_receipt().unwrap(), Some(first));
        assert_eq!(owner.held_projected, Some(first_projection));
        assert_eq!(first_projection.boundary.source_frame, 0);
        assert_eq!(first_projection.target_frame, 0);
        assert_eq!(first_projection.target_rate, 1000);
        // Simulate the owning pump's commit without discarding the queued laps.
        owner.started = true;
        owner.last_frames = Some((0, 0));
        owner.held = None;
        let second = owner.peek_receipt().unwrap().unwrap();
        assert_eq!(
            (
                second.kind,
                second.generation,
                second.iteration,
                second.playback_frame
            ),
            (PracticeBoundaryKind::Looped, 2, 1, 4)
        );
        owner.generation = 2;
        owner.iteration = 1;
        owner.last_frames = Some((4, 4));
        owner.held = None;
        let third = owner.peek_receipt().unwrap().unwrap();
        let third_projection = owner.held_projected.unwrap();
        assert_eq!(third_projection.boundary.source_frame, 8);
        assert_eq!(third_projection.target_frame, 8);
        assert_ne!(third_projection, first_projection);
        assert_eq!(
            (third.generation, third.iteration, third.playback_frame),
            (3, 2, 8)
        );
    }

    #[test]
    fn actual_nonrepeat_end_is_terminal_receipt_not_a_new_attempt() {
        let (mut owner, mut mixer) = setup(false);
        mixer.render(&mut [0.; 6]).unwrap();
        owner.peek_receipt().unwrap();
        owner.started = true;
        owner.held = None;
        let end = owner.peek_receipt().unwrap().unwrap();
        assert_eq!(
            (
                end.kind,
                end.generation,
                end.iteration,
                end.applied_song_time
            ),
            (PracticeBoundaryKind::Ended, 2, 1, ts(4))
        );
        assert!(owner.validate(end).is_ok());
    }

    #[test]
    fn malformed_generation_and_original_anchor_are_refused() {
        let (mut owner, mut mixer) = setup(false);
        mixer.render(&mut [0.; 1]).unwrap();
        let first = owner.peek_receipt().unwrap().unwrap();
        assert!(owner
            .validate(PracticeReceipt {
                generation: 2,
                ..first
            })
            .is_err());
        assert!(owner
            .validate(PracticeReceipt {
                applied_song_time: ts(1),
                ..first
            })
            .is_err());
        assert!(owner
            .validate(PracticeReceipt {
                playback_frame: 1,
                ..first
            })
            .is_err());
    }

    #[test]
    fn pending_edit_defers_terminal_and_rejected_revision_preserves_original_identity() {
        let (mut owner, mut mixer) = setup(false);
        mixer.render(&mut [0.; 6]).unwrap();
        owner.peek_receipt().unwrap();
        owner.started = true;
        owner.held = None;
        let end = owner.peek_receipt().unwrap().unwrap();
        owner.generation = end.generation;
        owner.iteration = end.iteration;
        owner.last_frames = Some((end.physical_frame, end.playback_frame));
        owner.ended = true;
        owner.held = None;
        assert!(owner.terminal_ready());
        let request = crate::practice_control::PracticeRequest {
            id: 11,
            generation: 1,
            action: PracticeAction::Scrub { target: ts(1) },
        };
        owner.pending = Some((request, PracticeRegion::new(ts(1), ts(4), false).unwrap()));
        assert!(!owner.terminal_ready());
        let refused = PracticeReceipt {
            request_id: 11,
            generation: 2,
            iteration: 1,
            physical_frame: 6,
            playback_frame: 6,
            requested_song_time: ts(1),
            applied_song_time: ts(4),
            correction_nanos: 3_000_000,
            kind: PracticeBoundaryKind::ControlRejectedRevision,
        };
        assert!(owner.validate(refused).is_ok());
        assert!(owner
            .validate(PracticeReceipt {
                request_id: 12,
                ..refused
            })
            .is_err());
        assert!(owner
            .validate(PracticeReceipt {
                correction_nanos: 0,
                ..refused
            })
            .is_err());
        assert_eq!(owner.pending.unwrap().0.generation, 1);
        assert_eq!(
            owner.region,
            PracticeRegion::new(ts(0), ts(4), false).unwrap()
        );
    }
    #[test]
    fn unlimited_program_ceiling_is_distinct_from_full_song_completion_intent() {
        let (mut owner, mut mixer) = setup_completion(false, true);
        assert_eq!(owner.section_end(), None);
        assert_eq!(owner.max_target(), ts(20));
        assert_eq!(owner.region().end, Timestamp::from_nanos(i64::MAX));
        assert!(!owner.can_complete_naturally());
        // Rendering beyond the selectable extent is not terminal proof.
        mixer.render(&mut [0.; 32]).unwrap();
        assert_eq!(
            owner.peek_receipt().unwrap().unwrap().kind,
            PracticeBoundaryKind::Started
        );
        owner.started = true;
        assert!(!owner.can_complete_naturally());
        owner.held = None;
        assert!(owner.can_complete_naturally());
        assert_eq!(owner.peek_receipt().unwrap(), None);
        owner.pending = Some((
            crate::practice_control::PracticeRequest {
                id: 1,
                generation: 1,
                action: PracticeAction::DisableLoop,
            },
            owner.region,
        ));
        assert!(!owner.can_complete_naturally());
        let (finite, _) = setup(false);
        assert_eq!(finite.section_end(), Some(ts(4)));
        assert!(!finite.can_complete_naturally());
    }

    #[test]
    fn sparse_original_bgm_start_floor_is_independent_of_key_sample_extent() {
        for (text, expected) in [
            ("#BPM 120\n#WAV01 key.wav\n#00011:01\n", Timestamp::ZERO),
            (
                "#BPM 120\n#WAV01 song.wav\n#00101:01\n#00301:01\n",
                ts(6000),
            ),
        ] {
            let (owner, _mixer) = setup_completion(false, true);
            let original =
                beatkernel_bms::parse(text, beatkernel_bms::ParseOptions::default()).unwrap();
            let owner = PracticePlayback::new_with_completion(
                owner.controller,
                original,
                owner.members,
                owner.region,
                ts(600_000),
                owner.basis,
                1,
                None,
            )
            .unwrap();
            // A decoded early key may contribute 600s to selectable extent;
            // completion still waits only future BGM starts plus actual voices.
            assert_eq!(owner.max_target(), ts(600_000));
            assert_eq!(owner.natural_completion_floor(), expected);
            assert_eq!(owner.section_end(), None);
        }
    }

    #[test]
    fn zero_player_is_refused_before_audio_render_or_control_admission() {
        let (mut owner, mut mixer) = setup(false);
        owner.members[0].player = PlayerId(0);
        assert!(PracticePlayback::validate_setup(
            &owner.members,
            owner.region,
            owner.max_target,
            owner.default_section_end
        )
        .is_err());
        assert_eq!(owner.controller.generation(), 1);
        assert_eq!(mixer.frame_cursor(), 0);
        // Cold borrowed validation leaves both actual endpoints usable.
        mixer.render(&mut [0.; 1]).unwrap();
        assert_eq!(
            owner.controller.try_pop_receipt().unwrap().kind,
            PracticeBoundaryKind::Started
        );

        let (mut owner, mixer) = setup(false);
        owner.members[0].player = PlayerId(0);
        assert!(PracticePlayback::new(
            owner.controller,
            owner.original,
            owner.members,
            owner.region,
            owner.max_target,
            owner.basis,
            1
        )
        .is_err());
        assert_eq!(mixer.frame_cursor(), 0);
    }

    #[test]
    fn completion_intent_refuses_malformed_finite_and_sentinel_boundaries() {
        let (owner, _) = setup(false);
        let validate = |region, end| {
            PracticePlayback::validate_setup(&owner.members, region, owner.max_target, end)
        };
        assert!(validate(owner.region, None).is_err());
        assert!(validate(owner.region, Some(ts(5))).is_err());
        assert!(validate(
            PracticeRegion::new(ts(-2), ts(-1), false).unwrap(),
            Some(ts(-1))
        )
        .is_err());
        let ceiling = Timestamp::from_nanos(i64::MAX);
        assert!(validate(PracticeRegion::new(ts(0), ceiling, true).unwrap(), None).is_err());
        assert!(validate(PracticeRegion::new(ts(21), ceiling, false).unwrap(), None).is_err());
        assert!(validate(PracticeRegion::new(ts(-2), ceiling, false).unwrap(), None).is_ok());
        assert!(validate(
            PracticeRegion::new(ts(-2), ts(4), false).unwrap(),
            Some(ts(4))
        )
        .is_ok());
    }

    fn owner_payload(
        owner: &PracticePlayback,
        next_launches: Vec<SessionLaunch>,
    ) -> PreparedPracticeCommit {
        PreparedPracticeCommit {
            receipt: owner.held.unwrap(),
            attempts: Vec::new(),
            presentation: None,
            next_launches,
            region: owner.region,
            end_intent: owner.end_intent,
            projected: owner.held_projected.unwrap(),
            reply: None,
            serial: owner.applied_serial + 1,
            owner_id: owner.owner_id,
            typed: None,
            typed_reply: None,
            runtime_headers: Vec::new(),
            runtime_phase: None,
            runtime_rate: None,
        }
    }
    struct RefusingObserver {
        calls: usize,
    }
    impl NativeGameplayHost for RefusingObserver {
        fn cancelled(&self) -> bool {
            false
        }
        fn pause_requested(&self) -> bool {
            false
        }
        fn retry_pause_publication(&mut self) {}
        fn publish_pause(&mut self, _: crate::native_gameplay_host::PauseState) {}
        fn publish_section_end(&mut self, _: Timestamp) {}
        fn publish_report(
            &mut self,
            _: &beatkernel::runtime::RuntimeReport,
        ) -> NativeGameplayResult<()> {
            Ok(())
        }
        fn publish_local_reports(
            &mut self,
            _: &[crate::local_runtime::PlayerReport],
        ) -> NativeGameplayResult<()> {
            Ok(())
        }
        fn diagnostic(&mut self, _: crate::native_gameplay_host::NativeGameplayDiagnostic<'_>) {}
        fn advertise_practice(
            &mut self,
            _: Option<PracticeCapability>,
        ) -> NativeGameplayResult<()> {
            self.calls += 1;
            Err("scripted late UI refusal".into())
        }
    }

    #[test]
    fn real_loop_application_keeps_fresh_recording_identity_after_observer_refusal() {
        let (mut owner, mut mixer) = setup(true);
        owner.members[0].launch = SessionLaunch::new(vec![
            "--chart".into(),
            "chart.bms".into(),
            "--record-replay".into(),
            "records/base.bkr".into(),
        ])
        .unwrap();
        mixer.render(&mut [0.; 5]).unwrap();
        owner.peek_receipt().unwrap();
        let mut bootstrap = owner_payload(&owner, Vec::new());
        owner.apply_boundary(&mut bootstrap).unwrap();
        let looped = owner.peek_receipt().unwrap().unwrap();
        assert_eq!(
            (looped.kind, looped.generation),
            (PracticeBoundaryKind::Looped, 2)
        );
        let mut observer = RefusingObserver { calls: 0 };
        let mut invalid = owner_payload(&owner, Vec::new());
        assert!(owner.apply_boundary(&mut invalid).is_err());
        assert_eq!(owner.generation(), 1);
        assert_eq!(owner.members()[0].launch.attempt(), 0);
        assert_eq!(owner.peek_receipt().unwrap(), Some(looped));
        let mut prepared = owner_payload(&owner, vec![owner.members()[0].launch.retry().unwrap()]);
        let applied = owner.apply_boundary(&mut prepared).unwrap();
        assert_eq!(observer.calls, 0);
        assert_eq!(applied.receipt(), looped);
        assert_eq!(owner.generation(), 2);
        assert_eq!(owner.members()[0].launch.attempt(), 1);
        assert!(owner.members()[0]
            .launch
            .args()
            .windows(2)
            .any(|pair| pair == ["--record-replay", "records/base.retry1.bkr"]));
        assert!(prepared.next_launches.is_empty());
        assert!(owner.apply_boundary(&mut prepared).is_err());
        assert_eq!(
            owner
                .publish_boundary(&applied, &mut observer)
                .unwrap_err()
                .to_string(),
            "scripted late UI refusal"
        );
        assert_eq!(observer.calls, 1);
        assert_eq!(owner.generation(), 2);
        assert_eq!(owner.members()[0].launch.attempt(), 1);
        assert!(owner.held_projected.is_none());
        assert_eq!(owner.members()[0].launch.retry().unwrap().attempt(), 2);
        let (mut other, mut other_mixer) = setup(true);
        other_mixer.render(&mut [0.; 5]).unwrap();
        other.peek_receipt().unwrap();
        let mut start = owner_payload(&other, Vec::new());
        other.apply_boundary(&mut start).unwrap();
        other.peek_receipt().unwrap();
        let mut next = owner_payload(&other, vec![other.members()[0].launch.retry().unwrap()]);
        other.apply_boundary(&mut next).unwrap();
        assert!(other.publish_boundary(&applied, &mut observer).is_err());
        assert_eq!(observer.calls, 1);
    }
}

/// Explicit control-thread effect. Implementations consume genuine captures,
/// preserving exclusive-create failure and the supplied original retry path.
pub trait PracticeRecordingPort {
    fn archive(
        &mut self,
        player: PlayerId,
        path: &Path,
        capture: LiveReplayCapture,
    ) -> NativeGameplayResult<()>;
}

/// Legacy generic placeholder. Genuine recording is never silently discarded.
pub struct NoopPracticeRecording;
impl PracticeRecordingPort for NoopPracticeRecording {
    fn archive(&mut self, _: PlayerId, _: &Path, _: LiveReplayCapture) -> NativeGameplayResult<()> {
        Err("retained practice recording effect is unsupported".into())
    }
}

/// Cold payload, not authorization to change the endpoint or input chronology.
pub struct PreparedPracticeCommit {
    pub receipt: PracticeReceipt,
    pub attempts: Vec<(PlayerId, PreparedPracticeAttempt)>,
    pub presentation: Option<PreparedPracticePresentation>,
    next_launches: Vec<SessionLaunch>,
    region: PracticeRegion,
    end_intent: Option<Timestamp>,
    projected: ProjectedPracticeReceipt,
    reply: Option<PracticeReply>,
    serial: u64,
    owner_id: u64,
    typed: Option<ProjectedPracticeEvent>,
    typed_reply: Option<PracticeTransactionReply>,
    runtime_headers: Vec<RuntimeHeader>,
    runtime_phase: Option<PositiveRatePhase>,
    runtime_rate: Option<PracticeRateState>,
}

/// Proof that the retained attempt identity has changed. Observer ports cannot
/// manufacture this token or roll the committed owner back after refusal.
pub struct AppliedPracticeBoundary {
    receipt: PracticeReceipt,
    reply: Option<PracticeReply>,
    serial: u64,
    owner_id: u64,
    typed_reply: Option<PracticeTransactionReply>,
}
impl AppliedPracticeBoundary {
    pub fn receipt(&self) -> PracticeReceipt {
        self.receipt
    }
}

pub struct PracticePlayback {
    controller: PracticeController,
    original: BmsChart,
    members: Vec<PracticeMember>,
    region: PracticeRegion,
    max_target: Timestamp,
    natural_completion_floor: Timestamp,
    default_section_end: Option<Timestamp>,
    end_intent: Option<Timestamp>,
    basis: OutputFrameBasis,
    generation: u64,
    iteration: u64,
    last_frames: Option<(u64, u64)>,
    held: Option<PracticeReceipt>,
    held_projected: Option<ProjectedPracticeReceipt>,
    pending: Option<(crate::practice_control::PracticeRequest, PracticeRegion)>,
    started: bool,
    ended: bool,
    applied_serial: u64,
    owner_id: u64,
    initial_setup: Option<LiveSpeedSetup>,
    runtime: Option<RuntimePractice>,
    held_event: Option<ProjectedPracticeEvent>,
    pending_transaction: Option<RuntimePending>,
}

impl PracticePlayback {
    /// Admission fence only: an existing output transaction still owns its work.
    pub fn transition_pending(&self) -> bool {
        let (source, target) = self.controller.history_retention_pins();
        self.pending.is_some()
            || self.held.is_some()
            || self.held_projected.is_some()
            || self.held_event.is_some()
            || self.pending_transaction.is_some()
            || source.is_some()
            || target.is_some()
    }

    /// Mathematical history pins for the projected receipt currently held by
    /// this owner. Queued receipts remain the audio endpoint's responsibility.
    pub fn history_retention_pins(&self) -> Option<(u64, InterpolatedTargetTime)> {
        if let Some(projected) = self.held_event {
            let target = InterpolatedTargetTime::new(projected.boundary.target_time, 0, 0, 1, 0, 1)
                .expect("exact TargetTime with zero interpolation is representable");
            return Some((projected.boundary.source_frame, target));
        }
        let projected = self.held_projected?;
        let target = InterpolatedTargetTime::new(projected.boundary.target_time, 0, 0, 1, 0, 1)
            .expect("exact TargetTime with zero interpolation is representable");
        Some((projected.receipt.physical_frame, target))
    }

    /// Original queued receipt pins, including raw Source receipts that do not
    /// yet have a Target projection. This does not consume or hold a receipt.
    pub fn queued_history_retention_pins(&self) -> (Option<u64>, Option<InterpolatedTargetTime>) {
        let (source, target) = self.controller.history_retention_pins();
        let target = target.map(|time| {
            InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1)
                .expect("exact TargetTime with zero interpolation is representable")
        });
        (source, target)
    }

    pub fn new(
        controller: PracticeController,
        original: BmsChart,
        members: Vec<PracticeMember>,
        initial_region: PracticeRegion,
        max_target: Timestamp,
        mixer_basis: OutputFrameBasis,
        _control_lead_frames: u64,
    ) -> NativeGameplayResult<Self> {
        Self::new_with_completion(
            controller,
            original,
            members,
            initial_region,
            max_target,
            mixer_basis,
            _control_lead_frames,
            Some(initial_region.end),
        )
    }

    /// `None` is dynamic full-song completion, whose program ceiling is only
    /// representability. Native idle/drain and input chronology remain required.
    pub fn new_with_completion(
        controller: PracticeController,
        original: BmsChart,
        members: Vec<PracticeMember>,
        initial_region: PracticeRegion,
        max_target: Timestamp,
        mixer_basis: OutputFrameBasis,
        _control_lead_frames: u64,
        default_section_end: Option<Timestamp>,
    ) -> NativeGameplayResult<Self> {
        Self::new_inner(
            controller,
            original,
            members,
            initial_region,
            max_target,
            mixer_basis,
            default_section_end,
            None,
        )
    }

    /// Opt-in initial whole-preroll validation. The Started receipt describes
    /// the initial snapshot and must not reanchor the existing exact transport.
    /// Fresh selected-speed commits remain gated on exact fractional phase.
    pub fn new_with_speed(
        controller: PracticeController,
        original: BmsChart,
        members: Vec<PracticeMember>,
        initial_region: PracticeRegion,
        max_target: Timestamp,
        mixer_basis: OutputFrameBasis,
        _control_lead_frames: u64,
        default_section_end: Option<Timestamp>,
        initial_setup: LiveSpeedSetup,
    ) -> NativeGameplayResult<Self> {
        if initial_setup.start() != initial_region.start
            || initial_setup.end() != default_section_end
            || initial_setup.start() < Timestamp::ZERO
            || controller.rate() != initial_setup.speed().rate()
            || controller.output_preroll() != initial_setup.preroll()
            || controller.has_open_end() != default_section_end.is_none()
        {
            return Err("retained practice initial speed setup differs from region".into());
        }
        for member in &members {
            crate::practice_session::validate_launch_speed(&member.launch, initial_setup.speed())?;
        }
        // The public receipt carries its signed initial snapshot correction.
        i64::try_from(
            i128::from(initial_setup.song_origin().as_nanos())
                - i128::from(initial_setup.start().as_nanos()),
        )?;
        Self::new_inner(
            controller,
            original,
            members,
            initial_region,
            max_target,
            mixer_basis,
            default_section_end,
            Some(initial_setup),
        )
    }

    fn new_inner(
        controller: PracticeController,
        original: BmsChart,
        members: Vec<PracticeMember>,
        initial_region: PracticeRegion,
        max_target: Timestamp,
        mixer_basis: OutputFrameBasis,
        default_section_end: Option<Timestamp>,
        initial_setup: Option<LiveSpeedSetup>,
    ) -> NativeGameplayResult<Self> {
        Self::validate_setup(&members, initial_region, max_target, default_section_end)?;
        if controller.generation() != 1 {
            return Err("retained practice controller is not pristine".into());
        }
        // A retained program owns future BGM itself, so an empty legacy feeder
        // is insufficient EOF evidence. Only cue starts belong in this floor;
        // actual active voices/native drain prove tails after a selected start.
        let natural_completion_floor = original
            .compile()?
            .bgm
            .iter()
            .map(|cue| cue.at)
            .max()
            .unwrap_or(Timestamp::ZERO);
        let owner_id = NEXT_OWNER_ID
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |next| next.checked_add(1),
            )
            .map_err(|_| "practice owner identity exhausted")?;
        Ok(Self {
            controller,
            original,
            members,
            region: initial_region,
            max_target,
            natural_completion_floor,
            default_section_end,
            end_intent: default_section_end,
            basis: mixer_basis,
            generation: 1,
            iteration: 0,
            last_frames: None,
            held: None,
            held_projected: None,
            pending: None,
            started: false,
            ended: false,
            applied_serial: 0,
            owner_id,
            initial_setup,
            runtime: None,
            held_event: None,
            pending_transaction: None,
        })
    }

    pub fn new_with_runtime_rates(
        controller: PracticeController,
        original: BmsChart,
        members: Vec<PracticeMember>,
        initial_region: PracticeRegion,
        max_target: Timestamp,
        mixer_basis: OutputFrameBasis,
        default_section_end: Option<Timestamp>,
        initial_setup: LiveSpeedSetup,
        headers: Vec<PracticeRuntimeHeader>,
    ) -> NativeGameplayResult<Self> {
        let metadata = controller.metadata()?;
        if metadata.revision != 0 || metadata.started || !metadata.active {
            return Err("runtime practice requires a pristine retained program".into());
        }
        // Also proves this controller opted into the retained runtime contract;
        // fixed-speed programs expose the same metadata but cannot bind rates.
        controller.prepare_rate(initial_setup.speed().rate())?;
        let mut owner = Self::new_with_speed(
            controller,
            original,
            members,
            initial_region,
            max_target,
            mixer_basis,
            0,
            default_section_end,
            initial_setup,
        )?;
        if headers.len() != owner.members.len() {
            return Err("runtime practice header roster differs".into());
        }
        let source = crate::section_start::source_at(&owner.original, initial_setup.start())?;
        let logical_domain = headers[0].header.normalized_clock;
        let mut actual = Vec::new();
        actual.try_reserve_exact(headers.len())?;
        for (member, identity) in owner.members.iter().zip(headers) {
            if member.player != identity.player
                || identity.header.normalized_clock != logical_domain
            {
                return Err("runtime practice header membership or clock differs".into());
            }
            let judge = crate::native_judge::NativeJudgeConfig {
                early: member.policy.judge().max_early().as_nanos(),
                late: member.policy.completion_late().as_nanos(),
                offset: member.policy.judge().input_offset().as_nanos(),
                preroll: 0,
                output: identity.header.normalized_clock,
                end: initial_setup.end(),
            }
            .judge_with_policy(&source, source.source.compile()?, &member.policy)?;
            let expected = crate::replay_capture::setup_play_policy_header_with_speed(
                &judge,
                identity.header.normalized_clock,
                identity.limits,
                initial_setup.start(),
                member.chart_seed,
                initial_setup.end(),
                beatkernel_bms::BmsInputMode::ButtonOnly,
                crate::input_sounds::InputSoundIdentity::from_source(&source)?,
                &member.policy,
                initial_setup.speed(),
            )?;
            if identity.header != expected {
                return Err("runtime practice header differs from actual initial policy".into());
            }
            let (_, schedule) = replay_speed_policy::split_schedule(&identity.header.options)?;
            if schedule.is_variable() || schedule.current_speed() != initial_setup.speed() {
                return Err("runtime practice initial rate history differs".into());
            }
            actual.push(RuntimeHeader { identity, schedule });
        }
        let captured: Vec<_> = owner
            .members
            .iter()
            .filter(|member| member.capture_limits.is_some())
            .map(|member| member.player)
            .collect();
        let headers = actual.into_iter().map(|header| header.identity).collect();
        owner.runtime = Some(RuntimePractice::new(
            headers,
            &captured,
            PositiveRatePhase::new(
                initial_setup.start(),
                initial_setup.preroll(),
                initial_setup.speed().rate(),
                initial_setup.end(),
            )?,
        )?);
        Ok(owner)
    }

    fn validate_setup(
        members: &[PracticeMember],
        initial_region: PracticeRegion,
        max_target: Timestamp,
        default_section_end: Option<Timestamp>,
    ) -> NativeGameplayResult<()> {
        PracticeRegion::new(
            initial_region.start,
            initial_region.end,
            initial_region.repeat,
        )?;
        if members.is_empty()
            || members.len() > crate::local_players::MAX_LOCAL_PLAYERS
            || members.iter().enumerate().any(|(i, m)| {
                m.player.0 == 0 || members[..i].iter().any(|old| old.player == m.player)
            })
            || max_target <= Timestamp::ZERO
            || initial_region.start > max_target
            || match default_section_end {
                Some(end) => {
                    initial_region.end != end
                        || end > max_target
                        || end <= Timestamp::ZERO
                        || end == Timestamp::from_nanos(i64::MAX)
                }
                None => {
                    initial_region.end != Timestamp::from_nanos(i64::MAX) || initial_region.repeat
                }
            }
        {
            return Err("invalid retained practice owners or bounds".into());
        }
        Ok(())
    }

    pub fn initial_setup(&self) -> Option<LiveSpeedSetup> {
        self.initial_setup
    }
    pub fn speed(&self) -> ReplaySpeed {
        self.initial_setup.map_or_else(
            || ReplaySpeed::new(beatkernel::transport::Rate::NORMAL).expect("NORMAL is positive"),
            |setup| setup.speed(),
        )
    }
    pub fn current_speed(&self) -> ReplaySpeed {
        self.runtime
            .as_ref()
            .map_or_else(|| self.speed(), RuntimePractice::current_speed)
    }
    pub fn rate_state(&self) -> Option<PracticeRateState> {
        self.runtime.as_ref().map(RuntimePractice::rate_state)
    }
    pub fn effective_header(&self, player: PlayerId) -> Option<&ReplayHeader> {
        self.runtime.as_ref()?.effective_header(player)
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn region(&self) -> PracticeRegion {
        self.region
    }
    pub fn members(&self) -> &[PracticeMember] {
        &self.members
    }
    pub fn mixer_basis(&self) -> OutputFrameBasis {
        self.basis
    }
    pub fn ended(&self) -> bool {
        self.ended
    }
    /// An already admitted control must receive its actual ordered audio result
    /// even when an autonomous end reached presentation first.
    pub fn terminal_ready(&self) -> bool {
        self.ended
            && self.pending.is_none()
            && self.held.is_none()
            && self.held_event.is_none()
            && self.pending_transaction.is_none()
    }
    pub fn original(&self) -> &BmsChart {
        &self.original
    }
    pub fn default_section_end(&self) -> Option<Timestamp> {
        self.default_section_end
    }
    pub fn max_target(&self) -> Timestamp {
        self.max_target
    }
    pub fn natural_completion_floor(&self) -> Timestamp {
        self.natural_completion_floor
    }
    pub fn section_end(&self) -> Option<Timestamp> {
        self.end_intent
    }
    pub fn can_complete_naturally(&self) -> bool {
        self.started
            && !self.ended
            && !self.region.repeat
            && self.end_intent.is_none()
            && self.pending.is_none()
            && self.held.is_none()
            && self.held_event.is_none()
            && self.pending_transaction.is_none()
    }

    pub fn advertise<H: NativeGameplayHost>(&self, host: &mut H) -> NativeGameplayResult<()> {
        host.advertise_practice(if self.terminal_ready() {
            None
        } else {
            Some(PracticeCapability {
                generation: self.generation,
                min_target: Timestamp::ZERO,
                max_target: self.max_target,
            })
        })
    }

    pub fn advertise_transactions<H: NativeGameplayHost>(
        &self,
        host: &mut H,
    ) -> NativeGameplayResult<()> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or("practice runtime rates are not enabled")?;
        host.advertise_practice_transactions(if self.terminal_ready() {
            None
        } else {
            Some(PracticeTransactionCapability {
                practice: PracticeCapability {
                    generation: self.generation,
                    min_target: Timestamp::ZERO,
                    max_target: self.max_target,
                },
                rate: runtime.rate,
            })
        })
    }

    fn validate_captures(
        &self,
        captures: &[(PlayerId, &mut Option<LiveReplayCapture>)],
    ) -> NativeGameplayResult<()> {
        self.runtime
            .as_ref()
            .ok_or("practice runtime rates are not enabled")?
            .validate_captures(captures)
    }

    fn requested_region(&self, action: PracticeAction) -> NativeGameplayResult<PracticeRegion> {
        let in_range = |time: Timestamp| time >= Timestamp::ZERO && time <= self.max_target;
        Ok(match action {
            PracticeAction::Scrub { target }
                if in_range(target) && self.default_section_end.is_none_or(|end| target < end) =>
            {
                PracticeRegion::new(
                    target,
                    self.default_section_end
                        .unwrap_or(Timestamp::from_nanos(i64::MAX)),
                    false,
                )?
            }
            PracticeAction::Loop { start, end } if in_range(start) && in_range(end) => {
                PracticeRegion::new(start, end, true)?
            }
            PracticeAction::DisableLoop => PracticeRegion {
                repeat: false,
                ..self.region
            },
            _ => return Err("practice target is outside the original song".into()),
        })
    }

    pub fn admit_transaction(
        &mut self,
        request: PracticeTransactionRequest,
        active: bool,
        transport: &mut Transport,
        captures: &mut [(PlayerId, &mut Option<LiveReplayCapture>)],
        fixed_competition: bool,
    ) -> NativeGameplayResult<()> {
        self.validate_captures(captures)?;
        let runtime = self
            .runtime
            .as_ref()
            .ok_or("practice runtime rates are not enabled")?;
        if self.pending_transaction.is_some()
            || self.held_event.is_some()
            || self.pending.is_some()
            || self.held.is_some()
        {
            return Err("another practice transaction is pending".into());
        }
        let expected = runtime.rate;
        let (id, generation, region, requested_speed) = match request {
            PracticeTransactionRequest::Rate {
                id,
                generation,
                expected: requested,
                requested_rate,
            } => {
                if requested != expected {
                    return Err("practice rate revision is stale".into());
                }
                let speed = ReplaySpeed::new(requested_rate)?;
                if fixed_competition && requested_rate != expected.rate {
                    return Err(
                        "runtime rate changes are unavailable in fixed-condition competition"
                            .into(),
                    );
                }
                (id, generation, self.region, Some(speed))
            }
            PracticeTransactionRequest::RestartOriginal {
                id,
                generation,
                expected: requested,
            } => {
                if requested != expected || !active {
                    return Err("original restart is unavailable or stale".into());
                }
                let setup = self
                    .initial_setup
                    .ok_or("original restart lacks pinned setup")?;
                (
                    id,
                    generation,
                    PracticeRegion::new(
                        setup.start(),
                        setup.end().unwrap_or(Timestamp::from_nanos(i64::MAX)),
                        false,
                    )?,
                    None,
                )
            }
            PracticeTransactionRequest::Region(region) => {
                if !active {
                    return Err("practice region is unavailable while paused".into());
                }
                (
                    region.id,
                    region.generation,
                    self.requested_region(region.action)?,
                    None,
                )
            }
        };
        if id == 0 || generation != self.generation || !self.started || self.ended {
            return Err("practice transaction belongs to an unavailable generation".into());
        }
        runtime.validate_transport(transport)?;
        expected
            .revision
            .checked_add(1)
            .ok_or("practice revision exhausted")?;
        transport.try_reserve_output_phase_controls(1)?;
        let pending = RuntimePending {
            request,
            expected,
            region,
            rate: if requested_speed.is_some() {
                Some(runtime.reserve_rate(request, transport, captures)?)
            } else {
                None
            },
        };
        // Every owned candidate and capture byte reservation exists before the
        // single final enqueue. Dropping this payload cancels all reservations.
        match request {
            PracticeTransactionRequest::Rate { requested_rate, .. } => {
                let prepared = self.controller.prepare_rate(requested_rate)?;
                self.controller
                    .try_set_rate_next(id, generation, &prepared)?;
            }
            PracticeTransactionRequest::RestartOriginal { .. } => {
                let prepared = self.controller.prepare_rate(self.speed().rate())?;
                self.controller
                    .try_restart_next(id, generation, region, &prepared)?;
            }
            PracticeTransactionRequest::Region(request) => {
                match request.action {
                    PracticeAction::DisableLoop => {
                        self.controller.try_disable_loop_next(id, generation)?
                    }
                    _ => self.controller.try_request_next(id, generation, region)?,
                }
                self.pending = Some((request, region));
            }
        }
        self.pending_transaction = Some(pending);
        Ok(())
    }

    pub fn service_transaction<H: NativeGameplayHost>(
        &mut self,
        host: &mut H,
        active: bool,
        transport: &mut Transport,
        captures: &mut [(PlayerId, &mut Option<LiveReplayCapture>)],
        fixed_competition: bool,
    ) -> NativeGameplayResult<()> {
        if self.transition_pending() {
            return Ok(());
        }
        let request = match host.take_practice_transaction(active) {
            Ok(request) => request,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let Some(request) = request else {
            return Ok(());
        };
        if let Err(error) =
            self.admit_transaction(request, active, transport, captures, fixed_competition)
        {
            let result = error.to_string();
            let reply = match request {
                PracticeTransactionRequest::Region(request) => PracticeTransactionReply::Region {
                    reply: PracticeReply {
                        id: request.id,
                        generation: request.generation,
                        result: Err(result),
                    },
                    rate: self.rate_state(),
                },
                PracticeTransactionRequest::Rate { id, generation, .. } => {
                    PracticeTransactionReply::Rate {
                        id,
                        generation,
                        result: Err(result),
                    }
                }
                PracticeTransactionRequest::RestartOriginal { id, generation, .. } => {
                    PracticeTransactionReply::RestartOriginal {
                        id,
                        generation,
                        result: Err(result),
                    }
                }
            };
            host.commit_practice_transaction_reply(&reply)?;
        }
        Ok(())
    }

    pub fn peek_event(&mut self) -> NativeGameplayResult<Option<ProjectedPracticeEvent>> {
        if self.runtime.is_none() {
            return Err("practice runtime rates are not enabled".into());
        }
        if self.held_event.is_none() {
            let projected = match self.controller.try_pop_projected_event() {
                Ok(event) => event,
                Err(PracticeError::Empty) => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let (physical, playback) = match projected.event {
                PracticeEvent::Boundary(receipt) => {
                    (receipt.physical_frame, receipt.playback_frame)
                }
                PracticeEvent::Rate(event) => (event.physical_frame, event.playback_frame),
                PracticeEvent::Restart(event) => {
                    (event.boundary.physical_frame, event.boundary.playback_frame)
                }
            };
            if projected.boundary.source_frame != physical
                || physical < self.basis.start_physical_frame()
                || playback > physical
                || projected.origin != self.basis.origin()
                || projected.target_rate == 0
                || self
                    .last_frames
                    .is_some_and(|(p, b)| physical < p || playback < b)
            {
                return Err("practice typed event projection differs from owner".into());
            }
            self.held_event = Some(projected);
            let receipt = match projected.event {
                PracticeEvent::Boundary(receipt) => Some(receipt),
                PracticeEvent::Restart(event) => Some(event.boundary),
                PracticeEvent::Rate(_) => None,
            };
            if let Some(receipt) = receipt {
                self.held = Some(receipt);
                self.held_projected = Some(ProjectedPracticeReceipt {
                    receipt,
                    boundary: projected.boundary,
                    target_frame: projected.target_frame,
                    target_rate: projected.target_rate,
                    origin: projected.origin,
                });
                self.validate(receipt)?;
            }
        }
        Ok(self.held_event)
    }

    pub fn qualify_event(
        &mut self,
        presentation: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> NativeGameplayResult<Option<PreparedPracticeEventBoundary>> {
        let Some(event) = self.peek_event()? else {
            return Ok(None);
        };
        let boundary = presentation.prepare_projected_practice_event(event, now)?;
        if let Some(boundary) = &boundary {
            self.runtime
                .as_ref()
                .ok_or("practice runtime rates are not enabled")?
                .validate_logical_domain(boundary.output().domain)?;
        }
        Ok(boundary)
    }

    pub fn prepare_rate_event<'live, 'capture>(
        &mut self,
        boundary: &PreparedPracticeEventBoundary,
        point: OutputPhasePoint,
        transport: &'live mut Transport,
        captures: &'live mut [(PlayerId, &'capture mut Option<LiveReplayCapture>)],
    ) -> NativeGameplayResult<PreparedPracticeRateCommit<'live, 'capture>>
    where
        'capture: 'live,
    {
        if point.logical() != boundary.output() {
            return Err("practice rate native proof differs from actual cut".into());
        }
        self.prepare_rate_event_inner(boundary.projected_event(), point, transport, captures)
    }

    /// Native-only rate preparation retains the actual first Target event
    /// while applying the exact Source cut qualified by its opaque token.
    pub(crate) fn prepare_rate_event_at_cut<'live, 'capture>(
        &mut self,
        cut: &PreparedPracticeRateCut,
        transport: &'live mut Transport,
        captures: &'live mut [(PlayerId, &'capture mut Option<LiveReplayCapture>)],
    ) -> NativeGameplayResult<PreparedPracticeRateCommit<'live, 'capture>>
    where
        'capture: 'live,
    {
        self.prepare_rate_event_inner(
            cut.projected_boundary().projected_event(),
            cut.phase(),
            transport,
            captures,
        )
    }

    fn prepare_rate_event_inner<'live, 'capture>(
        &mut self,
        original_projected: ProjectedPracticeEvent,
        point: OutputPhasePoint,
        transport: &'live mut Transport,
        captures: &'live mut [(PlayerId, &'capture mut Option<LiveReplayCapture>)],
    ) -> NativeGameplayResult<PreparedPracticeRateCommit<'live, 'capture>>
    where
        'capture: 'live,
    {
        let projected = self
            .held_event
            .ok_or("no actual typed practice event is held")?;
        if original_projected != projected {
            return Err("practice rate native proof differs from actual cut".into());
        }
        self.runtime
            .as_ref()
            .ok_or("practice runtime rates are not enabled")?
            .validate_logical_domain(point.logical().domain)?;
        let PracticeEvent::Rate(event) = projected.event else {
            return Err("held event is not a rate result".into());
        };
        if event.generation != self.generation || event.iteration != self.iteration {
            return Err("practice rate result differs from retained attempt".into());
        }
        let serial = self
            .applied_serial
            .checked_add(1)
            .ok_or("practice application identity exhausted")?;
        let pending = self
            .pending_transaction
            .as_mut()
            .ok_or("rate result has no correlated request")?;
        // Keep the original reservation alive through failed preparation and
        // discarded candidates. Reports must keep accounting for this actual
        // pending PCM change until its eventual commit or owner cancellation.
        let request = pending
            .rate
            .as_ref()
            .ok_or("rate result lost its reserved business candidates")?
            .clone();
        let business = self
            .runtime
            .as_ref()
            .ok_or("practice runtime rates are not enabled")?
            .prepare_rate(event, point, transport, captures, request)?;
        Ok(PreparedPracticeRateCommit {
            business,
            event,
            projected,
            serial,
            owner_id: self.owner_id,
        })
    }

    pub fn apply_rate_event(
        &mut self,
        prepared: PreparedPracticeRateCommit<'_, '_>,
    ) -> NativeGameplayResult<AppliedPracticeRateEvent> {
        if prepared.owner_id != self.owner_id
            || self.held_event != Some(prepared.projected)
            || self.applied_serial.checked_add(1) != Some(prepared.serial)
        {
            return Err("stale prepared practice rate commit".into());
        }
        let pending = self
            .pending_transaction
            .as_ref()
            .ok_or("prepared rate lost its original transaction")?;
        let PracticeTransactionRequest::Rate { id, generation, .. } = pending.request else {
            return Err("prepared rate lost its original identity".into());
        };
        if id != prepared.event.request_id || pending.rate.is_none() {
            return Err("prepared rate business reservation differs".into());
        }
        let applied = AppliedPracticeRateEvent {
            event: prepared.event,
            reply: PracticeTransactionReply::Rate {
                id,
                generation,
                result: Ok(prepared.event),
            },
            serial: prepared.serial,
            owner_id: self.owner_id,
        };
        self.runtime
            .as_mut()
            .ok_or("practice runtime rates are not enabled")?
            .apply_rate(prepared.business)?;
        self.last_frames = Some((prepared.event.physical_frame, prepared.event.playback_frame));
        self.applied_serial = prepared.serial;
        self.held_event = None;
        self.pending_transaction = None;
        Ok(applied)
    }

    pub fn publish_rate_event<H: NativeGameplayHost>(
        &self,
        applied: &AppliedPracticeRateEvent,
        host: &mut H,
    ) -> NativeGameplayResult<()> {
        if applied.owner_id != self.owner_id
            || applied.serial != self.applied_serial
            || self.last_frames
                != Some((applied.event.physical_frame, applied.event.playback_frame))
            || self.rate_state()
                != Some(PracticeRateState {
                    revision: applied.event.applied_revision,
                    rate: applied.event.applied_rate,
                })
        {
            return Err("stale applied rate publication".into());
        }
        host.commit_practice_transaction_reply(&applied.reply)?;
        self.advertise_transactions(host)
    }

    /// Admit a copied edit for the audio owner's next render boundary. The
    /// legacy cursor argument is retained for callers; no HOST scheduling occurs.
    pub fn service_request<H: NativeGameplayHost>(
        &mut self,
        host: &mut H,
        _actual_playback_cursor: u64,
    ) -> NativeGameplayResult<()> {
        if self.pending.is_some() || self.held.is_some() {
            return Ok(());
        }
        let request = match host.take_practice_request() {
            Ok(request) => request,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let Some(request) = request else {
            return Ok(());
        };
        let result = (|| -> NativeGameplayResult<PracticeRegion> {
            if self.ended || !self.started || request.generation != self.generation {
                return Err("practice request belongs to an unavailable generation".into());
            }
            let in_range = |t: Timestamp| t >= Timestamp::ZERO && t <= self.max_target;
            let region = match request.action {
                PracticeAction::Scrub { target }
                    if in_range(target)
                        && self.default_section_end.is_none_or(|end| target < end) =>
                {
                    PracticeRegion::new(
                        target,
                        self.default_section_end
                            .unwrap_or(Timestamp::from_nanos(i64::MAX)),
                        false,
                    )?
                }
                PracticeAction::Loop { start, end } if in_range(start) && in_range(end) => {
                    PracticeRegion::new(start, end, true)?
                }
                PracticeAction::DisableLoop => PracticeRegion {
                    repeat: false,
                    ..self.region
                },
                _ => return Err("practice target is outside the original song".into()),
            };
            let audio_generation = self.controller.generation();
            match request.action {
                PracticeAction::DisableLoop => self
                    .controller
                    .try_disable_loop_next(request.id, audio_generation)?,
                _ => self
                    .controller
                    .try_request_next(request.id, audio_generation, region)?,
            }
            Ok(region)
        })();
        match result {
            Ok(region) => self.pending = Some((request, region)),
            Err(error) => host.commit_practice_reply(&PracticeReply {
                id: request.id,
                generation: request.generation,
                result: Err(error.to_string()),
            })?,
        }
        Ok(())
    }

    /// Retains the first receipt; repeated polls cannot consume later laps.
    pub fn peek_receipt(&mut self) -> NativeGameplayResult<Option<PracticeReceipt>> {
        if self.runtime.is_some() {
            return Err("runtime practice requires the typed event consumer".into());
        }
        if self.held.is_none() {
            match self.controller.try_pop_projected_receipt() {
                Ok(projected) => {
                    if projected.boundary.source_frame != projected.receipt.physical_frame
                        || projected.target_rate == 0
                    {
                        return Err("practice source receipt and target projection differ".into());
                    }
                    self.validate(projected.receipt)?;
                    self.held = Some(projected.receipt);
                    self.held_projected = Some(projected);
                }
                Err(PracticeError::Empty) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(self.held)
    }

    pub fn qualify(
        &mut self,
        presentation: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> NativeGameplayResult<Option<PreparedPracticeBoundary>> {
        let Some(_) = self.peek_receipt()? else {
            return Ok(None);
        };
        presentation.prepare_projected_practice_boundary(
            self.held_projected
                .ok_or("practice receipt lacks its exact target projection")?,
            now,
        )
    }

    pub fn prepare_boundary<H: NativeGameplayHost>(
        &self,
        boundary: &PreparedPracticeBoundary,
        host: &mut H,
    ) -> NativeGameplayResult<PreparedPracticeCommit> {
        let receipt = self.held.ok_or("no retained practice receipt is held")?;
        let projected = self
            .held_projected
            .ok_or("practice receipt lacks its retained target projection")?;
        if boundary.receipt() != receipt || boundary.projected_receipt() != Some(projected) {
            return Err("practice native qualification differs from held receipt".into());
        }
        self.prepare_boundary_inner(receipt, projected, boundary.output().domain, host)
    }

    pub fn prepare_event_boundary<H: NativeGameplayHost>(
        &self,
        boundary: &PreparedPracticeEventBoundary,
        host: &mut H,
    ) -> NativeGameplayResult<PreparedPracticeCommit> {
        let event = self
            .held_event
            .ok_or("no typed practice boundary is held")?;
        if boundary.projected_event() != event || matches!(event.event, PracticeEvent::Rate(_)) {
            return Err("practice boundary proof differs from held typed event".into());
        }
        let receipt = self.held.ok_or("typed boundary lacks its actual receipt")?;
        let projected = self
            .held_projected
            .ok_or("typed boundary lacks its actual projection")?;
        self.prepare_boundary_inner(receipt, projected, boundary.output().domain, host)
    }

    fn prepare_boundary_inner<H: NativeGameplayHost>(
        &self,
        receipt: PracticeReceipt,
        projected: ProjectedPracticeReceipt,
        output_domain: beatkernel::time::ClockDomainId,
        host: &mut H,
    ) -> NativeGameplayResult<PreparedPracticeCommit> {
        self.validate(receipt)?;
        if let Some(runtime) = &self.runtime {
            runtime.validate_logical_domain(output_domain)?;
        }
        let restart = self.held_event.and_then(|event| match event.event {
            PracticeEvent::Restart(restart) => Some(restart),
            _ => None,
        });
        let region =
            if restart.is_some_and(|restart| restart.outcome == PracticeRateOutcome::Applied) {
                self.pending_transaction
                    .as_ref()
                    .ok_or("compound restart lost original region")?
                    .region
            } else if receipt.kind == PracticeBoundaryKind::Requested {
                self.pending
                    .ok_or("requested boundary lacks its original control")?
                    .1
            } else if receipt.kind == PracticeBoundaryKind::LoopDisabled {
                PracticeRegion {
                    repeat: false,
                    ..self.region
                }
            } else {
                self.region
            };
        let end_intent =
            if restart.is_some_and(|restart| restart.outcome == PracticeRateOutcome::Applied) {
                self.default_section_end
            } else if receipt.kind == PracticeBoundaryKind::Requested {
                match self
                    .pending
                    .ok_or("requested boundary lacks its original control")?
                    .0
                    .action
                {
                    PracticeAction::Scrub { .. } => self.default_section_end,
                    PracticeAction::Loop { end, .. } => Some(end),
                    PracticeAction::DisableLoop => {
                        return Err("invalid requested disable receipt".into());
                    }
                }
            } else {
                self.end_intent
            };
        let fresh = matches!(
            receipt.kind,
            PracticeBoundaryKind::Requested | PracticeBoundaryKind::Looped
        );
        let mut attempts = Vec::new();
        let mut next_launches = Vec::new();
        if fresh {
            attempts.try_reserve_exact(self.members.len())?;
            next_launches.try_reserve_exact(self.members.len())?;
            for member in &self.members {
                let config = PracticeAttemptConfig {
                    start: receipt.applied_song_time.max(Timestamp::ZERO),
                    end: end_intent,
                    domain: output_domain,
                    chart_seed: member.chart_seed,
                    capture_limits: member.capture_limits,
                };
                let attempt = if self.runtime.is_some() {
                    prepare_attempt_with_runtime_speed(
                        &self.original,
                        &member.policy,
                        &member.launch,
                        config,
                        if restart.is_some() {
                            self.speed()
                        } else {
                            self.current_speed()
                        },
                    )?
                } else if self.initial_setup.is_some() {
                    prepare_attempt_with_speed(
                        &self.original,
                        &member.policy,
                        &member.launch,
                        config,
                        self.speed(),
                    )?
                } else {
                    prepare_attempt(&self.original, &member.policy, &member.launch, config)?
                };
                next_launches.push(attempt.next_launch.clone());
                attempts.push((member.player, attempt));
            }
        }
        let presentation = if fresh {
            let borrowed: Vec<_> = attempts
                .iter()
                .map(|(id, attempt)| (*id, attempt))
                .collect();
            Some(host.prepare_practice_presentation(self.generation, &borrowed)?)
        } else {
            None
        };
        let reply = if self.runtime.is_some() || receipt.request_id == 0 {
            None
        } else {
            let request = self
                .pending
                .ok_or("prepared control lost its UI identity")?
                .0;
            Some(PracticeReply {
                id: request.id,
                generation: request.generation,
                result: if receipt.kind == PracticeBoundaryKind::ControlRejectedRevision {
                    Err("practice edit was superseded before the audio boundary".into())
                } else {
                    Ok(PracticeApplied {
                        generation: receipt.generation,
                        physical_frame: receipt.physical_frame,
                        playback_frame: receipt.playback_frame,
                        requested_target: receipt.requested_song_time,
                        applied_target: receipt.applied_song_time,
                    })
                },
            })
        };
        let serial = self
            .applied_serial
            .checked_add(1)
            .ok_or("practice application identity exhausted")?;
        let mut runtime_headers = Vec::new();
        let mut runtime_phase = None;
        let mut runtime_rate = None;
        let mut typed_reply = None;
        if let Some(runtime) = &self.runtime {
            let mut rate = runtime.rate;
            if let Some(restart) = restart {
                if restart.outcome == PracticeRateOutcome::Applied {
                    rate = PracticeRateState {
                        revision: restart.applied_revision,
                        rate: restart.applied_rate,
                    };
                }
            } else if receipt.request_id != 0
                && receipt.kind != PracticeBoundaryKind::ControlRejectedRevision
            {
                rate.revision = self
                    .pending_transaction
                    .as_ref()
                    .ok_or("typed region lost its request")?
                    .expected
                    .revision
                    .checked_add(1)
                    .ok_or("practice revision exhausted")?;
            }
            if fresh {
                runtime_headers.try_reserve_exact(attempts.len())?;
                for ((member, old), (player, attempt)) in
                    self.members.iter().zip(&runtime.headers).zip(&attempts)
                {
                    let header = if let Some(capture) = &attempt.capture {
                        capture.header().clone()
                    } else {
                        crate::replay_capture::setup_play_policy_header_with_speed(
                            &attempt.judge,
                            output_domain,
                            old.identity.limits,
                            attempt.config.start,
                            member.chart_seed,
                            end_intent,
                            beatkernel_bms::BmsInputMode::ButtonOnly,
                            crate::input_sounds::InputSoundIdentity::from_source(&attempt.source)?,
                            &member.policy,
                            attempt.speed(),
                        )?
                    };
                    runtime_headers.push(RuntimeHeader {
                        identity: PracticeRuntimeHeader {
                            player: *player,
                            header,
                            limits: old.identity.limits,
                        },
                        schedule: SourceRateSchedule::fixed(attempt.speed()),
                    });
                }
                runtime_phase = Some(PositiveRatePhase::new(
                    receipt.applied_song_time,
                    Duration::ZERO,
                    rate.rate,
                    end_intent,
                )?);
            }
            if receipt.request_id != 0 {
                let pending = self
                    .pending_transaction
                    .as_ref()
                    .ok_or("typed boundary lost request identity")?;
                typed_reply = Some(match pending.request {
                    PracticeTransactionRequest::RestartOriginal { id, generation, .. } => {
                        PracticeTransactionReply::RestartOriginal {
                            id,
                            generation,
                            result: Ok(restart.ok_or("original restart lacks compound event")?),
                        }
                    }
                    PracticeTransactionRequest::Region(request) => {
                        PracticeTransactionReply::Region {
                            reply: PracticeReply {
                                id: request.id,
                                generation: request.generation,
                                result: if receipt.kind
                                    == PracticeBoundaryKind::ControlRejectedRevision
                                {
                                    Err("practice edit was superseded before the audio boundary"
                                        .into())
                                } else {
                                    Ok(PracticeApplied {
                                        generation: receipt.generation,
                                        physical_frame: receipt.physical_frame,
                                        playback_frame: receipt.playback_frame,
                                        requested_target: receipt.requested_song_time,
                                        applied_target: receipt.applied_song_time,
                                    })
                                },
                            },
                            rate: Some(rate),
                        }
                    }
                    PracticeTransactionRequest::Rate { .. } => {
                        return Err("rate request received a region result".into());
                    }
                });
            }
            runtime_rate = Some(rate);
        }
        Ok(PreparedPracticeCommit {
            receipt,
            attempts,
            presentation,
            next_launches,
            region,
            end_intent,
            projected,
            reply,
            serial,
            owner_id: self.owner_id,
            typed: self.held_event,
            typed_reply,
            runtime_headers,
            runtime_phase,
            runtime_rate,
        })
    }

    /// Call only after the native cut and every live owner have committed.
    /// This commits only actual owners; no observer is acknowledged here.
    /// Validate all fallible invariants before the first move into live state.
    pub fn apply_boundary(
        &mut self,
        prepared: &mut PreparedPracticeCommit,
    ) -> NativeGameplayResult<AppliedPracticeBoundary> {
        if prepared.owner_id != self.owner_id
            || self.held != Some(prepared.receipt)
            || self.held_projected != Some(prepared.projected)
            || self.applied_serial.checked_add(1) != Some(prepared.serial)
            || prepared.typed != self.held_event
        {
            return Err("stale practice commit payload".into());
        }
        self.validate(prepared.receipt)?;
        let receipt = prepared.receipt;
        if prepared.typed.is_none() {
            match (&prepared.reply, receipt.request_id) {
                (None, 0) => {}
                (Some(reply), id)
                    if id != 0
                        && self.pending.is_some_and(|(request, _)| {
                            request.id == id
                                && reply.id == id
                                && reply.generation == request.generation
                        }) => {}
                _ => return Err("prepared control lost its original UI identity".into()),
            }
        } else if receipt.request_id != 0 && prepared.typed_reply.is_none() {
            return Err("typed practice boundary lost its reply identity".into());
        }
        if matches!(
            receipt.kind,
            PracticeBoundaryKind::Requested | PracticeBoundaryKind::Looped
        ) {
            if prepared.next_launches.len() != self.members.len()
                || self
                    .members
                    .iter()
                    .zip(&prepared.next_launches)
                    .any(|(member, launch)| {
                        member.launch.attempt().checked_add(1) != Some(launch.attempt())
                    })
            {
                return Err("practice launch cohort differs".into());
            }
        } else if !prepared.next_launches.is_empty() {
            return Err("nonfresh practice boundary contains new launches".into());
        }
        if let Some(runtime) = &self.runtime {
            let fresh = matches!(
                receipt.kind,
                PracticeBoundaryKind::Requested | PracticeBoundaryKind::Looped
            );
            if prepared.runtime_rate.is_none()
                || (fresh
                    && (prepared.runtime_headers.len() != runtime.headers.len()
                        || prepared.runtime_phase.is_none()))
                || (!fresh
                    && (!prepared.runtime_headers.is_empty() || prepared.runtime_phase.is_some()))
            {
                return Err("typed practice boundary cohort differs".into());
            }
        }
        for (member, launch) in self
            .members
            .iter_mut()
            .zip(prepared.next_launches.drain(..))
        {
            member.launch = launch;
        }
        self.region = prepared.region;
        self.end_intent = prepared.end_intent;
        self.generation = receipt.generation;
        self.iteration = receipt.iteration;
        self.started = true;
        if receipt.kind == PracticeBoundaryKind::Ended {
            self.ended = true;
        } else if matches!(
            receipt.kind,
            PracticeBoundaryKind::Started
                | PracticeBoundaryKind::Requested
                | PracticeBoundaryKind::Looped
        ) {
            self.ended = false;
        }
        self.last_frames = Some((receipt.physical_frame, receipt.playback_frame));
        self.held = None;
        self.held_projected = None;
        self.applied_serial = prepared.serial;
        self.held_event = None;
        if let Some(runtime) = &mut self.runtime {
            runtime.rate = prepared.runtime_rate.expect("validated runtime rate");
            if let Some(phase) = prepared.runtime_phase.take() {
                runtime.phase = phase;
                runtime.headers = std::mem::take(&mut prepared.runtime_headers);
            }
        }
        if receipt.request_id != 0 {
            self.pending = None;
            self.pending_transaction = None;
        }
        Ok(AppliedPracticeBoundary {
            receipt,
            reply: prepared.reply.take(),
            serial: prepared.serial,
            owner_id: self.owner_id,
            typed_reply: prepared.typed_reply.take(),
        })
    }

    /// Publish only after actual application and successful visual publication.
    /// An observer refusal cannot change the committed recording or retry path.
    pub fn publish_boundary<H: NativeGameplayHost>(
        &self,
        applied: &AppliedPracticeBoundary,
        host: &mut H,
    ) -> NativeGameplayResult<()> {
        if applied.owner_id != self.owner_id
            || applied.serial != self.applied_serial
            || applied.receipt.generation != self.generation
            || applied.receipt.iteration != self.iteration
            || self.last_frames
                != Some((
                    applied.receipt.physical_frame,
                    applied.receipt.playback_frame,
                ))
        {
            return Err("stale applied practice publication".into());
        }
        if let Some(reply) = &applied.reply {
            host.commit_practice_reply(reply)?;
        }
        if let Some(reply) = &applied.typed_reply {
            host.commit_practice_transaction_reply(reply)?;
        }
        if self.runtime.is_some() {
            self.advertise_transactions(host)
        } else {
            self.advertise(host)
        }
    }

    fn validate(&self, receipt: PracticeReceipt) -> NativeGameplayResult<()> {
        if let Some(PracticeEvent::Restart(restart)) = self.held_event.map(|event| event.event) {
            let runtime = self
                .runtime
                .as_ref()
                .ok_or("compound restart lacks runtime owner")?;
            let pending = self
                .pending_transaction
                .as_ref()
                .ok_or("compound restart lacks correlated request")?;
            let PracticeTransactionRequest::RestartOriginal {
                id,
                generation,
                expected,
            } = pending.request
            else {
                return Err("compound restart belongs to another transaction".into());
            };
            if receipt != restart.boundary
                || receipt.request_id != id
                || expected != pending.expected
                || restart.request_revision != expected.revision
                || restart.previous_rate != runtime.rate.rate
                || restart.requested_rate != self.speed().rate()
                || receipt.playback_frame > receipt.physical_frame
                || receipt.physical_frame < self.basis.start_physical_frame()
                || self
                    .last_frames
                    .is_some_and(|(p, b)| receipt.physical_frame < p || receipt.playback_frame < b)
            {
                return Err(
                    "compound restart differs from original request or actual frame".into(),
                );
            }
            let valid = match restart.outcome {
                PracticeRateOutcome::Applied => {
                    generation == self.generation
                        && expected == runtime.rate
                        && self.generation.checked_add(1) == Some(receipt.generation)
                        && receipt.iteration == 0
                        && receipt.kind == PracticeBoundaryKind::Requested
                        && receipt.requested_song_time == pending.region.start
                        && receipt.applied_song_time == pending.region.start
                        && receipt.correction_nanos == 0
                        && restart.applied_rate == self.speed().rate()
                        && expected.revision.checked_add(1) == Some(restart.applied_revision)
                }
                PracticeRateOutcome::Refused(reason) => {
                    receipt.generation == self.generation
                        && receipt.iteration == self.iteration
                        && receipt.kind == PracticeBoundaryKind::ControlRejectedRevision
                        && receipt.requested_song_time == receipt.applied_song_time
                        && receipt.correction_nanos == 0
                        && restart.applied_rate == runtime.rate.rate
                        && restart.applied_revision == runtime.rate.revision
                        && (generation == self.generation
                            || reason == PracticeRateRefusal::StaleGeneration)
                }
                PracticeRateOutcome::Noop => false,
            };
            return if valid {
                Ok(())
            } else {
                Err("compound restart outcome violates retained state".into())
            };
        }
        if (self.ended
            && !matches!(
                receipt.kind,
                PracticeBoundaryKind::Requested
                    | PracticeBoundaryKind::LoopDisabled
                    | PracticeBoundaryKind::ControlRejectedRevision
            ))
            || receipt.physical_frame < self.basis.start_physical_frame()
            || receipt.playback_frame > receipt.physical_frame
            || (if receipt.kind == PracticeBoundaryKind::ControlRejectedRevision
                || (receipt.kind == PracticeBoundaryKind::Started && self.initial_setup.is_some())
            {
                i128::from(receipt.applied_song_time.as_nanos())
                    - i128::from(receipt.requested_song_time.as_nanos())
                    != i128::from(receipt.correction_nanos)
            } else {
                receipt.correction_nanos != 0
                    || receipt.requested_song_time != receipt.applied_song_time
            })
            || self
                .last_frames
                .is_some_and(|(p, b)| receipt.physical_frame < p || receipt.playback_frame < b)
        {
            return Err("invalid retained practice frame or original anchor".into());
        }
        let next = self.generation.checked_add(1);
        let next_iteration = self.iteration.checked_add(1);
        let valid = match receipt.kind {
            PracticeBoundaryKind::Started => {
                !self.started
                    && receipt.request_id == 0
                    && receipt.generation == 1
                    && receipt.iteration == 0
                    && receipt.requested_song_time == self.region.start
                    && receipt.applied_song_time
                        == self
                            .initial_setup
                            .map_or(self.region.start, |setup| setup.song_origin())
            }
            PracticeBoundaryKind::Requested => {
                self.started
                    && next == Some(receipt.generation)
                    && receipt.iteration == 0
                    && self.pending.is_some_and(|(r, region)| {
                        r.id == receipt.request_id
                            && r.action != PracticeAction::DisableLoop
                            && r.generation <= self.generation
                            && region.start == receipt.applied_song_time
                    })
            }
            PracticeBoundaryKind::Looped => {
                self.started
                    && self.region.repeat
                    && receipt.request_id == 0
                    && next == Some(receipt.generation)
                    && next_iteration == Some(receipt.iteration)
                    && receipt.applied_song_time == self.region.start
            }
            PracticeBoundaryKind::Ended => {
                self.started
                    && !self.region.repeat
                    && receipt.request_id == 0
                    && next == Some(receipt.generation)
                    && next_iteration == Some(receipt.iteration)
                    && receipt.applied_song_time == self.region.end
            }
            PracticeBoundaryKind::LoopDisabled => {
                self.started
                    && receipt.generation == self.generation
                    && receipt.iteration == self.iteration
                    && receipt.applied_song_time
                        == if self.ended {
                            self.region.end
                        } else {
                            self.region.start
                        }
                    && self.pending.is_some_and(|(r, _)| {
                        r.id == receipt.request_id
                            && r.generation <= self.generation
                            && r.action == PracticeAction::DisableLoop
                    })
            }
            PracticeBoundaryKind::ControlRejectedRevision => {
                self.started
                    && receipt.generation == self.generation
                    && receipt.iteration == self.iteration
                    && receipt.applied_song_time
                        == if self.ended {
                            self.region.end
                        } else {
                            self.region.start
                        }
                    && self.pending.is_some_and(|(r, _)| {
                        r.id == receipt.request_id && r.generation <= self.generation
                    })
            }
        };
        if !valid {
            return Err("practice receipt violates ordered generation/control ownership".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "practice_playback_retention_fixtures.rs"]
mod practice_playback_retention_fixtures;
