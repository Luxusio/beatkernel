//! Deferred preparation/allocator fixtures using actual parsed BMS and core judges.
use crate::{input_sounds::InputSoundPlan, local_runtime::VoiceAllocator};
use beatkernel::{
    audio::{AudioCommand, SampleId, VoiceId},
    chart::{Beat, ObjectId},
    input::{
        ButtonEvent, ButtonState, ContactId, DeviceId, EventMeta, GameControlId, GameInputEvent,
        PhysicalControlId, PhysicalInputEvent, Position2, TouchEvent, TouchPhase,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow},
    runtime::{input_sound::InputSoundMarker, SoundBinding},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::Rate,
};
use beatkernel_bms::{parse, BmsChart, BmsInputMode, ParseOptions};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn chart() -> BmsChart {
    parse(
        "#BPM 120\n#BPM0a 240\n#STOPzz 48\n\
        #WAV0A upper.wav\n#WAV0a lower.wav\n#WAVzz last.wav\n#WAV01 unused.wav\n\
        #VOLWAV 25\n#00002:0.75\n#00008:000a00\n#00009:00zz00\n\
        #00049:zz0000\n#00032:0a000A\n#00031:0A0azz\n#00141:0a\n\
        #00011:00000A\n#00001:000a00\n#BASE 62",
        ParseOptions::default(),
    )
    .unwrap()
}
fn sound(object: u64, sample: u64, voice: u64, gain: f32) -> SoundBinding {
    SoundBinding {
        object: ObjectId(object),
        stage: JudgeStage::Instant,
        sample: SampleId(sample),
        voice: VoiceId(voice),
        gain,
    }
}
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn marker(control: u32, at: i64, sample: u64, voice: u64, gain: f32) -> InputSoundMarker {
    InputSoundMarker {
        control: GameControlId(control),
        at: ts(at),
        sample: SampleId(sample),
        voice: VoiceId(voice),
        gain,
    }
}
fn button(sequence: u64, state: ButtonState) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(0x11),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(u64::MAX),
                ClockPoint {
                    domain: ClockDomainId(9),
                    timestamp: ts(-17),
                },
                sequence,
            ),
            control: PhysicalControlId::keyboard(91),
            state,
        }),
    }
}
fn contact(sequence: u64, phase: TouchPhase) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(0x11),
        physical: PhysicalInputEvent::Touch(TouchEvent {
            meta: EventMeta::new(
                DeviceId(u64::MAX),
                ClockPoint {
                    domain: ClockDomainId(9),
                    timestamp: ts(-17),
                },
                sequence,
            ),
            control: PhysicalControlId::keyboard(91),
            contact: ContactId(u64::MAX),
            phase,
            position: Position2 { x: 3.0, y: -2.0 },
            pressure: Some(0.5),
        }),
    }
}

