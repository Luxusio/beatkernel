//! Genuine Mixer history with supplied CoreAudio publication metadata.
//! No stream, device, native callback, Mach clock or presentation ACK is created.
//! These macOS-gated fixtures require foreign execution; host typing is not PASS.
use super::*;
use crate::gameplay::output::ports::{
    NativeSourcePlaybackQuery, SourcePlaybackIdentity, SourcePlaybackPacketError,
};
use crate::native_audio_presentation::NativeAudioSnapshot;
use crate::native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate};
use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, InterpolatedTargetTime, MixerConfig, PcmLimits,
        SampleBank, SourcePlaybackSpan, SourcePlaybackState, TargetTime,
    },
    time::{ClockPair, ClockPoint, Timestamp},
};
use beatkernel_platform::{
    audio::presentation::validation::OriginalNativePresentationEvidence,
    macos::audio::{CoreAudioPresentation, CoreAudioSnapshot, CoreAudioSourcePlaybackSnapshot},
};

const EPOCH: u64 = 23;

fn at(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 1000).unwrap(), 0, 0, 1, 0, 1)
        .unwrap()
}

fn genuine_facts() -> (
    OutputFrameBasis,
    SourcePlaybackIdentity,
    CoreAudioSourcePlaybackSnapshot,
) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 128, 1).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    let basis = mixer.output_frame_basis();
    let render = mixer.render(&mut [0.; 8]).unwrap();
    let history = mixer.source_playback_history().unwrap();
    let identity = SourcePlaybackIdentity::new(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    let grid = basis.point_at_stream_frame(0).unwrap();
    let native = ClockPoint {
        domain: ClockDomainId(8),
        timestamp: Timestamp::from_nanos(987_654),
    };
    let snapshot = CoreAudioSnapshot {
        rendered_callbacks: 1,
        rendered_frames: 8,
        callback_failures: 0,
        configuration_changed: false,
        presentation: Some(CoreAudioPresentation {
            host_ticks: 91,
            native: Some(native),
            native_sample_frame: Some(400.25),
            output_grid: Some(grid),
            flags: 3,
            first_frame: render.start_frame,
            frames: u32::try_from(render.frames).unwrap(),
        }),
    };
    (
        basis,
        identity,
        CoreAudioSourcePlaybackSnapshot {
            snapshot,
            render: Some(render),
            playback: history.lookup_at_source_time(at(4)).unwrap(),
            pulled_source_end: history.pulled_end_frame(),
            delivered_end_frame: history.pulled_end_frame(),
            revision: 2,
        },
    )
}

fn candidate(basis: OutputFrameBasis, epoch: u64, physical: u64) -> CoordinateNativeAudioSnapshot {
    let frame = physical.checked_sub(basis.start_physical_frame()).unwrap();
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: ClockPoint {
                    domain: ClockDomainId(9),
                    timestamp: Timestamp::from_nanos(987_654),
                },
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}

fn packet(
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    snapshot: CoreAudioSourcePlaybackSnapshot,
) -> Result<NativeSourcePlaybackQuery<CoreAudioSourcePlaybackSnapshot>, CoreAudioReplacementError> {
    qualify_coreaudio_source_packet(EPOCH, basis, identity, query, snapshot)
}

#[test]
fn genuine_nonzero_history_and_creation_keep_original_coreaudio_metadata() {
    let (basis, identity, original) = genuine_facts();
    assert_eq!(basis.start_physical_frame(), 4);
    assert_eq!(
        (identity.initial_physical(), identity.initial_playback()),
        (2, 2)
    );
    assert_eq!(original.pulled_source_end, 12);
    assert_eq!(original.playback.physical_start(), 2);
    let coordinate = candidate(basis, EPOCH, 4);
    let query = coordinate.source_query().unwrap();
    assert!(query.compare_source_time_frame(4, 1000).unwrap().is_eq());
    let qualified = packet(basis, identity, query, original).unwrap();
    assert_eq!(qualified.native_evidence(), Some(&original));
    assert_eq!(qualified.facts().render, original.render);
    assert_eq!(qualified.facts().submitted_source_end, 12);
    let observation =
        qualify_coreaudio_observation_packet(coordinate, original.snapshot, identity, qualified)
            .unwrap()
            .unwrap();
    assert_eq!(observation.coordinate(), coordinate);
    assert_eq!(observation.source().native_evidence(), Some(&original));
    assert_eq!(observation.source().identity(), identity);
    assert_eq!(
        original.snapshot.presentation.unwrap().native_sample_frame,
        Some(400.25)
    );
}

