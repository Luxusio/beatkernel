//! Finite replay completion from actual core and native presentation evidence.
use crate::{
    bgm::BgmFeeder, completion::CompletionError, replay_audio::completed_render_cursor_for_feeder,
    step_gameplay::counter_values,
};
use beatkernel::{
    audio::{AudioLimits, RenderReport},
    time::{ClockPoint, Timestamp},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiniteReplayCompletion {
    origin: ClockPoint,
    rate: u32,
    endpoint: u64,
    last_render: Option<RenderReport>,
    last_presented: Option<Timestamp>,
}
impl FiniteReplayCompletion {
    pub fn new(origin: ClockPoint, rate: u32, endpoint: u64) -> Result<Self, CompletionError> {
        if rate == 0 {
            return Err(CompletionError("finite replay rate must be positive"));
        }
        let value = Self {
            origin,
            rate,
            endpoint,
            last_render: None,
            last_presented: None,
        };
        value.point(endpoint)?;
        Ok(value)
    }
    fn point(&self, frame: u64) -> Result<Timestamp, CompletionError> {
        let ns =
            (i128::from(frame) * 1_000_000_000 + i128::from(self.rate) - 1) / i128::from(self.rate);
        let ns = i128::from(self.origin.timestamp.as_nanos()) + ns;
        Ok(Timestamp::from_nanos(i64::try_from(ns).map_err(|_| {
            CompletionError("finite replay physical clock overflow")
        })?))
    }
    /// Refusal is atomic. Feeder callbacks must be the actual retained producer's.
    /// Missing evidence waits; it never substitutes a scheduling/wall clock.
    pub fn observe(
        &mut self,
        records_finished: bool,
        feeder: &BgmFeeder,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<bool, CompletionError> {
        if let Some(point) = presented {
            if point.domain != self.origin.domain
                || point.timestamp < self.origin.timestamp
                || self.last_presented.is_some_and(|old| point.timestamp < old)
            {
                return Err(CompletionError(
                    "finite replay presentation domain or chronology differs",
                ));
            }
        }
        if let Some(report) = rendered {
            if report.frames == 0
                || report.frames > AudioLimits::MAX_RENDER_FRAMES
                || report.active_voices > AudioLimits::MAX_VOICES
                || report.pending_commands > AudioLimits::MAX_COMMANDS
                || report.producer_disconnected
            {
                return Err(CompletionError(
                    "finite replay render capacity or producer differs",
                ));
            }
            let physical = completed_render_cursor_for_feeder(&report, feeder)
                .map_err(|_| CompletionError("finite replay core execution failed"))?;
            let playback = report
                .playback_start_frame
                .checked_add(report.playback_frames as u64)
                .ok_or(CompletionError("finite replay playback extent overflow"))?;
            if report.playback_start_frame > report.start_frame
                || report.playback_frames > report.frames
                || playback > self.endpoint
                || report.counters.rendered_frames != physical
                || report.counters.commands_applied > report.counters.commands_consumed
                || (!report.paused && report.playback_frames != report.frames)
            {
                return Err(CompletionError(
                    "finite replay output grid or counters differ",
                ));
            }
            self.point(physical)?;
            match report.playback_end_physical_frame {
                Some(marker)
                    if report.paused
                        && playback == self.endpoint
                        && marker >= self.endpoint
                        && marker <= physical
                        && (if report.playback_frames == 0 {
                            marker <= report.start_frame
                        } else {
                            report
                                .start_frame
                                .checked_add(report.playback_frames as u64)
                                == Some(marker)
                        }) =>
                {
                    self.point(marker)?;
                }
                None if playback < self.endpoint => {}
                _ => return Err(CompletionError("finite replay endpoint evidence differs")),
            }
            if let Some(old) = self.last_render {
                let old_playback = old
                    .playback_start_frame
                    .checked_add(old.playback_frames as u64)
                    .ok_or(CompletionError("previous playback extent overflow"))?;
                let gap = report.start_frame - report.playback_start_frame;
                let old_gap = old.start_frame - old.playback_start_frame;
                if (report.start_frame == old.start_frame && report != old)
                    || (report.start_frame != old.start_frame
                        && report.start_frame < old.counters.rendered_frames)
                    || playback < old_playback
                    || gap < old_gap
                    || counter_values(report.counters)
                        .into_iter()
                        .zip(counter_values(old.counters))
                        .any(|(new, old)| new < old)
                {
                    return Err(CompletionError(
                        "finite replay render or counters regressed",
                    ));
                }
                if old.playback_end_physical_frame.is_some()
                    && (report.playback_end_physical_frame != old.playback_end_physical_frame
                        || report.song_position != old.song_position
                        || report.active_voices != old.active_voices
                        || report.pending_commands != old.pending_commands
                        || counter_values(report.counters)[1..]
                            != counter_values(old.counters)[1..])
                {
                    return Err(CompletionError(
                        "finite replay state changed after endpoint",
                    ));
                }
            }
        }
        let report = rendered.or(self.last_render);
        if let (Some(point), Some(report)) = (presented, report) {
            if point.timestamp > self.point(report.counters.rendered_frames)? {
                return Err(CompletionError(
                    "finite replay presentation exceeds completed rendering",
                ));
            }
        }
        let feed = feeder.report();
        let ready = feed.remaining == 0 && feed.outstanding == 0;
        let marker = report.and_then(|report| report.playback_end_physical_frame);
        if feed.remaining == 0 && marker.is_some() {
            let report = report.expect("marker belongs to report");
            let admitted = u64::try_from(feed.total_admitted)
                .map_err(|_| CompletionError("finite replay admission count overflow"))?;
            if report.counters.commands_consumed != admitted
                || report.counters.commands_applied != admitted
            {
                return Err(CompletionError(
                    "finite replay endpoint omitted or added planned execution",
                ));
            }
        }
        let physical_presented = presented
            .map(|point| point.timestamp)
            .or(self.last_presented);
        let complete = records_finished
            && ready
            && marker
                .map(|marker| self.point(marker))
                .transpose()?
                .is_some_and(|target| physical_presented.is_some_and(|point| point >= target));
        // All validation precedes state adoption.
        if let Some(report) = rendered {
            self.last_render = Some(report);
        }
        if let Some(point) = presented.filter(|_| report.is_some()) {
            self.last_presented = Some(point.timestamp);
        }
        Ok(complete)
    }
}
#[cfg(test)]
#[path = "finite_replay_completion_fixtures.rs"]
mod fixtures;
