//! Cold scope and phase coherence only; these doubles grant no native playback proof.
use super::*;
use crate::{
    audio_authority::AudioAuthorityCoordinateBasis,
    gameplay::output::ports::SourcePlaybackIdentity,
    native_audio_presentation::{
        NativeAudioPresentation, NativeAudioSnapshot, TargetNativeAudioSnapshot,
    },
    native_audio_startup::{new_audio_presentation, new_coordinate_audio_presentation},
    native_gameplay::{AudioGameplayConfig, CoordinateGameplayConfig},
    native_output_coordinate::{
        CoordinateNativeAudioSnapshot, CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate,
    },
    replay_speed::ReplaySpeed,
};
use beatkernel::{
    audio::{OutputFrameBasis, TargetFrameBasis, TargetTime},
    time::{ClockMappingQuality, ClockPair, ClockPoint, Duration},
    transport::{OutputPhaseBasis, Rate},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters, presentation::validation::OriginalNativePresentationEvidence,
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn basis(kind: u8) -> AudioAuthorityCoordinateBasis {
    let source = OutputFrameBasis::new(point(2, 100), 3, 10).unwrap();
    match kind {
        0 => AudioAuthorityCoordinateBasis::SourceFrame(source),
        1 => AudioAuthorityCoordinateBasis::SourceCounter {
            basis: source,
            frequency: 7,
        },
        _ => AudioAuthorityCoordinateBasis::TargetFrame(
            TargetFrameBasis::new(point(2, 100), TargetTime::new(0, 1, 3).unwrap(), 11).unwrap(),
        ),
    }
}
fn accept(p: &mut NativeAudioPresentation, b: AudioAuthorityCoordinateBasis, n: u64) {
    let evidence = match b {
        AudioAuthorityCoordinateBasis::SourceTimestamp(_) => {
            panic!("SourceTimestamp is not a native coordinate in this fixture")
        }
        AudioAuthorityCoordinateBasis::SourceFrame(basis) => {
            let c = CoordinateNativeAudioSnapshot::new(
                NativeAudioSnapshot {
                    epoch: 7,
                    basis,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                        source: basis.point_at_stream_frame(n).unwrap(),
                        target: point(1, n as i64),
                    }),
                },
                NativeOutputCoordinate::StreamFrame { frame: n },
            )
            .unwrap();
            p.admit_coordinate(c).unwrap();
            return;
        }
        AudioAuthorityCoordinateBasis::TargetFrame(basis) => {
            let c = CoordinateTargetNativeAudioSnapshot::new(
                TargetNativeAudioSnapshot {
                    epoch: 7,
                    basis,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                        source: basis.point_at_stream_frame(n).unwrap(),
                        target: point(1, n as i64),
                    }),
                },
                NativeOutputCoordinate::StreamFrame { frame: n },
            )
            .unwrap();
            p.admit_target_coordinate(c).unwrap();
            return;
        }
        AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency } => {
            OriginalNativePresentationEvidence::Wasapi {
                basis: Some(basis),
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters::default(),
                    render: None,
                    clock: Some(AudioClockSnapshot {
                        position: n,
                        frequency,
                        qpc_100ns: n,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(point(1, n as i64)),
                        mapping_quality: ClockMappingQuality::Unknown,
                    }),
                },
            }
        }
    };
    let AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency } = b else {
        unreachable!()
    };
    p.admit_coordinate(
        CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: 7,
                basis,
                evidence,
            },
            NativeOutputCoordinate::Counter {
                position: n,
                frequency,
            },
        )
        .unwrap(),
    )
    .unwrap();
}
fn rig(
    kind: u8,
    rate: Rate,
) -> (
    CoordinateGameplayConfig,
    NativeAudioPresentation,
    Transport,
    LiveSpeedSetup,
) {
    let descriptor = basis(kind);
    let mut p = new_coordinate_audio_presentation(
        7,
        descriptor,
        ClockDomainId(1),
        point(3, 500),
        crate::audio_authority::AudioAuthorityConfig::default(),
    )
    .unwrap();
    accept(&mut p, descriptor, 1);
    accept(&mut p, descriptor, 2);
    let speed = LiveSpeedSetup::new(
        ts(100_001),
        Duration::from_nanos(3),
        ReplaySpeed::new(rate).unwrap(),
        Some(ts(100_012)),
    )
    .unwrap();
    let stream = p.authority().epoch().stream_origin;
    let cfg = CoordinateGameplayConfig {
        audio: AudioGameplayConfig {
            gameplay: NativeGameplayConfig {
                origin: point(1, 0),
                stream_origin: stream,
                playback_origin: stream,
                song_origin: speed.song_origin(),
                sample_rate: 3,
                end_song: speed.end(),
                advance_lag: Duration::ZERO,
                seconds: None,
                pause_supported: true,
                logical_schedule: true,
            },
            section_start: speed.start(),
        },
        source_history: SourcePlaybackIdentity::new(3, 2, 1).unwrap(),
    };
    let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 113, 6, 3).unwrap();
    let transport = Transport::new_with_output_phase(
        phase,
        point(3, 500),
        speed.start(),
        speed.preroll(),
        rate,
        speed.end(),
    )
    .unwrap();
    (cfg, p, transport, speed)
}
fn assert_result(
    cfg: &CoordinateGameplayConfig,
    p: &NativeAudioPresentation,
    t: &Transport,
    s: Option<LiveSpeedSetup>,
    valid: bool,
) {
    let before = format!("{p:?}");
    let old = t.clone();
    assert_eq!(validate_audio_coordinate_start(cfg, p, t, s).is_ok(), valid);
    assert_eq!(format!("{p:?}"), before);
    assert_eq!(t, &old);
}
fn changed_transport(
    t: &Transport,
    origin: ClockPoint,
    start: Timestamp,
    preroll: Duration,
    rate: Rate,
    end: Option<Timestamp>,
) -> Transport {
    Transport::new_with_output_phase(
        t.output_phase_basis().unwrap(),
        origin,
        start,
        preroll,
        rate,
        end,
    )
    .unwrap()
}

