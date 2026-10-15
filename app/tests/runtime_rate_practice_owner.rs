//! One actual PCM cut qualified by native evidence commits the common business owner.
//! Native supplied pairs are DI fixtures, not physical device/latency acceptance.
use beatkernel::{
    audio::*,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::JudgeEngine,
    replay::{
        codec::{encode_replay, ReplayCodecLimits, ReplayFile},
        ReplayHeader,
    },
    runtime::{Runtime, RuntimeReport},
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{OutputPhaseBasis, OutputPhasePoint, Rate, Transport},
};
use beatkernel_bms::{BmsChart, BmsInputMode};
use beatkernel_bms_runtime::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
    },
    live_speed::LiveSpeedSetup,
    local_input::InputMerger,
    local_players::PlayerId,
    local_runtime::PlayerReport,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_gameplay::NativeGameplayResult,
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
    play_policy::ResolvedPlayPolicy,
    practice_control::{
        PracticeAction, PracticeRateState, PracticeRequest as UiPracticeRequest,
        PracticeTransactionCapability, PracticeTransactionReply, PracticeTransactionRequest,
    },
    practice_playback::{PracticeMember, PracticePlayback, PracticeRuntimeHeader},
    replay_capture::LiveReplayCapture,
    replay_speed::ReplaySpeed,
    replay_speed_policy::split_schedule,
    session_launch::SessionLaunch,
};
use beatkernel_platform::audio::presentation::validation::{
    NativePresentationValidator, OriginalNativePresentationEvidence,
};

