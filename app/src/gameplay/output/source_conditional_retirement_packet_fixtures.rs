//! Static Source contracts and real history packets; no worker or hardware proof.
use super::*;
use crate::{
    audio_authority::AudioAuthorityCoordinateBasis,
    gameplay::output::application::owner::{
        fixtures as lifecycle, source_playback_owner_fixtures as source,
    },
    native_audio_presentation::NativeAudioSnapshot,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::{
    audio::{
        AudioCommand, ChannelMatrix, CommandProducer, InterpolatedTargetTime, SampleId, VoiceId,
    },
    time::{presentation::PresentationEstimator, ClockMappingQuality, Timestamp},
};
use beatkernel_platform::audio::{
    presentation::validation::OriginalNativePresentationEvidence, AudioClockReadingQuality,
    AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus, DeviceFormat, NativeOutputState,
    SampleEncoding, StreamCounters,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    marker::PhantomData,
    rc::Rc,
};

pub(crate) use lifecycle::Fault;
pub(crate) use source::Evidence;

// Neither a request, ticket nor native evidence requires Clone or Copy.
pub(crate) struct Request(pub Box<u64>);
pub(crate) struct Ticket(pub Box<[u64; 2]>);
impl Ticket {
    pub(crate) fn pointer(&self) -> *const [u64; 2] {
        self.0.as_ref()
    }
}
#[derive(Default)]
pub(crate) struct Effects {
    pub(crate) opens: RefCell<Vec<(*const u64, Option<Vec<f32>>)>>,
    pub(crate) submissions: RefCell<Vec<(*const u64, Option<Vec<f32>>)>>,
    pub(crate) decisions: RefCell<Vec<(&'static str, *const [u64; 2])>>,
    pub(crate) conditional_outputs: RefCell<Vec<*const ()>>,
    pub(crate) observed_inputs: RefCell<Vec<(*const (), SourcePlaybackIdentity)>>,
    pub(crate) queried_inputs:
        RefCell<Vec<(*const (), InterpolatedTargetTime, SourcePlaybackIdentity)>>,
    pub(crate) submitted: RefCell<VecDeque<Result<Option<Ticket>, Fault>>>,
    pub(crate) polled: RefCell<VecDeque<Result<SourceRetirementStatus, Fault>>>,
    pub(crate) cancelled: RefCell<VecDeque<Result<SourceRetirementStatus, Fault>>>,
    pub(crate) observations:
        RefCell<VecDeque<Result<Option<NativePlaybackObservation<Evidence>>, Fault>>>,
    pub(crate) queries:
        RefCell<VecDeque<Result<Option<NativeSourcePlaybackQuery<Evidence>>, Fault>>>,
    pub(crate) history: RefCell<Vec<(u64, OutputFrameBasis, SourcePlaybackIdentity, u64)>>,
    pub(crate) history_replies: RefCell<VecDeque<Result<Option<u64>, Fault>>>,
    pub(crate) created_error: RefCell<Option<Fault>>,
    pub(crate) open_error: RefCell<Option<Fault>>,
    pub(crate) prepare_error: RefCell<Option<Fault>>,
    pub(crate) created_calls: Cell<usize>,
    pub(crate) prepare_calls: Cell<usize>,
    pub(crate) observe_calls: Cell<usize>,
    pub(crate) query_calls: Cell<usize>,
    pub(crate) retire_calls: Cell<usize>,
}
pub(crate) struct Backend<O = Mixer> {
    pub(crate) effects: Rc<Effects>,
    pub(crate) frequency: Option<u64>,
    owner: PhantomData<O>,
}
impl<O> Backend<O> {
    pub(crate) fn new(effects: Rc<Effects>, frequency: Option<u64>) -> Self {
        Self {
            effects,
            frequency,
            owner: PhantomData,
        }
    }
}
pub(crate) struct Output<O = Mixer> {
    pub(crate) owner: Option<O>,
    pub(crate) epoch: u64,
    pub(crate) descriptor: AudioAuthorityCoordinateBasis,
    pub(crate) retired: bool,
}
impl<O> Output<O> {
    pub(crate) fn basis(&self) -> OutputFrameBasis {
        match self.descriptor {
            AudioAuthorityCoordinateBasis::SourceFrame(basis)
            | AudioAuthorityCoordinateBasis::SourceCounter { basis, .. } => basis,
            AudioAuthorityCoordinateBasis::TargetFrame(_) => unreachable!(),
        }
    }
}
impl<O> StoppedMixerSource<O> for Output<O> {
    type Error = Fault;
    fn take_stopped_mixer(&mut self) -> Result<Option<O>, Fault> {
        if !self.retired {
            return Err(Fault::new(90));
        }
        Ok(self.owner.take())
    }
}
impl<O: SoftwareOutputState> Backend<O> {
    fn open_with_matrix(
        &mut self,
        request: Request,
        owner: O,
        epoch: u64,
        matrix: Option<ChannelMatrix>,
    ) -> Result<Output<O>, OutputOpenFailure<Fault, Output<O>, O>> {
        self.effects.opens.borrow_mut().push((
            request.0.as_ref(),
            matrix.as_ref().map(|matrix| matrix.coefficients().to_vec()),
        ));
        if let Some(error) = self.effects.open_error.borrow_mut().take() {
            return Err(OutputOpenFailure::recovered_state(error, Some(owner)));
        }
        let basis = owner.output_frame_basis();
        Ok(Output {
            owner: Some(owner),
            epoch,
            descriptor: descriptor(basis, self.frequency),
            retired: false,
        })
    }
}
impl<O: SoftwareOutputState> OutputReplacementBackend<O> for Backend<O> {
    type Presentation = PresentationEstimator;
    type Output = Output<O>;
    type Request = Request;
    type Error = Fault;
    fn open(
        &mut self,
        request: Request,
        owner: O,
        epoch: u64,
    ) -> Result<Output<O>, OutputOpenFailure<Fault, Output<O>, O>> {
        self.open_with_matrix(request, owner, epoch, None)
    }
    fn retire(&mut self, output: &mut Output<O>) -> Result<(), Fault> {
        self.effects
            .retire_calls
            .set(self.effects.retire_calls.get() + 1);
        output.retired = true;
        Ok(())
    }
    fn start(&mut self, _: &mut Output<O>) -> Result<(), Fault> {
        Ok(())
    }
    fn prepare_replacement_start(&mut self, _: &mut Output<O>) -> Result<(), Fault> {
        self.effects
            .prepare_calls
            .set(self.effects.prepare_calls.get() + 1);
        self.effects
            .prepare_error
            .borrow_mut()
            .take()
            .map_or(Ok(()), Err)
    }
    fn epoch(&self, output: &Output<O>) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Output<O>) -> OutputFrameBasis {
        output.basis()
    }
    fn observe(&mut self, _: &mut Output<O>, _: &mut Self::Presentation) -> Result<(), Fault> {
        panic!("Source contract must not use scalar presentation")
    }
    fn render_report(&self, _: &Output<O>) -> Result<Option<RenderReport>, Fault> {
        panic!("Source contract must not acquire an independent report")
    }
}
impl<O: SoftwareOutputState> OutputChannelRemixBackend<O> for Backend<O> {
    fn open_remixed(
        &mut self,
        request: Request,
        owner: O,
        epoch: u64,
        matrix: ChannelMatrix,
    ) -> Result<Output<O>, OutputOpenFailure<Fault, Output<O>, O>> {
        self.open_with_matrix(request, owner, epoch, Some(matrix))
    }
}
impl<O: SoftwareOutputState> OriginalNativeOutputBackend<O> for Backend<O> {
    fn observe_native(&mut self, _: &mut Output<O>) -> Result<Option<NativeAudioSnapshot>, Fault> {
        panic!("Source contract must not synthesize scalar evidence")
    }
}
impl<O: SoftwareOutputState> OriginalCoordinateNativeOutputBackend<O> for Backend<O> {
    fn observe_native_coordinate(
        &mut self,
        _: &mut Output<O>,
    ) -> Result<Option<CoordinateNativeAudioSnapshot>, Fault> {
        panic!("Source packets must not acquire separate native evidence")
    }
}
impl<O: SoftwareOutputState> SourcePlaybackNativeOutputBackend<O> for Backend<O> {
    type NativeEvidence = Evidence;
    fn observe_native_playback(
        &mut self,
        output: &mut Output<O>,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Evidence>>, Fault> {
        self.effects
            .observed_inputs
            .borrow_mut()
            .push((output as *const Output<O> as *const (), identity));
        self.effects
            .observe_calls
            .set(self.effects.observe_calls.get() + 1);
        self.effects
            .observations
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None))
    }
    fn source_playback_at(
        &self,
        output: &Output<O>,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Evidence>>, Fault> {
        self.effects.queried_inputs.borrow_mut().push((
            output as *const Output<O> as *const (),
            *query,
            identity,
        ));
        self.effects
            .query_calls
            .set(self.effects.query_calls.get() + 1);
        self.effects
            .queries
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None))
    }
}
impl<O: SoftwareOutputState> SourceHistoryRetirementNativeOutputBackend<O> for Backend<O> {
    fn request_source_history_retirement(
        &mut self,
        _: &mut Output<O>,
        epoch: u64,
        basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        pin: u64,
    ) -> Result<Option<u64>, Fault> {
        self.effects
            .history
            .borrow_mut()
            .push((epoch, basis, identity, pin));
        self.effects
            .history_replies
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None))
    }
}
impl<O: SoftwareOutputState> ConditionalSourceRetirementNativeOutputBackend<O> for Backend<O> {
    type Ticket = Ticket;
    fn created_source_coordinate_basis(
        &self,
        output: &Output<O>,
    ) -> Result<AudioAuthorityCoordinateBasis, Fault> {
        self.effects
            .created_calls
            .set(self.effects.created_calls.get() + 1);
        match self.effects.created_error.borrow_mut().take() {
            Some(error) => Err(error),
            None => Ok(output.descriptor),
        }
    }
    fn submit_source_conditional_retirement(
        &mut self,
        output: &mut Output<O>,
        request: &Request,
        matrix: Option<&ChannelMatrix>,
    ) -> Result<Option<Ticket>, Fault> {
        self.effects
            .conditional_outputs
            .borrow_mut()
            .push(output as *const Output<O> as *const ());
        self.effects.submissions.borrow_mut().push((
            request.0.as_ref(),
            matrix.map(|matrix| matrix.coefficients().to_vec()),
        ));
        self.effects
            .submitted
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None))
    }
    fn poll_source_conditional_retirement(
        &mut self,
        output: &mut Output<O>,
        ticket: &Ticket,
    ) -> Result<SourceRetirementStatus, Fault> {
        self.effects
            .conditional_outputs
            .borrow_mut()
            .push(output as *const Output<O> as *const ());
        self.effects
            .decisions
            .borrow_mut()
            .push(("poll", ticket.pointer()));
        self.effects
            .polled
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(SourceRetirementStatus::Pending))
    }
    fn cancel_source_conditional_retirement(
        &mut self,
        output: &mut Output<O>,
        ticket: &Ticket,
    ) -> Result<SourceRetirementStatus, Fault> {
        self.effects
            .conditional_outputs
            .borrow_mut()
            .push(output as *const Output<O> as *const ());
        self.effects
            .decisions
            .borrow_mut()
            .push(("cancel", ticket.pointer()));
        self.effects
            .cancelled
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(SourceRetirementStatus::Cancelled))
    }
}
pub(crate) fn descriptor(
    basis: OutputFrameBasis,
    frequency: Option<u64>,
) -> AudioAuthorityCoordinateBasis {
    match frequency {
        Some(frequency) => AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency },
        None => AudioAuthorityCoordinateBasis::SourceFrame(basis),
    }
}
pub(crate) fn fixture(frequency: Option<u64>) -> (Backend, Output, Rc<Effects>, CommandProducer) {
    let (_, mut original, _, producer) = source::remix_fixture();
    let effects = Rc::new(Effects::default());
    let output = Output {
        owner: original.mixer.take(),
        epoch: original.epoch,
        descriptor: descriptor(original.basis, frequency),
        retired: false,
    };
    (
        Backend::new(effects.clone(), frequency),
        output,
        effects,
        producer,
    )
}
pub(crate) fn complete_owner() -> (NativeOutputState, CommandProducer) {
    let (mut original, mut producer, _) = lifecycle::initial(vec![]);
    let mut mixer = original.mixer.take().unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut owner = NativeOutputState::new(
        mixer,
        DeviceFormat::new(1000, 2, SampleEncoding::Float32, None).unwrap(),
        Some(ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap()),
        4,
    )
    .unwrap();
    owner.render_pending(4).unwrap();
    owner.admit(1).unwrap();
    (owner, producer)
}
pub(crate) fn native(output: &Output, position: u64) -> CoordinateNativeAudioSnapshot {
    let basis = output.basis();
    let host = source::point(1, 100);
    let (evidence, coordinate) = match output.descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(_) => (
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(position).unwrap(),
                target: host,
            }),
            NativeOutputCoordinate::StreamFrame { frame: position },
        ),
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => (
            OriginalNativePresentationEvidence::Wasapi {
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters::default(),
                    clock: Some(AudioClockSnapshot {
                        position,
                        frequency,
                        qpc_100ns: 1,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(host),
                        mapping_quality: ClockMappingQuality::Unknown,
                    }),
                    render: None,
                },
                basis: Some(basis),
            },
            NativeOutputCoordinate::Counter {
                position,
                frequency,
            },
        ),
        _ => unreachable!(),
    };
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: output.epoch,
            basis,
            evidence,
        },
        coordinate,
    )
    .unwrap()
}
pub(crate) fn packet(
    output: &Output,
    native: CoordinateNativeAudioSnapshot,
) -> NativeSourcePlaybackQuery<Evidence> {
    let query = native.source_query().unwrap();
    let history = output
        .owner
        .as_ref()
        .unwrap()
        .source_playback_history()
        .unwrap();
    NativeSourcePlaybackQuery::new(
        output.epoch,
        output.basis(),
        source::identity(),
        query,
        SourcePlaybackFacts {
            playback: history.lookup_at_source_time(query).unwrap(),
            pulled_source_end: history.pulled_end_frame(),
            submitted_source_end: history.pulled_end_frame(),
            render: None,
            revision: 2,
        },
        Some(Evidence {
            words: vec![19, 31],
        }),
    )
    .unwrap()
}

