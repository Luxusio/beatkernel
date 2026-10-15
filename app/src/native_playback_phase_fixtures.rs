//! Exact mathematical mapping fixtures; no native acquisition or pause ACK proof.
use super::*;
use crate::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
    },
    gameplay::output::ports::{
        NativeSourcePlaybackQuery, SourcePlaybackFacts, SourcePlaybackIdentity,
    },
    native_audio_presentation::NativeAudioPresentation,
};
use beatkernel::{
    audio::{
        ConvertedPhaseSpan, InterpolatedTargetTime, OutputFrameBasis, SourcePlaybackSpan,
        SourcePlaybackState, TargetFrameBasis, TargetTime,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
    transport::OutputPhaseBasis,
};
use beatkernel_platform::audio::presentation::validation::NativePresentationValidator;

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn source_basis() -> OutputFrameBasis {
    OutputFrameBasis::new(point(2, 100), 3, 1).unwrap()
}
fn identity() -> SourcePlaybackIdentity {
    SourcePlaybackIdentity::new(3, 0, 0).unwrap()
}
fn attempt() -> OutputPhaseBasis {
    OutputPhaseBasis::new(ClockDomainId(3), 11, 17, 1, 3).unwrap()
}
fn exact(t: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(t, 0, 0, 1, 0, 1).unwrap()
}
fn at(frame: u64) -> InterpolatedTargetTime {
    exact(TargetTime::from_frames(frame, 3).unwrap())
}
fn presentation(descriptor: AudioAuthorityCoordinateBasis, epoch: u64) -> NativeAudioPresentation {
    let origin = match descriptor {
        AudioAuthorityCoordinateBasis::SourceTimestamp(_) => {
            panic!("SourceTimestamp is not a native coordinate in this fixture")
        }
        AudioAuthorityCoordinateBasis::SourceFrame(b)
        | AudioAuthorityCoordinateBasis::SourceCounter { basis: b, .. } => {
            b.point_at_stream_frame(0).unwrap()
        }
        AudioAuthorityCoordinateBasis::TargetFrame(b) => b.point_at_stream_frame(0).unwrap(),
    };
    NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig::default(),
            AudioAuthorityEpoch {
                id: epoch,
                stream_origin: origin,
                logical_origin: point(3, 500),
                host_domain: ClockDomainId(1),
            },
            descriptor,
        )
        .unwrap(),
        NativePresentationValidator::new(epoch, origin, ClockDomainId(1)),
    )
    .unwrap()
}
fn logical(
    p: &NativeAudioPresentation,
    q: InterpolatedTargetTime,
    origin: ClockPoint,
) -> ClockPoint {
    p.authority()
        .epoch()
        .logical_output(q.point(origin).unwrap())
        .unwrap()
}
fn source<E>(
    q: InterpolatedTargetTime,
    span: SourcePlaybackSpan,
    evidence: E,
) -> NativeSourcePlaybackQuery<E> {
    NativeSourcePlaybackQuery::new(
        7,
        source_basis(),
        identity(),
        q,
        SourcePlaybackFacts {
            playback: span,
            pulled_source_end: 4,
            submitted_source_end: 4,
            render: None,
            revision: 2,
        },
        Some(evidence),
    )
    .unwrap()
}
fn running() -> SourcePlaybackSpan {
    SourcePlaybackSpan::new(0, None, 0, SourcePlaybackState::Running).unwrap()
}
fn target_basis() -> TargetFrameBasis {
    TargetFrameBasis::new(point(2, 100), TargetTime::from_frames(1, 3).unwrap(), 7).unwrap()
}
fn converted_facts(state: u64, playback: SourcePlaybackSpan) -> ConvertedPlaybackFacts {
    ConvertedPlaybackFacts {
        phase: ConvertedPhaseSpan::from_words([3, 1, 1, 2, 0, 1, 3, state]).unwrap(),
        segment_end: TargetTime::new(1, 0, 1).unwrap(),
        generated_target_end: TargetTime::new(1, 0, 1).unwrap(),
        submitted_target_end: TargetTime::new(0, 2, 3).unwrap(),
        playback,
        pulled_source_end: 4,
        revision: 2,
    }
}

