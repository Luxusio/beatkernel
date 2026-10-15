//! Actual shared startup loop and admitted records; mathematical IO doubles only.
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::{BgmConfig, BgmFeeder},
    native_audio_presentation::{
        NativeAudioPresentation, NativeAudioSnapshot, TargetNativeAudioSnapshot,
    },
    native_audio_startup::*,
    native_gameplay::NativeGameplayResult,
    native_output_coordinate::{
        CoordinateNativeAudioSnapshot, CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate,
    },
};
use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, CommandProducer, InterpolatedTargetTime, Mixer,
        MixerConfig, OutputFrameBasis, PcmLimits, RenderReport, SampleBank, TargetFrameBasis,
        TargetTime,
    },
    time::{ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp},
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
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn descriptors() -> [AudioAuthorityCoordinateBasis; 3] {
    let b = OutputFrameBasis::new(point(2, 100), 4_000_000_000, 0).unwrap();
    [
        AudioAuthorityCoordinateBasis::SourceFrame(b),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis: b,
            frequency: 4_000_000_000,
        },
        AudioAuthorityCoordinateBasis::TargetFrame(
            TargetFrameBasis::new(
                point(2, 100),
                TargetTime::from_frames(0, 1).unwrap(),
                4_000_000_000,
            )
            .unwrap(),
        ),
    ]
}
fn coordinate(b: AudioAuthorityCoordinateBasis, n: u64) -> NativeOutputCoordinate {
    match b {
        AudioAuthorityCoordinateBasis::SourceTimestamp(_) => {
            panic!("SourceTimestamp is not a native coordinate in this fixture")
        }
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => {
            NativeOutputCoordinate::Counter {
                position: n,
                frequency,
            }
        }
        _ => NativeOutputCoordinate::StreamFrame { frame: n },
    }
}
fn admit(
    p: &mut NativeAudioPresentation,
    b: AudioAuthorityCoordinateBasis,
    n: u64,
    at: i64,
) -> NativeGameplayResult<()> {
    match b {
        AudioAuthorityCoordinateBasis::SourceTimestamp(_) => {
            panic!("SourceTimestamp is not a native coordinate in this fixture")
        }
        AudioAuthorityCoordinateBasis::SourceFrame(basis) => {
            p.admit_coordinate(CoordinateNativeAudioSnapshot::new(
                NativeAudioSnapshot {
                    epoch: 7,
                    basis,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                        source: basis.point_at_stream_frame(n)?,
                        target: host(at),
                    }),
                },
                coordinate(b, n),
            )?)?;
        }
        AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency } => {
            p.admit_coordinate(CoordinateNativeAudioSnapshot::new(
                NativeAudioSnapshot {
                    epoch: 7,
                    basis,
                    evidence: OriginalNativePresentationEvidence::Wasapi {
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
                                host_point: Some(host(at)),
                                mapping_quality: ClockMappingQuality::Unknown,
                            }),
                        },
                    },
                },
                coordinate(b, n),
            )?)?;
        }
        AudioAuthorityCoordinateBasis::TargetFrame(basis) => {
            p.admit_target_coordinate(CoordinateTargetNativeAudioSnapshot::new(
                TargetNativeAudioSnapshot {
                    epoch: 7,
                    basis,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                        source: basis.point_at_stream_frame(n)?,
                        target: host(at),
                    }),
                },
                coordinate(b, n),
            )?)?;
        }
    }
    Ok(())
}
struct Port {
    basis: AudioAuthorityCoordinateBasis,
    mixer: Mixer,
    round: usize,
    observe_calls: usize,
    waits: usize,
    now: i64,
    first_host: i64,
    second_host: i64,
    missing: bool,
    cancel: bool,
    regress: bool,
    fault: u8,
    rebind: Option<NativeAudioPresentation>,
    authority_rebind: Option<AudioAuthority>,
}
impl NativeAudioSeedPort for Port {
    fn service_input(&mut self) -> NativeGameplayResult<bool> {
        self.round += 1;
        self.mixer.render(&mut [0.0; 1])?;
        if self.fault == 1 {
            return Err("input fault".into());
        }
        Ok(!self.cancel)
    }
    fn observe_audio(&mut self, p: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.observe_calls += 1;
        if self.fault == 2 {
            return Err("observation fault".into());
        }
        if self.missing {
            return Ok(());
        }
        if self.round == 2 {
            if let Some(authority) = self.authority_rebind.take() {
                *p.authority_mut() = authority;
            }
            if let Some(replacement) = self.rebind.take() {
                *p = replacement;
                let AudioAuthorityCoordinateBasis::TargetFrame(basis) =
                    p.authority().coordinate_basis().unwrap()
                else {
                    unreachable!()
                };
                p.admit_target_coordinate(CoordinateTargetNativeAudioSnapshot::new(
                    TargetNativeAudioSnapshot {
                        epoch: p.authority().epoch().id,
                        basis,
                        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                            source: basis.point_at_stream_frame(2)?,
                            target: host(self.second_host),
                        }),
                    },
                    NativeOutputCoordinate::StreamFrame { frame: 2 },
                )?)?;
                return Ok(());
            }
        }
        admit(
            p,
            self.basis,
            self.round as u64,
            if self.round == 1 {
                self.first_host
            } else {
                self.second_host
            },
        )
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        if self.fault == 3 {
            Err("report fault".into())
        } else {
            Ok(None)
        }
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        if self.fault == 5 {
            return Err("HOST read fault".into());
        }
        if self.fault == 6 {
            return Ok(point(9, self.now));
        }
        Ok(host(if self.regress && self.round >= 2 {
            99
        } else {
            self.now + self.round as i64
        }))
    }
    fn wait(&mut self, d: std::time::Duration) -> NativeGameplayResult<()> {
        assert_eq!(d, std::time::Duration::from_millis(1));
        self.waits += 1;
        if self.fault == 4 {
            return Err("wait fault".into());
        }
        Ok(())
    }
}
fn rig(
    basis: AudioAuthorityCoordinateBasis,
    age: i64,
) -> (Port, NativeAudioPresentation, BgmFeeder, CommandProducer) {
    let format = AudioFormat::new(2_000_000_000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 64, 1).unwrap()).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mixer = Mixer::new(
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
    let p = new_coordinate_audio_presentation(
        7,
        basis,
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig {
            max_observation_age: Duration::from_nanos(age),
            ..AudioAuthorityConfig::default()
        },
    )
    .unwrap();
    let bgm = BgmFeeder::new(
        vec![],
        BgmConfig {
            output_origin: point(2, 100),
            sample_rate: 2_000_000_000,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(1),
            max_pending: 1,
        },
    )
    .unwrap();
    (
        Port {
            basis,
            mixer,
            round: 0,
            observe_calls: 0,
            waits: 0,
            now: 100,
            first_host: 100,
            second_host: 102,
            missing: false,
            cancel: false,
            regress: false,
            fault: 0,
            rebind: None,
            authority_rebind: None,
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
    prime_coordinate_native_audio(port, p, bgm, producer, Duration::from_nanos(bound))
}
fn q(base: TargetTime, ticks: u64, f: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(base, ticks, ticks, f, 0, 1).unwrap()
}

#[test]
fn shared_startup_retains_original_flat_ns_rows_for_all_three_descriptors() {
    for basis in descriptors() {
        let (mut port, mut p, mut bgm, mut producer) = rig(basis, 100);
        let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
            .unwrap()
            .unwrap();
        assert_eq!(seed.basis(), basis);
        assert_eq!(seed.epoch(), p.authority().epoch());
        assert_eq!(seed.now(), host(102));
        let rows = seed.observations();
        assert_eq!(rows[0].coordinate(), Some(coordinate(basis, 1)));
        assert_eq!(rows[1].coordinate(), Some(coordinate(basis, 2)));
        assert_eq!(rows[1].pair(), p.latest_record().unwrap().pair());
        assert_eq!(rows[0].pair().source, point(2, 100));
        assert_eq!(rows[1].pair().source, point(2, 100));
        assert_eq!(p.authority().history_len(), 2);
        assert_eq!(port.observe_calls, 2);
        assert_eq!(port.waits, 1);
        assert_eq!(p.authority().acquired_prefix(), None);
        assert_eq!(p.authority().closed_host_prefix(), None);
    }
}
#[test]
fn future_right_association_is_waited_for_without_readmitting_another_row() {
    let (mut port, mut p, mut bgm, mut producer) = rig(descriptors()[0], 100);
    port.second_host = 105;
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    assert_eq!(seed.now(), host(105));
    assert_eq!(port.observe_calls, 2);
    assert_eq!(port.waits, 4);
    assert_eq!(p.authority().history_len(), 2);
}
#[test]
fn missing_rows_timeout_and_cancellation_do_not_invent_evidence() {
    for cancel in [false, true] {
        let (mut port, mut p, mut bgm, mut producer) = rig(descriptors()[0], 100);
        port.missing = true;
        port.cancel = cancel;
        let result = prime(&mut port, &mut p, &mut bgm, &mut producer, 3);
        if cancel {
            assert!(result.unwrap().is_none());
        } else {
            assert!(result.is_err());
        }
        assert_eq!(p.authority().history_len(), 0);
        assert!(p.latest_record().is_none());
    }
}
#[test]
fn stale_future_and_regressing_host_readiness_refuse() {
    for mode in 0..3 {
        let (mut port, mut p, mut bgm, mut producer) = rig(descriptors()[0], 1);
        match mode {
            0 => {
                port.now = 110;
                port.second_host = 102;
            }
            1 => port.second_host = 1000,
            _ => port.regress = true,
        }
        assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 5).is_err());
        assert!(p.authority().closed_host_prefix().is_none());
    }
}
#[test]
fn actual_shared_loop_propagates_each_input_observation_report_and_wait_fault() {
    for fault in 1..=4 {
        let (mut port, mut p, mut bgm, mut producer) = rig(descriptors()[0], 100);
        port.fault = fault;
        assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
        assert!(p.authority().acquired_prefix().is_none());
    }
}
#[test]
fn nonpositive_bound_and_non_cold_owner_refuse_before_servicing_port() {
    let (mut port, mut p, mut bgm, mut producer) = rig(descriptors()[0], 100);
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 0).is_err());
    assert_eq!(port.round, 0);
    admit(&mut p, descriptors()[0], 1, 100).unwrap();
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.round, 0);
}
#[test]
fn coordinate_primer_rejects_scalar_owner_without_consuming_input() {
    let (mut port, _, mut bgm, mut producer) = rig(descriptors()[0], 100);
    let AudioAuthorityCoordinateBasis::SourceFrame(basis) = descriptors()[0] else {
        unreachable!()
    };
    let mut p = new_audio_presentation(
        7,
        basis,
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.round, 0);
}
#[test]
fn inverse_uses_minimum_integer_host_not_rounded_audio_endpoint_inverse() {
    let (mut port, mut p, mut bgm, mut producer) = rig(descriptors()[0], 100);
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    let before = format!("{p:?}");
    let base = TargetTime::from_frames(0, 1).unwrap();
    assert_eq!(
        seed.host_for_query(&q(base, 3, 8_000_000_000), Duration::ZERO)
            .unwrap(),
        Some(host(101))
    );
    assert_eq!(
        seed.host_for_query(&q(base, 1, 4_000_000_000), Duration::ZERO)
            .unwrap(),
        Some(host(100))
    );
    assert_eq!(
        seed.host_for_query(&q(base, 1, 2_000_000_000), Duration::ZERO)
            .unwrap(),
        Some(host(102))
    );
    assert!(seed
        .host_for_query(&q(base, 5, 8_000_000_000), Duration::ZERO)
        .unwrap()
        .is_none());
    assert_eq!(format!("{p:?}"), before);
}
#[test]
fn inverse_finite_host_backward_permission_and_negative_permission_are_enforced() {
    let (mut port, mut p, mut bgm, mut producer) = rig(descriptors()[0], 100);
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    let zero = q(TargetTime::from_frames(0, 1).unwrap(), 0, 1);
    assert!(seed
        .host_for_query(&zero, Duration::from_nanos(1))
        .unwrap()
        .is_none());
    assert_eq!(
        seed.host_for_query(&zero, Duration::from_nanos(2)).unwrap(),
        Some(host(98))
    );
    assert!(seed
        .host_for_query(&zero, Duration::from_nanos(-1))
        .is_err());
}
#[test]
fn inverse_matches_independent_small_rational_ceil_oracle_with_fractional_creation() {
    for target in [false, true] {
        let basis = if target {
            AudioAuthorityCoordinateBasis::TargetFrame(
                TargetFrameBasis::new(point(2, 100), TargetTime::new(0, 1, 3).unwrap(), 3).unwrap(),
            )
        } else {
            AudioAuthorityCoordinateBasis::SourceFrame(
                OutputFrameBasis::new(point(2, 100), 3, 1).unwrap(),
            )
        };
        let (mut port, mut p, mut bgm, mut producer) = rig(basis, 100);
        port.second_host = 107;
        let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
            .unwrap()
            .unwrap();
        for numerator in 0..=12u64 {
            let query = q(TargetTime::new(0, 1, 3).unwrap(), numerator, 12);
            let relative = i128::from(numerator) * 7 - 28;
            let ceil = relative.div_euclid(4) + i128::from(relative.rem_euclid(4) != 0);
            let expected = if numerator > 8 {
                None
            } else {
                Some(host(100 + ceil as i64))
            };
            assert_eq!(
                seed.host_for_query(&query, Duration::from_nanos(7))
                    .unwrap(),
                expected,
                "target={target}, numerator={numerator}"
            );
        }
    }
}

