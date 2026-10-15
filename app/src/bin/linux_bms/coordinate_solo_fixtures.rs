//! Real software state produced by the Linux coordinate factory; no ALSA timing is fabricated.
use super::native::{
    coordinate_initial_query, coordinate_initial_transport, prepare_coordinate_target_state,
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, ChannelMatrix, CommandProducer,
        Mixer, MixerConfig, PcmLimits, PcmSample, ResampleQuality, SampleBank, SampleId,
        TargetTime, VoiceId,
    },
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

const ORIGIN: Timestamp = Timestamp::from_nanos(-123);
const DOMAIN: ClockDomainId = ClockDomainId(27);

fn device() -> DeviceFormat {
    DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap()
}

fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(4096, 4096, 2).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            (0..128).map(|i| 0.125 + i as f32 / 512.0).collect(),
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: ORIGIN,
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            DOMAIN,
            ORIGIN,
            AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 5]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 3]).unwrap();
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (8, 5)
    );
    producer.request_pause(false);
    (producer, mixer)
}

#[test]
fn coordinate_factory_captures_actual_nonzero_source_identity_before_conversion() {
    let (_producer, mixer) = rig();
    let counters = mixer.counters();
    let (state, identity) = prepare_coordinate_target_state(mixer, device(), 6, 8).unwrap();
    let cold_target = state.target_frame_basis();
    assert_eq!(
        (
            identity.hz(),
            identity.initial_physical(),
            identity.initial_playback()
        ),
        (1000, 8, 5)
    );
    assert_eq!(cold_target.sample_rate(), 1500);
    assert_eq!(cold_target.origin().domain, DOMAIN);
    assert_eq!(cold_target.origin().timestamp, ORIGIN);
    assert_eq!(
        cold_target.start_time(),
        TargetTime::from_frames(8, 1000).unwrap()
    );
    assert_eq!(
        state.mixer().config().format(),
        AudioFormat::new(1000, 1).unwrap()
    );
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(
        (
            state.mixer().frame_cursor(),
            state.mixer().playback_frame_cursor()
        ),
        (8, 5)
    );
    let source = state.mixer().source_playback_history().unwrap();
    assert_eq!(
        (
            source.source_rate(),
            source.initial_physical_frame(),
            source.initial_playback_frame()
        ),
        (1000, 8, 5)
    );
    assert_eq!(source.capacity(), 8);
    assert!(source.is_empty());
    let phase = state.phase_history().unwrap();
    assert_eq!(phase.source_rate(), 1000);
    assert_eq!(phase.initial_source_position().frame, 8);
    assert_eq!(phase.initial_source_position().numerator, 0);
    assert_eq!(phase.start_time(), cold_target.start_time());
    assert_eq!(phase.capacity(), 8);
    assert!(phase.is_empty());
    assert!(state.pending_report().is_none());
    assert!(state.admitted_target_time().is_none());
    assert_eq!(state.pending_frames(), 0);
}

