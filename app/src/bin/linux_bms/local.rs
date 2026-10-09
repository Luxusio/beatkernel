//! Actual Linux local cohort: independent input/judging, one native audio owner.
use super::native::{
    audio_timing_config, observe_target_output, open_target_output, output_origin, startup_input,
    startup_render, target_audio_timing_config, TargetStartupDevice, HOST, LOGICAL, OUTPUT,
};
use super::*;
#[cfg(test)]
use beatkernel::input::PhysicalControlId;
use beatkernel::{
    audio::PcmLimits,
    input::DeviceId,
    time::Duration,
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::native_audio::{prepare_audio, NativeAudioConfig, PreparedNativeAudio};
use beatkernel_bms_runtime::native_cohort_setup::{
    activate_audio_cohort_with_sounds, admit_cohort as admit_mode,
    finish_cohort_with_results_and_network, prepare_audio_cohort_with_policy, CohortPreparation,
    PreparedCohort,
};
#[cfg(test)]
use beatkernel_bms_runtime::{
    competition::ScoreSummary,
    local_input::InputMerger,
    native_cohort::{replay_path, PlayerState},
};
use beatkernel_bms_runtime::{
    competition_live::CompetitionOptions,
    native_chart::{prepare_chart_with_policy, NativeChartConfig},
    native_end::NativeEnd,
    playback_pause::NativePause,
    ChannelPolicy,
};
#[cfg(test)]
use beatkernel_bms_runtime::{
    local_players::PlayerId,
    native_cohort::{finite_cohort_done, lag_reaches},
    playback_pause::PauseKeyboard,
};
use beatkernel_bms_runtime::{
    native_cohort::{run_cohort_audio_with_policies_and_results, NativeAudioCohortSession},
    native_gameplay::{
        AudioGameplayConfig, InputBatch, NativeCollectedInput, NativeGameplayConfig,
        NativeGameplayDevice, NativeGameplayResult,
    },
};
use beatkernel_platform::{
    audio::{
        presentation::discipline::{DisciplineConfig, PresentationDiscipline},
        DeviceFormat, SampleEncoding,
    },
    linux::{AlsaRequest, MonotonicClock},
};

use beatkernel_bms_runtime::native_start::{start_target_committed, NativeStartConfig};

use beatkernel_bms_runtime::{
    gameplay_output_owner::GameplayOutputOwner, native_alsa_replacement::AlsaReplacementBackend,
};
type OwnedOutput = GameplayOutputOwner<
    beatkernel_bms_runtime::gameplay::output::adapters::remix::RemixedOutputBackend<
        AlsaReplacementBackend,
    >,
    beatkernel_platform::audio::NativeOutputState,
>;
use beatkernel_bms_runtime::native_alsa_output_ui::{
    ConvertedAlsaOutputOwner, ConvertedAlsaOutputUi,
};
enum CohortOutput {
    Legacy(
        OwnedOutput,
        beatkernel_bms_runtime::native_alsa_output_ui::NativeAlsaOutputUi,
    ),
    Target(ConvertedAlsaOutputOwner, ConvertedAlsaOutputUi),
}
impl CohortOutput {
    fn legacy_mut(&mut self) -> Result<&mut OwnedOutput> {
        match self {
            Self::Legacy(owner, _) => Ok(owner),
            Self::Target(..) => Err("committed startup requires the legacy source stream".into()),
        }
    }
    fn target(&self) -> bool {
        matches!(self, Self::Target(..))
    }
    fn configuration(&self) -> Result<&beatkernel_platform::linux::AlsaAppliedConfig> {
        match self {
            Self::Legacy(owner, _) => Ok(owner
                .current()
                .ok_or("ALSA output missing")?
                .stream()
                .configuration()),
            Self::Target(owner, _) => Ok(owner
                .current()
                .ok_or("target ALSA output missing")?
                .stream()
                .configuration()),
        }
    }
    fn start(&mut self) -> Result<()> {
        match self {
            Self::Legacy(owner, _) => owner
                .current_mut()
                .ok_or("ALSA output missing")?
                .stream_mut()
                .start()?,
            Self::Target(owner, _) => owner
                .current_mut()
                .ok_or("target ALSA output missing")?
                .stream_mut()
                .start()?,
        };
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        match self {
            Self::Legacy(owner, _) => owner.stop()?,
            Self::Target(owner, _) => owner.stop()?,
        };
        Ok(())
    }
    fn render_report(&self) -> Option<beatkernel::audio::RenderReport> {
        match self {
            Self::Legacy(owner, _) => owner
                .current()
                .and_then(|o| o.stream().last_render_report())
                .or(owner.render_report()),
            Self::Target(owner, _) => owner
                .current()
                .and_then(|o| o.stream().last_real_source_report())
                .or(owner.render_report()),
        }
    }
    fn timing_snapshot(&self) -> Option<beatkernel_platform::linux::AlsaTimingSnapshot> {
        match self {
            Self::Legacy(owner, _) => owner.current().and_then(|o| o.stream().timing_snapshot()),
            Self::Target(owner, _) => owner.current().and_then(|o| o.stream().timing_snapshot()),
        }
    }
    fn snapshot(&self) -> Option<beatkernel_platform::linux::AlsaSnapshot> {
        match self {
            Self::Legacy(owner, _) => owner.current().map(|o| o.stream().snapshot()),
            Self::Target(owner, _) => owner.current().map(|o| o.stream().snapshot()),
        }
    }
    fn pending(&self) -> bool {
        match self {
            Self::Legacy(owner, ui) => owner.replacement_pending() || ui.pending(),
            Self::Target(owner, ui) => owner.replacement_pending() || ui.pending(),
        }
    }
    fn suspended(&self) -> bool {
        match self {
            Self::Legacy(owner, _) => owner.output_clock_suspended(),
            Self::Target(owner, _) => owner.output_clock_suspended(),
        }
    }
    fn observe_audio(
        &mut self,
        presentation: &mut beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
    ) -> Result<()> {
        match self {
            Self::Legacy(owner, _) => owner.observe_native(presentation)?,
            Self::Target(owner, _) => owner.observe_target_native(presentation)?,
        };
        Ok(())
    }
    fn audio_pause(
        &mut self,
        presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        now: ClockPoint,
    ) -> Result<beatkernel_bms_runtime::live_pause::LivePauseObservation> {
        Ok(match self {
            Self::Legacy(owner, _) => owner.audio_pause_observation(presentation, now)?,
            Self::Target(owner, _) => owner.audio_pause_observation_target(presentation, now)?,
        })
    }
    fn audio_end(
        &mut self,
        end: &mut beatkernel_bms_runtime::native_end::NativeEnd,
        presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
    ) -> Result<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
        match self {
            Self::Legacy(owner, _) => owner.observe_audio_end(end, presentation),
            Self::Target(owner, _) => owner.observe_target_audio_end(end, presentation),
        }
    }
    fn service_audio(
        &mut self,
        context: beatkernel_bms_runtime::gameplay_presentation::GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> Result<bool> {
        match self {
            Self::Legacy(owner, ui) => ui.service_audio(owner, context, now),
            Self::Target(owner, ui) => ui.service_audio(owner, context, now),
        }
    }
}
struct CohortDevice<'a> {
    output: &'a mut CohortOutput,
    inputs: &'a mut beatkernel_bms_runtime::native_input::NativeInputCollector,
    clock: &'a MonotonicClock,
    retained: &'a mut NativeCollectedInput,
    startup_end: Option<&'a mut NativeEnd>,
    startup_primed: bool,
}
impl NativeGameplayDevice for CohortDevice<'_> {
    fn observe_audio(
        &mut self,
        presentation: &mut beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
    ) -> NativeGameplayResult<()> {
        self.output.observe_audio(presentation)
    }
    fn audio_pause_observation(
        &mut self,
        presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        now: ClockPoint,
    ) -> NativeGameplayResult<beatkernel_bms_runtime::live_pause::LivePauseObservation> {
        Ok(self.output.audio_pause(presentation, now)?)
    }
    fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        _: Option<beatkernel::audio::RenderReport>,
    ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
        self.output.audio_end(end, presentation)
    }
    fn publish_paused_audio_output(
        &mut self,
        context: beatkernel_bms_runtime::gameplay_presentation::GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        self.output.service_audio(context, now)
    }

    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        Ok(self.output.legacy_mut()?.observe(discipline)?)
    }
    fn output_clock_suspended(&self) -> bool {
        self.output.suspended()
    }
    fn output_replacement_pending(&self) -> bool {
        self.output.pending()
    }
    fn publish_paused_output(
        &mut self,
        context: beatkernel_bms_runtime::gameplay_presentation::GameplayOutputContext<
            '_,
            PresentationDiscipline,
        >,
    ) -> NativeGameplayResult<bool> {
        match self.output {
            CohortOutput::Legacy(owner, ui) => ui.service(owner, context, self.clock.now()?),
            CohortOutput::Target(..) => {
                Err("target output requires typed audio publication".into())
            }
        }
    }
    fn pause_observation(
        &mut self,
        pair: beatkernel::time::ClockPair,
    ) -> NativeGameplayResult<beatkernel_bms_runtime::live_pause::LivePauseObservation> {
        Ok(self
            .output
            .legacy_mut()?
            .pause_observation(pair, pair.target)?)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
        Ok(self.output.render_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.clock.now()?)
    }
    fn acquire(
        &mut self,
        events: &mut std::collections::VecDeque<beatkernel::input::PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        self.retained.acquire(self.inputs, events, 256)
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
        report: Option<beatkernel::audio::RenderReport>,
    ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
        let _ = report;
        self.output.legacy_mut()?.observe_end(end, discipline)
    }
    fn seed_resume(
        &mut self,
        discipline: &mut PresentationDiscipline,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        let _ = reference;
        Ok(self.output.legacy_mut()?.seed_resume(discipline)?)
    }
    fn set_audio_held(&mut self, held: bool) -> NativeGameplayResult<()> {
        if let CohortOutput::Target(owner, _) = self.output {
            owner.set_target_held(held)?;
        }
        Ok(())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Err("ALSA local cohorts use logical mixer scheduling".into())
    }
}

