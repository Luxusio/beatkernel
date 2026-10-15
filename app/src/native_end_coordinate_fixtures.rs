//! Real finite mixer reports and mathematical native qualification; no hardware proof.
use super::*;
use crate::audio_authority::{
    AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
};
use crate::native_output_coordinate::NativeOutputCoordinate;
use crate::gameplay::output::ports::SourcePlaybackIdentity;
use crate::local_input::InputMerger;
use beatkernel::audio::{
    command_queue, command_queue_with_start_gate, AudioFormat, AudioLimits, Mixer, MixerConfig,
    OutputFrameBasis, PcmLimits, SampleBank, TargetBoundary, TargetFrameBasis, TargetTime,
};
use beatkernel::input::{
    ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent,
};
use beatkernel::time::{Duration, ExtrapolationPolicy};
use beatkernel::transport::{OutputPhaseBasis, Rate, Transport};
use beatkernel_platform::audio::ConvertedBoundaryFacts;
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
fn identity(hz: u32) -> SourcePlaybackIdentity {
    SourcePlaybackIdentity::new(hz, 0, 0).unwrap()
}
fn counter(hz: u32, frequency: u64) -> AudioAuthorityCoordinateBasis {
    AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(point(2, 0), hz, 0).unwrap(),
        frequency,
    }
}
fn owner(basis: AudioAuthorityCoordinateBasis) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            history_capacity: 8,
            max_observation_age: Duration::from_nanos(100),
            input_extrapolation: ExtrapolationPolicy::Bounded {
                before: Duration::ZERO,
                after: Duration::ZERO,
            },
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: basis.origin().unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        basis,
    )
    .unwrap()
}
fn pair(basis: AudioAuthorityCoordinateBasis, units: u64, at: i64) -> ClockPair {
    ClockPair {
        source: match basis {
            AudioAuthorityCoordinateBasis::SourceTimestamp(_) => {
                panic!("SourceTimestamp is not a native coordinate in this fixture")
            }
            AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency } => {
                basis.point_at_native_counter(units, frequency).unwrap()
            }
            AudioAuthorityCoordinateBasis::SourceFrame(basis) => {
                basis.point_at_stream_frame(units).unwrap()
            }
            AudioAuthorityCoordinateBasis::TargetFrame(basis) => {
                basis.point_at_stream_frame(units).unwrap()
            }
        },
        target: host(at),
    }
}
fn observe(
    authority: &mut AudioAuthority,
    basis: AudioAuthorityCoordinateBasis,
    units: u64,
    at: i64,
) {
    let coordinate = match basis {
        AudioAuthorityCoordinateBasis::SourceTimestamp(_) => {
            panic!("SourceTimestamp is not a native coordinate in this fixture")
        }
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => {
            NativeOutputCoordinate::Counter {
                position: units,
                frequency,
            }
        }
        _ => NativeOutputCoordinate::StreamFrame { frame: units },
    };
    authority
        .observe_coordinate(7, pair(basis, units, at), coordinate)
        .unwrap();
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}
fn event(at: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(at), at as u64),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}
fn mixer(hz: u32, end: u64, gated: bool) -> (Mixer, beatkernel::audio::CommandProducer) {
    let (producer, consumer) = if gated {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    let format = AudioFormat::new(hz, 1).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        )
        .with_playback_end_frame(end),
        SampleBank::new(format, PcmLimits::new(1024, 1024, 2).unwrap()).unwrap(),
        consumer,
    )
    .unwrap();
    (mixer, producer)
}
fn finite_report(hz: u32, end: u64) -> RenderReport {
    let (mut mixer, _producer) = mixer(hz, end, false);
    mixer.render(&mut vec![0.0; end as usize + 1]).unwrap()
}
fn end_owner(hz: u32, end: u64, basis: AudioAuthorityCoordinateBasis) -> NativeEnd {
    NativeEnd::new(point(2, 0), ClockDomainId(1), hz, end)
        .unwrap()
        .with_coordinates(7, basis, identity(hz))
        .unwrap()
}
fn qualified(basis: AudioAuthorityCoordinateBasis, lower: u64, upper: u64) -> AudioAuthority {
    let mut authority = owner(basis);
    observe(&mut authority, basis, lower, 100);
    observe(&mut authority, basis, upper, 110);
    authority.record_acquired_prefix(host(110)).unwrap();
    authority
}
fn ack(
    end: &mut NativeEnd,
    staged: PreparedCoordinateEnd,
    authority: &AudioAuthority,
    merger: &InputMerger,
) -> CoordinateEndBoundary {
    let query = staged.query().unwrap();
    let cutoff = authority
        .presented_coordinate_boundary_host(&query, host(110))
        .unwrap()
        .unwrap();
    let token = authority
        .prepare_coordinate_resume_control_cutoff(7, query, cutoff, host(110), merger)
        .unwrap()
        .unwrap();
    end.commit_coordinate_end(staged, authority, &token, host(110), merger)
        .unwrap()
}

