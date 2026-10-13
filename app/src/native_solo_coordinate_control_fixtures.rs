//! Actual solo service/entry and source Mixer; scripted IO grants no hardware accuracy.
#[path = "native_solo_target_control_fixtures.rs"]
mod target_control_fixtures;
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::BgmConfig,
    gameplay::output::ports::{
        NativeSourcePlaybackQuery, SourcePlaybackFacts, SourcePlaybackIdentity,
    },
    gameplay_competition::NoopSoloCompetition,
    gameplay_presentation::GameplayDevice,
    native_audio_presentation::NativeAudioSnapshot,
    native_audio_startup::new_coordinate_audio_presentation,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::{ConvertedPlaybackFacts, NativePlaybackMapping},
    play_policy::ResolvedPlayPolicy,
    replay_speed::ReplaySpeed,
};
use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, ConvertedPhaseSpan, InterpolatedTargetTime, Mixer,
        MixerConfig, OutputFrameBasis, PcmLimits, PcmSample, SampleBank, SampleId,
        TargetFrameBasis, TargetTime, VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId,
    },
    judge::{JudgeEngine, JudgeStage},
    replay::{ReplayOperation, codec::ReplayCodecLimits},
    runtime::{RuntimeProcessingClock, SoundBinding},
    time::presentation::PresentationEstimator,
    transport::{OutputPhaseBasis, Rate, Transport},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters, presentation::validation::OriginalNativePresentationEvidence,
};
use std::{cell::Cell, rc::Rc};
use crate::gameplay::output::ports::NativePlaybackObservation;
use crate::native_coordinate_control::CoordinateControlFacts;

fn cp(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}

fn song_at(elapsed: i64, preroll: i64, rate: Rate) -> Timestamp {
    Timestamp::from_nanos(
        ((i128::from(elapsed) - i128::from(preroll)) * i128::from(rate.numerator())
            / i128::from(rate.denominator())) as i64,
    )
}

#[test]
fn actual_entry_acknowledges_source_pause_and_resume_at_frozen_original_elapsed_for_four_rates() {
    for counter in [false, true] {
        for rate in [
            Rate::NORMAL,
            Rate::new(1, 2).unwrap(),
            Rate::new(3, 2).unwrap(),
            Rate::new(2, 1).unwrap(),
        ] {
            let mut f = Fixture::new(counter, rate);
            assert!(f.run().unwrap().is_none());
            assert_eq!(f.pause.phase(), PausePhase::Running);
            assert!(!f.runtime.transport_mut().is_paused());
            assert_eq!(f.device.acquired[0], (1, false, 4, 4));
            assert_eq!(f.device.acquired[1], (2, true, 5, 5));
            assert_eq!(f.device.acquired[2], (3, true, 6, 5));
            assert_eq!(f.device.acquired[3], (4, true, 7, 5));
            assert_eq!(f.device.acquired[4], (5, false, 8, 5));
            assert!(f.host.pauses.contains(&PauseState::Pausing));
            assert!(f.host.pauses.contains(&PauseState::Paused));
            assert!(f.host.pauses.contains(&PauseState::Resuming));
            let pause_reports = f
                .host
                .reports
                .iter()
                .zip(&f.host.report_steps)
                .filter(|(_, step)| **step == 2)
                .map(|(report, _)| report)
                .collect::<Vec<_>>();
            assert!(
                !pause_reports.is_empty(),
                "actual pause must execute Runtime advancement"
            );
            assert_eq!(
                pause_reports.last().unwrap().song_time,
                song_at(1_333_333_333, 0, rate)
            );
            let resume_reports = f
                .host
                .reports
                .iter()
                .zip(&f.host.report_steps)
                .filter(|(_, step)| **step == 5)
                .map(|(report, _)| report)
                .collect::<Vec<_>>();
            assert!(
                !resume_reports.is_empty(),
                "actual resume must execute Runtime advancement"
            );
            assert_eq!(resume_reports[0].song_time, song_at(1_333_333_333, 0, rate));
            assert!(f
                .capture
                .as_ref()
                .unwrap()
                .records()
                .iter()
                .any(|record| record.song_time == song_at(1_333_333_333, 0, rate)));
        }
    }
}

