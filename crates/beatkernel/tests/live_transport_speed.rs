//! Exact opt-in song timing through the existing Transport and Runtime owners.
//! Literal expectations are independent of the production rate-mapping helpers.
use beatkernel::{
    time::{Duration, Timestamp},
    transport::{
        LiveTransportControlError, LiveTransportSetupError, PrerollTransportSetup, Rate, Transport,
        TransportAnchor, TransportError,
    },
};

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn ns(n: i64) -> Duration {
    Duration::from_nanos(n)
}
fn rate(n: i64, d: u64) -> Rate {
    Rate::new(n, d).unwrap()
}
fn live(origin: i64, start: i64, preroll: i64, speed: Rate) -> Transport {
    Transport::new_with_preroll(ts(origin), ts(start), ns(preroll), speed, None).unwrap()
}

#[test]
fn four_rates_map_original_song_with_output_preroll_and_original_start() {
    for (speed, initial, after) in [
        (rate(1, 2), 48_500_000, 52_000_000),
        (Rate::NORMAL, 47_000_000, 54_000_000),
        (rate(2, 1), 44_000_000, 58_000_000),
        (rate(3, 2), 45_500_000, 56_000_000),
    ] {
        let owner = live(100, 50_000_000, 3_000_000, speed);
        assert_eq!(owner.position_at(ts(100)), Ok(ts(initial)));
        assert_eq!(owner.position_at(ts(3_000_100)), Ok(ts(50_000_000)));
        assert_eq!(owner.position_at(ts(7_000_100)), Ok(ts(after)));
        assert_eq!(owner.position_at(ts(99)), Err(TransportError::BeforeOrigin));
        assert_eq!(
            owner.anchor(),
            TransportAnchor {
                host_time: ts(100),
                song_time: ts(initial),
                rate: speed,
            }
        );
        let history: &[TransportAnchor] = owner.anchors();
        assert_eq!(history, &[owner.anchor()]);
    }
}

#[test]
fn cold_setup_snapshot_is_normalized_and_complete() {
    let owner =
        Transport::new_with_preroll(ts(-20), ts(50), ns(3), rate(6, 4), Some(ts(90))).unwrap();
    let setup: PrerollTransportSetup = owner.preroll_setup().unwrap();
    let copy = setup;
    assert_eq!(copy.logical_origin, ts(-20));
    assert_eq!(copy.original_start, ts(50));
    assert_eq!(copy.output_preroll, ns(3));
    assert_eq!(copy.positive_rate, rate(3, 2));
    assert_eq!(copy.original_end, Some(ts(90)));
    assert!(Transport::new(ts(-20), ts(46), rate(3, 2))
        .preroll_setup()
        .is_none());
}

#[test]
fn one_truncation_crosses_fractional_preroll_without_rounding_the_anchor_twice() {
    for (preroll, speed, expected) in [
        (1, rate(1, 2), [0, 0, 0, 1]),
        (3, rate(3, 2), [-4, -3, -1, 0]),
    ] {
        let owner = live(70, 0, preroll, speed);
        for (elapsed, song) in expected.into_iter().enumerate() {
            assert_eq!(owner.position_at(ts(70 + elapsed as i64)), Ok(ts(song)));
        }
    }
}

#[test]
fn acknowledged_elapsed_may_lag_cutoff_and_resume_keeps_fractional_phase() {
    let mut owner = live(0, 0, 1, rate(1, 2));
    owner.pause_at_output_elapsed(ts(100), 2).unwrap();
    assert_eq!(owner.anchor().song_time, ts(0));
    assert!(owner.is_paused());
    assert_eq!(owner.position_at(ts(109)), Ok(ts(0)));
    // Older inputs continue to use the original phase, not the frozen boundary.
    assert_eq!(owner.position_at(ts(3)), Ok(ts(1)));
    owner.resume_preroll(ts(110)).unwrap();
    assert_eq!(owner.position_at(ts(110)), Ok(ts(0)));
    assert_eq!(owner.position_at(ts(111)), Ok(ts(1)));
    assert_eq!(owner.anchor().rate, rate(1, 2));

    let mut faster = live(0, 0, 3, rate(3, 2));
    faster.pause_at_output_elapsed(ts(10), 1).unwrap();
    assert_eq!(faster.position_at(ts(19)), Ok(ts(-3)));
    faster.resume_preroll(ts(20)).unwrap();
    assert_eq!(faster.position_at(ts(21)), Ok(ts(-1)));
    assert_eq!(faster.position_at(ts(0)), Ok(ts(-4)));
}