#[test]
fn fractional_before_equal_after_share_floor_but_only_exact_crossing_qualifies() {
    let basis = counter(3, 3_000_000_000);
    let end = end_owner(3, 1, basis);
    let report = finite_report(3, 1);
    let staged = end
        .stage_coordinate_source(
            7,
            basis,
            identity(3),
            Some(report),
            pair(basis, 999_999_999, 100),
        )
        .unwrap();
    assert_eq!(end.physical, None);
    assert!(!end.emitted);
    let query = staged.query().unwrap();
    let mut authority = owner(basis);
    observe(&mut authority, basis, 999_999_998, 99);
    observe(&mut authority, basis, 999_999_999, 100);
    authority.record_acquired_prefix(host(100)).unwrap();
    assert_eq!(
        authority
            .presented_coordinate_boundary_host(&query, host(100))
            .unwrap(),
        None
    );
    observe(&mut authority, basis, 1_000_000_000, 105);
    authority.record_acquired_prefix(host(105)).unwrap();
    assert_eq!(
        authority
            .presented_coordinate_boundary_host(&query, host(105))
            .unwrap(),
        Some(host(105))
    );
    observe(&mut authority, basis, 1_000_000_001, 110);
    authority.record_acquired_prefix(host(110)).unwrap();
    for units in [999_999_999, 1_000_000_000, 1_000_000_001] {
        assert_eq!(pair(basis, units, 110).source, point(2, 333_333_333));
    }
    let mut committed = end.clone();
    let boundary = ack(&mut committed, staged, &authority, &merger());
    assert_eq!(boundary.host, host(105));
    assert_eq!(boundary.raw_output, point(2, 333_333_333));
    assert_eq!(boundary.output, point(3, 333_333_833));
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (1, 1));
    assert!(committed.emitted);
}

#[test]
fn end_cutoff_uses_literal_minimum_integer_host_ceiling() {
    let basis = counter(3, 9);
    let mut end = end_owner(3, 1, basis);
    let staged = end
        .stage_coordinate_source(
            7,
            basis,
            identity(3),
            Some(finite_report(3, 1)),
            pair(basis, 2, 100),
        )
        .unwrap();
    let authority = qualified(basis, 2, 5);
    let boundary = ack(&mut end, staged, &authority, &merger());
    // 100 + (3-2)/(5-2)*10 = 103 1/3; first integer is 104.
    assert_eq!(boundary.host, host(104));
}

#[test]
fn finite_equality_payload_is_excluded_and_earlier_input_must_drain() {
    let basis = counter(3, 9);
    let mut end = end_owner(3, 1, basis);
    let staged = end
        .stage_coordinate_source(
            7,
            basis,
            identity(3),
            Some(finite_report(3, 1)),
            pair(basis, 2, 100),
        )
        .unwrap();
    let authority = qualified(basis, 2, 5);
    let mut merger = merger();
    let earlier = event(103);
    let equality = event(104);
    merger.admit(earlier.clone(), host(110)).unwrap();
    merger.admit(equality.clone(), host(110)).unwrap();
    assert!(authority
        .prepare_coordinate_resume_control_cutoff(
            7,
            staged.query().unwrap(),
            host(104),
            host(110),
            &merger
        )
        .unwrap()
        .is_none());
    assert!(!end.emitted);
    assert_eq!(merger.pop_ready(host(110)).unwrap(), Some(earlier));
    let boundary = ack(&mut end, staged, &authority, &merger);
    assert_eq!(boundary.host, host(104));
    assert_eq!(merger.peek_ready(host(110)).unwrap(), Some(&equality));
    assert_eq!(authority.committed_operation(), None);
    assert_eq!(authority.closed_host_prefix(), None);
}

