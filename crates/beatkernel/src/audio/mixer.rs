use super::{
    AudioCommand, AudioCounters, AudioError, CommandConsumer, MixerConfig, PcmSample, RenderReport,
    SampleBank, SampleId, VoiceId,
};
use crate::{time::Timestamp, transport::Rate};

/// Preallocated deterministic PCM scheduler and mixer.
///
/// Assets and queue storage remain owned until this mixer is destroyed outside
/// rendering. Render performs no allocation, deallocation, decoding or locking.
pub struct Mixer {
    config: MixerConfig,
    bank: SampleBank,
    consumer: CommandConsumer,
    voices: Vec<Option<Voice>>,
    pending: Vec<Pending>,
    frame_cursor: u64,
    playback_frame_cursor: u64,
    paused: bool,
    start_gate_open: bool,
    playback_end_physical_frame: Option<u64>,
    counters: AudioCounters,
    rate: Rate,
    song_position: Timestamp,
    practice: Option<super::practice::Practice>,
    practice_projection_deferred: bool,
    source_playback_history: Option<super::SourcePlaybackHistory>,
    // Only runtime retained BGM needs a transactional scalar voice simulation.
    // Storage is cold, and contains no owning PCM or callback allocations.
    practice_preflight_voices: Vec<Option<Voice>>,
}

#[derive(Clone, Copy)]
struct Pending {
    target: i128,
    preroll: bool,
    command: AudioCommand,
    scope: super::CommandScope,
}

#[derive(Clone, Copy)]
struct Voice {
    id: VoiceId,
    sample: SampleId,
    gain: f32,
    head: Head,
}

// Split rational frame position avoids multiplying a large frame coordinate
// by its fractional denominator. The cached step shares that denominator.
#[derive(Clone, Copy)]
struct Head {
    whole: i128,
    fraction: i128,
    denominator: i128,
    step_whole: i128,
    step_fraction: i128,
}

impl Head {
    fn new(frame: usize, source_rate: u32, output_rate: u32, rate: Rate) -> Option<Self> {
        Self {
            whole: frame as i128,
            fraction: 0,
            denominator: 1,
            step_whole: 0,
            step_fraction: 0,
        }
        .with_rate(source_rate, output_rate, rate)
    }

    fn with_rate(self, source_rate: u32, output_rate: u32, rate: Rate) -> Option<Self> {
        let numerator = i128::from(source_rate).checked_mul(i128::from(rate.numerator()))?;
        let denominator = i128::from(output_rate).checked_mul(i128::from(rate.denominator()))?;
        let divisor = gcd(numerator.abs(), denominator);
        let numerator = numerator / divisor;
        let step_denominator = denominator / divisor;
        let step_whole = numerator.div_euclid(step_denominator);
        let step_fraction = numerator.rem_euclid(step_denominator);

        let phase_divisor = gcd(self.fraction, self.denominator);
        let phase_fraction = self.fraction / phase_divisor;
        let phase_denominator = self.denominator / phase_divisor;
        let common = (phase_denominator / gcd(phase_denominator, step_denominator))
            .checked_mul(step_denominator)?;
        Some(Self {
            whole: self.whole,
            fraction: phase_fraction.checked_mul(common / phase_denominator)?,
            denominator: common,
            step_whole,
            step_fraction: step_fraction.checked_mul(common / step_denominator)?,
        })
    }

    fn advance(&mut self) {
        // Each fraction is in [0, denominator). Avoid overflowing their sum
        // even when the checked common denominator is close to i128::MAX.
        let distance_to_wrap = self.denominator - self.step_fraction;
        let carry = if self.fraction >= distance_to_wrap {
            self.fraction -= distance_to_wrap;
            1
        } else {
            self.fraction += self.step_fraction;
            0
        };
        // A live whole coordinate is bounded by an allocated asset's usize
        // length. Rate/source products fit at most 96 signed bits, so this
        // addition fits i128 even for i64::MIN and immediately retiring heads.
        self.whole += self.step_whole + carry;
    }

    fn in_bounds(self, frames: usize) -> bool {
        self.whole >= 0 && self.whole < frames as i128
    }

    // A sufficient (never optimistic) retirement proof. Ignoring an existing
    // proper fraction can only retain a carry voice longer. Wide products that
    // do not fit simply decline this optional capacity refinement.
    fn proven_retired_after(
        self,
        steps: u64,
        frames: usize,
        source_hz: u32,
        output_hz: u32,
        rate: Rate,
    ) -> bool {
        let remaining = frames as i128 - self.whole;
        if remaining <= 0 {
            return true;
        }
        let numerator = u128::from(source_hz) * rate.numerator() as u128;
        let denominator = u128::from(output_hz) * u128::from(rate.denominator());
        let whole_step = numerator / denominator;
        if whole_step != 0 && u128::from(steps) >= (remaining as u128).div_ceil(whole_step) {
            return true;
        }
        u128::from(steps)
            .checked_mul(numerator)
            .is_some_and(|advance| advance / denominator >= remaining as u128)
    }
}

impl Mixer {
    /// Installs original assets and bounded controls before the first render.
    /// Finite legacy endpoints are deliberately incompatible with retained play.
    pub fn install_practice(
        &mut self,
        program: super::PreparedPracticeProgram,
        endpoint: super::PracticeAudioEndpoint,
        region: super::PracticeRegion,
    ) -> Result<(), super::PracticeError> {
        if self.practice.is_some() {
            return Err(super::PracticeError::AlreadyInstalled);
        }
        if self.frame_cursor != 0
            || self.config.playback_end_frame().is_some()
            || program.data.output_rate != self.config.format().sample_rate()
            || program.data.channels != self.config.format().channels()
            || program.data.limits.max_overlap > self.voices.len()
        {
            return Err(super::PracticeError::IncompatibleMixer);
        }
        // The program may have been prepared against another bank. Recheck all
        // metadata before ownership changes; no asset identity can be substituted.
        for cue in &program.data.cues {
            let sample = self
                .bank
                .get(cue.cue.sample)
                .ok_or(super::PracticeError::UnknownSample)?;
            if sample.frames() != cue.frames || sample.format().sample_rate() != cue.source_rate {
                return Err(super::PracticeError::IncompatibleMixer);
            }
        }
        let practice = super::practice::Practice::new(program, endpoint, region)?;
        super::practice::Practice::validate_deadline(
            &practice.program.data,
            &practice.cursor,
            0,
            0,
            self.output_frame_basis().origin(),
        )?;
        let mut preflight_voices = Vec::new();
        if practice.program.data.runtime_program_id != 0 {
            preflight_voices
                .try_reserve_exact(practice.program.data.limits.max_overlap)
                .map_err(|_| super::PracticeError::Capacity)?;
            preflight_voices.resize(practice.program.data.limits.max_overlap, None);
        }
        // Cold state adoption, not a consumed or acknowledged queue command.
        self.rate = practice.program.data.rate;
        self.practice_preflight_voices = preflight_voices;
        self.practice = Some(practice);
        Ok(())
    }
    /// Current explicit program generation, distinct from native output epochs.
    pub fn practice_generation(&self) -> Option<u64> {
        self.practice.as_ref().map(|p| p.cursor.generation)
    }
    /// Whether the nonrepeating retained region reached its end boundary.
    pub fn practice_finished(&self) -> bool {
        self.practice.as_ref().is_some_and(|p| !p.cursor.active)
    }