#[test]
fn source_fractional_original_cut_floors_once_even_when_raw_points_match() {
    let p = presentation(
        AudioAuthorityCoordinateBasis::SourceFrame(source_basis()),
        7,
    );
    let before = format!("{p:?}");
    for (numerator, wanted) in [(3_333_333_348, 1), (3_333_333_342, 0)] {
        let q = exact(TargetTime::new(0, numerator, 10_000_000_000).unwrap());
        let raw = q.point(source_basis().origin()).unwrap();
        assert_eq!(raw.timestamp.as_nanos(), 333_333_434);
        let packet = source(q, running(), ());
        let mapping = NativePlaybackMapping::from_source(&packet);
        let phase = mapping
            .phase(&p, identity(), &q, attempt(), point(3, 501))
            .unwrap();
        assert_eq!(phase.elapsed_nanos(), wanted);
        assert_eq!(phase.basis(), attempt());
        assert_eq!(phase.logical(), point(3, 501));
    }
    assert_eq!(format!("{p:?}"), before);
}
#[test]
fn borrowed_noncopy_evidence_is_untouched_and_source_frozen_uses_adopted_playback() {
    struct Evidence(Vec<u64>);
    let q = at(3);
    let packet = source(
        q,
        SourcePlaybackSpan::new(2, None, 1, SourcePlaybackState::Frozen).unwrap(),
        Evidence(vec![7, 13]),
    );
    let pointer = packet.native_evidence().unwrap().0.as_ptr();
    let mapping = NativePlaybackMapping::from_source(&packet);
    let p = presentation(
        AudioAuthorityCoordinateBasis::SourceFrame(source_basis()),
        7,
    );
    assert_eq!(
        mapping
            .phase(
                &p,
                identity(),
                &q,
                attempt(),
                logical(&p, q, source_basis().origin())
            )
            .unwrap()
            .elapsed_nanos(),
        0
    );
    assert_eq!(packet.native_evidence().unwrap().0.as_ptr(), pointer);
    assert_eq!(packet.native_evidence().unwrap().0, [7, 13]);
}

#[test]
fn real_mixer_pause_adoption_selects_frozen_source_and_converted_mapping() {
    use beatkernel::audio::{
        command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank,
    };
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 128, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    mixer.render(&mut [0.0; 2]).unwrap();
    producer.request_pause(true);
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    assert!(report.paused);
    let history = mixer.source_playback_history().unwrap();
    let playback = history.lookup_at_source_time(at(2)).unwrap();
    assert_eq!(playback.state(), SourcePlaybackState::Frozen);
    assert_eq!(playback.playback_start(), 2);
    let q = at(3);
    let packet = NativeSourcePlaybackQuery::new(
        7,
        source_basis(),
        identity(),
        q,
        SourcePlaybackFacts {
            playback,
            pulled_source_end: history.pulled_end_frame(),
            submitted_source_end: 4,
            render: Some(report),
            revision: 2,
        },
        Some(()),
    )
    .unwrap();
    let p = presentation(
        AudioAuthorityCoordinateBasis::SourceFrame(source_basis()),
        7,
    );
    assert_eq!(
        NativePlaybackMapping::from_source(&packet)
            .phase(
                &p,
                identity(),
                &q,
                attempt(),
                logical(&p, q, source_basis().origin())
            )
            .unwrap()
            .elapsed_nanos(),
        333_333_333
    );
    let target_query = exact(TargetTime::new(0, 1, 2).unwrap());
    let facts = converted_facts(0, playback);
    let target_p = presentation(
        AudioAuthorityCoordinateBasis::TargetFrame(target_basis()),
        7,
    );
    assert_eq!(
        NativePlaybackMapping::converted(7, target_basis(), identity(), target_query, facts)
            .unwrap()
            .phase(
                &target_p,
                identity(),
                &target_query,
                attempt(),
                logical(&target_p, target_query, target_basis().origin())
            )
            .unwrap()
            .elapsed_nanos(),
        333_333_333
    );
}
#[test]
fn source_mapping_rejects_wrong_epoch_descriptor_identity_exact_query_point_and_hz() {
    let q = at(2);
    let packet = source(q, running(), ());
    let mapping = NativePlaybackMapping::from_source(&packet);
    let p = presentation(
        AudioAuthorityCoordinateBasis::SourceFrame(source_basis()),
        7,
    );
    let correct = logical(&p, q, source_basis().origin());
    let before = format!("{p:?}");
    assert!(mapping
        .phase(
            &presentation(
                AudioAuthorityCoordinateBasis::SourceFrame(source_basis()),
                8
            ),
            identity(),
            &q,
            attempt(),
            correct
        )
        .is_err());
    assert!(mapping
        .phase(
            &presentation(
                AudioAuthorityCoordinateBasis::TargetFrame(target_basis()),
                7
            ),
            identity(),
            &q,
            attempt(),
            correct
        )
        .is_err());
    assert!(mapping
        .phase(
            &p,
            SourcePlaybackIdentity::new(3, 1, 0).unwrap(),
            &q,
            attempt(),
            correct
        )
        .is_err());
    let extra =
        InterpolatedTargetTime::new(TargetTime::from_frames(2, 3).unwrap(), 0, 1, u64::MAX, 1, 1)
            .unwrap();
    assert_eq!(
        extra.point(source_basis().origin()).unwrap(),
        q.point(source_basis().origin()).unwrap()
    );
    assert!(mapping
        .phase(&p, identity(), &extra, attempt(), correct)
        .is_err());
    assert!(mapping
        .phase(
            &p,
            identity(),
            &q,
            attempt(),
            point(3, correct.timestamp.as_nanos() + 1)
        )
        .is_err());
    assert!(mapping
        .phase(
            &p,
            identity(),
            &q,
            OutputPhaseBasis::new(ClockDomainId(3), 11, 17, 1, 4).unwrap(),
            correct
        )
        .is_err());
    assert!(mapping
        .phase(
            &p,
            identity(),
            &q,
            OutputPhaseBasis::new(ClockDomainId(4), 11, 17, 1, 3).unwrap(),
            correct
        )
        .is_err());
    assert_eq!(format!("{p:?}"), before);
}
#[test]
fn source_counter_wide_frequency_maps_exact_query_without_rounded_elapsed() {
    let q =
        InterpolatedTargetTime::new(TargetTime::from_frames(1, 3).unwrap(), 1, 1, u64::MAX, 0, 1)
            .unwrap();
    let packet = source(q, running(), ());
    let mapping = NativePlaybackMapping::from_source(&packet);
    let p = presentation(
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis: source_basis(),
            frequency: u64::MAX,
        },
        7,
    );
    assert_eq!(
        mapping
            .phase(
                &p,
                identity(),
                &q,
                attempt(),
                logical(&p, q, source_basis().origin())
            )
            .unwrap()
            .elapsed_nanos(),
        0
    );
}