#[test]
fn successful_delivery_not_source_generation_bounds_exact_query() {
    let (basis, identity, mut original) = genuine_facts();
    original.delivered_end_frame = 8;
    original.snapshot.rendered_frames = 4;
    original.snapshot.presentation.as_mut().unwrap().frames = 4;
    let accepted = packet(basis, identity, at(8), original).unwrap();
    assert_eq!(accepted.facts().pulled_source_end, 12);
    assert_eq!(accepted.facts().submitted_source_end, 8);
    let just_after = InterpolatedTargetTime::new(
        TargetTime::from_frames(8, 1000).unwrap(),
        0,
        1,
        1,
        1,
        3_000_000_000,
    )
    .unwrap();
    assert_eq!(
        just_after.point(basis.origin()).unwrap(),
        at(8).point(basis.origin()).unwrap()
    );
    assert!(just_after
        .compare_source_time_frame(8, 1000)
        .unwrap()
        .is_gt());
    assert!(matches!(
        packet(basis, identity, just_after, original),
        Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::QueryOutsidePrefix
        ))
    ));
    assert!(matches!(
        packet(basis, identity, at(9), original),
        Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::QueryOutsidePrefix
        ))
    ));
}

#[test]
fn raced_original_native_publication_waits_without_stitching_clock_or_source() {
    let (basis, identity, original) = genuine_facts();
    let coordinate = candidate(basis, EPOCH, 4);
    for case in 0..7 {
        let mut changed = original.snapshot;
        match case {
            0 => changed.rendered_callbacks += 1,
            1 => changed.rendered_frames += 1,
            2 => changed.presentation.as_mut().unwrap().host_ticks += 1,
            3 => changed.presentation.as_mut().unwrap().native_sample_frame = Some(401.25),
            4 => changed.presentation.as_mut().unwrap().first_frame += 1,
            5 => changed.presentation.as_mut().unwrap().flags = 0,
            _ => changed.presentation = None,
        }
        let qualified = packet(basis, identity, at(4), original).unwrap();
        assert!(
            qualify_coreaudio_observation_packet(coordinate, changed, identity, qualified)
                .unwrap()
                .is_none(),
            "changed native field {case}"
        );
    }
}

#[test]
fn callback_failure_and_configuration_change_refuse_mathematical_publication() {
    let (basis, identity, original) = genuine_facts();
    let mut failed = original;
    failed.snapshot.callback_failures = 7;
    assert!(matches!(
        packet(basis, identity, at(4), failed),
        Err(CoreAudioReplacementError::CallbackFailure(7))
    ));
    let mut changed = original;
    changed.snapshot.configuration_changed = true;
    assert!(matches!(
        packet(basis, identity, at(4), changed),
        Err(CoreAudioReplacementError::Native(
            CoreAudioError::ConfigurationChanged
        ))
    ));
    let coordinate = candidate(basis, EPOCH, 4);
    let qualified = packet(basis, identity, at(4), original).unwrap();
    assert!(matches!(
        qualify_coreaudio_observation_packet(coordinate, failed.snapshot, identity, qualified,),
        Err(CoreAudioReplacementError::CallbackFailure(7))
    ));
    assert!(matches!(
        qualify_coreaudio_observation_packet(coordinate, changed.snapshot, identity, qualified,),
        Err(CoreAudioReplacementError::Native(
            CoreAudioError::ConfigurationChanged
        ))
    ));
}

#[test]
fn no_actual_even_publication_and_invalid_delivery_end_refuse() {
    let (basis, identity, original) = genuine_facts();
    for revision in [0, 1, 3, u64::MAX] {
        let mut invalid = original;
        invalid.revision = revision;
        assert!(
            matches!(
                packet(basis, identity, at(4), invalid),
                Err(CoreAudioReplacementError::Playback(
                    SourcePlaybackPacketError::InvalidRevision
                ))
            ),
            "revision {revision}"
        );
    }
    for end in [4, 13] {
        let mut invalid = original;
        invalid.delivered_end_frame = end;
        assert!(
            packet(basis, identity, at(4), invalid).is_err(),
            "delivery {end}"
        );
    }
    let mut invalid = original;
    invalid.snapshot.presentation.as_mut().unwrap().frames = 7;
    assert!(packet(basis, identity, at(4), invalid).is_err());
}

