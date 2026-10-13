//! Actual Mixer history plus explicit WASAPI DI evidence; never a device timing claim.
use super::{qualify_wasapi_source_packet, qualify_wasapi_source_observation_packet};
use crate::gameplay::output::ports::SourcePlaybackIdentity;
use crate::native_audio_presentation::NativeAudioSnapshot;
use crate::native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate};
use beatkernel::audio::*;
use beatkernel::time::{ClockDomainId, ClockMappingQuality, ClockPoint, Timestamp};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    SourcePlaybackSnapshot, StreamCounters,
    presentation::validation::OriginalNativePresentationEvidence,
};
use std::cmp::Ordering;

fn at(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 3).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn rig() -> (
    OutputFrameBasis,
    SourcePlaybackIdentity,
    SourcePlaybackSnapshot,
) {
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(32, 64, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 5]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 3]).unwrap();
    mixer.enable_source_playback_history(4).unwrap();
    let basis = mixer.output_frame_basis();
    let history = mixer.source_playback_history().unwrap();
    let identity = SourcePlaybackIdentity::new(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    let render = mixer.render(&mut [0.0; 4]).unwrap();
    let history = mixer.source_playback_history().unwrap();
    assert_eq!(
        (basis.start_physical_frame(), identity.initial_playback()),
        (8, 5)
    );
    assert_eq!(history.pulled_end_frame(), 12);
    let snapshot = AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters {
            submitted_frames: 2,
            buffer_fills: 1,
            padding_frames: 1,
            ..StreamCounters::default()
        },
        render: Some(render),
        clock: Some(AudioClockSnapshot {
            position: 1,
            frequency: u64::MAX,
            qpc_100ns: 11,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(ClockPoint {
                domain: ClockDomainId(8),
                timestamp: Timestamp::from_nanos(1100),
            }),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    };
    (
        basis,
        identity,
        SourcePlaybackSnapshot {
            snapshot,
            playback: history.lookup_at_source_time(at(8)).unwrap(),
            pulled_source_end: history.pulled_end_frame(),
            revision: 4,
        },
    )
}
fn coordinate(
    epoch: u64,
    basis: OutputFrameBasis,
    raw: AudioStreamSnapshot,
) -> CoordinateNativeAudioSnapshot {
    let clock = raw.clock.unwrap();
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch,
            basis,
            evidence: OriginalNativePresentationEvidence::Wasapi {
                snapshot: raw,
                basis: Some(basis),
            },
        },
        NativeOutputCoordinate::Counter {
            position: clock.position,
            frequency: clock.frequency,
        },
    )
    .unwrap()
}

#[test]
fn real_paused_history_original_render_and_partial_native_submission_are_preserved() {
    let (basis, id, raw) = rig();
    let native = coordinate(23, basis, raw.snapshot);
    let query = native.source_query().unwrap();
    let packet = qualify_wasapi_source_packet(23, basis, id, query, raw).unwrap();
    assert_eq!(packet.facts().submitted_source_end, 10);
    assert_eq!(packet.facts().pulled_source_end, 12);
    assert_eq!(packet.facts().render, raw.snapshot.render);
    assert_eq!(packet.facts().playback, raw.playback);
    assert_eq!(packet.facts().playback.state(), SourcePlaybackState::Frozen);
    assert_eq!(packet.facts().revision, 4);
    assert_eq!(packet.native_evidence(), Some(&raw.snapshot));
    let observed = qualify_wasapi_source_observation_packet(native, id, packet)
        .unwrap()
        .unwrap();
    assert_eq!(observed.coordinate(), native);
    assert_eq!(
        observed.source().query().compare_interpolated_time(query),
        Ordering::Equal
    );
    assert_eq!(observed.source().native_evidence(), Some(&raw.snapshot));
}

#[test]
fn actual_submitted_frontier_is_inclusive_and_unsubmitted_pulls_do_not_qualify() {
    let (basis, id, raw) = rig();
    assert!(qualify_wasapi_source_packet(23, basis, id, at(10), raw).is_ok());
    for query in [
        at(7),
        at(11),
        InterpolatedTargetTime::new(
            TargetTime::from_frames(10, 3).unwrap(),
            1,
            1,
            u64::MAX,
            0,
            1,
        )
        .unwrap(),
    ] {
        assert!(qualify_wasapi_source_packet(23, basis, id, query, raw).is_err());
    }
    let fraction = InterpolatedTargetTime::new(
        TargetTime::from_frames(10, 3).unwrap(),
        1,
        1,
        u64::MAX,
        0,
        1,
    )
    .unwrap();
    assert_eq!(
        fraction.point(basis.origin()).unwrap(),
        at(10).point(basis.origin()).unwrap()
    );
}