#[test]
fn closure_correspondence_accepts_original_queries_before_a_fresh_origin_without_granting_phase() {
    let source_time = TargetTime::from_frames(2, 3).unwrap();
    let target_time = TargetTime::new(0, 1, 2).unwrap();
    let source_query = exact(source_time);
    let target_query = exact(target_time);
    let fresh = OutputPhaseBasis::new(ClockDomainId(3), 11, 18, 3, 3).unwrap();
    for (mapping, descriptor, time, origin) in [
        (
            NativePlaybackMapping::from_source(&source(source_query, running(), ())),
            AudioAuthorityCoordinateBasis::SourceFrame(source_basis()),
            source_time,
            source_basis().origin(),
        ),
        (
            NativePlaybackMapping::converted(
                7,
                target_basis(),
                identity(),
                target_query,
                converted_facts(0, running()),
            )
            .unwrap(),
            AudioAuthorityCoordinateBasis::TargetFrame(target_basis()),
            target_time,
            target_basis().origin(),
        ),
    ] {
        let p = presentation(descriptor, 7);
        let q = exact(time);
        let correct = logical(&p, q, origin);
        let unchanged = format!("{p:?}");
        // Both actual mappings select playback Source frame2; a new attempt
        // begins at Source frame3. Correspondence grants no negative phase.
        assert_eq!(
            mapping.validate_correspondence(&p, identity(), &q, fresh, correct),
            Ok(())
        );
        assert_eq!(
            mapping.phase(&p, identity(), &q, fresh, correct),
            Err(NativePlaybackPhaseError::Arithmetic)
        );
        assert_eq!(
            mapping.validate_correspondence(
                &presentation(descriptor, 8),
                identity(),
                &q,
                fresh,
                correct
            ),
            Err(NativePlaybackPhaseError::Identity)
        );
        assert_eq!(
            mapping.validate_correspondence(
                &p,
                SourcePlaybackIdentity::new(3, 1, 0).unwrap(),
                &q,
                fresh,
                correct
            ),
            Err(NativePlaybackPhaseError::Identity)
        );
        assert_eq!(
            mapping.validate_correspondence(
                &p,
                identity(),
                &q,
                OutputPhaseBasis::new(ClockDomainId(3), 11, 18, 3, 4).unwrap(),
                correct
            ),
            Err(NativePlaybackPhaseError::Identity)
        );
        let extra = InterpolatedTargetTime::new(time, 0, 1, u64::MAX, 1, 1).unwrap();
        assert_eq!(extra.point(origin).unwrap(), q.point(origin).unwrap());
        assert_eq!(
            mapping.validate_correspondence(&p, identity(), &extra, fresh, correct),
            Err(NativePlaybackPhaseError::Query)
        );
        let (other_kind, other_origin) = match descriptor {
            AudioAuthorityCoordinateBasis::SourceFrame(_) => (
                AudioAuthorityCoordinateBasis::TargetFrame(target_basis()),
                AudioAuthorityCoordinateBasis::SourceFrame(
                    OutputFrameBasis::new(point(2, 101), 3, 1).unwrap(),
                ),
            ),
            AudioAuthorityCoordinateBasis::TargetFrame(_) => (
                AudioAuthorityCoordinateBasis::SourceFrame(source_basis()),
                AudioAuthorityCoordinateBasis::TargetFrame(
                    TargetFrameBasis::new(point(2, 101), TargetTime::from_frames(1, 3).unwrap(), 7)
                        .unwrap(),
                ),
            ),
            _ => unreachable!("only genuine Source/Target descriptors in this fixture"),
        };
        for bad in [other_kind, other_origin] {
            assert_eq!(
                mapping.validate_correspondence(
                    &presentation(bad, 7),
                    identity(),
                    &q,
                    fresh,
                    correct
                ),
                Err(NativePlaybackPhaseError::Descriptor)
            );
        }
        for bad in [
            point(3, correct.timestamp.as_nanos() + 1),
            point(4, correct.timestamp.as_nanos()),
        ] {
            assert_eq!(
                mapping.validate_correspondence(&p, identity(), &q, fresh, bad),
                Err(NativePlaybackPhaseError::LogicalPoint)
            );
        }
        assert_eq!(
            mapping.validate_correspondence(
                &p,
                identity(),
                &q,
                OutputPhaseBasis::new(ClockDomainId(4), 11, 18, 3, 3).unwrap(),
                correct
            ),
            Err(NativePlaybackPhaseError::Phase)
        );
        assert_eq!(format!("{p:?}"), unchanged);
    }
}
#[test]
fn converted_active_held_and_source_frozen_use_distinct_actual_mapping_facts() {
    let q = exact(TargetTime::new(0, 1, 2).unwrap());
    let p = presentation(
        AudioAuthorityCoordinateBasis::TargetFrame(target_basis()),
        7,
    );
    for (state, playback, wanted) in [
        (0, running(), 333_333_333),
        (1, running(), 166_666_666),
        (
            0,
            SourcePlaybackSpan::new(1, None, 1, SourcePlaybackState::Frozen).unwrap(),
            0,
        ),
        (
            1,
            SourcePlaybackSpan::new(1, None, 1, SourcePlaybackState::Frozen).unwrap(),
            0,
        ),
    ] {
        let mapping = NativePlaybackMapping::converted(
            7,
            target_basis(),
            identity(),
            q,
            converted_facts(state, playback),
        )
        .unwrap();
        assert_eq!(
            mapping
                .phase(
                    &p,
                    identity(),
                    &q,
                    attempt(),
                    logical(&p, q, target_basis().origin())
                )
                .unwrap()
                .elapsed_nanos(),
            wanted
        );
    }
}
#[test]
fn converted_boundaries_positive_prefix_and_fractional_future_are_checked_before_floor() {
    let q = exact(TargetTime::new(0, 1, 2).unwrap());
    let facts = converted_facts(0, running());
    for case in 0..7 {
        let mut bad = facts;
        match case {
            0 => bad.revision = 1,
            1 => bad.submitted_target_end = target_basis().start_time(),
            2 => bad.submitted_target_end = TargetTime::new(2, 0, 1).unwrap(),
            3 => bad.generated_target_end = TargetTime::new(0, 1, 3).unwrap(),
            4 => bad.segment_end = TargetTime::new(0, 1, 2).unwrap(),
            5 => bad.pulled_source_end = 1,
            _ => {
                bad.playback =
                    SourcePlaybackSpan::new(0, Some(2), 0, SourcePlaybackState::Running).unwrap()
            }
        };
        assert!(
            NativePlaybackMapping::converted(7, target_basis(), identity(), q, bad).is_err(),
            "case {case}"
        );
    }
    let end = exact(facts.submitted_target_end);
    assert!(NativePlaybackMapping::converted(7, target_basis(), identity(), end, facts).is_ok());
    let future =
        InterpolatedTargetTime::new(facts.submitted_target_end, 0, 1, u64::MAX, 1, 1).unwrap();
    assert_eq!(
        future.point(target_basis().origin()).unwrap(),
        end.point(target_basis().origin()).unwrap()
    );
    assert!(
        NativePlaybackMapping::converted(7, target_basis(), identity(), future, facts).is_err()
    );
}
#[test]
fn converted_source_halfopen_boundary_selects_next_actual_playback_span() {
    let q = exact(TargetTime::new(0, 1, 2).unwrap());
    let mut facts = converted_facts(0, running());
    facts.playback = SourcePlaybackSpan::new(2, None, 1, SourcePlaybackState::Frozen).unwrap();
    let mapping =
        NativePlaybackMapping::converted(7, target_basis(), identity(), q, facts).unwrap();
    let p = presentation(
        AudioAuthorityCoordinateBasis::TargetFrame(target_basis()),
        7,
    );
    assert_eq!(
        mapping
            .phase(
                &p,
                identity(),
                &q,
                attempt(),
                logical(&p, q, target_basis().origin())
            )
            .unwrap()
            .elapsed_nanos(),
        0
    );
}