#[test]
fn source_created_metadata_retains_class_and_native_frequency_without_minting_progress() {
    fn default_mixer<B: ConditionalSourceRetirementNativeOutputBackend>(
        backend: &B,
        output: &B::Output,
    ) -> AudioAuthorityCoordinateBasis {
        backend
            .created_source_coordinate_basis(output)
            .unwrap_or_else(|_| panic!("created descriptor"))
    }
    for frequency in [None, Some(u64::MAX)] {
        let (mut backend, mut output, effects, _producer) = fixture(frequency);
        assert_eq!(
            default_mixer(&backend, &output),
            descriptor(output.basis(), frequency)
        );
        assert!(backend
            .observe_native_playback(&mut output, source::identity())
            .unwrap()
            .is_none());
        assert!(backend
            .source_playback_at(
                &output,
                &source::query(1, 2_000_000_000),
                source::identity()
            )
            .unwrap()
            .is_none());
        assert_eq!(effects.created_calls.get(), 1);
        assert_eq!(effects.observe_calls.get(), 1);
        assert_eq!(effects.query_calls.get(), 1);
        assert!(effects.submissions.borrow().is_empty());
        assert_eq!(effects.retire_calls.get(), 0);
    }
}

#[test]
fn real_history_packets_require_exact_original_frame_or_counter_and_native_evidence() {
    for frequency in [None, Some(u64::MAX)] {
        let (_, output, _, _producer) = fixture(frequency);
        let coordinate = native(&output, 1);
        let packet = packet(&output, coordinate);
        let pointer = packet.native_evidence().unwrap().words.as_ptr();
        let facts = packet.facts();
        let observation = NativePlaybackObservation::new(coordinate, packet).unwrap();
        assert_eq!(observation.coordinate(), coordinate);
        assert_eq!(observation.source().facts(), facts);
        assert_eq!(
            observation
                .source()
                .native_evidence()
                .unwrap()
                .words
                .as_ptr(),
            pointer
        );
        let query = observation.source().query();
        let without_evidence = NativeSourcePlaybackQuery::<Evidence>::new(
            output.epoch,
            output.basis(),
            source::identity(),
            query,
            facts,
            None,
        )
        .unwrap();
        assert!(matches!(
            NativePlaybackObservation::new(coordinate, without_evidence),
            Err(SourcePlaybackPacketError::MissingNativeEvidence)
        ));
        if frequency.is_some() {
            let floor = source::query(0, output.basis().sample_rate());
            assert_eq!(
                query.point(output.basis().origin()).unwrap(),
                floor.point(output.basis().origin()).unwrap()
            );
            let rounded = NativeSourcePlaybackQuery::new(
                output.epoch,
                output.basis(),
                source::identity(),
                floor,
                facts,
                Some(Evidence {
                    words: vec![19, 31],
                }),
            )
            .unwrap();
            assert!(matches!(
                NativePlaybackObservation::new(coordinate, rounded),
                Err(SourcePlaybackPacketError::CoordinateMismatch)
            ));
        }
        for (invalid, expected) in [
            (
                SourcePlaybackFacts {
                    revision: 3,
                    ..facts
                },
                SourcePlaybackPacketError::InvalidRevision,
            ),
            (
                SourcePlaybackFacts {
                    submitted_source_end: facts.pulled_source_end + 1,
                    ..facts
                },
                SourcePlaybackPacketError::InvalidPrefix,
            ),
        ] {
            assert!(
                matches!(NativeSourcePlaybackQuery::<Evidence>::new(output.epoch, output.basis(), source::identity(), query, invalid, None), Err(error) if error == expected)
            );
        }
    }
}

