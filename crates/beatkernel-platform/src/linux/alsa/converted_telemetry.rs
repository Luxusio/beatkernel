//! Coherent fixed atomic converted-output facts, with bounded nonblocking reads.
use super::*;
use crate::audio::{
    source_playback_publication::SourcePlaybackPublication,
    telemetry::Telemetry,
    history_retirement_mailbox::{
        ConvertedHistoryRetirementMailbox, ConvertedHistoryRetirementRequest,
    },
    ConvertedBoundaryFacts, ConvertedNativeOutputState,
};
use beatkernel::audio::{
    ConvertedOutputState, ConvertedPhaseHistory, ConvertedPhaseSpan, ConvertedRenderReport,
    InterpolatedTargetTime, SourcePosition, SourcePlaybackSpan, TargetBoundary, TargetTime,
    TargetFrameBasis,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Coherent generated correspondence qualified by this stream's positive
/// software submission prefix. These facts do not authenticate native
/// presentation, an output epoch, or a mixer pause acknowledgment.
#[derive(Clone, Copy, Debug)]
pub struct ConvertedPhaseSnapshot {
    /// Actual generated source-to-target correspondence selected for the query.
    pub span: ConvertedPhaseSpan,
    /// Exclusive transition to the next span, or the current generated endpoint.
    pub segment_end: TargetTime,
    /// Inclusive endpoint of the actual generated target output history.
    pub generated_end: TargetTime,
    /// Immutable target-time origin of this native stream's submission prefix.
    pub stream_start: TargetTime,
    /// Endpoint of positively admitted target output in this stream.
    pub submitted_end: TargetTime,
    /// Inner converted publication revision, distinct from outer native timing.
    pub revision: u64,
    /// Original converted render report from this publication, when available.
    pub report: Option<ConvertedRenderReport>,
    /// Persisted projections from actual generated source adoption boundaries.
    pub boundaries: ConvertedBoundaryFacts,
    /// Last successful nonempty source render, without synthetic held reports.
    pub last_real_source: Option<RenderReport>,
}

/// Actual source playback adoption and converted output correspondence from one
/// publication revision. Native presentation and pause ACK remain unproven.
#[derive(Clone, Copy, Debug)]
pub struct ConvertedPlaybackPhaseSnapshot {
    /// Generated correspondence qualified by this stream's submitted prefix.
    pub phase: ConvertedPhaseSnapshot,
    /// Actual source playback interval selected before nanosecond flooring.
    pub playback: SourcePlaybackSpan,
    /// Inclusive endpoint of actual successful source pulls.
    pub pulled_source_end: u64,
}

const GENERATED: usize = 8;
const STREAM: usize = 11;
const HAS_SUBMITTED: usize = 14;
const SUBMITTED: usize = 15;

struct PhasePublication {
    slots: Vec<[AtomicU64; 8]>,
    // Count, source Hz, initial target/source, generated end, stream lower,
    // submitted presence and submitted end. Only real spans occupy slots.
    values: [AtomicU64; 18],
    // Mathematical owner retirement nonce, inside the existing generation.
    retirement_revision: AtomicU64,
    available: AtomicBool,
}

impl PhasePublication {
    fn publish(&self, history: Option<&ConvertedPhaseHistory>) {
        let valid = (|| {
            let history = history?;
            let old: [u64; 18] =
                std::array::from_fn(|index| self.values[index].load(Ordering::SeqCst));
            let old_count = usize::try_from(old[0]).ok()?;
            let old_retirement = self.retirement_revision.load(Ordering::SeqCst);
            let retirement = history.retirement_revision();
            let retired = retirement > old_retirement;
            let initial_time = decode_time(&old[2..5])?;
            let initial_source = decode_position(&old[5..8]);
            if history.capacity() != self.slots.len()
                || u64::from(history.source_rate()) != old[1]
                || history.start_time() != initial_time
                || history.initial_source_position() != initial_source
                || compare_time(
                    history.end_time(),
                    decode_time(&old[GENERATED..GENERATED + 3])?,
                )
                .is_lt()
                || retirement < old_retirement
                || old_count > self.slots.len()
                || history.len() > self.slots.len()
                || (!retired
                    && (history.len() < old_count || history.len() > old_count.saturating_add(1)))
            {
                return None;
            }
            if retired {
                let previous_start = if old_count == 0 {
                    initial_time
                } else {
                    let words =
                        std::array::from_fn(|index| self.slots[0][index].load(Ordering::SeqCst));
                    span_start(ConvertedPhaseSpan::from_words(words).ok()?)?
                };
                if !compare_time(span_start(history.span(0)?)?, previous_start).is_gt() {
                    return None;
                }
            }
            let first_changed = if retired { 0 } else { old_count };
            // Validate the entire replacement before changing any published slot.
            for index in first_changed..history.len() {
                let span = history.span(index)?;
                let words = span.to_words();
                ConvertedPhaseSpan::from_words(words).ok()?;
                let start = span_start(span)?;
                let source = decode_position(&words[1..4]);
                if words[0] != old[1]
                    || compare_time(start, initial_time).is_lt()
                    || !compare_time(start, history.end_time()).is_lt()
                    || compare_position(source, initial_source).is_lt()
                    || (index == 0
                        && retirement == 0
                        && (start != initial_time || source != initial_source))
                {
                    return None;
                }
                if index != 0 {
                    let previous = history.span(index - 1)?;
                    if !compare_time(span_start(previous)?, start).is_lt()
                        || compare_position(source, decode_position(&previous.to_words()[1..4]))
                            .is_lt()
                    {
                        return None;
                    }
                }
            }
            for index in first_changed..history.len() {
                for (field, word) in self.slots[index]
                    .iter()
                    .zip(history.span(index)?.to_words())
                {
                    field.store(word, Ordering::SeqCst);
                }
            }
            self.values[0].store(history.len() as u64, Ordering::SeqCst);
            let mut words = [0; 3];
            encode_time(history.end_time(), &mut words);
            for (field, word) in self.values[GENERATED..GENERATED + 3].iter().zip(words) {
                field.store(word, Ordering::SeqCst);
            }
            self.retirement_revision.store(retirement, Ordering::SeqCst);
            Some(())
        })();
        if valid.is_none() {
            self.available.store(false, Ordering::SeqCst);
        }
    }
}

pub(super) struct ConvertedTelemetry {
    generation: AtomicU64,
    held: AtomicBool,
    values: [AtomicU64; 58],
    source: Telemetry,
    real_source: Telemetry,
    phase: Option<PhasePublication>,
    playback: Option<SourcePlaybackPublication>,
    retirement: Option<ConvertedHistoryRetirementMailbox>,
}
impl ConvertedTelemetry {
    pub(super) fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            held: AtomicBool::new(false),
            values: std::array::from_fn(|_| AtomicU64::new(0)),
            source: Telemetry::new(),
            real_source: Telemetry::new(),
            phase: None,
            playback: None,
            retirement: None,
        }
    }
    pub(super) fn with_phase_history(
        state: &ConvertedNativeOutputState,
        basis: TargetFrameBasis,
    ) -> Result<Self, AudioError> {
        let mut telemetry = Self::new();
        telemetry.seed(state.boundaries(), state.last_real_source_report());
        if let Some(history) = state.phase_history() {
            let mut slots = Vec::new();
            slots
                .try_reserve_exact(history.capacity())
                .map_err(|_| AudioError::AllocationFailed)?;
            for index in 0..history.capacity() {
                let words = history.span(index).map_or([0; 8], |span| span.to_words());
                slots.push(words.map(AtomicU64::new));
            }
            let mut values = [0; 18];
            values[0] = history.len() as u64;
            values[1] = u64::from(history.source_rate());
            encode_time(history.start_time(), &mut values[2..5]);
            encode_position(history.initial_source_position(), &mut values[5..8]);
            encode_time(history.end_time(), &mut values[GENERATED..GENERATED + 3]);
            encode_time(basis.start_time(), &mut values[STREAM..STREAM + 3]);
            telemetry.phase = Some(PhasePublication {
                slots,
                values: values.map(AtomicU64::new),
                retirement_revision: AtomicU64::new(history.retirement_revision()),
                available: AtomicBool::new(true),
            });
        }
        if telemetry.phase.is_some() {
            if let Some(history) = state.converter_owner().source_playback_history() {
                telemetry.playback = Some(SourcePlaybackPublication::new(history)?);
                telemetry.retirement = Some(ConvertedHistoryRetirementMailbox::new(
                    basis,
                    history,
                    state.phase_history().ok_or(AudioError::InvalidCapacity)?,
                )?);
            }
        }
        Ok(telemetry)
    }
    /// Queues only mathematical pins qualified against one current publication.
    /// The returned revision grants neither native acknowledgment nor completion.
    pub(super) fn request_history_retirement(
        &self,
        expected_basis: TargetFrameBasis,
        source_identity: (u32, u64, u64),
        source_pin: u64,
        target_pin: TargetTime,
    ) -> Result<Option<u64>, AudioError> {
        let mailbox = self
            .retirement
            .as_ref()
            .ok_or(AudioError::InvalidCapacity)?;
        mailbox.validate_identity(expected_basis, source_identity)?;
        for _ in 0..3 {
            let before = self.generation.load(Ordering::SeqCst);
            if before % 2 != 0 {
                continue;
            }
            let valid = self.playback.as_ref().is_some_and(|playback| {
                playback.contains_retirement_pin(source_identity, source_pin)
            }) && self.contains_phase_retirement_pin(target_pin);
            if before != self.generation.load(Ordering::SeqCst) {
                continue;
            }
            if !valid {
                return Err(AudioError::InvalidFormat);
            }
            return mailbox.request(expected_basis, source_identity, source_pin, target_pin);
        }
        Ok(None)
    }
    pub(super) fn retirement_request(&self) -> Option<ConvertedHistoryRetirementRequest> {
        self.retirement.as_ref()?.latest()
    }
    fn contains_phase_retirement_pin(&self, pin: TargetTime) -> bool {
        let Some(phase) = &self.phase else {
            return false;
        };
        if !phase.available.load(Ordering::SeqCst) {
            return false;
        }
        let header: [u64; 18] =
            std::array::from_fn(|index| phase.values[index].load(Ordering::SeqCst));
        let Some(count) = usize::try_from(header[0])
            .ok()
            .filter(|count| *count <= phase.slots.len())
        else {
            return false;
        };
        let Some(initial) = decode_time(&header[2..5]) else {
            return false;
        };
        let Some(end) = decode_time(&header[GENERATED..GENERATED + 3]) else {
            return false;
        };
        if header[1] == 0
            || u32::try_from(header[1]).is_err()
            || header[7] == 0
            || header[6] >= header[7]
            || compare_time(end, initial).is_lt()
        {
            return false;
        }
        let retirement = phase.retirement_revision.load(Ordering::SeqCst);
        if count == 0 {
            return retirement == 0
                && compare_time(initial, end).is_eq()
                && compare_time(pin, initial).is_eq();
        }
        let words = std::array::from_fn(|index| phase.slots[0][index].load(Ordering::SeqCst));
        let Ok(first) = ConvertedPhaseSpan::from_words(words) else {
            return false;
        };
        let Some(start) = span_start(first) else {
            return false;
        };
        words[0] == header[1]
            && !compare_time(start, initial).is_lt()
            && compare_time(start, end).is_lt()
            && !compare_position(
                decode_position(&words[1..4]),
                decode_position(&header[5..8]),
            )
            .is_lt()
            && (retirement != 0 || (start == initial && words[1..4] == header[5..8]))
            && !compare_time(pin, start).is_lt()
            && !compare_time(pin, end).is_gt()
    }
    pub(super) fn phase_enabled(&self) -> bool {
        self.phase.is_some()
    }
    pub(super) fn set_held(&self, held: bool) {
        self.held.store(held, Ordering::Release);
    }
    pub(super) fn held(&self) -> bool {
        self.held.load(Ordering::Acquire)
    }
    pub(super) fn seed(&self, facts: ConvertedBoundaryFacts, source: Option<RenderReport>) {
        self.store(None, facts, source);
    }
    pub(super) fn publish(
        &self,
        report: ConvertedRenderReport,
        facts: ConvertedBoundaryFacts,
        real_source: Option<RenderReport>,
    ) {
        self.store(Some(report), facts, real_source);
    }
    fn store(
        &self,
        report: Option<ConvertedRenderReport>,
        facts: ConvertedBoundaryFacts,
        real_source: Option<RenderReport>,
    ) {
        let Some((version, next)) = self.begin_write() else {
            return;
        };
        self.store_payload(report, facts, real_source, version);
        self.generation.store(next, Ordering::SeqCst);
    }
    fn begin_write(&self) -> Option<(u64, u64)> {
        let version = self.generation.load(Ordering::SeqCst);
        let next = version.checked_add(2).filter(|_| version % 2 == 0);
        let Some(next) = next else {
            self.generation.store(u64::MAX, Ordering::SeqCst);
            return None;
        };
        self.generation.store(next - 1, Ordering::SeqCst);
        Some((version, next))
    }
    fn store_payload(
        &self,
        report: Option<ConvertedRenderReport>,
        facts: ConvertedBoundaryFacts,
        real_source: Option<RenderReport>,
        version: u64,
    ) {
        let mut values = [0; 58];
        values[0] = u64::from(report.is_some());
        if let Some(report) = report {
            values[1] = report.target_frames as u64;
            values[2] = report.target_frame_cursor;
            values[3] = report.pulled_source_frame_cursor;
            values[4] = u64::from(report.state == ConvertedOutputState::Held);
            values[5] = u64::from(report.target_rate);
            values[6] = u64::from(report.source_rate);
            encode_time(report.target_start_time, &mut values[7..10]);
            encode_time(report.target_end_time, &mut values[10..13]);
            encode_position(report.source_start_position, &mut values[13..16]);
            encode_position(report.source_position, &mut values[16..19]);
            values[19] = u64::from(report.startup_source_frame.is_some());
            values[20] = report.startup_source_frame.unwrap_or(0);
            values[21] = u64::from(report.pause_source_frame.is_some());
            values[22] = report.pause_source_frame.unwrap_or(0);
            values[45] = u64::from(report.resume_source_frame.is_some());
            values[46] = report.resume_source_frame.unwrap_or(0);
        }
        encode_boundary(facts.startup, &mut values[23..29]);
        encode_boundary(facts.pause, &mut values[29..35]);
        encode_boundary(facts.end, &mut values[35..41]);
        values[41] = u64::from(facts.origin.is_some());
        values[42] = facts.origin.map_or(0, |origin| u64::from(origin.domain.0));
        values[43] = facts
            .origin
            .map_or(0, |origin| origin.timestamp.as_nanos() as u64);
        values[44] = u64::from(facts.source_rate);
        encode_boundary(facts.resume, &mut values[47..53]);
        for (field, value) in self.values.iter().zip(values) {
            field.store(value, Ordering::SeqCst);
        }
        let mut source_version = version;
        let mut real_version = version;
        self.source.publish(
            snapshot(report.and_then(|report| report.source)),
            &mut source_version,
        );
        self.real_source
            .publish(snapshot(real_source), &mut real_version);
    }
    /// One writer publishes actual generation and associated source facts together.
    /// On a post-generation error the old report remains, while the committed
    /// converter extent is still made visible.
    pub(super) fn publish_generated(
        &self,
        result: Option<ConvertedRenderReport>,
        state: &ConvertedNativeOutputState,
    ) {
        // A capacity/preflight refusal commits nothing. Publish an error only
        // when the converter actually advanced before a later report error.
        if result.is_none() {
            if let Some((phase, history)) = self.phase.as_ref().zip(state.phase_history()) {
                let end_words: [u64; 3] = std::array::from_fn(|index| {
                    phase.values[GENERATED + index].load(Ordering::SeqCst)
                });
                if phase.values[0].load(Ordering::SeqCst) == history.len() as u64
                    && decode_time(&end_words) == Some(history.end_time())
                    && phase.retirement_revision.load(Ordering::SeqCst)
                        == history.retirement_revision()
                {
                    let source_unchanged = match (
                        &self.playback,
                        state.converter_owner().source_playback_history(),
                    ) {
                        (Some(publication), Some(history)) => publication.unchanged(history),
                        (None, _) => true,
                        _ => false,
                    };
                    if source_unchanged {
                        return;
                    }
                }
            }
        }
        let Some((version, next)) = self.begin_write() else {
            return;
        };
        if let Some(report) = result {
            self.store_payload(
                Some(report),
                state.boundaries(),
                state.last_real_source_report(),
                version,
            );
        }
        if let Some(phase) = &self.phase {
            phase.publish(state.phase_history());
        }
        self.publish_source_history(state);
        self.generation.store(next, Ordering::SeqCst);
    }
    fn publish_source_history(&self, state: &ConvertedNativeOutputState) {
        if let Some(publication) = &self.playback {
            publication.publish(state.converter_owner().source_playback_history());
        }
    }
    /// Called immediately after an actual positive PCM admission, before any
    /// fallible timing query. Publication cannot roll back that admission.
    pub(super) fn publish_submitted(&self, time: TargetTime) {
        let Some(phase) = &self.phase else {
            return;
        };
        let Some((_, next)) = self.begin_write() else {
            return;
        };
        let generated = decode_time(&std::array::from_fn::<_, 3, _>(|index| {
            phase.values[index + GENERATED].load(Ordering::SeqCst)
        }));
        let stream_start = decode_time(&std::array::from_fn::<_, 3, _>(|index| {
            phase.values[index + STREAM].load(Ordering::SeqCst)
        }));
        let previous = if phase.values[HAS_SUBMITTED].load(Ordering::SeqCst) == 1 {
            decode_time(&std::array::from_fn::<_, 3, _>(|index| {
                phase.values[index + SUBMITTED].load(Ordering::SeqCst)
            }))
        } else {
            stream_start
        };
        if generated.zip(previous).is_some_and(|(end, before)| {
            !compare_time(time, end).is_gt() && compare_time(time, before).is_gt()
        }) {
            let mut words = [0; 3];
            encode_time(time, &mut words);
            for (field, word) in phase.values[SUBMITTED..SUBMITTED + 3].iter().zip(words) {
                field.store(word, Ordering::SeqCst);
            }
            phase.values[HAS_SUBMITTED].store(1, Ordering::SeqCst);
        } else {
            phase.available.store(false, Ordering::SeqCst);
        }
        self.generation.store(next, Ordering::SeqCst);
    }
    fn load(&self) -> Option<([u64; 58], Option<RenderReport>, Option<RenderReport>)> {
        for _ in 0..3 {
            let before = self.generation.load(Ordering::SeqCst);
            if before % 2 != 0 {
                continue;
            }
            let values = std::array::from_fn(|index| self.values[index].load(Ordering::SeqCst));
            let source = self.source.read().render;
            let real = self.real_source.read().render;
            if before == self.generation.load(Ordering::SeqCst) {
                return Some((values, source, real));
            }
        }
        None
    }
    /// Source, target report and boundary association from one bounded load.
    pub(super) fn output_telemetry(
        &self,
    ) -> Option<(
        Option<RenderReport>,
        Option<ConvertedRenderReport>,
        ConvertedBoundaryFacts,
    )> {
        let (values, source, real) = self.load()?;
        decode_output(values, source, real)
    }
    pub(super) fn read(&self) -> Option<(ConvertedRenderReport, ConvertedBoundaryFacts)> {
        let (_, report, facts) = self.output_telemetry()?;
        Some((report?, facts))
    }
    pub(super) fn boundaries(&self) -> ConvertedBoundaryFacts {
        self.load()
            .and_then(|(values, _, _)| decode_facts(&values))
            .unwrap_or_default()
    }
    pub(super) fn last_real_source_report(&self) -> Option<RenderReport> {
        self.load()?.2
    }
    /// Bounded exact lookup inside both generated history and this stream's
    /// positively submitted prefix. No rounded-clock inverse is performed.
    pub(super) fn phase_snapshot_at(&self, query: TargetTime) -> Option<ConvertedPhaseSnapshot> {
        self.phase_snapshot_with_comparator(|bound| compare_time(query, bound), Some)
    }
    pub(super) fn phase_snapshot_at_interpolated_time(
        &self,
        query: &InterpolatedTargetTime,
    ) -> Option<ConvertedPhaseSnapshot> {
        self.phase_snapshot_with_comparator(|bound| query.compare_target_time(bound), Some)
    }
    pub(super) fn playback_phase_snapshot_at_interpolated_time(
        &self,
        query: &InterpolatedTargetTime,
    ) -> Option<ConvertedPlaybackPhaseSnapshot> {
        self.phase_snapshot_with_comparator(
            |bound| query.compare_target_time(bound),
            |phase| self.finish_playback_snapshot(phase, query),
        )
    }
    pub(super) fn playback_phase_snapshot_at_interpolated_time_with_identity(
        &self,
        query: &InterpolatedTargetTime,
        source_rate: u32,
        initial_physical: u64,
        initial_playback: u64,
    ) -> Option<ConvertedPlaybackPhaseSnapshot> {
        self.phase_snapshot_with_comparator(
            |bound| query.compare_target_time(bound),
            |phase| {
                self.finish_playback_snapshot_with_identity(
                    phase,
                    query,
                    source_rate,
                    initial_physical,
                    initial_playback,
                )
            },
        )
    }
    fn finish_playback_snapshot(
        &self,
        phase: ConvertedPhaseSnapshot,
        query: &InterpolatedTargetTime,
    ) -> Option<ConvertedPlaybackPhaseSnapshot> {
        let publication = self.playback.as_ref()?;
        let phase_publication = self.phase.as_ref()?;
        let initial_source = phase_publication.values[5].load(Ordering::SeqCst);
        if phase_publication.values[6].load(Ordering::SeqCst) != 0 {
            return None;
        }
        self.finish_playback_snapshot_with_identity(
            phase,
            query,
            u32::try_from(phase.span.to_words()[0]).ok()?,
            initial_source,
            publication.initial_playback_frame(),
        )
    }
    fn finish_playback_snapshot_with_identity(
        &self,
        phase: ConvertedPhaseSnapshot,
        query: &InterpolatedTargetTime,
        source_rate: u32,
        initial_physical: u64,
        initial_playback: u64,
    ) -> Option<ConvertedPlaybackPhaseSnapshot> {
        if u32::try_from(phase.span.to_words()[0]).ok()? != source_rate {
            return None;
        }
        let selected = self.playback.as_ref()?.select(
            source_rate,
            initial_physical,
            initial_playback,
            |frame| {
                phase
                    .span
                    .compare_source_frame_at_interpolated_time(*query, phase.segment_end, frame)
                    .ok()
            },
        )?;
        if phase
            .report
            .is_some_and(|report| report.pulled_source_frame_cursor > selected.pulled_end_frame)
        {
            return None;
        }
        Some(ConvertedPlaybackPhaseSnapshot {
            phase,
            playback: selected.span,
            pulled_source_end: selected.pulled_end_frame,
        })
    }
    fn phase_snapshot_with_comparator<R>(
        &self,
        compare_query: impl Fn(TargetTime) -> std::cmp::Ordering,
        finish: impl Fn(ConvertedPhaseSnapshot) -> Option<R>,
    ) -> Option<R> {
        let phase = self.phase.as_ref()?;
        for _ in 0..3 {
            let before = self.generation.load(Ordering::SeqCst);
            if before % 2 != 0 {
                continue;
            }
            let result = (|| {
                if !phase.available.load(Ordering::SeqCst) {
                    return None;
                }
                let header: [u64; 18] =
                    std::array::from_fn(|index| phase.values[index].load(Ordering::SeqCst));
                let retirement = phase.retirement_revision.load(Ordering::SeqCst);
                let count = usize::try_from(header[0]).ok()?;
                if count == 0 || count > phase.slots.len() || header[HAS_SUBMITTED] != 1 {
                    return None;
                }
                let initial_time = decode_time(&header[2..5])?;
                if header[1] == 0
                    || u32::try_from(header[1]).is_err()
                    || header[7] == 0
                    || header[6] >= header[7]
                {
                    return None;
                }
                let generated_end = decode_time(&header[GENERATED..GENERATED + 3])?;
                let stream_start = decode_time(&header[STREAM..STREAM + 3])?;
                let submitted_end = decode_time(&header[SUBMITTED..SUBMITTED + 3])?;
                if compare_query(initial_time).is_lt()
                    || compare_time(stream_start, initial_time).is_lt()
                    || compare_time(submitted_end, stream_start).is_lt()
                    || compare_time(submitted_end, generated_end).is_gt()
                    || compare_query(stream_start).is_lt()
                    || compare_query(submitted_end).is_gt()
                {
                    return None;
                }
                let load_span = |index: usize| {
                    let words =
                        std::array::from_fn(|word| phase.slots[index][word].load(Ordering::SeqCst));
                    if words[0] != header[1] {
                        return None;
                    }
                    ConvertedPhaseSpan::from_words(words).ok()
                };
                let first = load_span(0)?;
                if compare_query(span_start(first)?).is_lt()
                    || compare_time(span_start(first)?, initial_time).is_lt()
                    || compare_position(
                        decode_position(&first.to_words()[1..4]),
                        decode_position(&header[5..8]),
                    )
                    .is_lt()
                    || (retirement == 0
                        && (span_start(first)? != initial_time
                            || first.to_words()[1..4] != header[5..8]))
                {
                    return None;
                }
                // Upper-bound search chooses the new span at an exact transition.
                let mut low = 0;
                let mut high = count;
                while low < high {
                    let middle = low + (high - low) / 2;
                    if compare_query(span_start(load_span(middle)?)?).is_lt() {
                        high = middle;
                    } else {
                        low = middle + 1;
                    }
                }
                let index = low.checked_sub(1)?;
                let span = load_span(index)?;
                let start = span_start(span)?;
                if index != 0 && !compare_time(span_start(load_span(index - 1)?)?, start).is_lt() {
                    return None;
                }
                let segment_end = if index + 1 < count {
                    span_start(load_span(index + 1)?)?
                } else {
                    generated_end
                };
                if !compare_time(start, segment_end).is_lt()
                    || compare_time(segment_end, generated_end).is_gt()
                {
                    return None;
                }
                if compare_query(start).is_lt() || compare_query(segment_end).is_gt() {
                    return None;
                }
                let values = std::array::from_fn(|index| self.values[index].load(Ordering::SeqCst));
                // Strict opted publication validation does not alter the
                // historical legacy telemetry decoder.
                if values[0] != 0
                    && (values[4] > 1
                        || values[5] == 0
                        || values[6] == 0
                        || values[6] != header[1]
                        || values[15] == 0
                        || values[14] >= values[15]
                        || values[18] == 0
                        || values[17] >= values[18])
                {
                    return None;
                }
                let (last_real_source, report, boundaries) = decode_output(
                    values,
                    self.source.read().render,
                    self.real_source.read().render,
                )?;
                if report.is_some_and(|report| {
                    compare_time(report.target_start_time, report.target_end_time).is_gt()
                        || compare_time(report.target_end_time, generated_end).is_gt()
                }) {
                    return None;
                }
                finish(ConvertedPhaseSnapshot {
                    span,
                    segment_end,
                    generated_end,
                    stream_start,
                    submitted_end,
                    revision: before,
                    report,
                    boundaries,
                    last_real_source,
                })
            })();
            if before == self.generation.load(Ordering::SeqCst) {
                return result;
            }
        }
        None
    }
}
fn compare_time(left: TargetTime, right: TargetTime) -> std::cmp::Ordering {
    left.seconds().cmp(&right.seconds()).then_with(|| {
        (u128::from(left.numerator()) * u128::from(right.denominator()))
            .cmp(&(u128::from(right.numerator()) * u128::from(left.denominator())))
    })
}
fn compare_position(left: SourcePosition, right: SourcePosition) -> std::cmp::Ordering {
    left.frame.cmp(&right.frame).then_with(|| {
        (u128::from(left.numerator) * u128::from(right.denominator))
            .cmp(&(u128::from(right.numerator) * u128::from(left.denominator)))
    })
}
fn span_start(span: ConvertedPhaseSpan) -> Option<TargetTime> {
    let words = span.to_words();
    decode_time(&words[4..7])
}
fn decode_output(
    values: [u64; 58],
    source: Option<RenderReport>,
    real: Option<RenderReport>,
) -> Option<(
    Option<RenderReport>,
    Option<ConvertedRenderReport>,
    ConvertedBoundaryFacts,
)> {
    let facts = decode_facts(&values)?;
    if values[0] == 0 && facts == ConvertedBoundaryFacts::default() {
        return None;
    }
    let report = if values[0] == 0 {
        None
    } else {
        Some(ConvertedRenderReport {
            source,
            target_frames: usize::try_from(values[1]).ok()?,
            target_frame_cursor: values[2],
            pulled_source_frame_cursor: values[3],
            state: if values[4] == 0 {
                ConvertedOutputState::Active
            } else {
                ConvertedOutputState::Held
            },
            target_rate: u32::try_from(values[5]).ok()?,
            source_rate: u32::try_from(values[6]).ok()?,
            target_start_time: decode_time(&values[7..10])?,
            target_end_time: decode_time(&values[10..13])?,
            source_start_position: decode_position(&values[13..16]),
            source_position: decode_position(&values[16..19]),
            startup_source_frame: (values[19] != 0).then_some(values[20]),
            pause_source_frame: (values[21] != 0).then_some(values[22]),
            resume_source_frame: (values[45] != 0).then_some(values[46]),
        })
    };
    Some((real, report, facts))
}