    /// Earliest original Source and projected Target references in live practice
    /// receipts. Raw lookahead pins only Source; these facts grant no native ACK.
    /// The owning audio thread can inspect them without consuming queue entries.
    pub fn practice_history_retention_pins(&self) -> (Option<u64>, Option<super::TargetTime>) {
        self.practice.as_ref().map_or((None, None), |practice| {
            practice.endpoint.history_retention_pins()
        })
    }

    /// Actual immutable format, scheduling origin and capacities owned by this mixer.
    pub const fn configuration(&self) -> MixerConfig {
        self.config
    }

    /// Allocates all voice and pending-command storage before output begins.
    ///
    /// Bank output format must exactly match configuration; individual assets
    /// may retain different source rates. Queue capacity must match the config.
    pub fn new(
        config: MixerConfig,
        bank: SampleBank,
        consumer: CommandConsumer,
    ) -> Result<Self, AudioError> {
        if bank.format().channels() != config.format().channels() {
            return Err(AudioError::ChannelMismatch);
        }
        if bank.format() != config.format() {
            return Err(AudioError::InvalidFormat);
        }
        if consumer.capacity() != config.limits().queue_capacity() {
            return Err(AudioError::InvalidCapacity);
        }
        let mut voices = Vec::new();
        voices
            .try_reserve_exact(config.limits().max_voices())
            .map_err(|_| AudioError::AllocationFailed)?;
        voices.resize(config.limits().max_voices(), None);
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(config.limits().pending_capacity())
            .map_err(|_| AudioError::AllocationFailed)?;
        Ok(Self {
            config,
            bank,
            consumer,
            voices,
            pending,
            frame_cursor: 0,
            playback_frame_cursor: 0,
            paused: false,
            start_gate_open: false,
            playback_end_physical_frame: None,
            counters: AudioCounters::default(),
            rate: Rate::NORMAL,
            song_position: Timestamp::ZERO,
            practice: None,
            practice_projection_deferred: false,
            source_playback_history: None,
            practice_preflight_voices: Vec::new(),
        })
    }

    /// Cold opt-in retaining actual physical/playback intervals from the
    /// current integer cursors. Earlier output is not reconstructed.
    /// Repeated, zero-capacity or allocation-failed enabling preserves this
    /// mixer. This history authenticates no native presentation or pause ACK.
    pub fn enable_source_playback_history(&mut self, capacity: usize) -> Result<(), AudioError> {
        if self.source_playback_history.is_some() {
            return Err(AudioError::InvalidCapacity);
        }
        let history = super::SourcePlaybackHistory::new(
            self.config.format().sample_rate(),
            self.frame_cursor,
            self.playback_frame_cursor,
            capacity,
        )?;
        self.source_playback_history = Some(history);
        Ok(())
    }

    /// Cold growth to an absolute minimum transition capacity, preserving all
    /// retained source facts. Prepare any native publication storage afterward.
    /// Missing history, zero capacity or failed allocation preserves this mixer.
    pub fn reserve_source_playback_history(&mut self, capacity: usize) -> Result<(), AudioError> {
        self.source_playback_history
            .as_mut()
            .ok_or(AudioError::InvalidCapacity)?
            .reserve_capacity(capacity)
    }

    /// Actual source playback intervals retained by this mixer.
    pub const fn source_playback_history(&self) -> Option<&super::SourcePlaybackHistory> {
        self.source_playback_history.as_ref()
    }

    /// Retires complete source intervals before the interval covering `pin`.
    /// Call only on this mixer's owning thread after qualifying the minimum of
    /// all live consumer pins. This operation grants no native drain or ACK.
    /// Missing source history is refused without changing mixer state.
    pub fn retire_source_playback_before(&mut self, pin: u64) -> Result<usize, AudioError> {
        self.source_playback_history
            .as_mut()
            .ok_or(AudioError::InvalidCapacity)?
            .retire_before(pin)
    }

