//! Coherent fixed atomic converted-output facts, with bounded nonblocking reads.
use super::*;
use crate::audio::{telemetry::Telemetry, ConvertedBoundaryFacts};
use beatkernel::audio::{
    ConvertedOutputState, ConvertedRenderReport, SourcePosition, TargetBoundary, TargetTime,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(super) struct ConvertedTelemetry {
    generation: AtomicU64,
    held: AtomicBool,
    values: [AtomicU64; 58],
    source: Telemetry,
    real_source: Telemetry,
}
impl ConvertedTelemetry {
    pub(super) fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            held: AtomicBool::new(false),
            values: std::array::from_fn(|_| AtomicU64::new(0)),
            source: Telemetry::new(),
            real_source: Telemetry::new(),
        }
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
        let version = self.generation.load(Ordering::SeqCst);
        let Some(next) = version.checked_add(2) else {
            self.generation.store(u64::MAX, Ordering::SeqCst);
            return;
        };
        self.generation.store(next - 1, Ordering::SeqCst);
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