impl beatkernel_bms_runtime::native_audio_startup::NativeAudioSeedPort for CohortDevice<'_> {
    fn service_input(&mut self) -> NativeGameplayResult<bool> {
        let mut ignored = 0;
        startup_input(self.inputs, &mut ignored, self.retained, true)
    }
    fn observe_audio(
        &mut self,
        presentation: &mut beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
    ) -> NativeGameplayResult<()> {
        match self.output {
            CohortOutput::Legacy(owner, _) => {
                owner.observe_native(presentation)?;
                if !self.startup_primed {
                    if let Some(record) = presentation.latest_record() {
                        if let Some(end) = self.startup_end.as_deref_mut() {
                            end.prime(
                                startup_render(
                                    owner
                                        .current()
                                        .ok_or("startup ALSA output missing")?
                                        .stream(),
                                )?,
                                record.pair(),
                            )?;
                        }
                        self.startup_primed = true;
                    }
                }
            }
            CohortOutput::Target(owner, _) => {
                let primed = observe_target_output(
                    owner,
                    presentation,
                    if self.startup_primed {
                        None
                    } else {
                        self.startup_end.as_deref_mut()
                    },
                )?;
                self.startup_primed |= primed;
            }
        }
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
        match self.output {
            CohortOutput::Legacy(owner, _) => startup_render(
                owner
                    .current()
                    .ok_or("startup ALSA output missing")?
                    .stream(),
            ),
            CohortOutput::Target(..) => Ok(self.output.render_report()),
        }
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.clock.now()?)
    }
    fn wait(&mut self, duration: std::time::Duration) -> NativeGameplayResult<()> {
        std::thread::sleep(duration);
        Ok(())
    }
}

