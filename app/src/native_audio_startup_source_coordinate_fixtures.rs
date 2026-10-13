//! Actual mixer histories and compound software publications; no hardware timing claim.
use super::*;
use crate::{
    bgm::BgmConfig,
    gameplay::output::ports::{
        NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
        SourcePlaybackIdentity,
    },
    native_audio_presentation::NativeAudioSnapshot,
    native_coordinate_control::CoordinateControlFacts,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::audio::{
    command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
    SampleBank, SampleId, VoiceId, TargetFrameBasis, TargetTime,
};
use beatkernel::time::ClockMappingQuality;
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
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}

struct Port {
    mixer: Mixer,
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    counter: bool,
    round: usize,
    acquisitions: usize,
    waits: usize,
    host_base: i64,
    second_host: i64,
    missing: bool,
    missing_report: bool,
    cancel: bool,
    regress: bool,
    fault: u8,
    stale_return: bool,
    prior: Option<CoordinateControlFacts>,
    reports: Vec<RenderReport>,
}
impl NativeAudioSeedPort for Port {
    fn service_input(&mut self) -> NativeGameplayResult<bool> {
        self.round += 1;
        if self.fault == 1 {
            return Err("input failure".into());
        }
        Ok(!self.cancel)
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        panic!("source startup must not acquire an independent legacy observation")
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        panic!("source startup must not read an independent render cache")
    }
    fn observe_coordinate_audio(
        &mut self,
        p: &mut NativeAudioPresentation,
        expected: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        self.acquisitions += 1;
        if self.fault == 2 {
            return Err("compound acquisition failure".into());
        }
        if self.missing {
            return Ok(None);
        }
        assert_eq!(expected, self.identity);
        let report = self.mixer.render(&mut [0.0; 2])?;
        self.reports.push(report);
        let frame = self.round as u64;
        let absolute = self.basis.start_physical_frame() + frame;
        let query = InterpolatedTargetTime::new(
            TargetTime::from_frames(absolute, self.identity.hz())?,
            0,
            0,
            1,
            0,
            1,
        )?;
        let at = if self.round == 1 {
            100
        } else if self.round == 2 {
            self.second_host
        } else {
            self.second_host + self.round as i64 - 2
        };
        let coordinate = if self.counter {
            NativeOutputCoordinate::Counter {
                position: frame * 2,
                frequency: 6,
            }
        } else {
            NativeOutputCoordinate::StreamFrame { frame }
        };
        let evidence = if self.counter {
            OriginalNativePresentationEvidence::Wasapi {
                basis: Some(self.basis),
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters::default(),
                    render: None,
                    clock: Some(AudioClockSnapshot {
                        position: frame * 2,
                        frequency: 6,
                        qpc_100ns: frame,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(host(at)),
                        mapping_quality: ClockMappingQuality::Unknown,
                    }),
                },
            }
        } else {
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: self.basis.point_at_stream_frame(frame)?,
                target: host(at),
            })
        };
        let native = CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: 7,
                basis: self.basis,
                evidence,
            },
            coordinate,
        )?;
        let history = self.mixer.source_playback_history().unwrap();
        let playback = history
            .lookup_at_source_time(query)
            .map_err(|error| format!("actual startup source-history lookup: {error:?}"))?;
        let packet = NativeSourcePlaybackQuery::new(
            7,
            self.basis,
            self.identity,
            query,
            SourcePlaybackFacts {
                playback,
                pulled_source_end: history.pulled_end_frame(),
                submitted_source_end: history.pulled_end_frame(),
                render: if self.missing_report {
                    None
                } else {
                    Some(report)
                },
                revision: (self.acquisitions as u64) * 2,
            },
            Some(evidence),
        )?;
        let observation = NativePlaybackObservation::new(native, packet)?;
        p.admit_coordinate(native)?;
        let facts = CoordinateControlFacts::from_source(&observation, p, expected)?;
        if self.stale_return && self.round == 2 {
            return Ok(self.prior);
        }
        self.prior = Some(facts);
        Ok(Some(facts))
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        if self.fault == 4 {
            return Err("HOST failure".into());
        }
        if self.fault == 5 {
            return Ok(point(9, self.host_base));
        }
        Ok(host(if self.regress && self.round >= 2 {
            99
        } else {
            self.host_base + self.round as i64
        }))
    }
    fn wait(&mut self, duration: std::time::Duration) -> NativeGameplayResult<()> {
        assert_eq!(duration, std::time::Duration::from_millis(1));
        self.waits += 1;
        if self.fault == 3 {
            return Err("wait failure".into());
        }
        Ok(())
    }
}

