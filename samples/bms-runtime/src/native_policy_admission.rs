//! Cold nondefault native policy admission; no clocks, devices or publication.
use crate::{
    gauge::{BmsGauge, GaugeProfile, MAX_GAUGE_GRADES},
    native_gameplay::{NativeGameplayConfig, NativeGameplayResult},
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    judge::JudgeEngine,
    replay::{ReplayHeader, REPLAY_VERSION},
    time::Timestamp,
};

pub(crate) fn validate_initial(judge: &JudgeEngine, gauge: &BmsGauge) -> NativeGameplayResult<()> {
    if judge.effective_song_time().is_some()
        || judge.profile().windows().len() > MAX_GAUGE_GRADES
        || gauge.snapshot() != BmsGauge::new(gauge.profile().try_copy()?).snapshot()
    {
        return Err("nondefault native policy requires pristine judge and initial gauge".into());
    }
    Ok(())
}
pub(crate) fn validate_capture(
    judge: &JudgeEngine,
    gauge: &GaugeProfile,
    capture: Option<&LiveReplayCapture>,
    config: &NativeGameplayConfig,
) -> NativeGameplayResult<()> {
    if let Some(capture) = capture {
        if !capture.records().is_empty() {
            return Err("native policy capture is already processed".into());
        }
        validate_header(judge, gauge, capture.header(), config)?;
    }
    Ok(())
}
pub(crate) fn validate_header(
    judge: &JudgeEngine,
    gauge: &GaugeProfile,
    header: &ReplayHeader,
    config: &NativeGameplayConfig,
) -> NativeGameplayResult<()> {
    if header.version != REPLAY_VERSION
        || header.seed != 0
        || header.options.len() > 4096
        || header.normalized_clock != config.origin.domain
    {
        return Err("native policy header version, capacity or clock differs".into());
    }
    let setup = crate::replay_playback::decode_section_setup(&header.options)?;
    let start = i128::from(config.song_origin.as_nanos())
        + i128::from(config.playback_origin.timestamp.as_nanos())
        - i128::from(config.stream_origin.timestamp.as_nanos());
    let start = Timestamp::from_nanos(
        i64::try_from(start).map_err(|_| "native policy original start overflow")?,
    );
    if setup.profile != *judge.profile()
        || setup.gauge != *gauge
        || setup.start != start
        || setup.end != config.end_song
        || setup.input_mode != beatkernel_bms::BmsInputMode::ButtonOnly
    {
        return Err("native policy header differs from actual judge, gauge or section".into());
    }
    let identity = if let Some(bytes) = header.chart_identity.strip_prefix(b"bms-judge-setup/v1:") {
        if bytes.len() != 8 {
            return Err("native policy chart identity extent differs".into());
        }
        bytes
    } else if let Some(bytes) = header.chart_identity.strip_prefix(b"bms-judge-setup/v2:") {
        if bytes.len() != 16 {
            return Err("native policy chart identity extent differs".into());
        }
        &bytes[..8]
    } else {
        return Err("native policy chart identity schema differs".into());
    };
    if identity != judge.stable_hash()?.to_le_bytes() {
        return Err("native policy chart or rules identity differs".into());
    }
    let limits = crate::native_judge::capture_limits(true, 65536, 128)?.expect("enabled");
    let expected = crate::replay_capture::setup_gauge_header(
        judge,
        header.normalized_clock,
        limits,
        setup.start,
        setup.chart_seed,
        setup.end,
        setup.input_mode,
        None,
        gauge,
    )?;
    let expected =
        crate::replay_judgment_policy::wrap_header(expected, setup.judgments.as_ref(), limits)?;
    if header.rules_identity != expected.rules_identity || header.options != expected.options {
        return Err("native policy setup is not canonical".into());
    }
    Ok(())
}