pub(super) fn run(
    options: Options,
    competition_options: CompetitionOptions,
    launch: beatkernel_bms_runtime::session_launch::SessionLaunch,
) -> Result<()> {
    admit_mode(options.local_inputs.len(), false)?;
    let retained = competition_options.network.is_none();
    let playback_end = if retained {
        None
    } else {
        options.playback_end()?
    };
    let count = options.local_inputs.len();
    if options.local_players.len() != count {
        return Err("local player and native input assignment counts differ".into());
    }
    let song_origin = options.song_origin()?;
    // Validate both checked endpoint grids before loading assets or opening owners.
    let mut pause = NativePause::new(output_origin(), HOST, options.format.sample_rate())?;
    if let Some(end) = playback_end {
        pause = pause.with_playback_end_frame(end)?;
    }
    let mut native_end = playback_end
        .map(|end| NativeEnd::new(output_origin(), HOST, options.format.sample_rate(), end))
        .transpose()?;
    let clock = MonotonicClock::new(HOST);
    // Declared before native owners so delivery observations report after cleanup.
    let mut delivery = DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry::new(
        4096, HOST,
    )?);
    let judge_config = beatkernel_bms_runtime::native_judge::NativeJudgeConfig {
        early: options.early,
        late: options.late,
        offset: options.offset,
        preroll: options.preroll,
        output: OUTPUT,
        end: options.end_ns.map(Timestamp::from_nanos),
    };
    let chart_config = NativeChartConfig {
        path: &options.chart,
        format: options.format,
        limits: PcmLimits::new(
            64 * 1024 * 1024,
            256 * 1024 * 1024,
            beatkernel_bms_runtime::DEFAULT_BMS_PCM_SAMPLES,
        )?,
        channels: if options.mono_stereo {
            ChannelPolicy::MonoToStereo
        } else {
            ChannelPolicy::Exact
        },
        chart_seed: options.chart_seed,
        start: Timestamp::from_nanos(options.start_ns),
        bindings: &options.bindings,
    };
    let (mut prepared, initial_source, section, policy) = if retained {
        let prepared = beatkernel_bms_runtime::native_chart::prepare_retained_chart_with_policy(
            chart_config,
            &judge_config,
            options.gauge,
            options.timing,
        )?;
        (
            prepared.original,
            Some(prepared.initial_source),
            prepared.section,
            prepared.policy,
        )
    } else {
        let (prepared, section, policy) =
            prepare_chart_with_policy(chart_config, &judge_config, options.gauge, options.timing)?;
        (prepared, None, section, policy)
    };
    let completion_template = if retained {
        Some(beatkernel_bms_runtime::completion::SongCompletion::prepare(
            &prepared,
            policy.completion_late().as_nanos(),
            policy.judge().input_offset().as_nanos(),
            options.preroll,
            OUTPUT,
        )?)
    } else {
        None
    };
    let max_target = completion_template.as_ref().map(|template| {
        template.song_extent().max(
            options
                .end_ns
                .map(Timestamp::from_nanos)
                .unwrap_or(Timestamp::ZERO),
        )
    });
    // Select judge identities while retaining original PCM and full sound maps.
    let original_source = if let Some(source) = initial_source {
        let chart = source.source.compile()?;
        let source = std::mem::replace(&mut prepared.source, source);
        let chart = std::mem::replace(&mut prepared.compiled.chart, chart);
        Some((source, chart))
    } else {
        None
    };
    // The legacy cohort preparer pairs every SoundBinding with a selected
    // judge object. Filter only this cold preparation view, then restore the
    // original mappings for backward practice before native playback starts.
    let original_sounds = if retained {
        let selected_objects: std::collections::BTreeSet<_> = prepared
            .compiled
            .chart
            .objects()
            .iter()
            .map(|object| object.id)
            .collect();
        let sounds = std::mem::take(&mut prepared.sounds);
        prepared.sounds.try_reserve_exact(sounds.len())?;
        prepared.sounds.extend(
            sounds
                .iter()
                .copied()
                .filter(|sound| selected_objects.contains(&sound.object)),
        );
        Some(sounds)
    } else {
        None
    };
    let initial_completion_source = retained.then(|| prepared.source.clone());
    println!("prepared practice section={section:?}");
    if let Some(timing) = policy.timing() {
        println!(
            "applied timing preset={} precedence={:?} difficulty={:?} semantics={}",
            timing.selection().preset.id(),
            timing.selection().precedence,
            timing.profiles().difficulty(),
            timing.interaction_semantics()
        );
    }
    for warning in &prepared.source.warnings {
        eprintln!("BMS warning line{}: {}", warning.line, warning.message);
    }
    beatkernel_bms_runtime::player::publish_native_chart(
        &options.chart,
        &prepared.source,
        &prepared.compiled.chart,
        &options.local_players,
    )?;
    if beatkernel_bms_runtime::player::cancelled() {
        return Ok(());
    }
    let assignments = options
        .local_players
        .iter()
        .enumerate()
        .map(|(index, &player)| Ok((player, DeviceId(u64::try_from(index + 1)?))))
        .collect::<Result<Vec<_>>>()?;
    let PreparedCohort {
        mut network,
        mut configs,
        mut input_sounds,
        mut hazard_sounds,
        mut states,
        mut save_paths,
        reserved,
    } = prepare_audio_cohort_with_policy(
        &prepared,
        &assignments,
        &competition_options,
        &CohortPreparation {
            host: HOST,
            output: OUTPUT,
            early: policy.judge().max_early().as_nanos(),
            late: policy.judge().max_late().as_nanos(),
            offset: policy.judge().input_offset().as_nanos(),
            preroll: options.preroll,
            start: Timestamp::from_nanos(options.start_ns),
            end: options.end_ns.map(Timestamp::from_nanos),
            chart_seed: options.chart_seed,
            bindings: &options.bindings,
            record_replay: options.record_replay.as_deref(),
            replay_max_bytes: options.replay_max_bytes,
            replay_max_records: options.replay_max_records,
        },
        LOGICAL,
        &policy,
    )?;
    if let Some((source, chart)) = original_source {
        prepared.source = source;
        prepared.compiled.chart = chart;
        prepared.sounds = original_sounds.ok_or("retained cohort original sounds missing")?;
        let first = reserved
            .iter()
            .map(|voice| voice.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("cohort voice namespace overflow")?;
        let mut allocator = beatkernel_bms_runtime::local_runtime::VoiceAllocator::new(first);
        for member in &mut configs {
            let mut sounds = Vec::new();
            sounds.try_reserve_exact(prepared.sounds.len())?;
            sounds.extend_from_slice(&prepared.sounds);
            allocator.remap(&mut sounds)?;
            member.sounds = sounds;
        }
        if options.end_ns.is_none() {
            let template = completion_template
                .as_ref()
                .ok_or("retained cohort completion template missing")?;
            let source = initial_completion_source
                .as_ref()
                .ok_or("retained cohort selected source missing")?;
            for state in &mut states {
                let member = configs
                    .iter()
                    .find(|member| member.player == state.player)
                    .ok_or("retained completion player missing")?;
                state.completion = Some(template.prepare_practice(source, &member.judge)?);
            }
        }
        input_sounds = beatkernel_bms_runtime::local_preparation::prepare_local_input_sounds(
            &prepared, &configs, &reserved,
        )?;
        hazard_sounds = beatkernel_bms_runtime::local_preparation::prepare_local_mine_sounds(
            &prepared,
            &configs,
            &reserved,
            &input_sounds,
        )?;
    }
    let audio_config = NativeAudioConfig {
        output_origin: output_origin(),
        start: Timestamp::from_nanos(options.start_ns),
        preroll: Duration::from_nanos(options.preroll),
        lookahead: Duration::from_nanos(options.bgm_lookahead),
        voices: options.voices,
        max_render_frames: AudioLimits::MAX_RENDER_FRAMES,
        playback_end_frame: playback_end,
        gated_start: network.is_some(),
    };
    let (audio, mut practice) = if retained {
        let region = beatkernel::audio::PracticeRegion::new(
            song_origin,
            options
                .end_ns
                .map(Timestamp::from_nanos)
                .unwrap_or(Timestamp::from_nanos(i64::MAX)),
            false,
        )?;
        let mut members = Vec::new();
        members.try_reserve_exact(assignments.len())?;
        for (player, _) in &assignments {
            let member_policy = match options.timing {
                Some(timing) => {
                    beatkernel_bms_runtime::play_policy::ResolvedPlayPolicy::with_timing(
                        &prepared.source,
                        options.gauge,
                        timing,
                        options.offset,
                    )?
                }
                None => judge_config.resolve_play_policy(&section.original_gauge, options.gauge)?,
            };
            let path = save_paths
                .iter()
                .find(|(id, _)| id == player)
                .ok_or("missing local recording member")?
                .1
                .as_ref();
            let member_launch = if let Some(path) = path {
                let original_base = launch
                    .original_args()
                    .chunks_exact(2)
                    .find(|pair| pair[0] == "--record-replay")
                    .ok_or("original local recording base missing")?;
                let original_path = beatkernel_bms_runtime::native_cohort::replay_path(
                    std::path::Path::new(&original_base[1]),
                    *player,
                )?;
                launch.for_recording_path(original_path, path.clone())?
            } else {
                launch.clone()
            };
            members.push(beatkernel_bms_runtime::practice_playback::PracticeMember {
                player: *player,
                policy: member_policy,
                launch: member_launch,
                chart_seed: options.chart_seed,
                capture_limits: beatkernel_bms_runtime::native_judge::capture_limits(
                    path.is_some(),
                    options.replay_max_bytes,
                    options.replay_max_records,
                )?,
            });
        }
        let original = prepared.source.clone();
        let overlap = beatkernel_bms_runtime::native_audio::required_bgm_overlap(
            &prepared.bank,
            &prepared.bgm_commands,
        )?;
        let gameplay_voices = options
            .voices
            .checked_sub(overlap)
            .ok_or("BGM overlap exceeds total voice budget")?;
        if gameplay_voices == 0
            && (!prepared.sounds.is_empty()
                || !prepared.source.invisible.is_empty()
                || !prepared.source.mines.is_empty())
        {
            return Err("total voice budget leaves no gameplay voice".into());
        }
        let limits = beatkernel::audio::PracticeLimits::new(
            prepared.bgm_commands.len().max(1),
            overlap,
            AudioLimits::MAX_COMMANDS,
            8,
            256,
        )?;
        let prepared_audio = beatkernel_bms_runtime::native_audio::prepare_retained_audio(
            prepared.bank,
            prepared.bgm_commands,
            NativeAudioConfig {
                voices: gameplay_voices,
                ..audio_config
            },
            beatkernel_bms_runtime::native_audio::NativePracticeAudioConfig { region, limits },
        )?;
        let practice =
            beatkernel_bms_runtime::practice_playback::PracticePlayback::new_with_completion(
                prepared_audio.practice,
                original,
                members,
                region,
                max_target.unwrap(),
                prepared_audio.audio.mixer.output_frame_basis(),
                u64::from(options.buffer),
                options.end_ns.map(Timestamp::from_nanos),
            )?;
        (prepared_audio.audio, Some(practice))
    } else {
        (
            prepare_audio(prepared.bank, prepared.bgm_commands, audio_config)?,
            None,
        )
    };
    let PreparedNativeAudio {
        mut producer,
        bgm,
        mixer,
    } = audio;
    let mut bgm = BgmSession(bgm);
    let request = AlsaRequest {
        device: options.alsa.clone(),
        format: DeviceFormat::new(
            options.format.sample_rate(),
            options.format.channels(),
            SampleEncoding::Float32,
            None,
        )?,
        buffer_frames: options.buffer,
        period_frames: options.period,
        allow_size_rounding: false,
        monotonic_domain: HOST,
    };
    let owner = open_target_output(request, mixer)?;
    if network.is_none() {
        let initial = owner
            .current()
            .ok_or("initial target ALSA output missing")?;
        let basis = initial.stream().frame_basis();
        pause = pause.with_target_basis(initial.epoch(), basis)?;
        native_end = native_end
            .map(|end| end.with_target_basis(initial.epoch(), basis))
            .transpose()?;
    }
    // Network startup binds the cold source pause/end through its target policy.
    let ui = ConvertedAlsaOutputUi::new(&owner, network.is_none())?;
    let mut output = CohortOutput::Target(owner, ui);
    let (mut inputs, _descriptors, input_counters) =
        super::collected_input::open(options.local_inputs.clone())?;
    println!(
        "local players={count}; shared requested/applied ALSA={:?}; exact input devices={:?}; one asset bank/BGM/output; independent judges/captures/scores",
        output.configuration()?,
        options.local_inputs
    );
    let mut before_origin = 0u64;
    let mut startup_inputs = NativeCollectedInput::new()?;
    let outcome = (|| -> Result<Option<Vec<(beatkernel_bms_runtime::local_players::PlayerId,beatkernel_bms_runtime::play_result::CompletedPlayResult)>>> {
        let (network_origin, playback_origin) = if let Some(network) = network.as_mut() {
            let CohortOutput::Target(owner, _) = &mut output else {
                return Err("network startup requires target output".into());
            };
            let initial = owner.current_mut().ok_or("startup target ALSA output missing")?;
            let epoch = initial.epoch();
            let started = start_target_committed(
                &mut TargetStartupDevice {
                    stream: initial.stream_mut(),
                    epoch,
                    input: &mut inputs,
                    clock: &clock,
                    before_origin: &mut before_origin,
                    retained: &mut startup_inputs,
                },
                network,
                &mut producer,
                &mut pause,
                &mut native_end,
                NativeStartConfig {
                    output_origin: output_origin(),
                    sample_rate: options.format.sample_rate(),
                    playback_end_frame: playback_end,
                    setup_timeout: competition_options.setup_timeout,
                    max_clock_age_ns: competition_options.start_policy.max_age_ns,
                    max_rate_error_ppm: DisciplineConfig::default().max_rate_error_ppm,
                },
                |report, producer| {
                    beatkernel_bms_runtime::native_audio::feed_rendered(
                        &mut bgm,
                        report,
                        |command| producer.try_push(command),
                    )
                },
            )?;
            let Some(started) = started else {
                return Ok(None);
            };
            (Some(started.host_origin), started.plan.selected_output()?)
        } else {
            output.start()?;
            (None, output_origin())
        };
        let (mut presentation, startup_timeout) = match &output {
            CohortOutput::Legacy(owner, _) => {
                let initial = owner.current().ok_or("startup ALSA output missing")?;
                let (config, timeout) = audio_timing_config(initial.stream())?;
                (beatkernel_bms_runtime::native_audio_startup::new_audio_presentation(initial.epoch(), initial.stream().frame_basis(), HOST, ClockPoint { domain: LOGICAL, timestamp: Timestamp::ZERO }, config)?, timeout)
            }
            CohortOutput::Target(owner, _) => {
                let initial = owner.current().ok_or("startup target ALSA output missing")?;
                let (config, timeout) = target_audio_timing_config(initial.stream())?;
                (beatkernel_bms_runtime::native_audio_startup::new_target_audio_presentation(initial.epoch(), initial.stream().frame_basis(), HOST, ClockPoint { domain: LOGICAL, timestamp: Timestamp::ZERO }, config)?, timeout)
            }
        };
        let target = output.target();
        let seeded = {
            let mut device = CohortDevice { output:&mut output, inputs:&mut inputs, clock:&clock, retained:&mut startup_inputs,
                startup_end: if network_origin.is_none() { native_end.as_mut() } else { None }, startup_primed:false };
            if target {
                beatkernel_bms_runtime::native_audio_startup::prime_target_native_audio(&mut device, &mut presentation, &mut bgm, &mut producer, startup_timeout)?
            } else {
                beatkernel_bms_runtime::native_audio_startup::prime_native_audio(&mut device, &mut presentation, &mut bgm, &mut producer, startup_timeout)?
            }
        };
        let Some(seeded) = seeded else { return Ok(None); };
        let host_origin = match network_origin { Some(origin) => origin, None => seeded.host_for_output(playback_origin, startup_timeout)? };
        let logical_origin = presentation.logical_output(playback_origin)?;
        println!("local original native anchors={:?}; playback HOST={host_origin:?}; logical={logical_origin:?}; mapping accuracy unknown", seeded.observations);
        let (mut group, mut merger) = activate_audio_cohort_with_sounds(
            configs,
            &reserved,
            host_origin,
            OUTPUT,
            logical_origin,
            Transport::new(logical_origin.timestamp, song_origin, Rate::NORMAL),
            producer,
            options.end_ns.map(Timestamp::from_nanos),
            input_sounds,
            hazard_sounds,
        )?;
        if retained { group.set_audio_scope(beatkernel::audio::CommandScope(1)); }
        let pump = {
            let mut device = CohortDevice {
                output: &mut output,
                inputs: &mut inputs,
                clock: &clock,
                retained: &mut startup_inputs,
                startup_end: None,
                startup_primed: false,
            };
            let selected_policies: Vec<_> = assignments
                .iter()
                .map(|(player, _)| (*player, &policy))
                .collect();
            let session = NativeAudioCohortSession {
                    network: network.as_mut(),
                    group: &mut group,
                    states: &mut states,
                    merger: &mut merger,
                    bgm: &mut bgm,
                    discipline: &mut presentation,
                    pause: &mut pause,
                    end: &mut native_end,
                    delivery: &mut delivery,
                    pre_origin_inputs: &mut before_origin,
                };
            let config = AudioGameplayConfig { gameplay: NativeGameplayConfig {
                    origin: host_origin,
                    stream_origin: output_origin(),
                    playback_origin,
                    song_origin,
                    sample_rate: options.format.sample_rate(),
                    end_song: options.end_ns.map(Timestamp::from_nanos),
                    advance_lag: Duration::from_nanos(options.advance_lag),
                    seconds: options.seconds,
                    pause_supported: competition_options.network.is_none(),
                    logical_schedule: true,
                }, section_start: Timestamp::from_nanos(options.start_ns) };
            if let Some(practice) = practice.as_mut() {
                let mut recording = beatkernel_bms_runtime::native_gameplay::NativePracticeRecorder::new(save_capture);
                beatkernel_bms_runtime::native_cohort::run_cohort_audio_with_practice_and_policies_and_results(
                    &mut device, session, config, &selected_policies, practice, &mut recording)
            } else {
                run_cohort_audio_with_policies_and_results(&mut device, session, config, &selected_policies)
            }
        };
        for state in &states {
            if let Some(telemetry) = group.member_telemetry(state.player) {
                println!(
                    "player{} final score={:?}; processing={:?}; counters={:?}",
                    state.player.0,
                    state.score,
                    telemetry.processing(),
                    telemetry.counters()
                );
            }
        }
        pump
    })();
    let timing = output.timing_snapshot();
    inputs.cancel();
    let stop = output.stop();
    let input_stop = inputs.stop_and_join();
    println!(
        "shared final ALSA={:?}; timing={timing:?}; last observed mixer={:?}; pre-origin ignored={before_origin}; physical delivery unverified",
        output.snapshot(),
        output.render_report()
    );
    println!(
        "final local evdev counters={:?}",
        input_counters.try_recv().ok()
    );
    if let Err(error) = &stop {
        eprintln!("local ALSA stop/join error: {error}");
    }
    drop(inputs);
    let mut failures = Vec::new();
    if let Err(error) = input_stop {
        failures.push(format!("input cleanup: {error}"));
    }
    if let Err(error) = stop {
        failures.push(format!("output cleanup: {error}"));
    }
    let mut final_launch = launch.clone();
    if let Some(practice) = practice.as_ref() {
        let attempt = practice.members()[0].launch.attempt();
        while final_launch.attempt() < attempt {
            final_launch = final_launch.retry()?;
        }
    }
    let final_base = final_launch
        .args()
        .chunks_exact(2)
        .find(|pair| pair[0] == "--record-replay")
        .map(|pair| PathBuf::from(&pair[1]));
    if let Some(practice) = practice.as_ref() {
        for member in practice.members() {
            let path = member
                .launch
                .args()
                .chunks_exact(2)
                .find(|pair| pair[0] == "--record-replay")
                .map(|pair| PathBuf::from(&pair[1]));
            let target = save_paths
                .iter_mut()
                .find(|(id, _)| *id == member.player)
                .ok_or("missing final practice recording member")?;
            target.1 = path;
        }
    }
    let archive_paths = save_paths.clone();
    finish_cohort_with_results_and_network(
        outcome,
        states,
        network.as_mut(),
        save_paths,
        failures,
        final_base.as_deref(),
        save_capture,
        |archive, path| {
            beatkernel_bms_runtime::native_result_archive::save_cohort_sidecars_with_paths(
                archive,
                path.ok_or("completed archive missing base replay path")?,
                &archive_paths,
            )
        },
    )
}

