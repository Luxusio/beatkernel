//! Fixed-speed pause contracts on actual Mixer acknowledgements.
//! Controlled port pairs exercise policy; they do not claim a device clock.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, ChannelMatrix, CommandProducer,
        Mixer, MixerConfig, PcmLimits, PcmSample, RenderReport, ResampleQuality, SampleBank,
        SampleId, TargetFrameBasis, TargetTime, VoiceId,
    },
    input::DeviceId,
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    live_pause::{
        prepare_live_audio_transport_with_speed, prepare_live_transport,
        prepare_live_transport_with_speed, update_live_audio_pause,
        update_live_audio_pause_with_speed, update_live_pause_with_speed, validate_pre_pause_input,
        AudioLivePauseBoundary, LivePauseObservation,
    },
    live_speed::LiveSpeedSetup,
    local_input::InputMerger,
    native_start::StartInterval,
    playback_pause::{NativePause, PauseIntervalObservation, PausePhase},
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn selected(start: i64, preroll: i64, fraction: &str) -> LiveSpeedSetup {
    LiveSpeedSetup::new(
        ts(start),
        Duration::from_nanos(preroll),
        fraction.parse().unwrap(),
        None,
    )
    .unwrap()
}
fn point_evidence(ns: i64) -> LivePauseObservation {
    LivePauseObservation::Point(ClockPair {
        source: point(11, ns),
        target: point(22, ns),
    })
}
fn mixer(rate: u32, speed: Rate) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(rate, 1).unwrap();
    let pcm = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 512], pcm).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::SetRate {
            rate: speed,
            at: Timestamp::ZERO,
        })
        .unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(11),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn frame_ns(frame: u64, hz: u32) -> i64 {
    i64::try_from(i128::from(frame) * 1_000_000_000 / i128::from(hz)).unwrap()
}
fn interval(render: RenderReport, hz: u32, before: i64, after: i64) -> PauseIntervalObservation {
    PauseIntervalObservation {
        output_origin: point(11, 0),
        sample_rate: hz,
        render,
        clock: StartInterval::new(
            point(11, frame_ns(render.start_frame, hz)),
            point(22, before),
            point(22, after),
        )
        .unwrap(),
    }
}
fn interval_evidence(
    observation: Option<PauseIntervalObservation>,
    now: i64,
) -> LivePauseObservation {
    LivePauseObservation::Interval {
        observation,
        now: point(22, now),
    }
}
fn paused_point(
    setup: LiveSpeedSetup,
    hz: u32,
    active_frames: usize,
    gap_frames: usize,
) -> (CommandProducer, Mixer, NativePause, AudioLivePauseBoundary) {
    let (mut producer, mut output) = mixer(hz, setup.speed().rate());
    let first = output.render(&mut vec![0.0; active_frames]).unwrap();
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), hz).unwrap();
    let request = update_live_audio_pause_with_speed(
        &mut pause,
        point_evidence(frame_ns(active_frames as u64, hz)),
        Some(first),
        Some(true),
        setup,
        hz,
    )
    .unwrap();
    assert_eq!(request.requested, Some(true));
    assert!(request.boundary.is_none());
    producer.request_pause(true);
    let mut silence = vec![99.0; gap_frames];
    let frozen = output.render(&mut silence).unwrap();
    assert!(silence.iter().all(|sample| *sample == 0.0));
    let boundary = update_live_audio_pause_with_speed(
        &mut pause,
        point_evidence(frame_ns((active_frames + gap_frames) as u64, hz)),
        Some(frozen),
        None,
        setup,
        hz,
    )
    .unwrap()
    .boundary
    .unwrap();
    (producer, output, pause, boundary)
}

#[test]
fn four_rates_map_actual_source_ack_but_leave_host_and_raw_output_unscaled() {
    for (fraction, expected) in [
        ("1/2", 50_500_000),
        ("1/1", 51_000_000),
        ("2/1", 52_000_000),
        ("3/2", 51_500_000),
    ] {
        let setup = selected(50_000_000, 3_000_000, fraction);
        let (_producer, output, pause, boundary) = paused_point(setup, 1000, 4, 3);
        assert_eq!(boundary.original.song, ts(expected), "{fraction}");
        assert_eq!(boundary.original.playback_frame, 4);
        assert_eq!(boundary.raw_output, point(11, 4_000_000));
        assert_eq!(boundary.original.at, point(22, 4_000_000));
        assert_eq!(output.playback_frame_cursor(), 4);
        assert_eq!(output.frame_cursor(), 7);
        assert_eq!(pause.phase(), PausePhase::Paused);
        let original = setup.transport(Timestamp::ZERO).unwrap();
        let frozen =
            prepare_live_transport_with_speed(&original, boundary.original, ts(expected - 1), 1000)
                .unwrap();
        assert_eq!(frozen.position_at(ts(100_000_000)).unwrap(), ts(expected));
        assert_eq!(frozen.preroll_setup(), original.preroll_setup());
        assert!(!original.is_paused());
    }
}

