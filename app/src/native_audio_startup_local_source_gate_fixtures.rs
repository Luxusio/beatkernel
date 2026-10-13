//! Real closed-gate mixer startup; native timestamps are supplied DI, not device evidence.
use super::*;
use crate::{
    bgm::BgmConfig,
    playback_pause::NativePause,
    native_end::NativeEnd,
    gameplay::output::ports::{
        NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
    },
    native_audio_presentation::NativeAudioSnapshot,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::audio::*;
use beatkernel::time::ClockMappingQuality;
use beatkernel_platform::audio::{
    AudioClockSnapshot, AudioClockReadingQuality, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters, presentation::validation::OriginalNativePresentationEvidence,
};

const HZ: u32 = 1000;
const COUNTER_HZ: u64 = 12_000_000_000;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
struct Port {
    mixer: Mixer,
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    counter: bool,
    counter_frequency: u64,
    round: u64,
    acquisitions: usize,
    waits: usize,
    frozen_host: bool,
    cancel: bool,
    missing: bool,
    wrong_epoch: bool,
    wrong_frequency: bool,
    pcm: Vec<(u64, f32)>,
    cancel_at: Option<u64>,
    stale: bool,
    prior: Option<CoordinateControlFacts>,
}
impl NativeAudioSeedPort for Port {
    fn service_input(&mut self) -> NativeGameplayResult<bool> {
        self.round += 1;
        Ok(!self.cancel && self.cancel_at != Some(self.round))
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        panic!("no scalar observation")
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        panic!("no separate render cache")
    }
    fn observe_coordinate_audio(
        &mut self,
        p: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        self.acquisitions += 1;
        if self.missing {
            return Ok(None);
        }
        assert_eq!(identity, self.identity);
        let mut pcm = [99.0];
        let report = self.mixer.render(&mut pcm)?;
        self.pcm.push((report.start_frame, pcm[0]));
        let frame = report.start_frame + report.frames as u64;
        let native_host = point(1, (frame as i64) * 1_000_000);
        let frequency = if self.wrong_frequency {
            self.counter_frequency + 1
        } else {
            self.counter_frequency
        };
        let position = u64::try_from(
            u128::from(frame - self.basis.start_physical_frame())
                * u128::from(self.counter_frequency)
                / u128::from(HZ),
        )?;
        let query = if self.counter {
            InterpolatedTargetTime::new(
                TargetTime::from_frames(self.basis.start_physical_frame(), HZ)?,
                position,
                position,
                frequency,
                0,
                1,
            )?
        } else {
            InterpolatedTargetTime::new(TargetTime::from_frames(frame, HZ)?, 0, 0, 1, 0, 1)?
        };
        let coordinate = if self.counter {
            NativeOutputCoordinate::Counter {
                position,
                frequency,
            }
        } else {
            NativeOutputCoordinate::StreamFrame {
                frame: frame - self.basis.start_physical_frame(),
            }
        };
        let evidence = if self.counter {
            OriginalNativePresentationEvidence::Wasapi {
                basis: Some(self.basis),
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters {
                        submitted_frames: frame - self.basis.start_physical_frame(),
                        ..StreamCounters::default()
                    },
                    render: Some(report),
                    clock: Some(AudioClockSnapshot {
                        position,
                        frequency,
                        qpc_100ns: frame,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(native_host),
                        mapping_quality: ClockMappingQuality::Unknown,
                    }),
                },
            }
        } else {
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: self
                    .basis
                    .point_at_stream_frame(frame - self.basis.start_physical_frame())?,
                target: native_host,
            })
        };
        let epoch = if self.wrong_epoch { 8 } else { 7 };
        let native = CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch,
                basis: self.basis,
                evidence,
            },
            coordinate,
        )?;
        let history = self.mixer.source_playback_history().unwrap();
        let playback = history
            .lookup_at_source_time(query)
            .map_err(|error| format!("real Source history: {error:?}"))?;
        let packet = NativeSourcePlaybackQuery::new(
            epoch,
            self.basis,
            identity,
            query,
            SourcePlaybackFacts {
                playback,
                pulled_source_end: history.pulled_end_frame(),
                submitted_source_end: history.pulled_end_frame(),
                render: Some(report),
                revision: 2 * self.acquisitions as u64,
            },
            Some(evidence),
        )?;
        let observation = NativePlaybackObservation::new(native, packet)?;
        p.admit_coordinate(native)?;
        let facts = CoordinateControlFacts::from_source(&observation, p, identity)?;
        if self.stale && self.acquisitions == 2 {
            return Ok(self.prior);
        }
        self.prior = Some(facts);
        Ok(Some(facts))
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(point(
            1,
            if self.frozen_host {
                0
            } else {
                (self.round + self.basis.start_physical_frame()) as i64 * 1_000_000
            },
        ))
    }
    fn wait(&mut self, duration: std::time::Duration) -> NativeGameplayResult<()> {
        assert_eq!(duration, std::time::Duration::from_millis(1));
        self.waits += 1;
        Ok(())
    }
}
struct Rig {
    port: Port,
    p: NativeAudioPresentation,
    bgm: BgmFeeder,
    producer: CommandProducer,
    pause: NativePause,
    end: Option<NativeEnd>,
}
fn rig(counter: bool, gated: bool) -> Rig {
    rig_at(counter, gated, 0)
}
fn rig_at(counter: bool, gated: bool, initial_physical: usize) -> Rig {
    rig_with_counter_frequency(counter, gated, initial_physical, COUNTER_HZ)
}
fn rig_with_counter_frequency(
    counter: bool,
    gated: bool,
    initial_physical: usize,
    counter_frequency: u64,
) -> Rig {
    let format = AudioFormat::new(HZ, 1).unwrap();
    let limits = PcmLimits::new(512, 512, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 128], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = if gated {
        command_queue_with_start_gate(32).unwrap()
    } else {
        command_queue(32).unwrap()
    };
    producer
        .try_push(AudioCommand::Play {
            at: Timestamp::ZERO,
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(32, 2, 8, 64, 8).unwrap(),
        )
        .with_playback_end_frame(64),
        bank,
        consumer,
    )
    .unwrap();
    if initial_physical != 0 {
        let mut cold = vec![99.0; initial_physical];
        let report = mixer.render(&mut cold).unwrap();
        assert_eq!(report.playback_frames, 0);
        assert!(cold.iter().all(|value| *value == 0.0));
    }
    let identity = prepare_source_coordinate_history(&mut mixer, 128).unwrap();
    let basis = mixer.output_frame_basis();
    let descriptor = if counter {
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: counter_frequency,
        }
    } else {
        AudioAuthorityCoordinateBasis::SourceFrame(basis)
    };
    let p = new_coordinate_audio_presentation(
        7,
        descriptor,
        ClockDomainId(1),
        point(3, 0),
        AudioAuthorityConfig {
            history_capacity: 128,
            max_observation_age: Duration::from_nanos(50_000_000),
            ..AudioAuthorityConfig::default()
        },
    )
    .unwrap();
    let bgm = BgmFeeder::new(
        vec![],
        BgmConfig {
            output_origin: basis.origin(),
            sample_rate: HZ,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(1_000_000),
            max_pending: 8,
        },
    )
    .unwrap();
    Rig {
        port: Port {
            mixer,
            basis,
            identity,
            counter,
            counter_frequency,
            round: 0,
            acquisitions: 0,
            waits: 0,
            frozen_host: false,
            cancel: false,
            missing: false,
            wrong_epoch: false,
            wrong_frequency: false,
            pcm: vec![],
            cancel_at: None,
            stale: false,
            prior: None,
        },
        p,
        bgm,
        producer,
        pause: NativePause::new(basis.origin(), ClockDomainId(1), HZ)
            .unwrap()
            .with_playback_end_frame(64)
            .unwrap(),
        end: Some(NativeEnd::new(basis.origin(), ClockDomainId(1), HZ, 64).unwrap()),
    }
}
fn start(
    r: &mut Rig,
    buffer: u32,
    timeout: i64,
) -> NativeGameplayResult<Option<StartedSourceCoordinateNativeAudio>> {
    let identity = r.port.identity;
    start_local_source_coordinate_native_audio(
        &mut r.port,
        &mut r.p,
        identity,
        &mut r.bgm,
        &mut r.producer,
        &mut r.pause,
        &mut r.end,
        buffer,
        Duration::from_nanos(timeout),
    )
}
#[test]
fn closed_gate_source_frame_and_u64_counter_start_at_first_real_positive_pcm() {
    for counter in [false, true] {
        let mut r = rig(counter, true);
        let started = start(&mut r, 2, 1_000_000_000).unwrap().unwrap();
        let gate = started.source_frame;
        assert!(gate > 0);
        assert_eq!(r.producer.applied_start_frame(), Some(gate));
        assert_eq!(r.producer.start_gate_frame(), Some(Some(gate)));
        assert!(r
            .port
            .pcm
            .iter()
            .any(|(frame, value)| *frame < gate && *value == 0.0));
        assert!(r
            .port
            .pcm
            .iter()
            .filter(|(frame, _)| *frame < gate)
            .all(|(_, value)| *value == 0.0));
        assert_eq!(
            r.port.pcm.iter().find(|(_, value)| *value > 0.0).copied(),
            Some((gate, 0.25))
        );
        let query =
            InterpolatedTargetTime::new(TargetTime::from_frames(gate, HZ).unwrap(), 0, 0, 1, 0, 1)
                .unwrap();
        assert!(started.query.compare_interpolated_time(query).is_eq());
        assert_eq!(started.host_origin, point(1, gate as i64 * 1_000_000));
        assert!(started
            .seeded
            .host_for_query(&query, Duration::ZERO)
            .unwrap()
            .is_some());
        assert!(r.pause.clone().with_start_frame(gate).is_err());
        assert!(r
            .pause
            .clone()
            .with_coordinate_binding(7, started.seeded.basis(), r.port.identity)
            .is_ok());
        assert!(r
            .end
            .as_ref()
            .unwrap()
            .clone()
            .with_start_frame(gate)
            .is_err());
        assert!(r
            .end
            .as_ref()
            .unwrap()
            .clone()
            .with_coordinates(7, started.seeded.basis(), r.port.identity)
            .is_ok());
        assert!(r.p.authority().acquired_prefix().is_none());
        assert!(r.p.authority().closed_host_prefix().is_none());
        if counter {
            assert!(matches!(
                started.seeded.basis(),
                AudioAuthorityCoordinateBasis::SourceCounter {
                    frequency: COUNTER_HZ,
                    ..
                }
            ));
        }
    }
}
#[test]
fn invalid_gate_buffer_timeout_refuse_before_any_compound_acquisition() {
    for mode in 0..4 {
        let mut r = rig(false, mode != 0);
        if mode == 1 {
            r.producer.schedule_start_at(100).unwrap();
        }
        let gate_before = r.producer.start_gate_frame();
        let queue_before = r.producer.counters();
        assert!(start(
            &mut r,
            if mode == 2 { 0 } else { 2 },
            if mode == 3 { 0 } else { 50_000_000 }
        )
        .is_err());
        assert_eq!(r.port.round, 0);
        assert_eq!(r.port.acquisitions, 0);
        assert_eq!(r.port.waits, 0);
        assert_eq!(r.producer.start_gate_frame(), gate_before);
        assert_eq!(r.producer.counters(), queue_before);
        assert_eq!(r.producer.applied_start_frame(), None);
        assert_eq!(r.bgm.report().total_admitted, 0);
    }
}
#[test]
fn cancellation_preserves_closed_gate_and_never_acquires() {
    let mut r = rig(true, true);
    r.port.cancel = true;
    assert!(start(&mut r, 2, 50_000_000).unwrap().is_none());
    assert_eq!(r.port.acquisitions, 0);
    assert_eq!(r.producer.start_gate_frame(), Some(None));
    assert_eq!(r.producer.applied_start_frame(), None);
    assert_eq!(r.p.authority().history_len(), 0);
}
#[test]
fn unavailable_publications_timeout_with_gate_still_closed() {
    let mut r = rig(true, true);
    r.port.missing = true;
    assert!(start(&mut r, 2, 3_000_000).is_err());
    assert!(r.port.acquisitions > 0);
    assert_eq!(r.producer.start_gate_frame(), Some(None));
    assert_eq!(r.producer.applied_start_frame(), None);
    assert_eq!(r.p.authority().history_len(), 0);
}
#[test]
fn changed_original_epoch_or_u64_frequency_never_arms_gate() {
    for frequency in [false, true] {
        let mut r = rig(true, true);
        r.port.wrong_frequency = frequency;
        r.port.wrong_epoch = !frequency;
        assert!(start(&mut r, 2, 50_000_000).is_err());
        assert_eq!(r.port.acquisitions, 1);
        assert_eq!(r.producer.start_gate_frame(), Some(None));
        assert_eq!(r.producer.applied_start_frame(), None);
        assert!(r.port.pcm.iter().all(|(_, value)| *value == 0.0));
    }
}

