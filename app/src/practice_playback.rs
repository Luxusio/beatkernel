//! Control-thread ownership of ordered retained-output practice transitions.
//! Render receipts are held one at a time until the actual native input cut and
//! every attempt owner have committed. This module never opens an endpoint.
use crate::{
    local_players::PlayerId,
    native_audio_presentation::{NativeAudioPresentation, PreparedPracticeBoundary},
    native_gameplay::NativeGameplayResult,
    native_gameplay_host::NativeGameplayHost,
    play_policy::ResolvedPlayPolicy,
    player::PreparedPracticePresentation,
    practice_control::{PracticeAction, PracticeApplied, PracticeCapability, PracticeReply},
    practice_session::{PracticeAttemptConfig, PreparedPracticeAttempt, prepare_attempt},
    replay_capture::LiveReplayCapture,
    session_launch::SessionLaunch,
};
use beatkernel::{
    audio::{
        OutputFrameBasis, PracticeBoundaryKind, PracticeController, PracticeError, PracticeReceipt,
        PracticeRegion, ProjectedPracticeReceipt,
    },
    replay::codec::ReplayCodecLimits,
    time::{ClockPoint, Timestamp},
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
        assert!(
            owner
                .validate(PracticeReceipt {
                    generation: 2,
                    ..first
                })
                .is_err()
        );
        assert!(
            owner
                .validate(PracticeReceipt {
                    applied_song_time: ts(1),
                    ..first
                })
                .is_err()
        );
        assert!(
            owner
                .validate(PracticeReceipt {
                    playback_frame: 1,
                    ..first
                })
                .is_err()
        );
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
        assert!(
            owner
                .validate(PracticeReceipt {
                    request_id: 12,
                    ..refused
                })
                .is_err()
        );
        assert!(
            owner
                .validate(PracticeReceipt {
                    correction_nanos: 0,
                    ..refused
                })
                .is_err()
        );
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
        assert!(
            PracticePlayback::validate_setup(
                &owner.members,
                owner.region,
                owner.max_target,
                owner.default_section_end
            )
            .is_err()
        );
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
        assert!(
            PracticePlayback::new(
                owner.controller,
                owner.original,
                owner.members,
                owner.region,
                owner.max_target,
                owner.basis,
                1
            )
            .is_err()
        );
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
        assert!(
            validate(
                PracticeRegion::new(ts(-2), ts(-1), false).unwrap(),
                Some(ts(-1))
            )
            .is_err()
        );
        let ceiling = Timestamp::from_nanos(i64::MAX);
        assert!(validate(PracticeRegion::new(ts(0), ceiling, true).unwrap(), None).is_err());
        assert!(validate(PracticeRegion::new(ts(21), ceiling, false).unwrap(), None).is_err());
        assert!(validate(PracticeRegion::new(ts(-2), ceiling, false).unwrap(), None).is_ok());
        assert!(
            validate(
                PracticeRegion::new(ts(-2), ts(4), false).unwrap(),
                Some(ts(4))
            )
            .is_ok()
        );
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
        assert!(
            owner.members()[0]
                .launch
                .args()
                .windows(2)
                .any(|pair| pair == ["--record-replay", "records/base.retry1.bkr"])
        );
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
}

/// Proof that the retained attempt identity has changed. Observer ports cannot
/// manufacture this token or roll the committed owner back after refusal.
pub struct AppliedPracticeBoundary {
    receipt: PracticeReceipt,
    reply: Option<PracticeReply>,
    serial: u64,
    owner_id: u64,
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
}

impl PracticePlayback {
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
        })
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
        self.ended && self.pending.is_none() && self.held.is_none()
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
        self.validate(receipt)?;
        let region = if receipt.kind == PracticeBoundaryKind::Requested {
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
        let end_intent = if receipt.kind == PracticeBoundaryKind::Requested {
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
                let attempt = prepare_attempt(
                    &self.original,
                    &member.policy,
                    &member.launch,
                    PracticeAttemptConfig {
                        start: receipt.applied_song_time.max(Timestamp::ZERO),
                        end: end_intent,
                        domain: boundary.output().domain,
                        chart_seed: member.chart_seed,
                        capture_limits: member.capture_limits,
                    },
                )?;
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
        let reply = if receipt.request_id == 0 {
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
        {
            return Err("stale practice commit payload".into());
        }
        self.validate(prepared.receipt)?;
        let receipt = prepared.receipt;
        match (&prepared.reply, receipt.request_id) {
            (None, 0) => {}
            (Some(reply), id)
                if id != 0
                    && self.pending.is_some_and(|(request, _)| {
                        request.id == id && reply.id == id && reply.generation == request.generation
                    }) => {}
            _ => return Err("prepared control lost its original UI identity".into()),
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
        if receipt.request_id != 0 {
            self.pending = None;
        }
        Ok(AppliedPracticeBoundary {
            receipt,
            reply: prepared.reply.take(),
            serial: prepared.serial,
            owner_id: self.owner_id,
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
        self.advertise(host)
    }

    fn validate(&self, receipt: PracticeReceipt) -> NativeGameplayResult<()> {
        if (self.ended
            && !matches!(
                receipt.kind,
                PracticeBoundaryKind::Requested
                    | PracticeBoundaryKind::LoopDisabled
                    | PracticeBoundaryKind::ControlRejectedRevision
            ))
            || receipt.physical_frame < self.basis.start_physical_frame()
            || receipt.playback_frame > receipt.physical_frame
            || (if receipt.kind == PracticeBoundaryKind::ControlRejectedRevision {
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
                    && receipt.applied_song_time == self.region.start
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
