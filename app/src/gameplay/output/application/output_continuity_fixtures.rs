//! Controller evidence uses an actual Mixer and retained PCM; native admission
//! itself is covered by the platform production-pump fixtures.
use super::*;
use beatkernel::audio::*;
use beatkernel::time::presentation::{DisciplineConfig, PresentationEstimator};
use beatkernel::time::{ClockDomainId, ClockPair};
use beatkernel_platform::audio::{DeviceFormat, NativeOutputState, SampleEncoding};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy)]
enum Request {
    Normal,
    OpenFailure,
    PendingFailure,
    StartFailure,
}
pub(super) struct Output {
    pub(super) state: Option<NativeOutputState>,
    epoch: u64,
    basis: OutputFrameBasis,
    retired: bool,
    start_failure: bool,
}
impl StoppedMixerSource<NativeOutputState> for Output {
    type Error = &'static str;
    fn take_stopped_mixer(&mut self) -> Result<Option<NativeOutputState>, Self::Error> {
        if !self.retired {
            return Err("still live");
        }
        Ok(self.state.take())
    }
}
#[derive(Default)]
struct Trace {
    retirement_refusals: usize,
    opens: usize,
    observations: std::collections::VecDeque<(u64, bool)>,
}
struct Backend {
    trace: Rc<RefCell<Trace>>,
}
impl OutputReplacementBackend<NativeOutputState> for Backend {
    type Presentation = PresentationEstimator;
    type Output = Output;
    type Request = Request;
    type Error = &'static str;
    fn open(
        &mut self,
        request: Request,
        state: NativeOutputState,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<Self::Error, Output, NativeOutputState>> {
        self.trace.borrow_mut().opens += 1;
        if matches!(request, Request::OpenFailure) {
            return Err(OutputOpenFailure::recovered_state(
                "open refused",
                Some(state),
            ));
        }
        let out = Output {
            basis: state.output_frame_basis(),
            state: Some(state),
            epoch,
            retired: false,
            start_failure: matches!(request, Request::StartFailure),
        };
        if matches!(request, Request::PendingFailure) {
            return Err(OutputOpenFailure::pending_state("open pending", out)
                .with_cleanup_error("cleanup refused"));
        }
        Ok(out)
    }
    fn retire(&mut self, output: &mut Output) -> Result<(), Self::Error> {
        let mut trace = self.trace.borrow_mut();
        if trace.retirement_refusals != 0 {
            trace.retirement_refusals -= 1;
            return Err("retirement refused");
        }
        output.retired = true;
        Ok(())
    }
    fn start(&mut self, output: &mut Output) -> Result<(), Self::Error> {
        if output.start_failure {
            Err("start refused")
        } else {
            Ok(())
        }
    }
    fn epoch(&self, output: &Output) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Output) -> OutputFrameBasis {
        output.basis
    }
    fn observe(
        &mut self,
        _: &mut Output,
        _: &mut PresentationEstimator,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn render_report(&self, output: &Output) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.state.as_ref().unwrap().pending_report())
    }
}
impl crate::gameplay::output::ports::OutputChannelRemixBackend<NativeOutputState> for Backend {
    fn open_remixed(
        &mut self,
        request: Request,
        state: NativeOutputState,
        epoch: u64,
        matrix: ChannelMatrix,
    ) -> Result<Output, OutputOpenFailure<Self::Error, Output, NativeOutputState>> {
        assert_eq!(matrix.coefficients(), &[1.0]);
        self.open(request, state, epoch)
    }
}
impl crate::gameplay::output::ports::OriginalNativeOutputBackend<NativeOutputState> for Backend {
    fn observe_native(
        &mut self,
        output: &mut Output,
    ) -> Result<Option<NativeAudioSnapshot>, Self::Error> {
        let Some((frame, fresh)) = self.trace.borrow_mut().observations.pop_front() else {
            return Ok(None);
        };
        if fresh {
            let state = output.state.as_mut().unwrap();
            if state.pending_frames() != 0 {
                state.admit(state.pending_frames()).unwrap();
            }
            state.render_pending(1).unwrap();
        }
        Ok(Some(NativeAudioSnapshot {
            epoch: output.epoch,
            basis: output.basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(frame)),
        }))
    }
}
fn pair(frame: u64) -> ClockPair {
    let stamp = Timestamp::from_nanos((frame * 1_000_000_000 / 3) as i64);
    ClockPair {
        source: ClockPoint {
            domain: ClockDomainId(1),
            timestamp: stamp,
        },
        target: ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(stamp.as_nanos() + 100),
        },
    }
}
fn rig() -> (
    CommandProducer,
    NativePause,
    PresentationEstimator,
    OutputReplacement<Backend, NativeOutputState>,
) {
    let format = AudioFormat::new(3, 1).unwrap();
    let limits = PcmLimits::new(128, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            vec![0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0],
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(1),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut first = [0.0; 2];
    mixer.render(&mut first).unwrap();
    assert_eq!(first, [0.125, 0.25]);
    let mut pause = NativePause::new(pair(0).source, ClockDomainId(2), 3).unwrap();
    pause.request(true, pair(2)).unwrap();
    producer.request_pause(true);
    let paused = mixer.render(&mut [0.0; 1]).unwrap();
    pause.observe(Some(paused), pair(3)).unwrap().unwrap();
    let mut state = match NativeOutputState::new(
        mixer,
        DeviceFormat::new(3, 1, SampleEncoding::Float32, None).unwrap(),
        None,
        4,
    ) {
        Ok(state) => state,
        Err(_) => panic!("valid state"),
    };
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_000_000_000),
            gain: -0.5,
        })
        .unwrap();
    let output = Output {
        basis: state.output_frame_basis(),
        state: Some(state),
        epoch: 0,
        retired: false,
        start_failure: false,
    };
    let mut owner = OutputReplacement::new(Backend {
        trace: Rc::new(RefCell::new(Trace::default())),
    });
    assert!(owner.attach(output).is_ok());
    let mut presentation = PresentationEstimator::new(
        DisciplineConfig::default(),
        pair(0).source,
        ClockDomainId(2),
        Timestamp::ZERO,
    )
    .unwrap();
    presentation.observe_clock_pair(pair(3)).unwrap();
    (producer, pause, presentation, owner)
}
fn assert_recovered(mut state: NativeOutputState, mut producer: CommandProducer) {
    assert_eq!(state.output_frame_basis().start_physical_frame(), 4);
    assert_eq!(state.mixer().frame_cursor(), 7);
    assert_eq!(state.mixer().playback_frame_cursor(), 2);
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.pending_samples(), &[0.0; 3]);
    state.admit(3).unwrap();
    producer.request_pause(false);
    state.render_pending(3).unwrap();
    // Independent source+queued-voice sequence, with no rerender of the tail.
    assert_eq!(state.pending_samples(), &[0.375, 0.4375, 0.5]);
    assert_eq!(state.mixer().playback_frame_cursor(), 5);
}
#[test]
fn whole_owner_survives_open_start_pending_and_repeated_retirement_refusals() {
    for request in [
        Request::OpenFailure,
        Request::StartFailure,
        Request::PendingFailure,
    ] {
        let (mut producer, pause, presentation, mut owner) = rig();
        assert!(owner
            .begin(
                request,
                &presentation,
                &pause,
                Timestamp::ZERO,
                1_000_000,
                || producer.hold_pause()
            )
            .is_err());
        if matches!(request, Request::PendingFailure) {
            assert_eq!(owner.state(), ReplacementState::PendingRetirement);
            owner.backend_mut().trace.borrow_mut().retirement_refusals = 2;
            assert!(owner.retry_retirement().is_err());
            assert!(owner.retry_retirement().is_err());
            assert!(owner.take_recovered_mixer().is_none());
            assert!(matches!(owner.retry_retirement(), Ok(true)));
        }
        let state = owner.take_recovered_mixer().expect("whole owner returned");
        assert!(owner.take_recovered_mixer().is_none());
        assert_recovered(state, producer);
    }
}
#[test]
fn whole_owner_survives_cancel_timeout_and_initial_retirement_retry() {
    for scenario in 0..3 {
        let (mut producer, pause, presentation, mut owner) = rig();
        if scenario == 2 {
            owner.backend_mut().trace.borrow_mut().retirement_refusals = 1;
        }
        let begin = owner.begin(
            Request::Normal,
            &presentation,
            &pause,
            Timestamp::ZERO,
            1_000_000,
            || producer.hold_pause(),
        );
        if scenario == 2 {
            assert!(begin.is_err());
            assert_eq!(owner.backend().trace.borrow().opens, 0);
            assert!(matches!(owner.retry_retirement(), Ok(true)));
        } else {
            assert!(begin.is_ok());
            if scenario == 0 {
                assert!(matches!(owner.cancel(), Ok(true)));
            } else {
                assert!(matches!(owner.poll(pair(8).target), Ok(None)));
                let now = ClockPoint {
                    domain: ClockDomainId(2),
                    timestamp: Timestamp::from_nanos(
                        pair(8).target.timestamp.as_nanos() + 1_000_000,
                    ),
                };
                assert!(owner.poll(now).is_err());
            }
        }
        assert_recovered(
            owner
                .take_recovered_mixer()
                .expect("recover complete state"),
            producer,
        );
    }
}