#[cfg(test)]
use std::path::Path;
#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::input::{ButtonEvent, ButtonState, EventMeta, PhysicalInputEvent};
    #[test]
    #[ignore = "requires BEATKERNEL_TEST_ALSA_DEVICE=null; local output dispatch/recovery only, no evdev or native clock claim"]
    fn actual_local_target_dispatch_holds_one_source_owner_and_refuses_unavailable_clock() {
        use beatkernel::audio::{
            ConvertedOutputState, PcmSample, SampleBank, SampleId, StoppedMixerSource, VoiceId,
        };
        let device = std::env::var("BEATKERNEL_TEST_ALSA_DEVICE")
            .expect("explicit ALSA null endpoint required");
        assert_eq!(
            device, "null",
            "this fixture verifies only the null endpoint"
        );
        let source_rate = 44_100;
        let target_rate = 48_000;
        let section_start = Timestamp::from_nanos(604_800_000_000_000);
        let source_format = AudioFormat::new(source_rate, 1).unwrap();
        let pcm_bytes = 256 * std::mem::size_of::<f32>();
        let limits = PcmLimits::new(pcm_bytes, pcm_bytes, 1).unwrap();
        let mut bank = SampleBank::new(source_format, limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(
                source_format,
                (0..256).map(|n| 0.25 + n as f32 / 1024.0).collect(),
                limits,
            )
            .unwrap(),
        )
        .unwrap();
        let PreparedNativeAudio {
            mut producer,
            bgm,
            mixer,
        } = prepare_audio(
            bank,
            vec![AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: section_start,
                gain: 1.0,
            }],
            NativeAudioConfig {
                output_origin: output_origin(),
                start: section_start,
                preroll: Duration::ZERO,
                lookahead: Duration::from_nanos(1_000_000_000),
                voices: 2,
                max_render_frames: 64,
                playback_end_frame: Some(441),
                gated_start: false,
            },
        )
        .unwrap();
        assert_eq!(bgm.config().sample_rate, source_rate);
        assert_eq!(bgm.config().output_origin, output_origin());
        assert_eq!(bgm.report().total_admitted, 1);
        let counters = mixer.counters();
        let mut owner = open_target_output(
            AlsaRequest {
                device,
                format: DeviceFormat::new(target_rate, 2, SampleEncoding::Float32, None).unwrap(),
                buffer_frames: 256,
                period_frames: 64,
                allow_size_rounding: false,
                monotonic_domain: HOST,
            },
            mixer,
        )
        .unwrap();
        let initial = owner.current().unwrap();
        let basis = initial.stream().frame_basis();
        let (config, _) = target_audio_timing_config(initial.stream()).unwrap();
        let mut presentation =
            beatkernel_bms_runtime::native_audio_startup::new_target_audio_presentation(
                initial.epoch(),
                basis,
                HOST,
                ClockPoint {
                    domain: LOGICAL,
                    timestamp: Timestamp::ZERO,
                },
                config,
            )
            .unwrap();
        let mut end = NativeEnd::new(output_origin(), HOST, source_rate, 441)
            .unwrap()
            .with_target_basis(initial.epoch(), basis)
            .unwrap();
        let ui = ConvertedAlsaOutputUi::new(&owner, true).unwrap();
        owner.set_target_held(true).unwrap();
        let mut output = CohortOutput::Target(owner, ui);
        assert!(output.target());
        assert!(
            output.legacy_mut().is_err(),
            "target output cannot enter source-grid committed startup"
        );
        assert!(!output.pending());
        assert!(!output.suspended());
        assert_eq!(
            output.configuration().unwrap().format.sample_rate(),
            target_rate
        );
        assert_eq!(output.render_report(), None);
        assert!(output.audio_pause(&presentation, host(0)).is_err());
        assert_eq!(output.audio_end(&mut end, &presentation).unwrap(), None);
        output.start().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut first = None;
        let mut progressed = false;
        let mut observations = 0;
        while std::time::Instant::now() < deadline {
            observations += 1;
            output.observe_audio(&mut presentation).unwrap();
            let CohortOutput::Target(owner, _) = &output else {
                unreachable!()
            };
            if let Some(report) = owner.converted_report() {
                assert_eq!(report.state, ConvertedOutputState::Held);
                assert_eq!(report.source, None);
                assert_eq!(report.source_rate, source_rate);
                assert_eq!(report.target_rate, target_rate);
                assert_eq!(report.source_position.frame, 0);
                if first.is_some_and(|cursor| report.target_frame_cursor > cursor) {
                    progressed = true;
                    break;
                }
                first.get_or_insert(report.target_frame_cursor);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let snapshot = output.snapshot().unwrap();
        let timing = output.timing_snapshot();
        if !progressed {
            eprintln!(
                "local target dispatch observations={observations}, native={snapshot:?}, timing={timing:?}, native_record={:?}",
                presentation.latest_record()
            );
        }
        assert!(
            progressed,
            "bounded local dispatch needs actual target submission progress"
        );
        assert!(snapshot.submitted_frames > 0);
        assert_eq!(
            output.render_report(),
            None,
            "held target reports cannot be relabeled source callbacks"
        );
        // Null remains PREPARED and supplies no played-frame evidence. Local
        // pause/end dispatch must not create an audio authority from submissions.
        let native_pair = timing.and_then(|timing| {
            beatkernel_platform::linux::alsa_presentation_pair_with_target_basis(timing, basis)
                .unwrap()
        });
        assert_eq!(
            native_pair, None,
            "this test requires null submission-only evidence"
        );
        assert_eq!(presentation.latest_record(), None);
        assert_eq!(presentation.target_basis(), None);
        assert!(output.audio_pause(&presentation, host(0)).is_err());
        assert_eq!(output.audio_end(&mut end, &presentation).unwrap(), None);
        assert!(output.legacy_mut().is_err());
        output.stop().unwrap();
        assert_eq!(output.timing_snapshot(), None);
        let CohortOutput::Target(owner, _) = &mut output else {
            unreachable!()
        };
        let current = owner.current_mut().unwrap();
        let mut recovered = current.take_stopped_mixer().unwrap().unwrap();
        assert!(current.take_stopped_mixer().unwrap().is_none());
        assert_eq!(recovered.mixer().config().format(), source_format);
        assert_eq!(recovered.mixer().config().playback_end_frame(), Some(441));
        assert_eq!(recovered.mixer().counters(), counters);
        assert_eq!(recovered.converter_owner().source_position().frame, 0);
        assert!(recovered
            .pending_samples()
            .iter()
            .all(|sample| *sample == 0.0));
        let pending = recovered.pending_frames();
        if pending > 0 {
            recovered.admit(pending).unwrap();
        }
        // The same live source queue survives native joining. A later shared
        // Stop must not move or replace the already admitted first BGM cue.
        producer
            .try_push(AudioCommand::Stop {
                voice: VoiceId(1),
                at: Timestamp::from_nanos(1_000_000_000),
            })
            .unwrap();
        let report = recovered.render_pending(32).unwrap();
        assert_eq!(report.source_start_position.frame, 0);
        for (frame, stereo) in recovered.pending_samples().chunks_exact(2).enumerate() {
            let expected = 0.25 + frame as f64 * 147.0 / (160.0 * 1024.0);
            for &sample in stereo {
                assert!((f64::from(sample) - expected).abs() < 2e-6);
            }
        }
        assert_eq!(recovered.mixer().counters().commands_applied, 1);
        assert_eq!(recovered.boundaries().source_rate, source_rate);
        assert_eq!(recovered.boundaries().end, None);
        eprintln!(
            "local output fixture evidence tier: actual null submission/typed dispatch/shared source-owner recovery; evdev acquisition/native clock/full cohort run unproved"
        );
    }
    fn host(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: HOST,
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn button(device: u64, state: ButtonState, ns: i64, sequence: u64) -> PhysicalInputEvent {
        PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(device), host(ns), sequence),
            control: PhysicalControlId::keyboard(4),
            state,
        })
    }
    #[test]
    fn exact_pause_commit_waits_for_lag_without_hiding_later_merger_regression() {
        let mut merger =
            InputMerger::new(HOST, host(0), vec![DeviceId(1), DeviceId(2)], 4).unwrap();
        merger.commit(host(10_000_000)).unwrap();
        assert!(!lag_reaches(host(10_500_000), host(10_000_000), 2_000_000).unwrap());
        assert!(lag_reaches(host(12_000_000), host(10_000_000), 2_000_000).unwrap());
        assert_eq!(
            merger
                .watermark(host(12_000_000), 2_000_000, false)
                .unwrap(),
            Some(host(10_000_000))
        );
        assert!(merger
            .watermark(host(11_000_000), 2_000_000, false)
            .is_err());
        assert!(!lag_reaches(host(12_000_000), host(11_000_000), 2_000_000).unwrap());
        assert!(lag_reaches(host(13_000_000), host(11_000_000), 2_000_000).unwrap());
        assert!(!lag_reaches(host(i64::MIN), host(i64::MIN), 1).unwrap());
        assert!(lag_reaches(host(10), host(10), -1).is_err());
        assert!(lag_reaches(
            host(10),
            ClockPoint {
                domain: OUTPUT,
                timestamp: Timestamp::ZERO
            },
            0
        )
        .is_err());
    }
    #[test]
    fn shared_pause_frontier_drains_global_order_including_boundary_without_moving_commit() {
        let mut merger =
            InputMerger::new(HOST, host(0), (1..=4).map(DeviceId).collect(), 16).unwrap();
        let mut keyboard = PauseKeyboard::new();
        for event in [
            button(4, ButtonState::Down, 9, 1),
            button(2, ButtonState::Down, 9, 1),
            button(1, ButtonState::Down, 10, 1),
            button(3, ButtonState::Down, 11, 1),
        ] {
            merger.admit(event, host(20)).unwrap();
        }
        assert_eq!(merger.watermark(host(20), 2, true).unwrap(), None);
        let mut judged = Vec::new();
        while let Some(event) = merger.pop_ready(host(10)).unwrap() {
            if event.meta().timestamp >= host(10).timestamp {
                keyboard.observe_paused(event).unwrap();
            } else if keyboard.accept(&event).unwrap() {
                judged.push(event.meta().source);
            }
        }
        assert_eq!(judged, vec![DeviceId(2), DeviceId(4)]);
        merger.commit(host(10)).unwrap();
        let safe = merger.watermark(host(20), 2, false).unwrap().unwrap();
        while let Some(event) = merger.pop_ready(safe).unwrap() {
            keyboard.observe_paused(event).unwrap();
        }
        // Paused draining consumes levels but leaves the committed judge
        // frontier at the actual pause boundary, so later paused input is legal.
        merger
            .admit(button(3, ButtonState::Up, 12, 2), host(21))
            .unwrap();
        assert_eq!(merger.pending(), 1);
        assert!(!keyboard
            .accept(&button(1, ButtonState::Repeat, 21, 2))
            .unwrap());
    }
    #[test]
    fn resume_keeps_lag_and_reconciles_all_devices_before_postboundary_inputs() {
        let mut merger =
            InputMerger::new(HOST, host(0), vec![DeviceId(1), DeviceId(2)], 16).unwrap();
        let mut keyboard = PauseKeyboard::new();
        keyboard
            .accept(&button(1, ButtonState::Down, 1, 1))
            .unwrap();
        keyboard
            .accept(&button(2, ButtonState::Down, 1, 1))
            .unwrap();
        merger.commit(host(10)).unwrap();
        for event in [
            button(2, ButtonState::Up, 19, 2),
            button(1, ButtonState::Up, 18, 2),
            button(1, ButtonState::Down, 21, 3),
        ] {
            merger.admit(event, host(30)).unwrap();
        }
        assert_eq!(
            merger.watermark(host(21), 2, false).unwrap(),
            Some(host(19))
        );
        assert_eq!(merger.watermark(host(30), 2, true).unwrap(), None);
        let frontier = merger.watermark(host(30), 2, false).unwrap().unwrap();
        let mut at = Some(host(20));
        let mut admitted = Vec::new();
        while let Some(event) = merger.pop_ready(frontier).unwrap() {
            if let Some(boundary) = at {
                if event.meta().timestamp < boundary.timestamp {
                    keyboard.observe_paused(event).unwrap();
                    continue;
                }
                admitted.extend(keyboard.resume(boundary).unwrap());
                at = None;
            }
            if keyboard.accept(&event).unwrap() {
                admitted.push(event);
            }
        }
        assert!(at.is_none());
        assert_eq!(admitted.len(), 3);
        assert_eq!(admitted[0].meta().source, DeviceId(1));
        assert_eq!(admitted[1].meta().source, DeviceId(2));
        assert_eq!(admitted[0].meta().timestamp, host(20).timestamp);
        assert_eq!(admitted[0].meta().original_clock_point, Some(host(18)));
        assert_eq!(admitted[2].meta().timestamp, host(21).timestamp);
        merger.commit(frontier).unwrap();
        assert!(merger
            .admit(button(2, ButtonState::Down, 27, 3), host(31))
            .is_err());
    }
    fn finite_states(ids: &[PlayerId], song: i64) -> Vec<PlayerState> {
        ids.iter()
            .map(|&player| PlayerState {
                player,
                capture: None,
                competition: None,
                completion: None,
                score: ScoreSummary::default(),
                gauge: beatkernel_bms_runtime::gauge::BmsGauge::default(),
                last_song: Timestamp::from_nanos(song),
            })
            .collect()
    }
    #[test]
    fn finite_cohort_needs_every_member_actual_commit_native_ack_and_all_source_drain() {
        for count in [2, 3, 4, 64] {
            let mut ids: Vec<_> = (0..count).map(|i| PlayerId(1000 + i as u32 * 7)).collect();
            *ids.last_mut().unwrap() = PlayerId(u32::MAX);
            let mut states = finite_states(&ids, 10);
            assert!(finite_cohort_done(
                Some(10),
                Some(host(20)),
                Some(host(20)),
                &states,
                false,
                false
            ));
            for (boundary, committed, backlog, resuming) in [
                (None, Some(host(20)), false, false),
                (Some(host(20)), None, false, false),
                (Some(host(20)), Some(host(19)), false, false),
                (Some(host(20)), Some(host(20)), true, false),
                (Some(host(20)), Some(host(20)), false, true),
            ] {
                assert!(!finite_cohort_done(
                    Some(10),
                    boundary,
                    committed,
                    &states,
                    backlog,
                    resuming
                ));
            }
            states.last_mut().unwrap().last_song = Timestamp::from_nanos(9);
            assert!(!finite_cohort_done(
                Some(10),
                Some(host(20)),
                Some(host(21)),
                &states,
                false,
                false
            ));
            states.last_mut().unwrap().last_song = Timestamp::from_nanos(10);
            assert!(!finite_cohort_done(
                None,
                Some(host(20)),
                Some(host(21)),
                &states,
                false,
                false
            ));
            assert_eq!(states.last().unwrap().player, PlayerId(u32::MAX));
            assert!(states.iter().all(|state| state.completion.is_none()));
        }
    }
    #[test]
    fn finite_global_pop_keeps_preboundary_order_and_only_real_commit_finishes() {
        let states = finite_states(
            &[
                PlayerId(7),
                PlayerId(1000),
                PlayerId(u32::MAX),
                PlayerId(42),
            ],
            10,
        );
        let mut merger =
            InputMerger::new(HOST, host(0), (1..=4).map(DeviceId).collect(), 16).unwrap();
        for event in [
            button(4, ButtonState::Down, 9, 1),
            button(2, ButtonState::Down, 9, 1),
            button(1, ButtonState::Down, 10, 1),
            button(3, ButtonState::Down, 11, 1),
            button(2, ButtonState::Up, 21, 2),
        ] {
            merger.admit(event, host(30)).unwrap();
        }
        assert_eq!(merger.watermark(host(20), 2, true).unwrap(), None);
        let frontier = merger.watermark(host(20), 2, false).unwrap().unwrap();
        let mut gameplay = Vec::new();
        let mut acquired = Vec::new();
        while let Some(event) = merger.pop_ready(frontier).unwrap() {
            acquired.push(event.meta().source);
            if event.meta().timestamp < host(10).timestamp {
                gameplay.push(event.meta().source);
            }
        }
        assert_eq!(gameplay, vec![DeviceId(2), DeviceId(4)]);
        assert_eq!(
            acquired,
            vec![DeviceId(2), DeviceId(4), DeviceId(1), DeviceId(3)]
        );
        assert!(!finite_cohort_done(
            Some(10),
            Some(host(10)),
            None,
            &states,
            false,
            false
        ));
        merger.commit(frontier).unwrap();
        assert!(finite_cohort_done(
            Some(10),
            Some(host(10)),
            Some(frontier),
            &states,
            false,
            false
        ));
        // Future post-end acquisition is retained by the bounded merger until
        // owner cleanup; it neither blocks this prefix nor invents gameplay.
        assert_eq!(merger.pending(), 1);
        assert!(merger
            .admit(button(1, ButtonState::Up, 17, 2), host(30))
            .is_err());
    }
    #[test]
    fn local_replay_paths_preserve_parent_and_use_distinct_player_suffixes() {
        assert_eq!(
            replay_path(Path::new("records/run.bkr"), PlayerId(1)).unwrap(),
            PathBuf::from("records/run.p1.bkr")
        );
        assert_eq!(
            replay_path(Path::new("records/run.bkr"), PlayerId(64)).unwrap(),
            PathBuf::from("records/run.p64.bkr")
        );
        assert!(replay_path(Path::new("/"), PlayerId(1)).is_err());
        assert!(replay_path(Path::new("run.bkr"), PlayerId(0)).is_err());
        for id in [7, 1000, u32::MAX] {
            assert_eq!(
                replay_path(Path::new("run.bkr"), PlayerId(id)).unwrap(),
                PathBuf::from(format!("run.p{id}.bkr"))
            );
        }
    }
    #[test]
    fn local_roster_bounds_are_shared_by_offline_and_network_modes() {
        assert!(admit_mode(2, false).is_ok());
        assert!(admit_mode(64, false).is_ok());
        assert!(admit_mode(2, true).is_ok());
        assert!(admit_mode(64, true).is_ok());
        for (count, network) in [(1, false), (65, false), (1, true), (65, true)] {
            assert!(admit_mode(count, network).is_err());
        }
    }
}
