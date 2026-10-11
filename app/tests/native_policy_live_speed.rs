//! Public legacy setup compatibility remains independent of private admission.
use beatkernel::{
    input::CodecLimits,
    judge::JudgeEngine,
    replay::codec::ReplayCodecLimits,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms::BmsInputMode;
use beatkernel_bms_runtime::{
    play_policy::ResolvedPlayPolicy,
    replay_capture::{setup_play_policy_header, setup_play_policy_header_with_speed},
    replay_playback::decode_section_setup,
};

#[test]
fn explicit_normal_native_policy_identity_preserves_legacy_bytes_and_conditions() {
    let source =
        beatkernel_bms::parse("#BPM 120\n#WAV01 note.wav\n#00011:01\n", Default::default())
            .unwrap();
    let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        policy.judge().clone(),
    )
    .unwrap();
    let limits =
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 4096).unwrap()).unwrap();
    let old = setup_play_policy_header(
        &judge,
        ClockDomainId(7),
        limits,
        Timestamp::from_nanos(5),
        19,
        Some(Timestamp::from_nanos(100)),
        BmsInputMode::ButtonOnly,
        None,
        &policy,
    )
    .unwrap();
    let explicit = setup_play_policy_header_with_speed(
        &judge,
        ClockDomainId(7),
        limits,
        Timestamp::from_nanos(5),
        19,
        Some(Timestamp::from_nanos(100)),
        BmsInputMode::ButtonOnly,
        None,
        &policy,
        "8/8".parse().unwrap(),
    )
    .unwrap();
    assert_eq!(explicit, old);
    let decoded = decode_section_setup(&explicit.options).unwrap();
    assert_eq!(decoded.start, Timestamp::from_nanos(5));
    assert_eq!(decoded.end, Some(Timestamp::from_nanos(100)));
    assert_eq!(decoded.chart_seed, 19);
    assert_eq!(decoded.source_speed, "1/1".parse().unwrap());
    assert_eq!(decoded.profile, *policy.judge());
    assert_eq!(decoded.gauge, *policy.gauge());
    assert!(judge.effective_song_time().is_none());
}