#[test]
fn distinct_exact_cut_with_same_host_and_floor_cannot_ack_original_endpoint() {
    let basis = counter(3, 3_000_000_000);
    let mut end = end_owner(3, 1, basis);
    let staged = end
        .stage_coordinate_source(
            7,
            basis,
            identity(3),
            Some(finite_report(3, 1)),
            pair(basis, 999_999_999, 100),
        )
        .unwrap();
    let authority = qualified(basis, 999_999_999, 1_000_000_002);
    let original_host = authority
        .presented_coordinate_boundary_host(&staged.query().unwrap(), host(110))
        .unwrap()
        .unwrap();
    let distinct = beatkernel::audio::InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        1_000_000_000,
        1_000_000_001,
        3_000_000_000,
        1,
        100,
    )
    .unwrap();
    assert_eq!(
        distinct.compare_interpolated_time(staged.query().unwrap()),
        Ordering::Greater
    );
    let merger = merger();
    let token = authority
        .prepare_coordinate_resume_control_cutoff(7, distinct, original_host, host(110), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(token.raw_output(), point(2, 333_333_333));
    let before = end.clone();
    assert!(end
        .commit_coordinate_end(staged, &authority, &token, host(110), &merger)
        .is_err());
    assert_eq!(end, before);
}

#[test]
fn staging_report_alone_is_pure_and_missing_endpoint_has_no_cut() {
    let basis = counter(3, 9);
    let mut end = end_owner(3, 4, basis);
    let (mut mixer, _producer) = mixer(3, 4, false);
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    let before = end.clone();
    let staged = end
        .stage_coordinate_source(7, basis, identity(3), Some(report), pair(basis, 0, 100))
        .unwrap();
    assert_eq!(staged.physical_frame(), None);
    assert!(staged.query().is_none());
    assert_eq!(end, before);
    end.commit_coordinate_observation(staged).unwrap();
    assert!(!end.emitted && end.deferred_boundary.is_none());
    let no_report = end
        .stage_coordinate_source(7, basis, identity(3), None, pair(basis, 1, 101))
        .unwrap();
    assert!(no_report.query().is_none());
}

#[test]
fn malformed_source_report_and_identity_epoch_descriptor_refuse_atomically() {
    let basis = counter(3, 9);
    let end = end_owner(3, 1, basis);
    let report = finite_report(3, 1);
    for mutation in 0..7 {
        let mut report = report;
        let mut actual_basis = basis;
        let mut actual_identity = identity(3);
        let mut epoch = 7;
        match mutation {
            0 => report.playback_end_physical_frame = None,
            1 => report.playback_end_physical_frame = Some(2),
            2 => report.counters.rendered_frames = 0,
            3 => report.playback_frames += 1,
            4 => epoch = 8,
            5 => actual_basis = counter(3, 10),
            _ => actual_identity = identity(4),
        }
        let before = end.clone();
        assert!(end
            .stage_coordinate_source(
                epoch,
                actual_basis,
                actual_identity,
                Some(report),
                pair(basis, 0, 100)
            )
            .is_err());
        assert_eq!(end, before);
    }
}

#[test]
fn full_owner_snapshot_staleness_and_duplicate_ack_are_atomic() {
    let basis = counter(3, 9);
    let mut end = end_owner(3, 1, basis);
    let staged = end
        .stage_coordinate_source(
            7,
            basis,
            identity(3),
            Some(finite_report(3, 1)),
            pair(basis, 2, 100),
        )
        .unwrap();
    let authority = qualified(basis, 2, 5);
    let merger = merger();
    let token = authority
        .prepare_coordinate_resume_control_cutoff(
            7,
            staged.query().unwrap(),
            host(104),
            host(110),
            &merger,
        )
        .unwrap()
        .unwrap();
    let mut stale = end.clone();
    stale
        .commit_coordinate_observation(
            stale
                .stage_coordinate_source(7, basis, identity(3), None, pair(basis, 1, 99))
                .unwrap(),
        )
        .unwrap();
    let before = stale.clone();
    assert!(stale
        .commit_coordinate_end(staged.clone(), &authority, &token, host(110), &merger)
        .is_err());
    assert_eq!(stale, before);
    end.commit_coordinate_end(staged.clone(), &authority, &token, host(110), &merger)
        .unwrap();
    let before = end.clone();
    assert!(end
        .commit_coordinate_end(staged, &authority, &token, host(110), &merger)
        .is_err());
    assert_eq!(end, before);
}