    /// Renders a contiguous block on the immutable integer output frame grid.
    ///
    /// Invalid buffer alignment, configured extent or cursor overflow is
    /// rejected before any queue/state/output mutation. Empty output observes
    /// telemetry without consuming commands or changing state. Future commands
    /// are sorted in fixed-capacity storage; equal targets retain submission
    /// order. Late commands retain their original target ordering.
    /// Commands beyond the per-render drain budget remain queued. Partition
    /// invariance applies to commands admitted before their execution frames;
    /// callback partitions can change admission timing under queue backlog.
    ///
    /// Rate processing uses exact rational sample heads. Every single valid
    /// Rate can form a new head. A later rate change can be rejected when the
    /// checked common denominator with a live fractional head exceeds i128;
    /// such rejection leaves the rate and all voices unchanged.
    ///
    /// A requested pause is adopted only after nonempty render preflight. It
    /// emits silence on the physical output grid while preserving the playback
    /// cursor, voices, rational heads and all queued or pending commands.
    /// Command targets use the playback grid, excluding paused output frames.
    /// An immutable playback endpoint limits the active prefix, zeros its
    /// physical suffix and remains paused even if the producer requests resume.
    /// A gated queue emits silent startup frames without draining commands. Its
    /// first positive playback is exact even inside a block. An armed target
    /// missed before adoption, or manual pause spanning that target, rejects
    /// without mutation; pauses after gate opening retain ordinary semantics.
    pub fn render(&mut self, output: &mut [f32]) -> Result<RenderReport, AudioError> {
        let channels = usize::from(self.config.format().channels());
        if !output.len().is_multiple_of(channels) {
            return Err(AudioError::InvalidBuffer);
        }
        let frames = output.len() / channels;
        if frames > self.config.limits().max_render_frames() {
            return Err(AudioError::RenderCapacity);
        }
        let extent = u64::try_from(frames).map_err(|_| AudioError::Overflow)?;
        let end = self
            .frame_cursor
            .checked_add(extent)
            .ok_or(AudioError::Overflow)?;
        if frames != 0 && self.practice.is_some() && !self.practice_projection_deferred {
            super::TargetTime::from_frames(end, self.config.format().sample_rate())?
                .point(self.output_frame_basis().origin())?;
        }
        let start = self.frame_cursor;
        let playback_start = self.playback_frame_cursor;
        if frames == 0 {
            return Ok(self.report(start, playback_start, 0));
        }
        let pause_requested = self.consumer.pause_requested();
        let mut prefix_extent = 0;
        let mut opens_gate = false;
        if !self.start_gate_open {
            if let Some(target) = self.consumer.start_gate() {
                match target {
                    None => {
                        if self.consumer.is_disconnected() {
                            return Err(AudioError::StartGateDisconnected);
                        }
                        prefix_extent = extent;
                    }
                    Some(target) => {
                        if target < start {
                            return Err(AudioError::StartGateMissed);
                        }
                        prefix_extent = extent.min(target - start);
                        if prefix_extent < extent {
                            if pause_requested {
                                return Err(AudioError::StartGatePaused);
                            }
                            opens_gate = true;
                        }
                    }
                }
            }
        }
        let available_extent = extent - prefix_extent;
        let active_extent = self
            .config
            .playback_end_frame()
            .map_or(available_extent, |end| {
                available_extent.min(end.saturating_sub(playback_start))
            });
        let playback_end = playback_start
            .checked_add(active_extent)
            .ok_or(AudioError::Overflow)?;
        let prefix_frames = usize::try_from(prefix_extent).map_err(|_| AudioError::Overflow)?;
        let active_frames = usize::try_from(active_extent).map_err(|_| AudioError::Overflow)?;
        let prefix_samples = prefix_frames * channels;
        let active_samples = active_frames * channels;
        let active_start = start
            .checked_add(prefix_extent)
            .ok_or(AudioError::Overflow)?;
        let physical_prefix_end = active_start
            .checked_add(active_extent)
            .ok_or(AudioError::Overflow)?;
        let (practice_controls, rate_decision) = if !pause_requested && active_frames > 0 {
            let origin = self.output_frame_basis().origin();
            let decision = self
                .prepare_runtime_rate_decision(playback_start, active_start, origin)
                .map_err(practice_audio_error)?;
            let controls = self
                .preflight_practice(
                    playback_start,
                    active_frames,
                    active_start,
                    origin,
                    decision,
                )
                .map_err(practice_audio_error)?;
            (controls, decision)
        } else {
            (0, None)
        };
        let history_plan = self
            .source_playback_history
            .as_ref()
            .map(|history| {
                history.preflight(
                    self.config.format().sample_rate(),
                    start,
                    end,
                    playback_start,
                    active_start,
                    physical_prefix_end,
                    prefix_extent != extent && !pause_requested && active_extent != 0,
                )
            })
            .transpose()?;
        // All failure paths precede state, output, queue and evidence changes.
        if prefix_extent == extent {
            self.paused = true;
            output.fill(0.0);
            self.frame_cursor = end;
            self.consumer.publish_physical_frontier(end);
            self.counters.rendered_frames = self.counters.rendered_frames.saturating_add(extent);
            if let (Some(history), Some(plan)) =
                (self.source_playback_history.as_mut(), history_plan)
            {
                history.commit(plan);
            }
            return Ok(self.report(start, playback_start, frames));
        }
        if opens_gate {
            self.start_gate_open = true;
        }
        self.paused = pause_requested || active_extent == 0;
        if self.paused {
            self.mark_playback_end(active_start);
            if self.playback_end_physical_frame.is_some() {
                self.settle_ended_commands();
            }
            output.fill(0.0);
            self.frame_cursor = end;
            self.consumer.publish_physical_frontier(end);
            self.counters.rendered_frames = self.counters.rendered_frames.saturating_add(extent);
            if let (Some(history), Some(plan)) =
                (self.source_playback_history.as_mut(), history_plan)
            {
                history.commit(plan);
            }
            return Ok(self.report(start, playback_start, frames));
        }
        output[..prefix_samples].fill(0.0);

        let budget = self
            .consumer
            .available_up_to(self.config.limits().max_commands_per_render());
        for _ in 0..budget {
            let Ok((command, scope)) = self.consumer.try_pop_scoped() else {
                break;
            };
            increment(&mut self.counters.commands_consumed);
            if self
                .practice
                .as_ref()
                .is_some_and(|p| scope.0 < p.cursor.generation)
            {
                continue;
            }
            let Some(target) = self.target_frame(command.at()) else {
                increment(&mut self.counters.invalid_times);
                continue;
            };
            if self.pending.len() == self.config.limits().pending_capacity() {
                increment(&mut self.counters.pending_full);
                continue;
            }
            // Insertion after existing equal targets preserves submission order
            // without an overflowing sequence counter or an allocating sort.
            let index = self.pending.partition_point(|item| item.target <= target);
            self.pending.insert(
                index,
                Pending {
                    target,
                    preroll: command.at() < self.config.origin(),
                    command,
                    scope,
                },
            );
        }

        let mut applied_practice_controls = 0;
        for (frame_offset, frame) in output[prefix_samples..prefix_samples + active_samples]
            .chunks_exact_mut(channels)
            .enumerate()
        {
            if self.practice.is_some() {
                self.practice_frame(
                    active_start + frame_offset as u64,
                    practice_controls,
                    &mut applied_practice_controls,
                    (frame_offset == 0).then_some(rate_decision).flatten(),
                );
            }
            while self
                .pending
                .first()
                .is_some_and(|pending| pending.target <= i128::from(self.playback_frame_cursor))
            {
                let pending = self.pending.remove(0);
                if pending.preroll || pending.target < i128::from(self.playback_frame_cursor) {
                    increment(&mut self.counters.late_commands);
                }
                if self
                    .practice
                    .as_ref()
                    .is_none_or(|p| p.cursor.active && pending.scope.0 == p.cursor.generation)
                {
                    self.apply(pending.command);
                }
            }
            self.mix_frame(frame);
            self.playback_frame_cursor += 1;
        }
        output[prefix_samples + active_samples..].fill(0.0);
        self.frame_cursor = end;
        self.consumer.publish_physical_frontier(end);
        if opens_gate && active_frames > 0 {
            self.consumer.publish_applied_start(active_start);
        }
        self.paused = self
            .config
            .playback_end_frame()
            .is_some_and(|end| self.playback_frame_cursor >= end);
        self.mark_playback_end(physical_prefix_end);
        debug_assert_eq!(self.frame_cursor, end);
        debug_assert_eq!(self.playback_frame_cursor, playback_end);
        self.counters.rendered_frames = self.counters.rendered_frames.saturating_add(extent);
        if !self.practice_projection_deferred {
            self.project_direct_practice_receipts(start, frames);
        }
        if let (Some(history), Some(plan)) = (self.source_playback_history.as_mut(), history_plan) {
            history.commit(plan);
        }
        Ok(self.report(start, playback_start, frames))
    }

