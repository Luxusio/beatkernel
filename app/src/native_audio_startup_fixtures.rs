//! Actual cold native owners, Mixer/BGM credits and one retained original input queue.
use crate::{
    audio_authority::AudioAuthorityConfig,
    bgm::{BgmConfig, BgmFeeder},
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_audio_startup::{
        NativeAudioSeedPort, SeededNativeAudio, new_audio_presentation, prime_native_audio,
    },
    native_gameplay::NativeGameplayResult,
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig,
        OutputFrameBasis, PcmLimits, PcmSample, RenderReport, SampleBank, SampleId, VoiceId,
        command_queue,
    },
    input::{
        BackendId, ButtonEvent, ButtonState, DeviceId, EventMeta, NativeEventMeta,
        PhysicalControlId, PhysicalInputEvent,
    },
    time::{ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence;
use std::{collections::VecDeque, fmt};
const H: i64 = 10_000_000_000;
const R: i64 = 1_000_000_000;
const L: i64 = 5_000_000_000;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, H + ns)
}
fn raw(ns: i64) -> ClockPoint {
    point(2, R + ns)
}
fn logical(ns: i64) -> ClockPoint {
    point(3, L + ns)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Future,
    Missing,
    TimeoutFuture,
    Stale,
    WrongHost,
    HostRegression,
    WrongEpoch,
    NativeRegression,
    InputFault,
    ObserveFault,
    RenderFault,
    WideHost,
    LargeQuantum,
}
#[derive(Debug)]
struct Fault(Box<u64>);
impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scripted native seed refusal {}", self.0)
    }
}
impl std::error::Error for Fault {}
struct Port {
    mixer: Mixer,
    basis: OutputFrameBasis,
    epoch: u64,
    mode: Mode,
    round: usize,
    service_calls: usize,
    observe_calls: usize,
    render_calls: usize,
    waits: usize,
    cancel_round: Option<usize>,
    report: Option<RenderReport>,
    retained: VecDeque<PhysicalInputEvent>,
    pcm: Vec<f32>,
    fault: Option<Box<Fault>>,
}
impl Port {
    fn receipt(&self) -> ClockPoint {
        if self.mode == Mode::WideHost {
            return point(1, if self.round == 0 { i64::MIN } else { i64::MAX });
        }
        if self.mode == Mode::LargeQuantum {
            return host(self.round as i64 * 2_000_000_000);
        }
        let delta = if self.mode == Mode::Stale && self.round >= 2 {
            25_000_000
        } else if self.mode == Mode::HostRegression && self.round >= 2 {
            4_000_000
        } else {
            self.round as i64 * 5_000_000
        };
        if self.mode == Mode::WrongHost {
            point(4, H + delta)
        } else {
            host(delta)
        }
    }
    fn error(&mut self) -> Box<dyn std::error::Error> {
        self.fault.take().expect("one injected native IO failure")
    }
}
impl NativeAudioSeedPort for Port {
    fn service_input(&mut self) -> NativeGameplayResult<bool> {
        self.service_calls += 1;
        self.round += 1;
        let mut pcm = [0.0; 10];
        let frames = if self.mode == Mode::LargeQuantum {
            4
        } else {
            10
        };
        self.report = Some(self.mixer.render(&mut pcm[..frames])?);
        self.pcm.extend_from_slice(&pcm[..frames]);
        if self.round <= 2 {
            let ns = self.round as i64 * 1_000_000;
            let mut meta = EventMeta::new(DeviceId(u64::MAX), host(ns), self.round as u64 - 1);
            meta.native = Some(NativeEventMeta {
                backend: BackendId(7),
                code: Some(4),
                timestamp: Some(host(ns)),
            });
            self.retained
                .push_back(PhysicalInputEvent::Button(ButtonEvent {
                    meta,
                    control: PhysicalControlId::keyboard(7u16),
                    state: if self.round == 1 {
                        ButtonState::Down
                    } else {
                        ButtonState::Up
                    },
                }));
        }
        if self.mode == Mode::InputFault && self.round == 2 {
            return Err(self.error());
        }
        Ok(self.cancel_round != Some(self.round))
    }
    fn observe_audio(&mut self, p: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.observe_calls += 1;
        if self.mode == Mode::ObserveFault && self.round == 2 {
            return Err(self.error());
        }
        if self.mode == Mode::Missing {
            return Ok(());
        }
        let target = if self.mode == Mode::LargeQuantum {
            self.round as i64 * 2_000_000_000
        } else if self.round == 2 && self.mode == Mode::Future {
            50_000_000
        } else if self.round == 2 && self.mode == Mode::TimeoutFuture {
            100_000_000
        } else {
            self.round as i64 * 5_000_000
        };
        let output = if self.mode == Mode::NativeRegression && self.round == 2 {
            raw(9_000_000)
        } else {
            self.basis
                .point_at_stream_frame(self.mixer.frame_cursor())?
        };
        p.admit(NativeAudioSnapshot {
            epoch: if self.mode == Mode::WrongEpoch && self.round == 2 {
                self.epoch + 1
            } else {
                self.epoch
            },
            basis: self.basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: output,
                target: host(target),
            }),
        })?;
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        self.render_calls += 1;
        if self.mode == Mode::RenderFault && self.round == 2 {
            return Err(self.error());
        }
        Ok(self.report)
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.receipt())
    }
    fn wait(&mut self, d: std::time::Duration) -> NativeGameplayResult<()> {
        assert_eq!(d, std::time::Duration::from_millis(1));
        self.waits += 1;
        Ok(())
    }
}
fn setup(mode: Mode) -> (Port, NativeAudioPresentation, BgmFeeder, CommandProducer) {
    let format = AudioFormat::new(if mode == Mode::LargeQuantum { 2 } else { 1000 }, 1).unwrap();
    let pcm = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 8], pcm).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(16).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            raw(0).timestamp,
            AudioLimits::new(16, 8, 16, 16, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let basis = mixer.output_frame_basis();
    let cfg = AudioAuthorityConfig {
        max_observation_age: Duration::from_nanos(if mode == Mode::Stale {
            1_000_000
        } else {
            1_000_000_000
        }),
        ..Default::default()
    };
    let presentation = new_audio_presentation(7, basis, ClockDomainId(1), logical(0), cfg).unwrap();
    let bgm = BgmFeeder::new(
        vec![
            AudioCommand::Play {
                sample: SampleId(1),
                voice: VoiceId(10),
                at: Timestamp::from_nanos(if mode == Mode::LargeQuantum {
                    2_500_000_000
                } else {
                    30_000_000
                }),
                gain: 1.0,
            },
            AudioCommand::Play {
                sample: SampleId(1),
                voice: VoiceId(11),
                at: Timestamp::from_nanos(if mode == Mode::LargeQuantum {
                    4_500_000_000
                } else {
                    60_000_000
                }),
                gain: 1.0,
            },
        ],
        BgmConfig {
            output_origin: raw(0),
            sample_rate: format.sample_rate(),
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(100_000_000),
            max_pending: 2,
        },
    )
    .unwrap();
    (
        Port {
            mixer,
            basis,
            epoch: 7,
            mode,
            round: 0,
            service_calls: 0,
            observe_calls: 0,
            render_calls: 0,
            waits: 0,
            cancel_round: None,
            report: None,
            retained: VecDeque::new(),
            pcm: vec![],
            fault: Some(Box::new(Fault(Box::new(900)))),
        },
        presentation,
        bgm,
        producer,
    )
}
fn assert_no_gameplay_commit(p: &NativeAudioPresentation) {
    assert_eq!(p.authority().acquired_prefix(), None);
    assert_eq!(p.authority().closed_host_prefix(), None);
    assert_eq!(p.authority().committed_input_host(), None);
    assert_eq!(p.authority().committed_operation(), None);
    assert_eq!(p.authority().committed_presentation(), None);
}
fn assert_originals(port: &Port) {
    assert_eq!(port.retained.len(), 2);
    for (i, event) in port.retained.iter().enumerate() {
        assert_eq!(event.meta().source, DeviceId(u64::MAX));
        assert_eq!(event.meta().clock_domain, ClockDomainId(1));
        assert_eq!(
            event.meta().timestamp,
            host((i as i64 + 1) * 1_000_000).timestamp
        );
        assert_eq!(event.meta().sequence, i as u64);
        assert_eq!(event.meta().original_clock_point, None);
        assert_eq!(
            event.meta().native.unwrap().timestamp,
            Some(host((i as i64 + 1) * 1_000_000))
        );
    }
}

