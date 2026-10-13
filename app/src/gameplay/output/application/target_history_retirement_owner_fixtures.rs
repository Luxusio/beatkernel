//! Real converted histories and one static compound effect port, not hardware proof.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    gameplay::output::ports::{
        NativeTargetPlaybackFacts, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
        OriginalCoordinateTargetNativeOutputBackend, TargetPlaybackNativeOutputBackend,
        TargetOutputTelemetry, TargetHistoryRetirementNativeOutputBackend,
    },
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_audio_startup::{new_coordinate_audio_presentation, new_target_audio_presentation},
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::ConvertedPlaybackFacts,
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io,
    rc::Rc,
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
// Intentionally no Clone, Copy, Debug, or Error implementation.
struct Evidence;
#[derive(Default)]
struct Io {
    observations:
        RefCell<VecDeque<Result<Option<NativeTargetPlaybackObservation<Evidence>>, io::Error>>>,
    queries: RefCell<VecDeque<Result<Option<NativeTargetPlaybackQuery<Evidence>>, io::Error>>>,
    observed: Cell<usize>,
    queried: Cell<usize>,
    mutate_epoch: Cell<bool>,
    mutate_basis: Cell<bool>,
    requests: RefCell<
        Vec<(
            u64,
            TargetFrameBasis,
            SourcePlaybackIdentity,
            u64,
            TargetTime,
        )>,
    >,
    replies: RefCell<VecDeque<Result<Option<u64>, io::Error>>>,
}
struct Output {
    converted: Option<ConvertedNativeOutputState>,
    epoch: u64,
    basis: TargetFrameBasis,
    retired: bool,
}
impl StoppedMixerSource<ConvertedNativeOutputState> for Output {
    type Error = io::Error;
    fn take_stopped_mixer(&mut self) -> Result<Option<ConvertedNativeOutputState>, Self::Error> {
        if self.retired {
            Ok(self.converted.take())
        } else {
            Err(io::Error::other("still running"))
        }
    }
}
struct Backend {
    io: Rc<Io>,
}
impl OutputReplacementBackend<ConvertedNativeOutputState, TargetFrameBasis> for Backend {
    type Presentation = beatkernel::time::presentation::PresentationEstimator;
    type Output = Output;
    type Request = ();
    type Error = io::Error;
    fn open(
        &mut self,
        _: (),
        converted: ConvertedNativeOutputState,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<io::Error, Output, ConvertedNativeOutputState>> {
        let basis = converted.target_frame_basis();
        Ok(Output {
            converted: Some(converted),
            epoch,
            basis,
            retired: false,
        })
    }
    fn retire(&mut self, o: &mut Output) -> Result<(), io::Error> {
        o.retired = true;
        Ok(())
    }
    fn start(&mut self, _: &mut Output) -> Result<(), io::Error> {
        Ok(())
    }
    fn epoch(&self, o: &Output) -> u64 {
        o.epoch
    }
    fn basis(&self, o: &Output) -> TargetFrameBasis {
        o.basis
    }
    fn observe(&mut self, _: &mut Output, _: &mut Self::Presentation) -> Result<(), io::Error> {
        panic!("scalar observation fallback")
    }
    fn render_report(&self, _: &Output) -> Result<Option<RenderReport>, io::Error> {
        panic!("independent source report fallback")
    }
}
impl OriginalTargetNativeOutputBackend<ConvertedNativeOutputState> for Backend {
    fn planned_target_basis(
        &self,
        _: &(),
        o: &ConvertedNativeOutputState,
    ) -> Result<TargetFrameBasis, io::Error> {
        Ok(o.target_frame_basis())
    }
    fn observe_native_target(
        &mut self,
        _: &mut Output,
    ) -> Result<Option<TargetNativeAudioSnapshot>, io::Error> {
        panic!("independent target observation fallback")
    }
    fn converted_report(&self, _: &Output) -> Result<Option<ConvertedRenderReport>, io::Error> {
        panic!("independent converted report fallback")
    }
    fn boundary_facts(&self, _: &Output) -> Result<ConvertedBoundaryFacts, io::Error> {
        panic!("independent boundaries fallback")
    }
    fn output_telemetry(&self, _: &Output) -> Result<Option<TargetOutputTelemetry>, io::Error> {
        panic!("independent telemetry fallback")
    }
    fn set_held(&mut self, _: &mut Output, _: bool) -> Result<(), io::Error> {
        panic!("premature pause ACK")
    }
}
impl OriginalCoordinateTargetNativeOutputBackend<ConvertedNativeOutputState> for Backend {
    fn observe_native_target_coordinate(
        &mut self,
        _: &mut Output,
    ) -> Result<Option<CoordinateTargetNativeAudioSnapshot>, io::Error> {
        panic!("independent coordinate fallback")
    }
}
impl TargetPlaybackNativeOutputBackend<ConvertedNativeOutputState> for Backend {
    type NativeEvidence = Evidence;
    fn observe_native_target_playback(
        &mut self,
        o: &mut Output,
        _: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackObservation<Evidence>>, io::Error> {
        self.io.observed.set(self.io.observed.get() + 1);
        let result = self
            .io
            .observations
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None));
        if self.io.mutate_epoch.get() {
            o.epoch += 1;
        }
        if self.io.mutate_basis.get() {
            o.basis = TargetFrameBasis::new(o.basis.origin(), o.basis.start_time(), 14).unwrap();
        }
        result
    }
    fn target_playback_at(
        &self,
        _: &Output,
        _: &InterpolatedTargetTime,
        _: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackQuery<Evidence>>, io::Error> {
        self.io.queried.set(self.io.queried.get() + 1);
        self.io.queries.borrow_mut().pop_front().unwrap_or(Ok(None))
    }
}
type Owner = GameplayOutputOwner<Backend, ConvertedNativeOutputState, TargetFrameBasis>;
struct Rig {
    owner: Owner,
    p: NativeAudioPresentation,
    io: Rc<Io>,
    id: SourcePlaybackIdentity,
    facts: NativeTargetPlaybackFacts,
    _producer: CommandProducer,
}
fn rig() -> Rig {
    let format = AudioFormat::new(3, 1).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
            AudioLimits::new(8, 2, 8, 64, 8).unwrap(),
        ),
        SampleBank::new(format, PcmLimits::new(128, 256, 1).unwrap()).unwrap(),
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 1]).unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    mixer.render(&mut [0.0; 1]).unwrap();
    let id = SourcePlaybackIdentity::new(3, 1, 1).unwrap();
    let mut converted = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(7, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("valid converter"));
    converted.enable_phase_history(8).unwrap();
    let basis = converted.target_frame_basis();
    let report = converted.render_pending(7).unwrap();
    converted.admit(5).unwrap();
    let history = converted.phase_history().unwrap();
    let playback = converted
        .converter_owner()
        .source_playback_history()
        .unwrap()
        .lookup_at_source_time(exact(TargetTime::from_frames(2, 3).unwrap()))
        .unwrap();
    let facts = NativeTargetPlaybackFacts {
        playback: ConvertedPlaybackFacts {
            phase: history.span(0).unwrap(),
            segment_end: history.end_time(),
            generated_target_end: history.end_time(),
            submitted_target_end: converted.admitted_target_time().unwrap(),
            playback,
            pulled_source_end: converted.mixer().frame_cursor(),
            revision: 6,
        },
        source: converted.last_real_source_report(),
        converted: Some(report),
        boundaries: converted.boundaries(),
        revision: 20,
    };
    let io = Rc::new(Io::default());
    let owner = GameplayOutputOwner::new(
        Backend { io: io.clone() },
        Output {
            converted: Some(converted),
            epoch: 19,
            basis,
            retired: false,
        },
    );
    let p = new_coordinate_audio_presentation(
        19,
        AudioAuthorityCoordinateBasis::TargetFrame(basis),
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    Rig {
        owner,
        p,
        io,
        id,
        facts,
        _producer: producer,
    }
}
fn packet(
    r: &Rig,
    epoch: u64,
    basis: TargetFrameBasis,
    id: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    facts: NativeTargetPlaybackFacts,
) -> NativeTargetPlaybackQuery<Evidence> {
    let _ = r;
    NativeTargetPlaybackQuery::new(epoch, basis, id, query, facts, Some(Evidence)).unwrap()
}
fn query(r: &Rig, frame: u64) -> InterpolatedTargetTime {
    exact(
        r.owner
            .basis
            .start_time()
            .checked_add_frames(frame, 7)
            .unwrap(),
    )
}
fn observation(
    r: &Rig,
    frame: u64,
    host: i64,
    facts: NativeTargetPlaybackFacts,
) -> NativeTargetPlaybackObservation<Evidence> {
    let basis = r.owner.basis;
    NativeTargetPlaybackObservation::new(
        CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
                epoch: 19,
                basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: basis.point_at_stream_frame(frame).unwrap(),
                    target: point(1, host),
                }),
            },
            NativeOutputCoordinate::StreamFrame { frame },
        )
        .unwrap(),
        packet(r, 19, basis, r.id, query(r, frame), facts),
    )
    .unwrap()
}
fn push(r: &Rig, observation: NativeTargetPlaybackObservation<Evidence>) {
    r.io.observations
        .borrow_mut()
        .push_back(Ok(Some(observation)));
}
fn cache(
    r: &Rig,
) -> (
    Option<RenderReport>,
    Option<ConvertedRenderReport>,
    Option<ConvertedBoundaryFacts>,
) {
    (
        r.owner.render_report(),
        r.owner.converted_report(),
        r.owner.target_boundary_facts(),
    )
}
fn seed(r: &mut Rig) {
    push(r, observation(r, 1, 1_000_000_000, r.facts));
    r.owner
        .observe_target_native_playback(&mut r.p, r.id)
        .unwrap()
        .unwrap();
}