#[test]
fn complete_owner_static_contract_keeps_partial_pcm_remix_history_and_single_recovery() {
    fn complete<B: ConditionalSourceRetirementNativeOutputBackend<NativeOutputState>>(_: &B) {}
    let (owner, _producer) = complete_owner();
    let basis = owner.output_frame_basis();
    let pcm_pointer = owner.pending_samples().as_ptr();
    let pcm = owner.pending_samples().to_vec();
    let report = owner.pending_report();
    assert!(pcm.iter().any(|sample| *sample != 0.0));
    assert!(basis.start_physical_frame() < owner.mixer().frame_cursor());
    let effects = Rc::new(Effects::default());
    let mut backend = Backend::<NativeOutputState>::new(effects.clone(), Some(10_000_003));
    complete(&backend);
    let mut output = backend.open(Request(Box::new(4)), owner, 9).unwrap();
    assert_eq!(
        backend.created_source_coordinate_basis(&output).unwrap(),
        descriptor(basis, Some(10_000_003))
    );
    assert!(output.take_stopped_mixer().is_err());
    backend.retire(&mut output).unwrap();
    let recovered = output.take_stopped_mixer().unwrap().unwrap();
    assert!(output.take_stopped_mixer().unwrap().is_none());
    assert_eq!(recovered.pending_samples().as_ptr(), pcm_pointer);
    assert_eq!(recovered.pending_samples(), pcm);
    assert_eq!(recovered.pending_report(), report);
    assert_eq!(recovered.admitted_frames(), 1);
    assert_eq!(
        recovered
            .mixer()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        4
    );
    assert!(
        recovered.into_mixer().is_err(),
        "prepared remix cannot flatten to Mixer"
    );
    assert_eq!(effects.retire_calls.get(), 1);
}
