//! Real owner/controller traces; fake native dispatch does not prove DSP/device clocks.
use super::*;
use crate::{
    gameplay_output_owner::{fixtures::*, GameplayOutputOwner},
    gameplay_presentation::{GameplayOutputContext, GameplayPauseControl},
    local_runtime::SoloRuntime,
    playback_pause::NativePause,
};
use beatkernel::audio::ChannelMatrix;
use std::{cell::RefCell, rc::Rc};

struct BackendWithChannels {
    inner: Backend,
    marks: Rc<RefCell<Vec<&'static str>>>,
    matrices: Rc<RefCell<Vec<Vec<f32>>>>,
    pending: Option<(Fault, Fault)>,
    interval: bool,
}
impl OutputReplacementBackend for BackendWithChannels {
    type Presentation = PresentationEstimator;
    type Output = Output;
    type Request = Request;
    type Error = Fault;
    fn open(
        &mut self,
        req: Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<Fault, Output>> {
        self.marks.borrow_mut().push("strict");
        self.inner.open(req, mixer, epoch)
    }
    fn retire(&mut self, out: &mut Output) -> Result<(), Fault> {
        self.inner.retire(out)
    }
    fn start(&mut self, out: &mut Output) -> Result<(), Fault> {
        self.inner.start(out)
    }
    fn epoch(&self, out: &Output) -> u64 {
        self.inner.epoch(out)
    }
    fn basis(&self, out: &Output) -> OutputFrameBasis {
        self.inner.basis(out)
    }
    fn observe(&mut self, out: &mut Output, p: &mut PresentationEstimator) -> Result<(), Fault> {
        self.inner.observe(out, p)
    }
    fn render_report(&self, out: &Output) -> Result<Option<RenderReport>, Fault> {
        self.inner.render_report(out)
    }
    fn pause_observation(
        &self,
        out: &Output,
        pair: ClockPair,
        now: ClockPoint,
    ) -> Result<LivePauseObservation, Fault> {
        self.marks.borrow_mut().push("pause");
        if self.interval {
            Ok(LivePauseObservation::Interval {
                observation: None,
                now,
            })
        } else {
            self.inner.pause_observation(out, pair, now)
        }
    }
    fn observe_end(
        &self,
        out: &Output,
        end: &mut NativeEnd,
        pair: ClockPair,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.marks.borrow_mut().push("end");
        self.inner.observe_end(out, end, pair, report)
    }
}
impl OutputChannelRemixBackend for BackendWithChannels {
    fn open_remixed(
        &mut self,
        req: Request,
        mixer: Mixer,
        epoch: u64,
        matrix: ChannelMatrix,
    ) -> Result<Output, OutputOpenFailure<Fault, Output>> {
        self.marks.borrow_mut().push("remixed");
        self.matrices
            .borrow_mut()
            .push(matrix.coefficients().to_vec());
        let output = self.inner.open(req, mixer, epoch)?;
        match self.pending.take() {
            Some((error, cleanup)) => {
                Err(OutputOpenFailure::pending(error, output).with_cleanup_error(cleanup))
            }
            None => Ok(output),
        }
    }
}
struct Fixture {
    owner: GameplayOutputOwner<RemixedOutputBackend<BackendWithChannels>>,
    presentation: PresentationEstimator,
    pause: NativePause,
    config: crate::native_gameplay::NativeGameplayConfig,
    end: Option<NativeEnd>,
    runtime: SoloRuntime,
    trace: Rc<RefCell<Trace>>,
    marks: Rc<RefCell<Vec<&'static str>>>,
    matrices: Rc<RefCell<Vec<Vec<f32>>>>,
}
impl Fixture {
    fn new(pending: Option<(Fault, Fault)>) -> Self {
        let (mut output, mut producer, trace) = initial(vec![]);
        output.render(2);
        let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 1000).unwrap();
        pause.request(true, pair_frame(2)).unwrap();
        producer.request_pause(true);
        output.render(1);
        let boundary = pause
            .observe(output.report, pair_frame(3))
            .unwrap()
            .unwrap();
        output.render(2);
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(7),
                sample: SampleId(1),
                at: Timestamp::from_nanos(2_000_000),
                gain: 1.,
            })
            .unwrap();
        let mut transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
        transport.pause(boundary.host.timestamp).unwrap();
        transport
            .seek(boundary.host.timestamp, Timestamp::from_nanos(2_000_000))
            .unwrap();
        let mut runtime = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            transport,
            bindings(None),
            judge(&source()),
            producer,
            vec![],
            8,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let marks = Rc::new(RefCell::new(Vec::new()));
        let matrices = Rc::new(RefCell::new(Vec::new()));
        let backend = RemixedOutputBackend::new(BackendWithChannels {
            inner: Backend {
                trace: trace.clone(),
            },
            marks: marks.clone(),
            matrices: matrices.clone(),
            pending,
            interval: false,
        });
        let mut owner = GameplayOutputOwner::new(backend, output);
        let mut presentation =
            PresentationEstimator::new(settings(), point(2, 0), ClockDomainId(1), Timestamp::ZERO)
                .unwrap();
        owner.observe(&mut presentation).unwrap();
        Self {
            owner,
            presentation,
            pause,
            config: config(false),
            end: None,
            runtime,
            trace,
            marks,
            matrices,
        }
    }
    fn publish(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let now = point(1, self.trace.borrow().frames as i64 * 1_000_000);
        self.owner.publish_paused(
            GameplayOutputContext {
                presentation: &mut self.presentation,
                pause: &mut self.pause,
                config: &mut self.config,
                end: &mut self.end,
                control: GameplayPauseControl::solo(&mut self.runtime),
            },
            now,
        )
    }
    fn ready(&mut self) {
        for _ in 0..4 {
            if self.publish().unwrap() {
                return;
            }
        }
        panic!("no genuine replacement ready evidence");
    }
}
fn matrix() -> ChannelMatrix {
    ChannelMatrix::new(1, 2, &[1., 0.5]).unwrap()
}

