//! Full-song completion from actual judge, mixer and native presentation evidence.
//! Preparation and observation belong to the game owner, never audio callbacks.
use crate::{PreparedBms, bgm::BgmFeedReport};
use beatkernel::{
    audio::RenderReport,
    chart::ObjectId,
    interaction::InteractionState,
    judge::JudgeEngine,
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{collections::BTreeMap, error::Error, fmt};

/// Invalid preparation or observation, with no substituted clock or duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletionError(pub &'static str);
impl fmt::Display for CompletionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl Error for CompletionError {}

/// Prepared builtin BMS deadlines and a later output-drain barrier.
///
/// The calibration extent is conservative setup data, not an end-of-song clock.
/// An idle mixer block alone cannot prove native presentation or acoustic output.
pub struct SongCompletion {
    objects: Vec<ObjectId>,
    mine_count: usize,
    judge_until: Timestamp,
    calibration_seconds: u64,
    judged: bool,
    drain: OutputDrain,
}
impl SongCompletion {
    /// Uses actual chart endpoints and referenced PCM source rates/frame counts.
    /// The existing native compositions use normal-rate PCM and output frame zero.
    pub fn prepare(
        prepared: &PreparedBms,
        late_ns: i64,
        input_offset_ns: i64,
        preroll_ns: i64,
        output_domain: ClockDomainId,
    ) -> Result<Self, CompletionError> {
        if late_ns < 0 || preroll_ns < 0 {
            return Err(CompletionError("negative completion window or preroll"));
        }
        let mut objects = Vec::new();
        objects
            .try_reserve_exact(prepared.compiled.chart.objects().len())
            .map_err(|_| CompletionError("completion identity allocation failed"))?;
        let mut judge_until = 0i128;
        for object in prepared.compiled.chart.objects() {
            objects.push(object.id);
            let endpoint = object.time.end.unwrap_or(object.time.start).as_nanos();
            // Builtin deadlines are inclusive. Advance strictly past the last
            // deadline in effective time, while retaining unoffset song time.
            judge_until = judge_until
                .max(i128::from(endpoint) + i128::from(late_ns) - i128::from(input_offset_ns) + 1);
        }
        let invisible = if prepared.source.invisible.is_empty() {
            Vec::new()
        } else {
            prepared
                .source
                .compile_invisible()
                .map_err(|_| CompletionError("invalid invisible completion timeline"))?
        };
        for event in &invisible {
            judge_until = judge_until.max(i128::from(event.at.as_nanos()) + 1);
        }
        let mines = if prepared.source.mines.is_empty() {
            Vec::new()
        } else {
            prepared
                .source
                .compile_mines()
                .map_err(|_| CompletionError("invalid mine completion timeline"))?
        };
        for event in &mines {
            // Hazards have no normal-note late window. Their effective judge
            // boundary must be passed in the original, unoffset song clock.
            judge_until =
                judge_until.max(i128::from(event.at.as_nanos()) - i128::from(input_offset_ns) + 1);
        }
        let starts: BTreeMap<_, _> = prepared
            .compiled
            .chart
            .objects()
            .iter()
            .map(|object| (object.id, object.time.start.as_nanos()))
            .collect();
        let mut song_extent = judge_until;
        if prepared.source.samples.contains_key(&0)
            && mines.iter().any(|mine| !mine.damage.is_fatal())
        {
            let duration = sample_duration(prepared, beatkernel::audio::SampleId(0))?;
            for mine in mines.iter().filter(|mine| !mine.damage.is_fatal()) {
                let at = i128::from(mine.at.as_nanos()) - i128::from(input_offset_ns);
                song_extent = song_extent.max(at + duration);
            }
        }
        for sound in &prepared.sounds {
            let start = starts
                .get(&sound.object)
                .ok_or(CompletionError("completion sound has no chart object"))?;
            // A head may be hit at its inclusive late edge. Scheduling itself
            // still uses the native output frontier; this is only a calibration bound.
            let at =
                (i128::from(*start) + i128::from(late_ns) - i128::from(input_offset_ns)).max(0);
            song_extent = song_extent.max(at + sample_duration(prepared, sound.sample)?);
        }
        for event in &invisible {
            song_extent = song_extent
                .max(i128::from(event.at.as_nanos()) + sample_duration(prepared, event.sample)?);
        }
        for command in &prepared.bgm_commands {
            let beatkernel::audio::AudioCommand::Play { at, sample, .. } = *command else {
                return Err(CompletionError(
                    "completion expects prepared BGM Play commands",
                ));
            };
            song_extent =
                song_extent.max(i128::from(at.as_nanos()) + sample_duration(prepared, sample)?);
        }
        let seconds = ((song_extent.max(1) + 999_999_999) / 1_000_000_000).max(1);
        // Match native calibration's preroll and three-second extrapolation margin.
        // Reject unrepresentable extents rather than silently truncating a long chart.
        i64::try_from(seconds * 1_000_000_000 + i128::from(preroll_ns) + 3_000_000_000)
            .map_err(|_| CompletionError("full-song calibration extent exceeds timestamps"))?;
        Ok(Self {
            objects,
            mine_count: mines.len(),
            judge_until: Timestamp::from_nanos(
                i64::try_from(judge_until)
                    .map_err(|_| CompletionError("full-song judge deadline exceeds timestamps"))?,
            ),
            calibration_seconds: u64::try_from(seconds)
                .map_err(|_| CompletionError("full-song seconds overflow"))?,
            judged: false,
            drain: OutputDrain::new(output_domain, prepared.bank.format().sample_rate()),
        })
    }

    /// Finite song extent for the existing Windows initial calibration policy.
    pub const fn calibration_seconds(&self) -> u64 {
        self.calibration_seconds
    }

    /// Immutable unoffset deadline prepared from all retained gameplay timelines.
    pub(crate) const fn judge_until(&self) -> Timestamp {
        self.judge_until
    }

    /// Producer work invalidates an earlier idle barrier without undoing judging.
    /// Hosts with a separately acknowledged output queue must call this whenever
    /// new commands can still reach the mixer after the observed idle block.
    pub fn reset_drain(&mut self) {
        self.drain.after_frame = None;
        self.drain.idle_end = None;
    }

    /// Call after successful input/advance operations and BGM admission.
    /// The supplied presentation point must come from the native output domain.
    /// As in the three native compositions, the mixer must drain at least the
    /// queue's entire capacity per nonempty render; no later commands may be
    /// published once judging and all BGM admission are terminal.
    pub fn observe(
        &mut self,
        judge: &JudgeEngine,
        song_time: Timestamp,
        bgm: BgmFeedReport,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<bool, CompletionError> {
        if judge.hazard_count() != self.mine_count {
            return Err(CompletionError("completion judge hazard count differs"));
        }
        let hazards_finished = judge.remaining_hazards() == 0;
        if !self.judged && song_time >= self.judge_until {
            self.judged = hazards_finished
                && self
                    .objects
                    .iter()
                    .all(|&id| judge.state(id) == Some(InteractionState::Completed));
        }
        self.observe_terminal_ready(self.judged && hazards_finished, bgm, rendered, presented)
    }

    /// Uses caller-proven terminal gameplay without altering retained judge state.
    /// BGM retirement and the original later-render/presentation barrier still apply.
    pub(crate) fn observe_terminal_ready(
        &mut self,
        ready: bool,
        bgm: BgmFeedReport,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<bool, CompletionError> {
        self.drain.observe(
            ready && bgm.remaining == 0 && bgm.outstanding == 0,
            rendered,
            presented,
        )
    }
}

/// Recorded-prefix completion after actual operations, BGM admission and native drain.
/// No judge advancement is added to finish a recording.
pub struct ReplayCompletion {
    drain: OutputDrain,
}
impl ReplayCompletion {
    pub fn new(domain: ClockDomainId, rate: u32) -> Self {
        Self {
            drain: OutputDrain::new(domain, rate),
        }
    }
    /// New or retained outbound commands invalidate the earlier drain barrier.
    pub fn reset_drain(&mut self) {
        self.drain.after_frame = None;
        self.drain.idle_end = None;
    }
    pub fn observe(
        &mut self,
        records_finished: bool,
        bgm: BgmFeedReport,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<bool, CompletionError> {
        if self.drain.rate == 0 {
            return Err(CompletionError("replay completion rate must be positive"));
        }
        self.drain.observe(
            records_finished && bgm.remaining == 0 && bgm.outstanding == 0,
            rendered,
            presented,
        )
    }
}

fn sample_duration(
    prepared: &PreparedBms,
    id: beatkernel::audio::SampleId,
) -> Result<i128, CompletionError> {
    let sample = prepared
        .bank
        .get(id)
        .ok_or(CompletionError("completion sample missing"))?;
    let rate = i128::from(sample.format().sample_rate());
    Ok((sample.frames() as i128 * 1_000_000_000 + rate - 1) / rate)
}

struct OutputDrain {
    domain: ClockDomainId,
    rate: u32,
    after_frame: Option<u64>,
    idle_end: Option<u64>,
}
impl OutputDrain {
    fn new(domain: ClockDomainId, rate: u32) -> Self {
        Self {
            domain,
            rate,
            after_frame: None,
            idle_end: None,
        }
    }
    fn observe(
        &mut self,
        ready: bool,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<bool, CompletionError> {
        if presented.is_some_and(|point| point.domain != self.domain) {
            return Err(CompletionError(
                "completion presentation clock domain differs",
            ));
        }
        if !ready {
            self.after_frame = None;
            self.idle_end = None;
            return Ok(false);
        }
        let Some(report) = rendered else {
            return Ok(false);
        };
        if report.frames == 0 {
            return Ok(false);
        }
        let end = report
            .start_frame
            .checked_add(
                u64::try_from(report.frames)
                    .map_err(|_| CompletionError("completion render extent overflow"))?,
            )
            .ok_or(CompletionError("completion render cursor overflow"))?;
        let Some(barrier) = self.after_frame else {
            // This report may precede the last queue admission or have drained
            // before it. Require a subsequent block starting at/after its end.
            self.after_frame = Some(end);
            return Ok(false);
        };
        if report.start_frame < barrier {
            return Ok(false);
        }
        if report.active_voices != 0 || report.pending_commands != 0 {
            self.idle_end = None;
            return Ok(false);
        }
        // Keep the first idle block fixed: newer silence blocks may be buffered
        // ahead of presentation indefinitely and must not move the finish line.
        let target = *self.idle_end.get_or_insert(end);
        let Some(point) = presented else {
            return Ok(false);
        };
        Ok(
            i128::from(point.timestamp.as_nanos()) * i128::from(self.rate)
                >= i128::from(target) * 1_000_000_000,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatkernel::audio::AudioCounters;
    fn prepared() -> PreparedBms {
        use beatkernel::audio::{
            AudioCommand, AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId,
        };
        use beatkernel::runtime::SoundBinding;
        let source = beatkernel_bms::parse(
            "#BPM 0.1\n#WAV01 tone.wav\n#99911:01\n#99901:01\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let compiled = source.compile().unwrap();
        let limits = PcmLimits::new(1024, 1024, 1).unwrap();
        let mut bank = SampleBank::new(AudioFormat::new(48000, 1).unwrap(), limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(AudioFormat::new(1, 1).unwrap(), vec![0.5, 0.5], limits).unwrap(),
        )
        .unwrap();
        let object = &compiled.chart.objects()[0];
        let sounds = vec![SoundBinding {
            object: object.id,
            stage: beatkernel::judge::JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(object.id.0),
            gain: 1.0,
        }];
        let bgm_commands = vec![AudioCommand::Play {
            at: object.time.start,
            sample: SampleId(1),
            voice: VoiceId(99),
            gain: 1.0,
        }];
        PreparedBms {
            source,
            compiled,
            bank,
            sounds,
            bgm_commands,
        }
    }
    #[test]
    fn multiweek_chart_and_pcm_tail_preserve_integer_extent() {
        let prepared = prepared();
        let start = prepared.compiled.chart.objects()[0].time.start.as_nanos();
        assert!(start > 7 * 24 * 3600 * 1_000_000_000);
        let completion = SongCompletion::prepare(
            &prepared,
            150_000_000,
            -50_000_000,
            3_000_000_000,
            ClockDomainId(7),
        )
        .unwrap();
        assert_eq!(completion.judge_until.as_nanos(), start + 200_000_001);
        let expected_seconds = (start + 200_000_000 + 2_000_000_000 + 999_999_999) / 1_000_000_000;
        assert_eq!(completion.calibration_seconds(), expected_seconds as u64);
        assert!(
            SongCompletion::prepare(&prepared, i64::MAX, i64::MIN, 0, ClockDomainId(7)).is_err()
        );
        assert!(SongCompletion::prepare(&prepared, -1, 0, 0, ClockDomainId(7)).is_err());
    }
    fn report(start: u64, active: usize, pending: usize) -> RenderReport {
        RenderReport {
            start_frame: start,
            frames: 10,
            playback_start_frame: start,
            playback_frames: 10,
            paused: false,
            playback_end_physical_frame: None,
            active_voices: active,
            pending_commands: pending,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        }
    }
    fn point(nanos: i64) -> Option<ClockPoint> {
        Some(ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(nanos),
        })
    }
    #[test]
    fn completion_requires_later_idle_render_and_native_presentation() {
        let mut drain = OutputDrain::new(ClockDomainId(7), 10);
        assert!(
            !drain
                .observe(false, Some(report(0, 0, 0)), point(9_000_000_000))
                .unwrap()
        );
        assert!(
            !drain
                .observe(true, Some(report(0, 0, 0)), point(9_000_000_000))
                .unwrap()
        );
        assert!(
            !drain
                .observe(true, Some(report(0, 0, 0)), point(9_000_000_000))
                .unwrap()
        );
        assert!(
            !drain
                .observe(true, Some(report(10, 1, 0)), point(9_000_000_000))
                .unwrap()
        );
        assert!(
            !drain
                .observe(true, Some(report(20, 0, 1)), point(9_000_000_000))
                .unwrap()
        );
        assert!(
            !drain
                .observe(true, Some(report(30, 0, 0)), point(3_999_999_999))
                .unwrap()
        );
        // The target stays at40 even when the mixer renders more buffered silence.
        assert!(
            drain
                .observe(true, Some(report(50, 0, 0)), point(4_000_000_000))
                .unwrap()
        );
    }
    #[test]
    fn missing_evidence_and_host_domain_cannot_finish_play() {
        let mut drain = OutputDrain::new(ClockDomainId(7), 48000);
        assert!(!drain.observe(true, None, point(i64::MAX)).unwrap());
        assert!(!drain.observe(true, Some(report(0, 0, 0)), None).unwrap());
        assert!(!drain.observe(true, Some(report(10, 0, 0)), None).unwrap());
        assert!(
            drain
                .observe(
                    true,
                    Some(report(20, 0, 0)),
                    Some(ClockPoint {
                        domain: ClockDomainId(8),
                        timestamp: Timestamp::from_nanos(i64::MAX)
                    })
                )
                .is_err()
        );
        assert!(
            drain
                .observe(true, Some(report(u64::MAX, 0, 0)), point(0))
                .is_err()
        );
    }
    #[test]
    fn replay_completion_waits_for_records_bgm_and_actual_output_drain() {
        let mut completion = ReplayCompletion::new(ClockDomainId(7), 10);
        let clear = BgmFeedReport::default();
        assert!(
            !completion
                .observe(false, clear, Some(report(0, 0, 0)), point(i64::MAX))
                .unwrap()
        );
        for bgm in [
            BgmFeedReport {
                remaining: 1,
                ..clear
            },
            BgmFeedReport {
                outstanding: 1,
                ..clear
            },
        ] {
            assert!(
                !completion
                    .observe(true, bgm, Some(report(0, 0, 0)), point(i64::MAX))
                    .unwrap()
            );
        }
        assert!(
            !completion
                .observe(true, clear, Some(report(0, 0, 0)), point(i64::MAX))
                .unwrap()
        );
        assert!(
            !completion
                .observe(true, clear, Some(report(10, 1, 0)), point(i64::MAX))
                .unwrap()
        );
        assert!(
            !completion
                .observe(true, clear, Some(report(20, 0, 0)), None)
                .unwrap()
        );
        assert!(
            !completion
                .observe(true, clear, Some(report(30, 0, 0)), point(2_999_999_999))
                .unwrap()
        );
        assert!(
            completion
                .observe(true, clear, Some(report(40, 0, 0)), point(3_000_000_000))
                .unwrap()
        );
        assert!(
            ReplayCompletion::new(ClockDomainId(7), 0)
                .observe(true, clear, None, None)
                .is_err()
        );
        assert!(
            ReplayCompletion::new(ClockDomainId(7), 10)
                .observe(
                    true,
                    clear,
                    None,
                    Some(ClockPoint {
                        domain: ClockDomainId(8),
                        timestamp: Timestamp::ZERO
                    })
                )
                .is_err()
        );
    }
}