#[test]
fn odd_preroll_remains_one_exact_phase_after_actual_pause_resume() {
    for rate in [
        Rate::NORMAL,
        Rate::new(1, 2).unwrap(),
        Rate::new(3, 2).unwrap(),
        Rate::new(2, 1).unwrap(),
    ] {
        let mut f = Fixture::configured(false, rate, Duration::from_nanos(3), None);
        assert!(f.run().unwrap().is_none());
        for (report, step) in f.host.reports.iter().zip(&f.host.report_steps) {
            if *step == 2 || *step == 5 {
                assert_eq!(report.song_time, song_at(1_333_333_333, 3, rate));
            }
        }
        assert_eq!(
            f.runtime
                .transport_mut()
                .output_phase_basis()
                .unwrap()
                .active_origin_frame(),
            1
        );
        assert_eq!(
            f.runtime
                .transport_mut()
                .output_phase_basis()
                .unwrap()
                .source_hz(),
            3
        );
    }
}

#[test]
fn missing_compound_pause_facts_retains_pending_request_without_early_ack() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.device.missing_steps = vec![2, 3, 4];
    f.device.stop_at = 4;
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.pause.phase(), PausePhase::Pausing);
    assert!(f.device.mixer.pause_requested());
    assert!(!f.runtime.transport_mut().is_paused());
    assert!(!f.host.pauses.contains(&PauseState::Paused));
    assert!(!f.host.pauses.contains(&PauseState::Resuming));
    assert!(f.host.report_steps.iter().all(|step| *step == 1));
}

#[test]
fn missing_first_compound_acquisition_cannot_request_pause_from_scalar_render_receipt() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.device.missing_steps = vec![1, 2];
    f.device.stop_at = 2;
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert!(!f.device.mixer.pause_requested());
    assert!(!f.host.pauses.contains(&PauseState::Pausing));
    // The cold bracket already admitted physical source frame 3. Its real
    // retained mapping grants E=floor((3-active1)*1e9/3), independently of the
    // absent new control publication; missing facts cannot grant any new cut.
    assert_eq!(f.host.reports.len(), 1);
    assert_eq!(
        f.host.reports[0].song_time,
        Timestamp::from_nanos(666_666_666)
    );
    assert!(f.host.reports[0].input.is_none());
    assert_eq!(
        f.presentation
            .authority()
            .latest_coordinate_observation()
            .unwrap()
            .pair()
            .target,
        cp(1, 666_666_766)
    );
    let records = f.capture.as_ref().unwrap().records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].song_time, Timestamp::from_nanos(666_666_666));
    assert!(matches!(records[0].operation, ReplayOperation::Advance));
}

#[test]
fn pause_cut_equality_is_drained_with_literal_original_host_and_phase_provenance() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    // Q=5/3 seconds maps to minimal integer HOST 1,333,333,433.
    // The prior integer HOST is strictly before this original source-frame cut.
    f.device
        .events
        .push_back(input(1_333_333_432, 0, ButtonState::Down));
    f.device
        .events
        .push_back(input(1_333_333_433, 1, ButtonState::Up));
    // Acquisition only publishes events covered by its original prefix.
    assert!(f.run().unwrap().is_none());
    let reports = f
        .host
        .reports
        .iter()
        .filter(|r| r.input.is_some())
        .collect::<Vec<_>>();
    assert_eq!(reports.len(), 2);
    assert_eq!(
        reports[0]
            .input
            .as_ref()
            .unwrap()
            .meta()
            .original_clock_point,
        Some(cp(1, 1_333_333_432))
    );
    assert_eq!(
        reports[1]
            .input
            .as_ref()
            .unwrap()
            .meta()
            .original_clock_point,
        Some(cp(1, 1_333_333_433))
    );
    assert_eq!(reports[0].song_time, Timestamp::from_nanos(1_333_333_332));
    assert_eq!(reports[1].song_time, Timestamp::from_nanos(1_333_333_333));
    // Equality release is drained through PauseKeyboard, then reconciled at
    // the real resume source cut physical8/3; its original pause-cut HOST stays
    // intact while the normalized Runtime timestamp is the resume projection.
    assert_eq!(
        reports[1].input.as_ref().unwrap().meta().timestamp,
        Timestamp::from_nanos(2_333_333_833)
    );
    assert_eq!(
        reports[1].input.as_ref().unwrap().meta().clock_domain,
        ClockDomainId(3)
    );
    assert_eq!(
        reports[1].input.as_ref().unwrap().meta().source,
        DeviceId(9)
    );
    assert_eq!(reports[1].input.as_ref().unwrap().meta().sequence, 1);
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(cp(1, 1_333_333_432))
    );
}