const RAW: ClockDomainId = ClockDomainId(2);
const LOGICAL: ClockDomainId = ClockDomainId(3);
const HOST: ClockDomainId = ClockDomainId(1);
const BASE: i64 = 5_000_000_000;
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: ClockDomainId, n: i64) -> ClockPoint {
    ClockPoint {
        domain,
        timestamp: ts(n),
    }
}
fn speed(s: &str) -> ReplaySpeed {
    s.parse().unwrap()
}
fn limits() -> ReplayCodecLimits {
    beatkernel_bms_runtime::competition_live::replay_limits().unwrap()
}
fn source() -> BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#WAV01 key.wav\n#00011:01\n#00111:01\n",
        Default::default(),
    )
    .unwrap()
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn original_hit() -> RuntimeReport {
    let original = source();
    let policy = ResolvedPlayPolicy::builtin(1_000_000, 2_000_000, 0).unwrap();
    let judge = JudgeEngine::new(
        original.compile().unwrap().chart,
        original.rules(),
        policy.judge().clone(),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (producer, _consumer) = command_queue(16).unwrap();
    let mut runtime = Runtime::new(
        LOGICAL,
        RAW,
        Transport::new(ts(BASE), ts(0), Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    let mut meta = EventMeta::new(DeviceId(9), point(LOGICAL, BASE), 91);
    meta.original_clock_point = Some(point(HOST, 100));
    let report = runtime
        .process_input(
            PhysicalInputEvent::Button(ButtonEvent {
                meta,
                control: PhysicalControlId::keyboard(4),
                state: ButtonState::Down,
            }),
            &Identity,
            point(RAW, 0),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(0));
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(
        report.input.as_ref().unwrap().meta().original_clock_point,
        Some(point(HOST, 100))
    );
    report
}

#[derive(Default)]
struct Host {
    capabilities: Vec<Option<PracticeTransactionCapability>>,
    replies: Vec<PracticeTransactionReply>,
    pending: Option<PracticeTransactionRequest>,
    identities: usize,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, _: PauseState) {}
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn publish_report(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn publish_local_reports(&mut self, _: &[PlayerReport]) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn advertise_practice_transactions(
        &mut self,
        value: Option<PracticeTransactionCapability>,
    ) -> NativeGameplayResult<()> {
        self.capabilities.push(value);
        Ok(())
    }
    fn take_practice_transaction(
        &mut self,
        active: bool,
    ) -> NativeGameplayResult<Option<PracticeTransactionRequest>> {
        Ok(if active { self.pending.take() } else { None })
    }
    fn commit_practice_transaction_reply(
        &mut self,
        reply: &PracticeTransactionReply,
    ) -> NativeGameplayResult<()> {
        self.replies.push(reply.clone());
        Ok(())
    }
    fn apply_practice_identity(
        &mut self,
        _: &mut beatkernel_bms_runtime::player::PreparedPracticePresentation,
        _: u64,
    ) {
        self.identities += 1;
    }
    fn prepare_practice_presentation(
        &mut self,
        generation: u64,
        attempts: &[(
            PlayerId,
            &beatkernel_bms_runtime::practice_session::PreparedPracticeAttempt,
        )],
    ) -> NativeGameplayResult<beatkernel_bms_runtime::player::PreparedPracticePresentation> {
        beatkernel_bms_runtime::player::prepare_practice_presentation(generation, attempts)
    }
}

struct Rig {
    owner: PracticePlayback,
    mixer: Mixer,
    producer: CommandProducer,
    presentation: NativeAudioPresentation,
    inputs: InputMerger,
    transport: Transport,
    phase: OutputPhaseBasis,
    captures: Vec<(PlayerId, Option<LiveReplayCapture>)>,
    host: Host,
}
fn rig(recorders: &[bool], end: i64) -> Rig {
    rig_headers(recorders, end, |_| {}).unwrap()
}
fn rig_headers(
    recorders: &[bool],
    end: i64,
    modify: impl FnOnce(&mut Vec<PracticeRuntimeHeader>),
) -> NativeGameplayResult<Rig> {
    rig_headers_limits(recorders, end, None, modify)
}
fn rig_headers_limits(
    recorders: &[bool],
    end: i64,
    bounded_member: Option<(usize, ReplayCodecLimits)>,
    modify: impl FnOnce(&mut Vec<PracticeRuntimeHeader>),
) -> NativeGameplayResult<Rig> {
    let original = source();
    let initial = speed("1/2");
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm = PcmLimits::new(128, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, (0..32).map(|i| i as f32 / 32.0).collect(), pcm).unwrap(),
    )
    .unwrap();
    let program = PreparedPracticeProgram::new_with_runtime_rates(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: ts(0),
            gain: 1.0,
        }],
        PracticeLimits::new(8, 1, 128, 8, 32).unwrap(),
        initial.rate(),
        Duration::ZERO,
        false,
        14045,
    )
    .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let (producer, consumer) = command_queue(32).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            RAW,
            ts(0),
            AudioLimits::new(32, 8, 32, 256, 32).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let region = PracticeRegion::new(ts(0), ts(end), false).unwrap();
    mixer.install_practice(program, endpoint, region).unwrap();
    let creation = mixer.output_frame_basis();
    let mut members = Vec::new();
    let mut headers = Vec::new();
    let mut captures = Vec::new();
    for (index, &record) in recorders.iter().enumerate() {
        let player = PlayerId((index as u32 + 1) * 7);
        let codec = bounded_member
            .filter(|(member, _)| *member == index)
            .map_or_else(limits, |(_, limits)| limits);
        let policy = ResolvedPlayPolicy::builtin(1_000_000, 2_000_000, 0).unwrap();
        let judge = JudgeEngine::new(
            original.compile().unwrap().chart,
            original.rules(),
            policy.judge().clone(),
        )
        .unwrap();
        let capture = LiveReplayCapture::new_with_policy_and_speed(
            &judge,
            LOGICAL,
            codec,
            ts(0),
            0,
            Some(ts(end)),
            BmsInputMode::ButtonOnly,
            None,
            &policy,
            initial,
        )
        .unwrap();
        let mut arguments = vec![
            "--chart".into(),
            "chart.bms".into(),
            "--speed".into(),
            "1/2".into(),
            "--start-ns".into(),
            "0".into(),
            "--end-ns".into(),
            end.to_string(),
        ];
        if record {
            arguments.extend([
                "--record-replay".into(),
                format!("records/p{}.bkr", player.0),
            ]);
        }
        headers.push(PracticeRuntimeHeader {
            player,
            header: capture.header().clone(),
            limits: codec,
        });
        captures.push((player, record.then_some(capture)));
        members.push(PracticeMember {
            player,
            policy,
            launch: SessionLaunch::new(arguments).unwrap(),
            chart_seed: 0,
            capture_limits: record.then_some(codec),
        });
    }
    modify(&mut headers);
    let setup = LiveSpeedSetup::new(ts(0), Duration::ZERO, initial, Some(ts(end))).unwrap();
    let owner = PracticePlayback::new_with_runtime_rates(
        controller,
        original,
        members,
        region,
        ts(20_000_000),
        creation,
        Some(ts(end)),
        setup,
        headers,
    )?;
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: creation.point_at_stream_frame(0).unwrap(),
        logical_origin: point(LOGICAL, BASE),
        host_domain: HOST,
    };
    let presentation = NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig {
                history_capacity: 16,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                ..Default::default()
            },
            epoch,
            AudioAuthorityCoordinateBasis::SourceFrame(creation),
        )
        .unwrap(),
        NativePresentationValidator::new(7, epoch.stream_origin, HOST),
    )
    .unwrap();
    let phase = OutputPhaseBasis::new(LOGICAL, 14045, 1, 0, 1000).unwrap();
    let transport = Transport::new_with_runtime_output_phase(
        phase,
        point(LOGICAL, BASE),
        ts(0),
        Duration::ZERO,
        initial.rate(),
        Some(ts(end)),
    )
    .unwrap();
    Ok(Rig {
        owner,
        mixer,
        producer,
        presentation,
        inputs: InputMerger::new(HOST, point(HOST, 0), vec![DeviceId(9)], 16).unwrap(),
        transport,
        phase,
        captures,
        host: Host::default(),
    })
}
impl Rig {
    fn observe(&mut self, frame: u64) {
        let basis = self.owner.mixer_basis();
        self.presentation
            .admit_coordinate(
                CoordinateNativeAudioSnapshot::new(
                    NativeAudioSnapshot {
                        epoch: 7,
                        basis,
                        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                            source: basis.point_at_stream_frame(frame).unwrap(),
                            target: point(HOST, 100 + frame as i64 * 1_000_000),
                        }),
                    },
                    NativeOutputCoordinate::StreamFrame { frame },
                )
                .unwrap(),
            )
            .unwrap();
    }
    fn now(&self) -> ClockPoint {
        point(HOST, 100 + self.mixer.frame_cursor() as i64 * 1_000_000)
    }
    fn start(&mut self) {
        self.observe(0);
        self.presentation
            .pin_practice_mixer_basis(self.owner.mixer_basis())
            .unwrap();
        let mut pcm = [99.0; 3];
        self.mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.0, 0.015625, 0.03125]);
        self.observe(3);
        let now = self.now();
        let cut = self
            .owner
            .qualify_event(&self.presentation, now)
            .unwrap()
            .unwrap();
        assert!(matches!(
            cut.projected_event().event,
            PracticeEvent::Boundary(PracticeReceipt {
                kind: PracticeBoundaryKind::Started,
                ..
            })
        ));
        let mut prepared = self
            .owner
            .prepare_event_boundary(&cut, &mut self.host)
            .unwrap();
        assert!(prepared.attempts.is_empty());
        self.presentation
            .authority_mut()
            .record_acquired_prefix(now)
            .unwrap();
        assert!(self
            .presentation
            .practice_event_boundary_ready(&cut, now, &self.inputs)
            .unwrap());
        self.presentation
            .commit_practice_event_boundary(cut, now, &self.inputs)
            .unwrap();
        self.owner.apply_boundary(&mut prepared).unwrap();
        assert!(self.owner.peek_event().unwrap().is_none());
    }
    fn request(&self, id: u64, selected: &str) -> PracticeTransactionRequest {
        PracticeTransactionRequest::Rate {
            id,
            generation: self.owner.generation(),
            expected: self.owner.rate_state().unwrap(),
            requested_rate: speed(selected).rate(),
        }
    }
    fn admit(
        &mut self,
        request: PracticeTransactionRequest,
        competition: bool,
    ) -> NativeGameplayResult<()> {
        let mut captures: Vec<_> = self
            .captures
            .iter_mut()
            .map(|(id, capture)| (*id, capture))
            .collect();
        self.owner.admit_transaction(
            request,
            true,
            &mut self.transport,
            &mut captures,
            competition,
        )
    }
    fn render(&mut self, count: usize) -> Vec<f32> {
        let mut out = vec![99.0; count];
        self.mixer.render(&mut out).unwrap();
        self.observe(self.mixer.frame_cursor());
        out
    }
    fn commit_rate(&mut self) -> PracticeRateEvent {
        let now = self.now();
        let cut = self
            .owner
            .qualify_event(&self.presentation, now)
            .unwrap()
            .unwrap();
        let event = match cut.projected_event().event {
            PracticeEvent::Rate(e) => e,
            event => panic!("expected Rate, got {event:?}"),
        };
        self.presentation
            .authority_mut()
            .record_acquired_prefix(now)
            .unwrap();
        assert!(self
            .presentation
            .practice_event_boundary_ready(&cut, now, &self.inputs)
            .unwrap());
        let phase = self
            .phase
            .point_from_elapsed_nanos(cut.output(), event.active_elapsed_nanos)
            .unwrap();
        let mut captures: Vec<_> = self
            .captures
            .iter_mut()
            .map(|(id, capture)| (*id, capture))
            .collect();
        let prepared = self
            .owner
            .prepare_rate_event(&cut, phase, &mut self.transport, &mut captures)
            .unwrap();
        self.presentation
            .commit_practice_event_boundary(cut, now, &self.inputs)
            .unwrap();
        let applied = self.owner.apply_rate_event(prepared).unwrap();
        assert_eq!(applied.event(), event);
        self.owner
            .publish_rate_event(&applied, &mut self.host)
            .unwrap();
        event
    }
    fn headers(&self) -> Vec<ReplayHeader> {
        self.captures
            .iter()
            .map(|(id, _)| self.owner.effective_header(*id).unwrap().clone())
            .collect()
    }
    fn capture_bytes(&self) -> Vec<Option<Vec<u8>>> {
        self.captures
            .iter()
            .map(|(_, capture)| {
                capture.as_ref().map(|c| {
                    encode_replay(
                        &ReplayFile::new(c.header().clone(), c.records().to_vec()),
                        limits(),
                    )
                    .unwrap()
                })
            })
            .collect()
    }
    fn at(&self, physical: u64, elapsed: i128) -> OutputPhasePoint {
        self.phase
            .point_from_elapsed_nanos(point(LOGICAL, BASE + physical as i64 * 1_000_000), elapsed)
            .unwrap()
    }
    fn commit_fresh_boundary(&mut self, expected_speed: ReplaySpeed) -> PracticeReceipt {
        let now = self.now();
        let cut = self
            .owner
            .qualify_event(&self.presentation, now)
            .unwrap()
            .unwrap();
        let projected = cut.projected_event();
        let boundary = match projected.event {
            PracticeEvent::Boundary(boundary) => boundary,
            PracticeEvent::Restart(restart) => {
                assert_eq!(restart.outcome, PracticeRateOutcome::Applied);
                assert_eq!(restart.applied_rate, expected_speed.rate());
                restart.boundary
            }
            other => panic!("expected actual fresh boundary, got {other:?}"),
        };
        let mut prepared = self
            .owner
            .prepare_event_boundary(&cut, &mut self.host)
            .unwrap();
        assert_eq!(prepared.attempts.len(), self.captures.len());
        let next_phase = OutputPhaseBasis::new(
            LOGICAL,
            14045,
            boundary.generation,
            boundary.playback_frame,
            1000,
        )
        .unwrap();
        let next_transport = prepared.attempts[0]
            .1
            .transport_at_runtime_output_phase(next_phase, cut.output())
            .unwrap();
        let mut next_captures = Vec::new();
        for (player, attempt) in prepared.attempts.drain(..) {
            assert_eq!(attempt.speed(), expected_speed);
            assert_eq!(attempt.config.start, boundary.applied_song_time);
            assert_eq!(attempt.score, Default::default());
            if let Some(capture) = &attempt.capture {
                assert_eq!(
                    split_schedule(&capture.header().options)
                        .unwrap()
                        .1
                        .current_speed(),
                    expected_speed
                );
            }
            next_captures.push((player, attempt.capture));
        }
        self.presentation
            .authority_mut()
            .record_acquired_prefix(now)
            .unwrap();
        assert!(self
            .presentation
            .practice_event_boundary_ready(&cut, now, &self.inputs)
            .unwrap());
        self.presentation
            .commit_practice_event_boundary(cut, now, &self.inputs)
            .unwrap();
        self.transport = next_transport;
        self.phase = next_phase;
        self.captures = next_captures;
        let applied = self.owner.apply_boundary(&mut prepared).unwrap();
        assert_eq!(applied.receipt(), boundary);
        self.owner
            .publish_boundary(&applied, &mut self.host)
            .unwrap();
        boundary
    }
}