#[test]
fn actual_two_anchor_seed_preserves_originals_and_maps_normal_logical_selected_origin() {
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::Normal);
    let seed = prime_native_audio(
        &mut port,
        &mut p,
        &mut bgm,
        &mut producer,
        Duration::from_nanos(100_000_000),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        seed.observations,
        [
            ClockPair {
                source: raw(10_000_000),
                target: host(5_000_000)
            },
            ClockPair {
                source: raw(20_000_000),
                target: host(10_000_000)
            }
        ]
    );
    assert_eq!(seed.now, host(10_000_000));
    assert_eq!(port.observe_calls, 2);
    assert_eq!(p.authority().history_len(), 2);
    assert_originals(&port);
    assert_no_gameplay_commit(&p);
    assert_eq!(
        p.logical_output(raw(15_000_000)).unwrap(),
        logical(15_000_000)
    );
    let transport = Transport::new(
        p.logical_output(raw(15_000_000)).unwrap().timestamp,
        Timestamp::ZERO,
        Rate::NORMAL,
    );
    assert_eq!(
        transport
            .position_at(logical(17_000_000).timestamp)
            .unwrap(),
        Timestamp::from_nanos(2_000_000)
    );
    assert_eq!(transport.anchor().rate, Rate::NORMAL);
    p.authority_mut()
        .record_acquired_prefix(host(10_000_000))
        .unwrap();
    let mapped = p
        .authority()
        .prepare_input(host(7_500_000), host(10_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(mapped.output(), logical(15_000_000));
    assert_eq!(mapped.mapper().uncertainty(), None);
    assert_eq!(
        beatkernel::time::ClockMapper::quality(mapped.mapper()),
        ClockMappingQuality::Unknown
    );
}

#[test]
fn fixed_future_second_pair_waits_without_more_admission_while_render_input_and_bgm_continue() {
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::Future);
    let seed = prime_native_audio(
        &mut port,
        &mut p,
        &mut bgm,
        &mut producer,
        Duration::from_nanos(100_000_000),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        seed.observations[0],
        ClockPair {
            source: raw(10_000_000),
            target: host(5_000_000)
        }
    );
    assert_eq!(
        seed.observations[1],
        ClockPair {
            source: raw(20_000_000),
            target: host(50_000_000)
        }
    );
    assert_eq!(seed.now, host(50_000_000));
    assert_eq!(port.observe_calls, 2);
    assert_eq!(port.service_calls, 10);
    assert_eq!(port.render_calls, 10);
    assert_eq!(port.waits, 9);
    assert_eq!(port.mixer.frame_cursor(), 100);
    assert_eq!(port.report.unwrap().counters.commands_applied, 2);
    assert_eq!(bgm.report().total_admitted, 2);
    assert!(port.pcm.iter().any(|v| *v == 0.25));
    assert_eq!(p.authority().history_len(), 2);
    assert_originals(&port);
    assert_no_gameplay_commit(&p);
}

