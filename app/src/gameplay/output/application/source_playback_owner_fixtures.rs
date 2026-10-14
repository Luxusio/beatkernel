//! Real static owner with mathematical DI packets, never hardware authentication.
use super::fixtures as memory;
use super::*;
use crate::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
    },
    gameplay::output::ports::{
        NativePlaybackObservation, NativeSourcePlaybackQuery,
        OriginalCoordinateNativeOutputBackend, OutputChannelRemixBackend, SourcePlaybackFacts,
        SourcePlaybackIdentity, SourcePlaybackNativeOutputBackend,
    },
    native_audio_presentation::NativeAudioSnapshot,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, ChannelMatrix, CommandProducer,
        InterpolatedTargetTime, MixerConfig, PcmLimits, SampleBank, TargetTime,
    },
    time::{
        presentation::PresentationEstimator, ClockDomainId, ClockMappingQuality, Duration,
        Timestamp,
    },
};
use beatkernel_platform::audio::{
    presentation::validation::NativePresentationValidator, AudioClockReadingQuality,
    AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus, StreamCounters,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

pub(crate) struct Evidence {
    pub words: Vec<u64>,
}
pub(crate) struct Io {
    pub observations:
        RefCell<VecDeque<Result<Option<NativePlaybackObservation<Evidence>>, memory::Fault>>>,
    pub replacement_observations:
        RefCell<VecDeque<Result<Option<NativePlaybackObservation<Evidence>>, memory::Fault>>>,
    pub replacement_inputs: RefCell<Vec<(*const (), SourcePlaybackIdentity, ClockPoint)>>,
    pub queries:
        RefCell<VecDeque<Result<Option<NativeSourcePlaybackQuery<Evidence>>, memory::Fault>>>,
    pub observed_calls: Cell<usize>,
    pub replacement_observed_calls: Cell<usize>,
    pub query_calls: Cell<usize>,
    pub legacy_calls: Cell<usize>,
    pub report_calls: Cell<usize>,
    pub reported_epoch: Cell<Option<u64>>,
    pub mutate_query_identity: Cell<bool>,
    pub mutate_observe_identity: Cell<bool>,
}
impl Default for Io {
    fn default() -> Self {
        Self {
            observations: RefCell::new(VecDeque::new()),
            replacement_observations: RefCell::new(VecDeque::new()),
            replacement_inputs: RefCell::new(Vec::new()),
            queries: RefCell::new(VecDeque::new()),
            observed_calls: Cell::new(0),
            replacement_observed_calls: Cell::new(0),
            query_calls: Cell::new(0),
            legacy_calls: Cell::new(0),
            report_calls: Cell::new(0),
            reported_epoch: Cell::new(None),
            mutate_query_identity: Cell::new(false),
            mutate_observe_identity: Cell::new(false),
        }
    }
}
pub(crate) struct Backend {
    pub inner: memory::Backend,
    pub io: Rc<Io>,
}
impl OutputReplacementBackend for Backend {
    type Presentation = PresentationEstimator;
    type Output = memory::Output;
    type Request = memory::Request;
    type Error = memory::Fault;
    fn open(
        &mut self,
        r: Self::Request,
        m: Mixer,
        e: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        self.inner.open(r, m, e)
    }
    fn retire(&mut self, o: &mut Self::Output) -> Result<(), Self::Error> {
        self.inner.retire(o)
    }
    fn start(&mut self, o: &mut Self::Output) -> Result<(), Self::Error> {
        self.inner.start(o)
    }
    fn epoch(&self, o: &Self::Output) -> u64 {
        self.io
            .reported_epoch
            .get()
            .unwrap_or_else(|| self.inner.epoch(o))
    }
    fn basis(&self, o: &Self::Output) -> OutputFrameBasis {
        self.inner.basis(o)
    }
    fn observe(
        &mut self,
        o: &mut Self::Output,
        p: &mut Self::Presentation,
    ) -> Result<(), Self::Error> {
        self.io.legacy_calls.set(self.io.legacy_calls.get() + 1);
        self.inner.observe(o, p)
    }
    fn render_report(&self, o: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        self.io.report_calls.set(self.io.report_calls.get() + 1);
        self.inner.render_report(o)
    }
}
impl OriginalNativeOutputBackend for Backend {
    fn observe_native(
        &mut self,
        _: &mut Self::Output,
    ) -> Result<Option<NativeAudioSnapshot>, Self::Error> {
        panic!("coherent source route must not acquire a separate scalar observation")
    }
}
impl OriginalCoordinateNativeOutputBackend for Backend {
    fn observe_native_coordinate(
        &mut self,
        _: &mut Self::Output,
    ) -> Result<Option<CoordinateNativeAudioSnapshot>, Self::Error> {
        panic!("coherent source route must not acquire separate coordinates")
    }
}
impl SourcePlaybackNativeOutputBackend for Backend {
    type NativeEvidence = Evidence;
    fn observe_native_playback(
        &mut self,
        _: &mut Self::Output,
        _: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Evidence>>, Self::Error> {
        self.io.observed_calls.set(self.io.observed_calls.get() + 1);
        let result = self
            .io
            .observations
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None));
        if self.io.mutate_observe_identity.get() {
            self.io.reported_epoch.set(Some(8));
        }
        result
    }
    fn observe_native_playback_replacement_at(
        &mut self,
        output: &mut Self::Output,
        identity: SourcePlaybackIdentity,
        now: ClockPoint,
    ) -> Result<Option<NativePlaybackObservation<Evidence>>, Self::Error> {
        self.io
            .replacement_observed_calls
            .set(self.io.replacement_observed_calls.get() + 1);
        self.io.replacement_inputs.borrow_mut().push((
            output as *const Self::Output as *const (),
            identity,
            now,
        ));
        self.io
            .replacement_observations
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None))
    }
    fn source_playback_at(
        &self,
        _: &Self::Output,
        _: &InterpolatedTargetTime,
        _: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Evidence>>, Self::Error> {
        self.io.query_calls.set(self.io.query_calls.get() + 1);
        let result = self.io.queries.borrow_mut().pop_front().unwrap_or(Ok(None));
        if self.io.mutate_query_identity.get() {
            self.io.reported_epoch.set(Some(8));
        }
        result
    }
}
impl OutputChannelRemixBackend for Backend {
    fn open_remixed(
        &mut self,
        r: Self::Request,
        m: Mixer,
        e: u64,
        _: ChannelMatrix,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        self.inner.open(r, m, e)
    }
}
pub(crate) fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
pub(crate) fn query(frame: u64, hz: u32) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, hz).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
pub(crate) fn presentation(
    basis: OutputFrameBasis,
    counter: Option<u64>,
) -> NativeAudioPresentation {
    let descriptor = match counter {
        Some(frequency) => AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency },
        None => AudioAuthorityCoordinateBasis::SourceFrame(basis),
    };
    let origin = basis.point_at_stream_frame(0).unwrap();
    NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig {
                history_capacity: 8,
                max_observation_age: Duration::from_nanos(1_000_000),
                ..AudioAuthorityConfig::default()
            },
            AudioAuthorityEpoch {
                id: 7,
                stream_origin: origin,
                logical_origin: point(3, 500),
                host_domain: ClockDomainId(1),
            },
            descriptor,
        )
        .unwrap(),
        NativePresentationValidator::new(7, origin, ClockDomainId(1)),
    )
    .unwrap()
}
pub(crate) fn fixture() -> (
    GameplayOutputOwner<Backend>,
    NativeAudioPresentation,
    Rc<Io>,
    CommandProducer,
) {
    fixture_with_retained_prefix(false)
}
fn fixture_with_retained_prefix(
    prefix: bool,
) -> (
    GameplayOutputOwner<Backend>,
    NativeAudioPresentation,
    Rc<Io>,
    CommandProducer,
) {
    let format = AudioFormat::new(2_000_000_000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(256, 512, 1).unwrap()).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
            AudioLimits::new(8, 2, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    if prefix {
        mixer.render(&mut [0.0; 2]).unwrap();
    }
    let basis = mixer.output_frame_basis();
    let report = if prefix {
        mixer.render(&mut [0.0; 6]).unwrap()
    } else {
        mixer.render(&mut [0.0; 8]).unwrap()
    };
    let trace = Rc::new(RefCell::new(memory::Trace::default()));
    let output = memory::Output {
        mixer: Some(mixer),
        epoch: 7,
        basis,
        report: Some(report),
        quiet: 0,
        retired: false,
        trace: trace.clone(),
    };
    let io = Rc::new(Io::default());
    (
        GameplayOutputOwner::new(
            Backend {
                inner: memory::Backend { trace },
                io: io.clone(),
            },
            output,
        ),
        presentation(basis, None),
        io,
        producer,
    )
}
pub(crate) fn identity() -> SourcePlaybackIdentity {
    SourcePlaybackIdentity::new(2_000_000_000, 0, 0).unwrap()
}
pub(crate) fn remix_fixture() -> (Backend, memory::Output, Rc<Io>, CommandProducer) {
    let (mut owner, _presentation, io, producer) = fixture();
    let output = owner.current.take().unwrap();
    let backend = Backend {
        inner: memory::Backend {
            trace: output.trace.clone(),
        },
        io: io.clone(),
    };
    (backend, output, io, producer)
}
pub(crate) fn packet(
    basis: OutputFrameBasis,
    epoch: u64,
    id: SourcePlaybackIdentity,
    q: InterpolatedTargetTime,
    report: Option<RenderReport>,
) -> NativeSourcePlaybackQuery<Evidence> {
    NativeSourcePlaybackQuery::new(
        epoch,
        basis,
        id,
        q,
        SourcePlaybackFacts {
            playback: beatkernel::audio::SourcePlaybackSpan::new(
                id.initial_physical(),
                None,
                id.initial_playback(),
                beatkernel::audio::SourcePlaybackState::Running,
            )
            .unwrap(),
            pulled_source_end: 8,
            submitted_source_end: 8,
            render: report,
            revision: 2,
        },
        Some(Evidence {
            words: vec![19, 31],
        }),
    )
    .unwrap()
}
pub(crate) fn observation(
    basis: OutputFrameBasis,
    frame: u64,
    host: i64,
    report: Option<RenderReport>,
) -> NativePlaybackObservation<Evidence> {
    let coordinate = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: point(1, host),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap();
    NativePlaybackObservation::new(
        coordinate,
        packet(
            basis,
            7,
            identity(),
            query(
                basis.start_physical_frame().checked_add(frame).unwrap(),
                basis.sample_rate(),
            ),
            report,
        ),
    )
    .unwrap()
}
fn state(p: &NativeAudioPresentation) -> String {
    format!("{p:?}")
}

#[test]
fn coherent_frame_admission_keeps_same_floor_progress_and_only_same_packet_report() {
    let (mut owner, mut p, io, _producer) = fixture();
    let basis = owner.current().unwrap().basis;
    let report = owner.current().unwrap().report;
    for (frame, host) in [(2, 100), (3, 101)] {
        io.observations
            .borrow_mut()
            .push_back(Ok(Some(observation(basis, frame, host, report))));
        let got = owner
            .observe_native_playback(&mut p, identity())
            .unwrap()
            .unwrap();
        assert_eq!(got.source().native_evidence().unwrap().words, [19, 31]);
    }
    assert_eq!(
        p.authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 3 })
    );
    assert_eq!(owner.render_report(), report);
    assert_eq!(io.report_calls.get(), 0);
    assert_eq!(io.legacy_calls.get(), 0);
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(observation(basis, 4, 102, None))));
    owner.observe_native_playback(&mut p, identity()).unwrap();
    assert_eq!(owner.render_report(), None);
}
#[test]
fn unavailable_packet_and_missing_current_do_not_mutate_owner_or_history() {
    let (mut owner, mut p, io, _producer) = fixture();
    let basis = owner.current().unwrap().basis;
    io.observations.borrow_mut().push_back(Ok(Some(observation(
        basis,
        2,
        100,
        owner.current().unwrap().report,
    ))));
    owner.observe_native_playback(&mut p, identity()).unwrap();
    let before = state(&p);
    let report = owner.render_report();
    assert!(report.is_some());
    assert!(owner
        .observe_native_playback(&mut p, identity())
        .unwrap()
        .is_none());
    assert_eq!(state(&p), before);
    assert_eq!(owner.render_report(), report);
    let output = owner.current.take().unwrap();
    let calls = io.observed_calls.get();
    assert!(owner
        .observe_native_playback(&mut p, identity())
        .unwrap()
        .is_none());
    assert_eq!(io.observed_calls.get(), calls);
    assert!(owner
        .source_playback_at(&p, &query(2, identity().hz()), identity())
        .unwrap()
        .is_none());
    assert_eq!(io.query_calls.get(), 0);
    assert_eq!(state(&p), before);
    owner.current = Some(output);
}
#[test]
fn backend_failure_retains_exact_owner_and_cached_report() {
    let (mut owner, mut p, io, _producer) = fixture();
    let before = state(&p);
    let mixer = owner.current().unwrap().mixer.as_ref().unwrap() as *const Mixer;
    io.observations
        .borrow_mut()
        .push_back(Err(memory::Fault::new(71)));
    assert!(owner.observe_native_playback(&mut p, identity()).is_err());
    assert_eq!(state(&p), before);
    assert_eq!(
        owner.current().unwrap().mixer.as_ref().unwrap() as *const Mixer,
        mixer
    );
    assert_eq!(owner.render_report(), None);
}
#[test]
fn readonly_query_rejects_returned_epoch_basis_identity_and_exact_query_mismatch() {
    let (owner, p, io, _producer) = fixture();
    let basis = owner.current().unwrap().basis;
    let q = query(2, basis.sample_rate());
    let before = state(&p);
    for case in 0..4 {
        let (mut b, mut e, mut id, mut returned) = (basis, 7, identity(), q);
        match case {
            0 => e = 8,
            1 => b = OutputFrameBasis::new(point(2, 101), basis.sample_rate(), 0).unwrap(),
            2 => id = SourcePlaybackIdentity::new(basis.sample_rate(), 0, 0).unwrap(),
            _ => {
                returned = InterpolatedTargetTime::new(
                    TargetTime::from_frames(2, basis.sample_rate()).unwrap(),
                    0,
                    1,
                    u64::MAX,
                    1,
                    1,
                )
                .unwrap()
            }
        };
        if case == 2 {
            id = SourcePlaybackIdentity::new(basis.sample_rate(), 1, 0).unwrap();
            b = OutputFrameBasis::new(basis.origin(), basis.sample_rate(), 1).unwrap();
        }
        io.queries
            .borrow_mut()
            .push_back(Ok(Some(packet(b, e, id, returned, None))));
        assert!(
            owner.source_playback_at(&p, &q, identity()).is_err(),
            "case {case}"
        );
        assert_eq!(state(&p), before);
        assert_eq!(owner.render_report(), None);
    }
    io.queries.borrow_mut().push_back(Ok(Some(packet(
        basis,
        7,
        identity(),
        q,
        owner.current().unwrap().report,
    ))));
    let got = owner
        .source_playback_at(&p, &q, identity())
        .unwrap()
        .unwrap();
    assert_eq!(
        got.query().compare_interpolated_time(q),
        std::cmp::Ordering::Equal
    );
    assert_eq!(got.native_evidence().unwrap().words, [19, 31]);
    assert_eq!(state(&p), before);
    assert_eq!(owner.render_report(), None);
}
#[test]
fn descriptor_wrong_mode_and_creation_identity_refuse_before_io() {
    let (mut owner, _p, io, _producer) = fixture();
    let basis = owner.current().unwrap().basis;
    let mut p = presentation(basis, Some(2_000_000_000));
    let before = state(&p);
    io.observations.borrow_mut().push_back(Ok(Some(observation(
        basis,
        2,
        100,
        owner.current().unwrap().report,
    ))));
    assert!(owner.observe_native_playback(&mut p, identity()).is_err());
    assert_eq!(state(&p), before);
    assert_eq!(owner.render_report(), None);
    owner.current_mut().unwrap().epoch = 8;
    let calls = io.observed_calls.get();
    assert!(owner.observe_native_playback(&mut p, identity()).is_err());
    assert_eq!(io.observed_calls.get(), calls);
    assert_eq!(state(&p), before);
}