#[test]
fn paused_admission_uses_later_actual_cut_and_all_members_keep_one_attempt() {
    let mut rig = rig(&[true, false, true], 20_000_000);
    rig.start();
    let report = original_hit();
    for (_, capture) in &mut rig.captures {
        if let Some(capture) = capture {
            capture.record_report(&report).unwrap();
        }
    }
    let original_records: Vec<_> = rig
        .captures
        .iter()
        .map(|(_, capture)| capture.as_ref().map(|capture| capture.records().to_vec()))
        .collect();
    let old_headers = rig.headers();
    let old_bytes = rig.capture_bytes();
    let launches: Vec<_> = rig
        .owner
        .members()
        .iter()
        .map(|m| m.launch.args().to_vec())
        .collect();
    let request = rig.request(11, "2/1");
    rig.admit(request, false).unwrap();
    assert!(rig.owner.transition_pending());
    assert_eq!(rig.owner.current_speed(), speed("1/2"));
    assert_eq!(
        rig.owner.rate_state(),
        Some(PracticeRateState {
            revision: 0,
            rate: speed("1/2").rate()
        })
    );
    assert_eq!(rig.headers(), old_headers);
    assert_eq!(rig.capture_bytes(), old_bytes);
    rig.producer.request_pause(true);
    assert_eq!(rig.render(5), [0.0; 5]);
    assert!(rig.owner.peek_event().unwrap().is_none());
    assert_eq!(rig.headers(), old_headers);
    rig.producer.request_pause(false);
    assert_eq!(rig.render(2), [0.046875, 0.109375]);
    let actual = rig.owner.peek_event().unwrap().unwrap();
    assert_eq!(
        rig.owner.peek_event().unwrap(),
        Some(actual),
        "event remains latched"
    );
    assert_eq!(
        rig.headers(),
        old_headers,
        "rendered is not yet business committed"
    );
    let applied = rig.commit_rate();
    assert_eq!(
        (
            applied.physical_frame,
            applied.playback_frame,
            applied.active_elapsed_nanos
        ),
        (8, 3, 3_000_000)
    );
    assert_eq!(applied.outcome, PracticeRateOutcome::Applied);
    assert_eq!(rig.owner.generation(), 1);
    assert_eq!(rig.owner.current_speed(), speed("2/1"));
    assert_eq!(rig.owner.speed(), speed("1/2"));
    assert_eq!(rig.owner.rate_state().unwrap().revision, 1);
    assert_eq!(
        rig.host.identities, 0,
        "rate cannot reset score/attempt identity"
    );
    for (((id, capture), old), records) in
        rig.captures.iter().zip(old_headers).zip(original_records)
    {
        let effective = rig.owner.effective_header(*id).unwrap();
        assert_ne!(effective, &old);
        let (_, schedule) = split_schedule(&effective.options).unwrap();
        assert_eq!(schedule.initial_speed(), speed("1/2"));
        assert_eq!(schedule.current_speed(), speed("2/1"));
        assert_eq!(schedule.cuts().len(), 1);
        let cut = schedule.cuts()[0];
        assert_eq!(
            (cut.whole(), cut.numerator(), cut.denominator()),
            (1_500_000, 0, 1)
        );
        if let Some(capture) = capture {
            assert_eq!(capture.header(), effective);
            assert_eq!(capture.records(), records.as_ref().unwrap());
            assert_eq!(capture.records().len(), 1);
        }
    }
    for (member, old) in rig.owner.members().iter().zip(launches) {
        assert_eq!(member.launch.args(), old);
        assert_eq!(member.launch.attempt(), 0);
    }
    assert_eq!(
        rig.transport
            .position_at_output_phase(rig.at(2, 2_000_000))
            .unwrap(),
        ts(1_000_000)
    );
    assert_eq!(
        rig.transport
            .position_at_output_phase(rig.at(8, 3_000_000))
            .unwrap(),
        ts(1_500_000)
    );
    assert_eq!(
        rig.transport
            .position_at_output_phase(rig.at(9, 4_000_000))
            .unwrap(),
        ts(3_500_000)
    );
    assert!(
        matches!(rig.host.replies.last(), Some(PracticeTransactionReply::Rate { id: 11, generation: 1, result: Ok(event) }) if *event == applied)
    );
    assert!(!rig.owner.transition_pending());
}

