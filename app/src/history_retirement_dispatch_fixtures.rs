//! Exact retention dispatch is a read-only mathematical request, not native evidence.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    gameplay::output::ports::SourcePlaybackIdentity,
    local_input::InputMerger,
    native_audio_presentation::{
        NativeAudioPresentation, NativeAudioSnapshot, TargetNativeAudioSnapshot,
    },
    native_audio_startup::{new_audio_presentation, new_coordinate_audio_presentation},
    native_gameplay_bridge::NativeGameplayDevice,
    native_output_coordinate::{
        CoordinateNativeAudioSnapshot, CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate,
    },
};
use beatkernel::{
    audio::{InterpolatedTargetTime, OutputFrameBasis, TargetFrameBasis, TargetTime},
    input::DeviceId,
    time::{Duration, ExtrapolationPolicy},
};
use beatkernel_platform::audio::presentation::{
    discipline::PresentationDiscipline, validation::OriginalNativePresentationEvidence,
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
};
use std::cmp::Ordering;

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn target_basis() -> TargetFrameBasis {
    TargetFrameBasis::new(point(2, 100), TargetTime::new(0, 1, 3).unwrap(), 7).unwrap()
}
fn identity() -> SourcePlaybackIdentity {
    SourcePlaybackIdentity::new(3, 1, 1).unwrap()
}
fn config() -> AudioAuthorityConfig {
    AudioAuthorityConfig {
        history_capacity: 8,
        max_observation_age: Duration::from_nanos(1_000),
        input_extrapolation: ExtrapolationPolicy::Forbid,
        max_input_ahead: Duration::from_nanos(i64::MAX),
    }
}
fn presentation(basis: AudioAuthorityCoordinateBasis) -> NativeAudioPresentation {
    new_coordinate_audio_presentation(19, basis, ClockDomainId(1), point(3, 500), config()).unwrap()
}
fn target() -> NativeAudioPresentation {
    presentation(AudioAuthorityCoordinateBasis::TargetFrame(target_basis()))
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn query(frame: u64) -> InterpolatedTargetTime {
    exact(
        target_basis()
            .start_time()
            .checked_add_frames(frame, 7)
            .unwrap(),
    )
}
fn same(actual: InterpolatedTargetTime, expected: InterpolatedTargetTime) {
    assert_eq!(actual.compare_interpolated_time(expected), Ordering::Equal);
}
fn admit(p: &mut NativeAudioPresentation, frame: u64, at: i64) {
    let basis = target_basis();
    p.admit_target_coordinate(
        CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
                epoch: 19,
                basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: basis.point_at_stream_frame(frame).unwrap(),
                    target: host(at),
                }),
            },
            NativeOutputCoordinate::StreamFrame { frame },
        )
        .unwrap(),
    )
    .unwrap();
}
fn closed() -> NativeAudioPresentation {
    let mut p = target();
    admit(&mut p, 2, 100);
    admit(&mut p, 5, 110);
    admit(&mut p, 9, 120);
    let mut merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
    let a = p.authority_mut();
    a.record_acquired_prefix(host(115)).unwrap();
    let prepared = a
        .prepare_coordinate_frontier(host(120), &merger)
        .unwrap()
        .unwrap();
    a.commit_coordinate_frontier(prepared, host(120), &mut merger)
        .unwrap();
    assert_eq!(a.closed_host_prefix(), Some(host(115)));
    same(a.future_input_retention_query().unwrap(), query(5));
    p
}

