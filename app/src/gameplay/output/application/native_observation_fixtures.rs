//! Actual static owner, native records and Mixers with an explicit memory-only IO port.
use super::fixtures as memory;
use super::*;
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    gameplay::output::{
        adapters::{
            remix::RemixedOutputBackend,
            switch::{OutputBackendSwitch, Switched},
        },
        ports::{OriginalNativeOutputBackend, OutputChannelRemixBackend},
    },
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
};
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, ChannelMatrix, CommandProducer, MixerConfig, OutputOpenFailure,
        PcmLimits, SampleBank, command_queue,
    },
    time::{
        ClockDomainId, Duration, ExtrapolationPolicy, Timestamp,
        presentation::PresentationEstimator,
    },
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
    asio::{AsioPresentationObservation, MultimediaHostInterval},
    presentation::validation::{NativePresentationValidator, OriginalNativePresentationEvidence},
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn raw(ns: i64) -> ClockPoint {
    point(2, ns)
}
fn presentation(capacity: usize) -> NativeAudioPresentation {
    NativeAudioPresentation::new(
        AudioAuthority::new(
            AudioAuthorityConfig {
                history_capacity: capacity,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
            AudioAuthorityEpoch {
                id: 0,
                stream_origin: raw(0),
                logical_origin: point(3, 0),
                host_domain: ClockDomainId(1),
            },
        )
        .unwrap(),
        NativePresentationValidator::new(0, raw(0), ClockDomainId(1)),
    )
    .unwrap()
}
fn supplied(basis: OutputFrameBasis, output: i64, host_ns: i64) -> NativeAudioSnapshot {
    NativeAudioSnapshot {
        epoch: 0,
        basis,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: raw(output),
            target: host(host_ns),
        }),
    }
}
fn wasapi(basis: OutputFrameBasis, output: u64, host_ns: i64) -> NativeAudioSnapshot {
    NativeAudioSnapshot {
        epoch: 0,
        basis,
        evidence: OriginalNativePresentationEvidence::Wasapi {
            snapshot: AudioStreamSnapshot {
                telemetry_available: true,
                status: AudioStreamStatus::Running,
                counters: StreamCounters::default(),
                render: None,
                clock: Some(AudioClockSnapshot {
                    position: output,
                    frequency: 1_000_000_000,
                    qpc_100ns: host_ns as u64 / 100,
                    reading_quality: AudioClockReadingQuality::Accurate,
                    host_point: Some(host(host_ns)),
                    mapping_quality: beatkernel::time::ClockMappingQuality::Unknown,
                }),
            },
            basis: Some(basis),
        },
    }
}
#[derive(Default)]
struct Io {
    snapshots: RefCell<VecDeque<Result<Option<NativeAudioSnapshot>, memory::Fault>>>,
    render_error: RefCell<Option<memory::Fault>>,
    native_calls: Cell<usize>,
    report_calls: Cell<usize>,
}
struct Backend {
    inner: memory::Backend,
    io: Rc<Io>,
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
        o.epoch
    }
    fn basis(&self, o: &Self::Output) -> OutputFrameBasis {
        o.basis
    }
    fn observe(
        &mut self,
        _: &mut Self::Output,
        _: &mut Self::Presentation,
    ) -> Result<(), Self::Error> {
        panic!("audio authority must never enter the legacy estimator observer")
    }
    fn render_report(&self, o: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        self.io.report_calls.set(self.io.report_calls.get() + 1);
        if let Some(error) = self.io.render_error.borrow_mut().take() {
            return Err(error);
        }
        self.inner.render_report(o)
    }
}
impl OriginalNativeOutputBackend for Backend {
    fn observe_native(
        &mut self,
        _: &mut Self::Output,
    ) -> Result<Option<NativeAudioSnapshot>, Self::Error> {
        self.io.native_calls.set(self.io.native_calls.get() + 1);
        self.io
            .snapshots
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None))
    }
}
impl OutputChannelRemixBackend for Backend {
    fn open_remixed(
        &mut self,
        _: Self::Request,
        _: Mixer,
        _: u64,
        _: ChannelMatrix,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        panic!("native observation forwarding must not open or remix the output")
    }
}
fn fixture(
    capacity: usize,
) -> (
    GameplayOutputOwner<Backend>,
    NativeAudioPresentation,
    Rc<Io>,
    CommandProducer,
) {
    let (mut output, producer, trace) = memory::initial(vec![]);
    output.render(10);
    let io = Rc::new(Io::default());
    let backend = Backend {
        inner: memory::Backend { trace },
        io: io.clone(),
    };
    (
        GameplayOutputOwner::new(backend, output),
        presentation(capacity),
        io,
        producer,
    )
}
fn accepted_state(p: &NativeAudioPresentation) -> String {
    format!("{:?}", (p.authority(), p.latest_record()))
}

