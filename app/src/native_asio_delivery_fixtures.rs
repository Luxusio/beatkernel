//! Real retained Mixer reports with supplied ASIO intervals, not driver evidence.
use super::*;
use crate::{
    asio_replay::AsioReplayPresentation,
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis as CoordinateBasis},
    gameplay::output::{
        adapters::observation::{asio_pause_observation, checked_asio_pause_observation},
        ports::{
            NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
            SourcePlaybackIdentity, SourcePlaybackPacketError,
        },
    },
    live_pause::LivePauseObservation,
    local_input::InputMerger,
    native_end::NativeEnd,
    native_output_coordinate::NativeOutputCoordinateError,
    playback_pause::{NativePause, PausePhase},
};
use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        RenderReport, SampleBank,
    },
    input::DeviceId,
    time::{ClockDomainId, Duration, ExtrapolationPolicy, Timestamp},
};
use beatkernel_platform::audio::{
    asio::{
        AsioPcmEncoding, AsioPresentationError, AsioPresentationObservation, MultimediaHostInterval,
    },
    AsioPlanarLayout, NativeOutputState,
};

const RATE: u32 = 1000;
const EPOCH: u64 = 7;

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}

fn output(end: Option<u64>) -> (NativeOutputState, CommandProducer) {
    let format = AudioFormat::new(RATE, 1).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    let mut mixer = Mixer::new(
        config,
        SampleBank::new(format, PcmLimits::new(32, 256, 1).unwrap()).unwrap(),
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(16).unwrap();
    let mut output = NativeOutputState::from_mixer(mixer);
    output
        .reconfigure_asio(
            AsioPlanarLayout::new(format, &[(3, AsioPcmEncoding::Float32Lsb)]).unwrap(),
            None,
            8,
        )
        .unwrap();
    (output, producer)
}

fn owner(basis: OutputFrameBasis) -> NativeAudioPresentation {
    let origin = basis.point_at_stream_frame(0).unwrap();
    let epoch = AudioAuthorityEpoch {
        id: EPOCH,
        stream_origin: origin,
        logical_origin: point(3, 500),
        host_domain: ClockDomainId(1),
    };
    NativeAudioPresentation::new(
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
            epoch,
            CoordinateBasis::SourceFrame(basis),
        )
        .unwrap(),
        NativePresentationValidator::new(EPOCH, origin, ClockDomainId(1)),
    )
    .unwrap()
}

fn delivery(
    report: RenderReport,
    first: u64,
    frames: usize,
    at: i64,
) -> AsioPresentationObservation {
    AsioPresentationObservation::from_delivery(
        report,
        first,
        frames,
        RATE,
        MultimediaHostInterval {
            before: host(at),
            after: host(at + 2),
        },
        0,
        0,
        point(2, 0),
    )
    .unwrap()
}

fn coordinate(
    basis: OutputFrameBasis,
    observation: AsioPresentationObservation,
) -> CoordinateNativeAudioSnapshot {
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: EPOCH,
            basis,
            evidence: OriginalNativePresentationEvidence::Asio {
                observation,
                basis: Some(basis),
            },
        },
        NativeOutputCoordinate::StreamFrame {
            frame: observation.delivered_first_frame - basis.start_physical_frame(),
        },
    )
    .unwrap()
}

#[test]
fn adjacent_retained_prefixes_advance_the_actual_owner_and_validate_its_original_history() {
    let (mut output, _producer) = output(None);
    let basis = output.output_frame_basis();
    let report = output.render_pending(8).unwrap();
    let mut owner = owner(basis);
    for (index, first) in [0, 2, 4].into_iter().enumerate() {
        output.admit(2).unwrap();
        let candidate = coordinate(basis, delivery(report, first, 2, 100 + index as i64 * 10));
        assert_eq!(
            owner.admit_coordinate(candidate).unwrap(),
            NativeObservationAdmission::Progress
        );
        owner.validate_coordinate_seed().unwrap();
        owner.validate_coordinate_history().unwrap();
        assert_eq!(owner.authority().history_len(), index + 1);
        assert_eq!(
            owner.latest_record().unwrap().evidence(),
            &candidate.snapshot().evidence
        );
        assert_eq!(
            owner
                .authority()
                .latest_coordinate_observation()
                .unwrap()
                .coordinate(),
            Some(NativeOutputCoordinate::StreamFrame { frame: first })
        );
        assert_eq!(output.pending_report(), Some(report));
        assert_eq!(output.mixer().frame_cursor(), 8);
    }
    assert_eq!(output.admitted_frames(), 6);
    assert_eq!(output.pending_frames(), 2);
}