#[test]
fn cancellation_and_timeout_retain_genuine_partial_native_and_input_state() {
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::Future);
    port.cancel_round = Some(3);
    assert!(
        prime_native_audio(
            &mut port,
            &mut p,
            &mut bgm,
            &mut producer,
            Duration::from_nanos(100_000_000)
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(port.observe_calls, 2);
    assert_eq!(p.authority().history_len(), 2);
    assert_originals(&port);
    assert_no_gameplay_commit(&p);
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::TimeoutFuture);
    assert!(
        prime_native_audio(
            &mut port,
            &mut p,
            &mut bgm,
            &mut producer,
            Duration::from_nanos(20_000_000)
        )
        .is_err()
    );
    assert_eq!(port.observe_calls, 2);
    assert_eq!(p.authority().history_len(), 2);
    assert_originals(&port);
    assert_no_gameplay_commit(&p);
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::Missing);
    assert!(
        prime_native_audio(
            &mut port,
            &mut p,
            &mut bgm,
            &mut producer,
            Duration::from_nanos(15_000_000)
        )
        .is_err()
    );
    assert_eq!(p.authority().history_len(), 0);
    assert!(p.latest_record().is_none());
    assert_originals(&port);
    assert_no_gameplay_commit(&p);
}

#[test]
fn stale_wrong_domain_regressed_receipt_and_native_metadata_fail_without_gameplay_commit() {
    for (mode, accepted) in [
        (Mode::Stale, 2),
        (Mode::WrongHost, 0),
        (Mode::HostRegression, 2),
        (Mode::WrongEpoch, 1),
        (Mode::NativeRegression, 1),
    ] {
        let (mut port, mut p, mut bgm, mut producer) = setup(mode);
        assert!(
            prime_native_audio(
                &mut port,
                &mut p,
                &mut bgm,
                &mut producer,
                Duration::from_nanos(100_000_000)
            )
            .is_err()
        );
        assert_eq!(p.authority().history_len(), accepted);
        assert_no_gameplay_commit(&p);
        if accepted == 0 {
            assert_eq!(port.service_calls, 0);
            assert!(port.retained.is_empty());
        } else {
            assert_originals(&port);
        }
    }
}