fn rig(counter: bool, finite: bool) -> (Port, NativeAudioPresentation, BgmFeeder, CommandProducer) {
    let format = AudioFormat::new(3, 1).unwrap();
    let (producer, consumer) = command_queue(32).unwrap();
    let config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::from_nanos(100),
        AudioLimits::new(32, 2, 8, 64, 8).unwrap(),
    );
    let mut mixer = Mixer::new(
        if finite {
            config.with_playback_end_frame(4)
        } else {
            config
        },
        SampleBank::new(format, PcmLimits::new(256, 512, 1).unwrap()).unwrap(),
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 2]).unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    let history = mixer.source_playback_history().unwrap();
    let identity = SourcePlaybackIdentity::new(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    assert_eq!(identity.initial_physical(), 2);
    assert_eq!(identity.initial_playback(), 2);
    let basis = mixer.output_frame_basis();
    let descriptor = if counter {
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 6,
        }
    } else {
        AudioAuthorityCoordinateBasis::SourceFrame(basis)
    };
    let p = new_coordinate_audio_presentation(
        7,
        descriptor,
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig {
            history_capacity: 16,
            max_observation_age: Duration::from_nanos(100),
            ..AudioAuthorityConfig::default()
        },
    )
    .unwrap();
    let bgm = BgmFeeder::new(
        vec![AudioCommand::Play {
            at: Timestamp::from_nanos(2_000_000_000),
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 1.0,
        }],
        BgmConfig {
            output_origin: basis.origin(),
            sample_rate: 3,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(333_333_333),
            max_pending: 8,
        },
    )
    .unwrap();
    (
        Port {
            mixer,
            basis,
            identity,
            counter,
            round: 0,
            acquisitions: 0,
            waits: 0,
            host_base: 100,
            second_host: 102,
            missing: false,
            missing_report: false,
            cancel: false,
            regress: false,
            fault: 0,
            stale_return: false,
            prior: None,
            reports: vec![],
        },
        p,
        bgm,
        producer,
    )
}
fn prime(
    port: &mut Port,
    p: &mut NativeAudioPresentation,
    bgm: &mut BgmFeeder,
    producer: &mut CommandProducer,
    bound: i64,
) -> NativeGameplayResult<Option<SeededCoordinateNativeAudio>> {
    let identity = port.identity;
    prime_source_coordinate_native_audio(
        port,
        p,
        identity,
        bgm,
        producer,
        Duration::from_nanos(bound),
    )
}

#[test]
fn actual_nonzero_history_and_compound_source_frame_seed_preserve_originals() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    assert_eq!(
        seed.basis(),
        AudioAuthorityCoordinateBasis::SourceFrame(port.basis)
    );
    assert_eq!(
        seed.observations()[0].coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 1 })
    );
    assert_eq!(
        seed.observations()[1].coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 2 })
    );
    assert_eq!(seed.observations()[0].pair().target, host(100));
    assert_eq!(seed.observations()[1].pair().target, host(102));
    assert_eq!(seed.now(), host(102));
    assert_eq!(port.acquisitions, 2);
    assert_eq!(port.waits, 1);
    assert_eq!(
        port.prior.unwrap().source().render,
        port.reports.last().copied()
    );
    assert_eq!(p.authority().acquired_prefix(), None);
    assert_eq!(p.authority().closed_host_prefix(), None);
}