#[test]
fn creation_basis_inside_an_old_report_uses_delivered_frames_and_keeps_the_whole_report() {
    let (mut output, _producer) = output(None);
    let report = output.render_pending(8).unwrap();
    output.admit(2).unwrap();
    let basis = output.output_frame_basis();
    assert_eq!(basis.start_physical_frame(), 2);
    assert!(report.start_frame < basis.start_physical_frame());
    let mut owner = owner(basis);
    for (first, relative, at) in [(2, 0, 100), (4, 2, 110)] {
        output.admit(2).unwrap();
        let candidate = coordinate(basis, delivery(report, first, 2, at));
        assert_eq!(
            candidate.coordinate(),
            NativeOutputCoordinate::StreamFrame { frame: relative }
        );
        assert_eq!(
            owner.admit_coordinate(candidate).unwrap(),
            NativeObservationAdmission::Progress
        );
        owner.validate_coordinate_seed().unwrap();
        let OriginalNativePresentationEvidence::Asio {
            observation,
            basis: retained,
        } = *owner.latest_record().unwrap().evidence()
        else {
            panic!("original ASIO evidence must be retained");
        };
        assert_eq!(retained, Some(basis));
        assert_eq!(observation.render, report);
        assert_eq!(observation.delivered_first_frame, first);
        assert_eq!(observation.delivered_frames, 2);
    }
}

#[test]
fn duplicate_prefix_never_refreshes_age_and_changed_extent_cannot_replace_it() {
    let (mut output, _producer) = output(None);
    let basis = output.output_frame_basis();
    let report = output.render_pending(8).unwrap();
    let mut owner = owner(basis);
    owner
        .admit_coordinate(coordinate(basis, delivery(report, 0, 2, 100)))
        .unwrap();
    let before = format!("{owner:?}");
    assert_eq!(
        owner
            .admit_coordinate(coordinate(basis, delivery(report, 0, 2, 900)))
            .unwrap(),
        NativeObservationAdmission::Unchanged
    );
    assert_eq!(format!("{owner:?}"), before);
    assert!(!owner.authority().has_fresh_observation(host(202)).unwrap());
    assert!(owner
        .admit_coordinate(coordinate(basis, delivery(report, 0, 3, 110)))
        .is_err());
    assert_eq!(format!("{owner:?}"), before);
    assert_eq!(
        owner
            .admit_coordinate(coordinate(basis, delivery(report, 2, 2, 110)))
            .unwrap(),
        NativeObservationAdmission::Progress
    );
    owner.validate_coordinate_seed().unwrap();
}

#[test]
fn forged_geometry_coordinate_and_epoch_refuse_before_the_owner_changes() {
    let (mut output, _producer) = output(None);
    let basis = output.output_frame_basis();
    let report = output.render_pending(8).unwrap();
    let mut owner = owner(basis);
    owner
        .admit_coordinate(coordinate(basis, delivery(report, 0, 2, 100)))
        .unwrap();
    let before = format!("{owner:?}");
    for case in 0..6 {
        let mut observation = delivery(report, 2, 2, 110);
        match case {
            0 => observation.delivered_frames = 0,
            1 => observation.delivered_frames = 7,
            2 => observation.delivered_first_frame = 8,
            3 => observation.output = point(2, 0),
            4 => observation.delivered_first_frame = 1,
            _ => observation.render.frames = 1,
        }
        let result = CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: EPOCH,
                basis,
                evidence: OriginalNativePresentationEvidence::Asio {
                    observation,
                    basis: Some(basis),
                },
            },
            NativeOutputCoordinate::StreamFrame {
                frame: observation.delivered_first_frame,
            },
        );
        if let Ok(candidate) = result {
            assert!(owner.admit_coordinate(candidate).is_err(), "case {case}");
        }
        assert_eq!(format!("{owner:?}"), before, "case {case}");
    }
    let valid = coordinate(basis, delivery(report, 2, 2, 110));
    assert_eq!(
        CoordinateNativeAudioSnapshot::new(
            valid.snapshot(),
            NativeOutputCoordinate::StreamFrame { frame: 0 }
        ),
        Err(NativeOutputCoordinateError::CoordinateMismatch)
    );
    let wrong_epoch = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: EPOCH + 1,
            ..valid.snapshot()
        },
        valid.coordinate(),
    )
    .unwrap();
    assert!(owner.admit_coordinate(wrong_epoch).is_err());
    assert_eq!(format!("{owner:?}"), before);
    assert_eq!(
        owner.admit_coordinate(valid).unwrap(),
        NativeObservationAdmission::Progress
    );
    owner.validate_coordinate_seed().unwrap();
}