#[test]
fn original_io_errors_keep_exact_error_payload_and_accepted_prefix() {
    for (mode, accepted) in [
        (Mode::InputFault, 1),
        (Mode::ObserveFault, 1),
        (Mode::RenderFault, 2),
    ] {
        let (mut port, mut p, mut bgm, mut producer) = setup(mode);
        let pointer = port.fault.as_ref().unwrap().0.as_ref() as *const u64;
        let error = prime_native_audio(
            &mut port,
            &mut p,
            &mut bgm,
            &mut producer,
            Duration::from_nanos(100_000_000),
        )
        .unwrap_err();
        let fault = error.downcast_ref::<Fault>().unwrap();
        assert_eq!(fault.0.as_ref() as *const u64, pointer);
        assert_eq!(p.authority().history_len(), accepted);
        assert_originals(&port);
        assert_no_gameplay_commit(&p);
    }
}

#[test]
fn software_startup_origin_projection_uses_original_unequal_rate_and_explicit_finite_before() {
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::Normal);
    let seed = prime_native_audio(
        &mut port,
        &mut p,
        &mut bgm,
        &mut producer,
        Duration::from_nanos(100_000_000),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        seed.host_for_output(raw(15_000_000), Duration::ZERO)
            .unwrap(),
        host(7_500_000)
    );
    assert_eq!(
        seed.host_for_output(raw(0), Duration::from_nanos(10_000_000))
            .unwrap(),
        host(0)
    );
    assert_eq!(
        seed.host_for_output(raw(5_000_000), Duration::from_nanos(10_000_000))
            .unwrap(),
        host(2_500_000)
    );
    assert!(
        seed.host_for_output(raw(-1), Duration::from_nanos(10_000_000))
            .is_err()
    );
    assert!(seed.host_for_output(raw(0), Duration::ZERO).is_err());
    assert!(
        seed.host_for_output(raw(21_000_000), Duration::from_nanos(100_000_000))
            .is_err()
    );
    assert!(
        seed.host_for_output(point(4, R + 15_000_000), Duration::ZERO)
            .is_err()
    );
    assert!(
        seed.host_for_output(raw(15_000_000), Duration::from_nanos(-1))
            .is_err()
    );
    let overflow = SeededNativeAudio {
        observations: [
            ClockPair {
                source: point(2, 10),
                target: point(1, i64::MIN),
            },
            ClockPair {
                source: point(2, 20),
                target: point(1, i64::MIN + 10),
            },
        ],
        now: point(1, i64::MIN + 10),
    };
    let error = overflow
        .host_for_output(point(2, 0), Duration::from_nanos(10))
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<beatkernel::time::CalibrationError>(),
        Some(&beatkernel::time::CalibrationError::Overflow)
    );
    assert_no_gameplay_commit(&p);
}