#[test]
fn equal_reduced_rate_is_noop_without_header_capture_or_transport_history_growth() {
    let mut rig = rig(&[true, false], 20_000_000);
    rig.start();
    let before = (rig.headers(), rig.capture_bytes(), rig.transport.clone());
    let request = rig.request(1, "2/4");
    rig.admit(request, false).unwrap();
    rig.render(1);
    let event = rig.commit_rate();
    assert_eq!(event.outcome, PracticeRateOutcome::Noop);
    assert_eq!((event.request_revision, event.applied_revision), (0, 1));
    assert_eq!(
        (rig.headers(), rig.capture_bytes(), rig.transport.clone()),
        before
    );
    assert_eq!(rig.owner.rate_state().unwrap().revision, 1);
    assert_eq!(rig.owner.members()[0].launch.attempt(), 0);
}

#[test]
fn actual_finite_end_refusal_cancels_all_reservations_without_rewriting_identity() {
    let mut rig = rig(&[true, false], 1_500_000);
    rig.start();
    let before = (rig.headers(), rig.capture_bytes(), rig.transport.clone());
    let request = rig.request(1, "2/1");
    rig.admit(request, false).unwrap();
    assert_eq!(rig.render(1), [0.0]);
    let event = rig.commit_rate();
    assert_eq!(
        event.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::Ended)
    );
    assert_eq!((event.physical_frame, event.applied_revision), (3, 0));
    assert_eq!(
        (rig.headers(), rig.capture_bytes(), rig.transport.clone()),
        before
    );
    assert_eq!(rig.owner.current_speed(), speed("1/2"));
    assert_eq!(rig.owner.members()[0].launch.attempt(), 0);
    assert!(matches!(
        rig.owner.peek_event().unwrap().unwrap().event,
        PracticeEvent::Boundary(PracticeReceipt {
            kind: PracticeBoundaryKind::Ended,
            ..
        })
    ));
}