#[test]
fn repeated_odd_gaps_preserve_phase_and_historical_integer_snapshots() {
    let mut owner = live(0, 0, 1, rate(1, 2));
    owner.pause_at_output_elapsed(ts(2), 2).unwrap();
    owner.resume_preroll(ts(5)).unwrap();
    assert_eq!(owner.position_at(ts(6)), Ok(ts(1)));
    owner.pause_at_output_elapsed(ts(6), 3).unwrap();
    owner.resume_preroll(ts(9)).unwrap();
    assert_eq!(owner.position_at(ts(10)), Ok(ts(1)));
    owner.pause_at_output_elapsed(ts(10), 4).unwrap();
    owner.resume_preroll(ts(13)).unwrap();
    assert_eq!(owner.position_at(ts(14)), Ok(ts(2)));
    for (host, song) in [
        (0, 0),
        (1, 0),
        (2, 0),
        (4, 0),
        (5, 0),
        (6, 1),
        (8, 1),
        (9, 1),
        (10, 1),
        (12, 1),
        (13, 1),
    ] {
        assert_eq!(owner.position_at(ts(host)), Ok(ts(song)));
    }
    for anchor in owner.anchors() {
        assert_eq!(owner.position_at(anchor.host_time), Ok(anchor.song_time));
    }
    assert_eq!(owner.anchors().len(), 7);
}

#[test]
fn duplicate_controls_validate_elapsed_and_chronology_without_new_anchors() {
    let mut owner = live(0, 0, 1, rate(1, 2));
    owner.pause_at_output_elapsed(ts(10), 2).unwrap();
    let count = owner.anchors().len();
    owner.pause_at_output_elapsed(ts(12), 2).unwrap();
    assert_eq!(owner.anchors().len(), count);
    let frozen = owner.clone();
    assert!(owner.pause_at_output_elapsed(ts(13), 3).is_err());
    assert_eq!(owner, frozen);
    assert_eq!(
        owner.resume_preroll(ts(11)),
        Err(LiveTransportControlError::NonMonotonicHost)
    );
    assert_eq!(owner, frozen);
    owner.resume_preroll(ts(14)).unwrap();
    let count = owner.anchors().len();
    owner.resume_preroll(ts(16)).unwrap();
    assert_eq!(owner.anchors().len(), count);
    assert_eq!(owner.position_at(ts(17)), Ok(ts(2)));
    let running = owner.clone();
    assert_eq!(
        owner.pause_at_output_elapsed(ts(17), 3),
        Err(LiveTransportControlError::RegressingElapsed)
    );
    assert_eq!(owner, running);
}

#[test]
fn exact_controls_refuse_negative_regressing_and_legacy_phase_atomically() {
    let mut owner = live(0, 0, 1, rate(1, 2));
    let before = owner.clone();
    assert_eq!(
        owner.pause_at_output_elapsed(ts(1), -1),
        Err(LiveTransportControlError::NegativeElapsed)
    );
    assert_eq!(owner, before);
    assert_eq!(
        owner.pause_at_output_elapsed(ts(-1), 0),
        Err(LiveTransportControlError::NonMonotonicHost)
    );
    assert_eq!(owner, before);
    owner.set_rate(ts(10), rate(1, 2)).unwrap();
    let before = owner.clone();
    assert_eq!(
        owner.pause_at_output_elapsed(ts(11), 9),
        Err(LiveTransportControlError::RegressingElapsed)
    );
    assert_eq!(owner, before);
    owner.pause_at_output_elapsed(ts(11), 10).unwrap();
    assert_eq!(owner.anchor().song_time, ts(4));

    let mut legacy = Transport::new(ts(0), ts(0), Rate::NORMAL);
    let before = legacy.clone();
    assert_eq!(
        legacy.pause_at_output_elapsed(ts(1), 1),
        Err(LiveTransportControlError::MissingPrerollSetup)
    );
    assert_eq!(
        legacy.resume_preroll(ts(1)),
        Err(LiveTransportControlError::MissingPrerollSetup)
    );
    assert_eq!(legacy, before);
}