#[test]
fn authority_commit_before_native_ack_invalidates_original_token() {
    let basis = counter(3, 9);
    let mut end = end_owner(3, 1, basis);
    let staged = end
        .stage_coordinate_source(
            7,
            basis,
            identity(3),
            Some(finite_report(3, 1)),
            pair(basis, 2, 100),
        )
        .unwrap();
    let mut authority = qualified(basis, 2, 5);
    let merger = merger();
    let token = authority
        .prepare_coordinate_resume_control_cutoff(
            7,
            staged.query().unwrap(),
            host(104),
            host(110),
            &merger,
        )
        .unwrap()
        .unwrap();
    authority
        .commit_coordinate_control_cutoff(token, host(110), &merger)
        .unwrap();
    let before = end.clone();
    assert!(end
        .commit_coordinate_end(staged, &authority, &token, host(110), &merger)
        .is_err());
    assert_eq!(end, before);
}

#[test]
fn coordinate_legacy_observe_and_prime_cannot_preemit_or_defer() {
    let basis = counter(3, 9);
    for prime in [false, true] {
        let mut end = end_owner(3, 1, basis);
        let before = end.clone();
        if prime {
            assert!(end
                .prime(Some(finite_report(3, 1)), pair(basis, 3, 105))
                .is_err());
        } else {
            assert!(end
                .observe(Some(finite_report(3, 1)), pair(basis, 3, 105))
                .is_err());
        }
        assert_eq!(end, before);
    }
}

#[test]
fn old_unbound_scalar_endpoint_still_emits_once_from_real_mixer() {
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 3, 1).unwrap();
    end.observe(
        None,
        ClockPair {
            source: point(2, 0),
            target: host(100),
        },
    )
    .unwrap();
    let boundary = end
        .observe(
            Some(finite_report(3, 1)),
            ClockPair {
                source: point(2, 333_333_333),
                target: host(105),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (1, 1));
    assert_eq!(
        end.observe(
            None,
            ClockPair {
                source: point(2, 333_333_334),
                target: host(106)
            }
        )
        .unwrap(),
        None
    );
}

#[test]
fn actual_pause_gap_retains_original_physical_marker_and_absolute_playback_end() {
    let basis = AudioAuthorityCoordinateBasis::SourceFrame(
        OutputFrameBasis::new(point(2, 0), 1000, 0).unwrap(),
    );
    let mut end = end_owner(1000, 4, basis);
    let (mut mixer, mut producer) = mixer(1000, 4, false);
    let first = mixer.render(&mut [0.0]).unwrap();
    end.commit_coordinate_observation(
        end.stage_coordinate_source(7, basis, identity(1000), Some(first), pair(basis, 0, 100))
            .unwrap(),
    )
    .unwrap();
    producer.request_pause(true);
    let held = mixer.render(&mut [0.0; 3]).unwrap();
    end.commit_coordinate_observation(
        end.stage_coordinate_source(7, basis, identity(1000), Some(held), pair(basis, 1, 101))
            .unwrap(),
    )
    .unwrap();
    producer.request_pause(false);
    let crossed = mixer.render(&mut [0.0; 5]).unwrap();
    assert_eq!(crossed.playback_end_physical_frame, Some(7));
    let latest = mixer.render(&mut [0.0; 2]).unwrap();
    let staged = end
        .stage_coordinate_source(7, basis, identity(1000), Some(latest), pair(basis, 6, 102))
        .unwrap();
    // Use a fresh actual HOST chronology rather than the earlier stage HOST.
    let mut authority = owner(basis);
    observe(&mut authority, basis, 6, 102);
    observe(&mut authority, basis, 9, 110);
    authority.record_acquired_prefix(host(110)).unwrap();
    let boundary = ack(&mut end, staged, &authority, &merger());
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (7, 4));
    assert_eq!(boundary.raw_output, point(2, 7_000_000));
}