#[test]
fn distinct_valid_history_identity_with_identical_creation_basis_refuses_atomically() {
    for readonly in [true, false] {
        let (mut owner, mut p, io, _producer) = fixture_with_retained_prefix(true);
        let basis = owner.current().unwrap().basis;
        assert_eq!(basis.start_physical_frame(), 2);
        let history = owner
            .current()
            .unwrap()
            .mixer
            .as_ref()
            .unwrap()
            .source_playback_history()
            .unwrap();
        assert_eq!(history.initial_physical_frame(), 0);
        assert_eq!(history.pulled_end_frame(), 8);
        let original_report = owner.current().unwrap().report;
        io.observations.borrow_mut().push_back(Ok(Some(observation(
            basis,
            1,
            100,
            original_report,
        ))));
        owner.observe_native_playback(&mut p, identity()).unwrap();
        let before = state(&p);
        let cached = owner.render_report();
        assert!(cached.is_some());
        let wrong = SourcePlaybackIdentity::new(basis.sample_rate(), 1, 0).unwrap();
        let q = query(4, basis.sample_rate());
        let source = packet(basis, 7, wrong, q, original_report);
        // Both identities are mathematically valid on this same creation basis;
        // only the caller's retained-history identity distinguishes the packet.
        assert_eq!(source.basis(), basis);
        if readonly {
            io.queries.borrow_mut().push_back(Ok(Some(source)));
            assert!(owner.source_playback_at(&p, &q, identity()).is_err());
        } else {
            let coordinate = CoordinateNativeAudioSnapshot::new(
                NativeAudioSnapshot {
                    epoch: 7,
                    basis,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                        source: basis.point_at_stream_frame(2).unwrap(),
                        target: point(1, 101),
                    }),
                },
                NativeOutputCoordinate::StreamFrame { frame: 2 },
            )
            .unwrap();
            io.observations.borrow_mut().push_back(Ok(Some(
                NativePlaybackObservation::new(coordinate, source).unwrap(),
            )));
            assert!(owner.observe_native_playback(&mut p, identity()).is_err());
        }
        assert_eq!(state(&p), before);
        assert_eq!(owner.render_report(), cached);
        assert!(owner.current().unwrap().mixer.is_some());
    }
}

