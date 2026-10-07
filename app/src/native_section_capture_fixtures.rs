//! Deferred pristine source-aware capture metadata, without native IO.
use crate::{
    PreparedBms,
    native_judge::{
        NativeJudgeConfig, capture_limits, prepare_capture, prepare_capture_for_source,
        prepare_section_capture_for_source,
    },
    replay_capture::LiveReplayCapture,
    replay_playback::decode_section_setup,
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleBank},
    judge::JudgeEngine,
    time::{ClockDomainId, Timestamp},
};
fn prepared(start: i64) -> PreparedBms {
    let original = beatkernel_bms::parse(
        "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n",
        Default::default(),
    )
    .unwrap();
    let source = crate::section_start::source_at(&original, Timestamp::from_nanos(start)).unwrap();
    PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(
            AudioFormat::new(1000, 1).unwrap(),
            PcmLimits::new(64, 256, 1).unwrap(),
        )
        .unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    }
}
fn judge(prepared: &PreparedBms, end: Option<i64>) -> JudgeEngine {
    NativeJudgeConfig {
        early: 5,
        late: 7,
        offset: -2,
        preroll: 0,
        output: ClockDomainId(2),
        end: end.map(Timestamp::from_nanos),
    }
    .judge(&prepared.source, prepared.source.compile().unwrap().chart)
    .unwrap()
}
pub(crate) fn capture(start: i64, end: Option<i64>, seed: u64) -> LiveReplayCapture {
    let prepared = prepared(start);
    let judge = judge(&prepared, end);
    prepare_section_capture_for_source(
        &prepared.source,
        &judge,
        ClockDomainId(1),
        Timestamp::from_nanos(start),
        seed,
        end.map(Timestamp::from_nanos),
        capture_limits(true, 8192, 8).unwrap(),
    )
    .unwrap()
    .unwrap()
}
#[test]
fn real_pristine_prepared_judge_capture_preserves_original_finite_section_and_seed() {
    let capture = capture(20_000_000, Some(50_000_000), u64::MAX);
    let setup = decode_section_setup(&capture.header().options).unwrap();
    assert_eq!(setup.start, Timestamp::from_nanos(20_000_000));
    assert_eq!(setup.end, Some(Timestamp::from_nanos(50_000_000)));
    assert_eq!(setup.chart_seed, u64::MAX);
    assert_eq!(setup.profile.input_offset().as_nanos(), -2);
    assert_eq!(setup.profile.windows()[0].early.as_nanos(), 5);
    assert_eq!(setup.profile.windows()[0].late.as_nanos(), 7);
    assert_eq!(capture.header().normalized_clock, ClockDomainId(1));
    assert!(capture.records().is_empty());
    assert!(
        capture
            .header()
            .options
            .starts_with(b"bms-judge-profile/v4:")
    );
}
#[test]
fn unlimited_section_helper_retains_exact_existing_source_and_plain_capture_headers() {
    for (start, seed) in [(0, 0), (20_000_000, 0), (20_000_000, u64::MAX)] {
        let prepared = prepared(start);
        let judge = judge(&prepared, None);
        let limits = capture_limits(true, 8192, 8).unwrap();
        let old = prepare_capture_for_source(
            &prepared.source,
            &judge,
            ClockDomainId(1),
            Timestamp::from_nanos(start),
            seed,
            limits,
        )
        .unwrap()
        .unwrap();
        let new = prepare_section_capture_for_source(
            &prepared.source,
            &judge,
            ClockDomainId(1),
            Timestamp::from_nanos(start),
            seed,
            None,
            limits,
        )
        .unwrap()
        .unwrap();
        let plain = prepare_capture(
            &judge,
            ClockDomainId(1),
            Timestamp::from_nanos(start),
            seed,
            limits,
        )
        .unwrap()
        .unwrap();
        assert_eq!(new.header(), old.header());
        assert_eq!(new.header(), plain.header());
        assert_eq!(
            decode_section_setup(&new.header().options).unwrap().end,
            None
        );
    }
}
#[test]
fn disabled_capture_ignores_invalid_unused_extent_and_recording_limits() {
    let prepared = prepared(0);
    let judge = judge(&prepared, None);
    assert!(capture_limits(false, 0, 0).unwrap().is_none());
    assert!(
        prepare_section_capture_for_source(
            &prepared.source,
            &judge,
            ClockDomainId(1),
            Timestamp::from_nanos(-1),
            u64::MAX,
            Some(Timestamp::from_nanos(-2)),
            None
        )
        .unwrap()
        .is_none()
    );
    assert!(
        prepare_capture_for_source(
            &prepared.source,
            &judge,
            ClockDomainId(1),
            Timestamp::from_nanos(-1),
            0,
            None
        )
        .unwrap()
        .is_none()
    );
    assert!(
        prepare_capture(&judge, ClockDomainId(1), Timestamp::from_nanos(-1), 0, None)
            .unwrap()
            .is_none()
    );
}