#[test]
fn factory_captures_actual_creation_frame_zero_and_refuses_invalid_clock_or_numeric_identity() {
    let (mut port, _, _, _producer) = setup(Mode::Normal);
    port.mixer.render(&mut [0.0; 4]).unwrap();
    let captured = port.mixer.output_frame_basis();
    assert_eq!(captured.start_physical_frame(), 4);
    let p = new_audio_presentation(
        9,
        captured,
        ClockDomainId(1),
        logical(0),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    assert_eq!(p.authority().epoch().stream_origin, raw(4_000_000));
    assert_eq!(
        p.logical_output(raw(7_000_000)).unwrap(),
        logical(3_000_000)
    );
    assert!(
        new_audio_presentation(
            9,
            captured,
            ClockDomainId(2),
            logical(0),
            AudioAuthorityConfig::default()
        )
        .is_err()
    );
    assert!(
        new_audio_presentation(
            9,
            captured,
            ClockDomainId(1),
            host(0),
            AudioAuthorityConfig::default()
        )
        .is_err()
    );
    assert!(p.logical_output(point(4, R)).is_err());
    let huge = new_audio_presentation(
        9,
        captured,
        ClockDomainId(1),
        point(3, i64::MAX),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    assert!(huge.logical_output(raw(4_000_001)).is_err());
    assert_no_gameplay_commit(&huge);
}

#[test]
fn invalid_timeout_and_preseeded_owner_refuse_without_resetting_retained_state() {
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::Normal);
    assert!(
        prime_native_audio(&mut port, &mut p, &mut bgm, &mut producer, Duration::ZERO).is_err()
    );
    assert_eq!(port.service_calls, 0);
    assert_eq!(p.authority().history_len(), 0);
    port.service_input().unwrap();
    port.observe_audio(&mut p).unwrap();
    let before = format!("{:?}", (p.authority(), p.latest_record()));
    let originals = port.retained.clone();
    assert!(
        prime_native_audio(
            &mut port,
            &mut p,
            &mut bgm,
            &mut producer,
            Duration::from_nanos(100_000_000)
        )
        .is_err()
    );
    assert_eq!(format!("{:?}", (p.authority(), p.latest_record())), before);
    assert_eq!(port.service_calls, 1);
    assert_eq!(port.retained, originals);
    assert_no_gameplay_commit(&p);
}

#[test]
fn full_signed_host_span_times_out_with_wide_arithmetic_without_resetting_first_observation() {
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::WideHost);
    assert!(
        prime_native_audio(
            &mut port,
            &mut p,
            &mut bgm,
            &mut producer,
            Duration::from_nanos(i64::MAX)
        )
        .is_err()
    );
    assert_eq!(port.service_calls, 1);
    assert_eq!(port.observe_calls, 1);
    assert_eq!(port.retained.len(), 1);
    assert_eq!(p.authority().history_len(), 1);
    assert_eq!(
        p.authority().latest_observation(),
        Some(ClockPair {
            source: raw(10_000_000),
            target: host(5_000_000)
        })
    );
    assert_no_gameplay_commit(&p);
}