#[test]
fn real_start_gate_and_zero_endpoint_keep_actual_physical_prefix() {
    for playback_end in [0, 3] {
        let basis = AudioAuthorityCoordinateBasis::SourceFrame(
            OutputFrameBasis::new(point(2, 0), 1000, 0).unwrap(),
        );
        let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 1000, playback_end)
            .unwrap()
            .with_start_frame(4)
            .unwrap()
            .with_coordinates(7, basis, identity(1000))
            .unwrap();
        let (mut mixer, mut producer) = mixer(1000, playback_end, true);
        let held = mixer.render(&mut [0.0; 2]).unwrap();
        end.commit_coordinate_observation(
            end.stage_coordinate_source(7, basis, identity(1000), Some(held), pair(basis, 0, 99))
                .unwrap(),
        )
        .unwrap();
        producer.schedule_start_at(4).unwrap();
        let report = mixer.render(&mut [0.0; 6]).unwrap();
        assert_eq!(report.playback_end_physical_frame, Some(4 + playback_end));
        let staged = end
            .stage_coordinate_source(7, basis, identity(1000), Some(report), pair(basis, 3, 100))
            .unwrap();
        let authority = qualified(basis, 3, 8);
        let boundary = ack(&mut end, staged, &authority, &merger());
        assert_eq!(
            (boundary.physical_frame, boundary.playback_frame),
            (4 + playback_end, playback_end)
        );
    }
}

fn target_setup() -> (
    NativeEnd,
    AudioAuthorityCoordinateBasis,
    ConvertedBoundaryFacts,
    RenderReport,
) {
    let target = TargetFrameBasis::new(point(2, 0), TargetTime::new(0, 1, 7).unwrap(), 2).unwrap();
    let basis = AudioAuthorityCoordinateBasis::TargetFrame(target);
    let end = NativeEnd::new(point(2, 0), ClockDomainId(1), 3, 1)
        .unwrap()
        .with_target_basis(7, target)
        .unwrap()
        .with_coordinates(7, basis, identity(3))
        .unwrap();
    // Dependency-injected projected target time: a first target sample overshoots
    // source endpoint 1/3 sec. This fixture proves arithmetic, not native IO coherence.
    let facts = ConvertedBoundaryFacts {
        startup: None,
        pause: None,
        resume: None,
        end: Some(TargetBoundary {
            source_frame: 1,
            target_frame_offset: 1,
            target_time: TargetTime::new(0, 9, 14).unwrap(),
        }),
        origin: Some(point(2, 0)),
        source_rate: 3,
    };
    (end, basis, facts, finite_report(3, 1))
}

#[test]
fn target_projection_missing_mismatched_source_or_original_scope_never_grants_ack() {
    let (end, basis, facts, report) = target_setup();
    let mut missing = facts;
    missing.end = None;
    let staged = end
        .stage_coordinate_target(
            7,
            basis,
            identity(3),
            missing,
            Some(report),
            pair(basis, 0, 100),
        )
        .unwrap();
    assert!(staged.query().is_none());
    let before = end.clone();
    for mutation in 0..4 {
        let mut bad = facts;
        match mutation {
            0 => bad.end.as_mut().unwrap().source_frame = 2,
            1 => bad.origin = Some(point(2, 1)),
            2 => bad.source_rate = 4,
            _ => bad.end.as_mut().unwrap().target_time = TargetTime::new(0, 1, 8).unwrap(),
        }
        assert!(end
            .stage_coordinate_target(
                7,
                basis,
                identity(3),
                bad,
                Some(report),
                pair(basis, 0, 100)
            )
            .is_err());
        assert_eq!(end, before);
    }
}

#[test]
fn target_scalar_prime_and_observe_cannot_ack_or_defer_coordinate_owner() {
    let (end, basis, facts, report) = target_setup();
    let AudioAuthorityCoordinateBasis::TargetFrame(target) = basis else {
        unreachable!()
    };
    for prime in [false, true] {
        let mut owner = end.clone();
        if prime {
            assert!(owner
                .prime_target(7, target, facts, Some(report), pair(basis, 1, 105))
                .is_err());
        } else {
            assert!(owner
                .observe_target(7, target, facts, Some(report), pair(basis, 1, 105))
                .is_err());
        }
        assert_eq!(owner, end);
    }
}