#[test]
fn inverse_handles_full_signed_host_span_and_max_frequency_without_narrowing() {
    let basis = AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(point(2, 100), 3, 0).unwrap(),
        frequency: u64::MAX,
    };
    let (mut port, mut p, mut bgm, mut producer) = rig(basis, i64::MAX);
    port.first_host = i64::MIN;
    port.second_host = i64::MAX;
    port.now = i64::MAX - 2;
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 10)
        .unwrap()
        .unwrap();
    let query =
        InterpolatedTargetTime::new(TargetTime::from_frames(0, 1).unwrap(), 1, 2, u64::MAX, 1, 2)
            .unwrap();
    assert_eq!(
        seed.host_for_query(&query, Duration::ZERO).unwrap(),
        Some(host(0))
    );
    assert!(seed
        .host_for_query(&query, Duration::from_nanos(1))
        .is_err());
    assert!(seed
        .host_for_query(
            &q(TargetTime::from_frames(0, 1).unwrap(), 3, u64::MAX),
            Duration::ZERO
        )
        .unwrap()
        .is_none());
}

#[test]
fn host_failure_wrong_domain_and_invalid_cold_descriptor_refuse_without_service() {
    for fault in [5, 6] {
        let (mut port, mut p, mut bgm, mut producer) = rig(descriptors()[0], 100);
        port.fault = fault;
        assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
        assert_eq!(port.round, 0);
        assert_eq!(p.authority().history_len(), 0);
    }
    let AudioAuthorityCoordinateBasis::SourceFrame(basis) = descriptors()[0] else {
        unreachable!()
    };
    assert!(new_coordinate_audio_presentation(
        7,
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 0
        },
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig::default()
    )
    .is_err());
    assert!(new_coordinate_audio_presentation(
        7,
        AudioAuthorityCoordinateBasis::SourceFrame(basis),
        ClockDomainId(2),
        point(3, 500),
        AudioAuthorityConfig::default()
    )
    .is_err());
}