#[test]
fn actual_large_callback_quantum_gets_enough_bounded_time_for_two_observations() {
    let budget = crate::native_start::native_calibration_timeout(4, 2).unwrap();
    // Four real frames at 2 Hz occupy two seconds. Two callback observations
    // cannot fit in the former strict two-second calibration limit.
    assert_eq!(budget, std::time::Duration::from_millis(8100));
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::LargeQuantum);
    assert!(
        prime_native_audio(
            &mut port,
            &mut p,
            &mut bgm,
            &mut producer,
            Duration::from_nanos(2_000_000_000)
        )
        .is_err()
    );
    assert_eq!(port.mixer.frame_cursor(), 4);
    assert_eq!(p.authority().history_len(), 1);
    assert_no_gameplay_commit(&p);
    let (mut port, mut p, mut bgm, mut producer) = setup(Mode::LargeQuantum);
    let seed = prime_native_audio(
        &mut port,
        &mut p,
        &mut bgm,
        &mut producer,
        Duration::from_nanos(i64::try_from(budget.as_nanos()).unwrap()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(seed.now, host(4_000_000_000));
    assert_eq!(
        seed.observations,
        [
            ClockPair {
                source: raw(2_000_000_000),
                target: host(2_000_000_000)
            },
            ClockPair {
                source: raw(4_000_000_000),
                target: host(4_000_000_000)
            }
        ]
    );
    assert_eq!(port.service_calls, 2);
    assert_eq!(port.report.unwrap().frames, 4);
    assert_eq!(port.mixer.frame_cursor(), 8);
    assert_eq!(port.report.unwrap().counters.commands_applied, 1);
    assert!(port.pcm.iter().any(|v| *v == 0.25));
    assert_originals(&port);
    assert_no_gameplay_commit(&p);
}

#[test]
fn native_calibration_bound_rejects_zero_and_stays_finite_for_entire_u32_frame_range() {
    assert!(crate::native_start::native_calibration_timeout(0, 48000).is_err());
    assert!(crate::native_start::native_calibration_timeout(128, 0).is_err());
    assert_eq!(
        crate::native_start::native_calibration_timeout(128, 48000).unwrap(),
        std::time::Duration::from_secs(2)
    );
    let maximum = crate::native_start::native_calibration_timeout(u32::MAX, 1).unwrap();
    assert!(maximum > std::time::Duration::from_secs(2 * u64::from(u32::MAX)));
    assert!(maximum.as_nanos() <= u128::from(u64::MAX));
    // With u32 frames and a positive rate, even the largest legal budget fits
    // u64 nanoseconds. There is no representable valid input to force overflow.
}

fn finite_mixer(end: u64) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    (
        producer,
        Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                raw(0).timestamp,
                AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
            )
            .with_playback_end_frame(end),
            bank,
            consumer,
        )
        .unwrap(),
    )
}
#[test]
fn zero_and_short_finite_startup_prime_preserve_original_boundary_for_normal_handoff_once() {
    let (_producer, mut mixer) = finite_mixer(0);
    let first = mixer.render(&mut [0.0; 4]).unwrap();
    assert_eq!(first.playback_end_physical_frame, Some(0));
    let mut end = crate::native_end::NativeEnd::new(raw(0), ClockDomainId(1), 1000, 0).unwrap();
    end.prime(
        Some(first),
        ClockPair {
            source: raw(0),
            target: host(5_000_000),
        },
    )
    .unwrap();
    let current = mixer.render(&mut [0.0; 2]).unwrap();
    let before = format!("{:?}", end);
    assert!(
        end.observe(
            Some(current),
            ClockPair {
                source: point(4, R + 4_000_000),
                target: host(6_000_000)
            }
        )
        .is_err()
    );
    assert_eq!(format!("{:?}", end), before);
    let mut invalid = current;
    invalid.playback_frames = 1;
    assert!(
        end.observe(
            Some(invalid),
            ClockPair {
                source: raw(4_000_000),
                target: host(6_000_000)
            }
        )
        .is_err()
    );
    assert_eq!(format!("{:?}", end), before);
    let boundary = end
        .observe(
            Some(current),
            ClockPair {
                source: raw(4_000_000),
                target: host(6_000_000),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(boundary.output, raw(0));
    assert_eq!(boundary.host, host(5_000_000));
    assert_eq!(boundary.physical_frame, 0);
    assert_eq!(boundary.playback_frame, 0);
    assert!(
        end.observe(
            None,
            ClockPair {
                source: raw(5_000_000),
                target: host(7_000_000)
            }
        )
        .unwrap()
        .is_none()
    );
    let (_producer, mut mixer) = finite_mixer(4);
    let mut end = crate::native_end::NativeEnd::new(raw(0), ClockDomainId(1), 1000, 4).unwrap();
    let first = mixer.render(&mut [0.0; 2]).unwrap();
    end.prime(
        Some(first),
        ClockPair {
            source: raw(0),
            target: host(1_000_000),
        },
    )
    .unwrap();
    let reached = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(reached.playback_end_physical_frame, Some(4));
    end.prime(
        Some(reached),
        ClockPair {
            source: raw(2_000_000),
            target: host(3_000_000),
        },
    )
    .unwrap();
    let silence = mixer.render(&mut [0.0; 2]).unwrap();
    end.prime(
        Some(silence),
        ClockPair {
            source: raw(4_000_000),
            target: host(5_000_000),
        },
    )
    .unwrap();
    let later = mixer.render(&mut [0.0; 2]).unwrap();
    let boundary = end
        .observe(
            Some(later),
            ClockPair {
                source: raw(6_000_000),
                target: host(7_000_000),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(boundary.host, host(5_000_000));
    assert_eq!(boundary.output, raw(4_000_000));
    assert_eq!(boundary.physical_frame, 4);
    assert!(
        end.observe(
            None,
            ClockPair {
                source: raw(7_000_000),
                target: host(8_000_000)
            }
        )
        .unwrap()
        .is_none()
    );
}
#[test]
fn asio_finite_prime_retains_assessed_upper_host_and_rejects_invalid_current_without_losing_it() {
    use beatkernel_platform::audio::asio::{AsioPresentationObservation, MultimediaHostInterval};
    let (_producer, mut mixer) = finite_mixer(4);
    let mut end = crate::native_end::NativeEnd::new(raw(0), ClockDomainId(1), 1000, 4).unwrap();
    let observation = |render, before, after| {
        AsioPresentationObservation::from_render(
            render,
            1000,
            MultimediaHostInterval {
                before: host(before),
                after: host(after),
            },
            0,
            0,
            raw(0),
        )
        .unwrap()
    };
    end.prime_asio(observation(
        mixer.render(&mut [0.0; 2]).unwrap(),
        100_000,
        200_000,
    ))
    .unwrap();
    end.prime_asio(observation(
        mixer.render(&mut [0.0; 2]).unwrap(),
        2_100_000,
        2_200_000,
    ))
    .unwrap();
    let actual = observation(mixer.render(&mut [0.0; 2]).unwrap(), 4_100_000, 4_900_000);
    end.prime_asio(actual).unwrap();
    let current = observation(mixer.render(&mut [0.0; 2]).unwrap(), 6_100_000, 6_900_000);
    let before = format!("{:?}", end);
    let mut wrong = current;
    wrong.host.after = point(4, H + 6_900_000);
    assert!(end.observe_asio(wrong).is_err());
    assert_eq!(format!("{:?}", end), before);
    let mut wrong = current;
    wrong.render.playback_frames = 1;
    assert!(end.observe_asio(wrong).is_err());
    assert_eq!(format!("{:?}", end), before);
    let boundary = end.observe_asio(current).unwrap().unwrap();
    assert_eq!(boundary.host, actual.host.after);
    assert_eq!(boundary.host, host(4_900_000));
    assert_ne!(boundary.host, host(4_500_000));
    assert_eq!(boundary.output, raw(4_000_000));
    assert_eq!(boundary.physical_frame, 4);
    assert!(
        end.observe_asio(observation(
            mixer.render(&mut [0.0; 2]).unwrap(),
            8_100_000,
            8_900_000
        ))
        .unwrap()
        .is_none()
    );
}
#[test]
fn first_actual_snapshot_past_endpoint_without_native_lower_refuses_atomically() {
    let (_producer, mut mixer) = finite_mixer(4);
    let actual = mixer.render(&mut [0.0; 6]).unwrap();
    assert_eq!(actual.playback_end_physical_frame, Some(4));
    let mut end = crate::native_end::NativeEnd::new(raw(0), ClockDomainId(1), 1000, 4).unwrap();
    let before = format!("{:?}", end);
    assert!(
        end.prime(
            Some(actual),
            ClockPair {
                source: raw(6_000_000),
                target: host(6_000_000)
            }
        )
        .is_err()
    );
    assert_eq!(format!("{:?}", end), before);
}