impl TargetHistoryRetirementNativeOutputBackend<ConvertedNativeOutputState> for Backend {
    fn request_target_history_retirement(
        &mut self,
        _: &mut Output,
        epoch: u64,
        basis: TargetFrameBasis,
        id: SourcePlaybackIdentity,
        source_pin: u64,
        target_pin: TargetTime,
    ) -> Result<Option<u64>, io::Error> {
        self.io
            .requests
            .borrow_mut()
            .push((epoch, basis, id, source_pin, target_pin));
        self.io
            .replies
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(Some(2)))
    }
}
fn enqueue(r: &Rig, q: InterpolatedTargetTime) {
    r.io.queries
        .borrow_mut()
        .push_back(Ok(Some(packet(r, 19, r.owner.basis, r.id, q, r.facts))));
}
fn assert_unchanged(
    r: &Rig,
    before: &str,
    caches: (
        Option<RenderReport>,
        Option<ConvertedRenderReport>,
        Option<ConvertedBoundaryFacts>,
    ),
    observed: usize,
) {
    assert_eq!(format!("{:?}", r.p), before);
    assert_eq!(cache(r), caches);
    assert_eq!(r.io.observed.get(), observed);
}
#[test]
fn exact_covering_spans_dispatch_without_source_sampling_floor_or_new_observation() {
    let mut r = rig();
    seed(&mut r);
    let q = query(&r, 1);
    enqueue(&r, q);
    let before = format!("{:?}", r.p);
    let caches = cache(&r);
    let owner = r.owner.current().unwrap().converted.as_ref().unwrap();
    let next_floor = owner.converter_owner().source_position().frame;
    let physical = r.facts.playback.playback.physical_start();
    assert!(
        physical < next_floor,
        "covering source span predates next sampling position"
    );
    assert_eq!(r.id.initial_physical(), 1);
    assert_eq!(
        r.owner.basis.start_time(),
        TargetTime::from_frames(2, 3).unwrap()
    );
    assert_eq!(
        r.owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        &*r.io.requests.borrow(),
        &[(
            19,
            r.owner.basis,
            r.id,
            physical,
            r.owner.basis.start_time()
        )]
    );
    assert_eq!(r.io.queried.get(), 1);
    assert_unchanged(&r, &before, caches, 1);
}
#[test]
fn exact_subnanosecond_query_uses_original_span_start() {
    let mut r = rig();
    let q = InterpolatedTargetTime::new(
        r.owner.basis.start_time().checked_add_frames(1, 7).unwrap(),
        0,
        1,
        u64::MAX,
        1,
        1,
    )
    .unwrap();
    let floor = query(&r, 1);
    assert_eq!(
        q.point(r.owner.basis.origin()).unwrap(),
        floor.point(r.owner.basis.origin()).unwrap()
    );
    assert!(!q.compare_interpolated_time(floor).is_eq());
    enqueue(&r, q);
    assert_eq!(
        r.owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .unwrap(),
        Some(2)
    );
    assert_eq!(r.io.requests.borrow()[0].4, r.owner.basis.start_time());
    assert_eq!(r.io.requests.borrow()[0].3, 1);
    assert_eq!(r.io.observed.get(), 0);
    assert_eq!(r.p.authority().history_len(), 0);
}
#[test]
fn semantically_equal_query_encoding_is_accepted() {
    let mut r = rig();
    let q = query(&r, 1);
    let equal = InterpolatedTargetTime::new(r.owner.basis.start_time(), 0, 1, 7, 1, 1).unwrap();
    assert!(q.compare_interpolated_time(equal).is_eq());
    enqueue(&r, equal);
    assert_eq!(
        r.owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .unwrap(),
        Some(2)
    );
    assert_eq!(r.io.requests.borrow().len(), 1);
}
#[test]
fn absent_owner_or_packet_never_dispatches() {
    for missing_owner in [true, false] {
        let mut r = rig();
        let q = query(&r, 1);
        if missing_owner {
            r.owner.current.take();
        }
        let before = format!("{:?}", r.p);
        assert!(r
            .owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .unwrap()
            .is_none());
        assert_eq!(r.io.queried.get(), if missing_owner { 0 } else { 1 });
        assert!(r.io.requests.borrow().is_empty());
        assert_unchanged(&r, &before, (None, None, None), 0);
    }
}
#[test]
fn delegate_busy_and_error_preserve_presentation_and_report_caches() {
    for fail in [false, true] {
        let mut r = rig();
        seed(&mut r);
        let q = query(&r, 1);
        enqueue(&r, q);
        r.io.replies.borrow_mut().push_back(if fail {
            Err(io::Error::other("queued port failed"))
        } else {
            Ok(None)
        });
        let before = format!("{:?}", r.p);
        let caches = cache(&r);
        let result = r.owner.request_target_history_retirement(&r.p, &q, r.id);
        if fail {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_none());
        }
        assert_eq!(r.io.requests.borrow().len(), 1);
        assert_eq!(r.io.queried.get(), 1);
        assert_unchanged(&r, &before, caches, 1);
    }
}
#[test]
fn exact_query_lookup_error_never_queues_or_changes_state() {
    let mut r = rig();
    seed(&mut r);
    let q = query(&r, 1);
    let before = format!("{:?}", r.p);
    let caches = cache(&r);
    r.io.queries
        .borrow_mut()
        .push_back(Err(io::Error::other("history unavailable")));
    assert!(r
        .owner
        .request_target_history_retirement(&r.p, &q, r.id)
        .is_err());
    assert!(r.io.requests.borrow().is_empty());
    assert_unchanged(&r, &before, caches, 1);
}
#[test]
fn wrong_packet_epoch_basis_source_identity_and_floor_aliased_query_refuse_before_queue() {
    for case in 0..4 {
        let mut r = rig();
        let q = query(&r, 1);
        let mut facts = r.facts;
        let epoch = if case == 0 { 20 } else { 19 };
        let basis = if case == 1 {
            facts.converted = None;
            TargetFrameBasis::new(r.owner.basis.origin(), r.owner.basis.start_time(), 14).unwrap()
        } else {
            r.owner.basis
        };
        let id = if case == 2 {
            SourcePlaybackIdentity::new(3, 0, 0).unwrap()
        } else {
            r.id
        };
        let actual = if case == 3 {
            InterpolatedTargetTime::new(
                r.owner.basis.start_time().checked_add_frames(1, 7).unwrap(),
                0,
                1,
                u64::MAX,
                1,
                1,
            )
            .unwrap()
        } else {
            q
        };
        r.io.queries
            .borrow_mut()
            .push_back(Ok(Some(packet(&r, epoch, basis, id, actual, facts))));
        let before = format!("{:?}", r.p);
        assert!(r
            .owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .is_err());
        assert_eq!(r.io.queried.get(), 1);
        assert!(r.io.requests.borrow().is_empty());
        assert_unchanged(&r, &before, (None, None, None), 0);
    }
}
#[test]
fn wrong_current_output_epoch_or_basis_refuses_before_history_lookup() {
    for epoch in [true, false] {
        let mut r = rig();
        let q = query(&r, 1);
        if epoch {
            r.owner.current_mut().unwrap().epoch = 20;
        } else {
            r.owner.current_mut().unwrap().basis =
                TargetFrameBasis::new(r.owner.basis.origin(), r.owner.basis.start_time(), 14)
                    .unwrap();
        }
        assert!(r
            .owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .is_err());
        assert_eq!(r.io.queried.get(), 0);
        assert!(r.io.requests.borrow().is_empty());
        assert_eq!(r.io.observed.get(), 0);
    }
}
#[test]
fn wrong_presentation_modes_epoch_and_exact_descriptor_refuse_before_lookup() {
    for case in 0..5 {
        let mut r = rig();
        let b = r.owner.basis;
        let q = query(&r, 1);
        r.p = match case {
            0 => new_coordinate_audio_presentation(
                20,
                AudioAuthorityCoordinateBasis::TargetFrame(b),
                ClockDomainId(1),
                point(3, 500),
                AudioAuthorityConfig::default(),
            )
            .unwrap(),
            1 => new_coordinate_audio_presentation(
                19,
                AudioAuthorityCoordinateBasis::TargetFrame(
                    TargetFrameBasis::new(b.origin(), b.start_time(), 14).unwrap(),
                ),
                ClockDomainId(1),
                point(3, 500),
                AudioAuthorityConfig::default(),
            )
            .unwrap(),
            2 => new_coordinate_audio_presentation(
                19,
                AudioAuthorityCoordinateBasis::SourceFrame(
                    OutputFrameBasis::new(b.point_at_stream_frame(0).unwrap(), 3, 0).unwrap(),
                ),
                ClockDomainId(1),
                point(3, 500),
                AudioAuthorityConfig::default(),
            )
            .unwrap(),
            3 => new_target_audio_presentation(
                19,
                b,
                ClockDomainId(1),
                point(3, 500),
                AudioAuthorityConfig::default(),
            )
            .unwrap(),
            _ => new_coordinate_audio_presentation(
                19,
                AudioAuthorityCoordinateBasis::TargetFrame(
                    TargetFrameBasis::new(point(8, 100), b.start_time(), 7).unwrap(),
                ),
                ClockDomainId(1),
                point(3, 500),
                AudioAuthorityConfig::default(),
            )
            .unwrap(),
        };
        let before = format!("{:?}", r.p);
        assert!(r
            .owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .is_err());
        assert_eq!(r.io.queried.get(), 0);
        assert!(r.io.requests.borrow().is_empty());
        assert_unchanged(&r, &before, (None, None, None), 0);
    }
}
#[test]
fn admitted_source_rate_or_origin_mismatch_refuses_before_lookup() {
    for origin in [true, false] {
        let mut r = rig();
        r.owner.target_facts = Some(if origin {
            ConvertedBoundaryFacts {
                origin: Some(point(2, 101)),
                ..r.facts.boundaries
            }
        } else {
            ConvertedBoundaryFacts {
                source_rate: 4,
                ..r.facts.boundaries
            }
        });
        let q = query(&r, 1);
        let before = format!("{:?}", r.p);
        let caches = cache(&r);
        assert!(r
            .owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .is_err());
        assert!(r.io.requests.borrow().is_empty());
        assert_eq!(r.io.queried.get(), 0);
        assert_unchanged(&r, &before, caches, 0);
    }
}

