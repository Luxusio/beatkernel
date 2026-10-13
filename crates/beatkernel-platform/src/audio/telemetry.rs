//! Fixed scalar native telemetry publication, shared with portable unit tests.
//!
//! The worker is the sole publisher. SeqCst orders the odd generation marker,
//! payload writes and final even marker with every bounded reader observation.
//! Matching even generations therefore delimit one complete publication; checked
//! exhaustion never wraps into an earlier generation. Status is independently
//! observed so a joined worker panic remains visible without a new payload.

use super::{
    source_history_retirement_mailbox::{
        SourceHistoryRetirementMailbox, SourceHistoryRetirementRequest,
    },
    source_playback_publication::SourcePlaybackPublication,
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    SourcePlaybackSnapshot, StreamCounters,
};
use beatkernel::{
    audio::{
        AudioCounters, AudioError, InterpolatedTargetTime, OutputFrameBasis, RenderReport,
        SourcePlaybackHistory,
    },
    time::{ClockDomainId, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct Telemetry {
    version: AtomicU64,
    pub(crate) status: AtomicU64,
    values: [AtomicU64; 39],
    source_playback: Option<SourcePlaybackPublication>,
    source_retirement: Option<SourceHistoryRetirementMailbox>,
}

/// Observes one running deadline interval; Ready/prefill is not an interval.
/// Clear the baseline on native Start so its first observation only anchors.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn observe_deadline(
    previous: &mut Option<u64>,
    qpc_100ns: u64,
    period: Duration,
) -> bool {
    let before = previous.replace(qpc_100ns);
    before.is_some_and(|before| {
        qpc_100ns >= before
            && u128::from(qpc_100ns - before) * 100 > (period.as_nanos() as u128) * 2
    })
}

impl Telemetry {
    pub(crate) fn new() -> Self {
        Self {
            version: AtomicU64::new(0),
            status: AtomicU64::new(status_code(AudioStreamStatus::Ready)),
            values: std::array::from_fn(|_| AtomicU64::new(0)),
            source_playback: None,
            source_retirement: None,
        }
    }
    /// Cold opt-in allocation, before the native publisher starts.
    pub(crate) fn with_source_history(history: &SourcePlaybackHistory) -> Result<Self, AudioError> {
        Ok(Self {
            source_playback: Some(SourcePlaybackPublication::new(history)?),
            ..Self::new()
        })
    }
    /// Cold opt-in to callback-owned retirement intent. This does not retire
    /// history or establish native presentation authority.
    pub(crate) fn with_source_history_and_retirement(
        history: &SourcePlaybackHistory,
        basis: OutputFrameBasis,
    ) -> Result<Self, AudioError> {
        let mailbox = SourceHistoryRetirementMailbox::new(basis, history)?;
        Ok(Self {
            source_retirement: Some(mailbox),
            ..Self::with_source_history(history)?
        })
    }
    /// Queues only a minimum present in a coherent original Source publication.
    /// The returned revision describes intent, never callback application or ACK.
    pub(crate) fn request_source_history_retirement(
        &self,
        expected_basis: OutputFrameBasis,
        source_identity: (u32, u64, u64),
        source_pin: u64,
    ) -> Result<Option<u64>, AudioError> {
        let mailbox = self
            .source_retirement
            .as_ref()
            .ok_or(AudioError::InvalidCapacity)?;
        mailbox.validate_identity(expected_basis, source_identity)?;
        let publication = self
            .source_playback
            .as_ref()
            .ok_or(AudioError::InvalidCapacity)?;
        for _ in 0..3 {
            let before = self.version.load(Ordering::SeqCst);
            if !before.is_multiple_of(2) {
                continue;
            }
            let contains = publication.contains_retirement_pin(source_identity, source_pin);
            if self.version.load(Ordering::SeqCst) != before {
                continue;
            }
            if !contains {
                return Err(AudioError::InvalidFormat);
            }
            return mailbox.request(expected_basis, source_identity, source_pin);
        }
        Ok(None)
    }
    /// The callback may consume this intent; observation does not apply it.
    pub(crate) fn source_retirement_request(&self) -> Option<SourceHistoryRetirementRequest> {
        self.source_retirement.as_ref()?.latest()
    }
    pub(crate) fn publish(&self, snapshot: AudioStreamSnapshot, version: &mut u64) {
        self.publish_with_source_history(Some(snapshot), None, version);
    }
    pub(crate) fn publish_with_source_history(
        &self,
        snapshot: Option<AudioStreamSnapshot>,
        history: Option<&SourcePlaybackHistory>,
        version: &mut u64,
    ) {
        if let Some(snapshot) = snapshot {
            self.status
                .store(status_code(snapshot.status), Ordering::SeqCst);
        }
        if self.version.load(Ordering::SeqCst) == u64::MAX {
            return;
        }
        let Some(next) = version.checked_add(2) else {
            // Permanent unavailable marker instead of wrapping publication ABA.
            self.version.store(u64::MAX, Ordering::SeqCst);
            return;
        };
        self.version.store(next - 1, Ordering::SeqCst);
        if let Some(snapshot) = snapshot {
            let encoded = encode_snapshot(snapshot);
            for (destination, value) in self.values.iter().zip(encoded) {
                destination.store(value, Ordering::SeqCst);
            }
        }
        if let Some(publication) = &self.source_playback {
            publication.publish(history);
        }
        self.version.store(next, Ordering::SeqCst);
        *version = next;
    }
    /// Reads scalar and source facts within one bounded caller-owned revision.
    /// Submission counts and clock metadata supply no presentation authority.
    pub(crate) fn read_source_playback_at(
        &self,
        query: &InterpolatedTargetTime,
        expected_source_rate: u32,
        expected_initial_physical: u64,
        expected_initial_playback: u64,
    ) -> Option<SourcePlaybackSnapshot> {
        self.read_source_playback_with(
            query,
            expected_source_rate,
            expected_initial_physical,
            expected_initial_playback,
            |_| {},
        )
    }
    fn read_source_playback_with(
        &self,
        query: &InterpolatedTargetTime,
        expected_source_rate: u32,
        expected_initial_physical: u64,
        expected_initial_playback: u64,
        mut before_recheck: impl FnMut(usize),
    ) -> Option<SourcePlaybackSnapshot> {
        let publication = self.source_playback.as_ref()?;
        for attempt in 0..3 {
            let before = self.version.load(Ordering::SeqCst);
            if !before.is_multiple_of(2) {
                continue;
            }
            let values = std::array::from_fn(|index| self.values[index].load(Ordering::SeqCst));
            let selected = publication.select(
                expected_source_rate,
                expected_initial_physical,
                expected_initial_playback,
                |frame| {
                    query
                        .compare_source_time_frame(frame, expected_source_rate)
                        .ok()
                },
            );
            let selected =
                selected.filter(|selected| valid_source_report(&values, selected.pulled_end_frame));
            before_recheck(attempt);
            if self.version.load(Ordering::SeqCst) == before {
                let selected = selected?;
                let mut snapshot = decode_snapshot(values);
                snapshot.status = decode_status(self.status.load(Ordering::SeqCst));
                return Some(SourcePlaybackSnapshot {
                    snapshot,
                    playback: selected.span,
                    pulled_source_end: selected.pulled_end_frame,
                    revision: before,
                });
            }
        }
        None
    }
    pub(crate) fn read(&self) -> AudioStreamSnapshot {
        for _ in 0..3 {
            let before = self.version.load(Ordering::SeqCst);
            if !before.is_multiple_of(2) {
                continue;
            }
            let values = std::array::from_fn(|index| self.values[index].load(Ordering::SeqCst));
            if self.version.load(Ordering::SeqCst) == before {
                let mut snapshot = decode_snapshot(values);
                snapshot.status = decode_status(self.status.load(Ordering::SeqCst));
                return snapshot;
            }
        }
        AudioStreamSnapshot {
            telemetry_available: false,
            status: decode_status(self.status.load(Ordering::SeqCst)),
            counters: StreamCounters::default(),
            clock: None,
            render: None,
        }
    }
}

// Opted source reads reject malformed report flags and extents without changing
// the legacy scalar decoder or equating a report end with the latest pull.
fn valid_source_report(values: &[u64; 39], pulled_end: u64) -> bool {
    if [15, 21, 35, 36].into_iter().any(|index| values[index] > 1) {
        return false;
    }
    if values[15] == 0 {
        return true;
    }
    values[33] <= values[16]
        && values[34] <= values[17]
        && values[16]
            .checked_add(values[17])
            .is_some_and(|end| end <= pulled_end)
        && values[33].checked_add(values[34]).is_some()
        && usize::try_from(values[17]).is_ok()
        && usize::try_from(values[34]).is_ok()
        && (values[36] == 0 || values[37] <= pulled_end)
}

pub(crate) fn status_code(status: AudioStreamStatus) -> u64 {
    match status {
        AudioStreamStatus::Ready => 0,
        AudioStreamStatus::Running => 1,
        AudioStreamStatus::Stopped => 2,
        AudioStreamStatus::WorkerPanicked => 3,
        AudioStreamStatus::Failed { hresult } => (u64::from(hresult as u32) << 32) | 4,
    }
}

fn decode_status(value: u64) -> AudioStreamStatus {
    match value as u32 {
        0 => AudioStreamStatus::Ready,
        1 => AudioStreamStatus::Running,
        2 => AudioStreamStatus::Stopped,
        3 => AudioStreamStatus::WorkerPanicked,
        _ => AudioStreamStatus::Failed {
            hresult: (value >> 32) as i32,
        },
    }
}

fn encode_snapshot(snapshot: AudioStreamSnapshot) -> [u64; 39] {
    let mut values = [0u64; 39];
    let counters = snapshot.counters;
    values[..7].copy_from_slice(&[
        status_code(snapshot.status),
        counters.submitted_frames,
        counters.buffer_fills,
        u64::from(counters.padding_frames),
        counters.inferred_starvations,
        counters.inferred_deadline_misses,
        counters.native_failures,
    ]);
    if let Some(clock) = snapshot.clock {
        let quality = match clock.reading_quality {
            AudioClockReadingQuality::Accurate => 0,
            AudioClockReadingQuality::Degraded => 1,
            AudioClockReadingQuality::Unknown => 2,
        };
        values[7] = 1 | (quality << 1);
        values[8] = clock.position;
        values[9] = clock.frequency;
        values[10] = clock.qpc_100ns;
        if let Some(host) = clock.host_point {
            values[11] = host.timestamp.as_nanos() as u64;
            values[12] = u64::from(host.domain.0);
            values[14] = 1;
        }
        values[13] = match clock.mapping_quality {
            ClockMappingQuality::Exact => 0,
            ClockMappingQuality::Estimated { max_error } => max_error.as_nanos() as u64 + 1,
            ClockMappingQuality::Unknown => u64::MAX,
        };
    }
    if let Some(render) = snapshot.render {
        values[15] = 1;
        values[16..22].copy_from_slice(&[
            render.start_frame,
            render.frames as u64,
            render.active_voices as u64,
            render.pending_commands as u64,
            render.song_position.as_nanos() as u64,
            u64::from(render.producer_disconnected),
        ]);
        let counters = render.counters;
        values[22..33].copy_from_slice(&[
            counters.rendered_frames,
            counters.commands_consumed,
            counters.commands_applied,
            counters.late_commands,
            counters.pending_full,
            counters.voice_full,
            counters.unknown_samples,
            counters.unknown_stops,
            counters.invalid_gains,
            counters.invalid_rates,
            counters.invalid_times,
        ]);
        values[33..36].copy_from_slice(&[
            render.playback_start_frame,
            render.playback_frames as u64,
            u64::from(render.paused),
        ]);
        if let Some(frame) = render.playback_end_physical_frame {
            values[36] = 1;
            values[37] = frame;
        }
        values[38] = counters.commands_expired;
    }
    values
}

fn decode_snapshot(values: [u64; 39]) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: decode_status(values[0]),
        counters: StreamCounters {
            submitted_frames: values[1],
            buffer_fills: values[2],
            padding_frames: values[3] as u32,
            inferred_starvations: values[4],
            inferred_deadline_misses: values[5],
            native_failures: values[6],
        },
        clock: (values[7] != 0).then(|| AudioClockSnapshot {
            position: values[8],
            frequency: values[9],
            qpc_100ns: values[10],
            reading_quality: match values[7] >> 1 {
                0 => AudioClockReadingQuality::Accurate,
                1 => AudioClockReadingQuality::Degraded,
                _ => AudioClockReadingQuality::Unknown,
            },
            host_point: (values[14] != 0).then(|| ClockPoint {
                domain: ClockDomainId(values[12] as u32),
                timestamp: Timestamp::from_nanos(values[11] as i64),
            }),
            mapping_quality: match values[13] {
                0 => ClockMappingQuality::Exact,
                u64::MAX => ClockMappingQuality::Unknown,
                value => ClockMappingQuality::Estimated {
                    max_error: Duration::from_nanos((value - 1) as i64),
                },
            },
        }),
        render: (values[15] != 0).then(|| RenderReport {
            start_frame: values[16],
            frames: values[17] as usize,
            playback_start_frame: values[33],
            playback_frames: values[34] as usize,
            paused: values[35] != 0,
            playback_end_physical_frame: (values[36] != 0).then_some(values[37]),
            active_voices: values[18] as usize,
            pending_commands: values[19] as usize,
            song_position: Timestamp::from_nanos(values[20] as i64),
            producer_disconnected: values[21] != 0,
            counters: AudioCounters {
                rendered_frames: values[22],
                commands_consumed: values[23],
                commands_applied: values[24],
                commands_expired: values[38],
                late_commands: values[25],
                pending_full: values[26],
                voice_full: values[27],
                unknown_samples: values[28],
                unknown_stops: values[29],
                invalid_gains: values[30],
                invalid_rates: values[31],
                invalid_times: values[32],
            },
        }),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "telemetry/playback_fixtures.rs"]
mod playback_fixtures;

#[cfg(test)]
#[path = "telemetry/source_history_retirement_fixtures.rs"]
mod source_history_retirement_fixtures;
