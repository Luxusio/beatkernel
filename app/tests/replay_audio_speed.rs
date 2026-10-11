#[path = "support/replay_speed.rs"]
mod fixture;
use beatkernel::{
    audio::{AudioCommand, SampleId},
    input::ButtonState,
    replay::codec::encode_replay,
    time::{Duration, Timestamp},
    transport::Rate,
};
use beatkernel_bms_runtime::{
    gauge::GaugeProfile,
    replay_audio::{
        plan_audio, plan_audio_with_rate, plan_section_audio, plan_section_audio_with_rate,
    },
    section_start::prepare_at,
};
use fixture::*;

fn times(commands: &[AudioCommand]) -> Vec<i64> {
    commands.iter().map(|c| c.at().as_nanos()).collect()
}
#[test]
fn literal_rational_mapping_preserves_original_recording_and_full_judgment() {
    let p = main_prepared();
    let file = captured(&p, 0, Some(4_000_000_001));
    let bytes = encode_replay(&file, limits()).unwrap();
    let legacy = plan_audio(
        &p,
        file.clone(),
        limits(),
        point(100),
        Duration::from_nanos(7),
    )
    .unwrap();
    assert_eq!(
        times(&legacy.commands),
        [1_000_000_107, 1_000_000_107, 2_000_000_107]
    );
    for (num, den, expected) in [
        (1, 2, vec![100, 2_000_000_107, 2_000_000_107, 4_000_000_107]),
        (1, 1, vec![1_000_000_107, 1_000_000_107, 2_000_000_107]),
        (2, 1, vec![100, 500_000_107, 500_000_107, 1_000_000_107]),
        (3, 2, vec![100, 666_666_773, 666_666_773, 1_333_333_440]),
        (6, 4, vec![100, 666_666_773, 666_666_773, 1_333_333_440]),
        (8, 8, vec![1_000_000_107, 1_000_000_107, 2_000_000_107]),
    ] {
        let rate = Rate::new(num, den).unwrap();
        let plan = plan_audio_with_rate(
            &p,
            file.clone(),
            limits(),
            point(100),
            Duration::from_nanos(7),
            rate,
        )
        .unwrap();
        assert_eq!(times(&plan.commands), expected);
        assert_eq!(plan.output_origin, point(100));
        assert_eq!(plan.playback_end_frame, None);
        assert_eq!(plan.judge_events, legacy.judge_events);
        assert_eq!(plan.final_judge_hash, legacy.final_judge_hash);
        assert_eq!(plan.recorded_until, legacy.recorded_until);
        let first = usize::from(rate != Rate::NORMAL);
        if first == 1 {
            assert!(matches!(plan.commands[0],AudioCommand::SetRate{rate:r,..} if r==rate));
        }
        assert!(matches!(
            plan.commands[first],
            AudioCommand::Play {
                sample: SampleId(3),
                ..
            }
        ));
        assert!(matches!(
            plan.commands[first + 1],
            AudioCommand::Play {
                sample: SampleId(1),
                ..
            }
        ));
        assert_eq!(encode_replay(&file, limits()).unwrap(), bytes);
    }
}
#[test]
fn section_start_profile_offset_and_output_preroll_are_each_applied_once() {
    let p = prepared(
        "#BPM 60\n#WAV01 tap.wav\n#00011:00010000\n",
        3,
        &[(1, &[0.5])],
    );
    let start = Timestamp::from_nanos(500_000_000);
    let (p, _) = prepare_at(p, start, pcm_limits()).unwrap();
    for (offset, expected) in [(250_000_000, 325_000_100), (-250_000_000, 575_000_100)] {
        let file = captured_at(&p, offset, start, None, None);
        let plan = plan_audio_with_rate(
            &p,
            file,
            limits(),
            point(100),
            Duration::from_nanos(200_000_000),
            Rate::new(2, 1).unwrap(),
        )
        .unwrap();
        assert_eq!(times(&plan.commands), [100, expected]);
        assert_eq!(plan.judge_events[0].at.as_nanos(), 1_000_000_000);
    }
    let p = prepared("#BPM 60\n#WAV01 tap.wav\n#00011:01\n", 3, &[(1, &[0.5])]);
    let file = captured(&p, 1, None);
    assert!(
        plan_audio_with_rate(
            &p,
            file.clone(),
            limits(),
            point(0),
            Duration::ZERO,
            Rate::new(1, 2).unwrap()
        )
        .is_err()
    );
    let plan = plan_audio_with_rate(
        &p,
        file.clone(),
        limits(),
        point(-50),
        Duration::from_nanos(2),
        Rate::new(1, 2).unwrap(),
    )
    .unwrap();
    assert_eq!(times(&plan.commands), [-50, -50]);
    let plan = plan_audio_with_rate(
        &p,
        file,
        limits(),
        point(9),
        Duration::ZERO,
        Rate::new(3, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(
        times(&plan.commands),
        [9, 9],
        "negative fractions truncate toward zero before origin validation"
    );
}
#[test]
fn collapsed_nanoseconds_keep_control_then_bgm_then_original_gameplay_order() {
    let mut p = prepared(
        "#BPM 60\n#WAV01 tap.wav\n#WAV02 bgm.wav\n#00011:01\n#00012:01\n#00001:02\n",
        3,
        &[(1, &[0.5]), (2, &[0.25])],
    );
    let file = captured(&p, 0, None);
    let original = p.bgm_commands[0];
    let plan = plan_audio_with_rate(
        &p,
        file.clone(),
        limits(),
        point(0),
        Duration::ZERO,
        Rate::new(2, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(times(&plan.commands), [0, 0, 0, 0]);
    assert!(matches!(plan.commands[0], AudioCommand::SetRate { .. }));
    assert_eq!(plan.commands[1], original);
    let voices: Vec<_> = plan.commands[2..]
        .iter()
        .map(|c| match c {
            AudioCommand::Play { voice, .. } => *voice,
            _ => panic!(),
        })
        .collect();
    assert_eq!(voices, p.sounds.iter().map(|s| s.voice).collect::<Vec<_>>());
    if let AudioCommand::Play { at, .. } = &mut p.bgm_commands[0] {
        *at = Timestamp::from_nanos(1);
    }
    let file = captured(&p, 0, Some(10));
    let plan = plan_audio_with_rate(
        &p,
        file,
        limits(),
        point(0),
        Duration::ZERO,
        Rate::new(2, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(times(&plan.commands), [0, 0, 0, 0]);
}
#[test]
fn finite_fence_uses_quantized_inverse_duration_without_a_minimum_frame() {
    let mut p = prepared("#BPM 60\n#WAV01 bgm.wav\n#00001:01\n", 3, &[(1, &[0.5])]);
    let file = captured_at(
        &p,
        0,
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(1)),
        Some(1),
    );
    assert!(plan_audio(&p, file.clone(), limits(), point(0), Duration::ZERO).is_err());
    let zero = plan_section_audio_with_rate(
        &p,
        file.clone(),
        limits(),
        point(0),
        Duration::ZERO,
        Rate::new(2, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(zero.playback_end_frame, Some(0));
    assert!(zero.commands.is_empty());
    let positive = plan_section_audio_with_rate(
        &p,
        file.clone(),
        limits(),
        point(0),
        Duration::from_nanos(1),
        Rate::new(2, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(positive.playback_end_frame, Some(1));
    assert_eq!(positive.commands.len(), 1);
    assert!(matches!(positive.commands[0], AudioCommand::SetRate { .. }));
    if let AudioCommand::Play { at, .. } = &mut p.bgm_commands[0] {
        *at = Timestamp::from_nanos(1);
    }
    let fence = plan_section_audio_with_rate(
        &p,
        file,
        limits(),
        point(0),
        Duration::ZERO,
        Rate::new(1, 2).unwrap(),
    )
    .unwrap();
    assert_eq!(fence.playback_end_frame, Some(1));
    assert_eq!(
        fence.commands.len(),
        1,
        "BGM quantized execution frame equals exclusive fence"
    );
}
#[test]
fn extreme_ratios_preserve_representable_mapping_and_refuse_real_overflow() {
    let p = prepared(
        "#BPM 60\n#WAV01 tap.wav\n#00011:00010000\n",
        1,
        &[(1, &[0.5])],
    );
    let file = captured(&p, 0, None);
    let plan = plan_audio_with_rate(
        &p,
        file.clone(),
        limits(),
        point(-100),
        Duration::ZERO,
        Rate::new(i64::MAX, u64::MAX).unwrap(),
    )
    .unwrap();
    assert_eq!(times(&plan.commands), [-100, 1_999_999_900]);
    for (rate, origin, preroll) in [
        (Rate::new(1, u64::MAX).unwrap(), 0, 0),
        (Rate::new(1, 2).unwrap(), i64::MAX, 0),
        (Rate::new(2, 1).unwrap(), 0, i64::MAX),
    ] {
        assert!(
            plan_audio_with_rate(
                &p,
                file.clone(),
                limits(),
                point(origin),
                Duration::from_nanos(preroll),
                rate
            )
            .is_err()
        );
    }
    for rate in [Rate::ZERO, Rate::REVERSE] {
        assert!(
            plan_audio_with_rate(&p, file.clone(), limits(), point(0), Duration::ZERO, rate)
                .is_err()
        );
    }
}
#[test]
fn finite_endpoint_itself_must_be_a_representable_output_timestamp() {
    let p = prepared("#BPM 60\n", 3, &[]);
    let file = captured_at(
        &p,
        0,
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(4_000_000_000)),
        None,
    );
    let error = plan_section_audio_with_rate(
        &p,
        file.clone(),
        limits(),
        point(0),
        Duration::ZERO,
        Rate::new(1, 10_000_000_000).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<beatkernel_bms_runtime::replay_audio::ReplayAudioError>(),
        Some(beatkernel_bms_runtime::replay_audio::ReplayAudioError::Overflow)
    ));
    let cancellation = plan_section_audio_with_rate(
        &p,
        file,
        limits(),
        point(i64::MIN),
        Duration::ZERO,
        Rate::new(1, 3_000_000_000).unwrap(),
    )
    .unwrap();
    assert_eq!(cancellation.playback_end_frame, Some(36_000_000_000));
    assert_eq!(times(&cancellation.commands), [i64::MIN]);
}
#[test]
fn empty_and_prefix_logs_retain_logical_identity_with_one_real_control() {
    let p = main_prepared();
    let mut file = captured(&p, 0, Some(4_000_000_001));
    file.records.clear();
    let legacy = plan_audio(&p, file.clone(), limits(), point(0), Duration::ZERO).unwrap();
    let plan = plan_audio_with_rate(
        &p,
        file,
        limits(),
        point(0),
        Duration::ZERO,
        Rate::new(2, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(plan.commands.len(), 1);
    assert!(matches!(plan.commands[0], AudioCommand::SetRate { .. }));
    assert!(plan.judge_events.is_empty());
    assert_eq!(plan.recorded_until, None);
    assert_eq!(plan.final_judge_hash, legacy.final_judge_hash);
    let mut file = captured(&p, 0, Some(4_000_000_001));
    file.records.truncate(2);
    let plan = plan_audio_with_rate(
        &p,
        file,
        limits(),
        point(0),
        Duration::ZERO,
        Rate::new(2, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(times(&plan.commands), [0, 500_000_000, 500_000_000]);
    assert_eq!(plan.judge_events.len(), 1);
}
#[test]
fn original_invisible_press_mine_and_gauge_stop_selection_share_rate_mapping() {
    let p = prepared(
        "#BPM 60\n#WAV00 mine.wav\n#WAV01 tap.wav\n#WAV02 press.wav\n#00011:00010000\n#00031:02\n#000D1:001E0000\n",
        8,
        &[(0, &[0.125]), (1, &[0.5]), (2, &[0.25])],
    );
    // Keep the lane occupied while the actual Advance crosses its mine.
    // An Up at the exact mine instant releases ownership before evaluation.
    let events = [(0, 0x11, ButtonState::Down)];
    let gauge = GaugeProfile::new(1, 0, 0, -1, true, vec![]).unwrap();
    let file = capture_events(
        &p,
        0,
        Timestamp::ZERO,
        None,
        &gauge,
        &events,
        Some(1_000_000_001),
    );
    assert!(
        plan_audio(&p, file.clone(), limits(), point(0), Duration::ZERO).is_err(),
        "legacy consumer continues refusing nondefault gauge metadata"
    );
    let normal = plan_section_audio(&p, file.clone(), limits(), point(0), Duration::ZERO).unwrap();
    assert!(
        normal.commands.iter().any(|c| matches!(
            c,
            AudioCommand::Play {
                sample: SampleId(2),
                ..
            }
        )),
        "actual invisible fresh press"
    );
    assert!(
        normal.commands.iter().any(|c| matches!(
            c,
            AudioCommand::Play {
                sample: SampleId(0),
                ..
            }
        )),
        "actual mine contact"
    );
    assert!(
        normal
            .commands
            .iter()
            .any(|c| matches!(c, AudioCommand::Stop { .. })),
        "actual depleted gauge"
    );
    let fast = plan_section_audio_with_rate(
        &p,
        file,
        limits(),
        point(100),
        Duration::from_nanos(7),
        Rate::new(2, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(fast.judge_events, normal.judge_events);
    assert_eq!(fast.final_judge_hash, normal.final_judge_hash);
    assert!(matches!(fast.commands[0], AudioCommand::SetRate { .. }));
    assert!(
        fast.commands[1..]
            .iter()
            .all(|c| [107, 500_000_107].contains(&c.at().as_nanos()))
    );
    assert_eq!(fast.commands.len(), normal.commands.len() + 1);
}

#[test]
fn invisible_only_fresh_press_maps_original_operation_without_gauge_or_mines() {
    let p = prepared(
        "#BPM 60\n#WAV01 press.wav\n#00031:01\n",
        8,
        &[(1, &[0.25, -0.25])],
    );
    let events = [
        (1_000_000_000, 0x11, ButtonState::Down),
        (1_000_000_001, 0x11, ButtonState::Down),
        (1_000_000_002, 0x11, ButtonState::Up),
    ];
    let file = capture_events(
        &p,
        0,
        Timestamp::ZERO,
        None,
        &GaugeProfile::default(),
        &events,
        None,
    );
    let original_bytes = encode_replay(&file, limits()).unwrap();
    let normal = plan_audio(
        &p,
        file.clone(),
        limits(),
        point(100),
        Duration::from_nanos(7),
    )
    .unwrap();
    assert_eq!(times(&normal.commands), [1_000_000_107]);
    assert!(matches!(
        normal.commands[0],
        AudioCommand::Play {
            sample: SampleId(1),
            ..
        }
    ));
    assert!(
        normal.judge_events.is_empty(),
        "invisible press creates no judged object"
    );
    for (rate, expected) in [
        (Rate::new(1, 2).unwrap(), 2_000_000_107),
        (Rate::new(3, 2).unwrap(), 666_666_773),
    ] {
        let plan = plan_audio_with_rate(
            &p,
            file.clone(),
            limits(),
            point(100),
            Duration::from_nanos(7),
            rate,
        )
        .unwrap();
        assert_eq!(
            times(&plan.commands),
            [100, expected],
            "repeated Down selects no second sound"
        );
        assert!(
            matches!(plan.commands[0], AudioCommand::SetRate { rate: actual, .. } if actual == rate)
        );
        assert!(matches!(
            plan.commands[1],
            AudioCommand::Play {
                sample: SampleId(1),
                ..
            }
        ));
        assert_eq!(plan.judge_events, normal.judge_events);
        assert_eq!(plan.final_judge_hash, normal.final_judge_hash);
        assert_eq!(
            plan.recorded_until,
            Some(Timestamp::from_nanos(1_000_000_002))
        );
        assert_eq!(encode_replay(&file, limits()).unwrap(), original_bytes);
    }
}
