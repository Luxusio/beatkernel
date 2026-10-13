//! Bounded actual source playback facts protected by each caller's revision.
//! This payload has no coherence protocol or native presentation authority.
use beatkernel::audio::{
    AudioError, Mixer, SourcePlaybackHistory, SourcePlaybackSpan, SourcePlaybackState,
};
use std::cmp::Ordering as Comparison;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Cold preparation using actual native buffer geometry. Existing history is
/// preserved; this helper grants no native presentation authority.
pub(crate) fn prepare_native_source_history(
    mixer: &mut Mixer,
    buffer_frames: u32,
    headroom: usize,
) -> Result<(), AudioError> {
    let frames = usize::try_from(buffer_frames).map_err(|_| AudioError::Overflow)?;
    if frames == 0 {
        return Err(AudioError::InvalidCapacity);
    }
    if frames > mixer.configuration().limits().max_render_frames() {
        return Err(AudioError::RenderCapacity);
    }
    if mixer.source_playback_history().is_some() {
        return Ok(());
    }
    let capacity = frames.checked_add(headroom).ok_or(AudioError::Overflow)?;
    mixer.enable_source_playback_history(capacity)
}

pub(crate) struct SourcePlaybackPublication {
    slots: Vec<[AtomicU64; 3]>,
    // Count, source Hz, initial physical/playback frames, actual pulled end.
    values: [AtomicU64; 5],
    // Owner history's mathematical nonce, protected by the caller's revision.
    retirement_revision: AtomicU64,
    available: AtomicBool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PublishedSourcePlayback {
    pub(crate) span: SourcePlaybackSpan,
    pub(crate) pulled_end_frame: u64,
}

impl SourcePlaybackPublication {
    /// Cold allocation and seeding from actual successful source pulls.
    pub(crate) fn new(history: &SourcePlaybackHistory) -> Result<Self, AudioError> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(history.capacity())
            .map_err(|_| AudioError::AllocationFailed)?;
        for index in 0..history.capacity() {
            let words = history.span(index).map_or([0; 3], playback_words);
            slots.push(words.map(AtomicU64::new));
        }
        Ok(Self {
            slots,
            values: [
                history.len() as u64,
                u64::from(history.source_rate()),
                history.initial_physical_frame(),
                history.initial_playback_frame(),
                history.pulled_end_frame(),
            ]
            .map(AtomicU64::new),
            retirement_revision: AtomicU64::new(history.retirement_revision()),
            available: AtomicBool::new(true),
        })
    }
    /// Writer-side no-change check; the owning writer serializes this access.
    pub(crate) fn unchanged(&self, history: &SourcePlaybackHistory) -> bool {
        self.values[0].load(Ordering::SeqCst) == history.len() as u64
            && self.values[4].load(Ordering::SeqCst) == history.pulled_end_frame()
            && self.retirement_revision.load(Ordering::SeqCst) == history.retirement_revision()
    }
    /// Called inside the owning adapter's publication write revision.
    pub(crate) fn publish(&self, history: Option<&SourcePlaybackHistory>) {
        let old: [u64; 5] = std::array::from_fn(|index| self.values[index].load(Ordering::SeqCst));
        let old_retirement = self.retirement_revision.load(Ordering::SeqCst);
        let valid = (|| {
            let history = history?;
            let old_count = usize::try_from(old[0]).ok()?;
            let retirement = history.retirement_revision();
            let retired = retirement > old_retirement;
            if history.capacity() != self.slots.len()
                || u64::from(history.source_rate()) != old[1]
                || history.initial_physical_frame() != old[2]
                || history.initial_playback_frame() != old[3]
                || history.pulled_end_frame() < old[4]
                || retirement < old_retirement
                || old_count > self.slots.len()
                || (!retired
                    && (history.len() < old_count || history.len() > old_count.saturating_add(3)))
                || history.len() > self.slots.len()
            {
                return None;
            }
            if retired {
                let previous_start = if old_count == 0 {
                    old[2]
                } else {
                    self.slots[0][0].load(Ordering::SeqCst)
                };
                if history.span(0)?.physical_start() <= previous_start {
                    return None;
                }
            }
            let first_changed = if retired { 0 } else { old_count };
            for index in first_changed..history.len() {
                let span = history.span(index)?;
                if span.physical_start() >= history.pulled_end_frame()
                    || span.physical_end()
                        != history.span(index + 1).map(|next| next.physical_start())
                {
                    return None;
                }
                if index == 0 {
                    if (retirement == 0
                        && (span.physical_start() != old[2] || span.playback_start() != old[3]))
                        || span.physical_start() < old[2]
                        || span.playback_start() < old[3]
                    {
                        return None;
                    }
                } else if !playback_contiguous(history.span(index - 1)?, span) {
                    return None;
                }
            }
            for index in first_changed..history.len() {
                for (field, word) in self.slots[index]
                    .iter()
                    .zip(playback_words(history.span(index)?))
                {
                    field.store(word, Ordering::SeqCst);
                }
            }
            self.values[0].store(history.len() as u64, Ordering::SeqCst);
            self.values[4].store(history.pulled_end_frame(), Ordering::SeqCst);
            self.retirement_revision.store(retirement, Ordering::SeqCst);
            Some(())
        })();
        if valid.is_none() {
            self.available.store(false, Ordering::SeqCst);
        }
    }
    /// Selects facts within an already-started caller read revision. The caller
    /// must recheck that revision before accepting the returned facts.
    pub(crate) fn select(
        &self,
        expected_source_rate: u32,
        expected_initial_physical: u64,
        expected_initial_playback: u64,
        compare_frame: impl Fn(u64) -> Option<Comparison>,
    ) -> Option<PublishedSourcePlayback> {
        if !self.available.load(Ordering::SeqCst) {
            return None;
        }
        let header: [u64; 5] =
            std::array::from_fn(|index| self.values[index].load(Ordering::SeqCst));
        let retirement = self.retirement_revision.load(Ordering::SeqCst);
        let count = usize::try_from(header[0]).ok()?;
        if count == 0
            || count > self.slots.len()
            || header[1] != u64::from(expected_source_rate)
            || header[2] != expected_initial_physical
            || header[3] != expected_initial_playback
            || header[3] > header[2]
            || header[4] < header[2]
        {
            return None;
        }
        let load_span = |index: usize| {
            let words: [u64; 3] =
                std::array::from_fn(|word| self.slots[index][word].load(Ordering::SeqCst));
            let state = match words[2] {
                0 => SourcePlaybackState::Running,
                1 => SourcePlaybackState::Frozen,
                _ => return None,
            };
            SourcePlaybackSpan::new(words[0], None, words[1], state).ok()
        };
        let first = load_span(0)?;
        let last = load_span(count - 1)?;
        if (retirement == 0
            && (first.physical_start() != header[2] || first.playback_start() != header[3]))
            || first.physical_start() < header[2]
            || first.playback_start() < header[3]
            || last.physical_start() >= header[4]
        {
            return None;
        }
        if compare_frame(first.physical_start())?.is_lt() || compare_frame(header[4])?.is_gt() {
            return None;
        }
        let mut low = 0;
        let mut high = count;
        while low < high {
            let middle = low + (high - low) / 2;
            if compare_frame(load_span(middle)?.physical_start())?.is_lt() {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        let index = low.checked_sub(1)?;
        let selected = load_span(index)?;
        if index != 0 && !playback_contiguous(load_span(index - 1)?, selected) {
            return None;
        }
        let end = if index + 1 < count {
            let next = load_span(index + 1)?;
            if next.physical_start() > header[4] || !playback_contiguous(selected, next) {
                return None;
            }
            Some(next.physical_start())
        } else {
            None
        };
        if selected.physical_start() < header[2]
            || selected.physical_start() >= header[4]
            || compare_frame(selected.physical_start())?.is_lt()
            || end.is_some_and(|end| compare_frame(end).is_none_or(|order| !order.is_lt()))
        {
            return None;
        }
        let span = SourcePlaybackSpan::new(
            selected.physical_start(),
            end,
            selected.playback_start(),
            selected.state(),
        )
        .ok()?;
        Some(PublishedSourcePlayback {
            span,
            pulled_end_frame: header[4],
        })
    }
    /// Range qualification inside the caller's existing read revision.
    pub(crate) fn contains_retirement_pin(&self, identity: (u32, u64, u64), pin: u64) -> bool {
        if self
            .select(identity.0, identity.1, identity.2, |frame| {
                Some(pin.cmp(&frame))
            })
            .is_some()
        {
            return true;
        }
        let header: [u64; 5] =
            std::array::from_fn(|index| self.values[index].load(Ordering::SeqCst));
        self.available.load(Ordering::SeqCst)
            && header[0] == 0
            && header[1] != 0
            && header[1] == u64::from(identity.0)
            && header[2] == identity.1
            && header[3] == identity.2
            && header[3] <= header[2]
            && header[4] == header[2]
            && pin == header[2]
            && self.retirement_revision.load(Ordering::SeqCst) == 0
    }
    /// Scalar header load, qualified only by the surrounding caller revision.
    pub(crate) fn initial_playback_frame(&self) -> u64 {
        self.values[3].load(Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn test_header(&self) -> &[AtomicU64; 5] {
        &self.values
    }
    #[cfg(test)]
    pub(crate) fn test_slots(&self) -> &[[AtomicU64; 3]] {
        &self.slots
    }
    #[cfg(test)]
    pub(crate) fn test_available(&self) -> &AtomicBool {
        &self.available
    }
}
fn playback_words(span: SourcePlaybackSpan) -> [u64; 3] {
    [
        span.physical_start(),
        span.playback_start(),
        u64::from(span.state() == SourcePlaybackState::Frozen),
    ]
}
fn playback_contiguous(before: SourcePlaybackSpan, after: SourcePlaybackSpan) -> bool {
    if after.physical_start() <= before.physical_start() {
        return false;
    }
    let expected = match before.state() {
        SourcePlaybackState::Running => before
            .playback_start()
            .checked_add(after.physical_start() - before.physical_start()),
        SourcePlaybackState::Frozen => Some(before.playback_start()),
    };
    expected == Some(after.playback_start())
}

#[cfg(test)]
#[path = "source_playback_publication_fixtures.rs"]
mod fixtures;

#[cfg(test)]
#[path = "source_playback_retirement_publication_fixtures.rs"]
mod retirement_fixtures;