    /// Cold preflight before creating a fresh conversion owner from this mixer.
    /// Original-output receipts must drain before their output basis changes.
    /// Refusal preserves all owners and receipts. This is not required for a
    /// retained converter's retarget, which preserves the same projection owner.
    /// Native composition can call this before retiring an existing endpoint.
    pub fn validate_practice_projection_transfer(&self) -> Result<(), AudioError> {
        if self
            .practice
            .as_ref()
            .is_some_and(|p| p.endpoint.has_pending_receipts())
        {
            Err(AudioError::PracticeProjectionPending)
        } else {
            Ok(())
        }
    }
    pub(crate) fn defer_practice_projection(&mut self) {
        self.practice_projection_deferred = true;
    }
    fn project_direct_practice_receipts(&mut self, start: u64, frames: usize) {
        let rate = self.config.format().sample_rate();
        let origin = self.output_frame_basis().origin();
        let Some(practice) = self.practice.as_mut() else {
            return;
        };
        for _ in 0..practice.program.data.limits.receipt_capacity {
            let Some(frame) = practice.endpoint.raw_boundary() else {
                break;
            };
            if frame >= start + frames as u64 {
                break;
            }
            let time =
                super::TargetTime::from_frames(frame, rate).expect("validated direct output rate");
            practice.endpoint.publish_projection(
                super::TargetBoundary {
                    source_frame: frame,
                    target_frame_offset: frame.saturating_sub(start) as usize,
                    target_time: time,
                },
                frame,
                rate,
                origin,
            );
        }
    }
    pub(crate) fn project_converted_practice_receipts(
        &mut self,
        report: &super::ConvertedRenderReport,
    ) {
        if report.state != super::ConvertedOutputState::Active || report.target_frames == 0 {
            return;
        }
        let origin = self.output_frame_basis().origin();
        let Some(practice) = self.practice.as_mut() else {
            return;
        };
        for _ in 0..practice.program.data.limits.receipt_capacity {
            let Some(frame) = practice.endpoint.raw_boundary() else {
                break;
            };
            let boundary = if frame <= report.source_start_position.frame {
                // A boundary deferred at the previous exclusive target end
                // becomes proved by this first actual target sample. Held
                // output never reaches here and therefore moves its real time.
                super::TargetBoundary {
                    source_frame: frame,
                    target_frame_offset: 0,
                    target_time: report.target_start_time,
                }
            } else {
                let Some(boundary) = report
                    .project_source_boundary(frame)
                    .expect("validated source/target projection arithmetic")
                else {
                    break;
                };
                if boundary.target_frame_offset >= report.target_frames {
                    break;
                }
                boundary
            };
            let target_frame = report.target_frame_cursor - report.target_frames as u64
                + boundary.target_frame_offset as u64;
            practice.endpoint.publish_projection(
                boundary,
                target_frame,
                report.target_rate,
                origin,
            );
        }
    }

    /// Immutable format, clock origin and capacity contract.
    pub const fn config(&self) -> MixerConfig {
        self.config
    }

    /// Captures this physical next-frame basis without changing mixer state.
    pub fn output_frame_basis(&self) -> super::OutputFrameBasis {
        super::OutputFrameBasis::new(
            crate::time::ClockPoint {
                domain: self.config.domain(),
                timestamp: self.config.origin(),
            },
            self.config.format().sample_rate(),
            self.frame_cursor,
        )
        .expect("mixer format has a validated positive sample rate")
    }
    /// Absolute next output frame; Seek does not rewind it.
    pub const fn frame_cursor(&self) -> u64 {
        self.frame_cursor
    }
    /// Next scheduling frame, excluding output frames emitted while paused.
    /// Seek and SetRate ZERO do not rewind or freeze this cursor.
    pub const fn playback_frame_cursor(&self) -> u64 {
        self.playback_frame_cursor
    }
    /// Immutable startup gate: None is ungated, Some(None) is unarmed, and
    /// Some(Some(frame)) is the exact selected physical start target.
    pub fn start_gate_frame(&self) -> Option<Option<u64>> {
        self.consumer.start_gate()
    }
    /// Actual first positive playback frame, absent before genuine startup.
    /// Ungated, held and zero-length finite playback do not publish this evidence.
    pub fn applied_start_frame(&self) -> Option<u64> {
        self.consumer.applied_start_frame()
    }
    /// Effective producer pause state, including an outstanding exclusive hold.
    /// Reading it does not consume commands.
    /// Cold output preparation must keep this requested through native priming.
    pub fn pause_requested(&self) -> bool {
        self.consumer.pause_requested()
    }
    /// Applied end-state of the most recent valid nonempty render, including
    /// an immutable playback endpoint that queue resume cannot lift.
    pub const fn is_paused(&self) -> bool {
        self.paused
    }
    fn mark_playback_end(&mut self, physical: u64) {
        if self.playback_end_physical_frame.is_none()
            && self
                .config
                .playback_end_frame()
                .is_some_and(|end| self.playback_frame_cursor >= end)
        {
            self.playback_end_physical_frame = Some(physical);
        }
    }

    /// Fixed cumulative telemetry snapshot.
    pub const fn counters(&self) -> AudioCounters {
        self.counters
    }

    /// Currently applied signed rational sample-head speed.
    pub const fn rate(&self) -> Rate {
        self.rate
    }

    fn target_frame(&self, at: Timestamp) -> Option<i128> {
        let delta = i128::from(at.as_nanos()) - i128::from(self.config.origin().as_nanos());
        let numerator = delta.checked_mul(i128::from(self.config.format().sample_rate()))?;
        let denominator = 1_000_000_000i128;
        let floor = numerator.div_euclid(denominator);
        let target = floor.checked_add(i128::from(numerator.rem_euclid(denominator) != 0))?;
        if target > i128::from(u64::MAX) {
            None
        } else {
            Some(target)
        }
    }

