//! Static dispatch uses one original compound publication and its bound presentation.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    gameplay::output::{
        application::owner::{
            source_control_owner_fixtures as source_control,
            source_playback_owner_fixtures as source,
        },
        ports::{
            NativePlaybackObservation, NativeTargetPlaybackFacts, NativeTargetPlaybackObservation,
            NativeTargetPlaybackQuery, SourcePlaybackIdentity,
        },
    },
    native_audio_presentation::{NativeAudioPresentation, TargetNativeAudioSnapshot},
    native_audio_startup::new_coordinate_audio_presentation,
    native_coordinate_control::{
        CoordinateControlFacts, CoordinatePlaybackControlFacts, TargetCoordinateControlFacts,
    },
    native_gameplay_bridge::NativeGameplayDevice,
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::ConvertedPlaybackFacts,
};
use beatkernel::audio::*;
use beatkernel_platform::audio::{
    ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
    presentation::{
        discipline::PresentationDiscipline, validation::OriginalNativePresentationEvidence,
    },
};

// Neither Clone nor Debug: forwarding must not require either on native evidence.
struct TargetEvidence;
enum Reply {
    Source(NativePlaybackObservation<source::Evidence>),
    Target(NativeTargetPlaybackObservation<TargetEvidence>),
    None,
    Error,
}
struct State {
    expected_presentation: usize,
    expected_identity: SourcePlaybackIdentity,
    reply: Option<Reply>,
    calls: usize,
}
impl State {
    fn bind(p: &mut NativeAudioPresentation, id: SourcePlaybackIdentity, reply: Reply) -> Self {
        Self {
            expected_presentation: p as *mut _ as usize,
            expected_identity: id,
            reply: Some(reply),
            calls: 0,
        }
    }
    fn acquire(&mut self, p: &mut NativeAudioPresentation, id: SourcePlaybackIdentity) -> Reply {
        assert_eq!(p as *mut _ as usize, self.expected_presentation);
        assert_eq!(id, self.expected_identity);
        self.calls += 1;
        self.reply.take().expect("one compound acquisition only")
    }
    fn source(
        &mut self,
        p: &mut NativeAudioPresentation,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        match self.acquire(p, id) {
            Reply::Source(original) => {
                p.admit_coordinate(original.coordinate())?;
                Ok(Some(CoordinateControlFacts::from_source(&original, p, id)?))
            }
            Reply::None => Ok(None),
            Reply::Error => Err("compound publication fault".into()),
            Reply::Target(_) => panic!("Target must never be dispatched through old Source API"),
        }
    }
    fn target(
        &mut self,
        p: &mut NativeAudioPresentation,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinatePlaybackControlFacts>> {
        match self.acquire(p, id) {
            Reply::Target(original) => {
                p.admit_target_coordinate(original.coordinate())?;
                Ok(Some(
                    TargetCoordinateControlFacts::from_target(&original, p, id)?.into(),
                ))
            }
            Reply::None => Ok(None),
            Reply::Error => Err("compound publication fault".into()),
            Reply::Source(_) => {
                panic!("explicit Target override must retain original Target packet")
            }
        }
    }
}
fn source_case() -> (NativeAudioPresentation, SourcePlaybackIdentity, Reply) {
    let (owner, p, _io, _producer, id) = source_control::low_rate_fixture();
    let output = owner.current().unwrap();
    let original = source_control::frame_observation(output.basis, id, 1, 100, output.report);
    (p, id, Reply::Source(original))
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn target_case() -> (NativeAudioPresentation, SourcePlaybackIdentity, Reply) {
    let format = AudioFormat::new(3, 1).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
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
    let mut owner = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(7, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("valid conversion"));
    owner.enable_phase_history(8).unwrap();
    let basis = owner.target_frame_basis();
    let report = owner.render_pending(7).unwrap();
    owner.admit(5).unwrap();
    let exact = |time| InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap();
    let history = owner.phase_history().unwrap();
    let playback = owner
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
            submitted_target_end: owner.admitted_target_time().unwrap(),
            playback,
            pulled_source_end: owner.mixer().frame_cursor(),
            revision: 6,
        },
        source: owner.last_real_source_report(),
        converted: Some(report),
        boundaries: owner.boundaries(),
        revision: 20,
    };
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
    let original = NativeTargetPlaybackObservation::new(
        coordinate,
        NativeTargetPlaybackQuery::new(
            19,
            basis,
            id,
            exact(basis.start_time().checked_add_frames(1, 7).unwrap()),
            facts,
            Some(TargetEvidence),
        )
        .unwrap(),
    )
    .unwrap();
    let p = new_coordinate_audio_presentation(
        19,
        AudioAuthorityCoordinateBasis::TargetFrame(basis),
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    (p, id, Reply::Target(original))
}

struct DirectSource(State);
struct DirectTarget(State);
struct NativeSource(State);
struct NativeTarget(State);
trait FixtureDevice: GameplayDevice {
    fn new(state: State) -> Self;
    fn calls(&self) -> usize;
}
macro_rules! common_effects {
    () => {
        fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
            panic!("no independent audio observation")
        }
        fn observe_audio_end(
            &mut self,
            _: &mut NativeEnd,
            _: &NativeAudioPresentation,
            _: Option<RenderReport>,
        ) -> NativeGameplayResult<Option<EndBoundary>> {
            panic!("no independent audio end")
        }
        fn observe(&mut self, _: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
            panic!("no independent observation")
        }
        fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
            panic!("no independent report")
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
            panic!("no independent end")
        }
        fn seed_resume(
            &mut self,
            _: &mut PresentationDiscipline,
            _: ClockPair,
        ) -> NativeGameplayResult<()> {
            panic!("no resume seeding")
        }
        fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
            panic!("no scalar fallback")
        }
    };
}
macro_rules! source_method {
    () => {
        fn observe_coordinate_control(
            &mut self,
            p: &mut NativeAudioPresentation,
            id: SourcePlaybackIdentity,
        ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
            self.0.source(p, id)
        }
    };
}
macro_rules! target_method {
    () => {
        fn observe_coordinate_control(
            &mut self,
            _: &mut NativeAudioPresentation,
            _: SourcePlaybackIdentity,
        ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
            panic!("blanket dispatch lost explicit Target override")
        }
        fn observe_coordinate_playback_control(
            &mut self,
            p: &mut NativeAudioPresentation,
            id: SourcePlaybackIdentity,
        ) -> NativeGameplayResult<Option<CoordinatePlaybackControlFacts>> {
            self.0.target(p, id)
        }
    };
}
impl GameplayDevice for DirectSource {
    type Presentation = PresentationDiscipline;
    common_effects!();
    source_method!();
}
impl GameplayDevice for DirectTarget {
    type Presentation = PresentationDiscipline;
    common_effects!();
    target_method!();
}
impl NativeGameplayDevice for NativeSource {
    common_effects!();
    source_method!();
}
impl NativeGameplayDevice for NativeTarget {
    common_effects!();
    target_method!();
}
macro_rules! fixture_device {
    ($($ty:ty),+) => { $(impl FixtureDevice for $ty {
        fn new(state: State) -> Self { Self(state) }
        fn calls(&self) -> usize { self.0.calls }
    })+ };
}
fixture_device!(DirectSource, DirectTarget, NativeSource, NativeTarget);