#[test]
fn typed_channel_requests_use_real_owner_epochs_and_forward_original_overrides() {
    let mut f = Fixture::new(None);
    assert!(f
        .owner
        .queue(
            RemixedOutputRequest::remixed(request(1, 1), matrix()),
            100_000_000,
        )
        .is_ok());
    f.ready();
    assert_eq!(f.presentation.epoch(), 1);
    assert_eq!(f.pause.epoch(), 1);
    assert_eq!(&*f.matrices.borrow(), &[vec![1., 0.5]]);
    assert!(f
        .owner
        .queue(RemixedOutputRequest::strict(request(2, 1)), 100_000_000)
        .is_ok());
    f.ready();
    assert_eq!(f.presentation.epoch(), 2);
    assert!(f.marks.borrow().contains(&"strict"));
    assert!(f.marks.borrow().contains(&"remixed"));
    f.owner
        .pause_observation(f.presentation.latest_pair().unwrap(), point(1, 30_000_000))
        .unwrap();
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 1000, 1000).unwrap();
    let output = f.owner.current().unwrap();
    let backend = RemixedOutputBackend::new(BackendWithChannels {
        inner: Backend {
            trace: f.trace.clone(),
        },
        marks: f.marks.clone(),
        matrices: f.matrices.clone(),
        pending: None,
        interval: false,
    });
    backend
        .observe_end(
            output,
            &mut end,
            f.presentation.latest_pair().unwrap(),
            None,
        )
        .unwrap();
    assert!(f.marks.borrow().contains(&"pause"));
    assert!(f.marks.borrow().contains(&"end"));
    f.runtime.request_audio_pause(false);
    assert_eq!(
        f.owner
            .current()
            .unwrap()
            .mixer
            .as_ref()
            .unwrap()
            .playback_frame_cursor(),
        2
    );
    f.owner.current_mut().unwrap().render(2);
    assert_eq!(
        &f.trace.borrow().pcm[f.trace.borrow().pcm.len() - 2..],
        &[0.25, 0.5]
    );
}