#[test]
fn positive_original_counter_units_seed_same_source_query_without_counter_zero() {
    let (mut port, mut p, mut bgm, mut producer) = rig(true, false);
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    assert_eq!(
        seed.observations()[0].coordinate(),
        Some(NativeOutputCoordinate::Counter {
            position: 2,
            frequency: 6
        })
    );
    assert_eq!(
        seed.observations()[1].coordinate(),
        Some(NativeOutputCoordinate::Counter {
            position: 4,
            frequency: 6
        })
    );
    assert_eq!(
        seed.observations()[1].pair().source,
        port.basis.point_at_stream_frame(2).unwrap()
    );
    assert_eq!(port.acquisitions, 2);
}

#[test]
fn bgm_admits_only_from_same_publication_report() {
    for missing in [false, true] {
        let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
        port.missing_report = missing;
        prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
            .unwrap()
            .unwrap();
        assert_eq!(bgm.report().total_admitted, usize::from(!missing));
        assert_eq!(port.prior.unwrap().source().render.is_none(), missing);
    }
}

#[test]
fn second_future_origin_is_immutable_while_actual_packets_keep_bgm_progressing() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
    port.second_host = 105;
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    assert_eq!(seed.now(), host(105));
    assert_eq!(seed.observations()[0].pair().target, host(100));
    assert_eq!(seed.observations()[1].pair().target, host(105));
    assert_eq!(port.acquisitions, 5);
    assert_eq!(port.waits, 4);
    assert_eq!(
        p.authority()
            .latest_coordinate_observation()
            .unwrap()
            .pair()
            .target,
        host(108)
    );
    assert_eq!(bgm.report().total_admitted, 1);
    assert_eq!(
        seed.observations()[1].coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 2 })
    );
}

#[test]
fn unavailable_compound_packets_timeout_without_independent_report_or_native_evidence() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
    port.missing = true;
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 3).is_err());
    assert_eq!(port.acquisitions, 3);
    assert_eq!(p.authority().history_len(), 0);
    assert!(p.latest_record().is_none());
    assert_eq!(bgm.report().total_admitted, 0);
}

#[test]
fn cancellation_does_not_acquire_or_feed_compound_packet() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
    port.cancel = true;
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .is_none());
    assert_eq!(port.round, 1);
    assert_eq!(port.acquisitions, 0);
    assert_eq!(bgm.report().total_admitted, 0);
}

#[test]
fn stale_returned_facts_are_revalidated_before_bgm_feed() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
    port.stale_return = true;
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.acquisitions, 2);
    // The first report reaches frame four, before this cue's frame-six horizon.
    assert_eq!(bgm.report().total_admitted, 0);
    assert!(p.authority().closed_host_prefix().is_none());
}

#[test]
fn expired_original_second_row_refuses_readiness() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
    port.host_base = 110;
    let descriptor = p.authority().coordinate_basis().unwrap();
    p = new_coordinate_audio_presentation(
        7,
        descriptor,
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig {
            max_observation_age: Duration::from_nanos(1),
            ..AudioAuthorityConfig::default()
        },
    )
    .unwrap();
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.acquisitions, 2);
}

#[test]
fn regressing_host_refuses_without_input_or_control_ack() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
    port.regress = true;
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert!(p.authority().acquired_prefix().is_none());
    assert!(p.authority().closed_host_prefix().is_none());
}

#[test]
fn input_compound_wait_and_host_errors_propagate() {
    for fault in 1..=5 {
        let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
        port.fault = fault;
        assert!(
            prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err(),
            "fault {fault}"
        );
        assert!(p.authority().closed_host_prefix().is_none());
        if fault >= 4 {
            assert_eq!(port.round, 0);
        }
    }
}

#[test]
fn nonpositive_timeout_refuses_before_any_io() {
    for timeout in [0, -1] {
        let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
        assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, timeout).is_err());
        assert_eq!(port.round, 0);
        assert_eq!(port.acquisitions, 0);
    }
}

