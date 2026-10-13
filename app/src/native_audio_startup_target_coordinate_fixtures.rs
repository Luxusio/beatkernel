//! Actual unequal-rate histories and compound startup packets; native evidence is DI.
use super::*;
use crate::{
    bgm::BgmConfig,
    gameplay::output::ports::{
        NativeTargetPlaybackFacts, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
    },
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_coordinate_control::{CoordinatePlaybackControlFacts, TargetCoordinateControlFacts},
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::ConvertedPlaybackFacts,
};
use beatkernel::audio::*;
use beatkernel_platform::audio::{
    ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
    presentation::validation::OriginalNativePresentationEvidence,
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
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}

struct Port {
    owner: ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    identity: SourcePlaybackIdentity,
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
    stale: bool,
    advance_without_packet: bool,
    prior: Option<CoordinatePlaybackControlFacts>,
    reports: Vec<ConvertedRenderReport>,
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
        panic!("compound Target startup must not acquire scalar evidence")
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        panic!("compound Target startup must not read an independent render cache")
    }
    fn observe_coordinate_audio(
        &mut self,
        _: &mut NativeAudioPresentation,
        _: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        panic!("Target startup must not acquire Source-only evidence")
    }
    fn observe_coordinate_playback_audio(
        &mut self,
        p: &mut NativeAudioPresentation,
        _: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinatePlaybackControlFacts>> {
        self.acquisitions += 1;
        if self.fault == 2 {
            return Err("compound acquisition failure".into());
        }
        if self.missing && !self.advance_without_packet {
            return Ok(None);
        }
        let report = self.owner.render_pending(7)?;
        self.owner.admit(7)?;
        self.reports.push(report);
        let frame = self.round as u64;
        let query = exact(self.basis.time_at_stream_frame(frame)?);
        let history = self.owner.phase_history().unwrap();
        let mut selected = None;
        for i in 0..history.len() {
            let phase = history.span(i).unwrap();
            let words = phase.to_words();
            let start = TargetTime::new(words[4], words[5], words[6])?;
            let end = history
                .span(i + 1)
                .map(|next| {
                    let words = next.to_words();
                    TargetTime::new(words[4], words[5], words[6]).unwrap()
                })
                .unwrap_or(history.end_time());
            if !query.compare_target_time(start).is_lt()
                && !query.compare_target_time(end).is_gt()
                && !(i + 1 < history.len() && query.compare_target_time(end).is_eq())
            {
                selected = Some((phase, end));
            }
        }
        let (phase, segment_end) = selected.ok_or("query outside actual phase history")?;
        let source = self
            .owner
            .converter_owner()
            .source_playback_history()
            .unwrap();
        let playback = (0..source.len())
            .filter_map(|i| source.span(i))
            .find(|span| {
                phase
                    .compare_source_frame_at_interpolated_time(
                        query,
                        segment_end,
                        span.physical_start(),
                    )
                    .is_ok_and(|order| !order.is_lt())
                    && span.physical_end().is_none_or(|end| {
                        phase
                            .compare_source_frame_at_interpolated_time(query, segment_end, end)
                            .is_ok_and(|order| order.is_lt())
                    })
            })
            .ok_or("query outside actual Source history")?;
        let at = if self.round == 1 {
            100
        } else {
            self.second_host + self.round as i64 - 2
        };
        let evidence = OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: self.basis.point_at_stream_frame(frame)?,
            target: host(at),
        });
        let coordinate = CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
                epoch: 7,
                basis: self.basis,
                evidence,
            },
            NativeOutputCoordinate::StreamFrame { frame },
        )?;
        let packet = NativeTargetPlaybackQuery::new(
            7,
            self.basis,
            self.identity,
            query,
            NativeTargetPlaybackFacts {
                playback: ConvertedPlaybackFacts {
                    phase,
                    segment_end,
                    generated_target_end: history.end_time(),
                    submitted_target_end: self
                        .owner
                        .admitted_target_time()
                        .ok_or("actual target admission missing")?,
                    playback,
                    pulled_source_end: source.pulled_end_frame(),
                    revision: self.acquisitions as u64 * 2,
                },
                source: if self.missing_report {
                    None
                } else {
                    self.owner.last_real_source_report()
                },
                converted: Some(report),
                boundaries: self.owner.boundaries(),
                revision: self.acquisitions as u64 * 4,
            },
            Some(evidence),
        )?;
        let observation = NativeTargetPlaybackObservation::new(coordinate, packet)?;
        p.admit_target_coordinate(coordinate)?;
        let facts = CoordinatePlaybackControlFacts::Target(
            TargetCoordinateControlFacts::from_target(&observation, p, self.identity)?,
        );
        if self.missing {
            return Ok(None);
        }
        if self.stale && self.round == 2 {
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
fn rig(finite: bool) -> (Port, NativeAudioPresentation, BgmFeeder, CommandProducer) {
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
    mixer.render(&mut [0.0; 1]).unwrap();
    mixer.enable_source_playback_history(16).unwrap();
    let history = mixer.source_playback_history().unwrap();
    let identity = SourcePlaybackIdentity::new(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    mixer.render(&mut [0.0; 1]).unwrap();
    let mut owner = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(7, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("valid unequal-rate owner"));
    owner.enable_phase_history(16).unwrap();
    let basis = owner.target_frame_basis();
    assert_eq!(identity, SourcePlaybackIdentity::new(3, 1, 1).unwrap());
    assert_eq!(basis.start_time(), TargetTime::from_frames(2, 3).unwrap());
    let p = new_coordinate_audio_presentation(
        7,
        AudioAuthorityCoordinateBasis::TargetFrame(basis),
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
            at: Timestamp::from_nanos(3_000_000_000),
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
            owner,
            basis,
            identity,
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
            stale: false,
            advance_without_packet: false,
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
    timeout: i64,
) -> NativeGameplayResult<Option<SeededCoordinateNativeAudio>> {
    let identity = port.identity;
    prime_target_coordinate_native_audio(
        port,
        p,
        identity,
        bgm,
        producer,
        Duration::from_nanos(timeout),
    )
}

#[test]
fn actual_unequal_rate_target_seed_preserves_two_originals_without_native_ack() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false);
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    assert_eq!(
        seed.basis(),
        AudioAuthorityCoordinateBasis::TargetFrame(port.basis)
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
        port.prior.unwrap().report(),
        port.owner.last_real_source_report()
    );
    assert_eq!(p.authority().acquired_prefix(), None);
    assert_eq!(p.authority().closed_host_prefix(), None);
}
#[test]
fn exact_fraction_host_inverse_uses_original_target_grid_and_finite_bounds() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false);
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    let midpoint = InterpolatedTargetTime::new(port.basis.start_time(), 1, 2, 7, 1, 2).unwrap();
    assert_eq!(
        seed.host_for_query(&midpoint, Duration::ZERO).unwrap(),
        Some(host(101))
    );
    assert_eq!(
        seed.host_for_query(
            &exact(port.basis.time_at_stream_frame(2).unwrap()),
            Duration::ZERO
        )
        .unwrap(),
        Some(host(102))
    );
    assert_eq!(
        seed.host_for_query(&exact(port.basis.start_time()), Duration::ZERO)
            .unwrap(),
        None
    );
    assert_eq!(
        seed.host_for_query(&exact(port.basis.start_time()), Duration::from_nanos(2))
            .unwrap(),
        Some(host(98))
    );
    assert_eq!(
        seed.host_for_query(
            &exact(port.basis.time_at_stream_frame(3).unwrap()),
            Duration::from_nanos(2)
        )
        .unwrap(),
        None
    );
    assert!(seed
        .host_for_query(&midpoint, Duration::from_nanos(-1))
        .is_err());
}
#[test]
fn future_second_original_is_not_replaced_while_actual_compound_reports_progress() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false);
    port.second_host = 105;
    let seed = prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    assert_eq!(seed.now(), host(105));
    assert_eq!(seed.observations()[0].pair().target, host(100));
    assert_eq!(seed.observations()[1].pair().target, host(105));
    assert_eq!(
        seed.observations()[1].coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 2 })
    );
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
        port.prior.unwrap().report(),
        port.owner.last_real_source_report()
    );
}
#[test]
fn bgm_uses_only_report_in_the_same_target_packet_and_preserves_none() {
    for missing_report in [false, true] {
        let (mut port, mut p, mut bgm, mut producer) = rig(false);
        port.missing_report = missing_report;
        prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
            .unwrap()
            .unwrap();
        assert_eq!(bgm.report().total_admitted, usize::from(!missing_report));
        assert_eq!(port.prior.unwrap().report().is_none(), missing_report);
    }
}
#[test]
fn unavailable_packets_do_not_invent_report_or_observation() {
    for advanced in [false, true] {
        let (mut port, mut p, mut bgm, mut producer) = rig(false);
        port.missing = true;
        port.advance_without_packet = advanced;
        assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 3).is_err());
        assert_eq!(port.acquisitions, 3);
        assert_eq!(bgm.report().total_admitted, 0);
        assert_eq!(p.latest_record().is_some(), advanced);
        assert!(p.authority().closed_host_prefix().is_none());
    }
}
#[test]
fn stale_target_facts_refuse_before_later_report_can_feed_bgm() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false);
    port.stale = true;
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.acquisitions, 2);
    assert_eq!(bgm.report().total_admitted, 0);
    assert!(p.authority().closed_host_prefix().is_none());
}
#[test]
fn mismatched_original_source_identity_refuses_without_bgm_or_ack() {
    for identity in [
        SourcePlaybackIdentity::new(4, 1, 1).unwrap(),
        SourcePlaybackIdentity::new(3, 2, 1).unwrap(),
        SourcePlaybackIdentity::new(3, 1, 0).unwrap(),
    ] {
        let (mut port, mut p, mut bgm, mut producer) = rig(false);
        assert!(prime_target_coordinate_native_audio(
            &mut port,
            &mut p,
            identity,
            &mut bgm,
            &mut producer,
            Duration::from_nanos(100)
        )
        .is_err());
        assert_eq!(bgm.report().total_admitted, 0);
        assert!(p.authority().closed_host_prefix().is_none());
    }
}
#[test]
fn input_cancellation_does_not_acquire_or_generate_any_target_audio() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false);
    port.cancel = true;
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .is_none());
    assert_eq!(port.round, 1);
    assert_eq!(port.acquisitions, 0);
    assert!(port.reports.is_empty());
    assert_eq!(bgm.report().total_admitted, 0);
}
#[test]
fn input_acquisition_wait_and_host_failures_propagate_without_ack() {
    for fault in 1..=5 {
        let (mut port, mut p, mut bgm, mut producer) = rig(false);
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
fn host_regression_and_expired_original_second_observation_refuse_readiness() {
    for regression in [false, true] {
        let (mut port, mut p, mut bgm, mut producer) = rig(false);
        if regression {
            port.regress = true;
        } else {
            port.host_base = 110;
            p = new_coordinate_audio_presentation(
                7,
                AudioAuthorityCoordinateBasis::TargetFrame(port.basis),
                ClockDomainId(1),
                point(3, 500),
                AudioAuthorityConfig {
                    max_observation_age: Duration::from_nanos(1),
                    ..AudioAuthorityConfig::default()
                },
            )
            .unwrap();
        }
        assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
        assert!(p.authority().acquired_prefix().is_none());
        assert!(p.authority().closed_host_prefix().is_none());
    }
}
#[test]
fn nonpositive_timeout_refuses_before_input_or_audio_io() {
    for timeout in [0, -1] {
        let (mut port, mut p, mut bgm, mut producer) = rig(false);
        assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, timeout).is_err());
        assert_eq!(port.round, 0);
        assert_eq!(port.acquisitions, 0);
    }
}
#[test]
fn source_and_scalar_descriptors_refuse_before_io_without_legacy_fallback() {
    for scalar in [false, true] {
        let (mut port, _, mut bgm, mut producer) = rig(false);
        let basis = port.owner.mixer().output_frame_basis();
        let mut p = if scalar {
            new_audio_presentation(
                7,
                basis,
                ClockDomainId(1),
                point(3, 500),
                AudioAuthorityConfig::default(),
            )
            .unwrap()
        } else {
            new_coordinate_audio_presentation(
                7,
                AudioAuthorityCoordinateBasis::SourceFrame(basis),
                ClockDomainId(1),
                point(3, 500),
                AudioAuthorityConfig::default(),
            )
            .unwrap()
        };
        let before = format!("{p:?}");
        assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
        assert_eq!(port.round, 0);
        assert_eq!(format!("{p:?}"), before);
    }
}
#[test]
fn independently_rebound_epoch_refuses_cold_before_io() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false);
    let mut epoch = p.authority().epoch();
    epoch.id += 1;
    *p.authority_mut() = AudioAuthority::new_with_coordinates(
        p.authority().config(),
        epoch,
        AudioAuthorityCoordinateBasis::TargetFrame(port.basis),
    )
    .unwrap();
    let before = format!("{p:?}");
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.round, 0);
    assert_eq!(port.acquisitions, 0);
    assert_eq!(bgm.report().total_admitted, 0);
    assert!(p.authority().closed_host_prefix().is_none());
    assert_eq!(format!("{p:?}"), before);
}
#[test]
fn unpinned_cold_grid_mismatch_refuses_original_target_admission_without_mutating_owner() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false);
    let epoch = p.authority().epoch();
    let descriptor = AudioAuthorityCoordinateBasis::TargetFrame(
        TargetFrameBasis::new(port.basis.origin(), port.basis.start_time(), 14).unwrap(),
    );
    *p.authority_mut() =
        AudioAuthority::new_with_coordinates(p.authority().config(), epoch, descriptor).unwrap();
    // Cold presentation has no native pin yet. Its original port acquisition
    // exposes the actual 7 Hz creation basis; admission refuses the 14 Hz owner.
    let before = format!("{p:?}");
    assert!(prime(&mut port, &mut p, &mut bgm, &mut producer, 100).is_err());
    assert_eq!(port.round, 1);
    assert_eq!(port.acquisitions, 1);
    assert_eq!(bgm.report().total_admitted, 0);
    assert!(p.latest_record().is_none());
    assert!(p.authority().acquired_prefix().is_none());
    assert!(p.authority().closed_host_prefix().is_none());
    assert_eq!(format!("{p:?}"), before);
}
#[test]
fn already_observed_target_owner_refuses_before_another_io_iteration() {
    let (mut port, mut p, mut bgm, mut producer) = rig(false);
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
fn actual_finite_marker_persists_without_startup_end_ack_or_boundary_fabrication() {
    let (mut port, mut p, mut bgm, mut producer) = rig(true);
    prime(&mut port, &mut p, &mut bgm, &mut producer, 100)
        .unwrap()
        .unwrap();
    for report in &port.reports {
        assert_eq!(report.source.unwrap().playback_end_physical_frame, Some(4));
    }
    let facts = port.prior.unwrap();
    assert_eq!(facts.report().unwrap().playback_end_physical_frame, Some(4));
    assert!(facts.target_boundaries().unwrap().end.is_some());
    let subsequent = port.owner.render_pending(7).unwrap();
    assert_eq!(
        subsequent.source.unwrap().playback_end_physical_frame,
        Some(4)
    );
    assert_eq!(subsequent.source.unwrap().playback_frames, 0);
    assert!(p.authority().acquired_prefix().is_none());
    assert!(p.authority().closed_host_prefix().is_none());
}
#[test]
fn default_common_seed_method_calls_old_source_port_once_and_preserves_none_and_error() {
    struct SourceOnly {
        calls: usize,
        fail: bool,
    }
    impl NativeAudioSeedPort for SourceOnly {
        fn service_input(&mut self) -> NativeGameplayResult<bool> {
            panic!("not called")
        }
        fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
            panic!("not called")
        }
        fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
            panic!("not called")
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            panic!("not called")
        }
        fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
            panic!("not called")
        }
        fn observe_coordinate_audio(
            &mut self,
            _: &mut NativeAudioPresentation,
            _: SourcePlaybackIdentity,
        ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
            self.calls += 1;
            if self.fail {
                Err("original Source failure".into())
            } else {
                Ok(None)
            }
        }
    }
    for fail in [false, true] {
        let (port, mut p, _, _) = rig(false);
        let mut source = SourceOnly { calls: 0, fail };
        let result = source.observe_coordinate_playback_audio(&mut p, port.identity);
        assert_eq!(result.is_err(), fail);
        if !fail {
            assert!(result.unwrap().is_none());
        }
        assert_eq!(source.calls, 1);
        assert!(p.latest_record().is_none());
    }
}
