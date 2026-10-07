//! Deferred real render/interval provenance; supplied bounds prove no acoustics.
use super::*;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use beatkernel_platform::audio::{
    asio::{AsioPresentationObservation, AsioPresentationError, MultimediaHostInterval},
    presentation::discipline::{PresentationDiscipline, DisciplineConfig, DisciplineError},
};
use crate::live_pause::LivePauseObservation;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(3, 1).unwrap();
    let limits = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::from_nanos(-2_000_000_000),
            gain: 0.5,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(-2_000_000_000),
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 3]).unwrap();
    (producer, mixer)
}
fn observation(mixer: &mut Mixer, before: i64, after: i64) -> AsioPresentationObservation {
    let report = mixer.render(&mut [0.; 1]).unwrap();
    AsioPresentationObservation::from_render(
        report,
        3,
        MultimediaHostInterval {
            before: point(1, before),
            after: point(1, after),
        },
        1,
        7,
        mixer.output_frame_basis().origin(),
    )
    .unwrap()
}
fn observer(epoch: u64) -> PresentationDiscipline {
    let origin = point(2, -2_000_000_000);
    let mut p = PresentationDiscipline::new(
        DisciplineConfig::default(),
        origin,
        ClockDomainId(1),
        Timestamp::from_nanos(604_800_000_000_000),
    )
    .unwrap();
    if epoch != 0 {
        p.rebind_output(
            epoch,
            origin,
            origin,
            Timestamp::from_nanos(604_800_000_000_000),
        )
        .unwrap();
    }
    p
}
#[test]
fn optional_observation_waits_but_stale_epoch_refuses_before_none_or_malformed_metadata() {
    let (_producer, mut mixer) = rig();
    let mut original = observation(&mut mixer, 1_000_000_000, 1_000_000_010);
    original.sample_rate = 0;
    for epoch in [7, u64::MAX] {
        let mut p = observer(epoch);
        let before = format!("{p:?}");
        assert!(matches!(
            observe_asio(&mut p, 0, None),
            Err(ReplacementObservationError::EpochMismatch)
        ));
        assert_eq!(format!("{p:?}"), before);
        assert!(matches!(
            observe_asio(&mut p, 0, Some(original)),
            Err(ReplacementObservationError::EpochMismatch)
        ));
        assert_eq!(format!("{p:?}"), before);
        assert!(!observe_asio(&mut p, epoch, None).unwrap());
        assert_eq!(p.latest_pair(), None);
        assert_eq!(format!("{p:?}"), before);
    }
}
#[test]
fn actual_advanced_paused_report_uses_original_absolute_grid_and_full_latency_bounds_in_pause_projection()
 {
    let (_producer, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    assert_eq!(basis.start_physical_frame(), 5);
    let original = observation(&mut mixer, 1_000_000_000, 1_000_000_010);
    assert_eq!(
        (
            original.render.start_frame,
            original.render.playback_start_frame,
            original.render.playback_frames
        ),
        (5, 2, 0)
    );
    assert_eq!(original.output, point(2, -333_333_334));
    assert_eq!(original.output_origin, basis.origin());
    assert_eq!(original.host.before, point(1, 1_333_333_326));
    assert_eq!(original.host.after, point(1, 1_333_333_351));
    let now = point(1, 2_000_000_000);
    match asio_pause_observation(Some(original), now) {
        LivePauseObservation::Interval {
            observation: Some(projected),
            now: actual,
        } => {
            assert_eq!(actual, now);
            assert_eq!(projected.output_origin, original.output_origin);
            assert_eq!(projected.sample_rate, 3);
            assert_eq!(projected.render, original.render);
            assert_eq!(projected.clock.output, original.output);
            assert_eq!(projected.clock.before, original.host.before);
            assert_eq!(projected.clock.after, original.host.after);
        }
        _ => panic!("original full interval required"),
    }
    assert_eq!(
        asio_pause_observation(None, now),
        LivePauseObservation::Interval {
            observation: None,
            now
        }
    );
    let mut p = observer(u64::MAX);
    assert!(observe_asio(&mut p, u64::MAX, Some(original)).unwrap());
    assert_eq!(p.latest_pair().unwrap().source, original.output);
    assert_eq!(p.latest_pair().unwrap().target, point(1, 1_333_333_338));
}
#[test]
fn duplicate_and_host_suppressed_blocks_do_not_refresh_or_replace_accepted_interval_then_real_progress_and_pcm_continue()
 {
    let (mut producer, mut mixer) = rig();
    let first = observation(&mut mixer, 1_000_000_000, 1_000_000_010);
    let mut p = observer(7);
    assert!(observe_asio(&mut p, 7, Some(first)).unwrap());
    let before = format!("{p:?}");
    assert!(!observe_asio(&mut p, 7, Some(first)).unwrap());
    assert_eq!(format!("{p:?}"), before);
    let coarse = observation(&mut mixer, 1_000_000_000, 1_000_000_010);
    assert!(!observe_asio(&mut p, 7, Some(coarse)).unwrap());
    assert_eq!(format!("{p:?}"), before);
    assert_eq!(
        p.validate_host(point(1, 3_333_333_339)),
        Err(DisciplineError::Stale)
    );
    let next = observation(&mut mixer, 1_666_666_667, 1_666_666_677);
    assert!(observe_asio(&mut p, 7, Some(next)).unwrap());
    assert_eq!(p.latest_pair().unwrap().source, point(2, 333_333_333));
    assert_eq!(mixer.playback_frame_cursor(), 2);
    producer.request_pause(false);
    let mut pcm = [0.; 1];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.375]);
}
#[test]
fn malformed_intervals_report_extents_rate_domain_and_source_changes_refuse_atomically() {
    let (_producer, mut mixer) = rig();
    let first = observation(&mut mixer, 1_000_000_000, 1_000_000_010);
    let mut p = observer(7);
    observe_asio(&mut p, 7, Some(first)).unwrap();
    let before = format!("{p:?}");
    // Explicit malformed metadata injection; Mixer did not emit these changes.
    for case in 0..7 {
        let mut bad = first;
        match case {
            0 => bad.sample_rate = 0,
            1 => bad.output = point(2, bad.output.timestamp.as_nanos() + 1),
            2 => bad.host.after = point(9, 2_000_000_000),
            3 => bad.host.before = point(1, bad.host.after.timestamp.as_nanos() + 1),
            4 => bad.render.frames = 0,
            5 => bad.render.start_frame = u64::MAX,
            _ => bad.output_origin = point(2, -1_999_999_999),
        }
        assert!(observe_asio(&mut p, 7, Some(bad)).is_err());
        assert_eq!(format!("{p:?}"), before);
    }
    let report = mixer.render(&mut [0.; 1]).unwrap();
    let changed_rate = AsioPresentationObservation::from_render(
        report,
        4,
        MultimediaHostInterval {
            before: point(1, 2_000_000_000),
            after: point(1, 2_000_000_010),
        },
        0,
        0,
        point(2, -2_000_000_000),
    )
    .unwrap();
    assert!(matches!(
        observe_asio(&mut p, 7, Some(changed_rate)),
        Err(ReplacementObservationError::Discipline(
            DisciplineError::FrequencyChanged
        ))
    ));
    assert_eq!(format!("{p:?}"), before);
    let mut supplied = observer(7);
    supplied
        .observe_clock_pair_in_epoch(
            7,
            beatkernel::time::ClockPair {
                source: first.output,
                target: first.host.before,
            },
        )
        .unwrap();
    let before = format!("{supplied:?}");
    assert!(observe_asio(&mut supplied, 7, Some(first)).is_err());
    assert_eq!(format!("{supplied:?}"), before);
}
#[test]
fn complete_extreme_host_bounds_are_preserved_without_midpoint_substitution_and_overflow_is_explicit()
 {
    let (_producer, mut mixer) = rig();
    let report = mixer.render(&mut [0.; 1]).unwrap();
    let original = AsioPresentationObservation::from_render(
        report,
        3,
        MultimediaHostInterval {
            before: point(1, i64::MIN),
            after: point(1, i64::MAX),
        },
        0,
        0,
        point(2, -2_000_000_000),
    )
    .unwrap();
    match asio_pause_observation(Some(original), point(1, 0)) {
        LivePauseObservation::Interval {
            observation: Some(value),
            ..
        } => {
            assert_eq!(value.clock.before, point(1, i64::MIN));
            assert_eq!(value.clock.after, point(1, i64::MAX));
            assert_eq!(value.render, report);
        }
        _ => panic!("complete interval required"),
    }
    assert_eq!(
        AsioPresentationObservation::from_render(
            report,
            3,
            MultimediaHostInterval {
                before: point(1, i64::MAX),
                after: point(1, i64::MAX)
            },
            1,
            0,
            point(2, -2_000_000_000)
        ),
        Err(AsioPresentationError::Overflow)
    );
}
#[test]
fn staged_advanced_stream_origin_admits_original_absolute_asio_report_without_relabeling_or_adding_basis_twice()
 {
    let (_producer, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    let original = observation(&mut mixer, 1_000_000_000, 1_000_000_010);
    let native_zero = basis.point_at_stream_frame(0).unwrap();
    assert_ne!(native_zero, original.output_origin);
    assert_eq!(native_zero, original.output);
    for epoch in [7, u64::MAX] {
        let mut p = PresentationDiscipline::new(
            DisciplineConfig::default(),
            native_zero,
            ClockDomainId(1),
            Timestamp::from_nanos(604_800_000_000_000),
        )
        .unwrap();
        p.rebind_output(
            epoch,
            native_zero,
            native_zero,
            Timestamp::from_nanos(604_800_000_000_000),
        )
        .unwrap();
        let before = format!("{p:?}");
        let mut malformed = original;
        malformed.sample_rate = 0;
        assert!(matches!(
            observe_asio_with_basis(&mut p, 0, None, basis),
            Err(ReplacementObservationError::EpochMismatch)
        ));
        assert!(matches!(
            observe_asio_with_basis(&mut p, 0, Some(malformed), basis),
            Err(ReplacementObservationError::EpochMismatch)
        ));
        assert_eq!(format!("{p:?}"), before);
        assert!(!observe_asio_with_basis(&mut p, epoch, None, basis).unwrap());
        assert_eq!(p.latest_pair(), None);
        assert!(observe_asio_with_basis(&mut p, epoch, Some(original), basis).unwrap());
        assert_eq!(p.latest_pair().unwrap().source, point(2, -333_333_334));
        let accepted = format!("{p:?}");
        assert!(!observe_asio_with_basis(&mut p, epoch, Some(original), basis).unwrap());
        assert_eq!(format!("{p:?}"), accepted);
        for case in 0..3 {
            let mut bad = original;
            match case {
                0 => bad.sample_rate = 4,
                1 => bad.output_origin = point(2, -1_999_999_999),
                _ => bad.output = point(2, 1_333_333_333),
            }
            assert!(observe_asio_with_basis(&mut p, epoch, Some(bad), basis).is_err());
            assert_eq!(format!("{p:?}"), accepted);
        }
        let changed = OutputFrameBasis::new(basis.origin(), 3, 6).unwrap();
        assert!(observe_asio_with_basis(&mut p, epoch, Some(original), changed).is_err());
        assert_eq!(format!("{p:?}"), accepted);
        assert!(observe_asio(&mut p, epoch, Some(original)).is_err());
        assert_eq!(format!("{p:?}"), accepted);
        // An actual earlier rendered block is not admissible to this creation basis.
        let format = AudioFormat::new(3, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(64, 128, 1).unwrap()).unwrap();
        let (_producer, consumer) = command_queue(8).unwrap();
        let mut old = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::from_nanos(-2_000_000_000),
                AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        let early = observation(&mut old, 2_000_000_000, 2_000_000_010);
        assert_eq!(early.render.start_frame, 0);
        assert!(observe_asio_with_basis(&mut p, epoch, Some(early), basis).is_err());
        assert_eq!(format!("{p:?}"), accepted);
    }
    // Even at zero offset, tagged rich source identity distinguishes the legacy
    // unbased path from the explicitly based path within an epoch.
    let zero = OutputFrameBasis::new(basis.origin(), 3, 0).unwrap();
    let mut legacy = observer(7);
    observe_asio(&mut legacy, 7, Some(original)).unwrap();
    let before = format!("{legacy:?}");
    assert!(matches!(
        observe_asio_with_basis(&mut legacy, 7, Some(original), zero),
        Err(ReplacementObservationError::Discipline(
            DisciplineError::ObservationSourceChanged
        ))
    ));
    assert_eq!(format!("{legacy:?}"), before);
}