fn positive<D: FixtureDevice>(target: bool, timing: bool) {
    let (mut p, id, reply) = if target { target_case() } else { source_case() };
    assert!(p.latest_record().is_none());
    let mut device = D::new(State::bind(&mut p, id, reply));
    let facts = if timing {
        let mut bound = AudioTiming(&mut p, Timestamp::ZERO, None, Some(id));
        PumpTiming::coordinate_playback_control(&mut bound, &mut device)
            .unwrap()
            .unwrap()
    } else {
        GameplayDevice::observe_coordinate_playback_control(&mut device, &mut p, id)
            .unwrap()
            .unwrap()
    };
    assert_eq!(device.calls(), 1);
    facts.validate(&p, id).unwrap();
    assert_eq!(facts.pair(), p.latest_record().unwrap().pair());
    assert_eq!(facts.identity(), id);
    if target {
        let CoordinatePlaybackControlFacts::Target(original) = facts else {
            panic!("Target turned into Source")
        };
        assert_eq!(original.target().revision, 20);
        assert_eq!(original.target().playback.revision, 6);
        assert_eq!(original.target().converted.unwrap().target_frames, 7);
        assert_eq!(facts.report(), original.target().source);
        assert_eq!(
            facts.target_boundaries(),
            Some(original.target().boundaries)
        );
        assert_eq!(id, SourcePlaybackIdentity::new(3, 1, 1).unwrap());
    } else {
        let CoordinatePlaybackControlFacts::Source(original) = facts else {
            panic!("Source default changed")
        };
        assert_eq!(facts.report(), original.source().render);
        assert!(facts.target_boundaries().is_none());
    }
}
fn no_publication<D: FixtureDevice>(target: bool, error: bool, timing: bool) {
    let (mut p, id, _) = if target { target_case() } else { source_case() };
    let before = format!("{p:?}");
    let mut device = D::new(State::bind(
        &mut p,
        id,
        if error { Reply::Error } else { Reply::None },
    ));
    let result = if timing {
        let mut bound = AudioTiming(&mut p, Timestamp::ZERO, None, Some(id));
        PumpTiming::coordinate_playback_control(&mut bound, &mut device)
    } else {
        GameplayDevice::observe_coordinate_playback_control(&mut device, &mut p, id)
    };
    if error {
        assert_eq!(
            result.unwrap_err().to_string(),
            "compound publication fault"
        );
    } else {
        assert!(result.unwrap().is_none());
    }
    assert_eq!(device.calls(), 1);
    assert_eq!(format!("{p:?}"), before);
}
#[test]
fn direct_source_default_preserves_real_packet() {
    positive::<DirectSource>(false, false);
}
#[test]
fn native_source_default_survives_blanket_dispatch() {
    positive::<NativeSource>(false, false);
}
#[test]
fn direct_target_override_preserves_real_packet() {
    positive::<DirectTarget>(true, false);
}
#[test]
fn native_target_override_survives_blanket_dispatch() {
    positive::<NativeTarget>(true, false);
}
#[test]
fn audio_timing_forwards_bound_source_identity_and_presentation() {
    positive::<NativeSource>(false, true);
}
#[test]
fn audio_timing_forwards_target_override_through_blanket() {
    positive::<NativeTarget>(true, true);
}
#[test]
fn direct_source_none_and_error_have_no_fallback_or_mutation() {
    for error in [false, true] {
        no_publication::<DirectSource>(false, error, false);
    }
}
#[test]
fn native_source_none_and_error_have_no_fallback_or_mutation() {
    for error in [false, true] {
        no_publication::<NativeSource>(false, error, false);
    }
}
#[test]
fn direct_target_none_and_error_have_no_fallback_or_mutation() {
    for error in [false, true] {
        no_publication::<DirectTarget>(true, error, false);
    }
}
#[test]
fn native_target_none_and_error_have_no_fallback_or_mutation() {
    for error in [false, true] {
        no_publication::<NativeTarget>(true, error, false);
    }
}
#[test]
fn audio_timing_source_none_and_error_have_no_fallback_or_mutation() {
    for error in [false, true] {
        no_publication::<NativeSource>(false, error, true);
    }
}
#[test]
fn audio_timing_target_none_and_error_have_no_fallback_or_mutation() {
    for error in [false, true] {
        no_publication::<NativeTarget>(true, error, true);
    }
}
fn missing_identity<D: FixtureDevice>(target: bool) {
    let (mut p, id, reply) = if target { target_case() } else { source_case() };
    let before = format!("{p:?}");
    let mut device = D::new(State::bind(&mut p, id, reply));
    let result = PumpTiming::coordinate_playback_control(
        &mut AudioTiming(&mut p, Timestamp::ZERO, None, None),
        &mut device,
    );
    assert!(result.is_err());
    assert_eq!(device.calls(), 0);
    assert_eq!(format!("{p:?}"), before);
}
#[test]
fn audio_timing_missing_source_identity_refuses_before_acquisition() {
    missing_identity::<NativeSource>(false);
}
#[test]
fn audio_timing_missing_target_identity_refuses_before_acquisition() {
    missing_identity::<NativeTarget>(true);
}