fn snapshot(render: Option<RenderReport>) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        clock: None,
        render,
    }
}
fn encode_time(time: TargetTime, values: &mut [u64]) {
    values.copy_from_slice(&[time.seconds(), time.numerator(), time.denominator()]);
}
fn decode_time(values: &[u64]) -> Option<TargetTime> {
    TargetTime::new(values[0], values[1], values[2]).ok()
}
fn encode_position(position: SourcePosition, values: &mut [u64]) {
    values.copy_from_slice(&[position.frame, position.numerator, position.denominator]);
}
fn decode_position(values: &[u64]) -> SourcePosition {
    SourcePosition {
        frame: values[0],
        numerator: values[1],
        denominator: values[2],
    }
}
fn encode_boundary(boundary: Option<TargetBoundary>, values: &mut [u64]) {
    if let Some(boundary) = boundary {
        values[0] = 1;
        values[1] = boundary.source_frame;
        values[2] = boundary.target_frame_offset as u64;
        encode_time(boundary.target_time, &mut values[3..6]);
    }
}
fn decode_boundary(values: &[u64]) -> Option<Option<TargetBoundary>> {
    if values[0] == 0 {
        return Some(None);
    }
    Some(Some(TargetBoundary {
        source_frame: values[1],
        target_frame_offset: usize::try_from(values[2]).ok()?,
        target_time: decode_time(&values[3..6])?,
    }))
}
fn decode_facts(values: &[u64]) -> Option<ConvertedBoundaryFacts> {
    Some(ConvertedBoundaryFacts {
        startup: decode_boundary(&values[23..29])?,
        pause: decode_boundary(&values[29..35])?,
        end: decode_boundary(&values[35..41])?,
        source_rate: u32::try_from(values[44]).ok()?,
        resume: decode_boundary(&values[47..53])?,
        origin: if values[41] == 0 {
            None
        } else {
            Some(ClockPoint {
                domain: ClockDomainId(u32::try_from(values[42]).ok()?),
                timestamp: Timestamp::from_nanos(values[43] as i64),
            })
        },
    })
}
#[cfg(test)]
#[path = "converted_telemetry_fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[path = "converted_retirement_telemetry_fixtures.rs"]
mod retirement_fixtures;