#[test]
fn rebind_of_authority_epoch_or_descriptor_between_rows_refuses_seed() {
    for epoch_change in [false, true] {
        let (mut port, mut p, mut bgm, mut producer) = rig(descriptors()[0], 100);
        let mut epoch = p.authority().epoch();
        if epoch_change {
            epoch.id += 1;
        }
        let descriptor = if epoch_change {
            descriptors()[0]
        } else {
            descriptors()[2]
        };
        port.authority_rebind = Some(
            AudioAuthority::new_with_coordinates(p.authority().config(), epoch, descriptor)
                .unwrap(),
        );
        assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
        assert_eq!(port.observe_calls, 2);
        assert_eq!(p.latest_record().unwrap().pair().target, host(100));
        assert!(p.authority().closed_host_prefix().is_none());
    }
}

#[test]
fn independently_coherent_target_rows_cannot_mix_distinct_exact_creation_starts() {
    let original = TargetFrameBasis::new(
        point(2, 100),
        TargetTime::new(0, 1, 4_000_000_000).unwrap(),
        4_000_000_000,
    )
    .unwrap();
    let changed = TargetFrameBasis::new(
        point(2, 100),
        TargetTime::new(0, 1, 3_000_000_000).unwrap(),
        4_000_000_000,
    )
    .unwrap();
    assert_eq!(
        original.point_at_stream_frame(0).unwrap(),
        changed.point_at_stream_frame(0).unwrap()
    );
    assert_ne!(original.start_time(), changed.start_time());
    let (mut port, mut p, mut bgm, mut producer) =
        rig(AudioAuthorityCoordinateBasis::TargetFrame(original), 100);
    port.rebind = Some(
        new_coordinate_audio_presentation(
            7,
            AudioAuthorityCoordinateBasis::TargetFrame(changed),
            ClockDomainId(1),
            point(3, 500),
            AudioAuthorityConfig::default(),
        )
        .unwrap(),
    );
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.observe_calls, 2);
    // The second row was genuinely admitted by its own matching native owners.
    assert_eq!(p.target_basis(), Some(changed));
    assert_eq!(p.authority().history_len(), 1);
    assert_eq!(p.latest_record().unwrap().pair().target, host(102));
    assert!(p.authority().closed_host_prefix().is_none());
}

#[test]
fn independently_coherent_new_epoch_cannot_be_combined_with_first_old_epoch_row() {
    let descriptor = descriptors()[2];
    let (mut port, mut p, mut bgm, mut producer) = rig(descriptor, 100);
    port.rebind = Some(
        new_coordinate_audio_presentation(
            8,
            descriptor,
            ClockDomainId(1),
            point(3, 500),
            AudioAuthorityConfig::default(),
        )
        .unwrap(),
    );
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.observe_calls, 2);
    assert_eq!(p.authority().epoch().id, 8);
    assert_eq!(p.authority().history_len(), 1);
    assert_eq!(p.latest_record().unwrap().pair().target, host(102));
    assert!(p.authority().closed_host_prefix().is_none());
}