#[test]
fn no_op_rate_retains_exact_phase_and_advances_command_chronology() {
    let mut owner = live(0, 0, 1, rate(1, 2));
    let setup = owner.preroll_setup();
    owner.set_rate(ts(2), rate(2, 4)).unwrap();
    assert_eq!(owner.anchors().len(), 1);
    assert_eq!(owner.preroll_setup(), setup);
    assert_eq!(owner.position_at(ts(3)), Ok(ts(1)));
    let before = owner.clone();
    assert_eq!(
        owner.set_rate(ts(1), rate(1, 2)),
        Err(TransportError::NonMonotonicHost)
    );
    assert_eq!(owner, before);
}

#[test]
fn real_rate_change_quantizes_new_segment_without_rewriting_history() {
    let mut owner = live(0, 0, 1, rate(1, 2));
    owner.set_rate(ts(2), rate(2, 1)).unwrap();
    assert!(owner.preroll_setup().is_none());
    assert_eq!(owner.position_at(ts(1)), Ok(ts(0)));
    assert_eq!(owner.position_at(ts(2)), Ok(ts(0)));
    assert_eq!(owner.position_at(ts(3)), Ok(ts(2)));
    let before = owner.clone();
    assert!(owner.pause_at_output_elapsed(ts(4), 4).is_err());
    assert_eq!(owner, before);
}

#[test]
fn seek_resets_only_current_phase_and_retains_legacy_pause_resume_semantics() {
    let mut owner = live(0, 0, 3, rate(3, 2));
    owner.pause_at_output_elapsed(ts(1), 1).unwrap();
    owner.seek(ts(5), ts(10)).unwrap();
    assert!(owner.preroll_setup().is_none());
    assert_eq!(owner.position_at(ts(0)), Ok(ts(-4)));
    assert_eq!(owner.position_at(ts(2)), Ok(ts(-3)));
    assert_eq!(owner.position_at(ts(5)), Ok(ts(10)));
    let before = owner.clone();
    assert!(owner.resume_preroll(ts(6)).is_err());
    assert_eq!(owner, before);
    owner.resume(ts(6)).unwrap();
    assert_eq!(owner.position_at(ts(7)), Ok(ts(11)));
    assert_eq!(owner.position_at(ts(8)), Ok(ts(13)));
}

#[test]
fn legacy_public_anchor_layout_and_integer_quantization_are_preserved() {
    let anchor = TransportAnchor {
        host_time: ts(10),
        song_time: ts(3),
        rate: rate(1, 2),
    };
    let mut owner = Transport::new(anchor.host_time, anchor.song_time, anchor.rate);
    assert_eq!(owner.anchors(), &[anchor]);
    owner.pause(ts(11)).unwrap();
    owner.resume(ts(14)).unwrap();
    assert_eq!(owner.position_at(ts(15)), Ok(ts(3)));
    assert_eq!(owner.position_at(ts(16)), Ok(ts(4)));
    assert!(owner.preroll_setup().is_none());
}

#[test]
fn setup_rejects_nonpositive_rate_negative_preroll_invalid_end_and_negative_overflow() {
    for speed in [Rate::ZERO, Rate::REVERSE, rate(i64::MIN, u64::MAX)] {
        assert_eq!(
            Transport::new_with_preroll(ts(0), ts(0), ns(0), speed, None),
            Err(LiveTransportSetupError::NonPositiveRate)
        );
    }
    assert_eq!(
        Transport::new_with_preroll(ts(0), ts(0), ns(-1), Rate::NORMAL, None),
        Err(LiveTransportSetupError::NegativePreroll)
    );
    assert_eq!(
        Transport::new_with_preroll(ts(0), ts(2), ns(0), Rate::NORMAL, Some(ts(1))),
        Err(LiveTransportSetupError::InvalidEnd)
    );
    for end in [None, Some(Timestamp::MAX)] {
        assert_eq!(
            Transport::new_with_preroll(ts(0), Timestamp::MIN, ns(1), Rate::NORMAL, end),
            Err(LiveTransportSetupError::Overflow)
        );
    }
    assert!(Transport::new_with_preroll(ts(0), ts(2), ns(0), Rate::NORMAL, Some(ts(2))).is_ok());
}

