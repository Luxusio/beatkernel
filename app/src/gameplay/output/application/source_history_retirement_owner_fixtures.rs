//! Real Source history and static owner; opaque DI evidence is not hardware proof.
use super::*;
use super::{fixtures as memory, source_playback_owner_fixtures as source};
use crate::gameplay::output::ports::{OriginalCoordinateNativeOutputBackend, SourcePlaybackFacts};
use crate::native_audio_presentation::NativeAudioSnapshot;
use crate::native_output_coordinate::CoordinateNativeAudioSnapshot;
use beatkernel::audio::{CommandProducer, InterpolatedTargetTime, TargetTime};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

#[derive(Default)]
struct Effect {
    requests: RefCell<Vec<(u64, OutputFrameBasis, SourcePlaybackIdentity, u64)>>,
    replies: RefCell<VecDeque<Result<Option<u64>, memory::Fault>>>,
    changed_basis: Cell<Option<OutputFrameBasis>>,
    change_on_query: Cell<bool>,
    epoch_reads: Cell<usize>,
    change_on_epoch_read: Cell<Option<usize>>,
}
struct Backend {
    inner: source::Backend,
    effect: Rc<Effect>,
}
impl OutputReplacementBackend for Backend {
    type Presentation = <source::Backend as OutputReplacementBackend>::Presentation;
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
        let n = self.effect.epoch_reads.get() + 1;
        self.effect.epoch_reads.set(n);
        if self
            .effect
            .change_on_epoch_read
            .get()
            .is_some_and(|cut| n >= cut)
        {
            8
        } else {
            self.inner.epoch(o)
        }
    }
    fn basis(&self, o: &Self::Output) -> OutputFrameBasis {
        self.effect
            .changed_basis
            .get()
            .unwrap_or_else(|| self.inner.basis(o))
    }
    fn observe(
        &mut self,
        _: &mut Self::Output,
        _: &mut Self::Presentation,
    ) -> Result<(), Self::Error> {
        panic!("scalar observation fallback")
    }
    fn render_report(&self, _: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        panic!("independent report fallback")
    }
}
impl OriginalNativeOutputBackend for Backend {
    fn observe_native(
        &mut self,
        _: &mut Self::Output,
    ) -> Result<Option<NativeAudioSnapshot>, Self::Error> {
        panic!("independent native observation")
    }
}
impl OriginalCoordinateNativeOutputBackend for Backend {
    fn observe_native_coordinate(
        &mut self,
        _: &mut Self::Output,
    ) -> Result<Option<CoordinateNativeAudioSnapshot>, Self::Error> {
        panic!("coordinate fallback")
    }
}
impl SourcePlaybackNativeOutputBackend for Backend {
    type NativeEvidence = source::Evidence; // Deliberately not Clone or Copy.
    fn observe_native_playback(
        &mut self,
        o: &mut Self::Output,
        id: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Self::NativeEvidence>>, Self::Error> {
        self.inner.observe_native_playback(o, id)
    }
    fn source_playback_at(
        &self,
        o: &Self::Output,
        q: &InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Self::NativeEvidence>>, Self::Error> {
        let result = self.inner.source_playback_at(o, q, id);
        if self.effect.change_on_query.get() {
            self.effect.changed_basis.set(Some(
                OutputFrameBasis::new(
                    source::point(2, 101),
                    o.basis.sample_rate(),
                    o.basis.start_physical_frame(),
                )
                .unwrap(),
            ));
        }
        result
    }
}
impl SourceHistoryRetirementNativeOutputBackend for Backend {
    fn request_source_history_retirement(
        &mut self,
        o: &mut Self::Output,
        epoch: u64,
        basis: OutputFrameBasis,
        id: SourcePlaybackIdentity,
        pin: u64,
    ) -> Result<Option<u64>, Self::Error> {
        assert_eq!(self.epoch(o), epoch);
        assert_eq!(self.basis(o), basis);
        self.effect
            .requests
            .borrow_mut()
            .push((epoch, basis, id, pin));
        self.effect
            .replies
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(Some(2)))
    }
}
struct Rig {
    owner: GameplayOutputOwner<Backend>,
    p: NativeAudioPresentation,
    io: Rc<source::Io>,
    effect: Rc<Effect>,
    producer: CommandProducer,
    id: SourcePlaybackIdentity,
}
fn rig(counter: bool) -> Rig {
    let (mut previous, p, io, producer, id) =
        super::source_control_owner_fixtures::low_rate_fixture();
    let output = previous.current.take().unwrap();
    let effect = Rc::new(Effect::default());
    let basis = output.basis;
    let inner = source::Backend {
        inner: memory::Backend {
            trace: output.trace.clone(),
        },
        io: io.clone(),
    };
    Rig {
        owner: GameplayOutputOwner::new(
            Backend {
                inner,
                effect: effect.clone(),
            },
            output,
        ),
        p: if counter {
            source::presentation(basis, Some(u64::MAX))
        } else {
            p
        },
        io,
        effect,
        producer,
        id,
    }
}
fn later(r: &mut Rig) {
    r.producer.request_pause(true);
    r.owner.current_mut().unwrap().render(3);
}
fn query() -> InterpolatedTargetTime {
    // Source frame 9 + a subnanosecond fraction, inside the actual paused span.
    InterpolatedTargetTime::new(TargetTime::from_frames(9, 3).unwrap(), 0, 1, u64::MAX, 1, 1)
        .unwrap()
}
fn facts(r: &Rig, q: InterpolatedTargetTime) -> SourcePlaybackFacts {
    let output = r.owner.current().unwrap();
    let history = output
        .mixer
        .as_ref()
        .unwrap()
        .source_playback_history()
        .unwrap();
    SourcePlaybackFacts {
        playback: history.lookup_at_source_time(q).unwrap(),
        pulled_source_end: history.pulled_end_frame(),
        submitted_source_end: history.pulled_end_frame(),
        render: output.report,
        revision: 2,
    }
}
fn enqueue(r: &Rig, q: InterpolatedTargetTime, evidence: bool) {
    r.io.queries.borrow_mut().push_back(Ok(Some(
        NativeSourcePlaybackQuery::new(
            7,
            r.owner.basis,
            r.id,
            q,
            facts(r, q),
            evidence.then(|| source::Evidence {
                words: vec![19, 31],
            }),
        )
        .unwrap(),
    )));
}
fn unchanged(r: &Rig, before: &str, cache: Option<RenderReport>) {
    assert_eq!(format!("{:?}", r.p), before);
    assert_eq!(r.owner.render_report(), cache);
    assert_eq!(r.io.observed_calls.get(), 0);
    assert_eq!(r.io.report_calls.get(), 0);
    assert_eq!(r.io.legacy_calls.get(), 0);
    assert!(r.owner.current().unwrap().mixer.is_some());
}

#[test]
fn exact_frame_and_counter_query_dispatch_actual_covering_span_without_admission() {
    for counter in [false, true] {
        let mut r = rig(counter);
        later(&mut r);
        let q = query();
        let floor = source::query(9, 3);
        assert_eq!(
            q.point(r.owner.basis.origin()).unwrap(),
            floor.point(r.owner.basis.origin()).unwrap()
        );
        assert!(!q.compare_interpolated_time(floor).is_eq());
        assert_eq!(facts(&r, q).playback.physical_start(), 8);
        enqueue(&r, q, true);
        let before = format!("{:?}", r.p);
        assert_eq!(
            r.owner
                .request_source_history_retirement(&r.p, &q, r.id)
                .unwrap(),
            Some(2)
        );
        assert_eq!(&*r.effect.requests.borrow(), &[(7, r.owner.basis, r.id, 8)]);
        assert_eq!(r.io.query_calls.get(), 1);
        unchanged(&r, &before, None);
    }
}
#[test]
fn raw_original_pin_minimum_preserves_older_running_span() {
    for hint in [Some(2), Some(8), Some(9), Some(u64::MAX), None] {
        let mut r = rig(false);
        later(&mut r);
        let q = query();
        enqueue(&r, q, true);
        let history = r
            .owner
            .current()
            .unwrap()
            .mixer
            .as_ref()
            .unwrap()
            .source_playback_history()
            .unwrap();
        assert_eq!(history.span(0).unwrap().physical_start(), 2);
        let before = format!("{:?}", r.p);
        assert_eq!(
            r.owner
                .request_source_history_retirement_with_source_pin(&r.p, &q, r.id, hint)
                .unwrap(),
            Some(2)
        );
        assert_eq!(
            r.effect.requests.borrow()[0].3,
            hint.map_or(8, |h| h.min(8))
        );
        unchanged(&r, &before, None);
    }
}
#[test]
fn raw_pin_before_original_creation_refuses_before_query() {
    let mut r = rig(false);
    later(&mut r);
    let q = query();
    enqueue(&r, q, true);
    assert!(r
        .owner
        .request_source_history_retirement_with_source_pin(&r.p, &q, r.id, Some(1))
        .is_err());
    assert_eq!(r.io.query_calls.get(), 0);
    assert_eq!(r.io.queries.borrow().len(), 1);
    assert!(r.effect.requests.borrow().is_empty());
}
#[test]
fn missing_native_evidence_and_missing_packet_never_queue() {
    for missing in [true, false] {
        let mut r = rig(false);
        later(&mut r);
        let q = query();
        if !missing {
            enqueue(&r, q, false);
        }
        let before = format!("{:?}", r.p);
        let result = r.owner.request_source_history_retirement(&r.p, &q, r.id);
        if missing {
            assert!(result.unwrap().is_none());
        } else {
            assert!(result.is_err());
        }
        assert!(r.effect.requests.borrow().is_empty());
        unchanged(&r, &before, None);
    }
}
#[test]
fn wrong_packet_epoch_basis_identity_and_floor_aliased_query_refuse_without_effect() {
    for case in 0..4 {
        let mut r = rig(false);
        later(&mut r);
        let q = query();
        let b = if case == 1 {
            OutputFrameBasis::new(source::point(2, 101), 3, 2).unwrap()
        } else {
            r.owner.basis
        };
        let id = if case == 2 {
            SourcePlaybackIdentity::new(3, 1, 1).unwrap()
        } else {
            r.id
        };
        let returned = if case == 3 { source::query(9, 3) } else { q };
        r.io.queries.borrow_mut().push_back(Ok(Some(
            NativeSourcePlaybackQuery::new(
                if case == 0 { 8 } else { 7 },
                b,
                id,
                returned,
                facts(&r, returned),
                Some(source::Evidence {
                    words: vec![19, 31],
                }),
            )
            .unwrap(),
        )));
        let before = format!("{:?}", r.p);
        assert!(r
            .owner
            .request_source_history_retirement(&r.p, &q, r.id)
            .is_err());
        assert!(r.effect.requests.borrow().is_empty());
        unchanged(&r, &before, None);
    }
}
#[test]
fn current_identity_changes_during_query_and_before_effect_refuse() {
    for case in 0..3 {
        let mut r = rig(false);
        later(&mut r);
        let q = query();
        enqueue(&r, q, true);
        match case {
            0 => r.io.mutate_query_identity.set(true),
            1 => r.effect.change_on_query.set(true),
            // audio_identity, capture, post-query validation, then pre-effect check.
            _ => r
                .effect
                .change_on_epoch_read
                .set(Some(r.effect.epoch_reads.get() + 4)),
        }
        let before = format!("{:?}", r.p);
        assert!(r
            .owner
            .request_source_history_retirement(&r.p, &q, r.id)
            .is_err());
        assert!(r.effect.requests.borrow().is_empty());
        unchanged(&r, &before, None);
    }
}
#[test]
fn busy_and_backend_error_preserve_admitted_report_and_presentation() {
    for failure in [false, true] {
        let mut r = rig(false);
        let b = r.owner.basis;
        r.io.observations.borrow_mut().push_back(Ok(Some(
            super::source_control_owner_fixtures::frame_observation(
                b,
                r.id,
                1,
                100,
                r.owner.current().unwrap().report,
            ),
        )));
        r.owner
            .observe_native_playback(&mut r.p, r.id)
            .unwrap()
            .unwrap();
        later(&mut r);
        let q = query();
        enqueue(&r, q, true);
        r.effect.replies.borrow_mut().push_back(if failure {
            Err(memory::Fault::new(71))
        } else {
            Ok(None)
        });
        let before = format!("{:?}", r.p);
        let cache = r.owner.render_report();
        assert!(cache.is_some());
        let result = r.owner.request_source_history_retirement(&r.p, &q, r.id);
        if failure {
            assert!(matches!(
                result.unwrap_err().cause,
                ReplacementCause::Backend {
                    phase: ReplacementPhase::Retire,
                    ..
                }
            ));
        } else {
            assert!(result.unwrap().is_none());
        }
        assert_eq!(r.effect.requests.borrow().len(), 1);
        assert_eq!(format!("{:?}", r.p), before);
        assert_eq!(r.owner.render_report(), cache);
        assert_eq!(r.io.observed_calls.get(), 1);
        assert_eq!(r.io.report_calls.get(), 0);
    }
}
#[test]
fn absent_owner_and_query_error_never_dispatch() {
    for absent in [false, true] {
        let mut r = rig(false);
        let q = source::query(3, 3);
        if absent {
            r.owner.current.take();
        } else {
            r.io.queries
                .borrow_mut()
                .push_back(Err(memory::Fault::new(72)));
        }
        let before = format!("{:?}", r.p);
        let result = r.owner.request_source_history_retirement(&r.p, &q, r.id);
        if absent {
            assert!(result.unwrap().is_none());
            assert_eq!(r.io.query_calls.get(), 0);
        } else {
            assert!(matches!(
                result.unwrap_err().cause,
                ReplacementCause::Backend {
                    phase: ReplacementPhase::Observe,
                    ..
                }
            ));
        }
        assert!(r.effect.requests.borrow().is_empty());
        assert_eq!(format!("{:?}", r.p), before);
        assert_eq!(r.owner.render_report(), None);
    }
}

#[test]
fn wrong_current_output_and_presentation_identity_refuse_before_lookup() {
    for case in 0..4 {
        let mut r = rig(false);
        let q = source::query(3, 3);
        match case {
            0 => r.owner.current_mut().unwrap().epoch = 8,
            1 => {
                r.owner.current_mut().unwrap().basis =
                    OutputFrameBasis::new(source::point(2, 101), 3, 2).unwrap()
            }
            2 => {
                r.p = source::presentation(
                    OutputFrameBasis::new(source::point(2, 101), 3, 2).unwrap(),
                    None,
                )
            }
            _ => {
                r.p = source::presentation(
                    OutputFrameBasis::new(r.owner.basis.origin(), 6, 2).unwrap(),
                    None,
                )
            }
        }
        let before = format!("{:?}", r.p);
        assert!(r
            .owner
            .request_source_history_retirement(&r.p, &q, r.id)
            .is_err());
        assert_eq!(r.io.query_calls.get(), 0);
        assert!(r.effect.requests.borrow().is_empty());
        unchanged(&r, &before, None);
    }
}