#[test]
fn latest_generated_endpoint_is_inclusive_only_with_positive_matching_submission() {
    let mut facts = converted_facts(0, running());
    facts.submitted_target_end = facts.generated_target_end;
    let q = exact(facts.generated_target_end);
    let mapping =
        NativePlaybackMapping::converted(7, target_basis(), identity(), q, facts).unwrap();
    let p = presentation(
        AudioAuthorityCoordinateBasis::TargetFrame(target_basis()),
        7,
    );
    assert_eq!(
        mapping
            .phase(
                &p,
                identity(),
                &q,
                attempt(),
                logical(&p, q, target_basis().origin())
            )
            .unwrap()
            .elapsed_nanos(),
        833_333_333
    );
    let mut old = facts;
    old.segment_end = TargetTime::new(0, 1, 2).unwrap();
    assert!(NativePlaybackMapping::converted(
        7,
        target_basis(),
        identity(),
        exact(old.segment_end),
        old
    )
    .is_err());
}

use crate::{
    gameplay_presentation::GameplayDevice,
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{InputBatch, NativeGameplayResult},
    native_gameplay_bridge::NativeGameplayDevice,
};
use beatkernel::{audio::RenderReport, input::PhysicalInputEvent, time::ClockPair};
use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
use std::{cell::Cell, collections::VecDeque};
struct ForwardDevice {
    mapping: NativePlaybackMapping,
    calls: Cell<usize>,
}
struct UnsupportedDevice;
macro_rules! stub_device {
    ($ty:ty,$($extra:tt)*)=>{impl NativeGameplayDevice for $ty {
        fn observe(&mut self,_:&mut PresentationDiscipline)->NativeGameplayResult<()>{panic!("mapping hook must remain readonly")}
        fn render_report(&mut self)->NativeGameplayResult<Option<RenderReport>>{panic!("mapping hook must not read a separate report")}
        fn host_now(&self)->NativeGameplayResult<ClockPoint>{panic!("mapping hook must not substitute receipt time")}
        fn acquire(&mut self,_:&mut VecDeque<PhysicalInputEvent>)->NativeGameplayResult<InputBatch>{panic!("mapping hook must not consume input")}
        fn observe_end(&mut self,_:&mut NativeEnd,_:&PresentationDiscipline,_:Option<RenderReport>)->NativeGameplayResult<Option<EndBoundary>>{panic!("not exercised")}
        fn seed_resume(&mut self,_:&mut PresentationDiscipline,_:ClockPair)->NativeGameplayResult<()>{panic!("not exercised")}
        fn fallback_schedule(&mut self,_:u32)->NativeGameplayResult<ClockPoint>{panic!("mapping hook must not schedule")}
        $($extra)*
    }};
}
stub_device!(UnsupportedDevice,);
stub_device!(
    ForwardDevice,
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        q: &InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(id, self.mapping.identity());
        assert_eq!(
            q.compare_interpolated_time(self.mapping.query()),
            std::cmp::Ordering::Equal
        );
        Ok(Some(self.mapping))
    }
);
#[test]
fn actual_blanket_device_bridge_forwards_mapping_and_default_refuses() {
    let q = at(2);
    let p = presentation(
        AudioAuthorityCoordinateBasis::SourceFrame(source_basis()),
        7,
    );
    let mapping = NativePlaybackMapping::from_source(&source(q, running(), ()));
    let device = ForwardDevice {
        mapping,
        calls: Cell::new(0),
    };
    let returned = GameplayDevice::playback_mapping_at(&device, &p, &q, identity())
        .unwrap()
        .unwrap();
    assert_eq!(returned.epoch(), 7);
    assert_eq!(
        returned.query().compare_interpolated_time(q),
        std::cmp::Ordering::Equal
    );
    assert_eq!(device.calls.get(), 1);
    assert!(GameplayDevice::playback_mapping_at(&UnsupportedDevice, &p, &q, identity()).is_err());
    assert!(
        NativeGameplayDevice::playback_mapping_at(&UnsupportedDevice, &p, &q, identity()).is_err()
    );
}