#[test]
fn full_signed_host_span_and_wide_song_cancellation_narrow_only_final_value() {
    let owner = live(i64::MIN, i64::MIN, 0, Rate::NORMAL);
    assert_eq!(owner.position_at(Timestamp::MIN), Ok(Timestamp::MIN));
    assert_eq!(owner.position_at(ts(0)), Ok(ts(0)));
    assert_eq!(owner.position_at(Timestamp::MAX), Ok(Timestamp::MAX));
    let doubled = live(0, i64::MIN, 0, rate(2, 1));
    assert_eq!(doubled.position_at(Timestamp::MAX), Ok(ts(i64::MAX - 1)));
    let largest = live(0, 0, 0, rate(i64::MAX, u64::MAX));
    assert_eq!(largest.position_at(ts(2)), Ok(ts(0)));
    assert_eq!(largest.position_at(ts(3)), Ok(ts(1)));
    let cancelling_preroll = live(0, i64::MAX, i64::MAX, rate(2, 1));
    assert_eq!(cancelling_preroll.position_at(ts(0)), Ok(ts(-i64::MAX)));
    assert_eq!(
        cancelling_preroll.position_at(Timestamp::MAX),
        Ok(Timestamp::MAX)
    );
}

#[test]
fn finite_end_caps_wide_positive_overflow_but_uncapped_controls_refuse_atomically() {
    let mut capped = Transport::new_with_preroll(
        Timestamp::MIN,
        ts(50),
        ns(0),
        rate(i64::MAX, 1),
        Some(ts(70)),
    )
    .unwrap();
    assert_eq!(capped.position_at(Timestamp::MAX), Ok(ts(70)));
    capped
        .pause_at_output_elapsed(Timestamp::MAX, i128::MAX)
        .unwrap();
    assert_eq!(capped.anchor().song_time, ts(70));
    assert_eq!(capped.position_at(Timestamp::MAX), Ok(ts(70)));
    let mut uncapped = live(0, 50, 0, rate(2, 1));
    assert_eq!(
        uncapped.position_at(Timestamp::MAX),
        Err(TransportError::Overflow)
    );
    let before = uncapped.clone();
    assert_eq!(
        uncapped.pause_at_output_elapsed(ts(1), i128::MAX),
        Err(LiveTransportControlError::Overflow)
    );
    assert_eq!(uncapped, before);
    uncapped.pause_at_output_elapsed(ts(1), 1).unwrap();
    assert_eq!(uncapped.anchor().song_time, ts(52));
}

#[test]
fn quantized_and_zero_song_span_finite_caps_remain_numeric_mapping_only() {
    let quantized =
        Transport::new_with_preroll(ts(0), ts(0), ns(0), rate(2, 1), Some(ts(1))).unwrap();
    assert_eq!(quantized.position_at(ts(0)), Ok(ts(0)));
    assert_eq!(quantized.position_at(ts(1)), Ok(ts(1)));
    let zero = Transport::new_with_preroll(ts(0), ts(4), ns(0), rate(3, 2), Some(ts(4))).unwrap();
    assert_eq!(zero.position_at(ts(0)), Ok(ts(4)));
    assert_eq!(zero.position_at(Timestamp::MAX), Ok(ts(4)));
    // No completion, command-credit or physical-presentation authority exists here.
    assert_eq!(zero.anchors().len(), 1);
}