#[test]
fn nonzero_cold_creation_keeps_original_history_and_native_counter_offset() {
    let mut r = rig_at(true, true, 5);
    let identity = r.port.identity;
    let basis = r.port.basis;
    assert_eq!(identity.initial_physical(), 5);
    assert_eq!(identity.initial_playback(), 0);
    let started = start(&mut r, 2, 1_000_000_000).unwrap().unwrap();
    assert!(started.source_frame > 5);
    assert_eq!(r.port.identity, identity);
    assert_eq!(r.port.basis, basis);
    let history = r.port.mixer.source_playback_history().unwrap();
    assert_eq!(history.initial_physical_frame(), 5);
    assert_eq!(history.initial_playback_frame(), 0);
    assert_eq!(
        started.host_origin,
        point(1, started.source_frame as i64 * 1_000_000)
    );
    assert_eq!(
        r.port.pcm.iter().find(|(_, value)| *value > 0.0).copied(),
        Some((started.source_frame, 0.25))
    );
    assert_eq!(r.producer.applied_start_frame(), Some(started.source_frame));
}

#[test]
fn stale_compound_facts_never_admit_a_gate() {
    let mut r = rig(false, true);
    r.port.stale = true;
    assert!(start(&mut r, 2, 50_000_000).is_err());
    assert_eq!(r.port.acquisitions, 2);
    assert_eq!(r.producer.start_gate_frame(), Some(None));
    assert_eq!(r.producer.applied_start_frame(), None);
}
#[test]
fn cancellation_or_timeout_after_arming_never_claims_positive_playback() {
    for cancel in [false, true] {
        let mut r = rig(true, true);
        if cancel {
            r.port.cancel_at = Some(5);
        }
        let result = start(&mut r, 2, if cancel { 50_000_000 } else { 5_000_000 });
        if cancel {
            assert!(result.unwrap().is_none())
        } else {
            assert!(result.is_err())
        }
        assert!(r.producer.start_gate_frame().flatten().is_some());
        assert_eq!(r.producer.applied_start_frame(), None);
        assert!(r.port.pcm.iter().all(|(_, value)| *value == 0.0));
        assert!(r.p.authority().acquired_prefix().is_none());
        assert!(r.p.authority().closed_host_prefix().is_none());
    }
}

