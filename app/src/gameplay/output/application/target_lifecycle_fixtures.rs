//! Actual converted PCM ownership through the generic lifecycle controller.
//! Effect doubles provide retirement only; they supply no presentation authority.
use super::*;
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, ChannelMatrix, CommandProducer,
        MixerConfig, PcmLimits, PcmSample, ResampleQuality, SampleBank, SampleId, TargetFrameBasis,
        VoiceId,
    },
    time::{presentation::PresentationEstimator, ClockDomainId},
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

#[derive(Debug)]
struct Fault(Box<u64>);
impl Fault {
    fn new(id: u64) -> Self {
        Self(Box::new(id))
    }
    fn pointer(&self) -> *const u64 {
        self.0.as_ref()
    }
}
struct NativeEffectOutput {
    owner: Option<ConvertedNativeOutputState>,
    epoch: u64,
    retired: bool,
    take_refusals: VecDeque<Fault>,
    calls: Rc<RefCell<Vec<&'static str>>>,
}
impl StoppedMixerSource<ConvertedNativeOutputState> for NativeEffectOutput {
    type Error = Fault;
    fn take_stopped_mixer(&mut self) -> Result<Option<ConvertedNativeOutputState>, Fault> {
        self.calls.borrow_mut().push("take");
        if let Some(error) = self.take_refusals.pop_front() {
            return Err(error);
        }
        if !self.retired {
            return Err(Fault::new(90));
        }
        Ok(self.owner.take())
    }
}
struct NativeEffects {
    retirement: VecDeque<Result<(), Fault>>,
    calls: Rc<RefCell<Vec<&'static str>>>,
}
impl OutputReplacementBackend<ConvertedNativeOutputState, TargetFrameBasis> for NativeEffects {
    type Presentation = PresentationEstimator;
    type Output = NativeEffectOutput;
    type Request = ();
    type Error = Fault;
    fn open(
        &mut self,
        _: (),
        owner: ConvertedNativeOutputState,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Fault, Self::Output, ConvertedNativeOutputState>>
    {
        self.calls.borrow_mut().push("open");
        Ok(output(owner, epoch, &self.calls))
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Fault> {
        self.calls.borrow_mut().push("retire");
        self.retirement.pop_front().unwrap_or(Ok(()))?;
        output.retired = true;
        Ok(())
    }
    fn start(&mut self, _: &mut Self::Output) -> Result<(), Fault> {
        Err(Fault::new(91))
    }
    fn epoch(&self, output: &Self::Output) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Self::Output) -> TargetFrameBasis {
        output.owner.as_ref().unwrap().target_frame_basis()
    }
    fn observe(&mut self, _: &mut Self::Output, _: &mut Self::Presentation) -> Result<(), Fault> {
        Err(Fault::new(92))
    }
    fn render_report(&self, _: &Self::Output) -> Result<Option<RenderReport>, Fault> {
        Ok(None)
    }
    fn pause_observation(
        &self,
        _: &Self::Output,
        _: beatkernel::time::ClockPair,
        _: ClockPoint,
    ) -> Result<LivePauseObservation, Fault> {
        Err(Fault::new(93))
    }
    fn observe_end(
        &self,
        _: &Self::Output,
        _: &mut crate::native_end::NativeEnd,
        _: beatkernel::time::ClockPair,
        _: Option<RenderReport>,
    ) -> crate::native_gameplay::NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        Err("target lifecycle fixture supplies no native endpoint evidence".into())
    }
}
type Controller = OutputReplacement<NativeEffects, ConvertedNativeOutputState, TargetFrameBasis>;
fn controller() -> Controller {
    Controller::new(NativeEffects {
        retirement: VecDeque::new(),
        calls: Rc::new(RefCell::new(Vec::new())),
    })
}
fn output(
    owner: ConvertedNativeOutputState,
    epoch: u64,
    calls: &Rc<RefCell<Vec<&'static str>>>,
) -> NativeEffectOutput {
    NativeEffectOutput {
        owner: Some(owner),
        epoch,
        retired: false,
        take_refusals: VecDeque::new(),
        calls: calls.clone(),
    }
}
fn rig() -> (CommandProducer, ConvertedNativeOutputState) {
    let format = AudioFormat::new(44_100, 1).unwrap();
    let limits = PcmLimits::new(128, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            (0..32).map(|n| 0.125 + n as f32 / 128.0).collect(),
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
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let device = DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap();
    let mut owner = ConvertedNativeOutputState::new(
        mixer,
        device,
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap_or_else(|_| panic!("valid actual converter"));
    owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    (producer, owner)
}

#[test]
fn target_typed_attach_and_retirement_move_complete_partial_pcm_owner_once() {
    let (_producer, owner) = rig();
    let pcm = owner.pending_samples().to_vec();
    let report = owner.pending_report();
    let phase = owner.converter_owner().source_position();
    let basis = owner.target_frame_basis();
    let counters = owner.mixer().counters();
    assert_eq!(basis.sample_rate(), 48_000);
    assert_eq!(owner.mixer().config().format().sample_rate(), 44_100);
    let mut controller = controller();
    let native = output(owner, 17, &controller.backend().calls);
    assert_eq!(controller.backend().basis(&native), basis);
    assert!(controller.attach(native).is_ok());
    assert_eq!(controller.state(), ReplacementState::Attached);
    assert_eq!(controller.last_issued_epoch(), 17);
    assert!(controller.stop_owned().unwrap());
    assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
    let owner = controller.take_recovered_mixer().unwrap();
    assert_eq!(owner.pending_samples(), pcm);
    assert_eq!(owner.admitted_frames(), 2);
    assert_eq!(owner.pending_report(), report);
    assert_eq!(owner.converter_owner().source_position(), phase);
    assert_eq!(owner.target_frame_basis(), basis);
    assert_eq!(owner.mixer().counters(), counters);
    assert!(!owner.pending_is_held());
    assert!(controller.take_recovered_mixer().is_none());
    assert!(!controller.stop_owned().unwrap());
    assert!(!controller.cancel().unwrap());
    assert!(!controller.retry_retirement().unwrap());
    assert_eq!(
        controller.backend().calls.borrow().as_slice(),
        ["retire", "take"]
    );
}

#[test]
fn target_pending_retirement_retries_keep_full_owner_and_separate_original_diagnostics() {
    let (_producer, owner) = rig();
    let pcm = owner.pending_samples().to_vec();
    let phase = owner.converter_owner().source_position();
    let report = owner.pending_report();
    let basis = owner.target_frame_basis();
    let mut controller = controller();
    let primary = Fault::new(41);
    let primary_pointer = primary.pointer();
    let recovery = Fault::new(42);
    let recovery_pointer = recovery.pointer();
    controller.backend_mut().retirement.push_back(Err(primary));
    let mut native = output(owner, 7, &controller.backend().calls);
    native.take_refusals.push_back(recovery);
    native.take_refusals.push_back(Fault::new(52));
    assert!(controller.attach(native).is_ok());
    let first = controller.stop_owned().unwrap_err();
    match &first.cause {
        ReplacementCause::Backend {
            phase: ReplacementPhase::Retire,
            error,
        } => {
            assert_eq!(error.pointer(), primary_pointer);
            assert_eq!(*error.0, 41);
        }
        _ => panic!("original retirement diagnostic required"),
    }
    assert_eq!(first.recovery.as_ref().unwrap().pointer(), recovery_pointer);
    assert!(first.cleanup.is_none());
    assert_eq!(controller.state(), ReplacementState::PendingRetirement);
    assert!(controller.take_recovered_mixer().is_none());
    assert!(!controller.cancel().unwrap());
    controller
        .backend_mut()
        .retirement
        .push_back(Err(Fault::new(51)));
    let second = controller.retry_retirement().unwrap_err();
    assert!(
        matches!(&second.cause, ReplacementCause::Backend { phase: ReplacementPhase::Retire, error } if *error.0 == 51)
    );
    assert_eq!(*second.recovery.as_ref().unwrap().0, 52);
    assert_eq!(first.recovery.as_ref().unwrap().pointer(), recovery_pointer);
    assert!(controller.retry_retirement().unwrap());
    let owner = controller.take_recovered_mixer().unwrap();
    assert_eq!(owner.pending_samples(), pcm);
    assert_eq!(owner.admitted_frames(), 2);
    assert_eq!(owner.pending_report(), report);
    assert_eq!(owner.converter_owner().source_position(), phase);
    assert_eq!(owner.target_frame_basis(), basis);
    assert_eq!(
        controller.backend().calls.borrow().as_slice(),
        ["retire", "take", "retire", "take", "retire", "take"]
    );
}

#[test]
fn target_recovery_refusal_after_retirement_keeps_pending_owner_available_for_retry() {
    let (_producer, owner) = rig();
    let pcm = owner.pending_samples().to_vec();
    let basis = owner.target_frame_basis();
    let phase = owner.converter_owner().source_position();
    let mut controller = controller();
    let mut native = output(owner, 8, &controller.backend().calls);
    native.take_refusals.push_back(Fault::new(61));
    let failure = controller.retire_owned_output(native).unwrap_err();
    assert!(
        matches!(failure.cause, ReplacementCause::Backend { phase: ReplacementPhase::Recover, error } if *error.0 == 61)
    );
    assert!(failure.cleanup.is_none());
    assert!(failure.recovery.is_none());
    assert_eq!(controller.state(), ReplacementState::PendingRetirement);
    assert!(controller.take_recovered_mixer().is_none());
    assert!(controller.retry_retirement().unwrap());
    let owner = controller.take_recovered_mixer().unwrap();
    assert_eq!(owner.pending_samples(), pcm);
    assert_eq!(owner.target_frame_basis(), basis);
    assert_eq!(owner.converter_owner().source_position(), phase);
    assert_eq!(owner.admitted_frames(), 2);
}

#[test]
fn rejected_target_attach_returns_original_complete_owner_and_epochs_never_wrap() {
    let (_first_producer, first) = rig();
    let (_second_producer, second) = rig();
    let pcm = second.pending_samples().to_vec();
    let phase = second.converter_owner().source_position();
    let basis = second.target_frame_basis();
    let report = second.pending_report();
    let mut controller = controller();
    assert!(controller
        .attach(output(first, 3, &controller.backend().calls))
        .is_ok());
    let rejected = match controller.attach(output(second, u64::MAX, &controller.backend().calls)) {
        Err(output) => output,
        Ok(()) => panic!("attached controller must return rejected owner"),
    };
    assert_eq!(controller.last_issued_epoch(), 3);
    assert!(controller.backend().calls.borrow().is_empty());
    let actual = rejected.owner.as_ref().unwrap();
    assert_eq!(actual.pending_samples(), pcm);
    assert_eq!(actual.pending_report(), report);
    assert_eq!(actual.admitted_frames(), 2);
    assert_eq!(actual.converter_owner().source_position(), phase);
    assert_eq!(actual.target_frame_basis(), basis);
    assert!(!rejected.retired);
    controller.stop_owned().unwrap();
    let _first = controller.take_recovered_mixer().unwrap();
    assert!(controller.attach(rejected).is_ok());
    assert_eq!(controller.last_issued_epoch(), u64::MAX);
    controller.stop_owned().unwrap();
    let recovered = controller.take_recovered_mixer().unwrap();
    assert_eq!(recovered.pending_samples(), pcm);
    assert_eq!(recovered.target_frame_basis(), basis);
    assert!(controller
        .attach(output(recovered, 1, &controller.backend().calls))
        .is_ok());
    assert_eq!(controller.last_issued_epoch(), u64::MAX);
    controller.stop_owned().unwrap();
    assert!(controller.take_recovered_mixer().is_some());
}