#[test]
fn malformed_native_publication_facts_refuse_without_changing_original_payload() {
    let (basis, id, original) = rig();
    for case in 0..10 {
        let mut raw = original;
        match case {
            0 => raw.revision = 0,
            1 => raw.revision = 3,
            2 => raw.snapshot.telemetry_available = false,
            3 => raw.snapshot.counters.submitted_frames = 0,
            4 => raw.snapshot.counters.submitted_frames = 5,
            5 => raw.snapshot.counters.submitted_frames = u64::MAX,
            6 => raw.pulled_source_end = 7,
            7 => raw.snapshot.render.as_mut().unwrap().frames = 5,
            8 => raw.snapshot.render.as_mut().unwrap().playback_frames = 5,
            _ => {
                raw.playback =
                    SourcePlaybackSpan::new(8, None, 6, SourcePlaybackState::Frozen).unwrap()
            }
        }
        let saved = raw;
        assert!(
            qualify_wasapi_source_packet(23, basis, id, at(8), raw).is_err(),
            "case {case}"
        );
        assert_eq!(raw.snapshot, saved.snapshot);
        assert_eq!(raw.playback, saved.playback);
        assert_eq!(
            (raw.pulled_source_end, raw.revision),
            (saved.pulled_source_end, saved.revision)
        );
    }
}

#[test]
fn native_packet_rejects_wrong_original_source_header_and_half_open_span() {
    let (basis, id, mut raw) = rig();
    for wrong in [
        SourcePlaybackIdentity::new(4, 8, 5).unwrap(),
        SourcePlaybackIdentity::new(3, 9, 5).unwrap(),
        SourcePlaybackIdentity::new(3, 8, 6).unwrap(),
    ] {
        assert!(qualify_wasapi_source_packet(23, basis, wrong, at(8), raw).is_err());
    }
    raw.playback = SourcePlaybackSpan::new(8, Some(9), 5, SourcePlaybackState::Frozen).unwrap();
    assert!(qualify_wasapi_source_packet(23, basis, id, at(8), raw).is_ok());
    assert!(qualify_wasapi_source_packet(23, basis, id, at(9), raw).is_err());
}

#[test]
fn changed_complete_native_publication_waits_instead_of_stitching_evidence() {
    let (basis, id, original) = rig();
    let candidate = coordinate(23, basis, original.snapshot);
    let query = candidate.source_query().unwrap();
    for case in 0..8 {
        let mut raw = original;
        match case {
            0 => raw.snapshot.counters.padding_frames += 1,
            1 => raw.snapshot.counters.buffer_fills += 1,
            2 => raw.snapshot.counters.inferred_starvations += 1,
            3 => raw.snapshot.clock.as_mut().unwrap().qpc_100ns += 1,
            4 => {
                raw.snapshot
                    .clock
                    .as_mut()
                    .unwrap()
                    .host_point
                    .as_mut()
                    .unwrap()
                    .timestamp = Timestamp::from_nanos(1200)
            }
            5 => raw.snapshot.clock.as_mut().unwrap().mapping_quality = ClockMappingQuality::Exact,
            6 => raw.snapshot.status = AudioStreamStatus::Ready,
            _ => raw.snapshot.render = None,
        }
        let packet = qualify_wasapi_source_packet(23, basis, id, query, raw).unwrap();
        assert!(
            qualify_wasapi_source_observation_packet(candidate, id, packet)
                .unwrap()
                .is_none(),
            "case {case}"
        );
        assert_eq!(
            candidate.snapshot().evidence,
            OriginalNativePresentationEvidence::Wasapi {
                snapshot: original.snapshot,
                basis: Some(basis)
            }
        );
    }
}

#[test]
fn observation_header_and_subnanosecond_query_corruption_refuse_before_publication_matching() {
    let (basis, id, raw) = rig();
    let candidate = coordinate(23, basis, raw.snapshot);
    let exact = candidate.source_query().unwrap();
    let rounded = at(8);
    assert_eq!(
        exact.point(basis.origin()).unwrap(),
        rounded.point(basis.origin()).unwrap()
    );
    let packet = qualify_wasapi_source_packet(23, basis, id, rounded, raw).unwrap();
    assert!(qualify_wasapi_source_observation_packet(candidate, id, packet).is_err());
    let packet = qualify_wasapi_source_packet(24, basis, id, exact, raw).unwrap();
    assert!(qualify_wasapi_source_observation_packet(candidate, id, packet).is_err());
    let packet = qualify_wasapi_source_packet(23, basis, id, exact, raw).unwrap();
    assert!(qualify_wasapi_source_observation_packet(
        candidate,
        SourcePlaybackIdentity::new(3, 8, 4).unwrap(),
        packet
    )
    .is_err());
}