#[test]
fn actual_odd_nano_pause_resume_keeps_positive_fractional_phase() {
    let setup = selected(0, 1, "1/2");
    let (mut producer, mut output, mut pause, boundary) = paused_point(setup, 1_000_000_000, 2, 3);
    assert_eq!(boundary.original.song, ts(0));
    let original = setup.transport(Timestamp::ZERO).unwrap();
    let frozen =
        prepare_live_transport_with_speed(&original, boundary.original, ts(0), 1_000_000_000)
            .unwrap();
    assert_eq!(
        frozen.position_at(ts(1)).unwrap(),
        original.position_at(ts(1)).unwrap()
    );
    let request = update_live_audio_pause_with_speed(
        &mut pause,
        point_evidence(5),
        None,
        Some(false),
        setup,
        1_000_000_000,
    )
    .unwrap();
    assert_eq!(request.requested, Some(false));
    producer.request_pause(false);
    let report = output.render(&mut [0.0; 1]).unwrap();
    let resumed = update_live_audio_pause_with_speed(
        &mut pause,
        point_evidence(6),
        Some(report),
        None,
        setup,
        1_000_000_000,
    )
    .unwrap()
    .boundary
    .unwrap();
    assert_eq!(resumed.original.song, ts(0));
    assert_eq!(resumed.raw_output, point(11, 5));
    let running =
        prepare_live_transport_with_speed(&frozen, resumed.original, ts(0), 1_000_000_000).unwrap();
    assert_eq!(running.position_at(ts(5)).unwrap(), ts(0));
    assert_eq!(running.position_at(ts(6)).unwrap(), ts(1));
    assert_eq!(running.position_at(ts(2)).unwrap(), ts(0));
}

#[test]
fn actual_odd_nano_preroll_pause_resume_preserves_negative_sign_crossing_phase() {
    let setup = selected(0, 3, "3/2");
    let (mut producer, mut output, mut pause, boundary) = paused_point(setup, 1_000_000_000, 1, 3);
    assert_eq!(boundary.original.song, ts(-3));
    let original = setup.transport(Timestamp::ZERO).unwrap();
    let frozen =
        prepare_live_transport_with_speed(&original, boundary.original, ts(-3), 1_000_000_000)
            .unwrap();
    update_live_audio_pause_with_speed(
        &mut pause,
        point_evidence(4),
        None,
        Some(false),
        setup,
        1_000_000_000,
    )
    .unwrap();
    producer.request_pause(false);
    let report = output.render(&mut [0.0; 1]).unwrap();
    let resumed = update_live_audio_pause_with_speed(
        &mut pause,
        point_evidence(5),
        Some(report),
        None,
        setup,
        1_000_000_000,
    )
    .unwrap()
    .boundary
    .unwrap();
    let running =
        prepare_live_transport_with_speed(&frozen, resumed.original, ts(-3), 1_000_000_000)
            .unwrap();
    assert_eq!(running.position_at(ts(5)).unwrap(), ts(-1));
    assert_eq!(running.position_at(ts(0)).unwrap(), ts(-4));
    assert_eq!(running.position_at(ts(1)).unwrap(), ts(-3));
}