#[test]
fn original_history_rate_and_physical_identity_are_checked() {
    let (basis, identity, original) = genuine_facts();
    for malformed in [
        SourcePlaybackIdentity::new(999, 2, 2).unwrap(),
        SourcePlaybackIdentity::new(1000, 5, 2).unwrap(),
        SourcePlaybackIdentity::new(1000, 3, 2).unwrap(),
        SourcePlaybackIdentity::new(1000, 4, 4).unwrap(),
    ] {
        assert!(packet(basis, malformed, at(4), original).is_err());
    }
    let before_creation = at(3);
    assert!(matches!(
        packet(basis, identity, before_creation, original),
        Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::QueryOutsidePrefix
        ))
    ));
}

#[test]
fn corrupted_render_or_selected_span_never_becomes_valid_playback_mapping() {
    let (basis, identity, original) = genuine_facts();
    for case in 0..4 {
        let mut invalid = original;
        match case {
            0 => invalid.render.as_mut().unwrap().frames = usize::MAX,
            1 => invalid.render.as_mut().unwrap().playback_start_frame = 100,
            2 => {
                invalid.playback =
                    SourcePlaybackSpan::new(13, None, 2, SourcePlaybackState::Running).unwrap()
            }
            _ => {
                invalid.playback =
                    SourcePlaybackSpan::new(2, Some(4), 2, SourcePlaybackState::Running).unwrap()
            }
        }
        assert!(
            packet(basis, identity, at(4), invalid).is_err(),
            "corruption {case}"
        );
    }
}

#[test]
fn observation_header_and_exact_source_coordinate_are_not_replaceable() {
    let (basis, identity, original) = genuine_facts();
    let qualified = packet(basis, identity, at(4), original).unwrap();
    assert!(matches!(
        qualify_coreaudio_observation_packet(
            candidate(basis, EPOCH + 1, 4),
            original.snapshot,
            identity,
            qualified,
        ),
        Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::EpochMismatch
        ))
    ));
    let other_basis = OutputFrameBasis::new(basis.origin(), 1000, 3).unwrap();
    assert!(matches!(
        qualify_coreaudio_observation_packet(
            candidate(other_basis, EPOCH, 4),
            original.snapshot,
            identity,
            qualified,
        ),
        Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::BasisMismatch
        ))
    ));
    assert!(qualify_coreaudio_observation_packet(
        candidate(basis, EPOCH, 5),
        original.snapshot,
        identity,
        qualified,
    )
    .is_err());
    let changed_identity = SourcePlaybackIdentity::new(1000, 1, 1).unwrap();
    assert!(qualify_coreaudio_observation_packet(
        candidate(basis, EPOCH, 4),
        original.snapshot,
        changed_identity,
        qualified,
    )
    .is_err());
}

#[test]
fn missing_empty_before_creation_and_overflowed_callback_ranges_refuse() {
    let (basis, identity, original) = genuine_facts();
    let mut absent = original;
    absent.snapshot.presentation = None;
    assert!(matches!(
        packet(basis, identity, at(4), absent),
        Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::MissingNativeEvidence
        ))
    ));
    for case in 0..3 {
        let mut invalid = original;
        let presentation = invalid.snapshot.presentation.as_mut().unwrap();
        match case {
            0 => presentation.frames = 0,
            1 => presentation.first_frame = 3,
            _ => presentation.first_frame = u64::MAX,
        }
        assert!(
            matches!(
                packet(basis, identity, at(4), invalid),
                Err(CoreAudioReplacementError::Playback(
                    SourcePlaybackPacketError::InvalidPrefix
                ))
            ),
            "callback range {case}"
        );
    }
}

#[test]
fn repeated_math_reads_preserve_original_publication_without_creating_observation() {
    let (basis, identity, original) = genuine_facts();
    let before = original;
    for frame in 4..=12 {
        let qualified = packet(basis, identity, at(frame), original).unwrap();
        assert_eq!(qualified.epoch(), EPOCH);
        assert_eq!(qualified.basis(), basis);
        assert_eq!(qualified.identity(), identity);
        assert_eq!(qualified.native_evidence(), Some(&before));
        assert!(qualified
            .query()
            .compare_source_time_frame(frame, 1000)
            .unwrap()
            .is_eq());
    }
    assert_eq!(original, before);
    assert_eq!(original.snapshot.callback_failures, 0);
    assert_eq!(original.playback.state(), SourcePlaybackState::Running);
}