#[test]
fn scalar_replay_rejects_first_and_later_partial_prefixes_even_if_the_report_is_a_duplicate() {
    let (mut output, _producer) = output(None);
    let report = output.render_pending(8).unwrap();
    let whole = AsioPresentationObservation::from_render(
        report,
        RATE,
        MultimediaHostInterval {
            before: host(100),
            after: host(102),
        },
        0,
        0,
        point(2, 0),
    )
    .unwrap();
    for partial in [delivery(report, 0, 2, 100), delivery(report, 2, 2, 100)] {
        let mut replay =
            AsioReplayPresentation::new(point(2, 0), ClockDomainId(1), RATE, 2).unwrap();
        assert!(replay.observe(Some(partial), host(0)).is_err());
        assert_eq!(replay.observe(None, host(0)).unwrap(), None);
        assert_eq!(replay.observe(Some(whole), host(0)).unwrap(), None);
        // Rejection must precede the legacy whole-report duplicate fast path.
        assert!(replay.observe(Some(partial), host(900)).is_err());
        assert_eq!(replay.observe(None, host(101)).unwrap(), None);
        assert_eq!(replay.observe(None, host(102)).unwrap(), Some(whole.output));
    }
}

#[test]
fn checked_pause_conversion_rejects_both_partial_shapes_and_legacy_helper_fails_closed() {
    let (mut output, _producer) = output(None);
    let report = output.render_pending(8).unwrap();
    let now = host(200);
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), RATE).unwrap();
    for partial in [delivery(report, 0, 2, 100), delivery(report, 2, 2, 100)] {
        let before = pause.clone();
        assert_eq!(
            checked_asio_pause_observation(Some(partial), now),
            Err(AsioPresentationError::Malformed)
        );
        assert_eq!(pause, before);
        assert_eq!(
            asio_pause_observation(Some(partial), now),
            LivePauseObservation::Interval {
                observation: None,
                now
            }
        );
    }
    let whole = AsioPresentationObservation::from_render(
        report,
        RATE,
        MultimediaHostInterval {
            before: host(100),
            after: host(102),
        },
        0,
        0,
        point(2, 0),
    )
    .unwrap();
    let converted = checked_asio_pause_observation(Some(whole), now).unwrap();
    assert_eq!(converted, asio_pause_observation(Some(whole), now));
    let LivePauseObservation::Interval {
        observation: Some(interval),
        now: received,
    } = converted
    else {
        panic!("whole legacy observation must retain its interval");
    };
    assert_eq!(received, now);
    assert_eq!(interval.render, report);
    assert_eq!(interval.clock.output, whole.output);
    assert_eq!(
        (interval.clock.before, interval.clock.after),
        (whole.host.before, whole.host.after)
    );
    assert!(pause.request_interval(true, interval).unwrap());
    assert_eq!(pause.phase(), PausePhase::Pausing);
    assert_eq!(pause.last_render_report(), Some(report));
    assert_eq!(
        checked_asio_pause_observation(None, now).unwrap(),
        LivePauseObservation::Interval {
            observation: None,
            now
        }
    );
}

#[test]
fn scalar_end_rejects_partial_marker_reports_without_ack_and_keeps_whole_legacy_crossing() {
    let (mut output, _producer) = output(Some(4));
    let report = output.render_pending(8).unwrap();
    assert_eq!(report.playback_end_physical_frame, Some(4));
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), RATE, 4).unwrap();
    for partial in [delivery(report, 0, 2, 100), delivery(report, 2, 2, 110)] {
        let before = end.clone();
        assert!(end.observe_asio(partial).is_err());
        assert_eq!(end, before);
    }
    let whole = AsioPresentationObservation::from_render(
        report,
        RATE,
        MultimediaHostInterval {
            before: host(100),
            after: host(102),
        },
        0,
        0,
        point(2, 0),
    )
    .unwrap();
    assert!(end.observe_asio(whole).unwrap().is_none());
    let before = end.clone();
    assert!(end.observe_asio(delivery(report, 0, 2, 110)).is_err());
    assert_eq!(end, before);
    output.admit(8).unwrap();
    let silent = output.render_pending(8).unwrap();
    let crossing = AsioPresentationObservation::from_render(
        silent,
        RATE,
        MultimediaHostInterval {
            before: host(120),
            after: host(122),
        },
        0,
        0,
        point(2, 0),
    )
    .unwrap();
    let boundary = end.observe_asio(crossing).unwrap().unwrap();
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (4, 4));
    assert_eq!(boundary.host, host(122));
    assert!(end.observe_asio(crossing).unwrap().is_none());
}