#[test]
fn static_switch_and_remix_preserve_full_state_and_original_side_diagnostics() {
    use crate::gameplay::output::adapters::{
        remix::RemixedOutputBackend,
        switch::{OutputBackendSwitch, Switched},
    };
    use crate::gameplay::output::domain::remix::RemixedOutputRequest;
    for (second, pending_open) in [(false, false), (true, false), (false, true), (true, true)] {
        let (producer, _, _, mut owner) = rig();
        owner.stop_owned().unwrap();
        let state = owner.take_recovered_mixer().unwrap();
        let first = Backend {
            trace: Rc::new(RefCell::new(Trace::default())),
        };
        let other = Backend {
            trace: Rc::new(RefCell::new(Trace::default())),
        };
        let mut backend = OutputBackendSwitch::new(first, other);
        // Strict routing still preserves a complete state in the switch carrier.
        let native = if pending_open {
            Request::PendingFailure
        } else {
            Request::OpenFailure
        };
        let request = if second {
            Switched::Second(native)
        } else {
            Switched::First(native)
        };
        let failure = match backend.open(request, state, 1) {
            Err(failure) => failure,
            Ok(_) => panic!("injected open refusal"),
        };
        let (error, state, pending, cleanup) = failure.into_parts();
        let expected = if pending_open {
            "open pending"
        } else {
            "open refused"
        };
        assert_eq!(
            error,
            if second {
                Switched::Second(expected)
            } else {
                Switched::First(expected)
            }
        );
        if pending_open {
            assert!(state.is_none());
            assert_eq!(
                cleanup,
                Some(if second {
                    Switched::Second("cleanup refused")
                } else {
                    Switched::First("cleanup refused")
                })
            );
            let mut pending = pending.unwrap();
            assert!(pending.take_stopped_mixer().is_err());
            backend.retire(&mut pending).unwrap();
            assert_recovered(pending.take_stopped_mixer().unwrap().unwrap(), producer);
            assert!(pending.take_stopped_mixer().unwrap().is_none());
        } else {
            assert!(pending.is_none());
            assert!(cleanup.is_none());
            assert_recovered(state.unwrap(), producer);
        }
    }
    let (producer, _, _, mut owner) = rig();
    owner.stop_owned().unwrap();
    let state = owner.take_recovered_mixer().unwrap();
    let mut backend = RemixedOutputBackend::new(Backend {
        trace: Rc::new(RefCell::new(Trace::default())),
    });
    let request = RemixedOutputRequest::remixed(
        Request::PendingFailure,
        ChannelMatrix::new(1, 1, &[1.0]).unwrap(),
    );
    let failure = match backend.open(request, state, 1) {
        Err(failure) => failure,
        Ok(_) => panic!("injected pending refusal"),
    };
    let (error, state, pending, cleanup) = failure.into_parts();
    assert_eq!(error, "open pending");
    assert_eq!(cleanup, Some("cleanup refused"));
    assert!(state.is_none());
    let mut pending = pending.unwrap();
    assert!(pending.take_stopped_mixer().is_err());
    backend.retire(&mut pending).unwrap();
    assert_recovered(pending.take_stopped_mixer().unwrap().unwrap(), producer);
    assert!(pending.take_stopped_mixer().unwrap().is_none());
}