#[test]
fn genuine_pause_advance_ack_and_capture_survive_post_operation_publication_failure() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.host.fail_step = Some(2);
    let error = f.run().unwrap_err();
    assert!(error
        .downcast_ref::<NativeReportObservationError>()
        .is_some());
    assert_eq!(f.pause.phase(), PausePhase::Paused);
    assert!(f.runtime.transport_mut().is_paused());
    assert_eq!(
        f.presentation
            .authority()
            .committed_operation()
            .unwrap()
            .timestamp,
        Timestamp::from_nanos(1_333_333_833)
    );
    assert_eq!(
        f.host.reports.last().unwrap().song_time,
        Timestamp::from_nanos(1_333_333_333)
    );
    assert_eq!(
        f.capture
            .as_ref()
            .unwrap()
            .records()
            .last()
            .unwrap()
            .song_time,
        Timestamp::from_nanos(1_333_333_333)
    );
}

#[test]
fn finite_entry_uses_absolute_native_endpoint_minus_nonzero_active_origin() {
    let mut f = Fixture::configured(
        false,
        Rate::NORMAL,
        Duration::ZERO,
        Some(Timestamp::from_nanos(2_000_000_000)),
    );
    f.host.pause_enabled = false;
    let result = f.run().unwrap();
    assert!(
        result.is_some(),
        "finite native ACK must continue through existing completion gates"
    );
    assert_eq!(
        f.runtime.judge().effective_song_time(),
        Some(Timestamp::from_nanos(2_000_000_000))
    );
    assert_eq!(
        f.host.reports.last().unwrap().song_time,
        Timestamp::from_nanos(2_000_000_000)
    );
    assert!(f.device.report.playback_end_physical_frame.is_some());
    assert_eq!(f.device.mixer.playback_frame_cursor(), 7);
    assert!(f
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .any(|r| r.song_time == Timestamp::from_nanos(2_000_000_000)));
}

#[test]
fn normal_coordinate_control_entry_without_selected_speed_uses_same_actual_phase() {
    let mut f = Fixture::new(true, Rate::NORMAL);
    assert!(f.run_with_speed(None).unwrap().is_none());
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert_eq!(f.device.acquired[4], (5, false, 8, 5));
    assert!(f
        .host
        .reports
        .iter()
        .zip(&f.host.report_steps)
        .any(|(r, step)| *step == 5 && r.song_time == Timestamp::from_nanos(1_333_333_333)));
}

#[test]
fn cold_invalid_capture_refuses_without_binding_actual_native_pause_or_end_owners() {
    let mut f = Fixture::configured(
        false,
        Rate::new(2, 1).unwrap(),
        Duration::ZERO,
        Some(Timestamp::from_nanos(4_000_000_000)),
    );
    let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
    // The capture has a different selected rate; the actual native owners and
    // Runtime still have their otherwise pristine, matching configuration.
    f.capture = Some(
        LiveReplayCapture::new_with_policy_and_speed(
            f.runtime.judge(),
            ClockDomainId(3),
            limits(),
            Timestamp::ZERO,
            0,
            f.cfg.audio.gameplay.end_song,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &policy,
            ReplaySpeed::new(Rate::NORMAL).unwrap(),
        )
        .unwrap(),
    );
    let pause = f.pause.clone();
    let end = f.end.clone();
    let presentation = format!("{:?}", f.presentation);
    let transport = f.runtime.transport_mut().clone();
    let mixer_cursor = f.device.mixer.frame_cursor();
    assert!(f.run().is_err());
    assert_eq!(f.pause, pause);
    assert_eq!(f.end, end);
    assert_eq!(format!("{:?}", f.presentation), presentation);
    assert_eq!(&*f.runtime.transport_mut(), &transport);
    assert_eq!(f.device.mixer.frame_cursor(), mixer_cursor);
    assert_eq!(f.device.step, 0);
    assert!(f.host.reports.is_empty());
}