struct ActualRuntimeRateMapping {
    output: beatkernel_platform::audio::ConvertedNativeOutputState,
    source: OutputFrameBasis,
    target: TargetFrameBasis,
    projected: beatkernel::audio::ProjectedPracticeEvent,
    facts: ConvertedPlaybackFacts,
}

fn actual_runtime_rate_mapping() -> ActualRuntimeRateMapping {
    use beatkernel::audio::*;
    use beatkernel::time::Duration;
    use beatkernel::transport::Rate;
    use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(256, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 64], limits).unwrap(),
    )
    .unwrap();
    let program = PreparedPracticeProgram::new_with_runtime_rates(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        PracticeLimits::new(8, 1, 64, 8, 16).unwrap(),
        Rate::new(1, 2).unwrap(),
        Duration::ZERO,
        false,
        14105,
    )
    .unwrap();
    let (mut controller, endpoint) = practice_queue(&program).unwrap();
    let (_, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(1_000_000_000), false)
                .unwrap(),
        )
        .unwrap();
    let source = mixer.output_frame_basis();
    let mut output = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap_or_else(|_| panic!("valid actual converter"));
    output.enable_phase_history(16).unwrap();
    output.enable_source_playback_history(16).unwrap();
    let target = output.target_frame_basis();
    output.render_pending(24).unwrap();
    output.admit(24).unwrap();
    assert!(matches!(
        controller.try_pop_projected_event().unwrap().event,
        PracticeEvent::Boundary(PracticeReceipt {
            kind: PracticeBoundaryKind::Started,
            ..
        })
    ));
    assert_eq!(output.converter_owner().pulled_source_frame_cursor(), 17);
    let preparation = controller.prepare_rate(Rate::new(3, 2).unwrap()).unwrap();
    controller.try_set_rate_next(12, 1, &preparation).unwrap();
    output.render_pending(16).unwrap();
    output.admit(16).unwrap();
    let projected = controller.try_pop_projected_event().unwrap();
    assert_eq!(projected.boundary.source_frame, 17);
    assert_eq!(projected.target_frame, 26);
    assert_eq!(
        projected.boundary.target_time,
        TargetTime::from_frames(26, 1500).unwrap()
    );
    let query = exact(projected.boundary.target_time);
    let history = output.phase_history().unwrap();
    let (phase, segment_end) = (0..history.len())
        .find_map(|index| {
            let phase = history.span(index).unwrap();
            let words = phase.to_words();
            let start = TargetTime::new(words[4], words[5], words[6]).unwrap();
            let end = history
                .span(index + 1)
                .map(|next| {
                    let words = next.to_words();
                    TargetTime::new(words[4], words[5], words[6]).unwrap()
                })
                .unwrap_or(history.end_time());
            (!query.compare_target_time(start).is_lt()
                && !query.compare_target_time(end).is_gt()
                && (index + 1 == history.len() || !query.compare_target_time(end).is_eq()))
            .then_some((phase, end))
        })
        .expect("original projected event lies inside actual converter history");
    let history_source = output.converter_owner().source_playback_history().unwrap();
    let playback = (0..history_source.len())
        .filter_map(|index| history_source.span(index))
        .find(|span| {
            !phase
                .compare_source_frame_at_interpolated_time(
                    query,
                    segment_end,
                    span.physical_start(),
                )
                .unwrap()
                .is_lt()
                && span.physical_end().is_none_or(|end| {
                    phase
                        .compare_source_frame_at_interpolated_time(query, segment_end, end)
                        .unwrap()
                        .is_lt()
                })
        })
        .unwrap();
    let facts = ConvertedPlaybackFacts {
        phase,
        segment_end,
        generated_target_end: history.end_time(),
        submitted_target_end: output.admitted_target_time().unwrap(),
        playback,
        pulled_source_end: history_source.pulled_end_frame(),
        revision: 2,
    };
    ActualRuntimeRateMapping {
        output,
        source,
        target,
        projected,
        facts,
    }
}