#[test]
fn actual_source_counter_target_owners_accept_exact_fixed_rates_and_odd_preroll() {
    for kind in 0..3 {
        for rate in [
            Rate::NORMAL,
            Rate::new(1, 2).unwrap(),
            Rate::new(3, 2).unwrap(),
            Rate::new(2, 1).unwrap(),
        ] {
            let (cfg, p, t, s) = rig(kind, rate);
            assert_result(&cfg, &p, &t, Some(s), true);
            assert_eq!(cfg.audio.gameplay.end_song, Some(ts(100_012)));
            assert_ne!(
                t.output_phase_basis().unwrap().owner(),
                p.authority().epoch().id
            );
        }
    }
}
#[test]
fn target_creation_rate_is_independent_of_immutable_source_phase_rate() {
    let (cfg, p, t, s) = rig(2, Rate::new(3, 2).unwrap());
    let AudioAuthorityCoordinateBasis::TargetFrame(b) = p.authority().coordinate_basis().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(b.sample_rate(), 11);
    assert_eq!(cfg.source_history.hz(), 3);
    assert_result(&cfg, &p, &t, Some(s), true);
}
#[test]
fn none_speed_uses_normal_section_minus_song_preroll() {
    let (cfg, p, t, _) = rig(0, Rate::NORMAL);
    assert_result(&cfg, &p, &t, None, true);
    let (mut cfg, p, t, _) = rig(0, Rate::new(1, 2).unwrap());
    assert_eq!(
        cfg.audio.section_start.as_nanos() - cfg.audio.gameplay.song_origin.as_nanos(),
        1
    );
    assert_result(&cfg, &p, &t, None, false);
    cfg.audio.gameplay.song_origin = ts(cfg.audio.section_start.as_nanos() - 3);
    assert_result(&cfg, &p, &t, None, false);
}
#[test]
fn selected_half_speed_preroll_is_not_reconstructed_from_rounded_song_origin() {
    let (cfg, p, t, s) = rig(1, Rate::new(1, 2).unwrap());
    let changed =
        LiveSpeedSetup::new(s.start(), Duration::from_nanos(2), s.speed(), s.end()).unwrap();
    assert_eq!(changed.song_origin(), s.song_origin());
    assert_result(&cfg, &p, &t, Some(s), true);
    assert_result(&cfg, &p, &t, Some(changed), false);
}
#[test]
fn missing_descriptor_or_phase_and_scalar_transport_mix_are_refused() {
    let (cfg, p, t, s) = rig(0, Rate::NORMAL);
    let AudioAuthorityCoordinateBasis::SourceFrame(b) = p.authority().coordinate_basis().unwrap()
    else {
        unreachable!()
    };
    let scalar = new_audio_presentation(
        7,
        b,
        ClockDomainId(1),
        point(3, 500),
        crate::audio_authority::AudioAuthorityConfig::default(),
    )
    .unwrap();
    assert_result(&cfg, &scalar, &t, Some(s), false);
    let scalar_t = s.transport(ts(500)).unwrap();
    assert_result(&cfg, &p, &scalar_t, Some(s), false);
}
#[test]
fn host_raw_and_phase_domains_must_match_actual_epoch() {
    for changed in 0..4 {
        let (mut cfg, p, t, s) = rig(0, Rate::NORMAL);
        let t = match changed {
            0 => {
                cfg.audio.gameplay.origin.domain = ClockDomainId(9);
                t
            }
            1 => {
                cfg.audio.gameplay.stream_origin.domain = ClockDomainId(9);
                t
            }
            2 => {
                cfg.audio.gameplay.playback_origin.domain = ClockDomainId(9);
                t
            }
            _ => Transport::new_with_output_phase(
                OutputPhaseBasis::new(ClockDomainId(9), 91, 113, 6, 3).unwrap(),
                point(9, 500),
                s.start(),
                s.preroll(),
                s.speed().rate(),
                s.end(),
            )
            .unwrap(),
        };
        assert_result(&cfg, &p, &t, Some(s), false);
    }
}
#[test]
fn source_rate_history_origin_and_active_cut_are_explicit_not_inferred() {
    for changed in 0..4 {
        let (mut cfg, p, t, s) = rig(0, Rate::NORMAL);
        let t = match changed {
            0 => {
                cfg.source_history = SourcePlaybackIdentity::new(4, 2, 1).unwrap();
                t
            }
            1 => {
                cfg.audio.gameplay.sample_rate = 4;
                t
            }
            2 => {
                cfg.source_history = SourcePlaybackIdentity::new(3, 11, 1).unwrap();
                t
            }
            _ => Transport::new_with_output_phase(
                OutputPhaseBasis::new(ClockDomainId(3), 91, 113, 0, 3).unwrap(),
                point(3, 500),
                s.start(),
                s.preroll(),
                s.speed().rate(),
                s.end(),
            )
            .unwrap(),
        };
        assert_result(&cfg, &p, &t, Some(s), false);
    }
}
#[test]
fn declared_raw_stream_origin_cannot_shift_actual_creation_epoch() {
    let (mut cfg, p, t, s) = rig(1, Rate::NORMAL);
    cfg.audio.gameplay.stream_origin.timestamp =
        ts(cfg.audio.gameplay.stream_origin.timestamp.as_nanos() - 1);
    assert_result(&cfg, &p, &t, Some(s), false);
}
#[test]
fn changed_start_end_rate_preroll_and_logical_origin_refuse_before_mutation() {
    for changed in 0..5 {
        let (cfg, p, t, s) = rig(2, Rate::new(3, 2).unwrap());
        let (origin, start, preroll, rate, end) = match changed {
            0 => (
                point(3, 500),
                ts(100_002),
                s.preroll(),
                s.speed().rate(),
                s.end(),
            ),
            1 => (
                point(3, 500),
                s.start(),
                s.preroll(),
                s.speed().rate(),
                Some(ts(100_013)),
            ),
            2 => (point(3, 500), s.start(), s.preroll(), Rate::NORMAL, s.end()),
            3 => (
                point(3, 500),
                s.start(),
                Duration::from_nanos(4),
                s.speed().rate(),
                s.end(),
            ),
            _ => (
                point(3, 501),
                s.start(),
                s.preroll(),
                s.speed().rate(),
                s.end(),
            ),
        };
        let wrong = changed_transport(&t, origin, start, preroll, rate, end);
        assert_result(&cfg, &p, &wrong, Some(s), false);
    }
}
#[test]
fn changed_config_original_section_and_end_refuse_even_with_same_transport() {
    for changed in [false, true] {
        let (mut cfg, p, t, s) = rig(0, Rate::NORMAL);
        if changed {
            cfg.audio.section_start = ts(100_002);
        } else {
            cfg.audio.gameplay.end_song = Some(ts(100_013));
        }
        assert_result(&cfg, &p, &t, Some(s), false);
    }
}
#[test]
fn hidden_noop_command_chronology_is_not_pristine_cold_phase() {
    let (cfg, p, mut t, s) = rig(0, Rate::NORMAL);
    let anchors = t.anchors().len();
    let phase = t
        .output_phase_basis()
        .unwrap()
        .point_from_elapsed_nanos(point(3, 501), 0)
        .unwrap();
    t.resume_at_output_phase(phase).unwrap();
    assert_eq!(t.anchors().len(), anchors);
    assert_result(&cfg, &p, &t, Some(s), false);
}
#[test]
fn distinct_attempt_owner_generation_are_not_native_epoch_numbers() {
    let (cfg, p, _old, s) = rig(0, Rate::new(2, 1).unwrap());
    let phase = OutputPhaseBasis::new(ClockDomainId(3), u64::MAX, u64::MAX - 1, 6, 3).unwrap();
    let t = Transport::new_with_output_phase(
        phase,
        point(3, 500),
        s.start(),
        s.preroll(),
        s.speed().rate(),
        s.end(),
    )
    .unwrap();
    assert_result(&cfg, &p, &t, Some(s), true);
    assert_ne!(
        t.output_phase_basis().unwrap().generation(),
        p.authority().epoch().id
    );
}
#[test]
fn earlier_history_origin_and_nonzero_playback_origin_are_not_double_added() {
    let (mut cfg, p, t, s) = rig(1, Rate::NORMAL);
    let epoch = p.authority().epoch();
    cfg.audio.gameplay.playback_origin.timestamp = ts(epoch.stream_origin.timestamp.as_nanos() + 7);
    let exact = changed_transport(
        &t,
        point(3, 507),
        s.start(),
        s.preroll(),
        s.speed().rate(),
        s.end(),
    );
    assert_result(&cfg, &p, &exact, Some(s), true);
    assert_result(&cfg, &p, &t, Some(s), false);
}
#[test]
fn public_coordinate_wrapper_preserves_old_audio_config_literal_shape() {
    let (cfg, _, _, _) = rig(0, Rate::NORMAL);
    let old = AudioGameplayConfig {
        gameplay: cfg.audio.gameplay,
        section_start: cfg.audio.section_start,
    };
    let wrapped = CoordinateGameplayConfig {
        audio: old,
        source_history: cfg.source_history,
    };
    assert_eq!(wrapped.audio.section_start, old.section_start);
    assert_eq!(wrapped.source_history, cfg.source_history);
}