pub(super) fn full_owner_native_ready() -> (
    ReadyAudioOutput<Output>,
    NativeAudioPresentation,
    NativePause,
    InputMerger,
    CommandProducer,
    ClockPoint,
) {
    use crate::audio_authority::{AudioAuthority, AudioAuthorityConfig};
    use beatkernel::input::DeviceId;
    let (mut producer, pause, _, mut owner) = rig();
    let current = NativeAudioPresentation::new(
        AudioAuthority::new(
            AudioAuthorityConfig::default(),
            AudioAuthorityEpoch {
                id: 0,
                stream_origin: pair(0).source,
                logical_origin: ClockPoint {
                    domain: ClockDomainId(3),
                    timestamp: Timestamp::ZERO,
                },
                host_domain: ClockDomainId(2),
            },
        )
        .unwrap(),
        NativePresentationValidator::new(0, pair(0).source, ClockDomainId(2)),
    )
    .unwrap();
    let merger = InputMerger::new(ClockDomainId(2), pair(0).target, vec![DeviceId(1)], 8).unwrap();
    owner.backend_mut().trace.borrow_mut().observations.extend([
        (4, false),
        (6, false),
        (8, true),
        (9, true),
    ]);
    assert!(owner
        .begin_audio(
            Request::Normal,
            &current,
            &pause,
            &merger,
            Timestamp::ZERO,
            10_000_000_000,
            || producer.hold_pause()
        )
        .is_ok());
    let poll_now = pair(10).target;
    assert!(matches!(
        owner.poll_audio(&current, &merger, poll_now),
        Ok(None)
    ));
    match &owner.slot {
        Slot::Waiting(Waiting {
            timing: WaitingTiming::Audio(timing),
            ..
        }) => {
            assert!(timing.validator.latest_record().is_none());
            assert!(timing.snapshots.iter().all(Option::is_none));
            assert!(timing.pairs.iter().all(Option::is_none));
        }
        _ => panic!("early tail must remain waiting without anchors"),
    }
    assert!(matches!(
        owner.poll_audio(&current, &merger, poll_now),
        Ok(None)
    ));
    assert!(matches!(
        owner.poll_audio(&current, &merger, poll_now),
        Ok(None)
    ));
    let ready = match owner.poll_audio(&current, &merger, poll_now) {
        Ok(Some(ready)) => ready,
        _ => panic!("two fresh original anchors required"),
    };
    assert_eq!(ready.basis.start_physical_frame(), 4);
    assert_eq!(
        ready.snapshots[0].evidence,
        OriginalNativePresentationEvidence::SuppliedPair(pair(8))
    );
    assert_eq!(
        ready.snapshots[1].evidence,
        OriginalNativePresentationEvidence::SuppliedPair(pair(9))
    );
    assert_eq!(current.authority().epoch().id, 0);
    let state = ready.output.state.as_ref().unwrap();
    assert_eq!(state.mixer().playback_frame_cursor(), 2);
    assert_eq!(state.pending_samples(), &[0.0]);
    (ready, current, pause, merger, producer, poll_now)
}