#[test]
fn coordinate_factory_preserves_original_commands_pcm_and_partial_submission_truth() {
    let (mut coordinate_commands, mixer) = rig();
    let (mut reference_commands, reference_mixer) = rig();
    let (mut coordinate, identity) =
        prepare_coordinate_target_state(mixer, device(), 6, 8).unwrap();
    let mut reference = ConvertedNativeOutputState::new(
        reference_mixer,
        device(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        6,
    )
    .unwrap_or_else(|_| panic!("valid scalar reference"));
    let cold_target = coordinate.target_frame_basis();
    assert!(reference.phase_history().is_none());
    assert!(reference.mixer().source_playback_history().is_none());
    for commands in [&mut coordinate_commands, &mut reference_commands] {
        commands
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::from_nanos(ORIGIN.as_nanos() + 8_000_000),
                gain: 0.5,
            })
            .unwrap();
    }
    for block in 0..4 {
        let report = coordinate.render_pending(6).unwrap();
        let reference_report = reference.render_pending(6).unwrap();
        assert_eq!(report, reference_report);
        assert_eq!(coordinate.pending_samples(), reference.pending_samples());
        assert!(coordinate
            .pending_samples()
            .iter()
            .any(|sample| *sample > 0.0));
        assert_eq!(coordinate.mixer().counters(), reference.mixer().counters());
        assert_eq!(
            coordinate.mixer().frame_cursor(),
            reference.mixer().frame_cursor()
        );
        let source = coordinate.mixer().source_playback_history().unwrap();
        assert_eq!(
            (
                source.initial_physical_frame(),
                source.initial_playback_frame()
            ),
            (8, 5)
        );
        assert!(source.pulled_end_frame() > identity.initial_physical());
        let first_span = source.span(0).unwrap();
        assert_eq!(
            (first_span.physical_start(), first_span.playback_start()),
            (8, 5)
        );
        let phase = coordinate.phase_history().unwrap();
        assert_eq!(phase.start_time(), cold_target.start_time());
        assert_eq!(
            phase.initial_source_position().frame,
            identity.initial_physical()
        );
        assert_eq!(phase.end_time(), report.target_end_time);
        if block == 0 {
            assert!(coordinate.admitted_target_time().is_none());
        }
        let pcm = coordinate.pending_samples().to_vec();
        coordinate.admit(2).unwrap();
        reference.admit(2).unwrap();
        assert_eq!(coordinate.pending_samples(), &pcm[2..]);
        assert_eq!(coordinate.pending_frames(), 4);
        let admitted = report
            .target_start_time
            .checked_add_frames(2, 1500)
            .unwrap();
        assert_eq!(coordinate.admitted_target_time(), Some(admitted));
        assert_eq!(coordinate.target_frame_basis().start_time(), admitted);
        assert_eq!(
            coordinate.admitted_target_time(),
            reference.admitted_target_time()
        );
        let counters = coordinate.mixer().counters();
        assert!(coordinate.render_pending(6).is_err());
        assert_eq!(coordinate.mixer().counters(), counters);
        assert_eq!(coordinate.pending_samples(), &pcm[2..]);
        coordinate.admit(4).unwrap();
        reference.admit(4).unwrap();
        assert_eq!(
            coordinate.admitted_target_time(),
            Some(report.target_end_time)
        );
        assert_eq!(
            coordinate.target_frame_basis(),
            reference.target_frame_basis()
        );
        assert_eq!(coordinate.pending_frames(), 0);
    }
    assert_eq!(coordinate.mixer().counters().commands_consumed, 2);
    assert!(reference.phase_history().is_none());
    assert!(reference.mixer().source_playback_history().is_none());
}

#[test]
fn coordinate_factory_refuses_invalid_cold_capacity_and_period_without_hardware() {
    for (period, capacity) in [(0, 8), (6, 0), (usize::MAX, 8), (6, usize::MAX)] {
        let (_producer, mixer) = rig();
        assert!(prepare_coordinate_target_state(mixer, device(), period, capacity).is_err());
    }
}

#[test]
fn coordinate_factory_refuses_preexisting_source_history_instead_of_replacing_identity() {
    let (_producer, mut mixer) = rig();
    mixer.enable_source_playback_history(3).unwrap();
    assert!(prepare_coordinate_target_state(mixer, device(), 6, 8).is_err());
}

#[test]
fn coordinate_initial_query_uses_exact_cold_target_time_before_and_after_generation() {
    let (_producer, mixer) = rig();
    let (mut state, _) = prepare_coordinate_target_state(mixer, device(), 6, 8).unwrap();
    let cold = state.target_frame_basis();
    let query = coordinate_initial_query(cold).unwrap();
    assert_eq!(
        query.compare_target_time(cold.start_time()),
        std::cmp::Ordering::Equal
    );
    state.render_pending(6).unwrap();
    state.admit(6).unwrap();
    assert_eq!(
        coordinate_initial_query(state.target_frame_basis())
            .unwrap()
            .compare_target_time(cold.start_time()),
        std::cmp::Ordering::Greater
    );
    assert_eq!(
        coordinate_initial_query(cold)
            .unwrap()
            .compare_interpolated_time(query),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        state.phase_history().unwrap().start_time(),
        cold.start_time()
    );
}