#[test]
fn runtime_rate_exact_source17_cut_differs_from_actual_first_target26_projection() {
    use beatkernel::audio::PracticeEvent;
    let fixture = actual_runtime_rate_mapping();
    let p = presentation(
        AudioAuthorityCoordinateBasis::TargetFrame(fixture.target),
        7,
    );
    let id = SourcePlaybackIdentity::new(1000, 0, 0).unwrap();
    let attempt = OutputPhaseBasis::new(ClockDomainId(3), 41, 1, 0, 1000).unwrap();
    let projected_query = exact(fixture.projected.boundary.target_time);
    let projected_logical = logical(&p, projected_query, fixture.target.origin());
    let event = match fixture.projected.event {
        PracticeEvent::Rate(event) => event,
        _ => unreachable!(),
    };
    assert_eq!(
        (
            event.physical_frame,
            event.playback_frame,
            event.active_elapsed_nanos
        ),
        (17, 17, 17_000_000)
    );
    let mapping =
        NativePlaybackMapping::converted(7, fixture.target, id, projected_query, fixture.facts)
            .unwrap();
    let original_phase = mapping
        .phase(&p, id, &projected_query, attempt, projected_logical)
        .unwrap();
    assert_eq!(original_phase.elapsed_nanos(), 17_333_333);
    assert_eq!(
        mapping.compare_physical_source_frame(17).unwrap(),
        std::cmp::Ordering::Greater
    );
    assert_eq!(
        mapping.compare_physical_source_frame(18).unwrap(),
        std::cmp::Ordering::Less
    );
    let (query, phase) = mapping
        .runtime_rate_cut(
            &p,
            id,
            attempt,
            (&projected_query, projected_logical),
            event,
        )
        .unwrap();
    assert!(query
        .compare_target_time(TargetTime::from_frames(17, 1000).unwrap())
        .is_eq());
    assert!(query.compare_interpolated_time(projected_query).is_lt());
    assert_eq!(phase.elapsed_nanos(), 17_000_000);
    assert_eq!(phase.logical(), point(3, 17_000_500));
    assert_eq!(phase.basis(), attempt);
    let exact_mapping =
        NativePlaybackMapping::converted(7, fixture.target, id, query, fixture.facts).unwrap();
    for (frame, expected) in [
        (16, std::cmp::Ordering::Greater),
        (17, std::cmp::Ordering::Equal),
        (18, std::cmp::Ordering::Less),
    ] {
        assert_eq!(
            exact_mapping.compare_physical_source_frame(frame).unwrap(),
            expected
        );
    }
    assert_eq!(
        fixture.projected.target_frame, 26,
        "original native projected proof is not retimed"
    );
    assert!(fixture
        .facts
        .phase
        .compare_source_frame_at_interpolated_time(query, fixture.facts.segment_end, 17)
        .unwrap()
        .is_eq());

    let source_p = presentation(
        AudioAuthorityCoordinateBasis::SourceFrame(fixture.source),
        7,
    );
    let source_query = exact(TargetTime::from_frames(17, 1000).unwrap());
    let history = fixture
        .output
        .converter_owner()
        .source_playback_history()
        .unwrap();
    let packet = NativeSourcePlaybackQuery::new(
        7,
        fixture.source,
        id,
        source_query,
        SourcePlaybackFacts {
            playback: fixture.facts.playback,
            pulled_source_end: history.pulled_end_frame(),
            submitted_source_end: history.pulled_end_frame(),
            render: None,
            revision: 2,
        },
        Some(()),
    )
    .unwrap();
    let source_mapping = NativePlaybackMapping::from_source(&packet);
    for (frame, expected) in [
        (16, std::cmp::Ordering::Greater),
        (17, std::cmp::Ordering::Equal),
        (18, std::cmp::Ordering::Less),
    ] {
        assert_eq!(
            source_mapping.compare_physical_source_frame(frame).unwrap(),
            expected
        );
    }
    let (direct_query, direct_phase) = source_mapping
        .runtime_rate_cut(
            &source_p,
            id,
            attempt,
            (
                &source_query,
                logical(&source_p, source_query, fixture.source.origin()),
            ),
            event,
        )
        .unwrap();
    assert!(direct_query.compare_interpolated_time(query).is_eq());
    assert_eq!(direct_phase, phase);
}