#[derive(Clone, Copy)]
enum Reply {
    Queued,
    Busy,
    Error,
}
struct Recording {
    pointer: usize,
    calls: usize,
    query: Option<InterpolatedTargetTime>,
    source: Option<u64>,
    reply: Reply,
}
impl Recording {
    fn new(p: &NativeAudioPresentation, reply: Reply) -> Self {
        Self {
            pointer: p as *const _ as usize,
            calls: 0,
            query: None,
            source: None,
            reply,
        }
    }
    fn request(
        &mut self,
        p: &NativeAudioPresentation,
        q: &InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
        source: Option<u64>,
    ) -> NativeGameplayResult<Option<u64>> {
        assert_eq!(p as *const _ as usize, self.pointer);
        assert_eq!(id, identity());
        self.calls += 1;
        self.query = Some(*q);
        self.source = source;
        match self.reply {
            Reply::Queued => Ok(Some(42)),
            Reply::Busy => Ok(None),
            Reply::Error => Err("retirement queue fault".into()),
        }
    }
}
struct Direct(Recording);
struct Native(Recording);
struct DefaultDevice;
macro_rules! effects {
    () => {
        fn observe(&mut self, _: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
            panic!("no native observation")
        }
        fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
            panic!("no audio observation")
        }
        fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
            panic!("no report acquisition")
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            panic!("no receipt clock")
        }
        fn acquire(
            &mut self,
            _: &mut VecDeque<PhysicalInputEvent>,
        ) -> NativeGameplayResult<InputBatch> {
            panic!("no input acquisition")
        }
        fn observe_end(
            &mut self,
            _: &mut NativeEnd,
            _: &PresentationDiscipline,
            _: Option<RenderReport>,
        ) -> NativeGameplayResult<Option<EndBoundary>> {
            panic!("no end observation")
        }
        fn seed_resume(
            &mut self,
            _: &mut PresentationDiscipline,
            _: ClockPair,
        ) -> NativeGameplayResult<()> {
            panic!("no resume seed")
        }
        fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
            panic!("no scalar fallback")
        }
    };
}
macro_rules! retirement {
    () => {
        fn request_history_retirement(
            &mut self,
            p: &NativeAudioPresentation,
            q: &InterpolatedTargetTime,
            id: SourcePlaybackIdentity,
            source: Option<u64>,
        ) -> NativeGameplayResult<Option<u64>> {
            self.0.request(p, q, id, source)
        }
    };
}
impl GameplayDevice for Direct {
    type Presentation = PresentationDiscipline;
    effects!();
    retirement!();
}
impl NativeGameplayDevice for Native {
    effects!();
    retirement!();
}
impl GameplayDevice for DefaultDevice {
    type Presentation = PresentationDiscipline;
    effects!();
}

#[test]
fn closed_input_prefix_and_older_pending_cut_select_exact_minimum() {
    let mut p = closed();
    let before = format!("{p:?}");
    let mut d = Direct(Recording::new(&p, Reply::Queued));
    let result = PumpTiming::retire_coordinate_history(
        &mut AudioTiming(&mut p, Timestamp::ZERO, None, Some(identity())),
        &mut d,
        &[None, Some(query(8)), Some(query(3)), Some(query(4))],
        None,
    )
    .unwrap();
    assert_eq!(result, Some(42));
    assert_eq!(d.0.calls, 1);
    same(d.0.query.unwrap(), query(3));
    assert_eq!(format!("{p:?}"), before);
}

#[test]
fn absent_and_later_optional_queries_preserve_future_input_bound() {
    for extras in [[None, None], [Some(query(9)), None]] {
        let mut p = closed();
        let mut d = Direct(Recording::new(&p, Reply::Queued));
        PumpTiming::retire_coordinate_history(
            &mut AudioTiming(&mut p, Timestamp::ZERO, None, Some(identity())),
            &mut d,
            &extras,
            None,
        )
        .unwrap();
        assert_eq!(d.0.calls, 1);
        same(d.0.query.unwrap(), query(5));
    }
}

#[test]
fn subnanosecond_optional_cut_is_compared_without_flooring() {
    let mut p = target();
    admit(&mut p, 2, 100);
    let later = exact(TargetTime::new(0, 2, 4_000_000_000).unwrap());
    let earlier = exact(TargetTime::new(0, 1, 4_000_000_000).unwrap());
    assert_eq!(earlier.compare_interpolated_time(later), Ordering::Less);
    assert_eq!(earlier.elapsed_nanos_since_source_frame(0, 1).unwrap(), 0);
    assert_eq!(later.elapsed_nanos_since_source_frame(0, 1).unwrap(), 0);
    let mut d = Direct(Recording::new(&p, Reply::Queued));
    PumpTiming::retire_coordinate_history(
        &mut AudioTiming(&mut p, Timestamp::ZERO, None, Some(identity())),
        &mut d,
        &[Some(later), Some(earlier)],
        None,
    )
    .unwrap();
    same(d.0.query.unwrap(), earlier);
}

#[test]
fn raw_source_hint_is_forwarded_without_inventing_target_query() {
    let mut p = closed();
    let mut d = Direct(Recording::new(&p, Reply::Queued));
    PumpTiming::retire_coordinate_history(
        &mut AudioTiming(&mut p, Timestamp::ZERO, None, Some(identity())),
        &mut d,
        &[None, None],
        Some(2),
    )
    .unwrap();
    assert_eq!(d.0.source, Some(2));
    same(d.0.query.unwrap(), query(5));
}