#[test]
fn coherent_counter_admission_retains_original_ticks_despite_flat_nanoseconds() {
    let (mut owner, _p, io, _producer) = fixture();
    let basis = owner.current().unwrap().basis;
    let mut p = presentation(basis, Some(2_000_000_000));
    for (tick, host, qpc) in [(2, 100, 1), (3, 101, 2)] {
        let raw = AudioStreamSnapshot {
            telemetry_available: true,
            status: AudioStreamStatus::Running,
            counters: StreamCounters::default(),
            render: None,
            clock: Some(AudioClockSnapshot {
                position: tick,
                frequency: 2_000_000_000,
                qpc_100ns: qpc,
                reading_quality: AudioClockReadingQuality::Accurate,
                host_point: Some(point(1, host)),
                mapping_quality: ClockMappingQuality::Unknown,
            }),
        };
        let coordinate = CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: 7,
                basis,
                evidence: OriginalNativePresentationEvidence::Wasapi {
                    snapshot: raw,
                    basis: Some(basis),
                },
            },
            NativeOutputCoordinate::Counter {
                position: tick,
                frequency: 2_000_000_000,
            },
        )
        .unwrap();
        let source = packet(
            basis,
            7,
            identity(),
            query(tick, 2_000_000_000),
            owner.current().unwrap().report,
        );
        io.observations.borrow_mut().push_back(Ok(Some(
            NativePlaybackObservation::new(coordinate, source).unwrap(),
        )));
        owner
            .observe_native_playback(&mut p, identity())
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        p.authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate(),
        Some(NativeOutputCoordinate::Counter {
            position: 3,
            frequency: 2_000_000_000
        })
    );
    assert_eq!(io.report_calls.get(), 0);
}