#[test]
fn fixed_competition_refusal_occurs_before_pcm_enqueue_and_preserves_opponent_context() {
    let mut rig = rig(&[true, false], 20_000_000);
    rig.start();
    let before = (rig.headers(), rig.capture_bytes(), rig.transport.clone());
    let request = rig.request(1, "2/1");
    assert!(rig.admit(request, true).is_err());
    assert!(!rig.owner.transition_pending());
    assert_eq!(rig.render(2), [0.046875, 0.0625]);
    assert!(rig.owner.peek_event().unwrap().is_none());
    assert_eq!(
        (rig.headers(), rig.capture_bytes(), rig.transport.clone()),
        before
    );
    assert_eq!(rig.owner.rate_state().unwrap().revision, 0);
    rig.admit(request, false).unwrap();
    rig.render(1);
    assert_eq!(rig.commit_rate().outcome, PracticeRateOutcome::Applied);
}

#[test]
fn wrong_capture_roster_or_phase_cannot_partially_change_any_member() {
    let mut rig = rig(&[true, false, true], 20_000_000);
    rig.start();
    let before = (rig.headers(), rig.capture_bytes(), rig.transport.clone());
    let request = rig.request(1, "2/1");
    {
        let mut missing: Vec<_> = rig
            .captures
            .iter_mut()
            .take(2)
            .map(|(id, c)| (*id, c))
            .collect();
        assert!(rig
            .owner
            .admit_transaction(request, true, &mut rig.transport, &mut missing, false)
            .is_err());
    }
    assert_eq!(
        (rig.headers(), rig.capture_bytes(), rig.transport.clone()),
        before
    );
    assert!(!rig.owner.transition_pending());
    rig.admit(request, false).unwrap();
    rig.render(1);
    let now = rig.now();
    let cut = rig
        .owner
        .qualify_event(&rig.presentation, now)
        .unwrap()
        .unwrap();
    let event = match cut.projected_event().event {
        PracticeEvent::Rate(e) => e,
        _ => panic!("expected rate"),
    };
    let wrong_e = rig
        .phase
        .point_from_elapsed_nanos(cut.output(), event.active_elapsed_nanos + 1)
        .unwrap();
    {
        let mut captures: Vec<_> = rig.captures.iter_mut().map(|(id, c)| (*id, c)).collect();
        assert!(rig
            .owner
            .prepare_rate_event(&cut, wrong_e, &mut rig.transport, &mut captures)
            .is_err());
    }
    assert_eq!(
        (rig.headers(), rig.capture_bytes(), rig.transport.clone()),
        before
    );
    assert_eq!(rig.owner.peek_event().unwrap(), Some(cut.projected_event()));
    let event = rig.commit_rate();
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
    assert_eq!(rig.owner.rate_state().unwrap().revision, 1);
}