#[test]
fn actual_owner_forwards_native_wasapi_before_cache_and_never_calls_legacy_observer() {
    let (mut owner, mut p, io, _producer) = fixture(4);
    let basis = owner.current().unwrap().basis;
    let original = wasapi(basis, 10_000_000, 10_000_000);
    io.snapshots.borrow_mut().push_back(Ok(Some(original)));
    owner.observe_native(&mut p).unwrap();
    assert_eq!(io.native_calls.get(), 1);
    assert_eq!(io.report_calls.get(), 1);
    assert_eq!(p.latest_record().unwrap().evidence(), &original.evidence);
    assert_eq!(
        p.authority().latest_observation(),
        Some(ClockPair {
            source: raw(10_000_000),
            target: host(10_000_000)
        })
    );
    assert_eq!(owner.render_report(), owner.current().unwrap().report);
    assert_eq!(
        owner.audio_pause_observation(&p, host(11_000_000)).unwrap(),
        LivePauseObservation::Point(ClockPair {
            source: raw(10_000_000),
            target: host(10_000_000)
        })
    );
}

#[test]
fn render_read_failure_occurs_before_admission_and_preserves_prior_cache_and_typed_error() {
    let (mut owner, mut p, io, _producer) = fixture(4);
    let basis = owner.current().unwrap().basis;
    io.snapshots
        .borrow_mut()
        .push_back(Ok(Some(supplied(basis, 10_000_000, 10_000_000))));
    owner.observe_native(&mut p).unwrap();
    let before = accepted_state(&p);
    let cached = owner.render_report();
    owner.current_mut().unwrap().render(10);
    io.snapshots
        .borrow_mut()
        .push_back(Ok(Some(supplied(basis, 20_000_000, 20_000_000))));
    let error = memory::Fault::new(501);
    let pointer = error.pointer();
    *io.render_error.borrow_mut() = Some(error);
    let failure = owner.observe_native(&mut p).unwrap_err();
    let ReplacementCause::Backend { phase, error } = failure.cause else {
        panic!("expected typed backend read refusal")
    };
    assert_eq!(phase, ReplacementPhase::Observe);
    assert_eq!(error.pointer(), pointer);
    assert_eq!(accepted_state(&p), before);
    assert_eq!(owner.render_report(), cached);
}

#[test]
fn waiting_none_and_missing_render_retain_reports_without_fabricating_presentation() {
    let (mut owner, mut p, io, _producer) = fixture(4);
    let before = accepted_state(&p);
    owner.observe_native(&mut p).unwrap();
    assert_eq!(accepted_state(&p), before);
    let cached = owner.render_report();
    assert!(cached.is_some());
    let basis = owner.current().unwrap().basis;
    io.snapshots
        .borrow_mut()
        .push_back(Ok(Some(supplied(basis, 10_000_000, 10_000_000))));
    owner.current_mut().unwrap().report = None;
    owner.observe_native(&mut p).unwrap();
    assert_eq!(owner.render_report(), cached);
    let cached_pause = owner.audio_pause_observation(&p, host(11_000_000)).unwrap();
    let calls = io.native_calls.get();
    let _held = owner.current.take();
    let before = accepted_state(&p);
    owner.observe_native(&mut p).unwrap();
    assert_eq!(io.native_calls.get(), calls);
    assert_eq!(accepted_state(&p), before);
    assert_eq!(owner.render_report(), cached);
    assert_eq!(
        owner.audio_pause_observation(&p, host(12_000_000)).unwrap(),
        cached_pause
    );
}