#[test]
fn frozen_host_with_missing_publications_cannot_wait_forever_or_arm_the_gate() {
    let mut r = rig(true, true);
    r.port.frozen_host = true;
    r.port.missing = true;
    let error = start(&mut r, 2, 2_000_000).unwrap_err();
    assert!(error.to_string().contains("watchdog"));
    assert!(r.port.acquisitions > 0);
    assert!(r.port.waits > 0);
    assert_eq!(r.producer.start_gate_frame(), Some(None));
    assert_eq!(r.producer.applied_start_frame(), None);
    assert_eq!(r.p.authority().history_len(), 0);
    assert!(r.port.pcm.is_empty());
}

#[test]
fn nondivisible_u64_counter_frequency_keeps_exact_start_cut_and_ceil_host() {
    let mut r = rig_with_counter_frequency(true, true, 0, COUNTER_HZ + 1);
    let started = start(&mut r, 2, 1_000_000_000).unwrap().unwrap();
    assert_eq!(r.producer.applied_start_frame(), Some(started.source_frame));
    assert_eq!(
        r.port.pcm.iter().find(|(_, value)| *value > 0.0).copied(),
        Some((started.source_frame, 0.25))
    );
    // Counter observations fall just below each integral Source frame. The
    // first original HOST reaching the exact Source cut is therefore the next ns.
    assert_eq!(
        started.host_origin,
        point(
            1,
            i64::try_from(started.source_frame).unwrap() * 1_000_000 + 1
        )
    );
    assert_eq!(
        started
            .seeded
            .host_for_query(&started.query, Duration::ZERO)
            .unwrap(),
        Some(started.host_origin)
    );
    assert_eq!(
        started.seeded.basis(),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis: r.port.basis,
            frequency: COUNTER_HZ + 1,
        }
    );
}
