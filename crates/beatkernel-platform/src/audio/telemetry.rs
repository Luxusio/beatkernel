//! Fixed scalar native telemetry publication, shared with portable unit tests.
//!
//! The worker is the sole publisher. SeqCst orders the odd generation marker,
//! payload writes and final even marker with every bounded reader observation.
//! Matching even generations therefore delimit one complete publication; checked
//! exhaustion never wraps into an earlier generation. Status is independently
//! observed so a joined worker panic remains visible without a new payload.

use super::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
};
use beatkernel::{
    audio::{AudioCounters, RenderReport},
    time::{ClockDomainId, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct Telemetry {
    version: AtomicU64,
    pub(crate) status: AtomicU64,
    values: [AtomicU64; 33],
}

impl Telemetry {
    pub(crate) fn new() -> Self {
        Self {
            version: AtomicU64::new(0),
            status: AtomicU64::new(status_code(AudioStreamStatus::Ready)),
            values: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
    pub(crate) fn publish(&self, snapshot: AudioStreamSnapshot, version: &mut u64) {
        self.status
            .store(status_code(snapshot.status), Ordering::SeqCst);
        let Some(next) = version.checked_add(2) else {
            // Permanent unavailable marker instead of wrapping publication ABA.
            self.version.store(u64::MAX, Ordering::SeqCst);
            return;
        };
        self.version.store(next - 1, Ordering::SeqCst);
        let encoded = encode_snapshot(snapshot);
        for (destination, value) in self.values.iter().zip(encoded) {
            destination.store(value, Ordering::SeqCst);
        }
        self.version.store(next, Ordering::SeqCst);
        *version = next;
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

fn encode_snapshot(snapshot: AudioStreamSnapshot) -> [u64; 33] {
    let mut values = [0u64; 33];
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
        values[22..].copy_from_slice(&[
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
    }
    values
}

fn decode_snapshot(values: [u64; 33]) -> AudioStreamSnapshot {
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
            active_voices: values[18] as usize,
            pending_commands: values[19] as usize,
            song_position: Timestamp::from_nanos(values[20] as i64),
            producer_disconnected: values[21] != 0,
            counters: AudioCounters {
                rendered_frames: values[22],
                commands_consumed: values[23],
                commands_applied: values[24],
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
