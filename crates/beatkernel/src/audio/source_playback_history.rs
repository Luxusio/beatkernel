//! Actual source pulls and playback adoption retained by a complete converter owner.

use super::{
    AudioError, ConvertedPhaseError, ConvertedPhaseSpan, InterpolatedTargetTime,
    SourcePlaybackSpan, SourcePlaybackState, TargetTime,
};

/// Bounded history of actual nonempty Mixer source intervals.
/// These facts do not authenticate native submission, presentation or pause ACK.
pub struct SourcePlaybackHistory {
    source_rate: u32,
    initial_physical: u64,
    initial_playback: u64,
    pulled_end: u64,
    capacity: usize,
    spans: Vec<SourcePlaybackSpan>,
    retirement_revision: u64,
}

pub(super) struct SourcePlaybackPlan {
    spans: [Option<SourcePlaybackSpan>; 3],
    count: usize,
    merge_first: bool,
    end: u64,
}

impl SourcePlaybackHistory {
    pub(super) fn new(
        source_rate: u32,
        physical: u64,
        playback: u64,
        capacity: usize,
    ) -> Result<Self, AudioError> {
        if capacity == 0 {
            return Err(AudioError::InvalidCapacity);
        }
        if source_rate == 0 || playback > physical {
            return Err(AudioError::InvalidFormat);
        }
        let mut spans = Vec::new();
        spans
            .try_reserve_exact(capacity)
            .map_err(|_| AudioError::AllocationFailed)?;
        Ok(Self {
            source_rate,
            initial_physical: physical,
            initial_playback: playback,
            pulled_end: physical,
            capacity,
            spans,
            retirement_revision: 0,
        })
    }

    /// Immutable original source sample rate.
    pub const fn source_rate(&self) -> u32 {
        self.source_rate
    }
    /// Physical source cursor captured at cold enabling.
    pub const fn initial_physical_frame(&self) -> u64 {
        self.initial_physical
    }
    /// Active playback cursor captured at cold enabling.
    pub const fn initial_playback_frame(&self) -> u64 {
        self.initial_playback
    }
    /// Inclusive mathematical endpoint of actual successful source pulls.
    pub const fn pulled_end_frame(&self) -> u64 {
        self.pulled_end
    }
    /// Earliest physical source frame whose playback interval is retained.
    /// An empty cold history retains its original physical starting point.
    pub fn retained_start_frame(&self) -> u64 {
        self.spans
            .first()
            .map_or(self.initial_physical, |span| span.physical_start())
    }
    /// Mathematical retirement nonce, incremented only when spans are removed.
    /// This value proves no native publication, freshness or acknowledgement.
    pub const fn retirement_revision(&self) -> u64 {
        self.retirement_revision
    }
    /// Checks the same mathematical range and checked nonce as `retire_before`
    /// without changing the history or allocating. A successful check grants
    /// no consumer qualification or native retirement permission.
    pub fn validate_retirement_before(&self, pin: u64) -> Result<(), AudioError> {
        let remove = self.retirement_count(pin)?;
        self.checked_retirement_revision(remove)?;
        Ok(())
    }
    /// Removes complete intervals preceding the interval covering `pin`.
    /// The owning thread must qualify `pin` against the minimum of all live
    /// consumer references before calling; the integer alone grants no native
    /// drain, presentation or acknowledgement permission. The covering interval
    /// and all later intervals retain their original descriptors and storage.
    pub fn retire_before(&mut self, pin: u64) -> Result<usize, AudioError> {
        let remove = self.retirement_count(pin)?;
        let revision = self.checked_retirement_revision(remove)?;
        if remove == 0 {
            return Ok(0);
        }
        self.spans.copy_within(remove.., 0);
        self.spans.truncate(self.spans.len() - remove);
        self.retirement_revision = revision;
        Ok(remove)
    }

    fn retirement_count(&self, pin: u64) -> Result<usize, AudioError> {
        if pin < self.retained_start_frame() || pin > self.pulled_end {
            return Err(AudioError::InvalidFormat);
        }
        Ok(self
            .spans
            .partition_point(|span| span.physical_start() <= pin)
            .saturating_sub(1))
    }

    fn checked_retirement_revision(&self, remove: usize) -> Result<u64, AudioError> {
        if remove == 0 {
            return Ok(self.retirement_revision);
        }
        self.retirement_revision
            .checked_add(1)
            .ok_or(AudioError::Overflow)
    }
    /// Number of actual retained playback transitions.
    pub fn len(&self) -> usize {
        self.spans.len()
    }
    /// Whether no nonempty source pull has been recorded.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
    /// Fixed transition capacity reserved before rendering.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }
    /// Actual half-open transition descriptor. The last descriptor has no next
    /// transition; callers must separately qualify its actual pulled frontier.
    pub fn span(&self, index: usize) -> Option<SourcePlaybackSpan> {
        let span = *self.spans.get(index)?;
        Some(
            SourcePlaybackSpan::new(
                span.physical_start(),
                self.spans.get(index + 1).map(|next| next.physical_start()),
                span.playback_start(),
                span.state(),
            )
            .expect("validated source history"),
        )
    }