#[test]
fn published_identity_snapshot_faults_and_pinned_history_refuse_without_cached_publication() {
    let (mut owner, mut p, io, _producer) = fixture(2);
    let basis = owner.current().unwrap().basis;
    owner.current_mut().unwrap().epoch = 1;
    let before = accepted_state(&p);
    assert!(owner.observe_native(&mut p).is_err());
    assert_eq!(io.native_calls.get(), 0);
    assert_eq!(io.report_calls.get(), 0);
    assert_eq!(accepted_state(&p), before);
    owner.current_mut().unwrap().epoch = 0;
    for mut dto in [
        supplied(basis, 10_000_000, 10_000_000),
        supplied(
            OutputFrameBasis::new(raw(0), 2000, 0).unwrap(),
            10_000_000,
            10_000_000,
        ),
    ] {
        if dto.basis == basis {
            dto.epoch = 1;
        }
        io.snapshots.borrow_mut().push_back(Ok(Some(dto)));
        let before = accepted_state(&p);
        let cached = owner.render_report();
        assert!(owner.observe_native(&mut p).is_err());
        assert_eq!(accepted_state(&p), before);
        assert_eq!(owner.render_report(), cached);
    }
    for ns in [10_000_000, 20_000_000] {
        if ns == 20_000_000 {
            owner.current_mut().unwrap().render(10);
        }
        io.snapshots
            .borrow_mut()
            .push_back(Ok(Some(supplied(basis, ns, ns))));
        owner.observe_native(&mut p).unwrap();
    }
    let before = accepted_state(&p);
    let cached = owner.render_report();
    let calls = (io.native_calls.get(), io.report_calls.get());
    owner.current_mut().unwrap().basis = OutputFrameBasis::new(raw(0), 2000, 0).unwrap();
    assert!(owner.observe_native(&mut p).is_err());
    assert_eq!((io.native_calls.get(), io.report_calls.get()), calls);
    assert_eq!(accepted_state(&p), before);
    assert_eq!(owner.render_report(), cached);
    owner.current_mut().unwrap().basis = basis;
    let before = accepted_state(&p);
    let cached = owner.render_report();
    owner.current_mut().unwrap().render(10);
    io.snapshots
        .borrow_mut()
        .push_back(Ok(Some(supplied(basis, 30_000_000, 30_000_000))));
    assert!(matches!(
        owner.observe_native(&mut p).unwrap_err().cause,
        ReplacementCause::Timing(_)
    ));
    assert_eq!(accepted_state(&p), before);
    assert_eq!(owner.render_report(), cached);
    io.snapshots
        .borrow_mut()
        .push_back(Ok(Some(supplied(basis, 19_000_000, 19_000_000))));
    assert!(owner.observe_native(&mut p).is_err());
    assert_eq!(accepted_state(&p), before);
    assert_eq!(owner.render_report(), cached);
}

#[test]
fn actual_remix_and_both_switched_branches_forward_original_records_and_exact_errors() {
    let (mut first_output, _first_producer, first_trace) = memory::initial(vec![]);
    first_output.render(10);
    let (mut second_output, _second_producer, second_trace) = memory::initial(vec![]);
    second_output.render(10);
    let first_io = Rc::new(Io::default());
    let second_io = Rc::new(Io::default());
    let first = Backend {
        inner: memory::Backend { trace: first_trace },
        io: first_io.clone(),
    };
    let second = Backend {
        inner: memory::Backend {
            trace: second_trace,
        },
        io: second_io.clone(),
    };
    let mut switched = OutputBackendSwitch::new(first, second);
    let first_dto = wasapi(first_output.basis, 10_000_000, 10_000_000);
    first_io
        .snapshots
        .borrow_mut()
        .push_back(Ok(Some(first_dto)));
    let mut first_output = Switched::First(first_output);
    assert_eq!(
        switched.observe_native(&mut first_output).unwrap(),
        Some(first_dto)
    );
    let second_dto = wasapi(second_output.basis, 10_000_000, 20_000_000);
    second_io
        .snapshots
        .borrow_mut()
        .push_back(Ok(Some(second_dto)));
    let mut second_output = Switched::Second(second_output);
    assert_eq!(
        switched.observe_native(&mut second_output).unwrap(),
        Some(second_dto)
    );
    for (io, output, first_branch) in [
        (first_io.clone(), &mut first_output, true),
        (second_io.clone(), &mut second_output, false),
    ] {
        let error = memory::Fault::new(701);
        let pointer = error.pointer();
        io.snapshots.borrow_mut().push_back(Err(error));
        let error = switched.observe_native(output).unwrap_err();
        match error {
            Switched::First(error) => {
                assert!(first_branch);
                assert_eq!(error.pointer(), pointer)
            }
            Switched::Second(error) => {
                assert!(!first_branch);
                assert_eq!(error.pointer(), pointer)
            }
        }
    }
    let (mut output, _producer, trace) = memory::initial(vec![]);
    output.render(10);
    let io = Rc::new(Io::default());
    let mut remix = RemixedOutputBackend::new(Backend {
        inner: memory::Backend { trace },
        io: io.clone(),
    });
    let original = wasapi(output.basis, 10_000_000, 10_000_000);
    io.snapshots.borrow_mut().push_back(Ok(Some(original)));
    assert_eq!(remix.observe_native(&mut output).unwrap(), Some(original));
    let error = memory::Fault::new(702);
    let pointer = error.pointer();
    io.snapshots.borrow_mut().push_back(Err(error));
    assert_eq!(
        remix.observe_native(&mut output).unwrap_err().pointer(),
        pointer
    );
    assert_eq!(io.native_calls.get(), 2);
}