    fn prepare_runtime_rate_decision(
        &self,
        playback: u64,
        physical: u64,
        origin: crate::time::ClockPoint,
    ) -> Result<Option<super::practice::RateDecision>, super::PracticeError> {
        use super::practice::Practice;
        use super::{PracticeRateOutcome, PracticeRateRefusal};
        let Some(practice) = self.practice.as_ref() else {
            return Ok(None);
        };
        let Some(mut decision) = practice.prepare_rate_decision(playback, physical, origin)? else {
            return Ok(None);
        };
        if decision.outcome != PracticeRateOutcome::Applied {
            return Ok(Some(decision));
        }
        let program = &practice.program.data;
        let cursor = decision.cursor;
        let rate = cursor.rate;
        let refuse = |decision: &mut super::practice::RateDecision, reason| {
            decision.cursor = practice.cursor;
            decision.outcome = PracticeRateOutcome::Refused(reason);
        };
        if !decision.control.restart
            && self.voices.iter().flatten().any(|voice| {
                let sample = self.bank.get(voice.sample).expect("owned voice asset");
                voice
                    .head
                    .with_rate(sample.format().sample_rate(), program.output_rate, rate)
                    .is_none()
            })
        {
            refuse(&mut decision, PracticeRateRefusal::VoiceDenominatorCapacity);
            return Ok(Some(decision));
        }
        let first_index = if decision.control.restart {
            cursor.next_cue
        } else {
            decision.cut_due_end
        };
        let future_burst =
            program.future_cue_burst(cursor.region, first_index, decision.control.max_cue_burst);
        if future_burst > program.limits.max_events_per_render
            || (cursor.region.repeat || decision.control.restart)
                && program.pass_event_bound(cursor.region, decision.control.max_cue_burst)
                    > program.limits.max_events_per_render
        {
            refuse(&mut decision, PracticeRateRefusal::EventBudget);
            return Ok(Some(decision));
        }
        let first = program
            .nonempty_cues
            .partition_point(|index| *index < first_index);
        let end = program
            .nonempty_cues
            .partition_point(|index| program.cues[*index].cue.at < cursor.region.end);
        if cursor.region.repeat
            && program.pass_voice_bound(cursor.region, decision.control.max_overlap)
                > program.limits.max_overlap
        {
            // A later pass revisits cues already behind this cut. Its upper
            // bound is the smaller of the cold global overlap and every voice
            // that can be born in that pass, including clipped start voices.
            // Only the bounded overlap tree is visited; future count is indexed.
            refuse(&mut decision, PracticeRateRefusal::BgmCapacity);
            return Ok(Some(decision));
        }
        let future_bound = decision.control.max_overlap.min(end.saturating_sub(first));
        let first_future_delta = if first < end {
            let cue = program.cues[program.nonempty_cues[first]];
            let relative =
                match Practice::phase_frame(cursor.phase.unwrap(), cue.cue.at, program.output_rate)
                {
                    Ok(frame) => frame,
                    Err(_) => {
                        refuse(&mut decision, PracticeRateRefusal::DeadlineOverflow);
                        return Ok(Some(decision));
                    }
                };
            match cursor
                .origin
                .checked_add(relative)
                .and_then(|frame| frame.checked_sub(playback))
            {
                Some(delta) => Some(delta),
                None => {
                    refuse(&mut decision, PracticeRateRefusal::DeadlineOverflow);
                    return Ok(Some(decision));
                }
            }
        } else {
            None
        };
        let mut carry = 0usize;
        let mut carry_at_future = 0usize;
        let mut include = |head: Head, frames: usize, source_hz: u32| {
            carry += 1;
            if first_future_delta.is_some_and(|delta| {
                !head.proven_retired_after(delta, frames, source_hz, program.output_rate, rate)
            }) {
                carry_at_future += 1;
            }
        };
        if decision.control.restart {
            program.overlaps(cursor.anchor, |index| {
                let cue = program.cues[index];
                let selected = super::practice::ceil(
                    (i128::from(cursor.anchor.as_nanos()) - i128::from(cue.cue.at.as_nanos()))
                        * i128::from(cue.source_rate),
                    1_000_000_000,
                ) as usize;
                if let Some(head) = Head::new(selected, cue.source_rate, program.output_rate, rate)
                {
                    include(head, cue.frames, cue.source_rate);
                }
            });
        } else {
            for voice in self.voices[..program.limits.max_overlap].iter().flatten() {
                let sample = self.bank.get(voice.sample).expect("owned voice asset");
                include(voice.head, sample.frames(), sample.format().sample_rate());
            }
            for cue in &program.cues[practice.cursor.next_cue..decision.cut_due_end] {
                if cue.frames != 0 {
                    let head = Head::new(0, cue.source_rate, program.output_rate, rate)
                        .expect("single positive rate head fits i128");
                    include(head, cue.frames, cue.source_rate);
                }
            }
        }
        if carry > program.limits.max_overlap
            || carry_at_future + future_bound > program.limits.max_overlap
        {
            refuse(&mut decision, PracticeRateRefusal::BgmCapacity);
        }
        Ok(Some(decision))
    }

    fn preflight_practice(
        &mut self,
        playback: u64,
        frames: usize,
        physical: u64,
        origin: crate::time::ClockPoint,
        decision: Option<super::practice::RateDecision>,
    ) -> Result<usize, super::PracticeError> {
        use super::practice::PreflightVoiceStep;
        let Some(practice) = self.practice.as_ref() else {
            return Ok(0);
        };
        if practice.program.data.runtime_program_id == 0 {
            return practice.preflight_at(playback, frames, physical, origin, decision, |_| Ok(()));
        }
        let program = &practice.program.data;
        let bank = &self.bank;
        let scratch = &mut self.practice_preflight_voices;
        scratch.copy_from_slice(&self.voices[..program.limits.max_overlap]);
        practice.preflight_at(playback, frames, physical, origin, decision, |step| {
            match step {
                PreflightVoiceStep::Rate(rate) => {
                    for voice in scratch.iter_mut().flatten() {
                        let sample = bank.get(voice.sample).expect("owned voice asset");
                        voice.head = voice
                            .head
                            .with_rate(sample.format().sample_rate(), program.output_rate, rate)
                            .ok_or(super::PracticeError::Capacity)?;
                    }
                }
                PreflightVoiceStep::Reset(cursor) => {
                    scratch.fill(None);
                    if cursor.active {
                        let mut result = Ok(());
                        program.overlaps(cursor.anchor, |index| {
                            if result.is_err() {
                                return;
                            }
                            let cue = program.cues[index];
                            let selected = super::practice::ceil(
                                (i128::from(cursor.anchor.as_nanos())
                                    - i128::from(cue.cue.at.as_nanos()))
                                    * i128::from(cue.source_rate),
                                1_000_000_000,
                            ) as usize;
                            result = start_program_voice(
                                scratch,
                                bank,
                                cue,
                                selected,
                                program.output_rate,
                                program.limits.max_overlap,
                                cursor.rate,
                            );
                        });
                        result?;
                    }
                }
                PreflightVoiceStep::Cue(cue, rate) => {
                    start_program_voice(
                        scratch,
                        bank,
                        cue,
                        0,
                        program.output_rate,
                        program.limits.max_overlap,
                        rate,
                    )?;
                }
                PreflightVoiceStep::Advance => advance_voices(scratch, bank),
            }
            Ok(())
        })
    }