#[test]
fn projected_target_overshoot_cannot_change_final_elapsed_or_current_attempt_identity() {
    let (mut end, basis, facts, report) = target_setup();
    let staged = end
        .stage_coordinate_target(
            7,
            basis,
            identity(3),
            facts,
            Some(report),
            pair(basis, 0, 100),
        )
        .unwrap();
    let authority = qualified(basis, 0, 2);
    let boundary = ack(&mut end, staged, &authority, &merger());
    assert_eq!(
        boundary
            .query
            .compare_native_time_with_offset(TargetTime::new(0, 9, 14).unwrap(), 0, 1, 0)
            .unwrap(),
        Ordering::Equal
    );
    for active_cut in [0, 1] {
        for rate in [
            Rate::NORMAL,
            Rate::new(1, 2).unwrap(),
            Rate::new(3, 2).unwrap(),
            Rate::new(2, 1).unwrap(),
        ] {
            let phase_basis =
                OutputPhaseBasis::new(ClockDomainId(3), 99, 42, active_cut, 3).unwrap();
            let phase = coordinate_final_phase(boundary, phase_basis).unwrap();
            assert_eq!(phase.basis(), phase_basis);
            let elapsed = if active_cut == 0 { 333_333_333 } else { 0 };
            assert_eq!(phase.elapsed_nanos(), elapsed);
            let transport = Transport::new_with_output_phase(
                phase_basis,
                point(3, 500),
                Timestamp::from_nanos(10),
                Duration::from_nanos(7),
                rate,
                None,
            )
            .unwrap();
            let song = transport.position_at_output_phase(phase).unwrap();
            let expected_delta = (i128::from(elapsed) - 7) * i128::from(rate.numerator())
                / i128::from(rate.denominator());
            // Transport owns the signed preroll/rate calculation; projection 9/14
            // must never be substituted for source-relative final elapsed.
            assert_eq!(song.as_nanos(), i64::try_from(10 + expected_delta).unwrap());
        }
    }
    let wrong_hz = OutputPhaseBasis::new(ClockDomainId(3), 99, 42, 0, 4).unwrap();
    assert!(coordinate_final_phase(boundary, wrong_hz).is_err());
    let after_end = OutputPhaseBasis::new(ClockDomainId(3), 99, 43, 2, 3).unwrap();
    assert!(coordinate_final_phase(boundary, after_end).is_err());
}

#[test]
fn ack_revalidates_authority_freshness_scope_kind_and_pending_prefix() {
    let basis = counter(3, 9);
    for mutation in 0..6 {
        let mut end = end_owner(3, 1, basis);
        let staged = end
            .stage_coordinate_source(
                7,
                basis,
                identity(3),
                Some(finite_report(3, 1)),
                pair(basis, 2, 100),
            )
            .unwrap();
        let mut authority = qualified(basis, 2, 5);
        let mut merger = merger();
        let token = if mutation == 0 {
            authority
                .prepare_coordinate_control_cutoff(
                    7,
                    staged.query().unwrap(),
                    host(104),
                    host(110),
                    &merger,
                )
                .unwrap()
                .unwrap()
        } else {
            authority
                .prepare_coordinate_resume_control_cutoff(
                    7,
                    staged.query().unwrap(),
                    host(104),
                    host(110),
                    &merger,
                )
                .unwrap()
                .unwrap()
        };
        let now = match mutation {
            1 => {
                authority.record_acquired_prefix(host(111)).unwrap();
                host(111)
            }
            2 => host(211),
            3 => point(9, 110),
            4 => {
                merger.admit(event(103), host(110)).unwrap();
                host(110)
            }
            5 => {
                authority = qualified(counter(3, 10), 2, 5);
                host(110)
            }
            _ => host(110),
        };
        let before = end.clone();
        let authority_before = format!("{:?}", authority);
        let pending = merger.pending();
        assert!(end
            .validate_coordinate_end(&staged, &authority, &token, now, &merger)
            .is_err());
        assert!(end
            .commit_coordinate_end(staged, &authority, &token, now, &merger)
            .is_err());
        assert_eq!(end, before);
        assert_eq!(format!("{:?}", authority), authority_before);
        assert_eq!(merger.pending(), pending);
    }
}

#[test]
fn cold_coordinate_binding_refuses_observed_emitted_or_rebound_owner() {
    let basis = counter(3, 9);
    let fresh = || NativeEnd::new(point(2, 0), ClockDomainId(1), 3, 1).unwrap();
    let mut observed = fresh();
    observed.observe(None, pair(basis, 0, 100)).unwrap();
    assert!(observed.with_coordinates(7, basis, identity(3)).is_err());
    let mut emitted = fresh();
    emitted.observe(None, pair(basis, 0, 100)).unwrap();
    emitted
        .observe(Some(finite_report(3, 1)), pair(basis, 3, 105))
        .unwrap();
    assert!(emitted.with_coordinates(7, basis, identity(3)).is_err());
    let bound = end_owner(3, 1, basis);
    assert!(bound.with_coordinates(8, basis, identity(3)).is_err());
    assert!(fresh().with_coordinates(7, basis, identity(4)).is_err());
}