#[test]
fn actual_radix_tempo_stop_and_gain_plan_preserves_original_samples_and_assigns_lane_ordered_voices()
 {
    let source = chart();
    let original = source.clone();
    let compiled = source.compile().unwrap();
    assert_eq!(compiled.chart.objects().len(), 1);
    assert_eq!(compiled.chart.objects()[0].time.start, ts(1_000_000_000));
    assert_eq!(compiled.bgm.len(), 1);
    assert_eq!(
        (compiled.bgm[0].sample, compiled.bgm[0].at),
        (SampleId(36), ts(500_000_000))
    );
    let occupied = [sound(
        source.notes[0].object.0,
        10,
        9_007_199_254_741_000,
        -0.5,
    )];
    let bgm = [play(36, 9_007_199_254_741_004, 500_000_000, 2.0)];
    let before_sounds = occupied;
    let before_bgm = bgm;
    let plan = InputSoundPlan::prepare(&source, &occupied, &bgm, 7).unwrap();
    assert_eq!(plan.samples(), [SampleId(10), SampleId(36), SampleId(3843)]);
    assert_eq!(plan.markers().len(), 7);
    // Expected times are pre-STOP: one beat at 120 BPM, then a 250 ms STOP,
    // then 250 ms per beat. Invisible rows were deliberately not lane ordered.
    let mut actual = plan.markers().to_vec();
    actual.sort_by_key(|entry| (entry.control.0, entry.at));
    assert_eq!(
        actual,
        [
            marker(0x11, 0, 10, 9_007_199_254_741_005, 0.25),
            marker(0x11, 500_000_000, 36, 9_007_199_254_741_005, 0.25),
            marker(0x11, 1_000_000_000, 3843, 9_007_199_254_741_005, 0.25),
            marker(0x12, 0, 36, 9_007_199_254_741_006, 0.25),
            marker(0x12, 1_000_000_000, 10, 9_007_199_254_741_006, 0.25),
            marker(0x21, 1_250_000_000, 36, 9_007_199_254_741_007, 0.25),
            marker(0x29, 0, 3843, 9_007_199_254_741_008, 0.25),
        ]
    );
    assert_eq!(source, original);
    assert_eq!(
        source.compile().unwrap(),
        compiled,
        "selections create neither notes nor automatic BGM"
    );
    assert_eq!(occupied, before_sounds);
    assert_eq!(bgm, before_bgm);
    let detached = plan.timeline();
    let another = plan.timeline();
    drop(plan);
    assert_eq!(detached, another);
    assert_eq!(
        detached.command_for(GameControlId(0x21), ts(1_249_999_999), ts(8)),
        None
    );
    assert_eq!(
        detached.command_for(GameControlId(0x21), ts(1_250_000_000), ts(-8)),
        Some(play(36, 9_007_199_254_741_007, -8, 0.25))
    );
}

#[test]
fn prepared_timeline_uses_real_fresh_press_ownership_and_hit_precedence_at_original_song_time() {
    let source = chart();
    let plan = InputSoundPlan::prepare(&source, &[sound(1, 10, 44, 1.0)], &[], 7).unwrap();
    let timeline = plan.timeline();
    let mut judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules_with_input_mode(BmsInputMode::ButtonOrContact),
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
    let down = button(1, ButtonState::Down);
    let fresh = judge.is_fresh_press(&down);
    let results = judge.push_input(&down, ts(0)).unwrap();
    assert!(fresh && results.is_empty());
    assert_eq!(
        timeline.command_for_press(&down, fresh, ts(0), ts(9_007_199_254_740_993), &results),
        Some(play(10, 45, 9_007_199_254_740_993, 0.25))
    );
    let duplicate = button(2, ButtonState::Down);
    let fresh = judge.is_fresh_press(&duplicate);
    let results = judge.push_input(&duplicate, ts(499_999_999)).unwrap();
    assert!(!fresh);
    assert_eq!(
        timeline.command_for_press(&duplicate, fresh, ts(499_999_999), ts(10), &results),
        None
    );
    judge
        .push_input(&button(3, ButtonState::Up), ts(499_999_999))
        .unwrap();
    let down = button(4, ButtonState::Down);
    let fresh = judge.is_fresh_press(&down);
    let results = judge.push_input(&down, ts(500_000_000)).unwrap();
    assert_eq!(
        timeline.command_for_press(&down, fresh, ts(500_000_000), ts(-123), &results),
        Some(play(36, 45, -123, 0.25)),
        "output time must not select the keysound"
    );
    judge
        .push_input(&button(5, ButtonState::Up), ts(500_000_000))
        .unwrap();
    let touch = contact(6, TouchPhase::Down);
    let fresh = judge.is_fresh_press(&touch);
    let results = judge.push_input(&touch, ts(1_000_000_000)).unwrap();
    assert!(fresh);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].stage, JudgeStage::Instant);
    assert!(matches!(results[0].outcome, JudgeOutcome::Hit { .. }));
    assert_eq!(
        timeline.command_for_press(&touch, fresh, ts(1_000_000_000), ts(0), &results),
        None
    );
    let checkpoint = judge.snapshot().unwrap();
    judge
        .push_input(&contact(7, TouchPhase::Cancel), ts(1_000_000_000))
        .unwrap();
    assert!(judge.is_fresh_press(&touch));
    judge.restore(&checkpoint).unwrap();
    assert!(!judge.is_fresh_press(&touch));
    let down = button(8, ButtonState::Down);
    let fresh = judge.is_fresh_press(&down);
    let results = judge.push_input(&down, ts(1_000_000_000)).unwrap();
    assert!(fresh && results.is_empty());
    assert_eq!(
        timeline.command_for_press(&down, fresh, ts(1_000_000_000), ts(i64::MAX), &results),
        Some(play(3843, 45, i64::MAX, 0.25))
    );
}