#[test]
fn repeated_actual_odd_gaps_never_accumulate_integer_seek_drift() {
    let setup = selected(0, 1, "1/2");
    let hz = 1_000_000_000;
    let (mut producer, mut output, mut pause, mut boundary) = paused_point(setup, hz, 2, 3);
    let mut transport = setup.transport(Timestamp::ZERO).unwrap();
    for (cycle, frozen_song, next_song) in [(0, 0, 1), (1, 1, 1), (2, 1, 2), (3, 2, 2)] {
        assert_eq!(boundary.original.song, ts(frozen_song));
        transport =
            prepare_live_transport_with_speed(&transport, boundary.original, ts(frozen_song), hz)
                .unwrap();
        let physical = output.frame_cursor();
        update_live_audio_pause_with_speed(
            &mut pause,
            point_evidence(physical as i64),
            None,
            Some(false),
            setup,
            hz,
        )
        .unwrap();
        producer.request_pause(false);
        let report = output.render(&mut [0.0; 1]).unwrap();
        let resumed = update_live_audio_pause_with_speed(
            &mut pause,
            point_evidence(output.frame_cursor() as i64),
            Some(report),
            None,
            setup,
            hz,
        )
        .unwrap()
        .boundary
        .unwrap();
        transport =
            prepare_live_transport_with_speed(&transport, resumed.original, ts(frozen_song), hz)
                .unwrap();
        assert_eq!(
            transport
                .position_at(ts(output.frame_cursor() as i64))
                .unwrap(),
            ts(next_song)
        );
        assert_eq!(transport.position_at(ts(1)).unwrap(), ts(0));
        if cycle != 3 {
            update_live_audio_pause_with_speed(
                &mut pause,
                point_evidence(output.frame_cursor() as i64),
                Some(report),
                Some(true),
                setup,
                hz,
            )
            .unwrap();
            producer.request_pause(true);
            let report = output.render(&mut [0.0; 3]).unwrap();
            boundary = update_live_audio_pause_with_speed(
                &mut pause,
                point_evidence(output.frame_cursor() as i64),
                Some(report),
                None,
                setup,
                hz,
            )
            .unwrap()
            .boundary
            .unwrap();
        }
    }
}

#[test]
fn four_rates_keep_interval_lower_pause_and_upper_resume_endpoints_and_wait_until_upper() {
    for (fraction, song) in [
        ("1/2", 50_500_000),
        ("1/1", 51_000_000),
        ("2/1", 52_000_000),
        ("3/2", 51_500_000),
    ] {
        let setup = selected(50_000_000, 3_000_000, fraction);
        let (mut producer, mut output) = mixer(1000, setup.speed().rate());
        let first = interval(output.render(&mut [0.0; 4]).unwrap(), 1000, 0, 100_000);
        let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
        let request = update_live_audio_pause_with_speed(
            &mut pause,
            interval_evidence(Some(first), 100_000),
            None,
            Some(true),
            setup,
            1000,
        )
        .unwrap();
        assert_eq!(request.requested, Some(true));
        producer.request_pause(true);
        let frozen = interval(
            output.render(&mut [0.0; 3]).unwrap(),
            1000,
            3_900_000,
            4_100_000,
        );
        assert!(update_live_audio_pause_with_speed(
            &mut pause,
            interval_evidence(Some(frozen), 4_099_999),
            None,
            None,
            setup,
            1000
        )
        .unwrap()
        .boundary
        .is_none());
        let boundary = update_live_audio_pause_with_speed(
            &mut pause,
            interval_evidence(None, 4_100_000),
            None,
            None,
            setup,
            1000,
        )
        .unwrap()
        .boundary
        .unwrap();
        assert_eq!(boundary.original.at, point(22, 3_900_000));
        assert_eq!(boundary.original.window.latest(), point(22, 4_100_000));
        assert_eq!(boundary.raw_output, point(11, 4_000_000));
        assert_eq!(boundary.original.song, ts(song));
        update_live_audio_pause_with_speed(
            &mut pause,
            interval_evidence(Some(frozen), 5_000_000),
            None,
            Some(false),
            setup,
            1000,
        )
        .unwrap();
        producer.request_pause(false);
        let resumed = interval(
            output.render(&mut [0.0; 2]).unwrap(),
            1000,
            6_900_000,
            7_100_000,
        );
        assert!(update_live_audio_pause_with_speed(
            &mut pause,
            interval_evidence(Some(resumed), 7_099_999),
            None,
            None,
            setup,
            1000
        )
        .unwrap()
        .boundary
        .is_none());
        let resumed = update_live_audio_pause_with_speed(
            &mut pause,
            interval_evidence(None, 7_100_000),
            None,
            None,
            setup,
            1000,
        )
        .unwrap()
        .boundary
        .unwrap();
        assert_eq!(resumed.original.at, point(22, 7_100_000));
        assert_eq!(resumed.original.window.earliest(), point(22, 6_900_000));
        assert_eq!(resumed.raw_output, point(11, 7_000_000));
        assert_eq!(resumed.original.song, ts(song));
    }
}