#[test]
fn stale_same_publication_facts_refuse_before_native_pause_ack_and_runtime_control() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.device.stale_step = Some(2);
    assert!(f.run().is_err());
    assert_eq!(f.pause.phase(), PausePhase::Pausing);
    assert!(!f.runtime.transport_mut().is_paused());
    assert!(!f.host.pauses.contains(&PauseState::Paused));
    assert!(f.host.report_steps.iter().all(|step| *step == 1));
}

#[test]
fn paused_keyboard_reconciliation_preserves_original_device_and_original_host_at_frozen_song() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.device
        .events
        .push_back(input(1_000_000_100, 0, ButtonState::Down));
    f.device
        .events
        .push_back(input(2_000_000_100, 1, ButtonState::Up));
    assert!(f.run().unwrap().is_none());
    let report = f
        .host
        .reports
        .iter()
        .filter(|r| r.input.is_some())
        .last()
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(1_333_333_333));
    let meta = report.input.as_ref().unwrap().meta();
    assert_eq!(meta.source, DeviceId(9));
    assert_eq!(meta.original_clock_point, Some(cp(1, 2_000_000_100)));
    assert_eq!(meta.clock_domain, ClockDomainId(3));
    assert!(f
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .any(|r| matches!(r.operation, ReplayOperation::Input(_))
            && r.song_time == Timestamp::from_nanos(1_333_333_333)));
}
fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#TOTAL 100\n#WAV01 key.wav\n#00011:01\n#00111:01",
        Default::default(),
    )
    .unwrap()
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn input(at: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), cp(1, at), sequence),
        control: PhysicalControlId::keyboard(7u16),
        state,
    })
}
struct Device {
    shared: Rc<Cell<usize>>,
    counter: bool,
    missing_steps: Vec<usize>,
    stale_step: Option<usize>,
    last_facts: Option<CoordinateControlFacts>,
    stop_at: usize,
    acquired: Vec<(usize, bool, u64, u64)>,
    mixer: Mixer,
    basis: OutputFrameBasis,
    target: Option<TargetFrameBasis>,
    report: RenderReport,
    missing: Cell<bool>,
    wrong_epoch: Cell<bool>,
    suspended: bool,
    mapping_calls: Cell<usize>,
    report_fail: bool,
    step: usize,
    events: VecDeque<PhysicalInputEvent>,
}
fn original(basis: OutputFrameBasis, frame: u64, counter: bool) -> CoordinateNativeAudioSnapshot {
    let pair = ClockPair {
        source: basis.point_at_stream_frame(frame).unwrap(),
        target: cp(1, 100 + (frame * 1_000_000_000 / 3) as i64),
    };
    let (coordinate, evidence) = if counter {
        (
            NativeOutputCoordinate::Counter {
                position: frame,
                frequency: 3,
            },
            OriginalNativePresentationEvidence::Wasapi {
                basis: Some(basis),
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters::default(),
                    render: None,
                    clock: Some(AudioClockSnapshot {
                        position: frame,
                        frequency: 3,
                        qpc_100ns: frame,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(pair.target),
                        mapping_quality: ClockMappingQuality::Unknown,
                    }),
                },
            },
        )
    } else {
        (
            NativeOutputCoordinate::StreamFrame { frame },
            OriginalNativePresentationEvidence::SuppliedPair(pair),
        )
    };
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 0,
            basis,
            evidence,
        },
        coordinate,
    )
    .unwrap()
}
impl Device {
    fn pair(&self) -> ClockPair {
        let frame = self.report.start_frame - self.basis.start_physical_frame();
        ClockPair {
            source: self.basis.point_at_stream_frame(frame).unwrap(),
            target: cp(1, 100 + (frame * 1_000_000_000 / 3) as i64),
        }
    }
    fn actual_schedule(&self) -> ClockPoint {
        TargetTime::from_frames(
            self.report.playback_start_frame + self.report.playback_frames as u64,
            3,
        )
        .unwrap()
        .point(self.basis.origin())
        .unwrap()
    }
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;