mod runtime_fixture {
    use super::*;
    use beatkernel::{
        audio::{command_queue, AudioCommand, SampleId, VoiceId},
        chart::{
            Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
        },
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
            GameControlId, PhysicalControlId, PhysicalInputEvent,
        },
        interaction::InstantEvaluator,
        judge::{
            HazardId, HazardMarker, HazardOutcome, HazardTimeline, JudgeEngine, JudgeGrade,
            JudgeProfile, JudgeStage, JudgeWindow, Rule,
        },
        runtime::{Runtime, RuntimeProcessingClock, SoundBinding},
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint},
    };

    fn point(domain: u32, nanos: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: ts(nanos),
        }
    }
    struct Clocks;
    impl ClockMapper for Clocks {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            if from.domain == ClockDomainId(1) && to == ClockDomainId(2) {
                from.timestamp.checked_add(ns(1000))
            } else {
                None
            }
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Estimated { max_error: ns(2) }
        }
    }

    #[test]
    fn actual_runtime_judges_original_song_preserves_input_provenance_and_raw_audio_at() {
        for (speed, original_ms) in [
            (rate(1, 2), 7),
            (Rate::NORMAL, 14),
            (rate(2, 1), 28),
            (rate(3, 2), 21),
        ] {
            let mut chart = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
            chart.objects.push(SourceObject {
                id: ObjectId(1),
                start: Beat::new(original_ms).unwrap(),
                end: None,
                interaction: InteractionId(1),
                visual: VisualId(1),
                audio: None,
                metadata: ObjectMetadata::default(),
            });
            let mut judge = JudgeEngine::new(
                chart.compile().unwrap(),
                vec![Rule {
                    interaction: InteractionId(1),
                    control: GameControlId(1),
                    evaluator: Box::new(InstantEvaluator),
                }],
                JudgeProfile::new(
                    vec![JudgeWindow {
                        grade: JudgeGrade(1),
                        early: Duration::ZERO,
                        late: Duration::ZERO,
                    }],
                    Duration::ZERO,
                )
                .unwrap(),
            )
            .unwrap();
            judge
                .configure_hazards(
                    HazardTimeline::new(
                        vec![HazardMarker {
                            id: HazardId(17),
                            at: ts(original_ms * 1_000_000),
                            control: GameControlId(1),
                            value: 5,
                        }],
                        1,
                    )
                    .unwrap(),
                )
                .unwrap();
            let bindings = BindingMap::from_bindings([Binding {
                device: DeviceSelector::Exact(DeviceId(1)),
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(1),
            }])
            .unwrap();
            let (producer, mut consumer) = command_queue(4).unwrap();
            let mut runtime = Runtime::new(
                ClockDomainId(2),
                ClockDomainId(3),
                live(1000, 0, 3_000_000, speed),
                bindings,
                judge,
                producer,
                vec![SoundBinding {
                    object: ObjectId(1),
                    stage: JudgeStage::Instant,
                    sample: SampleId(1),
                    voice: VoiceId(1),
                    gain: 1.0,
                }],
                4,
            )
            .unwrap();
            runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
            let input = PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(1), point(1, 17_000_000), 1),
                control: PhysicalControlId::keyboard(4),
                state: ButtonState::Down,
            });
            let report = runtime
                .process_input(input, &Clocks, point(3, 17_000_000))
                .unwrap();
            assert_eq!(report.song_time, ts(original_ms * 1_000_000));
            assert_eq!(report.audio_at, point(3, 17_000_000));
            let normalized = report.input.as_ref().unwrap().meta();
            assert_eq!(normalized.timestamp, ts(17_001_000));
            assert_eq!(normalized.clock_domain, ClockDomainId(2));
            assert_eq!(normalized.original_clock_point, Some(point(1, 17_000_000)));
            assert_eq!(report.bound_inputs.len(), 1);
            assert_eq!(report.judge_events.len(), 1);
            assert_eq!(report.hazard_events.len(), 1);
            assert_eq!(report.hazard_events[0].at, ts(original_ms * 1_000_000));
            assert_eq!(report.hazard_events[0].outcome, HazardOutcome::Triggered);
            assert_eq!(report.hazard_events[0].input, Some(*normalized));
            assert!(report.judge_error.is_none());
            assert!(report.audio_failures.is_empty());
            let expected = AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: ts(17_000_000),
                gain: 1.0,
            };
            assert_eq!(report.audio_commands, [expected]);
            assert_eq!(consumer.try_pop().unwrap(), expected);
            assert!(consumer.try_pop().is_err());
        }
    }
}