fn converted(setup: LiveSpeedSetup) -> (CommandProducer, ConvertedNativeOutputState) {
    let (producer, mixer) = mixer(24_000, setup.speed().rate());
    let device = DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap();
    let output = ConvertedNativeOutputState::new(
        mixer,
        device,
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        128,
    )
    .unwrap_or_else(|_| panic!("valid converted rig"));
    (producer, output)
}
fn target_evidence(
    output: &ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    frame: u64,
) -> LivePauseObservation {
    LivePauseObservation::Target {
        epoch: 7,
        basis,
        facts: output.boundaries(),
        source: output.last_real_source_report(),
        pair: ClockPair {
            source: basis.point_at_stream_frame(frame).unwrap(),
            target: point(22, 1_000_000_000 + frame_ns(frame, 48_000)),
        },
    }
}

#[test]
fn four_rates_converted_boundary_waits_for_target_crossing_and_maps_original_source_grid() {
    for (fraction, song) in [
        ("1/2", 1_000_041_666),
        ("1/1", 1_000_083_332),
        ("2/1", 1_000_166_664),
        ("3/2", 1_000_124_998),
    ] {
        let setup = selected(1_000_000_000, 1, fraction);
        let (mut producer, mut output) = converted(setup);
        let basis = output.target_frame_basis();
        let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 24_000)
            .unwrap()
            .with_target_basis(7, basis)
            .unwrap();
        output.render_pending(1).unwrap();
        output.admit(1).unwrap();
        update_live_audio_pause_with_speed(
            &mut pause,
            target_evidence(&output, basis, 0),
            None,
            None,
            setup,
            24_000,
        )
        .unwrap();
        let request = update_live_audio_pause_with_speed(
            &mut pause,
            target_evidence(&output, basis, 1),
            None,
            Some(true),
            setup,
            24_000,
        )
        .unwrap();
        assert_eq!(request.requested, Some(true));
        producer.request_pause(true);
        output.render_pending(8).unwrap();
        assert!(update_live_audio_pause_with_speed(
            &mut pause,
            target_evidence(&output, basis, 3),
            None,
            None,
            setup,
            24_000
        )
        .unwrap()
        .boundary
        .is_none());
        let ack = update_live_audio_pause_with_speed(
            &mut pause,
            target_evidence(&output, basis, 4),
            None,
            None,
            setup,
            24_000,
        )
        .unwrap()
        .boundary
        .unwrap();
        assert_eq!(ack.original.playback_frame, 2);
        assert_eq!(ack.original.song, ts(song));
        assert_eq!(ack.raw_output, basis.point_at_stream_frame(4).unwrap());
        assert_eq!(ack.original.at, point(22, 1_000_083_333));
        assert_eq!(pause.phase(), PausePhase::Paused);
    }
}

#[test]
fn wrong_target_epoch_basis_domains_and_source_rate_refuse_atomically() {
    let setup = selected(0, 1, "3/2");
    let (_producer, mut output) = converted(setup);
    let basis = output.target_frame_basis();
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 24_000)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    update_live_audio_pause_with_speed(
        &mut pause,
        target_evidence(&output, basis, 0),
        None,
        None,
        setup,
        24_000,
    )
    .unwrap();
    let before = format!("{pause:?}");
    for case in 0..6 {
        let LivePauseObservation::Target {
            mut epoch,
            mut basis,
            mut facts,
            source,
            mut pair,
        } = target_evidence(&output, basis, 1)
        else {
            unreachable!()
        };
        match case {
            0 => epoch = 8,
            1 => basis = TargetFrameBasis::new(basis.origin(), basis.start_time(), 32_000).unwrap(),
            2 => facts.source_rate = 48_000,
            3 => facts.origin = Some(point(11, 1)),
            4 => pair.source.domain = ClockDomainId(99),
            _ => pair.target.domain = ClockDomainId(99),
        }
        assert!(update_live_audio_pause_with_speed(
            &mut pause,
            LivePauseObservation::Target {
                epoch,
                basis,
                facts,
                source,
                pair
            },
            None,
            Some(true),
            setup,
            24_000
        )
        .is_err());
        assert_eq!(format!("{pause:?}"), before);
    }
}