#[test]
fn actual_held_span_dispatches_its_fractional_start_and_real_covering_source_span() {
    let mut r = rig();
    let (report, phase, end, playback, pulled, boundaries) = {
        let state = r.owner.current_mut().unwrap().converted.as_mut().unwrap();
        state.admit(2).unwrap();
        let report = state.render_held_pending(3).unwrap();
        state.admit(3).unwrap();
        let history = state.phase_history().unwrap();
        let phase = history.span(history.len() - 1).unwrap();
        let q = exact(report.target_start_time.checked_add_frames(1, 7).unwrap());
        assert_eq!(
            history.lookup(report.target_start_time).unwrap().state(),
            ConvertedOutputState::Held
        );
        let end = history.end_time();
        let playback = state
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .lookup_at_interpolated_time(phase, q, end)
            .unwrap();
        (
            report,
            phase,
            end,
            playback,
            state.mixer().frame_cursor(),
            state.boundaries(),
        )
    };
    let q = exact(report.target_start_time.checked_add_frames(1, 7).unwrap());
    r.facts.playback = ConvertedPlaybackFacts {
        phase,
        segment_end: end,
        generated_target_end: end,
        submitted_target_end: end,
        playback,
        pulled_source_end: pulled,
        revision: 8,
    };
    r.facts.source = report.source;
    r.facts.converted = Some(report);
    r.facts.boundaries = boundaries;
    enqueue(&r, q);
    let before = format!("{:?}", r.p);
    assert_eq!(
        r.owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        &*r.io.requests.borrow(),
        &[(
            19,
            r.owner.basis,
            r.id,
            playback.physical_start(),
            report.target_start_time
        )]
    );
    assert!(report.target_start_time.numerator() != 0);
    assert_unchanged(&r, &before, (None, None, None), 0);
}