#[test]
fn independently_restarted_authority_cannot_grant_binding_from_old_native_record() {
    let (cfg, mut p, t, s) = rig(0, Rate::NORMAL);
    let merger = crate::local_input::InputMerger::new(
        ClockDomainId(1),
        point(1, 0),
        vec![beatkernel::input::DeviceId(9)],
        8,
    )
    .unwrap();
    let restart = p.authority().prepare_correlation_restart(&merger).unwrap();
    p.authority_mut()
        .commit_correlation_restart(restart, &merger)
        .unwrap();
    assert_eq!(p.authority().history_len(), 0);
    assert!(p.latest_record().is_some());
    assert_result(&cfg, &p, &t, Some(s), false);
}

#[test]
fn phase_rate_mismatch_refuses_even_with_matching_descriptor_and_config() {
    let (cfg, p, _old, s) = rig(0, Rate::NORMAL);
    let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 113, 6, 4).unwrap();
    let t = Transport::new_with_output_phase(
        phase,
        point(3, 500),
        s.start(),
        s.preroll(),
        s.speed().rate(),
        s.end(),
    )
    .unwrap();
    assert_result(&cfg, &p, &t, Some(s), false);
}

fn runtime_rig(
    kind: u8,
    rate: Rate,
) -> (
    CoordinateGameplayConfig,
    NativeAudioPresentation,
    Transport,
    LiveSpeedSetup,
) {
    let (cfg, presentation, fixed, setup) = rig(kind, rate);
    let transport = Transport::new_with_runtime_output_phase(
        fixed.output_phase_basis().unwrap(),
        point(3, 500),
        setup.start(),
        setup.preroll(),
        setup.speed().rate(),
        setup.end(),
    )
    .unwrap();
    (cfg, presentation, transport, setup)
}