    fn practice_frame(
        &mut self,
        physical: u64,
        control_snapshot: usize,
        consumed: &mut usize,
        decision: Option<super::practice::RateDecision>,
    ) {
        use super::practice::{ceil, Practice};
        use super::{PracticeBoundaryKind, PracticeReceipt};
        let practice = self.practice.as_mut().expect("installed program");
        if let Some(decision) = decision {
            if decision.outcome == super::PracticeRateOutcome::Applied {
                if decision.control.restart {
                    self.voices.fill(None);
                    self.pending
                        .retain(|pending| pending.scope.0 >= decision.cursor.generation);
                    let mut result = Ok(());
                    practice
                        .program
                        .data
                        .overlaps(decision.cursor.anchor, |index| {
                            if result.is_err() {
                                return;
                            }
                            let cue = practice.program.data.cues[index];
                            let selected = ceil(
                                (i128::from(decision.cursor.anchor.as_nanos())
                                    - i128::from(cue.cue.at.as_nanos()))
                                    * i128::from(cue.source_rate),
                                1_000_000_000,
                            ) as usize;
                            result = start_program_voice(
                                &mut self.voices,
                                &self.bank,
                                cue,
                                selected,
                                practice.program.data.output_rate,
                                practice.program.data.limits.max_overlap,
                                decision.cursor.rate,
                            );
                        });
                    result.expect("whole-block retained voice preflight");
                } else {
                    for voice in self.voices.iter_mut().flatten() {
                        let sample = self.bank.get(voice.sample).expect("owned voice asset");
                        voice.head = voice
                            .head
                            .with_rate(
                                sample.format().sample_rate(),
                                practice.program.data.output_rate,
                                decision.cursor.rate,
                            )
                            .expect("frozen all-head runtime rate decision");
                    }
                }
                self.rate = decision.cursor.rate;
            }
            practice.cursor = decision.cursor;
            practice.endpoint.consume();
            *consumed += 1;
            practice
                .endpoint
                .rate_event(&decision, physical, self.playback_frame_cursor);
        }
        loop {
            let request = if *consumed < control_snapshot {
                practice.endpoint.request(0)
            } else {
                None
            };
            let boundary = Practice::step(
                &practice.program.data,
                &mut practice.cursor,
                self.playback_frame_cursor,
                request,
            )
            .expect("scalar practice preflight");
            let Some((id, kind)) = boundary else {
                break;
            };
            {
                if matches!(
                    kind,
                    PracticeBoundaryKind::Requested
                        | PracticeBoundaryKind::LoopDisabled
                        | PracticeBoundaryKind::ControlRejectedRevision
                ) {
                    practice.endpoint.consume();
                    *consumed += 1;
                }
                let cursor = practice.cursor;
                if !matches!(
                    kind,
                    PracticeBoundaryKind::LoopDisabled
                        | PracticeBoundaryKind::ControlRejectedRevision
                ) {
                    self.voices.fill(None);
                    self.pending
                        .retain(|pending| pending.scope.0 >= cursor.generation);
                    self.song_position = cursor.anchor;
                    if cursor.active {
                        let bank = &self.bank;
                        let voices = &mut self.voices;
                        practice.program.data.overlaps(cursor.anchor, |index| {
                            let cue = practice.program.data.cues[index];
                            let selected = ceil(
                                (i128::from(cursor.anchor.as_nanos())
                                    - i128::from(cue.cue.at.as_nanos()))
                                    * i128::from(cue.source_rate),
                                1_000_000_000,
                            ) as usize;
                            start_program_voice(
                                voices,
                                bank,
                                cue,
                                selected,
                                practice.program.data.output_rate,
                                practice.program.data.limits.max_overlap,
                                cursor.rate,
                            )
                            .expect("preflight retained overlap capacity");
                        });
                    }
                }
                let requested = if kind == PracticeBoundaryKind::Ended {
                    cursor.region.end
                } else if kind == PracticeBoundaryKind::LoopDisabled {
                    cursor.anchor
                } else if kind == PracticeBoundaryKind::ControlRejectedRevision {
                    request
                        .filter(|control| !control.disable)
                        .map_or(cursor.anchor, |control| control.request.region.start)
                } else {
                    cursor.region.start
                };
                let applied = if kind == PracticeBoundaryKind::ControlRejectedRevision
                    || kind == PracticeBoundaryKind::Started
                {
                    cursor.anchor
                } else {
                    requested
                };
                practice.endpoint.receipt(
                    PracticeReceipt {
                        request_id: id,
                        generation: cursor.generation,
                        iteration: cursor.iteration,
                        physical_frame: physical,
                        playback_frame: self.playback_frame_cursor,
                        requested_song_time: requested,
                        applied_song_time: applied,
                        correction_nanos: i64::try_from(
                            i128::from(applied.as_nanos()) - i128::from(requested.as_nanos()),
                        )
                        .expect("preflight checked rejection correction"),
                        kind,
                    },
                    &cursor,
                );
            }
        }
        while Practice::cue_due(
            &practice.program.data,
            &practice.cursor,
            self.playback_frame_cursor,
        ) {
            let cue = practice.program.data.cues[practice.cursor.next_cue];
            Practice::advance_cue(&practice.program.data, &mut practice.cursor)
                .expect("scalar practice preflight");
            start_program_voice(
                &mut self.voices,
                &self.bank,
                cue,
                0,
                practice.program.data.output_rate,
                practice.program.data.limits.max_overlap,
                practice.cursor.rate,
            )
            .expect("whole-block retained voice preflight");
        }
        self.song_position = Practice::song_at(
            &practice.program.data,
            &practice.cursor,
            self.playback_frame_cursor,
        )
        .expect("scalar practice preflight");
    }