#[test]
fn bad_grid_wrong_host_and_overflow_do_not_publish_pause_ack_or_request() {
    let setup = selected(0, 1, "3/2");
    let (mut producer, mut output) = mixer(1000, setup.speed().rate());
    let first = output.render(&mut [0.0; 4]).unwrap();
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    let before = format!("{pause:?}");
    for hz in [0, 1_000_000_001] {
        assert!(update_live_pause_with_speed(
            &mut pause,
            point_evidence(4_000_000),
            Some(first),
            Some(true),
            setup,
            hz
        )
        .is_err());
        assert_eq!(format!("{pause:?}"), before);
    }
    assert!(update_live_pause_with_speed(
        &mut pause,
        LivePauseObservation::Point(ClockPair {
            source: point(11, 4_000_000),
            target: point(99, 4_000_000),
        }),
        Some(first),
        Some(true),
        setup,
        1000
    )
    .is_err());
    assert_eq!(format!("{pause:?}"), before);
    update_live_pause_with_speed(
        &mut pause,
        point_evidence(4_000_000),
        Some(first),
        Some(true),
        setup,
        1000,
    )
    .unwrap();
    producer.request_pause(true);
    let frozen = output.render(&mut [0.0; 3]).unwrap();
    let before = format!("{pause:?}");
    let overflow = selected(i64::MAX, 0, "2/1");
    assert!(update_live_pause_with_speed(
        &mut pause,
        point_evidence(7_000_000),
        Some(frozen),
        None,
        overflow,
        1000
    )
    .is_err());
    assert_eq!(format!("{pause:?}"), before);
    assert!(update_live_pause_with_speed(
        &mut pause,
        point_evidence(3_000_000),
        Some(frozen),
        None,
        setup,
        1000
    )
    .is_err());
    assert_eq!(format!("{pause:?}"), before);
    assert_eq!(
        update_live_pause_with_speed(
            &mut pause,
            point_evidence(7_000_000),
            Some(frozen),
            None,
            setup,
            1000
        )
        .unwrap()
        .boundary
        .unwrap()
        .song,
        ts(5_999_998)
    );
}

#[test]
fn exact_preparation_rejects_wrong_song_prefix_state_grid_and_chronology_without_mutation() {
    let setup = selected(0, 1, "1/2");
    let (_producer, _output, _pause, boundary) = paused_point(setup, 1_000_000_000, 2, 3);
    let original = setup.transport(Timestamp::ZERO).unwrap();
    let before = original.clone();
    let mut wrong = boundary.original;
    wrong.song = ts(1);
    assert!(prepare_live_transport_with_speed(&original, wrong, ts(0), 1_000_000_000).is_err());
    assert!(
        prepare_live_transport_with_speed(&original, boundary.original, ts(1), 1_000_000_000)
            .is_err()
    );
    for hz in [0, 1_000_000_001] {
        assert!(
            prepare_live_transport_with_speed(&original, boundary.original, ts(0), hz).is_err()
        );
    }
    let legacy = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::new(1, 2).unwrap());
    assert!(
        prepare_live_transport_with_speed(&legacy, boundary.original, ts(0), 1_000_000_000)
            .is_err()
    );
    let mut too_late = original.clone();
    too_late.set_rate(ts(3), Rate::new(1, 2).unwrap()).unwrap();
    let late_before = too_late.clone();
    assert!(
        prepare_live_transport_with_speed(&too_late, boundary.original, ts(0), 1_000_000_000)
            .is_err()
    );
    assert_eq!(too_late, late_before);
    assert_eq!(original, before);
    let frozen =
        prepare_live_transport_with_speed(&original, boundary.original, ts(0), 1_000_000_000)
            .unwrap();
    assert!(
        prepare_live_transport_with_speed(&frozen, boundary.original, ts(0), 1_000_000_000)
            .is_err()
    );
}

#[test]
fn pre_pause_inputs_stay_in_original_host_domain_and_strictly_before_ack_cutoff() {
    let setup = selected(0, 1, "3/2");
    let (_producer, _output, _pause, boundary) = paused_point(setup, 1_000_000_000, 2, 3);
    let transport = setup.transport(Timestamp::ZERO).unwrap();
    assert_eq!(boundary.original.song, ts(1));
    assert!(validate_pre_pause_input(&transport, boundary.original, point(22, 1)).is_ok());
    for input in [point(22, 2), point(22, 3), point(99, 1)] {
        assert!(validate_pre_pause_input(&transport, boundary.original, input).is_err());
    }
}

