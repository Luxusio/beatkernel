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
}

#[derive(Clone, Copy)]
struct Pending {
    target: i128,
    preroll: bool,
    command: AudioCommand,
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
}

impl Mixer {
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
        })
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
        // All failure paths precede state, output, queue and evidence changes.
        if prefix_extent == extent {
            self.paused = true;
            output.fill(0.0);
            self.frame_cursor = end;
            self.consumer.publish_physical_frontier(end);
            self.counters.rendered_frames = self.counters.rendered_frames.saturating_add(extent);
            return Ok(self.report(start, playback_start, frames));
        }
        if opens_gate {
            self.start_gate_open = true;
        }
        self.paused = pause_requested || active_extent == 0;
        if self.paused {
            self.mark_playback_end(active_start);
            output.fill(0.0);
            self.frame_cursor = end;
            self.consumer.publish_physical_frontier(end);
            self.counters.rendered_frames = self.counters.rendered_frames.saturating_add(extent);
            return Ok(self.report(start, playback_start, frames));
        }
        output[..prefix_samples].fill(0.0);

        let budget = self
            .consumer
            .available_up_to(self.config.limits().max_commands_per_render());
        for _ in 0..budget {
            let Ok(command) = self.consumer.try_pop() else {
                break;
            };
            increment(&mut self.counters.commands_consumed);
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
                },
            );
        }

        for frame in
            output[prefix_samples..prefix_samples + active_samples].chunks_exact_mut(channels)
        {
            while self
                .pending
                .first()
                .is_some_and(|pending| pending.target <= i128::from(self.playback_frame_cursor))
            {
                let pending = self.pending.remove(0);
                if pending.preroll || pending.target < i128::from(self.playback_frame_cursor) {
                    increment(&mut self.counters.late_commands);
                }
                self.apply(pending.command);
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
        Ok(self.report(start, playback_start, frames))
    }

    /// Immutable format, clock origin and capacity contract.
    pub const fn config(&self) -> MixerConfig {
        self.config
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

    fn apply(&mut self, command: AudioCommand) {
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

    fn play(&mut self, id: VoiceId, sample_id: SampleId, gain: f32) {
        if !gain.is_finite() {
            increment(&mut self.counters.invalid_gains);
            return;
        }
        let Some(sample) = self.bank.get(sample_id) else {
            increment(&mut self.counters.unknown_samples);
            return;
        };
        let existing = self
            .voices
            .iter()
            .position(|slot| slot.is_some_and(|active| active.id == id));
        // A valid empty asset replaces an existing voice with silence and needs
        // no free slot of its own. No storage ownership is released here.
        if sample.frames() == 0 {
            if let Some(index) = existing {
                self.voices[index] = None;
            }
            increment(&mut self.counters.commands_applied);
            return;
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
            return;
        };
        let Some(index) = existing.or_else(|| self.voices.iter().position(Option::is_none)) else {
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

    fn set_rate(&mut self, rate: Rate) {
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
                return;
            }
        }
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
        for slot in &mut self.voices {
            if let Some(voice) = slot {
                voice.head.advance();
                let sample = self.bank.get(voice.sample).expect("owned voice asset");
                if !voice.head.in_bounds(sample.frames()) {
                    *slot = None;
                }
            }
        }
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
mod start_gate_race_fixtures {
    use super::*;
    use crate::{
        audio::{AudioFormat, AudioLimits, PcmLimits, command_queue_with_start_gate},
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