#[test]
fn initial_effective_headers_require_exact_member_order_unique_identity_and_conditions() {
    for issue in 0..6 {
        let result = rig_headers(&[false, false], 20_000_000, |headers| match issue {
            0 => {
                headers.pop();
            }
            1 => {
                headers[1].player = headers[0].player;
            }
            2 => headers.swap(0, 1),
            3 => headers[1].player = PlayerId(99),
            4 => {
                headers[1].header.options.push(0);
            }
            5 => {
                headers[1].header.normalized_clock = RAW;
            }
            _ => unreachable!(),
        });
        assert!(
            result.is_err(),
            "accepted invalid runtime header case {issue}"
        );
    }
    let rig = rig(&[false, false], 20_000_000);
    assert!(rig.owner.effective_header(PlayerId(99)).is_none());
    assert_eq!(
        rig.owner.effective_header(PlayerId(7)),
        rig.owner.effective_header(PlayerId(14))
    );
}

#[test]
fn last_members_file_limit_refuses_before_enqueue_and_releases_earlier_reservations() {
    let original = rig(&[true, true, true], 20_000_000);
    let bytes = original.capture_bytes()[2].as_ref().unwrap().len();
    let bounded =
        ReplayCodecLimits::new(bytes + 1, 100, bytes + 1, limits().input_limits()).unwrap();
    let mut rig =
        rig_headers_limits(&[true, true, true], 20_000_000, Some((2, bounded)), |_| {}).unwrap();
    rig.start();
    let before = (rig.headers(), rig.capture_bytes(), rig.transport.clone());
    let request = rig.request(11, "2/1");
    assert!(rig.admit(request, false).is_err());
    assert!(!rig.owner.transition_pending());
    assert_eq!(
        (rig.headers(), rig.capture_bytes(), rig.transport.clone()),
        before
    );
    let first = rig.captures[0].1.as_mut().unwrap();
    let reservation = first.prepare_source_rate_change(speed("3/2")).unwrap();
    first.cancel_source_rate_change(reservation).unwrap();
    assert_eq!(rig.render(1), [0.046875]);
    assert!(rig.owner.peek_event().unwrap().is_none());
    // The failed admission cannot consume this request id or reserve a lasting
    // partial cohort transaction. A zero-growth equal-rate control still works.
    let noop = rig.request(11, "2/4");
    rig.admit(noop, false).unwrap();
    rig.render(1);
    assert_eq!(rig.commit_rate().outcome, PracticeRateOutcome::Noop);
    assert_eq!(
        (rig.headers(), rig.capture_bytes(), rig.transport.clone()),
        before
    );
}