#[test]
fn prepared_authority_uses_logical_audio_not_scaled_host_and_refuses_wrong_tokens() {
    let setup = selected(0, 1, "1/2");
    let (_producer, _output, _pause, mut boundary) = paused_point(setup, 1_000_000_000, 2, 3);
    // Original port HOST clock is twice raw output; logical output has its own origin.
    boundary.original.window =
        beatkernel_bms_runtime::native_start::HostStartWindow::new(point(22, 4), point(22, 4))
            .unwrap();
    boundary.original.at = point(22, 4);
    let mut authority = AudioAuthority::new(
        AudioAuthorityConfig::default(),
        AudioAuthorityEpoch {
            id: 0,
            stream_origin: point(11, 0),
            logical_origin: point(33, 100),
            host_domain: ClockDomainId(22),
        },
    )
    .unwrap();
    for (raw, host) in [(2, 4), (5, 10)] {
        authority
            .observe(
                0,
                ClockPair {
                    source: point(11, raw),
                    target: point(22, host),
                },
            )
            .unwrap();
    }
    authority.record_acquired_prefix(point(22, 10)).unwrap();
    let merger = InputMerger::new(ClockDomainId(22), point(22, 0), vec![DeviceId(9)], 8).unwrap();
    let control = authority
        .prepare_control_cutoff(
            0,
            boundary.raw_output,
            boundary.original.at,
            point(22, 10),
            &merger,
        )
        .unwrap()
        .unwrap();
    assert_eq!(control.output(), point(33, 102));
    let transport = setup.transport(ts(100)).unwrap();
    let frozen = prepare_live_audio_transport_with_speed(
        &transport,
        boundary,
        &control,
        ts(0),
        1_000_000_000,
    )
    .unwrap();
    assert_eq!(frozen.position_at(ts(102)).unwrap(), ts(0));
    assert_eq!(frozen.position_at(ts(101)).unwrap(), ts(0));
    let original = transport.clone();
    for case in 0..3 {
        let mut invalid = boundary;
        match case {
            0 => invalid.epoch = 1,
            1 => invalid.raw_output = point(11, 3),
            _ => invalid.original.paused = false,
        }
        assert!(prepare_live_audio_transport_with_speed(
            &transport,
            invalid,
            &control,
            ts(0),
            1_000_000_000
        )
        .is_err());
        assert_eq!(transport, original);
    }
    authority.commit_control_cutoff(control, &merger).unwrap();
    assert!(authority.commit_control_cutoff(control, &merger).is_err());
}

#[test]
fn normal_zero_preroll_update_and_legacy_preparation_keep_existing_behavior() {
    let setup = selected(47_000_000, 0, "1/1");
    let (mut producer, mut output) = mixer(1000, Rate::NORMAL);
    let first = output.render(&mut [0.0; 4]).unwrap();
    let initial = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    let mut old = initial.clone();
    let mut new = initial;
    let legacy = update_live_audio_pause(
        &mut old,
        point_evidence(4_000_000),
        Some(first),
        Some(true),
        ts(47_000_000),
        1000,
    )
    .unwrap();
    let selected = update_live_audio_pause_with_speed(
        &mut new,
        point_evidence(4_000_000),
        Some(first),
        Some(true),
        setup,
        1000,
    )
    .unwrap();
    assert_eq!(legacy, selected);
    producer.request_pause(true);
    let frozen = output.render(&mut [0.0; 3]).unwrap();
    let legacy = update_live_audio_pause(
        &mut old,
        point_evidence(7_000_000),
        Some(frozen),
        None,
        ts(47_000_000),
        1000,
    )
    .unwrap();
    let selected = update_live_audio_pause_with_speed(
        &mut new,
        point_evidence(7_000_000),
        Some(frozen),
        None,
        setup,
        1000,
    )
    .unwrap();
    assert_eq!(legacy, selected);
    assert_eq!(format!("{old:?}"), format!("{new:?}"));
    let original = Transport::new(Timestamp::ZERO, ts(47_000_000), Rate::NORMAL);
    let candidate =
        prepare_live_transport(&original, legacy.boundary.unwrap().original, ts(50_000_000))
            .unwrap();
    assert_eq!(
        candidate.position_at(ts(100_000_000)).unwrap(),
        ts(51_000_000)
    );
    assert!(candidate.preroll_setup().is_none());
}