#[test]
fn queued_retirement_preserves_an_actual_prepared_coordinate_input_token() {
    let mut r = rig();
    seed(&mut r);
    push(&r, observation(&r, 2, 1_100_000_000, r.facts));
    r.owner
        .observe_target_native_playback(&mut r.p, r.id)
        .unwrap()
        .unwrap();
    let now = point(1, 1_100_000_000);
    r.p.authority_mut().record_acquired_prefix(now).unwrap();
    let prepared =
        r.p.authority()
            .prepare_coordinate_input(point(1, 1_050_000_000), now)
            .unwrap()
            .unwrap();
    let q = prepared.query();
    enqueue(&r, q);
    let before = format!("{:?}", r.p);
    let caches = cache(&r);
    assert_eq!(
        r.owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .unwrap(),
        Some(2)
    );
    assert_unchanged(&r, &before, caches, 2);
    r.p.authority()
        .validate_coordinate_input(&prepared, now)
        .unwrap();
}

#[test]
fn recovered_stream_start_does_not_replace_older_actual_covering_phase_start() {
    let mut r = rig();
    let original = r.owner.basis.start_time();
    let later = TargetFrameBasis::new(
        r.owner.basis.origin(),
        original.checked_add_frames(1, 7).unwrap(),
        7,
    )
    .unwrap();
    r.owner.basis = later;
    r.owner.current_mut().unwrap().basis = later;
    r.facts.converted = None;
    r.p = new_coordinate_audio_presentation(
        19,
        AudioAuthorityCoordinateBasis::TargetFrame(later),
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    let q = query(&r, 1);
    enqueue(&r, q);
    assert_eq!(
        r.owner
            .request_target_history_retirement(&r.p, &q, r.id)
            .unwrap(),
        Some(2)
    );
    assert_eq!(r.io.requests.borrow()[0].1, later);
    assert_eq!(r.io.requests.borrow()[0].4, original);
    assert!(exact(original)
        .compare_interpolated_time(exact(later.start_time()))
        .is_lt());
    assert_eq!(r.io.observed.get(), 0);
}

// The original running Source span is still a live raw receipt reference while
// a later converter query covers a genuinely paused Source span.
fn later_source_cover(r: &mut Rig) -> InterpolatedTargetTime {
    r._producer.request_pause(true);
    let state = r.owner.current_mut().unwrap().converted.as_mut().unwrap();
    state.admit(2).unwrap();
    let report = state.render_pending(7).unwrap();
    state.admit(7).unwrap();
    let time = report.target_start_time.checked_add_frames(6, 7).unwrap();
    let q = exact(time);
    let history = state.phase_history().unwrap();
    let phase = history.span(history.len() - 1).unwrap();
    let end = history.end_time();
    let playback = state
        .converter_owner()
        .source_playback_history()
        .unwrap()
        .lookup_at_interpolated_time(phase, q, end)
        .unwrap();
    assert_eq!(playback.state(), SourcePlaybackState::Frozen);
    assert!(playback.physical_start() > r.id.initial_physical());
    let original = state
        .converter_owner()
        .source_playback_history()
        .unwrap()
        .span(0)
        .unwrap();
    assert_eq!(original.physical_start(), r.id.initial_physical());
    assert_eq!(original.state(), SourcePlaybackState::Running);
    r.facts.playback = ConvertedPlaybackFacts {
        phase,
        segment_end: end,
        generated_target_end: end,
        submitted_target_end: end,
        playback,
        pulled_source_end: state.mixer().frame_cursor(),
        revision: 8,
    };
    r.facts.source = state.last_real_source_report();
    r.facts.converted = Some(report);
    r.facts.boundaries = state.boundaries();
    q
}

#[test]
fn original_raw_source_minimum_precedes_actual_later_cover_without_rebasing_target_pin() {
    let mut r = rig();
    let q = later_source_cover(&mut r);
    enqueue(&r, q);
    let before = format!("{:?}", r.p);
    let caches = cache(&r);
    let original_source = r.id.initial_physical();
    assert_eq!(original_source, 1);
    assert_eq!(
        r.owner.basis.start_time(),
        TargetTime::from_frames(2, 3).unwrap()
    );
    assert_eq!(
        r.owner
            .request_target_history_retirement_with_source_pin(
                &r.p,
                &q,
                r.id,
                Some(original_source),
            )
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        &*r.io.requests.borrow(),
        &[(
            19,
            r.owner.basis,
            r.id,
            original_source,
            r.owner.basis.start_time()
        )]
    );
    assert_eq!(r.io.queried.get(), 1);
    assert_unchanged(&r, &before, caches, 0);
}

#[test]
fn equal_later_future_and_absent_source_hints_keep_original_covering_source_pin() {
    for case in 0..4 {
        let mut r = rig();
        let q = later_source_cover(&mut r);
        let cover = r.facts.playback.playback.physical_start();
        let hint = match case {
            0 => Some(cover),
            1 => Some(cover + 1),
            2 => Some(u64::MAX),
            _ => None,
        };
        enqueue(&r, q);
        let before = format!("{:?}", r.p);
        let caches = cache(&r);
        assert_eq!(
            r.owner
                .request_target_history_retirement_with_source_pin(&r.p, &q, r.id, hint)
                .unwrap(),
            Some(2)
        );
        assert_eq!(
            &*r.io.requests.borrow(),
            &[(19, r.owner.basis, r.id, cover, r.owner.basis.start_time())]
        );
        assert_eq!(r.io.queried.get(), 1);
        assert_unchanged(&r, &before, caches, 0);
    }
}

#[test]
fn source_hint_before_original_creation_refuses_before_exact_query_or_queue() {
    let mut r = rig();
    seed(&mut r);
    let q = query(&r, 1);
    enqueue(&r, q);
    let before = format!("{:?}", r.p);
    let caches = cache(&r);
    assert!(r
        .owner
        .request_target_history_retirement_with_source_pin(
            &r.p,
            &q,
            r.id,
            Some(r.id.initial_physical() - 1),
        )
        .is_err());
    assert_eq!(r.io.queried.get(), 0);
    assert_eq!(r.io.queries.borrow().len(), 1);
    assert!(r.io.requests.borrow().is_empty());
    assert_unchanged(&r, &before, caches, 1);
}

#[test]
fn source_hint_busy_and_error_keep_effective_original_pins_and_all_cached_state() {
    for fail in [false, true] {
        let mut r = rig();
        let q = later_source_cover(&mut r);
        enqueue(&r, q);
        r.io.replies.borrow_mut().push_back(if fail {
            Err(io::Error::other("source-pinned queue failed"))
        } else {
            Ok(None)
        });
        let before = format!("{:?}", r.p);
        let caches = cache(&r);
        let result = r.owner.request_target_history_retirement_with_source_pin(
            &r.p,
            &q,
            r.id,
            Some(r.id.initial_physical()),
        );
        if fail {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_none());
        }
        assert_eq!(
            &*r.io.requests.borrow(),
            &[(
                19,
                r.owner.basis,
                r.id,
                r.id.initial_physical(),
                r.owner.basis.start_time()
            )]
        );
        assert_eq!(r.io.queried.get(), 1);
        assert_unchanged(&r, &before, caches, 0);
    }
}
