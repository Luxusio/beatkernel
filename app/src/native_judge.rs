//! Stage-separated native judge, completion and optional capture policy.
use crate::{
    completion::SongCompletion, input_sounds::InputSoundIdentity,
    native_gameplay::NativeGameplayResult, replay_capture::LiveReplayCapture, PreparedBms,
};
use beatkernel::{
    chart::CompiledChart,
    input::CodecLimits,
    judge::{JudgeEngine, JudgeProfile},
    replay::codec::ReplayCodecLimits,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms::{BmsChart, BmsInputMode};

pub struct NativeJudgeConfig {
    pub early: i64,
    pub late: i64,
    pub offset: i64,
    pub preroll: i64,
    pub output: ClockDomainId,
    pub end: Option<Timestamp>,
}
impl NativeJudgeConfig {
    /// Retain the builtin single grade-one window and signed input calibration.
    pub fn profile(&self) -> NativeGameplayResult<JudgeProfile> {
        Ok(
            crate::play_policy::ResolvedPlayPolicy::builtin(self.early, self.late, self.offset)
                .map_err(|error| -> Box<dyn std::error::Error> {
                    match error {
                        crate::play_policy::PolicyError::Judge(error) => Box::new(error),
                        other => Box::new(other),
                    }
                })?
                .into_parts()
                .0,
        )
    }
    /// Native simple timing uses one explicit PGREAT hit class; misses retain POOR.
    pub fn resolve_play_policy(
        &self,
        context: &crate::play_policy::OriginalGaugeContext,
        selection: crate::play_policy::GaugeSelection,
    ) -> NativeGameplayResult<crate::play_policy::ResolvedPlayPolicy> {
        use crate::play_policy::{ClassifiedWindow, GaugeSelection, ResolvedPlayPolicy};
        match selection {
            GaugeSelection::BeatKernel => {
                ResolvedPlayPolicy::builtin(self.early, self.late, self.offset).map_err(|error| {
                    match error {
                        crate::play_policy::PolicyError::Judge(error) => {
                            Box::new(error) as Box<dyn std::error::Error>
                        }
                        other => Box::new(other),
                    }
                })
            }
            GaugeSelection::Bms(kind) => {
                let profile = self.profile()?;
                Ok(ResolvedPlayPolicy::from_context(
                    context,
                    kind,
                    &[ClassifiedWindow {
                        judgment: beatkernel_bms::BmsJudgment::PGreat,
                        window: profile.windows()[0],
                    }],
                    self.offset,
                )?)
            }
        }
    }
    pub fn judge_with_policy(
        &self,
        source: &BmsChart,
        chart: CompiledChart,
        policy: &crate::play_policy::ResolvedPlayPolicy,
    ) -> NativeGameplayResult<JudgeEngine> {
        validate_source_timing(source, policy)?;
        Ok(crate::mine_plan::prepare_judge_with_timing(
            source,
            chart,
            policy.judge().clone(),
            BmsInputMode::ButtonOnly,
            beatkernel_bms::ParseOptions::default().max_objects,
            policy.timing().map(|timing| timing.profiles()),
        )?)
    }
    /// Constructs the actual source-aware pristine judge before native output
    /// starts. Mine timing and identity use the common ButtonOnly composition.
    pub fn judge(
        &self,
        source: &BmsChart,
        chart: CompiledChart,
    ) -> NativeGameplayResult<JudgeEngine> {
        Ok(crate::mine_plan::prepare_judge(
            source,
            chart,
            self.profile()?,
            BmsInputMode::ButtonOnly,
            beatkernel_bms::ParseOptions::default().max_objects,
        )?)
    }
    /// Finite sections use their own endpoint owner instead of a full-song deadline.
    pub fn completion(
        &self,
        prepared: &PreparedBms,
    ) -> NativeGameplayResult<Option<SongCompletion>> {
        if self.end.is_some() {
            return Ok(None);
        }
        Ok(Some(SongCompletion::prepare(
            prepared,
            self.late,
            self.offset,
            self.preroll,
            self.output,
        )?))
    }
    /// Uses the actual immutable selected stage deadline and calibration.
    pub fn completion_with_policy(
        &self,
        prepared: &PreparedBms,
        policy: &crate::play_policy::ResolvedPlayPolicy,
    ) -> NativeGameplayResult<Option<SongCompletion>> {
        if self.end.is_some() { return Ok(None); }
        Ok(Some(SongCompletion::prepare(prepared, policy.completion_late().as_nanos(),
            policy.judge().input_offset().as_nanos(), self.preroll, self.output)?))
    }
}
// Timing admission observes source declarations even when capture is disabled.
// Legacy policies continue to leave otherwise unused rank metadata untouched.
fn validate_source_timing(
    source: &BmsChart,
    policy: &crate::play_policy::ResolvedPlayPolicy,
) -> NativeGameplayResult<()> {
    if let Some(timing) = policy.timing() {
        let selected = source
            .judge_rank_metadata()?
            .resolve(timing.selection().precedence);
        if selected != Some(timing.profiles().difficulty()) {
            return Err(
                "selected timing differs from original source difficulty declaration".into(),
            );
        }
    }
    Ok(())
}

/// Refuse unsupported identity preparation before native/ghost/network acquisition.
pub fn validate_policy_competition(
    selection: crate::play_policy::GaugeSelection,
    options: &crate::competition_live::CompetitionOptions,
) -> NativeGameplayResult<()> {
    if selection != crate::play_policy::GaugeSelection::BeatKernel && options.network.is_some() {
        return Err("nondefault gauges currently require disabled multiplayer competition".into());
    }
    Ok(())
}
/// Build checked selected meanings before optional recording or network acquisition.
#[allow(clippy::too_many_arguments)]
pub fn prepare_policy_header(
    source: &BmsChart,
    judge: &JudgeEngine,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    domain: ClockDomainId,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
) -> NativeGameplayResult<beatkernel::replay::ReplayHeader> {
    if judge.effective_song_time().is_some() || judge.profile() != policy.judge() {
        return Err("selected competition requires a pristine matching judge".into());
    }
    validate_source_timing(source, policy)?;
    policy.validate_timing(judge, BmsInputMode::ButtonOnly)?;
    let limits = crate::competition_live::replay_limits()?;
    let header = crate::replay_capture::setup_play_policy_header(
        judge,
        domain,
        limits,
        start,
        chart_seed,
        end,
        BmsInputMode::ButtonOnly,
        InputSoundIdentity::from_source(source)?,
        policy,
    )?;
    let file = beatkernel::replay::codec::ReplayFile::new(header, Vec::new());
    crate::replay_playback::validate_section_setup(source, &file, limits)?;
    Ok(file.header)
}

/// Network endpoint agreement around unchanged canonical selected setup meanings.
#[allow(clippy::too_many_arguments)]
pub fn prepare_policy_competition_identity(
    source: &BmsChart,
    judge: &JudgeEngine,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    domain: ClockDomainId,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
) -> NativeGameplayResult<Vec<u8>> {
    let header = prepare_policy_header(source, judge, policy, domain, start, chart_seed, None)?;
    Ok(crate::multiplayer::competition_identity_for_section(
        &header,
        env!("CARGO_PKG_VERSION"),
        crate::competition_live::replay_limits()?,
        end,
    )?)
}

/// Disabled recording does not validate otherwise unused recording settings.
pub fn capture_limits(
    enabled: bool,
    max_bytes: usize,
    max_records: usize,
) -> NativeGameplayResult<Option<ReplayCodecLimits>> {
    if !enabled {
        return Ok(None);
    }
    Ok(Some(ReplayCodecLimits::new(
        max_bytes,
        max_records,
        4096,
        CodecLimits::new(65536, 32768)?,
    )?))
}
/// Called at the existing cleanup-protected capture stage with the pristine judge.
pub fn prepare_capture(
    judge: &JudgeEngine,
    domain: ClockDomainId,
    start: Timestamp,
    chart_seed: u64,
    limits: Option<ReplayCodecLimits>,
) -> NativeGameplayResult<Option<LiveReplayCapture>> {
    limits
        .map(|limits| {
            Ok(LiveReplayCapture::new_at_with_chart_seed(
                judge, domain, limits, start, chart_seed,
            )?)
        })
        .transpose()
}

/// Captures the actual selected source identity at the same native setup stage.
/// Disabled recording does not validate unused invisible source metadata.
pub fn prepare_capture_for_source(
    source: &BmsChart,
    judge: &JudgeEngine,
    domain: ClockDomainId,
    start: Timestamp,
    chart_seed: u64,
    limits: Option<ReplayCodecLimits>,
) -> NativeGameplayResult<Option<LiveReplayCapture>> {
    prepare_section_capture_for_source(source, judge, domain, start, chart_seed, None, limits)
}

/// Preserve the actual finite section endpoint in the pristine capture header.
pub fn prepare_section_capture_for_source(
    source: &BmsChart,
    judge: &JudgeEngine,
    domain: ClockDomainId,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
    limits: Option<ReplayCodecLimits>,
) -> NativeGameplayResult<Option<LiveReplayCapture>> {
    let Some(limits) = limits else {
        return Ok(None);
    };
    let identity = InputSoundIdentity::from_source(source)?;
    Ok(Some(LiveReplayCapture::new_with_input_sounds(
        judge,
        domain,
        limits,
        start,
        chart_seed,
        end,
        BmsInputMode::ButtonOnly,
        identity,
    )?))
}

/// Canonical native capture for a pristine judge paired with a resolved policy.
/// Disabled capture checks policy/lifetime, but does not acquire source identity.
#[allow(clippy::too_many_arguments)]
pub fn prepare_section_capture_for_policy(
    source: &BmsChart,
    judge: &JudgeEngine,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    domain: ClockDomainId,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
    limits: Option<ReplayCodecLimits>,
) -> NativeGameplayResult<Option<LiveReplayCapture>> {
    if judge.effective_song_time().is_some() || judge.profile() != policy.judge() {
        return Err("native policy capture requires a pristine matching judge profile".into());
    }
    validate_source_timing(source, policy)?;
    policy.validate_timing(judge, BmsInputMode::ButtonOnly)?;
    let Some(limits) = limits else {
        return Ok(None);
    };
    let identity = InputSoundIdentity::from_source(source)?;
    Ok(Some(LiveReplayCapture::new_with_policy(
        judge,
        domain,
        limits,
        start,
        chart_seed,
        end,
        BmsInputMode::ButtonOnly,
        identity,
        policy,
    )?))
}

#[cfg(test)]
use beatkernel::{judge::JudgeGrade, time::Duration};
#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        audio::*,
        input::*,
        runtime::{Runtime, SoundBinding},
        time::{ClockMapper, ClockMappingQuality, ClockPoint},
        transport::{Rate, Transport},
    };
    use beatkernel_bms::{ParseOptions, parse};
    fn config() -> NativeJudgeConfig {
        NativeJudgeConfig {
            early: 11,
            late: 23,
            offset: -19,
            preroll: 0,
            output: ClockDomainId(7),
            end: None,
        }
    }
    fn prepared() -> PreparedBms {
        let source = parse(
            "#BPM 120\n#WAV01 head.wav\n#00011:01\n",
            ParseOptions::default(),
        )
        .unwrap();
        let compiled = source.compile().unwrap();
        let limits = PcmLimits::new(64, 64, 1).unwrap();
        let mut bank = SampleBank::new(AudioFormat::new(48000, 1).unwrap(), limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(AudioFormat::new(1, 1).unwrap(), vec![0.5, 0.25], limits).unwrap(),
        )
        .unwrap();
        let sounds = vec![SoundBinding {
            object: compiled.chart.objects()[0].id,
            stage: beatkernel::judge::JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 1.0,
        }];
        PreparedBms {
            source,
            compiled,
            bank,
            sounds,
            bgm_commands: vec![],
        }
    }
    fn judge(prepared: &PreparedBms, config: &NativeJudgeConfig) -> JudgeEngine {
        JudgeEngine::new(
            prepared.compiled.chart.clone(),
            prepared.source.rules(),
            config.profile().unwrap(),
        )
        .unwrap()
    }
    fn point(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(17),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn input(ns: i64) -> PhysicalInputEvent {
        PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(3), point(ns), 0),
            control: PhysicalControlId::keyboard(7),
            state: ButtonState::Down,
        })
    }
    #[test]
    fn asymmetry_and_signed_offset_are_actual_judge_deadlines_and_hits() {
        let p = prepared();
        let cfg = config();
        let mut left = judge(&p, &cfg);
        let mut right = judge(&p, &cfg);
        let game = GameInputEvent {
            game_control: GameControlId(0x11),
            physical: input(19),
        };
        let hits = left.push_input(&game, Timestamp::from_nanos(19)).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(matches!(
            hits[0].outcome,
            beatkernel::judge::JudgeOutcome::Hit {
                grade: JudgeGrade(1),
                ..
            }
        ));
        assert_eq!(hits[0].at, Timestamp::ZERO);
        assert!(
            right
                .advance_to(Timestamp::from_nanos(42))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            right.advance_to(Timestamp::from_nanos(43)).unwrap().len(),
            1
        );
        let mut positive = config();
        positive.offset = 19;
        let mut positive_judge = judge(&p, &positive);
        assert!(
            positive_judge
                .advance_to(Timestamp::from_nanos(4))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            positive_judge
                .advance_to(Timestamp::from_nanos(5))
                .unwrap()
                .len(),
            1
        );
        let mut early_judge = judge(&p, &cfg);
        let early_input = GameInputEvent {
            game_control: GameControlId(0x11),
            physical: input(8),
        };
        let early = early_judge
            .push_input(&early_input, Timestamp::from_nanos(8))
            .unwrap();
        assert!(
            matches!(early[0].outcome, beatkernel::judge::JudgeOutcome::Hit {delta, ..} if delta.as_nanos() == -11)
        );
        for (early, late) in [(-1, 23), (11, -1)] {
            let mut bad = config();
            bad.early = early;
            bad.late = late;
            assert!(bad.profile().is_err());
        }
    }
    #[test]
    fn completion_uses_source_pcm_extent_and_requires_real_later_render_presentation() {
        let p = prepared();
        let mut cfg = config();
        cfg.early = -1; // completion must not eagerly validate profile.
        let mut completion = cfg.completion(&p).unwrap().unwrap();
        assert_eq!(completion.calibration_seconds(), 3); // 2 source frames at1Hz plus inclusive late edge.
        let mut engine = judge(&p, &config());
        engine.advance_to(Timestamp::from_nanos(43)).unwrap();
        let (_, consumer) = command_queue(1).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                p.bank.format(),
                cfg.output,
                Timestamp::ZERO,
                AudioLimits::new(1, 1, 1, 1, 1).unwrap(),
            ),
            p.bank,
            consumer,
        )
        .unwrap();
        let bgm = crate::bgm::BgmFeeder::new(
            vec![],
            crate::bgm::BgmConfig {
                output_origin: ClockPoint {
                    domain: cfg.output,
                    timestamp: Timestamp::ZERO,
                },
                sample_rate: 48000,
                preroll: Duration::ZERO,
                lookahead: Duration::from_nanos(1),
                max_pending: 1,
            },
        )
        .unwrap()
        .report();
        let first = mixer.render(&mut [0.0]).unwrap();
        assert!(
            !completion
                .observe(&engine, Timestamp::from_nanos(43), bgm, Some(first), None)
                .unwrap()
        );
        let second = mixer.render(&mut [0.0]).unwrap();
        let native = |n| {
            Some(ClockPoint {
                domain: cfg.output,
                timestamp: Timestamp::from_nanos(n),
            })
        };
        assert!(
            !completion
                .observe(
                    &engine,
                    Timestamp::from_nanos(43),
                    bgm,
                    Some(second),
                    native(41666)
                )
                .unwrap()
        );
        assert!(
            completion
                .observe(
                    &engine,
                    Timestamp::from_nanos(43),
                    bgm,
                    Some(second),
                    native(41667)
                )
                .unwrap()
        );
        cfg.end = Some(Timestamp::ZERO);
        cfg.late = -1;
        cfg.preroll = -1;
        assert!(cfg.completion(&prepared()).unwrap().is_none());
    }
    #[test]
    fn disabled_settings_are_inert_and_enabled_limits_retain_exact_bounds() {
        assert!(capture_limits(false, 0, 0).unwrap().is_none());
        assert!(capture_limits(true, 0, 1).is_err());
        assert!(capture_limits(true, 4095, 1).is_err());
        assert!(capture_limits(true, 8192, 0).is_err());
        let limits = capture_limits(true, 8192, 8).unwrap().unwrap();
        assert_eq!(limits.max_file_bytes(), 8192);
        assert_eq!(limits.max_records(), 8);
        assert_eq!(limits.max_header_bytes(), 4096);
        assert_eq!(limits.input_limits().max_encoded_bytes(), 65536);
        assert_eq!(limits.input_limits().max_payload_bytes(), 32768);
        let mut engine = judge(&prepared(), &config());
        engine.advance_to(Timestamp::ZERO).unwrap();
        assert!(
            prepare_capture(
                &engine,
                ClockDomainId(0),
                Timestamp::from_nanos(-1),
                u64::MAX,
                None
            )
            .unwrap()
            .is_none()
        );
        assert!(
            prepare_capture(&engine, ClockDomainId(17), Timestamp::ZERO, 0, Some(limits)).is_err()
        );
        assert!(
            prepare_capture(
                &judge(&prepared(), &config()),
                ClockDomainId(17),
                Timestamp::from_nanos(-1),
                0,
                Some(limits)
            )
            .is_err()
        );
    }
    #[test]
    fn capture_preserves_legacy_bytes_and_original_seed_start_domain() {
        let p = prepared();
        let engine = judge(&p, &config());
        let limits = capture_limits(true, 8192, 8).unwrap().unwrap();
        let old = LiveReplayCapture::new(&engine, ClockDomainId(17), limits)
            .unwrap()
            .into_file();
        let common = prepare_capture(&engine, ClockDomainId(17), Timestamp::ZERO, 0, Some(limits))
            .unwrap()
            .unwrap()
            .into_file();
        assert_eq!(
            beatkernel::replay::codec::encode_replay(&old, limits).unwrap(),
            beatkernel::replay::codec::encode_replay(&common, limits).unwrap()
        );
        let capture = prepare_capture(
            &engine,
            ClockDomainId(23),
            Timestamp::from_nanos(2),
            u64::MAX,
            Some(limits),
        )
        .unwrap()
        .unwrap();
        let (_, start, seed) =
            crate::replay_playback::decode_chart_setup(&capture.header().options).unwrap();
        assert_eq!(start, Timestamp::from_nanos(2));
        assert_eq!(seed, u64::MAX);
        assert_eq!(capture.header().normalized_clock, ClockDomainId(23));
    }
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    #[test]
    fn actual_runtime_reports_capture_and_reconstruct_the_accepted_prefix() {
        let p = prepared();
        let mut cfg = config();
        cfg.offset = 0;
        let engine = judge(&p, &cfg);
        let limits = capture_limits(true, 8192, 8).unwrap().unwrap();
        let mut capture =
            prepare_capture(&engine, ClockDomainId(17), Timestamp::ZERO, 3, Some(limits))
                .unwrap()
                .unwrap();
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Exact(DeviceId(3)),
            physical: PhysicalControlId::keyboard(7),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let (producer, _consumer) = command_queue(1).unwrap();
        let mut runtime = Runtime::new(
            ClockDomainId(17),
            ClockDomainId(17),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            bindings,
            engine,
            producer,
            vec![],
            0,
        )
        .unwrap();
        let hit = runtime
            .process_input(input(0), &Identity, point(0))
            .unwrap();
        capture.record_report(&hit).unwrap();
        let deadline = runtime.advance_to(point(1), &Identity, point(1)).unwrap();
        capture.record_report(&deadline).unwrap();
        let file = capture.into_file();
        let mut replay = crate::replay_playback::reconstruct(&p.source, file, limits).unwrap();
        replay.seek_cursor(2).unwrap();
        assert_eq!(replay.results(), hit.judge_events);
    }
}