#[test]
fn invalid_capacity_configuration_source_and_voice_exhaustion_return_no_partial_plan() {
    let source = chart();
    let before = source.clone();
    let empty = parse("#BPM 120\n#WAV01 unused", ParseOptions::default()).unwrap();
    for cap in [0, 100_001, usize::MAX] {
        assert!(InputSoundPlan::prepare(&source, &[], &[], cap).is_err());
        assert!(InputSoundPlan::prepare(&empty, &[], &[], cap).is_err());
    }
    assert!(InputSoundPlan::prepare(&source, &[], &[], 6).is_err());
    assert!(InputSoundPlan::prepare(&source, &[], &[], 7).is_ok());
    for gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for chart in [&source, &empty] {
            assert!(InputSoundPlan::prepare(chart, &[sound(1, 10, 1, gain)], &[], 7).is_err());
            assert!(InputSoundPlan::prepare(chart, &[], &[play(1, 1, 0, gain)], 7).is_err());
        }
    }
    for command in [
        AudioCommand::Stop {
            voice: VoiceId(1),
            at: ts(0),
        },
        AudioCommand::SetRate {
            rate: Rate::NORMAL,
            at: ts(0),
        },
        AudioCommand::Seek {
            song_time: ts(0),
            at: ts(0),
        },
    ] {
        assert!(InputSoundPlan::prepare(&source, &[], &[command], 7).is_err());
        assert!(InputSoundPlan::prepare(&empty, &[], &[command], 7).is_err());
    }
    let maximum_sound = [sound(1, 1, u64::MAX, -0.0)];
    let maximum_bgm = [play(1, u64::MAX, i64::MAX, -1.0)];
    let no_markers = InputSoundPlan::prepare(&empty, &maximum_sound, &maximum_bgm, 1).unwrap();
    assert!(no_markers.markers().is_empty() && no_markers.samples().is_empty());
    assert_eq!(
        no_markers
            .timeline()
            .command_for(GameControlId(0x11), ts(0), ts(0)),
        None
    );
    assert!(InputSoundPlan::prepare(&source, &maximum_sound, &[], 7).is_err());
    assert!(InputSoundPlan::prepare(&source, &[], &maximum_bgm, 7).is_err());
    let one_lane = parse("#WAV01 x\n#00031:0101", ParseOptions::default()).unwrap();
    let last =
        InputSoundPlan::prepare(&one_lane, &[sound(1, 1, u64::MAX - 1, 1.0)], &[], 2).unwrap();
    assert_eq!(
        last.markers()
            .iter()
            .map(|entry| entry.voice)
            .collect::<Vec<_>>(),
        [VoiceId(u64::MAX); 2]
    );
    assert!(InputSoundPlan::prepare(&source, &[sound(1, 1, u64::MAX - 1, 1.0)], &[], 7).is_err());
    let mut bad_grid = source.clone();
    bad_grid.invisible_ticks_per_beat = 0;
    let mut missing_sample = source.clone();
    missing_sample.invisible[0].sample = SampleId(3844);
    let mut repeated = source.clone();
    repeated.invisible.push(repeated.invisible[0]);
    let mut overflow = source.clone();
    overflow.invisible[0].beat = Beat::new(i64::MAX).unwrap();
    let mut bad_gain = source.clone();
    bad_gain.metadata.insert("VOLWAV".into(), "NaN".into());
    for invalid in [bad_grid, missing_sample, repeated, overflow, bad_gain] {
        let preserved = invalid.clone();
        assert!(InputSoundPlan::prepare(&invalid, &[], &[], 100).is_err());
        assert_eq!(invalid, preserved);
    }
    assert_eq!(source, before);
    assert_eq!(
        InputSoundPlan::prepare(&source, &[], &[], 7)
            .unwrap()
            .samples(),
        [SampleId(10), SampleId(36), SampleId(3843)]
    );
}