#[test]
fn coordinate_initial_transport_preserves_selected_preroll_source_cut_and_attempt_identity() {
    use beatkernel::time::{ClockPoint, Duration};
    use beatkernel::transport::Rate;
    use beatkernel_bms_runtime::{live_speed::LiveSpeedSetup, replay_speed::ReplaySpeed};
    let (_producer, mixer) = rig();
    let (_state, identity) = prepare_coordinate_target_state(mixer, device(), 6, 8).unwrap();
    let logical = ClockPoint {
        domain: super::native::LOGICAL,
        timestamp: Timestamp::from_nanos(123_456),
    };
    for rate in [Rate::NORMAL, Rate::new(3, 2).unwrap()] {
        let setup = LiveSpeedSetup::new(
            Timestamp::from_nanos(50_000_000),
            Duration::from_nanos(2_000_000),
            ReplaySpeed::new(rate).unwrap(),
            Some(Timestamp::from_nanos(55_000_000)),
        )
        .unwrap();
        let transport = coordinate_initial_transport(91, identity, logical, setup).unwrap();
        let phase = transport.output_phase_basis().unwrap();
        assert_eq!(
            (
                phase.owner(),
                phase.generation(),
                phase.active_origin_frame(),
                phase.source_hz()
            ),
            (91, 1, 5, 1000)
        );
        assert_eq!(phase.domain(), logical.domain);
        assert_eq!(transport.anchor().rate, rate);
        assert_eq!(transport.runtime_output_rate(), Some(rate));
        assert_eq!(transport.anchor().host_time, logical.timestamp);
        assert_eq!(transport.anchor().song_time, setup.song_origin());
        let original = transport.preroll_setup().unwrap();
        assert_eq!(original.logical_origin, logical.timestamp);
        assert_eq!(original.original_start, setup.start());
        assert_eq!(original.output_preroll, setup.preroll());
        assert_eq!(original.positive_rate, rate);
        assert_eq!(original.original_end, setup.end());
        for (elapsed, expected) in [
            (0, setup.song_origin()),
            (2_000_000, setup.start()),
            (20_000_000, setup.end().unwrap()),
        ] {
            let at = ClockPoint {
                domain: logical.domain,
                timestamp: Timestamp::from_nanos(logical.timestamp.as_nanos() + elapsed as i64),
            };
            // Pure elapsed arithmetic tests the launch setup, not native clock admission.
            let point = phase.point_from_elapsed_nanos(at, elapsed).unwrap();
            assert_eq!(transport.position_at_output_phase(point).unwrap(), expected);
        }
        let foreign = ClockPoint {
            domain: ClockDomainId(999),
            timestamp: logical.timestamp,
        };
        // The reusable constructor honors caller-selected chronologies; only
        // the configured Linux adapter requires its own LOGICAL axis.
        let custom = beatkernel_bms_runtime::native_audio_startup::initial_source_coordinate_runtime_transport(
            91, identity, foreign, setup,
        )
        .unwrap();
        let custom_phase = custom.output_phase_basis().unwrap();
        assert_eq!(custom_phase.domain(), foreign.domain);
        assert_eq!(custom_phase.owner(), phase.owner());
        assert_eq!(custom_phase.generation(), phase.generation());
        assert_eq!(
            custom_phase.active_origin_frame(),
            phase.active_origin_frame()
        );
        assert_eq!(custom_phase.source_hz(), phase.source_hz());
        assert_eq!(custom.runtime_output_rate(), Some(rate));
        assert_eq!(custom.anchor(), transport.anchor());
        assert_eq!(
            custom
                .position_at_output_phase(
                    custom_phase.point_from_elapsed_nanos(foreign, 0).unwrap()
                )
                .unwrap(),
            setup.song_origin()
        );
        assert!(coordinate_initial_transport(91, identity, foreign, setup).is_err());
        for domain in [super::native::HOST, super::native::OUTPUT] {
            let error = coordinate_initial_transport(
                91,
                identity,
                ClockPoint {
                    domain,
                    timestamp: logical.timestamp,
                },
                setup,
            )
            .unwrap_err();
            assert_eq!(
                error.downcast_ref::<beatkernel::transport::OutputPhaseError>(),
                Some(&beatkernel::transport::OutputPhaseError::WrongDomain)
            );
        }
    }
}