// An existing Source-only timing implementation inherits the additive sum method.
struct SourceOnlyTiming<'a>(AudioTiming<'a>);
impl<D: GameplayDevice> PumpTiming<D> for SourceOnlyTiming<'_> {
    const AUDIO: bool = true;
    fn coordinate_control(
        &mut self,
        device: &mut D,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        <AudioTiming<'_> as PumpTiming<D>>::coordinate_control(&mut self.0, device)
    }
    fn observe(&mut self, _: &mut D) -> NativeGameplayResult<()> {
        panic!("no independent observation")
    }
    fn latest_pair(&self) -> Option<ClockPair> {
        panic!("no independent pair")
    }
    fn validate_host(&self, _: ClockPoint) -> NativeGameplayResult<()> {
        panic!("no independent HOST validation")
    }
    fn correction(
        &mut self,
        _: ClockPoint,
        _: &mut Transport,
    ) -> NativeGameplayResult<Option<DisciplineUpdate>> {
        panic!("no scalar correction")
    }
    fn quality(&self) -> ClockMappingQuality {
        panic!("no scalar quality")
    }
    fn logical_domain(&self, _: NativeGameplayConfig) -> ClockDomainId {
        panic!("no independent domain")
    }
    fn pause_evidence(
        &mut self,
        _: &mut D,
        _: ClockPair,
        _: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        panic!("no independent pause evidence")
    }
    fn end(
        &mut self,
        _: &mut D,
        _: &mut NativeEnd,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("no independent end")
    }
    fn resume(
        &mut self,
        _: &mut D,
        _: NativeGameplayConfig,
        _: &crate::playback_pause::NativePause,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("no resume effect")
    }
    fn publish(
        &mut self,
        _: &mut D,
        _: GameplayPauseControl<'_>,
        _: Option<&crate::local_input::InputMerger>,
        _: &mut crate::playback_pause::NativePause,
        _: &mut NativeGameplayConfig,
        _: &mut Option<NativeEnd>,
        _: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        panic!("no publication effect")
    }
}
#[test]
fn pump_timing_default_wraps_existing_source_control_once() {
    let (mut p, id, reply) = source_case();
    let mut device = NativeSource::new(State::bind(&mut p, id, reply));
    let mut timing = SourceOnlyTiming(AudioTiming(&mut p, Timestamp::ZERO, None, Some(id)));
    let facts = PumpTiming::coordinate_playback_control(&mut timing, &mut device)
        .unwrap()
        .unwrap();
    assert!(matches!(facts, CoordinatePlaybackControlFacts::Source(_)));
    facts.validate(&p, id).unwrap();
    assert_eq!(device.calls(), 1);
}
#[test]
fn pump_timing_default_preserves_existing_source_none_and_error() {
    for error in [false, true] {
        let (mut p, id, _) = source_case();
        let before = format!("{p:?}");
        let mut device = NativeSource::new(State::bind(
            &mut p,
            id,
            if error { Reply::Error } else { Reply::None },
        ));
        let mut timing = SourceOnlyTiming(AudioTiming(&mut p, Timestamp::ZERO, None, Some(id)));
        let result = PumpTiming::coordinate_playback_control(&mut timing, &mut device);
        if error {
            assert_eq!(
                result.unwrap_err().to_string(),
                "compound publication fault"
            );
        } else {
            assert!(result.unwrap().is_none());
        }
        assert_eq!(device.calls(), 1);
        assert_eq!(format!("{p:?}"), before);
    }
}