#[test]
fn scalar_and_missing_identity_have_no_effects() {
    let source = OutputFrameBasis::new(point(2, 100), 3, 1).unwrap();
    let cases = [
        new_audio_presentation(19, source, ClockDomainId(1), point(3, 500), config()).unwrap(),
        target(),
        presentation(AudioAuthorityCoordinateBasis::SourceFrame(source)),
        presentation(AudioAuthorityCoordinateBasis::SourceCounter {
            basis: source,
            frequency: 4_000_000_000,
        }),
    ];
    for (i, mut p) in cases.into_iter().enumerate() {
        let before = format!("{p:?}");
        let mut d = Direct(Recording::new(&p, Reply::Queued));
        let id = if i == 0 { Some(identity()) } else { None };
        assert_eq!(
            PumpTiming::retire_coordinate_history(
                &mut AudioTiming(&mut p, Timestamp::ZERO, None, id),
                &mut d,
                &[Some(query(0))],
                Some(1)
            )
            .unwrap(),
            None
        );
        assert_eq!(d.0.calls, 0);
        assert_eq!(format!("{p:?}"), before);
    }
}

fn source_closed(counter: bool) -> NativeAudioPresentation {
    let basis = OutputFrameBasis::new(point(2, 100), 3, 1).unwrap();
    let frequency = 4_000_000_001;
    let mut p = presentation(if counter {
        AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency }
    } else {
        AudioAuthorityCoordinateBasis::SourceFrame(basis)
    });
    for (position, at, qpc) in [(2, 100, 1), (5, 110, 2), (9, 120, 3)] {
        let (evidence, coordinate) = if counter {
            (
                OriginalNativePresentationEvidence::Wasapi {
                    basis: Some(basis),
                    snapshot: AudioStreamSnapshot {
                        telemetry_available: true,
                        status: AudioStreamStatus::Running,
                        counters: StreamCounters::default(),
                        render: None,
                        clock: Some(AudioClockSnapshot {
                            position,
                            frequency,
                            qpc_100ns: qpc,
                            reading_quality: AudioClockReadingQuality::Accurate,
                            host_point: Some(host(at)),
                            mapping_quality: beatkernel::time::ClockMappingQuality::Unknown,
                        }),
                    },
                },
                NativeOutputCoordinate::Counter {
                    position,
                    frequency,
                },
            )
        } else {
            (
                OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: basis.point_at_stream_frame(position).unwrap(),
                    target: host(at),
                }),
                NativeOutputCoordinate::StreamFrame { frame: position },
            )
        };
        p.admit_coordinate(
            CoordinateNativeAudioSnapshot::new(
                NativeAudioSnapshot {
                    epoch: 19,
                    basis,
                    evidence,
                },
                coordinate,
            )
            .unwrap(),
        )
        .unwrap();
    }
    let mut merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
    p.authority_mut().record_acquired_prefix(host(115)).unwrap();
    let prepared = p
        .authority()
        .prepare_coordinate_frontier(host(120), &merger)
        .unwrap()
        .unwrap();
    p.authority_mut()
        .commit_coordinate_frontier(prepared, host(120), &mut merger)
        .unwrap();
    assert_eq!(p.authority().closed_host_prefix(), Some(host(115)));
    p
}

#[test]
fn source_frame_and_counter_forward_original_fractional_consumer_minima() {
    for counter in [false, true] {
        let mut p = source_closed(counter);
        let future = p.authority().future_input_retention_query().unwrap();
        let frequency = if counter { 4_000_000_001 } else { 3 };
        let base = TargetTime::from_frames(1, 3).unwrap();
        let at = |position| {
            InterpolatedTargetTime::new(base, position, position, frequency, 0, 1).unwrap()
        };
        same(future, at(5));
        // Pending pause/end, queued practice and held practice references share
        // an exact minimum. The separate raw Source pin stays unmodified.
        let earlier = InterpolatedTargetTime::new(base, 3, 4, frequency, 1, 3).unwrap();
        let later = InterpolatedTargetTime::new(base, 3, 4, frequency, 2, 3).unwrap();
        assert_eq!(earlier.compare_interpolated_time(later), Ordering::Less);
        if counter {
            assert_eq!(earlier.elapsed_nanos_since_source_frame(1, 3).unwrap(), 0);
            assert_eq!(later.elapsed_nanos_since_source_frame(1, 3).unwrap(), 0);
        }
        let before = format!("{p:?}");
        let mut d = Direct(Recording::new(&p, Reply::Queued));
        assert_eq!(
            PumpTiming::retire_coordinate_history(
                &mut AudioTiming(&mut p, Timestamp::ZERO, None, Some(identity())),
                &mut d,
                &[Some(at(8)), Some(later), None, Some(earlier), Some(at(4))],
                Some(2),
            )
            .unwrap(),
            Some(42)
        );
        assert_eq!(d.0.calls, 1);
        assert_eq!(d.0.source, Some(2));
        same(d.0.query.unwrap(), earlier);
        assert_eq!(format!("{p:?}"), before);
    }
}