#[test]
fn runtime_rate_exact_cut_rejects_wrong_identity_generation_elapsed_playback_and_query() {
    use beatkernel::audio::PracticeEvent;
    let fixture = actual_runtime_rate_mapping();
    let p = presentation(
        AudioAuthorityCoordinateBasis::TargetFrame(fixture.target),
        7,
    );
    let id = SourcePlaybackIdentity::new(1000, 0, 0).unwrap();
    let attempt = OutputPhaseBasis::new(ClockDomainId(3), 41, 1, 0, 1000).unwrap();
    let query = exact(fixture.projected.boundary.target_time);
    let at = logical(&p, query, fixture.target.origin());
    let event = match fixture.projected.event {
        PracticeEvent::Rate(event) => event,
        _ => unreachable!(),
    };
    let mapping =
        NativePlaybackMapping::converted(7, fixture.target, id, query, fixture.facts).unwrap();
    let before = format!("{p:?}");
    assert!(mapping
        .runtime_rate_cut(
            &p,
            SourcePlaybackIdentity::new(1000, 1, 0).unwrap(),
            attempt,
            (&query, at),
            event
        )
        .is_err());
    let wrong_generation = OutputPhaseBasis::new(ClockDomainId(3), 41, 2, 0, 1000).unwrap();
    assert!(mapping
        .runtime_rate_cut(&p, id, wrong_generation, (&query, at), event)
        .is_err());
    let mut wrong_e = event;
    wrong_e.active_elapsed_nanos = 17_333_333;
    let mut wrong_playback = event;
    wrong_playback.playback_frame = 16;
    let mut wrong_physical = event;
    wrong_physical.physical_frame = 16;
    for invalid in [wrong_e, wrong_playback, wrong_physical] {
        assert!(mapping
            .runtime_rate_cut(&p, id, attempt, (&query, at), invalid)
            .is_err());
    }
    let wrong_query = exact(TargetTime::from_frames(17, 1000).unwrap());
    assert!(mapping
        .runtime_rate_cut(&p, id, attempt, (&wrong_query, at), event)
        .is_err());
    assert_eq!(
        format!("{p:?}"),
        before,
        "pure mapping grants no native mutation"
    );
}