    fn observe_coordinate_control(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        self.step += 1;
        self.shared.set(self.step);
        self.report = self.mixer.render(&mut [0.0; 1])?;
        self.acquired.push((
            self.step,
            self.report.paused,
            self.report.start_frame,
            self.report.playback_start_frame,
        ));
        if self.missing_steps.contains(&self.step) {
            return Ok(None);
        }
        let frame = self.report.start_frame - self.basis.start_physical_frame();
        let coordinate = original(self.basis, frame, self.counter);
        let query = InterpolatedTargetTime::new(
            TargetTime::from_frames(self.report.start_frame, 3).unwrap(),
            0,
            0,
            1,
            0,
            1,
        )
        .unwrap();
        let history = self.mixer.source_playback_history().unwrap();
        let playback = history.lookup_at_source_time(query).map_err(|_| {
            std::io::Error::other("actual control source history has no covering span")
        })?;
        let packet = NativeSourcePlaybackQuery::new(
            0,
            self.basis,
            identity,
            query,
            SourcePlaybackFacts {
                playback,
                pulled_source_end: history.pulled_end_frame(),
                submitted_source_end: history.pulled_end_frame(),
                render: Some(self.report),
                revision: (self.step as u64) * 2,
            },
            Some(()),
        )?;
        let observation = NativePlaybackObservation::new(coordinate, packet)?;
        presentation.admit_coordinate(coordinate)?;
        let facts = CoordinateControlFacts::from_source(&observation, presentation, identity)?;
        if self.stale_step == Some(self.step) {
            return Ok(self.last_facts);
        }
        self.last_facts = Some(facts);
        Ok(Some(facts))
    }

    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("no scalar estimator path")
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        panic!("control-enabled entry must use one compound publication")
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        if self.report_fail {
            Err("render receipt fault".into())
        } else {
            Ok(Some(self.report))
        }
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.pair().target)
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        while self
            .events
            .front()
            .is_some_and(|event| event.meta().timestamp <= self.pair().target.timestamp)
        {
            events.push_back(self.events.pop_front().unwrap());
        }
        Ok(InputBatch {
            completed_through: Some(self.pair().target),
            backlog: false,
            closed: self.step >= self.stop_at,
        })
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("no scalar end path")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("no scalar resume path")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(self.actual_schedule())
    }
    fn output_clock_suspended(&self) -> bool {
        self.suspended
    }
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        self.mapping_calls.set(self.mapping_calls.get() + 1);
        if self.missing.get() {
            return Ok(None);
        }
        let history = self.mixer.source_playback_history().unwrap();
        let epoch = if self.wrong_epoch.get() { 1 } else { 0 };
        if let Some(basis) = self.target {
            return Ok(Some(NativePlaybackMapping::converted(
                epoch,
                basis,
                identity,
                *query,
                ConvertedPlaybackFacts {
                    phase: ConvertedPhaseSpan::from_words([3, 1, 0, 1, 0, 1, 3, 0]).unwrap(),
                    segment_end: TargetTime::from_frames(4, 3).unwrap(),
                    generated_target_end: TargetTime::from_frames(4, 3).unwrap(),
                    submitted_target_end: TargetTime::from_frames(4, 3).unwrap(),
                    playback: history.span(0).unwrap(),
                    pulled_source_end: history.pulled_end_frame(),
                    revision: 2,
                },
            )?));
        }
        let playback = history.lookup_at_source_time(*query).map_err(|_| {
            std::io::Error::other("actual source history does not cover the exact playback query")
        })?;
        let packet = NativeSourcePlaybackQuery::new(
            epoch,
            self.basis,
            identity,
            *query,
            SourcePlaybackFacts {
                playback,
                pulled_source_end: history.pulled_end_frame(),
                submitted_source_end: history.pulled_end_frame(),
                render: Some(self.report),
                revision: 2,
            },
            Some(()),
        )?;
        Ok(Some(NativePlaybackMapping::from_source(&packet)))
    }
}
struct Host {
    shared: Rc<Cell<usize>>,
    pause_enabled: bool,
    pauses: Vec<PauseState>,
    report_steps: Vec<usize>,
    fail_step: Option<usize>,
    reports: Vec<RuntimeReport>,
    fail: bool,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        self.pause_enabled && self.shared.get() <= 3
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, state: PauseState) {
        self.pauses.push(state);
    }
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.report_steps.push(self.shared.get());
        self.reports.push(report.clone());
        if self.fail || self.fail_step == Some(self.shared.get()) {
            Err("post-operation publication refusal".into())
        } else {
            Ok(())
        }
    }
    fn publish_local_reports(
        &mut self,
        _: &[crate::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        panic!("solo only")
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
}
#[derive(Default)]
struct Control {
    waits: usize,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        Ok(0)
    }
    fn checked_add(moment: u64, d: WallDuration) -> Option<u64> {
        moment.checked_add(u64::try_from(d.as_nanos()).ok()?)
    }
    fn wait(&mut self, _: WallDuration) -> NativeGameplayResult<()> {
        self.waits += 1;
        if self.waits > 12 {
            return Err("fixture production loop did not close".into());
        }
        Ok(())
    }
}
struct Fixture {
    device: Device,
    runtime: SoloRuntime,
    presentation: NativeAudioPresentation,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    pause: NativePause,
    end: Option<NativeEnd>,
    completion: Option<SongCompletion>,
    capture: Option<LiveReplayCapture>,
    competition: Option<NoopSoloCompetition>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    merger: InputMerger,
    cfg: CoordinateGameplayConfig,
    speed: LiveSpeedSetup,
    host: Host,
}
impl Fixture {
    fn new(counter: bool, rate: Rate) -> Self {
        Self::configured(counter, rate, Duration::ZERO, None)
    }
    fn configured(
        counter: bool,
        rate: Rate,
        preroll: Duration,
        end_song: Option<Timestamp>,
    ) -> Self {
        let shared = Rc::new(Cell::new(0));
        let format = AudioFormat::new(3, 1).unwrap();
        let pcm = PcmLimits::new(64, 256, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25; 8], pcm).unwrap(),
        )
        .unwrap();
        let (producer, consumer) = command_queue(16).unwrap();
        let mixer_config = MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(16, 8, 16, 16, 16).unwrap(),
        );
        let mixer_config = if end_song.is_some() {
            mixer_config.with_playback_end_frame(7)
        } else {
            mixer_config
        };
        let mut mixer = Mixer::new(mixer_config, bank, consumer).unwrap();
        mixer.enable_source_playback_history(8).unwrap();
        mixer.render(&mut [0.0; 1]).unwrap();
        let basis = mixer.output_frame_basis();
        let report = mixer.render(&mut [0.0; 3]).unwrap();
        let target_basis = None;
        let descriptor = if counter {
            AudioAuthorityCoordinateBasis::SourceCounter {
                basis,
                frequency: 3,
            }
        } else {
            AudioAuthorityCoordinateBasis::SourceFrame(basis)
        };
        let mut presentation = new_coordinate_audio_presentation(
            0,
            descriptor,
            ClockDomainId(1),
            cp(3, 500),
            AudioAuthorityConfig::default(),
        )
        .unwrap();
        for frame in [if counter { 1 } else { 0 }, 2] {
            presentation
                .admit_coordinate(original(basis, frame, counter))
                .unwrap();
        }
        let speed = LiveSpeedSetup::new(
            Timestamp::ZERO,
            preroll,
            ReplaySpeed::new(rate).unwrap(),
            end_song,
        )
        .unwrap();
        let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 13, 1, 3).unwrap();
        let transport = Transport::new_with_output_phase(
            phase,
            cp(3, 500),
            Timestamp::ZERO,
            preroll,
            rate,
            end_song,
        )
        .unwrap();
        let chart = source();
        let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
        let judge = JudgeEngine::new(
            chart.compile().unwrap().chart,
            chart.rules(),
            policy.judge().clone(),
        )
        .unwrap();
        let capture = LiveReplayCapture::new_with_policy_and_speed(
            &judge,
            ClockDomainId(3),
            limits(),
            Timestamp::ZERO,
            0,
            end_song,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &policy,
            speed.speed(),
        )
        .unwrap();
        let sounds = judge
            .chart()
            .objects()
            .iter()
            .map(|o| SoundBinding {
                object: o.id,
                stage: JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(o.id.0 + 1),
                gain: 1.0,
            })
            .collect();
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(7u16),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let mut runtime = SoloRuntime::new(
            ClockDomainId(3),
            ClockDomainId(2),
            transport,
            bindings,
            judge,
            producer,
            sounds,
            16,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        if let Some(end) = end_song {
            runtime.set_song_end(end).unwrap();
        }
        let stream = presentation.authority().epoch().stream_origin;
        let cfg = CoordinateGameplayConfig {
            audio: AudioGameplayConfig {
                gameplay: NativeGameplayConfig {
                    origin: cp(1, 90),
                    stream_origin: stream,
                    playback_origin: stream,
                    song_origin: speed.song_origin(),
                    sample_rate: 3,
                    end_song,
                    advance_lag: Duration::ZERO,
                    seconds: None,
                    pause_supported: true,
                    logical_schedule: true,
                },
                section_start: Timestamp::ZERO,
            },
            source_history: SourcePlaybackIdentity::new(3, 0, 0).unwrap(),
        };
        let merger = InputMerger::new_dynamic(ClockDomainId(1), cp(1, 90), 4096, 16).unwrap();
        Self {
            device: Device {
                shared: shared.clone(),
                counter,
                missing_steps: vec![],
                stale_step: None,
                last_facts: None,
                stop_at: 8,
                acquired: vec![],
                mixer,
                basis,
                target: target_basis,
                report,
                missing: Cell::new(false),
                wrong_epoch: Cell::new(false),
                suspended: false,
                mapping_calls: Cell::new(0),
                report_fail: false,
                step: 0,
                events: VecDeque::new(),
            },
            runtime,
            presentation,
            gauge: BmsGauge::new(policy.gauge().try_copy().unwrap()),
            bgm: BgmFeeder::new(
                vec![],
                BgmConfig {
                    output_origin: cp(2, 0),
                    sample_rate: 3,
                    preroll: Duration::ZERO,
                    lookahead: Duration::from_nanos(1),
                    max_pending: 1,
                },
            )
            .unwrap(),
            pause: if end_song.is_some() {
                NativePause::new(cp(2, 0), ClockDomainId(1), 3)
                    .unwrap()
                    .with_playback_end_frame(7)
                    .unwrap()
            } else {
                NativePause::new(cp(2, 0), ClockDomainId(1), 3).unwrap()
            },
            end: end_song.map(|_| NativeEnd::new(cp(2, 0), ClockDomainId(1), 3, 7).unwrap()),
            completion: None,
            capture: Some(capture),
            competition: None,
            delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
            pre: 0,
            merger,
            cfg,
            speed,
            host: Host {
                shared,
                pause_enabled: true,
                pauses: vec![],
                report_steps: vec![],
                fail_step: None,
                reports: vec![],
                fail: false,
            },
        }
    }
    fn run(&mut self) -> NativeGameplayResult<Option<CompletedPlayResult>> {
        self.run_with_speed(Some(self.speed))
    }
    fn run_with_speed(
        &mut self,
        speed: Option<LiveSpeedSetup>,
    ) -> NativeGameplayResult<Option<CompletedPlayResult>> {
        self.cfg.audio.gameplay.logical_schedule = false;
        run_gameplay_audio_coordinate_with_result_and_ports(
            &mut self.device,
            AudioGameplaySession {
                session: GameplaySession {
                    runtime: &mut self.runtime,
                    gauge: &mut self.gauge,
                    bgm: &mut self.bgm,
                    discipline: &mut self.presentation,
                    pause: &mut self.pause,
                    end: &mut self.end,
                    completion: &mut self.completion,
                    capture: &mut self.capture,
                    competition: &mut self.competition,
                    delivery: &mut self.delivery,
                    pre_origin_inputs: &mut self.pre,
                },
                merger: &mut self.merger,
            },
            self.cfg,
            &mut Control::default(),
            &mut self.host,
            speed,
        )
    }
}
