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
    native_gameplay_bridge::NativeGameplayDevice,
    native_gameplay::{NativeGameplayResult, InputBatch},
    native_end::{NativeEnd, EndBoundary},
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