    fn apply(&mut self, command: AudioCommand) {
        if self.practice.is_some()
            && matches!(
                command,
                AudioCommand::SetRate { .. } | AudioCommand::Seek { .. }
            )
        {
            // Retained program owns its original-song mapping. Pause remains the
            // independent queue pause; legacy Seek/rate semantics stay program-free.
            increment(&mut self.counters.invalid_times);
            return;
        }
        match command {
            AudioCommand::Play {
                voice,
                sample,
                gain,
                ..
            } => self.play(voice, sample, gain),
            AudioCommand::Stop { voice, .. } => {
                if let Some(slot) = self
                    .voices
                    .iter_mut()
                    .skip(
                        self.practice
                            .as_ref()
                            .map_or(0, |p| p.program.data.limits.max_overlap),
                    )
                    .find(|slot| slot.is_some_and(|active| active.id == voice))
                {
                    *slot = None;
                    increment(&mut self.counters.commands_applied);
                } else {
                    increment(&mut self.counters.unknown_stops);
                    increment(&mut self.counters.commands_applied);
                }
            }
            AudioCommand::SetRate { rate, .. } => self.set_rate(rate),
            AudioCommand::Seek { song_time, .. } => {
                self.voices.fill(None);
                self.song_position = song_time;
                increment(&mut self.counters.commands_applied);
            }
        }
    }

    fn settle_ended_commands(&mut self) {
        let mut remaining = self.config.limits().max_commands_per_render();
        // Execution order is no longer observable: none of these commands may
        // change the frozen playback state. Pop avoids shifting a pending prefix.
        while remaining != 0 {
            let Some(pending) = self.pending.pop() else {
                break;
            };
            remaining -= 1;
            self.expire(pending.command);
        }
        // One snapshot only; publication concurrent with this render belongs to
        // a later callback and cannot extend the callback's bounded work.
        let available = self.consumer.available_up_to(remaining);
        for _ in 0..available {
            let Ok((command, _)) = self.consumer.try_pop_scoped() else {
                break;
            };
            increment(&mut self.counters.commands_consumed);
            self.expire(command);
        }
    }

    fn expire(&mut self, command: AudioCommand) {
        if self.target_frame(command.at()).is_none() {
            increment(&mut self.counters.invalid_times);
            return;
        }
        let valid = match command {
            AudioCommand::Play { sample, gain, .. } => self.play_head(sample, gain).is_some(),
            AudioCommand::SetRate { rate, .. } => self.validate_rate(rate),
            AudioCommand::Stop { .. } | AudioCommand::Seek { .. } => true,
        };
        if valid {
            increment(&mut self.counters.commands_expired);
        }
    }

    // Some(None) is a valid empty asset; None is a rejected command.
    fn play_head(&mut self, sample_id: SampleId, gain: f32) -> Option<Option<Head>> {
        if !gain.is_finite() {
            increment(&mut self.counters.invalid_gains);
            return None;
        }
        let Some(sample) = self.bank.get(sample_id) else {
            increment(&mut self.counters.unknown_samples);
            return None;
        };
        if sample.frames() == 0 {
            return Some(None);
        }
        let initial_frame = if self.rate.numerator() < 0 {
            sample.frames() - 1
        } else {
            0
        };
        let Some(head) = Head::new(
            initial_frame,
            sample.format().sample_rate(),
            self.config.format().sample_rate(),
            self.rate,
        ) else {
            increment(&mut self.counters.invalid_rates);
            return None;
        };
        Some(Some(head))
    }

    fn play(&mut self, id: VoiceId, sample_id: SampleId, gain: f32) {
        let Some(head) = self.play_head(sample_id, gain) else {
            return;
        };
        let reserved = self
            .practice
            .as_ref()
            .map_or(0, |p| p.program.data.limits.max_overlap);
        let existing = self
            .voices
            .iter()
            .enumerate()
            .skip(reserved)
            .find(|(_, slot)| slot.is_some_and(|active| active.id == id))
            .map(|(index, _)| index);
        // A valid empty asset replaces an existing voice with silence and needs
        // no free slot of its own. No storage ownership is released here.
        let Some(head) = head else {
            if let Some(index) = existing {
                self.voices[index] = None;
            }
            increment(&mut self.counters.commands_applied);
            return;
        };
        let Some(index) = existing.or_else(|| {
            self.voices
                .iter()
                .enumerate()
                .skip(reserved)
                .find(|(_, slot)| slot.is_none())
                .map(|(index, _)| index)
        }) else {
            increment(&mut self.counters.voice_full);
            return;
        };
        self.voices[index] = Some(Voice {
            id,
            sample: sample_id,
            gain,
            head,
        });
        increment(&mut self.counters.commands_applied);
    }

    fn validate_rate(&mut self, rate: Rate) -> bool {
        let output_rate = self.config.format().sample_rate();
        // Preflight every voice before changing any voice or global rate.
        for voice in self.voices.iter().flatten() {
            let sample = self.bank.get(voice.sample).expect("owned voice asset");
            if voice
                .head
                .with_rate(sample.format().sample_rate(), output_rate, rate)
                .is_none()
            {
                increment(&mut self.counters.invalid_rates);
                return false;
            }
        }
        true
    }

    fn set_rate(&mut self, rate: Rate) {
        if !self.validate_rate(rate) {
            return;
        }
        let output_rate = self.config.format().sample_rate();
        for voice in self.voices.iter_mut().flatten() {
            let sample = self.bank.get(voice.sample).expect("owned voice asset");
            voice.head = voice
                .head
                .with_rate(sample.format().sample_rate(), output_rate, rate)
                .expect("all rational rate changes were preflighted");
        }
        self.rate = rate;
        increment(&mut self.counters.commands_applied);
    }

    fn mix_frame(&mut self, output: &mut [f32]) {
        output.fill(0.0);
        if self.rate == Rate::ZERO {
            return;
        }
        // Deterministic slot order, with one wide sum and clamp per channel.
        // Even maximal finite f32 samples/gains and MAX_VOICES fit in f64.
        for (channel, destination) in output.iter_mut().enumerate() {
            let mut sum = 0.0f64;
            for voice in self.voices.iter().flatten() {
                let sample = self.bank.get(voice.sample).expect("owned voice asset");
                sum += interpolated(sample, voice.head, channel) * f64::from(voice.gain);
            }
            *destination = sum.clamp(-1.0, 1.0) as f32;
        }
        advance_voices(&mut self.voices, &self.bank);
    }

    fn report(&self, start_frame: u64, playback_start_frame: u64, frames: usize) -> RenderReport {
        RenderReport {
            start_frame,
            frames,
            playback_start_frame,
            playback_frames: usize::try_from(self.playback_frame_cursor - playback_start_frame)
                .expect("playback extent is bounded by this block's usize extent"),
            paused: self.paused,
            playback_end_physical_frame: self.playback_end_physical_frame,
            active_voices: self.voices.iter().filter(|slot| slot.is_some()).count(),
            pending_commands: self.pending.len(),
            song_position: self.song_position,
            producer_disconnected: self.consumer.is_disconnected(),
            counters: self.counters,
        }
    }
}

