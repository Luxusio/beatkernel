//! Real cold Mixer state and mathematical launch phases. No native observation or ACK.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig,
        OutputFrameBasis, PcmLimits, PcmSample, SampleBank, SampleId, SourcePlaybackState,
        SourcePosition, TargetTime, VoiceId,
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::Rate,
};
use beatkernel_bms_runtime::{
    live_speed::LiveSpeedSetup,
    native_audio_startup::{
        cold_source_coordinate_identity, initial_source_coordinate_query,
        initial_source_coordinate_transport, prepare_source_coordinate_history,
        source_coordinate_history_capacity, SOURCE_COORDINATE_HISTORY_HEADROOM,
    },
    replay_speed::ReplaySpeed,
};

const RAW: ClockDomainId = ClockDomainId(27);
const LOGICAL: ClockDomainId = ClockDomainId(71);
const ORIGIN: Timestamp = Timestamp::from_nanos(-123);

fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 128], limits).unwrap(),
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
            RAW,
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
    (producer, mixer)
}

#[test]
fn pure_cold_identity_capture_matches_later_history_enable_without_changing_mixer() {
    let (producer, mut mixer) = rig();
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
        mixer.output_frame_basis(),
    );
    let identity = cold_source_coordinate_identity(&mixer).unwrap();
    assert_eq!(
        (
            identity.hz(),
            identity.initial_physical(),
            identity.initial_playback()
        ),
        (1000, 8, 5)
    );
    assert!(mixer.source_playback_history().is_none());
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            producer.counters(),
            mixer.output_frame_basis(),
        ),
        before
    );
    let enabled = prepare_source_coordinate_history(
        &mut mixer,
        source_coordinate_history_capacity(64).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (
            enabled.hz(),
            enabled.initial_physical(),
            enabled.initial_playback()
        ),
        (
            identity.hz(),
            identity.initial_physical(),
            identity.initial_playback()
        )
    );
    let history = mixer.source_playback_history().unwrap();
    assert_eq!(history.capacity(), 4418);
    assert!(history.is_empty());
    assert_eq!(history.pulled_end_frame(), 8);
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            producer.counters(),
            mixer.output_frame_basis(),
        ),
        before
    );
}

#[test]
fn pure_identity_capture_uses_original_history_tuple_after_generation_and_retirement() {
    let (mut producer, mut mixer) = rig();
    let original = prepare_source_coordinate_history(&mut mixer, 8).unwrap();
    for paused in [true, false, true, false] {
        producer.request_pause(paused);
        mixer.render(&mut [0.0; 2]).unwrap();
    }
    assert_eq!(mixer.retire_source_playback_before(13).unwrap(), 2);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (16, 9)
    );
    let before = (
        mixer.counters(),
        producer.counters(),
        mixer.output_frame_basis(),
        mixer.source_playback_history().unwrap().span(0),
        mixer.source_playback_history().unwrap().span(1),
    );
    let captured = cold_source_coordinate_identity(&mixer).unwrap();
    assert_eq!(
        (
            captured.hz(),
            captured.initial_physical(),
            captured.initial_playback()
        ),
        (
            original.hz(),
            original.initial_physical(),
            original.initial_playback()
        )
    );
    assert_eq!(
        (
            captured.hz(),
            captured.initial_physical(),
            captured.initial_playback()
        ),
        (1000, 8, 5)
    );
    let history = mixer.source_playback_history().unwrap();
    assert_eq!(
        (
            history.capacity(),
            history.len(),
            history.retirement_revision()
        ),
        (8, 2, 1)
    );
    assert_eq!(
        (history.retained_start_frame(), history.pulled_end_frame()),
        (12, 16)
    );
    assert_eq!(
        (
            mixer.counters(),
            producer.counters(),
            mixer.output_frame_basis(),
            history.span(0),
            history.span(1),
        ),
        before
    );
}