#[test]
fn source_future_input_bound_survives_later_consumers_and_busy_queue() {
    for counter in [false, true] {
        for reply in [Reply::Queued, Reply::Busy, Reply::Error] {
            let mut p = source_closed(counter);
            let future = p.authority().future_input_retention_query().unwrap();
            let later = InterpolatedTargetTime::new(
                TargetTime::from_frames(1, 3).unwrap(),
                9,
                9,
                if counter { 4_000_000_001 } else { 3 },
                0,
                1,
            )
            .unwrap();
            let before = format!("{p:?}");
            let mut d = Native(Recording::new(&p, reply));
            let result = PumpTiming::retire_coordinate_history(
                &mut AudioTiming(&mut p, Timestamp::ZERO, None, Some(identity())),
                &mut d,
                &[None, Some(later)],
                Some(1),
            );
            match reply {
                Reply::Queued => assert_eq!(result.unwrap(), Some(42)),
                Reply::Busy => assert_eq!(result.unwrap(), None),
                Reply::Error => {
                    assert_eq!(result.unwrap_err().to_string(), "retirement queue fault")
                }
            }
            assert_eq!(d.0.calls, 1);
            assert_eq!(d.0.source, Some(1));
            same(d.0.query.unwrap(), future);
            assert_eq!(format!("{p:?}"), before);
        }
    }
}

#[test]
fn busy_and_error_preserve_original_owner_and_prepared_input_token() {
    for reply in [Reply::Busy, Reply::Error] {
        let mut p = closed();
        p.authority_mut().record_acquired_prefix(host(120)).unwrap();
        let prepared = p
            .authority()
            .prepare_coordinate_input(host(116), host(120))
            .unwrap()
            .unwrap();
        let before = format!("{p:?}");
        let mut d = Direct(Recording::new(&p, reply));
        let result = PumpTiming::retire_coordinate_history(
            &mut AudioTiming(&mut p, Timestamp::ZERO, None, Some(identity())),
            &mut d,
            &[],
            None,
        );
        match reply {
            Reply::Busy => assert_eq!(result.unwrap(), None),
            Reply::Error => assert_eq!(result.unwrap_err().to_string(), "retirement queue fault"),
            Reply::Queued => unreachable!(),
        }
        assert_eq!(d.0.calls, 1);
        assert_eq!(format!("{p:?}"), before);
        p.authority_mut()
            .commit_coordinate_input(prepared, host(120))
            .unwrap();
    }
}

#[test]
fn native_blanket_bridge_and_audio_timing_forward_exactly_once() {
    let mut p = closed();
    let mut d = Native(Recording::new(&p, Reply::Queued));
    assert_eq!(
        PumpTiming::retire_coordinate_history(
            &mut AudioTiming(&mut p, Timestamp::ZERO, None, Some(identity())),
            &mut d,
            &[Some(query(3))],
            Some(2)
        )
        .unwrap(),
        Some(42)
    );
    assert_eq!(d.0.calls, 1);
    assert_eq!(d.0.source, Some(2));
    same(d.0.query.unwrap(), query(3));
}

#[test]
fn additive_device_default_is_stateless_and_unsupported() {
    let mut p = closed();
    let before = format!("{p:?}");
    let mut d = DefaultDevice;
    assert_eq!(
        GameplayDevice::request_history_retirement(&mut d, &p, &query(3), identity(), Some(2))
            .unwrap(),
        None
    );
    assert_eq!(
        PumpTiming::retire_coordinate_history(
            &mut AudioTiming(&mut p, Timestamp::ZERO, None, Some(identity())),
            &mut d,
            &[],
            None
        )
        .unwrap(),
        None
    );
    assert_eq!(format!("{p:?}"), before);
}
