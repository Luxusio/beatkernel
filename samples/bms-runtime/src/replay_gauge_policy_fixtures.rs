use super::*;
use crate::gauge::{BmsGauge, GaugeDynamics, GaugeFailure};
use beatkernel::{
    input::codec::CodecLimits,
    replay::{
        REPLAY_VERSION,
        codec::{ReplayFile, encode_replay, decode_replay},
    },
    time::{ClockDomainId, ClockPoint, Timestamp, Duration},
};
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 1024, 8192, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn profile() -> GaugeProfile {
    GaugeProfile::new(
        50_000_000,
        0,
        1_000_000,
        -50_000_000,
        true,
        vec![GradeDelta {
            grade: JudgeGrade(u32::MAX),
            delta: i64::MIN,
        }],
    )
    .unwrap()
    .with_dynamics(GaugeDynamics {
        minimum_alive: 0,
        failure_below: 1_000_000,
        damage_reduction_below: 32_000_000,
    })
    .unwrap()
}
fn header() -> ReplayHeader {
    ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: vec![1],
        rules_identity: vec![2],
        options: b"bms-judge-profile/v1:example".to_vec(),
        seed: 0,
        normalized_clock: ClockDomainId(1),
    }
}
#[test]
fn policy_codec_is_bounded_canonical_and_preserves_legacy_bytes() {
    let original = header();
    assert_eq!(
        wrap_header(original.clone(), &GaugeProfile::default(), limits()).unwrap(),
        original
    );
    let wrapped = wrap_header(original.clone(), &profile(), limits()).unwrap();
    let (judge, gauge) = split_options(&wrapped.options).unwrap();
    assert_eq!(judge, original.options);
    assert_eq!(gauge, profile());
    assert!(wrap_header(wrapped.clone(), &profile(), limits()).is_err());
    for end in PREFIX.len()..wrapped.options.len() {
        assert!(
            split_options(&wrapped.options[..end]).is_err(),
            "prefix {end}"
        );
    }
    let mut changed = wrapped.options.clone();
    changed.push(0);
    assert!(split_options(&changed).is_err());
    let fixed = PREFIX.len() + 4 + original.options.len();
    for (offset, bytes) in [
        (32, vec![2]),
        (33, 65u32.to_le_bytes().to_vec()),
        (37, u64::MAX.to_le_bytes().to_vec()),
        (45, u64::MAX.to_le_bytes().to_vec()),
    ] {
        let mut changed = wrapped.options.clone();
        changed[fixed + offset..fixed + offset + bytes.len()].copy_from_slice(&bytes);
        assert!(split_options(&changed).is_err());
    }
    let total = wrapped.options.len()
        + wrapped.chart_identity.len()
        + wrapped.rules_identity.len()
        + env!("CARGO_PKG_VERSION").len();
    let exact =
        ReplayCodecLimits::new(65536, 1024, total, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    assert!(wrap_header(original.clone(), &profile(), exact).is_ok());
    let short = ReplayCodecLimits::new(
        65536,
        1024,
        total - 1,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap();
    assert_eq!(
        wrap_header(original, &profile(), short),
        Err(PolicyError::HeaderTooLarge)
    );
    assert!(crate::replay_playback::decode_chart_setup(&wrapped.options).is_err());
}

#[test]
fn policy_survives_real_capture_validation_visual_and_audio_depletion_without_mines() {
    use crate::mine_audio_consumers_fixtures::{data, recorded, replay_limits, Action};
    use beatkernel::audio::AudioCommand;
    let prepared = data("#BPM 60\n#WAV01 note\n#00011:01010101", false);
    let original = recorded(
        &prepared,
        &[
            Action::Press(0, 91),
            Action::Release(0, 91),
            Action::Advance(1_500_000_000),
            Action::Press(2_000_000_000, 91),
            Action::Release(2_000_000_000, 91),
            Action::Advance(4_000_000_000),
        ],
        beatkernel_bms::BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let policy = GaugeProfile::new(20_000_000, 0, 0, -30_000_000, true, vec![]).unwrap();
    let mut file = original.file;
    file.header = wrap_header(file.header, &policy, replay_limits()).unwrap();
    let bytes = encode_replay(&file, replay_limits()).unwrap();
    let decoded = decode_replay(&bytes, replay_limits()).unwrap();
    assert_eq!(decoded, file);
    let mut visual = crate::replay_visual::ReplayVisual::new_section(
        &prepared.source,
        &decoded,
        replay_limits(),
    )
    .unwrap();
    let mut expected = BmsGauge::new(policy.clone());
    for report in &original.reports {
        expected
            .observe(&report.judge_events, &report.hazard_events)
            .unwrap();
    }
    let events = visual
        .advance_to(Timestamp::from_nanos(4_000_000_000))
        .unwrap();
    assert_eq!(visual.gauge(), &expected);
    assert_eq!(
        visual.gauge().snapshot().failure,
        Some(GaugeFailure::Depleted)
    );
    let plan = crate::replay_audio::plan_section_audio(
        &prepared,
        decoded,
        replay_limits(),
        ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(1_000_000_000),
        },
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(plan.judge_events, events);
    assert_eq!(plan.final_judge_hash, original.hash);
    assert!(plan.commands.iter().any(
        |c| matches!(c, AudioCommand::Stop {at,..} if *at == Timestamp::from_nanos(2_500_000_000))
    ));
    assert!(!plan.commands.iter().any(
        |c| matches!(c, AudioCommand::Play {at,..} if *at >= Timestamp::from_nanos(3_000_000_000))
    ));
    let pristine =
        crate::replay_playback::validate_section_setup(&prepared.source, &file, replay_limits())
            .unwrap();
    let mut captured = crate::replay_capture::LiveReplayCapture::new_with_gauge(
        &pristine,
        file.header.normalized_clock,
        replay_limits(),
        Timestamp::ZERO,
        0,
        None,
        beatkernel_bms::BmsInputMode::ButtonOnly,
        None,
        &policy,
    )
    .unwrap();
    assert_eq!(captured.header(), &file.header);
    for report in &original.reports {
        captured.record_report(report).unwrap();
    }
    let captured_file = captured.into_file();
    assert_eq!(captured_file, file);
    let empty = ReplayFile::new(captured_file.header.clone(), vec![]);
    assert_eq!(
        crate::replay_playback::decode_section_setup(&empty.header.options)
            .unwrap()
            .gauge,
        policy
    );
}