#[test]
fn cold_preparation_keeps_actual_physical_and_playback_cursors_and_future_pause_spans() {
    let (mut producer, mut mixer) = rig();
    let before = (
        mixer.counters(),
        producer.counters(),
        mixer.output_frame_basis(),
    );
    let identity = prepare_source_coordinate_history(&mut mixer, 4).unwrap();
    assert_eq!(
        (
            identity.hz(),
            identity.initial_physical(),
            identity.initial_playback()
        ),
        (1000, 8, 5)
    );
    assert_eq!(
        (
            mixer.counters(),
            producer.counters(),
            mixer.output_frame_basis()
        ),
        before
    );
    let cold = mixer.source_playback_history().unwrap();
    assert_eq!(
        (
            cold.initial_physical_frame(),
            cold.initial_playback_frame(),
            cold.pulled_end_frame()
        ),
        (8, 5, 8)
    );
    assert!(cold.is_empty());
    let mut silent = [99.0; 2];
    assert!(mixer.render(&mut silent).unwrap().paused);
    assert_eq!(silent, [0.0; 2]);
    producer.request_pause(false);
    let mut audible = [99.0; 4];
    assert!(!mixer.render(&mut audible).unwrap().paused);
    assert_eq!(audible, [0.25; 4]);
    let history = mixer.source_playback_history().unwrap();
    assert_eq!((history.len(), history.pulled_end_frame()), (2, 14));
    let frozen = history.span(0).unwrap();
    let running = history.span(1).unwrap();
    assert_eq!(
        (
            frozen.physical_start(),
            frozen.physical_end(),
            frozen.playback_start(),
            frozen.state()
        ),
        (8, Some(10), 5, SourcePlaybackState::Frozen)
    );
    assert_eq!(
        (
            running.physical_start(),
            running.physical_end(),
            running.playback_start(),
            running.state()
        ),
        (10, None, 5, SourcePlaybackState::Running)
    );
    assert_eq!(
        (
            history.initial_physical_frame(),
            history.initial_playback_frame()
        ),
        (identity.initial_physical(), identity.initial_playback())
    );
}

#[test]
fn cold_refusal_and_repeated_preparation_preserve_original_history_and_commands() {
    let (mut producer, mut mixer) = rig();
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
    );
    for capacity in [0, usize::MAX] {
        assert!(prepare_source_coordinate_history(&mut mixer, capacity).is_err());
        assert!(mixer.source_playback_history().is_none());
        assert_eq!(
            (
                mixer.frame_cursor(),
                mixer.playback_frame_cursor(),
                mixer.counters(),
                producer.counters()
            ),
            before
        );
    }
    let identity = prepare_source_coordinate_history(&mut mixer, 3).unwrap();
    mixer.render(&mut [0.0; 2]).unwrap();
    let span = mixer.source_playback_history().unwrap().span(0).unwrap();
    let prepared = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
    );
    for capacity in [0, 4, usize::MAX] {
        assert!(prepare_source_coordinate_history(&mut mixer, capacity).is_err());
        assert_eq!(
            (
                mixer.frame_cursor(),
                mixer.playback_frame_cursor(),
                mixer.counters(),
                producer.counters()
            ),
            prepared
        );
        let history = mixer.source_playback_history().unwrap();
        assert_eq!(history.span(0), Some(span));
        assert_eq!(
            (
                history.capacity(),
                history.initial_physical_frame(),
                history.initial_playback_frame()
            ),
            (3, identity.initial_physical(), identity.initial_playback())
        );
    }
    producer.request_pause(false);
    let mut pcm = [99.0; 4];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25; 4]);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (14, 9)
    );
}

#[test]
fn enabling_history_preserves_pcm_render_reports_and_original_scheduled_command_time() {
    let (mut prepared_commands, mut prepared) = rig();
    let (mut reference_commands, mut reference) = rig();
    prepare_source_coordinate_history(&mut prepared, 8).unwrap();
    for commands in [&mut prepared_commands, &mut reference_commands] {
        commands.request_pause(false);
        commands
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::from_nanos(ORIGIN.as_nanos() + 7_000_000),
                gain: 0.5,
            })
            .unwrap();
    }
    for frames in [1, 4, 7, 3] {
        let mut actual = vec![99.0; frames];
        let mut expected = vec![99.0; frames];
        assert_eq!(
            prepared.render(&mut actual).unwrap(),
            reference.render(&mut expected).unwrap()
        );
        assert_eq!(actual, expected);
        assert!(actual.iter().any(|sample| *sample > 0.0));
        assert_eq!(prepared.counters(), reference.counters());
        assert!(reference.source_playback_history().is_none());
    }
    assert_eq!(prepared.counters().commands_consumed, 2);
}