#[test]
fn consecutive_gameplay_and_fallback_remaps_keep_local_ranges_disjoint_and_exhaustion_atomic() {
    let plan = InputSoundPlan::prepare(&chart(), &[sound(1, 1, 44, 1.0)], &[], 7).unwrap();
    let select = |control, at| {
        *plan
            .markers()
            .iter()
            .find(|entry| entry.control == GameControlId(control) && entry.at == ts(at))
            .unwrap()
    };
    // Non-sorted old voices pin first-occurrence allocation and lane replacement.
    let original_markers = vec![
        select(0x29, 0),
        select(0x11, 0),
        select(0x11, 500_000_000),
        select(0x12, 0),
    ];
    let original_sounds = vec![
        sound(1, 10, 77, 0.5),
        sound(2, 36, 77, -0.5),
        sound(3, 3843, 5, 1.0),
    ];
    let mut allocator = VoiceAllocator::new(100); // Actual caller has reserved BGM 0..99.
    for (expected_sounds, expected_markers) in [
        ([100, 100, 101], [102, 103, 103, 104]),
        ([105, 105, 106], [107, 108, 108, 109]),
        ([110, 110, 111], [112, 113, 113, 114]),
    ] {
        let mut sounds = original_sounds.clone();
        let mut markers = original_markers.clone();
        allocator.remap(&mut sounds).unwrap();
        allocator.remap_input_sounds(&mut markers).unwrap();
        assert_eq!(
            sounds.iter().map(|entry| entry.voice.0).collect::<Vec<_>>(),
            expected_sounds
        );
        assert_eq!(
            markers
                .iter()
                .map(|entry| entry.voice.0)
                .collect::<Vec<_>>(),
            expected_markers
        );
        for (actual, original) in markers.iter().zip(&original_markers) {
            let mut expected = *original;
            expected.voice = actual.voice;
            assert_eq!(*actual, expected);
        }
        for (actual, original) in sounds.iter().zip(&original_sounds) {
            let mut expected = *original;
            expected.voice = actual.voice;
            assert_eq!(*actual, expected);
        }
    }
    let mut allocator = VoiceAllocator::new(u64::MAX - 1);
    let mut too_many = original_markers.clone();
    let before = too_many.clone();
    assert!(allocator.remap_input_sounds(&mut too_many).is_err());
    assert_eq!(too_many, before);
    allocator.remap_input_sounds(&mut []).unwrap();
    let mut final_gameplay = vec![sound(1, 1, 19, 1.0)];
    allocator.remap(&mut final_gameplay).unwrap();
    assert_eq!(
        final_gameplay[0].voice,
        VoiceId(u64::MAX - 1),
        "failed fallback remap consumed no IDs"
    );
    let mut repeated_voice = vec![original_markers[1], original_markers[2]];
    allocator.remap_input_sounds(&mut repeated_voice).unwrap();
    assert_eq!(
        repeated_voice
            .iter()
            .map(|entry| entry.voice)
            .collect::<Vec<_>>(),
        [VoiceId(u64::MAX); 2]
    );
    allocator.remap_input_sounds(&mut []).unwrap();
    allocator.remap(&mut []).unwrap();
    let mut refused_marker = [original_markers[1]];
    let before_marker = refused_marker;
    assert!(allocator.remap_input_sounds(&mut refused_marker).is_err());
    assert_eq!(refused_marker, before_marker);
    let mut refused_sound = [original_sounds[0]];
    let before_sound = refused_sound;
    assert!(allocator.remap(&mut refused_sound).is_err());
    assert_eq!(refused_sound, before_sound);
}
