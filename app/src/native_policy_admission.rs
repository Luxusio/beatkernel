//! Cold nondefault native policy admission; no clocks, devices or publication.
use crate::{
    gauge::{BmsGauge, GaugeProfile, MAX_GAUGE_GRADES},
    live_speed::LiveSpeedSetup,
    native_gameplay::{NativeGameplayConfig, NativeGameplayResult},
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    judge::JudgeEngine,
    replay::{ReplayHeader, REPLAY_VERSION},
    time::{ClockDomainId, Timestamp},
    transport::{PrerollTransportSetup, Transport},
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
pub(crate) fn validate_capture_in_domain(
    judge: &JudgeEngine,
    gauge: &GaugeProfile,
    capture: Option<&LiveReplayCapture>,
    config: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    section_start: Option<Timestamp>,
) -> NativeGameplayResult<()> {
    validate_capture_in_domain_inner(
        judge,
        gauge,
        capture,
        config,
        logical_domain,
        section_start,
        None,
    )
}

pub(crate) fn validate_capture_in_domain_with_speed(
    judge: &JudgeEngine,
    gauge: &GaugeProfile,
    capture: Option<&LiveReplayCapture>,
    config: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    section_start: Option<Timestamp>,
    speed: LiveSpeedSetup,
) -> NativeGameplayResult<()> {
    validate_capture_in_domain_inner(
        judge,
        gauge,
        capture,
        config,
        logical_domain,
        section_start,
        Some(speed),
    )
}

fn validate_capture_in_domain_inner(
    judge: &JudgeEngine,
    gauge: &GaugeProfile,
    capture: Option<&LiveReplayCapture>,
    config: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    section_start: Option<Timestamp>,
    speed: Option<LiveSpeedSetup>,
) -> NativeGameplayResult<()> {
    if let Some(speed) = speed {
        validate_speed_config(config, section_start, speed)?;
    }
    if let Some(capture) = capture {
        if !capture.records().is_empty() {
            return Err("native policy capture is already processed".into());
        }
        validate_header_in_domain_inner(
            judge,
            gauge,
            capture.header(),
            config,
            logical_domain,
            section_start,
            speed,
        )?;
    }
    Ok(())
}
pub(crate) fn validate_header(
    judge: &JudgeEngine,
    gauge: &GaugeProfile,
    header: &ReplayHeader,
    config: &NativeGameplayConfig,
) -> NativeGameplayResult<()> {
    validate_header_in_domain(judge, gauge, header, config, config.origin.domain, None)
}
pub(crate) fn validate_header_in_domain(
    judge: &JudgeEngine,
    gauge: &GaugeProfile,
    header: &ReplayHeader,
    config: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    section_start: Option<Timestamp>,
) -> NativeGameplayResult<()> {
    validate_header_in_domain_inner(
        judge,
        gauge,
        header,
        config,
        logical_domain,
        section_start,
        None,
    )
}

pub(crate) fn validate_header_in_domain_with_speed(
    judge: &JudgeEngine,
    gauge: &GaugeProfile,
    header: &ReplayHeader,
    config: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    section_start: Option<Timestamp>,
    speed: LiveSpeedSetup,
) -> NativeGameplayResult<()> {
    validate_header_in_domain_inner(
        judge,
        gauge,
        header,
        config,
        logical_domain,
        section_start,
        Some(speed),
    )
}

fn validate_header_in_domain_inner(
    judge: &JudgeEngine,
    gauge: &GaugeProfile,
    header: &ReplayHeader,
    config: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    section_start: Option<Timestamp>,
    speed: Option<LiveSpeedSetup>,
) -> NativeGameplayResult<()> {
    if header.version != REPLAY_VERSION
        || header.seed != 0
        || header.options.len() > 4096
        || header.normalized_clock != logical_domain
    {
        return Err("native policy header version, capacity or clock differs".into());
    }
    let (_, source_rates) = crate::replay_speed_policy::split_schedule(&header.options)?;
    if source_rates.is_variable() {
        return Err(
            "variable source speed is unsupported for fixed native policy admission".into(),
        );
    }
    let setup = crate::replay_playback::decode_section_setup(&header.options)?;
    if let Some(speed) = speed {
        validate_speed_config(config, section_start, speed)?;
    }
    let start = match (speed, section_start) {
        (Some(speed), _) => speed.start(),
        (None, Some(start)) => start,
        (None, None) => {
            let start = i128::from(config.song_origin.as_nanos())
                + i128::from(config.playback_origin.timestamp.as_nanos())
                - i128::from(config.stream_origin.timestamp.as_nanos());
            let start = Timestamp::from_nanos(
                i64::try_from(start).map_err(|_| "native policy original start overflow")?,
            );
            start
        }
    };
    if setup.profile != *judge.profile()
        || setup.gauge != *gauge
        || setup.start != start
        || setup.end != config.end_song
        || setup.source_speed
            != speed.map_or_else(
                || {
                    crate::replay_speed::ReplaySpeed::new(beatkernel::transport::Rate::NORMAL)
                        .expect("positive NORMAL")
                },
                LiveSpeedSetup::speed,
            )
        || setup.input_mode != beatkernel_bms::BmsInputMode::ButtonOnly
    {
        return Err("native policy header differs from actual judge, gauge or section".into());
    }
    if let Some(timing) = &setup.timing {
        timing.validate_judge(judge, setup.input_mode)?;
    } else if judge
        .chart()
        .objects()
        .iter()
        .any(|object| judge.builtin_timing(object.id).is_some())
    {
        return Err("native staged rules require recorded timing identity".into());
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
    let expected = crate::replay_capture::wrap_timing_header(
        expected,
        setup.timing.as_ref(),
        setup.input_mode,
        limits,
    )?;
    let expected = crate::replay_speed_policy::wrap_header(expected, setup.source_speed, limits)?;
    if header.rules_identity != expected.rules_identity || header.options != expected.options {
        return Err("native policy setup is not canonical".into());
    }
    Ok(())
}

/// Compare selected meanings as well as the recorded judge/gauge setup.
pub(crate) fn validate_selected(
    judge: &JudgeEngine,
    gauge: &BmsGauge,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    capture: Option<&LiveReplayCapture>,
    competition: Option<&ReplayHeader>,
    config: &NativeGameplayConfig,
) -> NativeGameplayResult<()> {
    validate_selected_in_domain(
        judge,
        gauge,
        policy,
        capture,
        competition,
        config,
        config.origin.domain,
        None,
    )
}
pub(crate) fn validate_selected_in_domain(
    judge: &JudgeEngine,
    gauge: &BmsGauge,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    capture: Option<&LiveReplayCapture>,
    competition: Option<&ReplayHeader>,
    config: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    section_start: Option<Timestamp>,
) -> NativeGameplayResult<()> {
    validate_selected_in_domain_inner(
        judge,
        gauge,
        policy,
        capture,
        competition,
        config,
        logical_domain,
        section_start,
        None,
    )
}

pub(crate) fn validate_selected_in_domain_with_speed(
    judge: &JudgeEngine,
    gauge: &BmsGauge,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    capture: Option<&LiveReplayCapture>,
    competition: Option<&ReplayHeader>,
    config: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    section_start: Option<Timestamp>,
    speed: LiveSpeedSetup,
) -> NativeGameplayResult<()> {
    validate_selected_in_domain_inner(
        judge,
        gauge,
        policy,
        capture,
        competition,
        config,
        logical_domain,
        section_start,
        Some(speed),
    )
}

fn validate_selected_in_domain_inner(
    judge: &JudgeEngine,
    gauge: &BmsGauge,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    capture: Option<&LiveReplayCapture>,
    competition: Option<&ReplayHeader>,
    config: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    section_start: Option<Timestamp>,
    speed: Option<LiveSpeedSetup>,
) -> NativeGameplayResult<()> {
    if let Some(speed) = speed {
        validate_speed_config(config, section_start, speed)?;
    }
    validate_initial(judge, gauge)?;
    if judge.profile() != policy.judge() || gauge.profile() != policy.gauge() {
        return Err("selected native policy differs from actual judge or gauge".into());
    }
    policy.validate_timing(judge, beatkernel_bms::BmsInputMode::ButtonOnly)?;
    validate_capture_in_domain_inner(
        judge,
        gauge.profile(),
        capture,
        config,
        logical_domain,
        section_start,
        speed,
    )?;
    for header in capture
        .map(LiveReplayCapture::header)
        .into_iter()
        .chain(competition)
    {
        validate_header_in_domain_inner(
            judge,
            gauge.profile(),
            header,
            config,
            logical_domain,
            section_start,
            speed,
        )?;
        let setup = crate::replay_playback::decode_section_setup(&header.options)?;
        if setup.judgments.as_ref() != policy.judgments() {
            return Err("selected native judgment classes differ from recorded policy".into());
        }
        if setup.timing.as_ref() != policy.timing() {
            return Err("selected native timing differs from recorded policy".into());
        }
    }
    if let (Some(capture), Some(header)) = (capture, competition) {
        if capture.header() != header {
            return Err("native capture and competition policy identities differ".into());
        }
    }
    Ok(())
}

pub(crate) fn validate_config(config: &NativeGameplayConfig) -> NativeGameplayResult<()> {
    if config.origin.domain == config.stream_origin.domain
        || config.stream_origin.domain != config.playback_origin.domain
        || config.playback_origin.timestamp < config.stream_origin.timestamp
        || config.sample_rate == 0
        || config.sample_rate > 1_000_000_000
        || !(0..=1_000_000_000).contains(&config.advance_lag.as_nanos())
    {
        return Err("invalid selected native gameplay clocks/rate/lag".into());
    }
    Ok(())
}

/// Validates an explicitly declared original section independently of physical output origins.
pub(crate) fn validate_audio_config(
    config: &crate::native_gameplay::AudioGameplayConfig,
) -> NativeGameplayResult<()> {
    validate_config(&config.gameplay)?;
    let preroll = config
        .section_start
        .as_nanos()
        .checked_sub(config.gameplay.song_origin.as_nanos())
        .ok_or("audio section preroll difference overflows")?;
    if config.section_start.as_nanos() < 0
        || preroll < 0
        || config
            .gameplay
            .end_song
            .is_some_and(|end| end < config.section_start)
    {
        return Err("invalid original audio section identity".into());
    }
    Ok(())
}

/// Validates the declared section without recovering preroll from a rounded song snapshot.
pub(crate) fn validate_audio_config_with_speed(
    config: &crate::native_gameplay::AudioGameplayConfig,
    speed: LiveSpeedSetup,
) -> NativeGameplayResult<()> {
    validate_speed_config(&config.gameplay, Some(config.section_start), speed)
}

/// Checks explicit coordinate/history binding against the exact cold phase.
/// No native observation, input prefix or gameplay state is committed here.
pub(crate) fn validate_audio_coordinate_start(
    config: &crate::native_gameplay::CoordinateGameplayConfig,
    presentation: &crate::native_audio_presentation::NativeAudioPresentation,
    transport: &Transport,
    speed: Option<LiveSpeedSetup>,
) -> NativeGameplayResult<()> {
    match speed {
        Some(speed) => validate_audio_config_with_speed(&config.audio, speed)?,
        None => validate_audio_config(&config.audio)?,
    }
    presentation.validate_coordinate_seed()?;
    let authority = presentation.authority();
    let epoch = authority.epoch();
    let gameplay = &config.audio.gameplay;
    let identity = config.source_history;
    let phase = transport
        .output_phase_basis()
        .ok_or("coordinate gameplay requires an output-phase Transport")?;
    if gameplay.origin.domain != epoch.host_domain
        || gameplay.stream_origin != epoch.stream_origin
        || gameplay.playback_origin.domain != epoch.stream_origin.domain
        || phase.domain() != epoch.logical_origin.domain
        || identity.hz() != gameplay.sample_rate
        || phase.source_hz() != identity.hz()
        || phase.active_origin_frame() < identity.initial_playback()
    {
        return Err("coordinate gameplay clocks or source-history identity differ".into());
    }
    match authority
        .coordinate_basis()
        .ok_or("coordinate gameplay requires an original coordinate descriptor")?
    {
        crate::audio_authority::AudioAuthorityCoordinateBasis::SourceFrame(basis)
        | crate::audio_authority::AudioAuthorityCoordinateBasis::SourceCounter { basis, .. } => {
            if basis.sample_rate() != identity.hz()
                || basis.start_physical_frame() < identity.initial_physical()
            {
                return Err(
                    "coordinate creation basis differs from retained source history".into(),
                );
            }
        }
        crate::audio_authority::AudioAuthorityCoordinateBasis::TargetFrame(_) => {
            // validate_coordinate_seed retains the exact target creation basis.
            // Its device rate is independent of the source history's rate.
        }
        crate::audio_authority::AudioAuthorityCoordinateBasis::SourceTimestamp(_) => {
            return Err("native gameplay requires original native coordinate units".into());
        }
    }
    let (preroll, rate) = match speed {
        Some(speed) => (speed.preroll(), speed.speed().rate()),
        None => (
            beatkernel::time::Duration::from_nanos(
                config
                    .audio
                    .section_start
                    .as_nanos()
                    .checked_sub(gameplay.song_origin.as_nanos())
                    .ok_or("coordinate section preroll difference overflows")?,
            ),
            beatkernel::transport::Rate::NORMAL,
        ),
    };
    let constructor = if transport.runtime_output_rate().is_some() {
        Transport::new_with_runtime_output_phase
    } else {
        Transport::new_with_output_phase
    };
    let pristine = constructor(
        phase,
        epoch.logical_output(gameplay.playback_origin)?,
        config.audio.section_start,
        preroll,
        rate,
        gameplay.end_song,
    )?;
    if transport != &pristine {
        return Err("coordinate gameplay requires the exact pristine output phase".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_coordinate_binding_fixtures.rs"]
mod native_coordinate_binding_fixtures;

fn validate_speed_config(
    config: &NativeGameplayConfig,
    section_start: Option<Timestamp>,
    speed: LiveSpeedSetup,
) -> NativeGameplayResult<()> {
    validate_config(config)?;
    if speed.start().as_nanos() < 0
        || section_start.is_some_and(|start| start != speed.start())
        || config.song_origin != speed.song_origin()
        || config.end_song != speed.end()
    {
        return Err("native selected speed differs from original section or song origin".into());
    }
    speed.playback_end_frame(config.sample_rate)?;
    Ok(())
}

/// Checks the exact cold phase at the actual authority-mapped logical origin.
/// Integer anchor equality alone cannot distinguish rounded preroll phases.
pub(crate) fn validate_transport_with_speed(
    transport: &Transport,
    logical_origin: Timestamp,
    speed: LiveSpeedSetup,
) -> NativeGameplayResult<()> {
    let expected = PrerollTransportSetup {
        logical_origin,
        original_start: speed.start(),
        output_preroll: speed.preroll(),
        positive_rate: speed.speed().rate(),
        original_end: speed.end(),
    };
    // Equality also checks hidden command chronology: a no-op command can
    // advance it without adding an anchor. This bounded allocation is cold.
    let pristine = speed.transport(logical_origin)?;
    if transport.preroll_setup() != Some(expected) || transport != &pristine {
        return Err("native selected speed requires the exact pristine Transport phase".into());
    }
    Ok(())
}