#[test]
fn wrong_source_rate_or_history_creation_refuses_cold_before_io() {
    for supplied in [
        SourcePlaybackIdentity::new(4, 2, 2).unwrap(),
        SourcePlaybackIdentity::new(3, 3, 2).unwrap(),
    ] {
        let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
        let before = format!("{p:?}");
        assert!(prime_source_coordinate_native_audio(
            &mut port,
            &mut p,
            supplied,
            &mut bgm,
            &mut producer,
            Duration::from_nanos(100)
        )
        .is_err());
        assert_eq!(port.round, 0);
        assert_eq!(port.acquisitions, 0);
        assert_eq!(format!("{p:?}"), before);
    }
}

#[test]
fn target_descriptor_refuses_before_first_io_and_never_stitches_source_report() {
    let (mut port, _, mut bgm, mut producer) = rig(false, false);
    let basis = TargetFrameBasis::new(
        point(2, 100),
        TargetTime::from_frames(2, 3).unwrap(),
        48_000,
    )
    .unwrap();
    let mut p = new_coordinate_audio_presentation(
        7,
        AudioAuthorityCoordinateBasis::TargetFrame(basis),
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    let before = format!("{p:?}");
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.round, 0);
    assert_eq!(port.acquisitions, 0);
    assert_eq!(format!("{p:?}"), before);
}

#[test]
fn scalar_descriptor_refuses_before_io_without_coordinate_fallback() {
    let (mut port, _, mut bgm, mut producer) = rig(false, false);
    let mut p = new_audio_presentation(
        7,
        port.basis,
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.round, 0);
}

#[test]
fn independently_rebound_authority_epoch_refuses_before_io() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
    let mut epoch = p.authority().epoch();
    epoch.id += 1;
    *p.authority_mut() = AudioAuthority::new_with_coordinates(
        p.authority().config(),
        epoch,
        p.authority().coordinate_basis().unwrap(),
    )
    .unwrap();
    let before = format!("{p:?}");
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.round, 0);
    assert_eq!(port.acquisitions, 0);
    assert_eq!(format!("{p:?}"), before);
}

#[test]
fn already_observed_owners_refuse_before_another_io_iteration() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, false);
    prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    let before = format!("{p:?}");
    let rounds = port.round;
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.round, rounds);
    assert_eq!(format!("{p:?}"), before);
}

#[test]
fn actual_finite_marker_persists_after_seed_without_startup_end_ack() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false, true);
    prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    assert_eq!(port.reports[0].playback_end_physical_frame, Some(4));
    assert_eq!(port.reports[1].playback_end_physical_frame, Some(4));
    assert_eq!(
        port.prior
            .unwrap()
            .source()
            .render
            .unwrap()
            .playback_end_physical_frame,
        Some(4)
    );
    let subsequent = port.mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(subsequent.playback_end_physical_frame, Some(4));
    assert_eq!(subsequent.playback_frames, 0);
    assert!(p.authority().closed_host_prefix().is_none());
    assert!(p.authority().acquired_prefix().is_none());
}

#[test]
fn legacy_only_seed_port_defaults_to_unsupported_without_scalar_fallback() {
    struct LegacyOnly;
    impl NativeAudioSeedPort for LegacyOnly {
        fn service_input(&mut self) -> NativeGameplayResult<bool> {
            Ok(true)
        }
        fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
            panic!("unsupported source seed must not call legacy observation")
        }
        fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
            panic!("unsupported source seed must not read legacy report")
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(host(100))
        }
        fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
            panic!("unsupported source seed must return before waiting")
        }
    }
    let (port, mut p, mut bgm, mut producer) = rig(false, false);
    let before = format!("{p:?}");
    assert!(prime_source_coordinate_native_audio(
        &mut LegacyOnly,
        &mut p,
        port.identity,
        &mut bgm,
        &mut producer,
        Duration::from_nanos(100)
    )
    .is_err());
    assert_eq!(format!("{p:?}"), before);
    assert_eq!(bgm.report().total_admitted, 0);
}