#[test]
fn explicit_runtime_pristine_accepts_actual_source_counter_and_target_descriptors() {
    for kind in 0..3 {
        for rate in [
            Rate::NORMAL,
            Rate::new(1, 2).unwrap(),
            Rate::new(3, 2).unwrap(),
        ] {
            let (cfg, presentation, transport, setup) = runtime_rig(kind, rate);
            assert_eq!(transport.runtime_output_rate(), Some(rate));
            assert_result(&cfg, &presentation, &transport, Some(setup), true);
            assert_eq!(transport.anchors().len(), 1);
        }
    }
}

#[test]
fn runtime_opt_in_keeps_exact_preroll_section_and_original_end_validation() {
    for changed in 0..4 {
        let (mut cfg, presentation, transport, setup) = runtime_rig(1, Rate::new(1, 2).unwrap());
        let selected = match changed {
            0 => Some(
                LiveSpeedSetup::new(
                    setup.start(),
                    Duration::from_nanos(2),
                    setup.speed(),
                    setup.end(),
                )
                .unwrap(),
            ),
            1 => {
                cfg.audio.section_start = ts(100_002);
                Some(setup)
            }
            2 => {
                cfg.audio.gameplay.end_song = Some(ts(100_013));
                Some(setup)
            }
            _ => None,
        };
        assert_result(&cfg, &presentation, &transport, selected, false);
    }
}

