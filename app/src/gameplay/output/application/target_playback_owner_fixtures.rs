//! Real converted histories and one static compound effect port, not hardware proof.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    gameplay::output::ports::{
        NativeTargetPlaybackFacts, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
        OriginalCoordinateTargetNativeOutputBackend, TargetPlaybackNativeOutputBackend,
        TargetOutputTelemetry,
    },
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_audio_startup::{new_coordinate_audio_presentation, new_target_audio_presentation},
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::{ConvertedPlaybackFacts, NativePlaybackMapping},
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
    transport::OutputPhaseBasis,
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
struct Evidence {
    native_words: [u64; 2],
}
#[derive(Default)]
struct Io {
    observations:
        RefCell<VecDeque<Result<Option<NativeTargetPlaybackObservation<Evidence>>, io::Error>>>,
    queries: RefCell<VecDeque<Result<Option<NativeTargetPlaybackQuery<Evidence>>, io::Error>>>,
    observed: Cell<usize>,
    queried: Cell<usize>,
    mutate_epoch: Cell<bool>,
    mutate_basis: Cell<bool>,
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
    NativeTargetPlaybackQuery::new(
        epoch,
        basis,
        id,
        query,
        facts,
        Some(Evidence {
            native_words: [31, 47],
        }),
    )
    .unwrap()
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

#[test]
fn single_original_packet_admits_real_target_and_commits_only_same_packet_cache() {
    let mut r = rig();
    let original = observation(&r, 1, 1_000_000_000, r.facts);
    let coordinate = original.coordinate();
    push(&r, original);
    let returned = r
        .owner
        .observe_target_native_playback(&mut r.p, r.id)
        .unwrap()
        .unwrap();
    assert_eq!(returned.coordinate(), coordinate);
    assert_eq!(
        returned.playback().native_evidence().unwrap().native_words,
        [31, 47]
    );
    assert_eq!(r.io.observed.get(), 1);
    assert_eq!(r.io.queried.get(), 0);
    assert_eq!(r.p.authority().history_len(), 1);
    assert_eq!(r.p.target_basis(), Some(r.owner.basis));
    assert_eq!(
        r.p.latest_record().unwrap().pair().target,
        point(1, 1_000_000_000)
    );
    assert_eq!(
        cache(&r),
        (r.facts.source, r.facts.converted, Some(r.facts.boundaries))
    );
    assert_eq!(returned.playback().facts().revision, 20);
    assert_eq!(returned.playback().facts().playback.revision, 6);
    assert_eq!(r.id.hz(), 3);
    assert_eq!(r.owner.basis.sample_rate(), 7);
    assert_eq!(
        r.owner.basis.start_time(),
        TargetTime::from_frames(2, 3).unwrap()
    );
}
#[test]
fn none_and_real_backend_errors_preserve_admitted_owner_and_caches() {
    let mut r = rig();
    seed(&mut r);
    let before = format!("{:?}", r.p);
    let caches = cache(&r);
    assert!(r
        .owner
        .observe_target_native_playback(&mut r.p, r.id)
        .unwrap()
        .is_none());
    r.io.observations
        .borrow_mut()
        .push_back(Err(io::Error::other("actual port error")));
    assert!(r
        .owner
        .observe_target_native_playback(&mut r.p, r.id)
        .is_err());
    assert_eq!(format!("{:?}", r.p), before);
    assert_eq!(cache(&r), caches);
    assert_eq!(r.io.observed.get(), 3);
}
#[test]
fn absent_current_owner_does_not_call_either_packet_port() {
    let mut r = rig();
    let output = r.owner.current.take().unwrap();
    let before = format!("{:?}", r.p);
    assert!(r
        .owner
        .observe_target_native_playback(&mut r.p, r.id)
        .unwrap()
        .is_none());
    assert!(r
        .owner
        .target_playback_at(&r.p, &query(&r, 1), r.id)
        .unwrap()
        .is_none());
    assert_eq!(r.io.observed.get(), 0);
    assert_eq!(r.io.queried.get(), 0);
    assert_eq!(format!("{:?}", r.p), before);
    r.owner.current = Some(output);
}
#[test]
fn actual_native_admission_regression_preserves_all_previous_packet_caches() {
    let mut r = rig();
    push(&r, observation(&r, 2, 1_100_000_000, r.facts));
    r.owner
        .observe_target_native_playback(&mut r.p, r.id)
        .unwrap()
        .unwrap();
    let before = format!("{:?}", r.p);
    let caches = cache(&r);
    let mut facts = r.facts;
    facts.source = None;
    facts.converted = None;
    push(&r, observation(&r, 1, 1_200_000_000, facts));
    assert!(r
        .owner
        .observe_target_native_playback(&mut r.p, r.id)
        .is_err());
    assert_eq!(format!("{:?}", r.p), before);
    assert_eq!(cache(&r), caches);
}
#[test]
fn original_host_domain_mismatch_refuses_without_partial_presentation_or_cache() {
    let mut r = rig();
    let basis = r.owner.basis;
    let coordinate = CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch: 19,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(1).unwrap(),
                target: point(9, 1_000_000_000),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame: 1 },
    )
    .unwrap();
    push(
        &r,
        NativeTargetPlaybackObservation::new(
            coordinate,
            packet(&r, 19, basis, r.id, query(&r, 1), r.facts),
        )
        .unwrap(),
    );
    let before = format!("{:?}", r.p);
    assert!(r
        .owner
        .observe_target_native_playback(&mut r.p, r.id)
        .is_err());
    assert_eq!(format!("{:?}", r.p), before);
    assert_eq!(cache(&r), (None, None, None));
}
#[test]
fn live_output_epoch_or_basis_mutation_is_refused_even_when_port_returns_none() {
    for (epoch, present) in [(true, true), (false, true), (true, false), (false, false)] {
        let mut r = rig();
        if present {
            push(&r, observation(&r, 1, 1_000_000_000, r.facts));
        }
        r.io.mutate_epoch.set(epoch);
        r.io.mutate_basis.set(!epoch);
        let before = format!("{:?}", r.p);
        assert!(r
            .owner
            .observe_target_native_playback(&mut r.p, r.id)
            .is_err());
        assert_eq!(r.io.observed.get(), 1);
        assert_eq!(format!("{:?}", r.p), before);
        assert_eq!(cache(&r), (None, None, None));
    }
}
#[test]
fn wrong_returned_source_creation_identity_or_epoch_cannot_enter_authority() {
    for epoch in [true, false] {
        let mut r = rig();
        let basis = r.owner.basis;
        let e = if epoch { 20 } else { 19 };
        let id = if epoch {
            r.id
        } else {
            SourcePlaybackIdentity::new(3, 0, 0).unwrap()
        };
        let coordinate = CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
                epoch: e,
                basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: basis.point_at_stream_frame(1).unwrap(),
                    target: point(1, 1_000_000_000),
                }),
            },
            NativeOutputCoordinate::StreamFrame { frame: 1 },
        )
        .unwrap();
        push(
            &r,
            NativeTargetPlaybackObservation::new(
                coordinate,
                packet(&r, e, basis, id, query(&r, 1), r.facts),
            )
            .unwrap(),
        );
        let before = format!("{:?}", r.p);
        assert!(r
            .owner
            .observe_target_native_playback(&mut r.p, r.id)
            .is_err());
        assert_eq!(format!("{:?}", r.p), before);
        assert_eq!(cache(&r), (None, None, None));
    }
}
#[test]
fn wrong_returned_target_basis_refuses_before_native_pin_or_cache_change() {
    let mut r = rig();
    let basis =
        TargetFrameBasis::new(r.owner.basis.origin(), r.owner.basis.start_time(), 14).unwrap();
    let mut facts = r.facts;
    facts.converted = None;
    let q = exact(basis.start_time().checked_add_frames(1, 14).unwrap());
    let coordinate = CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch: 19,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(1).unwrap(),
                target: point(1, 1_000_000_000),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame: 1 },
    )
    .unwrap();
    push(
        &r,
        NativeTargetPlaybackObservation::new(coordinate, packet(&r, 19, basis, r.id, q, facts))
            .unwrap(),
    );
    let before = format!("{:?}", r.p);
    assert!(r
        .owner
        .observe_target_native_playback(&mut r.p, r.id)
        .is_err());
    assert_eq!(format!("{:?}", r.p), before);
    assert_eq!(cache(&r), (None, None, None));
    assert_eq!(r.p.target_basis(), None);
}
#[test]
fn cold_wrong_epoch_target_rate_source_descriptor_and_scalar_refuse_before_io() {
    for case in 0..5 {
        let mut r = rig();
        let b = r.owner.basis;
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
            .observe_target_native_playback(&mut r.p, r.id)
            .is_err());
        assert!(r
            .owner
            .target_playback_at(&r.p, &query(&r, 1), r.id)
            .is_err());
        assert_eq!(r.io.observed.get(), 0);
        assert_eq!(r.io.queried.get(), 0);
        assert_eq!(format!("{:?}", r.p), before);
    }
}
#[test]
fn previously_published_source_rate_or_origin_mismatch_refuses_before_io() {
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
        let before = format!("{:?}", r.p);
        let caches = cache(&r);
        assert!(r
            .owner
            .observe_target_native_playback(&mut r.p, r.id)
            .is_err());
        assert!(r
            .owner
            .target_playback_at(&r.p, &query(&r, 1), r.id)
            .is_err());
        assert_eq!(r.io.observed.get(), 0);
        assert_eq!(r.io.queried.get(), 0);
        assert_eq!(format!("{:?}", r.p), before);
        assert_eq!(cache(&r), caches);
    }
}
#[test]
fn same_publication_none_and_zero_source_reports_clear_cache_without_fallback() {
    for zero in [false, true] {
        let mut r = rig();
        seed(&mut r);
        let mut facts = r.facts;
        facts.source = None;
        if zero {
            let converted = r.owner.current_mut().unwrap().converted.as_mut().unwrap();
            converted.admit(2).unwrap();
            for _ in 0..12 {
                let report = converted.render_pending(1).unwrap();
                converted.admit(1).unwrap();
                if report.source.unwrap().frames == 0 {
                    facts.source = report.source;
                    break;
                }
            }
            assert_eq!(
                facts.source.unwrap().frames,
                0,
                "actual cached converter callback"
            );
            let history = converted.phase_history().unwrap();
            facts.playback.segment_end = history.end_time();
            facts.playback.generated_target_end = history.end_time();
            facts.playback.submitted_target_end = converted.admitted_target_time().unwrap();
            facts.playback.pulled_source_end = converted.mixer().frame_cursor();
            facts.boundaries = converted.boundaries();
        }
        facts.converted = None;
        push(&r, observation(&r, 2, 1_200_000_000, facts));
        let returned = r
            .owner
            .observe_target_native_playback(&mut r.p, r.id)
            .unwrap()
            .unwrap();
        assert_eq!(returned.playback().facts().source, facts.source);
        assert_eq!(cache(&r), (None, None, Some(facts.boundaries)));
        assert_eq!(r.io.observed.get(), 2);
    }
}
#[test]
fn readonly_query_preserves_native_evidence_and_exact_normalized_phase_without_admission() {
    let r = rig();
    let q = query(&r, 1);
    r.io.queries
        .borrow_mut()
        .push_back(Ok(Some(packet(&r, 19, r.owner.basis, r.id, q, r.facts))));
    let before = format!("{:?}", r.p);
    let returned = r.owner.target_playback_at(&r.p, &q, r.id).unwrap().unwrap();
    let logical =
        r.p.authority()
            .epoch()
            .logical_output(q.point(r.owner.basis.origin()).unwrap())
            .unwrap();
    let phase = NativePlaybackMapping::from_target(&returned)
        .phase(
            &r.p,
            r.id,
            &q,
            OutputPhaseBasis::new(ClockDomainId(3), 11, 17, 2, 3).unwrap(),
            logical,
        )
        .unwrap();
    assert_eq!(phase.elapsed_nanos(), 142_857_142);
    assert_eq!(returned.native_evidence().unwrap().native_words, [31, 47]);
    assert_eq!(returned.facts().source, r.facts.source);
    assert_eq!(r.io.queried.get(), 1);
    assert_eq!(r.io.observed.get(), 0);
    assert_eq!(format!("{:?}", r.p), before);
    assert_eq!(cache(&r), (None, None, None));
}
#[test]
fn readonly_query_none_and_error_leave_existing_admission_and_caches_untouched() {
    let mut r = rig();
    seed(&mut r);
    let q = query(&r, 1);
    let before = format!("{:?}", r.p);
    let caches = cache(&r);
    assert!(r
        .owner
        .target_playback_at(&r.p, &q, r.id)
        .unwrap()
        .is_none());
    r.io.queries
        .borrow_mut()
        .push_back(Err(io::Error::other("query failure")));
    assert!(r.owner.target_playback_at(&r.p, &q, r.id).is_err());
    assert_eq!(format!("{:?}", r.p), before);
    assert_eq!(cache(&r), caches);
    assert_eq!(r.io.queried.get(), 2);
}
#[test]
fn readonly_query_wrong_epoch_basis_identity_and_floor_aliased_query_refuse() {
    for case in 0..4 {
        let r = rig();
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
        let different = if case == 3 {
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
        if case == 3 {
            assert_eq!(
                different.point(basis.origin()).unwrap(),
                q.point(basis.origin()).unwrap()
            );
        }
        r.io.queries
            .borrow_mut()
            .push_back(Ok(Some(packet(&r, epoch, basis, id, different, facts))));
        let before = format!("{:?}", r.p);
        assert!(r.owner.target_playback_at(&r.p, &q, r.id).is_err());
        assert_eq!(format!("{:?}", r.p), before);
        assert_eq!(cache(&r), (None, None, None));
        assert_eq!(r.io.queried.get(), 1);
    }
}
#[test]
fn readonly_mathematically_equal_query_encoding_is_accepted_without_new_clock() {
    let r = rig();
    let q = query(&r, 1);
    let equivalent =
        InterpolatedTargetTime::new(r.owner.basis.start_time(), 0, 1, 7, 1, 1).unwrap();
    assert!(q.compare_interpolated_time(equivalent).is_eq());
    r.io.queries.borrow_mut().push_back(Ok(Some(packet(
        &r,
        19,
        r.owner.basis,
        r.id,
        equivalent,
        r.facts,
    ))));
    assert!(r
        .owner
        .target_playback_at(&r.p, &q, r.id)
        .unwrap()
        .is_some());
    assert_eq!(r.p.authority().history_len(), 0);
    assert_eq!(cache(&r), (None, None, None));
}
#[test]
fn readonly_software_query_without_native_evidence_cannot_admit_or_ack() {
    let r = rig();
    let q = query(&r, 1);
    r.io.queries.borrow_mut().push_back(Ok(Some(
        NativeTargetPlaybackQuery::<Evidence>::new(19, r.owner.basis, r.id, q, r.facts, None)
            .unwrap(),
    )));
    let before = format!("{:?}", r.p);
    let result = r.owner.target_playback_at(&r.p, &q, r.id).unwrap().unwrap();
    assert!(result.native_evidence().is_none());
    assert_eq!(format!("{:?}", r.p), before);
    assert_eq!(r.p.authority().history_len(), 0);
    assert_eq!(cache(&r), (None, None, None));
}

#[test]
fn target_control_duplicates_wait_without_refreshing_accepted_age() {
    let mut r = rig();
    push(&r, observation(&r, 2, 1_000_000_000, r.facts));
    let first = r
        .owner
        .observe_target_native_control(&mut r.p, r.id)
        .unwrap()
        .unwrap();
    first.validate(&r.p, r.id).unwrap();
    let accepted = format!("{:?}", r.p);
    let caches = cache(&r);
    push(&r, observation(&r, 2, 1_000_000_000, r.facts));
    let repeated = r
        .owner
        .observe_target_native_control(&mut r.p, r.id)
        .unwrap()
        .unwrap();
    repeated.validate(&r.p, r.id).unwrap();
    assert_eq!(repeated.pair(), first.pair());
    assert_eq!(format!("{:?}", r.p), accepted);
    assert_eq!(r.io.observed.get(), 2);
    for (index, host) in [1_100_000_000, 2_100_000_000].into_iter().enumerate() {
        let candidate = observation(&r, 2, host, r.facts);
        assert!(
            crate::native_coordinate_control::TargetCoordinateControlFacts::from_target(
                &candidate, &r.p, r.id
            )
            .is_err()
        );
        push(&r, candidate);
        assert!(r
            .owner
            .observe_target_native_control(&mut r.p, r.id)
            .unwrap()
            .is_none());
        assert_eq!(format!("{:?}", r.p), accepted);
        assert_eq!(cache(&r), caches);
        assert_eq!(r.io.observed.get(), index + 3);
    }
    assert!(!r
        .p
        .authority()
        .has_fresh_observation(point(1, 2_100_000_000))
        .unwrap());
    push(&r, observation(&r, 3, 2_100_000_001, r.facts));
    let progressed = r
        .owner
        .observe_target_native_control(&mut r.p, r.id)
        .unwrap()
        .unwrap();
    progressed.validate(&r.p, r.id).unwrap();
    assert_eq!(progressed.pair().target, point(1, 2_100_000_001));
    assert!(first.validate(&r.p, r.id).is_err());
    assert_eq!(r.io.observed.get(), 5);
    let progressed_state = format!("{:?}", r.p);
    push(&r, observation(&r, 2, 2_100_000_002, r.facts));
    assert!(r
        .owner
        .observe_target_native_control(&mut r.p, r.id)
        .is_err());
    assert_eq!(format!("{:?}", r.p), progressed_state);
    assert_eq!(cache(&r), caches);
    assert_eq!(r.io.observed.get(), 6);
    assert_eq!(r.io.queried.get(), 0);
}

#[test]
fn ordinary_target_duplicate_returns_packet_with_new_real_converted_report() {
    let mut r = rig();
    seed(&mut r);
    let accepted = format!("{:?}", r.p);
    let initial = r.facts.converted;
    let converted = r.owner.current_mut().unwrap().converted.as_mut().unwrap();
    converted.admit(2).unwrap();
    let report = converted.render_pending(1).unwrap();
    converted.admit(1).unwrap();
    let history = converted.phase_history().unwrap();
    let mut facts = r.facts;
    facts.source = converted.last_real_source_report();
    facts.converted = Some(report);
    facts.playback.segment_end = history.end_time();
    facts.playback.generated_target_end = history.end_time();
    facts.playback.submitted_target_end = converted.admitted_target_time().unwrap();
    facts.playback.pulled_source_end = converted.mixer().frame_cursor();
    facts.boundaries = converted.boundaries();
    facts.revision = 22;
    assert_ne!(facts.converted, initial);
    for (index, host) in [1_000_000_000, 1_000_000_000, 1_100_000_000]
        .into_iter()
        .enumerate()
    {
        push(&r, observation(&r, 1, host, facts));
        if index == 0 {
            let returned = r
                .owner
                .observe_target_native_control(&mut r.p, r.id)
                .unwrap()
                .unwrap();
            returned.validate(&r.p, r.id).unwrap();
            assert_eq!(returned.target().converted, Some(report));
            assert_eq!(returned.target().revision, 22);
        } else {
            let returned = r
                .owner
                .observe_target_native_playback(&mut r.p, r.id)
                .unwrap()
                .unwrap();
            assert_eq!(returned.playback().facts().converted, Some(report));
            assert_eq!(returned.playback().facts().revision, 22);
        }
        assert_eq!(r.owner.converted_report(), Some(report));
        assert_eq!(r.owner.target_boundary_facts(), Some(facts.boundaries));
        assert_eq!(format!("{:?}", r.p), accepted);
        assert_eq!(r.io.observed.get(), index + 2);
    }
    assert_eq!(r.io.queried.get(), 0);
}
