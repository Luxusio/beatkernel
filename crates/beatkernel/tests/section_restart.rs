use beatkernel::{
    audio::{AudioFormat, AudioLimits, MixerConfig, PcmLimits, PcmSample, SampleId, VoiceId},
    runtime::restart::{FrameRounding, RestartError, RestartPlan},
    time::{
        AffineClockMapper, CalibrationUncertainty, ClockDomainId, ClockInterval,
        ClockMappingQuality, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp,
    },
};

#[test]
fn section_starts_on_original_frame_with_fresh_output_each_time() {
    let pcm_limits = PcmLimits::new(128, 128, 1).unwrap();
    let source = PcmSample::new(
        AudioFormat::new(3, 1).unwrap(),
        vec![0.1, 0.2, 0.3, 0.4],
        pcm_limits,
    )
    .unwrap();
    let requested = Timestamp::from_nanos(500_000_000);
    for _ in 0..32 {
        let plan = RestartPlan::select(&source, Timestamp::ZERO, requested, FrameRounding::Nearest)
            .unwrap();
        assert_eq!(plan.source_frame(), 2);
        assert_eq!(plan.applied_song_time(), Timestamp::from_nanos(666_666_667));
        assert_eq!(plan.correction_nanos(), 166_666_667);
        let config = MixerConfig::new(
            source.format(),
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(4, 4, 4, 8, 4).unwrap(),
        );
        let mut prepared = plan
            .prepare_audio(config, pcm_limits, SampleId(1), VoiceId(1))
            .unwrap();
        let mut buffer = [0.0; 3];
        prepared.mixer.render(&mut buffer).unwrap();
        assert_eq!(buffer, [0.3, 0.4, 0.0]);
    }
    assert!(matches!(
        RestartPlan::select(&source, Timestamp::ZERO, requested, FrameRounding::Exact),
        Err(RestartError::BetweenFrames)
    ));
}

#[test]
fn extreme_song_origins_use_wide_intermediates_and_reject_unrepresentable_anchor() {
    let limits = PcmLimits::new(16, 16, 1).unwrap();
    let source = PcmSample::new(AudioFormat::new(1, 1).unwrap(), vec![0.2], limits).unwrap();
    let plan = RestartPlan::select(
        &source,
        Timestamp::MIN,
        Timestamp::from_nanos(i64::MIN + 1_000_000_000),
        FrameRounding::Exact,
    )
    .unwrap();
    assert_eq!(plan.source_frame(), 1);
    assert!(plan.copy_pcm(limits).unwrap().samples().is_empty());
    assert!(matches!(
        RestartPlan::select(
            &source,
            Timestamp::from_nanos(i64::MAX - 5),
            Timestamp::MAX,
            FrameRounding::Ceil
        ),
        Err(RestartError::Overflow)
    ));
    assert!(matches!(
        RestartPlan::select(
            &source,
            Timestamp::MAX,
            Timestamp::MIN,
            FrameRounding::Floor
        ),
        Err(RestartError::OutsideSource)
    ));
}

#[test]
fn prepared_transport_uses_explicit_presentation_mapping_and_preserves_unknown_error() {
    let limits = PcmLimits::new(16, 16, 1).unwrap();
    let format = AudioFormat::new(1, 1).unwrap();
    let source = PcmSample::new(format, vec![0.1, 0.2], limits).unwrap();
    let plan = RestartPlan::select(
        &source,
        Timestamp::ZERO,
        Timestamp::from_nanos(1_000_000_000),
        FrameRounding::Exact,
    )
    .unwrap();
    let config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(4, 4, 4, 8, 4).unwrap(),
    );
    let prepared = plan
        .prepare_audio(config, limits, SampleId(1), VoiceId(1))
        .unwrap();
    let pair = |source, target| ClockPair {
        source: ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(source),
        },
        target: ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(target),
        },
    };
    let mapping = AffineClockMapper::from_pairs(
        pair(0, 10_000_000_000),
        pair(1_000_000_000, 11_000_000_000),
        ClockInterval {
            start: Timestamp::ZERO,
            end: Timestamp::from_nanos(1_000_000_000),
        },
        ExtrapolationPolicy::Forbid,
        CalibrationUncertainty {
            observation_error: Duration::from_nanos(100),
            residual_drift_error: None,
        },
    )
    .unwrap();
    let (transport, quality) = prepared
        .mapped_transport(ClockDomainId(1), &mapping)
        .unwrap();
    assert_eq!(quality, ClockMappingQuality::Unknown);
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(10_000_000_000))
            .unwrap(),
        plan.applied_song_time()
    );
    assert!(matches!(
        prepared.mapped_transport(ClockDomainId(3), &mapping),
        Err(RestartError::UnmappedClock)
    ));
}