#[test]
fn runtime_noop_control_chronology_still_cannot_impersonate_pristine_startup() {
    let (cfg, presentation, mut transport, setup) = runtime_rig(2, Rate::new(3, 2).unwrap());
    let phase = transport.output_phase_basis().unwrap();
    let at = phase.point_from_elapsed_nanos(point(3, 501), 0).unwrap();
    transport
        .set_rate_at_output_phase(at, setup.speed().rate())
        .unwrap();
    assert_eq!(transport.anchors().len(), 1);
    assert_eq!(transport.runtime_output_rate(), Some(setup.speed().rate()));
    assert_result(&cfg, &presentation, &transport, Some(setup), false);
}

#[test]
fn runtime_changed_basis_logical_origin_and_native_epoch_are_not_admission_proof() {
    for changed in 0..4 {
        let (mut cfg, presentation, old, setup) = runtime_rig(0, Rate::NORMAL);
        let transport = match changed {
            0 => {
                cfg.audio.gameplay.stream_origin.domain = ClockDomainId(9);
                old
            }
            1 => {
                cfg.source_history = SourcePlaybackIdentity::new(4, 2, 1).unwrap();
                old
            }
            2 => Transport::new_with_runtime_output_phase(
                old.output_phase_basis().unwrap(),
                point(3, 501),
                setup.start(),
                setup.preroll(),
                setup.speed().rate(),
                setup.end(),
            )
            .unwrap(),
            _ => Transport::new_with_runtime_output_phase(
                OutputPhaseBasis::new(ClockDomainId(9), 91, 113, 6, 3).unwrap(),
                point(9, 500),
                setup.start(),
                setup.preroll(),
                setup.speed().rate(),
                setup.end(),
            )
            .unwrap(),
        };
        assert_result(&cfg, &presentation, &transport, Some(setup), false);
    }
}
