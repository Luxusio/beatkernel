//! Exact target-time specialization of the existing output replacement transaction.
use super::*;
use crate::{
    gameplay::output::ports::OriginalTargetNativeOutputBackend,
    native_audio_presentation::{
        prepare_target_native_snapshot, PreparedTargetNativeAudioEpoch, TargetNativeAudioSnapshot,
    },
    native_end::NativeEnd,
};
use beatkernel::audio::{ConvertedRenderReport, RenderReport, TargetFrameBasis};
use beatkernel_platform::audio::{ConvertedBoundaryFacts, ConvertedNativeOutputState};

pub(super) struct TargetWaitingTiming {
    pause: NativePause,
    end: Option<NativeEnd>,
    original_end: Option<NativeEnd>,
    basis: TargetFrameBasis,
    next: AudioAuthorityEpoch,
    validator: NativePresentationValidator,
    snapshots: [Option<TargetNativeAudioSnapshot>; 2],
    pairs: [Option<beatkernel::time::ClockPair>; 2],
    source_rate: u32,
}
/// Complete ready native owner and the original producer hold; refusal returns both.
pub struct ReadyTargetAudioOutput<O> {
    pub output: O,
    pub pause: NativePause,
    end: Option<NativeEnd>,
    pub basis: TargetFrameBasis,
    pub playback_origin: ClockPoint,
    pub snapshots: [TargetNativeAudioSnapshot; 2],
    pub prepared: PreparedTargetNativeAudioEpoch,
    pub converted_report: ConvertedRenderReport,
    pub facts: ConvertedBoundaryFacts,
    pub source_report: Option<RenderReport>,
    pub hold: PauseHold,
    original_end: Option<NativeEnd>,
    source_rate: u32,
}
impl<O> ReadyTargetAudioOutput<O> {
    /// Staged immutable finite session; publication controls its transfer.
    pub fn end(&self) -> Option<&NativeEnd> {
        self.end.as_ref()
    }
}
impl<B: OriginalTargetNativeOutputBackend<ConvertedNativeOutputState>>
    OutputReplacement<B, ConvertedNativeOutputState, TargetFrameBasis>
{
    /// Stages exact timing/source identity while the full recovered owner is still cold.
    /// Retirement/open/start/failure ownership follows the same lifecycle as other outputs.
    pub fn begin_target_audio(
        &mut self,
        request: B::Request,
        current: &NativeAudioPresentation,
        pause: &NativePause,
        end: Option<&NativeEnd>,
        merger: &InputMerger,
        _original_song: Timestamp,
        wait_ns: u64,
        acquire: impl FnOnce() -> Result<PauseHold, PauseHoldError>,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        if merger.pending() != 0 {
            return Err(ReplacementFailure::policy(
                "target replacement requires drained paused input",
            ));
        }
        let epoch = current.authority().epoch();
        if pause.epoch() != epoch.id || current.target_basis().is_none() {
            return Err(ReplacementFailure::policy(
                "target replacement requires matching actual target epoch",
            ));
        }
        self.begin_lifecycle_with(
            request,
            epoch.id,
            pause,
            wait_ns,
            acquire,
            |backend, request, id, owner| {
                let basis = backend
                    .planned_target_basis(request, owner)
                    .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Open, error))?;
                let prepared = (|| -> Result<_, Box<dyn std::error::Error>> {
                    let mut candidate = pause.clone();
                    candidate.rebind_target_output_with_basis(id, owner, basis)?;
                    let playback_origin = basis.point_at_stream_frame(0)?;
                    let next = AudioAuthorityEpoch {
                        id,
                        stream_origin: playback_origin,
                        logical_origin: current
                            .authority()
                            .checked_logical_output(playback_origin)?,
                        host_domain: epoch.host_domain,
                    };
                    let staged_end = end
                        .map(|end| end.prepare_restart_for_target_output(id, owner, basis))
                        .transpose()?;
                    Ok(TargetWaitingTiming {
                        pause: candidate,
                        end: staged_end,
                        original_end: end.cloned(),
                        basis,
                        next,
                        validator: NativePresentationValidator::new(
                            id,
                            playback_origin,
                            epoch.host_domain,
                        ),
                        snapshots: [None; 2],
                        pairs: [None; 2],
                        source_rate: owner.mixer().config().format().sample_rate(),
                    })
                })()
                .map_err(ReplacementFailure::timing)?;
                Ok((WaitingTiming::Target(prepared), basis))
            },
        )
    }
    /// Requires two fresh original observations after retained output has drained.
    /// Earlier native positions are validated but never enter candidate anchors.
    pub fn poll_target_audio(
        &mut self,
        now: ClockPoint,
        current: &NativeAudioPresentation,
        merger: &InputMerger,
    ) -> Result<Option<ReadyTargetAudioOutput<B::Output>>, ReplacementFailure<B::Error>> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        let Slot::Waiting(mut waiting) = slot else {
            self.slot = slot;
            return Err(ReplacementFailure::policy(
                "target output replacement is not waiting",
            ));
        };
        let WaitingTiming::Target(timing) = &mut waiting.timing else {
            self.slot = Slot::Waiting(waiting);
            return Err(ReplacementFailure::policy(
                "target polling cannot consume other timing modes",
            ));
        };
        let result = (|| -> Result<Option<(PreparedTargetNativeAudioEpoch, ConvertedRenderReport, ConvertedBoundaryFacts, Option<RenderReport>)>, ReplacementFailure<B::Error>> {
            if now.domain != timing.next.host_domain || waiting.last_poll.is_some_and(|old| now.timestamp < old.timestamp) {
                return Err(ReplacementFailure::policy("target replacement poll clock changed or regressed"));
            }
            let first_poll = waiting.first_poll.get_or_insert(now);
            if i128::from(now.timestamp.as_nanos()) - i128::from(first_poll.timestamp.as_nanos()) >= i128::from(waiting.wait_ns) {
                return Err(ReplacementFailure::policy("target replacement observation timed out"));
            }
            waiting.last_poll = Some(now);
            if self.backend.epoch(&waiting.output) != timing.next.id || self.backend.basis(&waiting.output) != timing.basis {
                return Err(ReplacementFailure::policy("target output changed its original creation epoch or basis"));
            }
            let snapshot = self.backend.observe_native_target(&mut waiting.output)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))?;
            let telemetry = self.backend.output_telemetry(&waiting.output)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))?;
            if snapshot.is_some_and(|snapshot| snapshot.epoch != timing.next.id || snapshot.basis != timing.basis) {
                return Err(ReplacementFailure::policy("target snapshot differs from creation identity"));
            }
            let Some(telemetry) = telemetry else { return Ok(None); };
            let facts = telemetry.facts;
            let report = telemetry.converted;
            let source = telemetry.source;
            if facts.origin != Some(timing.basis.origin()) || facts.source_rate != timing.source_rate
                || report.is_some_and(|report| report.target_rate != timing.basis.sample_rate() || report.source_rate != timing.source_rate) {
                return Err(ReplacementFailure::policy("target replacement changed immutable source association"));
            }
            let Some(snapshot) = snapshot else { return Ok(None); };
            let prepared = prepare_target_native_snapshot(&timing.validator, snapshot).map_err(ReplacementFailure::timing)?;
            let Some(pair) = prepared.correlation_pair() else { return Ok(None); };
            if !timing.pause.target_replacement_observation_ready(report, pair)
                .map_err(|error| ReplacementFailure::timing(Box::new(error)))? {
                return Ok(None);
            }
            if let Some(end) = timing.end.as_ref() {
                if !end.target_replacement_observation_ready(report, pair)
                    .map_err(|error| ReplacementFailure::timing(Box::new(error)))? { return Ok(None); }
            }
            if pair.target.timestamp > now.timestamp { return Ok(None); }
            let report = report.expect("ready observation requires nonempty fresh held report");
            // All generation/hold checks precede mutation of native or pause anchors.
            let mut candidate_pause = timing.pause.clone();
            candidate_pause.observe_target(timing.next.id, timing.basis, facts, source, pair)
                .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
            if candidate_pause.phase() != PausePhase::Paused { return Err(ReplacementFailure::policy("target replacement changed frozen pause")); }
            let mut candidate_end = timing.end.clone();
            if let Some(end) = candidate_end.as_mut() {
                if end.observe_target(timing.next.id, timing.basis, facts, source, pair)
                    .map_err(|error| ReplacementFailure::timing(Box::new(error)))?.is_some() {
                    return Err(ReplacementFailure::policy("target replacement reached finite endpoint"));
                }
            }
            timing.validator.commit(prepared).map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
            timing.pause = candidate_pause; timing.end = candidate_end;
            let previous = timing.pairs[1].or(timing.pairs[0]);
            if previous.is_none_or(|old| pair.source.timestamp > old.source.timestamp && pair.target.timestamp > old.target.timestamp) {
                if timing.pairs[0].is_none() { timing.pairs[0] = Some(pair); timing.snapshots[0] = Some(snapshot); }
                else { if timing.pairs[1].is_some() { timing.pairs[0] = timing.pairs[1]; timing.snapshots[0] = timing.snapshots[1]; }
                    timing.pairs[1] = Some(pair); timing.snapshots[1] = Some(snapshot); }
            }
            let [Some(first), Some(second)] = timing.pairs else { return Ok(None); };
            if merger.pending() != 0 || i128::from(now.timestamp.as_nanos()) - i128::from(second.target.timestamp.as_nanos())
                > i128::from(current.authority().config().max_observation_age.as_nanos())
                || [current.authority().closed_host_prefix(), current.authority().committed_input_host(),
                    current.authority().latest_observation().map(|pair| pair.target)]
                    .into_iter().flatten().any(|host| second.target.timestamp < host.timestamp) {
                return Ok(None);
            }
            let first_output = current.authority().checked_logical_output(first.source)
                .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
            if current.authority().committed_presentation().is_some_and(|old| first_output.timestamp < old.timestamp) { return Ok(None); }
            let snapshots = [timing.snapshots[0].expect("first pair"), timing.snapshots[1].expect("second pair")];
            let token = current.prepare_target_output_epoch(timing.next, timing.basis, snapshots, now, merger)
                .map_err(ReplacementFailure::timing)?;
            Ok(Some((token, report, facts, source)))
        })();
        match result {
            Ok(None) => {
                self.slot = Slot::Waiting(waiting);
                Ok(None)
            }
            Ok(Some((prepared, converted_report, facts, source_report))) => {
                let WaitingTiming::Target(timing) = waiting.timing else {
                    unreachable!("target checked");
                };
                Ok(Some(ReadyTargetAudioOutput {
                    output: waiting.output,
                    pause: timing.pause,
                    end: timing.end,
                    original_end: timing.original_end,
                    basis: timing.basis,
                    playback_origin: timing.next.stream_origin,
                    snapshots: [
                        timing.snapshots[0].expect("first"),
                        timing.snapshots[1].expect("second"),
                    ],
                    prepared,
                    converted_report,
                    facts,
                    source_report,
                    source_rate: timing.source_rate,
                    hold: waiting.hold,
                }))
            }
            Err(mut failure) => {
                let (cleanup, recovery) = self.cleanup_output(waiting.output);
                self.retain_retirement_hold(waiting.hold);
                failure.cleanup = cleanup;
                failure.recovery = recovery;
                Err(failure)
            }
        }
    }
}
/// Publication refusal retains the exact ready owner and its exclusive pause hold.
pub struct ReadyTargetAudioPublicationFailure<O> {
    pub error: Box<dyn std::error::Error>,
    pub ready: ReadyTargetAudioOutput<O>,
}
/// Publishes after all staged checks and releases the hold normally.
pub fn publish_ready_target_audio_output<O>(
    ready: ReadyTargetAudioOutput<O>,
    output: &mut Option<O>,
    context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
    now: ClockPoint,
) -> Result<(), ReadyTargetAudioPublicationFailure<O>> {
    publish_ready_target_audio_output_held(ready, output, context, now).map(drop)
}
/// Single existing audio authority transaction; no native callback follows commit.
pub fn publish_ready_target_audio_output_held<O>(
    ready: ReadyTargetAudioOutput<O>,
    output: &mut Option<O>,
    context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
    now: ClockPoint,
) -> Result<PauseHold, ReadyTargetAudioPublicationFailure<O>> {
    let staged = (|| -> Result<(), Box<dyn std::error::Error>> {
        let old_epoch = context.presentation.authority().epoch();
        if output.is_some()
            || old_epoch.id != context.pause.epoch()
            || ready.pause.epoch() != ready.prepared.epoch()
            || ready.pause.epoch() <= old_epoch.id
            || ready.prepared.basis() != ready.basis
            || ready.prepared.snapshots() != ready.snapshots
            || ready.source_rate != context.config.sample_rate
            || ready.facts.source_rate != ready.source_rate
            || ready.facts.origin != Some(ready.basis.origin())
            || ready.basis.origin().domain != context.config.playback_origin.domain
            || ready.pause.host_domain() != context.config.origin.domain
            || old_epoch.host_domain != context.config.origin.domain
            || ready.playback_origin != ready.basis.point_at_stream_frame(0)?
            || !context.config.logical_schedule
            || ready.end.is_some() != ready.original_end.is_some()
            || context.end.as_ref() != ready.original_end.as_ref()
        {
            return Err("target replacement changed publication/source identity".into());
        }
        // Empty queues still belong to one host domain. Validate the actual
        // publication frontier without consuming input or changing its owner.
        context.merger.peek_ready(now)?;
        context
            .presentation
            .validate_target_output_epoch(&ready.prepared, now, context.merger)?;
        let pair = ready.prepared.latest_record().pair();
        context.pause.validate_target_replacement(
            &ready.pause,
            ready.basis,
            ready.converted_report,
            pair,
        )?;
        if let Some(end) = ready.end.as_ref() {
            if !end.target_replacement_observation_ready(Some(ready.converted_report), pair)? {
                return Err("target finite replacement lacks fresh native publication".into());
            }
        }
        Ok(())
    })();
    if let Err(error) = staged {
        return Err(ReadyTargetAudioPublicationFailure { error, ready });
    }
    if let Err(error) =
        context
            .presentation
            .commit_target_output_epoch(ready.prepared.clone(), now, context.merger)
    {
        return Err(ReadyTargetAudioPublicationFailure { error, ready });
    }
    let ReadyTargetAudioOutput {
        output: candidate,
        pause,
        end,
        playback_origin,
        hold,
        ..
    } = ready;
    *output = Some(candidate);
    *context.pause = pause;
    *context.end = end;
    // These origins describe raw native presentation. Source rate, scheduling
    // origin, BGM timestamps, Runtime and queued commands remain the same owners.
    context.config.stream_origin = playback_origin;
    context.config.playback_origin = playback_origin;
    Ok(hold)
}
#[cfg(test)]
#[path = "target_replacement_fixtures.rs"]
mod fixtures;