#[test]
fn actual_scrub_and_loop_keep_current_speed_and_compound_f5_restores_original_once() {
    let mut rig = rig(&[true, false], 20_000_000);
    rig.start();
    let originals: Vec<_> = rig
        .owner
        .members()
        .iter()
        .map(|m| m.launch.original_args().to_vec())
        .collect();
    let rate = rig.request(1, "2/1");
    rig.admit(rate, false).unwrap();
    rig.render(1);
    rig.commit_rate();
    let scrub = PracticeTransactionRequest::Region(UiPracticeRequest {
        id: 2,
        generation: rig.owner.generation(),
        action: PracticeAction::Scrub {
            target: ts(1_000_000),
        },
    });
    rig.admit(scrub, false).unwrap();
    rig.render(1);
    let scrubbed = rig.commit_fresh_boundary(speed("2/1"));
    assert_eq!(
        (scrubbed.generation, scrubbed.applied_song_time),
        (2, ts(1_000_000))
    );
    assert_eq!(rig.owner.current_speed(), speed("2/1"));
    assert_eq!(rig.owner.members()[0].launch.attempt(), 1);
    let region = PracticeTransactionRequest::Region(UiPracticeRequest {
        id: 3,
        generation: rig.owner.generation(),
        action: PracticeAction::Loop {
            start: ts(2_000_000),
            end: ts(5_000_000),
        },
    });
    rig.admit(region, false).unwrap();
    rig.render(1);
    let looped = rig.commit_fresh_boundary(speed("2/1"));
    assert_eq!(
        (looped.generation, looped.applied_song_time),
        (3, ts(2_000_000))
    );
    assert!(rig.owner.region().repeat);
    assert_eq!(rig.owner.section_end(), Some(ts(5_000_000)));
    let restart = PracticeTransactionRequest::RestartOriginal {
        id: 4,
        generation: rig.owner.generation(),
        expected: rig.owner.rate_state().unwrap(),
    };
    rig.admit(restart, false).unwrap();
    rig.render(1);
    let original = rig.commit_fresh_boundary(speed("1/2"));
    assert_eq!(
        (
            original.generation,
            original.iteration,
            original.applied_song_time
        ),
        (4, 0, ts(0))
    );
    assert_eq!((original.physical_frame, original.playback_frame), (6, 6));
    assert_eq!(rig.owner.current_speed(), speed("1/2"));
    assert_eq!(
        rig.owner.region(),
        PracticeRegion::new(ts(0), ts(20_000_000), false).unwrap()
    );
    assert_eq!(rig.owner.section_end(), Some(ts(20_000_000)));
    assert!(
        rig.owner.peek_event().unwrap().is_none(),
        "F5 must produce one compound event"
    );
    for (member, original) in rig.owner.members().iter().zip(originals) {
        assert_eq!(member.launch.attempt(), 3);
        assert_eq!(member.launch.original_args(), original);
    }
    let zero = rig
        .phase
        .point_from_elapsed_nanos(point(LOGICAL, BASE + 6_000_000), 0)
        .unwrap();
    let one = rig
        .phase
        .point_from_elapsed_nanos(point(LOGICAL, BASE + 7_000_000), 1_000_000)
        .unwrap();
    assert_eq!(rig.transport.position_at_output_phase(zero).unwrap(), ts(0));
    assert_eq!(
        rig.transport.position_at_output_phase(one).unwrap(),
        ts(500_000)
    );
}