    /// Selects an actual source playback interval using the exact physical
    /// coordinate before any nanosecond floor. Equality at the actual pull
    /// frontier is permitted; fractions beyond that frontier are refused.
    /// Generated converter correspondence alone proves no native admission.
    pub fn lookup_at_interpolated_time(
        &self,
        converted_span: ConvertedPhaseSpan,
        query: InterpolatedTargetTime,
        qualified_end: TargetTime,
    ) -> Result<SourcePlaybackSpan, ConvertedPhaseError> {
        if converted_span.to_words()[0] != u64::from(self.source_rate) {
            return Err(ConvertedPhaseError::InvalidDescriptor);
        }
        if self.spans.is_empty() {
            return Err(ConvertedPhaseError::AfterGeneratedRange);
        }
        if converted_span
            .compare_source_frame_at_interpolated_time(
                query,
                qualified_end,
                self.retained_start_frame(),
            )?
            .is_lt()
        {
            return Err(ConvertedPhaseError::BeforeRetainedRange);
        }
        if converted_span
            .compare_source_frame_at_interpolated_time(query, qualified_end, self.pulled_end)?
            .is_gt()
        {
            return Err(ConvertedPhaseError::AfterGeneratedRange);
        }
        let mut low = 0;
        let mut high = self.spans.len();
        while low < high {
            let mid = low + (high - low) / 2;
            if converted_span
                .compare_source_frame_at_interpolated_time(
                    query,
                    qualified_end,
                    self.spans[mid].physical_start(),
                )?
                .is_lt()
            {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        let index = low
            .checked_sub(1)
            .ok_or(ConvertedPhaseError::BeforeRetainedRange)?;
        self.span(index)
            .ok_or(ConvertedPhaseError::BeforeRetainedRange)
    }

    /// Selects an actual source interval directly from exact physical source
    /// duration. The pulled frontier is inclusive, but fractions beyond it are
    /// refused before rounding. These source facts grant no native clock proof.
    pub fn lookup_at_source_time(
        &self,
        query: InterpolatedTargetTime,
    ) -> Result<SourcePlaybackSpan, ConvertedPhaseError> {
        if self.spans.is_empty() {
            return Err(ConvertedPhaseError::AfterGeneratedRange);
        }
        if query
            .compare_source_time_frame(self.retained_start_frame(), self.source_rate)?
            .is_lt()
        {
            return Err(ConvertedPhaseError::BeforeRetainedRange);
        }
        if query
            .compare_source_time_frame(self.pulled_end, self.source_rate)?
            .is_gt()
        {
            return Err(ConvertedPhaseError::AfterGeneratedRange);
        }
        let mut low = 0;
        let mut high = self.spans.len();
        while low < high {
            let mid = low + (high - low) / 2;
            if query
                .compare_source_time_frame(self.spans[mid].physical_start(), self.source_rate)?
                .is_lt()
            {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        let index = low
            .checked_sub(1)
            .ok_or(ConvertedPhaseError::BeforeRetainedRange)?;
        self.span(index)
            .ok_or(ConvertedPhaseError::BeforeRetainedRange)
    }

    pub(super) fn preflight(
        &self,
        source_rate: u32,
        start: u64,
        end: u64,
        playback_start: u64,
        active_start: u64,
        active_end: u64,
        running: bool,
    ) -> Result<SourcePlaybackPlan, AudioError> {
        if source_rate != self.source_rate || start != self.pulled_end || end <= start {
            return Err(AudioError::InvalidFormat);
        }
        let expected = self.spans.last().map_or(self.initial_playback, |last| {
            last.playback_start()
                + if last.state() == SourcePlaybackState::Running {
                    start - last.physical_start()
                } else {
                    0
                }
        });
        if playback_start != expected {
            return Err(AudioError::InvalidFormat);
        }
        let mut plan = SourcePlaybackPlan {
            spans: [None; 3],
            count: 0,
            merge_first: false,
            end,
        };
        let mut add = |physical_start, physical_end, playback_start, state| {
            if physical_start < physical_end {
                plan.spans[plan.count] = Some(
                    SourcePlaybackSpan::new(
                        physical_start,
                        Some(physical_end),
                        playback_start,
                        state,
                    )
                    .expect("validated Mixer extent"),
                );
                plan.count += 1;
            }
        };
        if running {
            add(
                start,
                active_start,
                playback_start,
                SourcePlaybackState::Frozen,
            );
            add(
                active_start,
                active_end,
                playback_start,
                SourcePlaybackState::Running,
            );
            add(
                active_end,
                end,
                playback_start + (active_end - active_start),
                SourcePlaybackState::Frozen,
            );
        } else {
            add(start, end, playback_start, SourcePlaybackState::Frozen);
        }
        plan.merge_first = self.spans.last().is_some_and(|last| {
            last.state() == plan.spans[0].expect("positive source extent").state()
        });
        let needed = plan.count - usize::from(plan.merge_first);
        if needed > self.capacity - self.spans.len() {
            return Err(AudioError::RenderCapacity);
        }
        if plan.merge_first {
            let last = self.spans.last().expect("preflighted merge");
            plan.spans[0] = Some(
                SourcePlaybackSpan::new(
                    last.physical_start(),
                    plan.spans[0]
                        .expect("positive source extent")
                        .physical_end(),
                    last.playback_start(),
                    last.state(),
                )
                .map_err(|_| AudioError::InvalidFormat)?,
            );
        }
        Ok(plan)
    }

    pub(super) fn commit(&mut self, plan: SourcePlaybackPlan) {
        for (index, span) in plan.spans.into_iter().take(plan.count).enumerate() {
            let span = span.expect("preflighted source interval");
            if index == 0 && plan.merge_first {
                *self.spans.last_mut().expect("preflighted merge") = span;
            } else {
                self.spans.push(span);
            }
        }
        self.pulled_end = plan.end;
    }
}