#[test]
fn runtime_rate_exact_cut_refuses_held_or_no_longer_retained_converter_span() {
    use beatkernel::audio::{ConvertedPhaseSpan, PracticeEvent};
    let fixture = actual_runtime_rate_mapping();
    let p = presentation(
        AudioAuthorityCoordinateBasis::TargetFrame(fixture.target),
        7,
    );
    let id = SourcePlaybackIdentity::new(1000, 0, 0).unwrap();
    let attempt = OutputPhaseBasis::new(ClockDomainId(3), 41, 1, 0, 1000).unwrap();
    let query = exact(fixture.projected.boundary.target_time);
    let at = logical(&p, query, fixture.target.origin());
    let event = match fixture.projected.event {
        PracticeEvent::Rate(event) => event,
        _ => unreachable!(),
    };
    let mut held = fixture.facts;
    let mut words = held.phase.to_words();
    words[7] = 1;
    held.phase = ConvertedPhaseSpan::from_words(words).unwrap();
    let mapping = NativePlaybackMapping::converted(7, fixture.target, id, query, held).unwrap();
    assert!(mapping
        .runtime_rate_cut(&p, id, attempt, (&query, at), event)
        .is_err());
    // An otherwise valid active span starts at the actual first crossing. The
    // integer Source17 cut is strictly before it, so inversion must not extrapolate.
    let mut shortened = fixture.facts;
    let mut words = shortened.phase.to_words();
    words[1] = 17;
    words[2] = 1;
    words[3] = 3;
    let time = fixture.projected.boundary.target_time;
    words[4] = time.seconds();
    words[5] = time.numerator();
    words[6] = time.denominator();
    shortened.phase = ConvertedPhaseSpan::from_words(words).unwrap();
    let mapping =
        NativePlaybackMapping::converted(7, fixture.target, id, query, shortened).unwrap();
    assert!(mapping
        .runtime_rate_cut(&p, id, attempt, (&query, at), event)
        .is_err());
    // An invalidated upper bound must remain an arithmetic range refusal. This
    // intentionally invalid descriptor grants no native proof or admission.
    let mut invalid_range = mapping;
    if let Mapping::Converted { ref mut facts, .. } = invalid_range.mapping {
        facts.segment_end = TargetTime::from_frames(25, 1500).unwrap();
    }
    assert!(invalid_range.compare_physical_source_frame(17).is_err());
}