#[test]
fn original_asio_pause_and_end_use_actual_full_intervals_and_upper_endpoint() {
    let trace = Rc::new(RefCell::new(memory::Trace::default()));
    let io = Rc::new(Io::default());
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(16).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(16, 2, 8, 64, 8).unwrap(),
        )
        .with_playback_end_frame(10),
        bank,
        consumer,
    )
    .unwrap();
    let basis = mixer.output_frame_basis();
    let output = memory::Output {
        mixer: Some(mixer),
        epoch: 0,
        basis,
        report: None,
        quiet: 0,
        retired: false,
        trace: trace.clone(),
    };
    let mut owner = GameplayOutputOwner::new(
        Backend {
            inner: memory::Backend { trace },
            io: io.clone(),
        },
        output,
    );
    let mut p = presentation(4);
    let mut end = NativeEnd::new(raw(0), ClockDomainId(1), 1000, 10).unwrap();
    assert!(owner.observe_audio_end(&mut end, &p).unwrap().is_none());
    assert!(owner.audio_pause_observation(&p, host(0)).is_err());
    for (frames, before, after) in [
        (4, 1_000_000, 2_000_000),
        (8, 4_000_000, 6_000_000),
        (4, 12_000_000, 16_000_000),
    ] {
        owner.current_mut().unwrap().render(frames);
        let render = owner.current().unwrap().report.unwrap();
        let original = AsioPresentationObservation::from_render(
            render,
            1000,
            MultimediaHostInterval {
                before: host(before),
                after: host(after),
            },
            0,
            0,
            raw(0),
        )
        .unwrap();
        // ASIO's complete original report is authoritative even when a
        // separate backend report getter has no new telemetry.
        owner.current_mut().unwrap().report = None;
        io.snapshots
            .borrow_mut()
            .push_back(Ok(Some(NativeAudioSnapshot {
                epoch: 0,
                basis,
                evidence: OriginalNativePresentationEvidence::Asio {
                    observation: original,
                    basis: Some(basis),
                },
            })));
        owner.observe_native(&mut p).unwrap();
        assert_eq!(owner.render_report(), Some(render));
        let pause = owner.audio_pause_observation(&p, host(after + 1)).unwrap();
        let LivePauseObservation::Interval {
            observation: Some(interval),
            now,
        } = pause
        else {
            panic!("ASIO must retain interval evidence")
        };
        assert_eq!(interval.render, render);
        assert_eq!(interval.clock.before, host(before));
        assert_eq!(interval.clock.after, host(after));
        assert_eq!(now, host(after + 1));
        let boundary = owner.observe_audio_end(&mut end, &p).unwrap();
        if after < 16_000_000 {
            assert!(boundary.is_none());
        } else {
            let boundary = boundary.unwrap();
            assert_eq!(boundary.host, host(16_000_000));
            assert_ne!(boundary.host, host(14_000_000));
            assert_eq!(boundary.output, raw(10_000_000));
            assert_eq!(boundary.physical_frame, 10);
        }
    }
}

#[test]
fn embedded_asio_report_does_not_bypass_render_read_failure_before_admission() {
    let (mut owner, mut p, io, _producer) = fixture(4);
    let basis = owner.current().unwrap().basis;
    let first = AsioPresentationObservation::from_render(
        owner.current().unwrap().report.unwrap(),
        1000,
        MultimediaHostInterval {
            before: host(0),
            after: host(2_000_000),
        },
        0,
        0,
        raw(0),
    )
    .unwrap();
    io.snapshots
        .borrow_mut()
        .push_back(Ok(Some(NativeAudioSnapshot {
            epoch: 0,
            basis,
            evidence: OriginalNativePresentationEvidence::Asio {
                observation: first,
                basis: Some(basis),
            },
        })));
    owner.observe_native(&mut p).unwrap();
    let before = accepted_state(&p);
    let cached = owner.render_report();
    owner.current_mut().unwrap().render(10);
    let next = AsioPresentationObservation::from_render(
        owner.current().unwrap().report.unwrap(),
        1000,
        MultimediaHostInterval {
            before: host(12_000_000),
            after: host(14_000_000),
        },
        0,
        0,
        raw(0),
    )
    .unwrap();
    io.snapshots
        .borrow_mut()
        .push_back(Ok(Some(NativeAudioSnapshot {
            epoch: 0,
            basis,
            evidence: OriginalNativePresentationEvidence::Asio {
                observation: next,
                basis: Some(basis),
            },
        })));
    let error = memory::Fault::new(801);
    let pointer = error.pointer();
    *io.render_error.borrow_mut() = Some(error);
    let failure = owner.observe_native(&mut p).unwrap_err();
    let ReplacementCause::Backend { phase, error } = failure.cause else {
        panic!("expected backend render refusal")
    };
    assert_eq!(phase, ReplacementPhase::Observe);
    assert_eq!(error.pointer(), pointer);
    assert_eq!(accepted_state(&p), before);
    assert_eq!(owner.render_report(), cached);
    assert_eq!(p.authority().history_len(), 1);
}