#[test]
fn cold_query_preserves_subnanosecond_and_fractional_source_time_independently_of_display_origin() {
    for (hz, frame) in [(3, 2), (44_100, 17), (u32::MAX, 1), (u32::MAX, 2)] {
        let exact = TargetTime::from_frames(frame, hz).unwrap();
        let a = OutputFrameBasis::new(
            ClockPoint {
                domain: RAW,
                timestamp: ORIGIN,
            },
            hz,
            frame,
        )
        .unwrap();
        let b = OutputFrameBasis::new(
            ClockPoint {
                domain: ClockDomainId(99),
                timestamp: Timestamp::from_nanos(i64::MAX),
            },
            hz,
            frame,
        )
        .unwrap();
        let query = initial_source_coordinate_query(a).unwrap();
        assert_eq!(query.compare_target_time(exact), std::cmp::Ordering::Equal);
        assert_eq!(
            query.compare_interpolated_time(initial_source_coordinate_query(b).unwrap()),
            std::cmp::Ordering::Equal
        );
        let zero = ClockPoint {
            domain: RAW,
            timestamp: Timestamp::ZERO,
        };
        let displayed = query.point(zero).unwrap().timestamp.as_nanos();
        let floor = TargetTime::new(0, displayed as u64, 1_000_000_000).unwrap();
        assert_eq!(
            query.compare_target_time(floor),
            std::cmp::Ordering::Greater
        );
    }
    let one = initial_source_coordinate_query(
        OutputFrameBasis::new(
            ClockPoint {
                domain: RAW,
                timestamp: ORIGIN,
            },
            u32::MAX,
            1,
        )
        .unwrap(),
    )
    .unwrap();
    let two = initial_source_coordinate_query(
        OutputFrameBasis::new(
            ClockPoint {
                domain: RAW,
                timestamp: ORIGIN,
            },
            u32::MAX,
            2,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        one.point(ClockPoint {
            domain: RAW,
            timestamp: Timestamp::ZERO
        })
        .unwrap(),
        two.point(ClockPoint {
            domain: RAW,
            timestamp: Timestamp::ZERO
        })
        .unwrap()
    );
    assert_eq!(one.compare_interpolated_time(two), std::cmp::Ordering::Less);
}

#[test]
fn captured_cold_query_is_immutable_after_actual_mixer_generation() {
    let (mut producer, mut mixer) = rig();
    prepare_source_coordinate_history(&mut mixer, 4).unwrap();
    let cold = mixer.output_frame_basis();
    let query = initial_source_coordinate_query(cold).unwrap();
    producer.request_pause(false);
    mixer.render(&mut [0.0; 4]).unwrap();
    assert_eq!(
        initial_source_coordinate_query(mixer.output_frame_basis())
            .unwrap()
            .compare_interpolated_time(query),
        std::cmp::Ordering::Greater
    );
    assert_eq!(
        initial_source_coordinate_query(cold)
            .unwrap()
            .compare_target_time(TargetTime::from_frames(8, 1000).unwrap()),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        mixer
            .source_playback_history()
            .unwrap()
            .initial_physical_frame(),
        8
    );
}

#[test]
fn history_budget_rejects_zero_and_checks_large_configurations_without_allocating() {
    assert_eq!(SOURCE_COORDINATE_HISTORY_HEADROOM, 4354);
    assert!(source_coordinate_history_capacity(0).is_err());
    for frames in [1_u32, 64, 1024, u32::MAX] {
        let required = u64::from(frames) + SOURCE_COORDINATE_HISTORY_HEADROOM as u64;
        match usize::try_from(required) {
            Ok(expected) => assert_eq!(
                source_coordinate_history_capacity(frames).unwrap(),
                expected
            ),
            Err(_) => assert!(source_coordinate_history_capacity(frames).is_err()),
        }
    }
}

#[test]
fn actual_cold_playback_cut_seeds_selected_phase_with_preroll_end_and_domain_guards() {
    let (_producer, mut mixer) = rig();
    let identity = prepare_source_coordinate_history(&mut mixer, 4).unwrap();
    let logical = ClockPoint {
        domain: LOGICAL,
        timestamp: Timestamp::from_nanos(123_456),
    };
    for rate in [
        Rate::NORMAL,
        Rate::new(3, 2).unwrap(),
        Rate::new(1, 2).unwrap(),
    ] {
        let setup = LiveSpeedSetup::new(
            Timestamp::from_nanos(50_000_000),
            Duration::from_nanos(2_000_000),
            ReplaySpeed::new(rate).unwrap(),
            Some(Timestamp::from_nanos(55_000_000)),
        )
        .unwrap();
        let transport = initial_source_coordinate_transport(91, identity, logical, setup).unwrap();
        let phase = transport.output_phase_basis().unwrap();
        assert_eq!(
            (
                phase.domain(),
                phase.owner(),
                phase.generation(),
                phase.active_origin_frame(),
                phase.source_hz()
            ),
            (LOGICAL, 91, 1, 5, 1000)
        );
        assert_eq!(transport.anchor().rate, rate);
        for (frames, expected) in [
            (0, setup.song_origin()),
            (2, setup.start()),
            (22, setup.end().unwrap()),
        ] {
            let at = ClockPoint {
                domain: LOGICAL,
                timestamp: Timestamp::from_nanos(logical.timestamp.as_nanos() + frames * 1_000_000),
            };
            let point = phase
                .point(
                    at,
                    SourcePosition {
                        frame: identity.initial_playback() + frames as u64,
                        numerator: 0,
                        denominator: 1,
                    },
                )
                .unwrap();
            assert_eq!(transport.position_at_output_phase(point).unwrap(), expected);
        }
        let fractional = phase
            .point(
                ClockPoint {
                    domain: LOGICAL,
                    timestamp: Timestamp::from_nanos(logical.timestamp.as_nanos() + 333_333),
                },
                SourcePosition {
                    frame: identity.initial_playback(),
                    numerator: 1,
                    denominator: 3,
                },
            )
            .unwrap();
        assert_eq!(fractional.elapsed_nanos(), 333_333);
        // Whole signed preroll is scaled once; separately rounded anchors
        // and advances would differ by one nanosecond before the song start.
        let (before_preroll, after_preroll) = if rate == Rate::NORMAL {
            (48_333_333, 50_333_333)
        } else if rate == Rate::new(3, 2).unwrap() {
            (47_500_000, 50_499_999)
        } else {
            (49_166_667, 50_166_666)
        };
        assert_eq!(
            transport.position_at_output_phase(fractional).unwrap(),
            Timestamp::from_nanos(before_preroll)
        );
        let after = phase
            .point(
                ClockPoint {
                    domain: LOGICAL,
                    timestamp: Timestamp::from_nanos(logical.timestamp.as_nanos() + 2_333_333),
                },
                SourcePosition {
                    frame: identity.initial_playback() + 2,
                    numerator: 1,
                    denominator: 3,
                },
            )
            .unwrap();
        assert_eq!(after.elapsed_nanos(), 2_333_333);
        assert_eq!(
            transport.position_at_output_phase(after).unwrap(),
            Timestamp::from_nanos(after_preroll)
        );
        assert!(phase
            .point(
                logical,
                SourcePosition {
                    frame: identity.initial_playback() - 1,
                    numerator: 0,
                    denominator: 1
                }
            )
            .is_err());
        let foreign = ClockPoint {
            domain: RAW,
            timestamp: logical.timestamp,
        };
        assert!(phase
            .point(
                foreign,
                SourcePosition {
                    frame: 5,
                    numerator: 0,
                    denominator: 1
                }
            )
            .is_err());
        assert!(transport.position_at(logical.timestamp).is_err());
        let wrong = beatkernel::transport::OutputPhaseBasis::new(LOGICAL, 92, 1, 5, 1000)
            .unwrap()
            .point(
                logical,
                SourcePosition {
                    frame: 5,
                    numerator: 0,
                    denominator: 1,
                },
            )
            .unwrap();
        assert!(transport.position_at_output_phase(wrong).is_err());
    }
}