#[test]
fn returned_observation_wrong_published_epoch_or_basis_is_atomic() {
    let (mut owner, mut p, io, _producer) = fixture();
    let actual = owner.current().unwrap().basis;
    let before = state(&p);
    for case in 0..2 {
        let (epoch, basis) = if case == 0 {
            (8, actual)
        } else {
            (
                7,
                OutputFrameBasis::new(point(2, 101), actual.sample_rate(), 0).unwrap(),
            )
        };
        let coordinate = CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch,
                basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: basis.point_at_stream_frame(2).unwrap(),
                    target: point(1, 100),
                }),
            },
            NativeOutputCoordinate::StreamFrame { frame: 2 },
        )
        .unwrap();
        let source = packet(
            basis,
            epoch,
            identity(),
            query(2, basis.sample_rate()),
            owner.current().unwrap().report,
        );
        io.observations.borrow_mut().push_back(Ok(Some(
            NativePlaybackObservation::new(coordinate, source).unwrap(),
        )));
        assert!(owner.observe_native_playback(&mut p, identity()).is_err());
        assert_eq!(state(&p), before);
        assert_eq!(owner.render_report(), None);
        assert!(owner.current().unwrap().mixer.is_some());
    }
}

#[test]
fn existing_scalar_owner_observation_remains_on_legacy_path() {
    let (output, _producer, trace) = memory::initial(vec![]);
    let mut owner = GameplayOutputOwner::new(
        memory::Backend {
            trace: trace.clone(),
        },
        output,
    );
    let mut estimator = PresentationEstimator::new(
        memory::settings(),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    owner.current_mut().unwrap().render(2);
    owner.observe(&mut estimator).unwrap();
    assert_eq!(owner.render_report(), owner.current().unwrap().report);
    assert!(trace.borrow().calls.contains(&"observe"));
}

#[test]
fn identity_changed_during_readonly_or_mutable_io_refuses_before_admission() {
    for readonly in [true, false] {
        let (mut owner, mut p, io, _producer) = fixture();
        let basis = owner.current().unwrap().basis;
        let before = state(&p);
        let report = owner.current().unwrap().report;
        if readonly {
            io.mutate_query_identity.set(true);
            io.queries.borrow_mut().push_back(Ok(Some(packet(
                basis,
                7,
                identity(),
                query(2, basis.sample_rate()),
                report,
            ))));
            assert!(owner
                .source_playback_at(&p, &query(2, basis.sample_rate()), identity())
                .is_err());
        } else {
            io.mutate_observe_identity.set(true);
            io.observations
                .borrow_mut()
                .push_back(Ok(Some(observation(basis, 2, 100, report))));
            assert!(owner.observe_native_playback(&mut p, identity()).is_err());
        }
        assert_eq!(state(&p), before);
        assert_eq!(owner.render_report(), None);
        assert!(owner.current().unwrap().mixer.is_some());
        assert_eq!(io.reported_epoch.get(), Some(8));
    }
}