fn start_program_voice(
    voices: &mut [Option<Voice>],
    bank: &SampleBank,
    cue: super::practice::Cue,
    selected: usize,
    output_rate: u32,
    reserved: usize,
    rate: Rate,
) -> Result<(), super::PracticeError> {
    if selected >= cue.frames {
        return Ok(());
    }
    let sample = bank
        .get(cue.cue.sample)
        .expect("cold validated original asset");
    let index = voices[..reserved]
        .iter()
        .position(Option::is_none)
        .ok_or(super::PracticeError::Capacity)?;
    voices[index] = Some(Voice {
        id: cue.cue.voice,
        sample: cue.cue.sample,
        gain: cue.cue.gain,
        head: Head::new(selected, sample.format().sample_rate(), output_rate, rate)
            .ok_or(super::PracticeError::Capacity)?,
    });
    Ok(())
}

fn advance_voices(voices: &mut [Option<Voice>], bank: &SampleBank) {
    for slot in voices {
        if let Some(voice) = slot {
            voice.head.advance();
            let sample = bank.get(voice.sample).expect("owned voice asset");
            if !voice.head.in_bounds(sample.frames()) {
                *slot = None;
            }
        }
    }
}

fn practice_audio_error(error: super::PracticeError) -> AudioError {
    match error {
        super::PracticeError::ReceiptFull => AudioError::PracticeReceiptFull,
        super::PracticeError::EventBudget => AudioError::PracticeEventBudget,
        _ => AudioError::PracticeControl,
    }
}

fn interpolated(sample: &PcmSample, head: Head, channel: usize) -> f64 {
    let channels = usize::from(sample.format().channels());
    let frame = head.whole as usize;
    let next = (frame + 1).min(sample.frames() - 1);
    let first = f64::from(sample.samples()[frame * channels + channel]);
    let second = f64::from(sample.samples()[next * channels + channel]);
    let fraction = head.fraction as f64 / head.denominator as f64;
    first + (second - first) * fraction
}

fn increment(counter: &mut u64) {
    *counter = counter.saturating_add(1);
}

fn gcd(mut left: i128, mut right: i128) -> i128 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

#[cfg(test)]
#[path = "runtime_rate_fixtures.rs"]
mod runtime_rate_fixtures;

#[cfg(test)]
mod start_gate_race_fixtures {
    use super::*;
    use crate::{
        audio::{command_queue_with_start_gate, AudioFormat, AudioLimits, PcmLimits},
        time::ClockDomainId,
    };
    #[test]
    fn target_missed_during_publication_rejects_without_callback_mutation() {
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = AudioLimits::new(1, 1, 1, 8, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(4, 4, 1).unwrap()).unwrap();
        let (mut producer, consumer) = command_queue_with_start_gate(1).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(format, ClockDomainId(1), Timestamp::ZERO, limits),
            bank,
            consumer,
        )
        .unwrap();
        producer
            .try_push(AudioCommand::Seek {
                at: Timestamp::ZERO,
                song_time: Timestamp::from_nanos(99),
            })
            .unwrap();
        producer.schedule_start_at(2).unwrap();
        // Model the race where a held render already selected its silent span
        // before publication, then passed the target without opening the gate.
        mixer.frame_cursor = 3;
        let before = mixer.report(3, 0, 0);
        let mut output = [99.0; 2];
        assert_eq!(mixer.render(&mut output), Err(AudioError::StartGateMissed));
        assert_eq!(output, [99.0; 2]);
        assert_eq!(mixer.report(3, 0, 0), before);
        assert_eq!(producer.applied_start_frame(), None);
        assert_eq!(mixer.consumer.available(), 1);
        assert_eq!(mixer.render(&mut []).unwrap(), before);
    }
}

#[cfg(test)]
mod practice_snapshot_race_fixtures {
    use super::*;
    use crate::audio::{
        command_queue, practice_queue, AudioFormat, AudioLimits, PcmLimits, PracticeBoundaryKind,
        PracticeCue, PracticeLimits, PracticeRegion, PreparedPracticeProgram,
    };
    use crate::time::ClockDomainId;
    #[test]
    fn next_control_published_after_snapshot_survives_actual_autonomous_frame_steps() {
        let format = AudioFormat::new(1000, 1).unwrap();
        let pl = PcmLimits::new(128, 128, 1).unwrap();
        let mut bank = SampleBank::new(format, pl).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.1, 0.2, 0.3, 0.4], pl).unwrap(),
        )
        .unwrap();
        let program = PreparedPracticeProgram::new(
            &bank,
            vec![PracticeCue {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            }],
            PracticeLimits::new(1, 1, 64, 4, 16).unwrap(),
        )
        .unwrap();
        let (mut controller, endpoint) = practice_queue(&program).unwrap();
        let (_producer, consumer) = command_queue(4).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(1),
                Timestamp::ZERO,
                AudioLimits::new(4, 2, 4, 16, 4).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(1_000_000), true)
                    .unwrap(),
            )
            .unwrap();
        // Deterministically reproduce producer publication immediately AFTER
        // the real callback's bounded snapshot. Execute the same production
        // per-frame methods with that frozen snapshot, without thread sleeps.
        let snapshot = mixer.practice.as_ref().unwrap().preflight(0, 4).unwrap();
        assert_eq!(snapshot, 0);
        controller
            .try_request_next(
                1,
                1,
                PracticeRegion::new(
                    Timestamp::from_nanos(2_000_000),
                    Timestamp::from_nanos(4_000_000),
                    false,
                )
                .unwrap(),
            )
            .unwrap();
        let mut consumed = 0;
        let mut old_pcm = [0.0; 4];
        for (physical, frame) in old_pcm.iter_mut().enumerate() {
            mixer.practice_frame(physical as u64, snapshot, &mut consumed, None);
            mixer.mix_frame(std::slice::from_mut(frame));
            mixer.playback_frame_cursor += 1;
            mixer.frame_cursor += 1;
        }
        mixer.project_direct_practice_receipts(0, 4);
        assert_eq!(old_pcm, [0.1; 4]);
        assert_eq!(controller.generation(), 4);
        let mut new_pcm = [0.0; 2];
        mixer.render(&mut new_pcm).unwrap();
        assert_eq!(new_pcm, [0.3, 0.4]);
        for generation in 1..=4 {
            assert_eq!(controller.try_pop_receipt().unwrap().generation, generation);
        }
        let applied = controller.try_pop_receipt().unwrap();
        assert_eq!(
            (
                applied.request_id,
                applied.generation,
                applied.physical_frame,
                applied.kind
            ),
            (1, 5, 4, PracticeBoundaryKind::Requested)
        );
    }
}