#[test]
fn channel_open_failure_keeps_original_error_and_mixer_for_explicit_retry() {
    let mut f = Fixture::new(None);
    let fault = Fault::new(73);
    let ptr = fault.pointer();
    let mut requested = request(1, 0);
    requested.open_error = Some(fault);
    assert!(f
        .owner
        .queue(
            RemixedOutputRequest::remixed(requested, matrix()),
            100_000_000,
        )
        .is_ok());
    let error = f
        .publish()
        .unwrap_err()
        .downcast::<ReplacementFailure<Fault>>()
        .unwrap();
    assert!(
        matches!(&error.cause, ReplacementCause::Backend { error, .. } if error.pointer() == ptr)
    );
    assert_eq!(f.owner.state(), ReplacementState::RecoveredMixer);
    assert!(f
        .owner
        .queue(RemixedOutputRequest::strict(request(2, 0)), 100_000_000)
        .is_ok());
    f.ready();
    assert_eq!(f.presentation.epoch(), 2);
}

#[test]
fn channel_pending_failure_retains_cleanup_owner_and_error_identity() {
    let original = Fault::new(81);
    let cleanup = Fault::new(82);
    let ptr = original.pointer();
    let cleanup_ptr = cleanup.pointer();
    let mut f = Fixture::new(Some((original, cleanup)));
    assert!(f
        .owner
        .queue(
            RemixedOutputRequest::remixed(request(1, 0), matrix()),
            100_000_000,
        )
        .is_ok());
    let error = f
        .publish()
        .unwrap_err()
        .downcast::<ReplacementFailure<Fault>>()
        .unwrap();
    assert!(
        matches!(&error.cause, ReplacementCause::Backend { error, .. } if error.pointer() == ptr)
    );
    assert_eq!(error.cleanup.as_ref().unwrap().pointer(), cleanup_ptr);
    assert_eq!(f.owner.state(), ReplacementState::PendingRetirement);
    assert!(f.owner.take_recovered_mixer().is_none());
}

#[test]
fn original_interval_pause_evidence_is_not_replaced_by_point_fallback() {
    let (output, _producer, trace) = initial(vec![]);
    let backend = RemixedOutputBackend::new(BackendWithChannels {
        inner: Backend { trace },
        marks: Rc::new(RefCell::new(Vec::new())),
        matrices: Rc::new(RefCell::new(Vec::new())),
        pending: None,
        interval: true,
    });
    let now = point(1, 123);
    assert_eq!(
        backend
            .pause_observation(&output, pair_frame(0), now)
            .unwrap(),
        LivePauseObservation::Interval {
            observation: None,
            now
        }
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit ALSA null native diagnostic; no acoustic/device clock proof"]
fn actual_alsa_adapter_routes_matrix_and_recovers_original_source() {
    use crate::gameplay::output::adapters::alsa::AlsaReplacementBackend;
    use beatkernel::audio::StoppedMixerSource;
    use beatkernel_platform::{
        audio::{DeviceFormat, SampleEncoding},
        linux::AlsaRequest,
    };
    let (mut output, _producer, _trace) = initial(vec![]);
    let mixer = output.mixer.take().unwrap();
    let before = mixer.output_frame_basis();
    let request = AlsaRequest {
        device: "null".into(),
        format: DeviceFormat::new(1000, 2, SampleEncoding::Float32, None).unwrap(),
        period_frames: 2,
        buffer_frames: 8,
        allow_size_rounding: false,
        monotonic_domain: ClockDomainId(1),
    };
    let mut backend = RemixedOutputBackend::new(AlsaReplacementBackend);
    let mut native = backend
        .open(
            RemixedOutputRequest::remixed(request, matrix()),
            beatkernel_platform::audio::NativeOutputState::from_mixer(mixer),
            9,
        )
        .unwrap_or_else(|f| panic!("{}", f.error()));
    assert_eq!(backend.epoch(&native), 9);
    assert_eq!(backend.basis(&native), before);
    backend.start(&mut native).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while native.stream().snapshot().submitted_frames < 2 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(backend.render_report(&native).unwrap().is_some());
    backend.retire(&mut native).unwrap();
    let original = native.take_stopped_mixer().unwrap().unwrap();
    assert_eq!(original.mixer().config().format().channels(), 1);
    assert_eq!(original.output_frame_basis().origin(), before.origin());
}