fn packet(
    output: &NativeOutputState,
    basis: OutputFrameBasis,
    query: beatkernel::audio::InterpolatedTargetTime,
    observation: AsioPresentationObservation,
    submitted: u64,
    revision: u64,
) -> Result<NativeSourcePlaybackQuery<AsioPresentationObservation>, SourcePlaybackPacketError> {
    let history = output.mixer().source_playback_history().unwrap();
    NativeSourcePlaybackQuery::new(
        EPOCH,
        basis,
        SourcePlaybackIdentity::new(
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
        )
        .unwrap(),
        query,
        SourcePlaybackFacts {
            playback: history.lookup_at_source_time(query).unwrap(),
            pulled_source_end: history.pulled_end_frame(),
            submitted_source_end: submitted,
            render: Some(observation.render),
            revision,
        },
        Some(observation),
    )
}

#[test]
fn source_end_waits_for_delivered_native_crossing_instead_of_the_original_unsent_marker() {
    let (mut output, _producer) = output(Some(4));
    let basis = output.output_frame_basis();
    let descriptor = CoordinateBasis::SourceFrame(basis);
    let identity = SourcePlaybackIdentity::new(RATE, 0, 0).unwrap();
    let report = output.render_pending(8).unwrap();
    assert_eq!(
        (
            report.frames,
            report.playback_frames,
            report.playback_end_physical_frame
        ),
        (8, 4, Some(4))
    );
    let mut owner = owner(basis);
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), RATE, 4)
        .unwrap()
        .with_coordinates(EPOCH, descriptor, identity)
        .unwrap();
    let first = delivery(report, 0, 2, 100);
    output.admit(2).unwrap();
    let first_coordinate = coordinate(basis, first);
    owner.admit_coordinate(first_coordinate).unwrap();
    let first_packet = packet(
        &output,
        basis,
        first_coordinate.source_query().unwrap(),
        first,
        2,
        2,
    )
    .unwrap();
    NativePlaybackObservation::new(first_coordinate, first_packet).unwrap();
    let staged = end
        .stage_coordinate_source(
            EPOCH,
            descriptor,
            identity,
            Some(report),
            owner.latest_record().unwrap().pair(),
        )
        .unwrap();
    let cut = staged.query().unwrap();
    assert_eq!(staged.physical_frame(), Some(4));
    assert!(matches!(
        packet(&output, basis, cut, first, 2, 2),
        Err(SourcePlaybackPacketError::QueryOutsidePrefix)
    ));
    assert_eq!(
        owner
            .authority()
            .presented_coordinate_boundary_host(&cut, host(102))
            .unwrap(),
        None
    );
    assert_eq!(output.pending_report(), Some(report));
    let second = delivery(report, 2, 2, 110);
    output.admit(2).unwrap();
    owner.admit_coordinate(coordinate(basis, second)).unwrap();
    // Generated endpoint and even completed submission are still not a native crossing.
    assert!(packet(&output, basis, cut, second, 4, 4).is_ok());
    assert_eq!(
        owner
            .authority()
            .presented_coordinate_boundary_host(&cut, host(112))
            .unwrap(),
        None
    );
    let crossing = delivery(report, 4, 2, 120);
    output.admit(2).unwrap();
    let crossing_coordinate = coordinate(basis, crossing);
    let native = NativePlaybackObservation::new(
        crossing_coordinate,
        packet(&output, basis, cut, crossing, 6, 6).unwrap(),
    )
    .unwrap();
    assert_eq!(native.source().native_evidence(), Some(&crossing));
    owner.admit_coordinate(native.coordinate()).unwrap();
    owner.validate_coordinate_seed().unwrap();
    let cutoff_host = owner
        .authority()
        .presented_coordinate_boundary_host(&cut, host(122))
        .unwrap()
        .unwrap();
    assert_eq!(cutoff_host, host(121));
    owner
        .authority_mut()
        .record_acquired_prefix(host(122))
        .unwrap();
    let merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
    let cutoff = owner
        .authority()
        .prepare_coordinate_resume_control_cutoff(EPOCH, cut, cutoff_host, host(122), &merger)
        .unwrap()
        .unwrap();
    let boundary = end
        .commit_coordinate_end(staged, owner.authority(), &cutoff, host(122), &merger)
        .unwrap();
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (4, 4));
    assert_eq!(boundary.host, host(121));
    assert_eq!(output.pending_report(), Some(report));
    assert_eq!(output.admitted_frames(), 6);
    assert_eq!(output.mixer().frame_cursor(), 8);
}