#[test]
fn dropped_actual_cut_preparation_keeps_all_reservations_while_late_reports_arrive() {
    let mut rig = rig(&[true, false, true], 20_000_000);
    rig.start();
    let before = (rig.headers(), rig.capture_bytes(), rig.transport.clone());
    let request = rig.request(1, "2/1");
    rig.admit(request, false).unwrap();
    rig.render(1);
    let now = rig.now();
    let cut = rig
        .owner
        .qualify_event(&rig.presentation, now)
        .unwrap()
        .unwrap();
    let event = match cut.projected_event().event {
        PracticeEvent::Rate(event) => event,
        event => panic!("expected actual Rate, got {event:?}"),
    };
    assert_eq!(
        (
            event.physical_frame,
            event.playback_frame,
            event.active_elapsed_nanos
        ),
        (3, 3, 3_000_000)
    );
    let phase = rig
        .phase
        .point_from_elapsed_nanos(cut.output(), event.active_elapsed_nanos)
        .unwrap();
    {
        let mut captures: Vec<_> = rig
            .captures
            .iter_mut()
            .map(|(id, capture)| (*id, capture))
            .collect();
        let prepared = rig
            .owner
            .prepare_rate_event(&cut, phase, &mut rig.transport, &mut captures)
            .unwrap();
        drop(prepared);
    }
    assert_eq!(
        (rig.headers(), rig.capture_bytes(), rig.transport.clone()),
        before
    );
    assert_eq!(rig.owner.peek_event().unwrap(), Some(cut.projected_event()));
    assert!(rig.owner.transition_pending());
    // This original-time report represents an earlier acquired input drained
    // while the actual PCM result is held. It must fit against the still-live
    // reserved header, and it must survive retrying that same actual cut.
    let report = original_hit();
    for (_, capture) in &mut rig.captures {
        if let Some(capture) = capture {
            assert!(capture.prepare_source_rate_change(speed("3/2")).is_err());
            capture.record_report(&report).unwrap();
        }
    }
    let records: Vec<_> = rig
        .captures
        .iter()
        .map(|(_, capture)| capture.as_ref().map(|capture| capture.records().to_vec()))
        .collect();
    assert_eq!(rig.commit_rate(), event);
    for ((player, capture), expected) in rig.captures.iter().zip(records) {
        if let Some(capture) = capture {
            assert_eq!(capture.records(), expected.as_ref().unwrap());
            assert_eq!(
                capture.header(),
                rig.owner.effective_header(*player).unwrap()
            );
            assert_eq!(capture.records().len(), 1);
        }
    }
    assert_eq!(rig.owner.current_speed(), speed("2/1"));
    assert_eq!(rig.owner.rate_state().unwrap().revision, 1);
    assert!(!rig.owner.transition_pending());
}

#[test]
fn replay_logical_clock_must_bind_to_actual_transport_and_original_native_proof() {
    let mut valid = rig(&[false, false], 20_000_000);
    valid.start();
    assert_ne!(RAW, LOGICAL);
    let wrong_basis = OutputPhaseBasis::new(RAW, 14045, 1, 0, 1000).unwrap();
    let mut wrong_transport = Transport::new_with_runtime_output_phase(
        wrong_basis,
        point(RAW, 0),
        ts(0),
        Duration::ZERO,
        speed("1/2").rate(),
        Some(ts(20_000_000)),
    )
    .unwrap();
    let wrong_before = wrong_transport.clone();
    let before = (valid.headers(), valid.transport.clone());
    let request = valid.request(1, "2/1");
    {
        let mut captures: Vec<_> = valid
            .captures
            .iter_mut()
            .map(|(id, capture)| (*id, capture))
            .collect();
        assert!(valid
            .owner
            .admit_transaction(request, true, &mut wrong_transport, &mut captures, false)
            .is_err());
    }
    assert_eq!(wrong_transport, wrong_before);
    assert_eq!((valid.headers(), valid.transport.clone()), before);
    assert!(!valid.owner.transition_pending());
    assert!(valid.owner.peek_event().unwrap().is_none());
    valid.admit(request, false).unwrap();
    valid.render(1);
    assert_eq!(valid.commit_rate().outcome, PracticeRateOutcome::Applied);

    // The constructor has no actual normalized-domain parameter. A roster
    // with one consistent alternate domain can be cold-created, but cannot
    // consume a genuine Started proof in another logical domain.
    let mut alternate = rig_headers(&[false, false], 20_000_000, |headers| {
        for header in headers {
            header.header.normalized_clock = RAW;
        }
    })
    .unwrap();
    alternate.observe(0);
    alternate
        .presentation
        .pin_practice_mixer_basis(alternate.owner.mixer_basis())
        .unwrap();
    alternate.mixer.render(&mut [0.0; 3]).unwrap();
    alternate.observe(3);
    let now = alternate.now();
    let actual = alternate.owner.peek_event().unwrap().unwrap();
    assert!(matches!(
        actual.event,
        PracticeEvent::Boundary(PracticeReceipt {
            kind: PracticeBoundaryKind::Started,
            ..
        })
    ));
    let proof = alternate
        .presentation
        .prepare_projected_practice_event(actual, now)
        .unwrap()
        .unwrap();
    assert_eq!(proof.raw_output().domain, RAW);
    assert_eq!(proof.output().domain, LOGICAL);
    assert_eq!(
        alternate
            .owner
            .effective_header(PlayerId(7))
            .unwrap()
            .normalized_clock,
        RAW
    );
    let before = alternate.headers();
    assert!(alternate
        .owner
        .prepare_event_boundary(&proof, &mut alternate.host)
        .is_err());
    assert_eq!(alternate.headers(), before);
    assert_eq!(alternate.owner.peek_event().unwrap(), Some(actual));
    assert_eq!(alternate.owner.members()[0].launch.attempt(), 0);
    assert_eq!(alternate.owner.generation(), 1);
    assert!(alternate.host.replies.is_empty());
}