#[test]
fn resume_refuses_different_active_frame_even_when_rounded_song_is_identical() {
    let setup = selected(0, 1, "1/2");
    let (mut producer, mut output, mut pause, boundary) = paused_point(setup, 1_000_000_000, 2, 3);
    let frozen = prepare_live_transport_with_speed(
        &setup.transport(Timestamp::ZERO).unwrap(),
        boundary.original,
        ts(0),
        1_000_000_000,
    )
    .unwrap();
    update_live_audio_pause_with_speed(
        &mut pause,
        point_evidence(5),
        None,
        Some(false),
        setup,
        1_000_000_000,
    )
    .unwrap();
    producer.request_pause(false);
    let report = output.render(&mut [0.0; 1]).unwrap();
    let resumed = update_live_audio_pause_with_speed(
        &mut pause,
        point_evidence(6),
        Some(report),
        None,
        setup,
        1_000_000_000,
    )
    .unwrap()
    .boundary
    .unwrap();
    let mut fake = resumed.original;
    fake.playback_frame = 1; // Both E=1 and E=2 map to song 0 at this phase.
    assert_eq!(fake.song, ts(0));
    let before = frozen.clone();
    assert!(prepare_live_transport_with_speed(&frozen, fake, ts(0), 1_000_000_000).is_err());
    assert_eq!(frozen, before);
    assert_eq!(
        prepare_live_transport_with_speed(&frozen, resumed.original, ts(0), 1_000_000_000)
            .unwrap()
            .position_at(ts(6))
            .unwrap(),
        ts(1)
    );
}

#[test]
fn converted_held_output_and_real_resume_crossing_keep_frozen_source_phase() {
    let setup = selected(1_000_000_000, 1, "3/2");
    let (mut producer, mut output) = converted(setup);
    let basis = output.target_frame_basis();
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 24_000)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    update_live_audio_pause_with_speed(
        &mut pause,
        target_evidence(&output, basis, 0),
        None,
        None,
        setup,
        24_000,
    )
    .unwrap();
    update_live_audio_pause_with_speed(
        &mut pause,
        target_evidence(&output, basis, 1),
        None,
        Some(true),
        setup,
        24_000,
    )
    .unwrap();
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    let frozen = update_live_audio_pause_with_speed(
        &mut pause,
        target_evidence(&output, basis, 4),
        None,
        None,
        setup,
        24_000,
    )
    .unwrap()
    .boundary
    .unwrap();
    output.admit(8).unwrap();
    update_live_audio_pause_with_speed(
        &mut pause,
        target_evidence(&output, basis, 9),
        None,
        Some(false),
        setup,
        24_000,
    )
    .unwrap();
    producer.request_pause(false);
    let source_phase = output.converter_owner().source_position();
    output.render_held_pending(4).unwrap();
    assert_eq!(output.converter_owner().source_position(), source_phase);
    assert!(output.mixer().is_paused());
    assert!(update_live_audio_pause_with_speed(
        &mut pause,
        target_evidence(&output, basis, 13),
        None,
        None,
        setup,
        24_000
    )
    .unwrap()
    .boundary
    .is_none());
    output.admit(4).unwrap();
    for _ in 0..8 {
        output.render_pending(1).unwrap();
        output.admit(1).unwrap();
        if output.boundaries().resume.is_some() {
            break;
        }
    }
    let mapped = output
        .boundaries()
        .resume
        .expect("real source resume must be consumed by converter");
    assert_eq!(
        mapped.target_time,
        TargetTime::from_frames(16, 48_000).unwrap()
    );
    assert!(update_live_audio_pause_with_speed(
        &mut pause,
        target_evidence(&output, basis, 15),
        None,
        None,
        setup,
        24_000
    )
    .unwrap()
    .boundary
    .is_none());
    let resumed = update_live_audio_pause_with_speed(
        &mut pause,
        target_evidence(&output, basis, 16),
        None,
        None,
        setup,
        24_000,
    )
    .unwrap()
    .boundary
    .unwrap();
    assert_eq!(resumed.original.song, frozen.original.song);
    assert_eq!(resumed.original.playback_frame, 2);
    assert_eq!(resumed.raw_output, basis.point_at_stream_frame(16).unwrap());
    assert_eq!(
        resumed.raw_output.timestamp.as_nanos() - frozen.raw_output.timestamp.as_nanos(),
        250_000
    );
    assert_eq!(pause.phase(), PausePhase::Running);
}
