//! Complete Source output through the actual, statically supplied HAL callback.
//! Raw native buffer access remains in the macOS adapter; this step validates
//! numeric descriptors and the entire delivery before the first channel copy.
use super::{
    source_history_retirement_mailbox::SourceHistoryRetirementRequest,
    source_output_retirement::{SourceRetirementDecision, SourceRetirementSlot},
    NativeOutputState, SampleEncoding,
};
use beatkernel::audio::RenderReport;
use std::sync::atomic::{AtomicU32, Ordering};

const INVALID_ARGUMENT: i32 = -50;
const MAX_BUFFERS: usize = 32;

/// Callback-local native metadata, never a license to dereference an address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoreAudioBufferDescriptor {
    pub(crate) channels: u32,
    pub(crate) byte_size: u32,
    pub(crate) address: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CoreAudioBufferGeometry {
    pub(crate) frames: usize,
    pub(crate) channels: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoreAudioSourcePhase {
    Quarantined,
    Publication,
    Configuration,
    Layout,
    Pending,
    History,
    Render,
    Extent,
    Admission,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CoreAudioSourceFailure {
    pub(crate) phase: CoreAudioSourcePhase,
    pub(crate) code: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoreAudioSourceStep {
    Retire,
    Delivered {
        report: RenderReport,
        first_frame: u64,
        frames: u32,
        end_frame: u64,
    },
    Failure(CoreAudioSourceFailure),
}

/// Validates every group, including disjoint native extents, before any write.
/// Addresses describe native allocations whose validity is the adapter's duty.
pub(crate) fn validate_buffer_descriptors(
    expected_layout: &[u32],
    buffers: &[CoreAudioBufferDescriptor],
    channels: usize,
    max_frames: usize,
) -> Result<CoreAudioBufferGeometry, CoreAudioSourcePhase> {
    let invalid = CoreAudioSourcePhase::Layout;
    if !(1..=MAX_BUFFERS).contains(&buffers.len())
        || buffers.len() != expected_layout.len()
        || !(1..=32).contains(&channels)
        || max_frames == 0
    {
        return Err(invalid);
    }
    let mut frames = None;
    let mut total_channels = 0usize;
    for (index, buffer) in buffers.iter().enumerate() {
        if buffer.channels == 0
            || buffer.channels != expected_layout[index]
            || buffer.address == 0
            || !buffer.address.is_multiple_of(std::mem::align_of::<f32>())
        {
            return Err(invalid);
        }
        let group_channels = usize::try_from(buffer.channels).map_err(|_| invalid)?;
        total_channels = total_channels.checked_add(group_channels).ok_or(invalid)?;
        if total_channels > channels {
            return Err(invalid);
        }
        let bytes_per_frame = group_channels
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or(invalid)?;
        let bytes = usize::try_from(buffer.byte_size).map_err(|_| invalid)?;
        if bytes == 0 || bytes > isize::MAX as usize || !bytes.is_multiple_of(bytes_per_frame) {
            return Err(invalid);
        }
        let count = bytes / bytes_per_frame;
        if count > max_frames || frames.is_some_and(|frames| frames != count) {
            return Err(invalid);
        }
        frames = Some(count);
        let end = buffer.address.checked_add(bytes).ok_or(invalid)?;
        for earlier in &buffers[..index] {
            let earlier_end = earlier
                .address
                .checked_add(earlier.byte_size as usize)
                .ok_or(invalid)?;
            if buffer.address < earlier_end && earlier.address < end {
                return Err(invalid);
            }
        }
    }
    if total_channels != channels {
        return Err(invalid);
    }
    Ok(CoreAudioBufferGeometry {
        frames: frames.ok_or(invalid)?,
        channels,
    })
}

/// Original generated block plus admitted prefix defines native coordinates.
pub(crate) fn checked_native_interval(
    report: RenderReport,
    admitted: usize,
    frames: usize,
) -> Result<(u64, u64), CoreAudioSourcePhase> {
    let invalid = CoreAudioSourcePhase::Extent;
    if frames == 0
        || admitted
            .checked_add(frames)
            .is_none_or(|end| end > report.frames)
    {
        return Err(invalid);
    }
    let first = report
        .start_frame
        .checked_add(u64::try_from(admitted).map_err(|_| invalid)?)
        .ok_or(invalid)?;
    let end = first
        .checked_add(u64::try_from(frames).map_err(|_| invalid)?)
        .ok_or(invalid)?;
    Ok((first, end))
}

/// Only the native adapter dereferences validated output descriptors. A copy
/// cannot fail: all groups and source extents have been checked beforehand.
pub(crate) trait CoreAudioSourceOps {
    /// An error means no publication began. Every success gets exactly one end.
    fn begin_publication(&mut self) -> Result<(), i32>;
    fn end_publication(&mut self);
    fn check_configuration(&mut self) -> Result<(), i32>;
    fn buffers(&mut self) -> Result<(&[u32], &[CoreAudioBufferDescriptor]), i32>;
    fn copy_channel_group(
        &mut self,
        buffer: CoreAudioBufferDescriptor,
        samples: &[f32],
        channels: usize,
        first_channel: usize,
    );

    fn history_request(&self) -> Option<SourceHistoryRetirementRequest> {
        None
    }

    /// Native overrides record cadence only around this actual new render.
    fn render_pending(
        &mut self,
        state: &mut NativeOutputState,
        frames: usize,
    ) -> Result<RenderReport, i32> {
        state.render_pending(frames).map_err(|_| INVALID_ARGUMENT)
    }

    fn publish_generated(&mut self, _state: &NativeOutputState, _report: Option<RenderReport>) {}

    fn delivered(
        &mut self,
        _report: RenderReport,
        _first_frame: u64,
        _frames: u32,
        _end_frame: u64,
    ) {
    }
}

fn failure(phase: CoreAudioSourcePhase, code: i32) -> CoreAudioSourceStep {
    CoreAudioSourceStep::Failure(CoreAudioSourceFailure { phase, code })
}

/// The sole retirement decision precedes every adapter operation, including
/// configuration checks and publication. A newly submitted request waits for
/// the next callback; an authorized request remains retired without effects.
pub(crate) fn source_callback_step(
    state: &mut NativeOutputState,
    slot: &SourceRetirementSlot,
    retirement_geometry: &AtomicU32,
    ops: &mut impl CoreAudioSourceOps,
    completed_history: &mut u64,
) -> CoreAudioSourceStep {
    match slot.decision_with(state, || {
        // The slot acquired the exact pending request before reading its
        // payload, and its CAS still compares that same request word.
        let fixed_frames = retirement_geometry.load(Ordering::Relaxed) as usize;
        if fixed_frames == 0 {
            state.pending_frames() == 0
        } else {
            state.pending_frames().is_multiple_of(fixed_frames)
        }
    }) {
        SourceRetirementDecision::Retire => return CoreAudioSourceStep::Retire,
        SourceRetirementDecision::Quarantined => {
            return failure(CoreAudioSourcePhase::Quarantined, INVALID_ARGUMENT);
        }
        SourceRetirementDecision::Continue => {}
    }
    if let Err(code) = ops.begin_publication() {
        return failure(CoreAudioSourcePhase::Publication, code);
    }
    let result = callback_in_publication(state, ops, completed_history);
    ops.end_publication();
    result
}

fn callback_in_publication(
    state: &mut NativeOutputState,
    ops: &mut impl CoreAudioSourceOps,
    completed_history: &mut u64,
) -> CoreAudioSourceStep {
    if let Err(code) = ops.check_configuration() {
        return failure(CoreAudioSourcePhase::Configuration, code);
    }
    let Some(format) = state
        .format()
        .filter(|f| f.encoding() == SampleEncoding::Float32)
    else {
        return failure(CoreAudioSourcePhase::Layout, INVALID_ARGUMENT);
    };
    let (layout, buffers) = match ops.buffers() {
        Ok(buffers) => buffers,
        Err(code) => return failure(CoreAudioSourcePhase::Layout, code),
    };
    let geometry = match validate_buffer_descriptors(
        layout,
        buffers,
        usize::from(format.channels()),
        state.max_frames(),
    ) {
        Ok(geometry) => geometry,
        Err(phase) => return failure(phase, INVALID_ARGUMENT),
    };
    // Keep the validated metadata stable across rendering and native copying.
    let mut descriptors = [CoreAudioBufferDescriptor::default(); MAX_BUFFERS];
    let count = buffers.len();
    descriptors[..count].copy_from_slice(buffers);
    let buffers = &descriptors[..count];
    let frames = geometry.frames;
    if state.pending_frames() != 0 && state.pending_frames() < frames {
        return failure(CoreAudioSourcePhase::Pending, INVALID_ARGUMENT);
    }
    // Refuse coordinate overflow before history mutation or a fresh render.
    let expected_interval = match state.pending_report() {
        Some(report) => checked_native_interval(report, state.admitted_frames(), frames),
        None => {
            let first = state.mixer().output_frame_basis().start_physical_frame();
            u64::try_from(frames)
                .ok()
                .and_then(|frames| first.checked_add(frames))
                .map(|end| (first, end))
                .ok_or(CoreAudioSourcePhase::Extent)
        }
    };
    let expected_interval = match expected_interval {
        Ok(interval) => interval,
        Err(phase) => return failure(phase, INVALID_ARGUMENT),
    };
    if let Some(request) = ops.history_request() {
        if state
            .apply_source_history_retirement(request, completed_history)
            .is_err()
        {
            ops.publish_generated(state, None);
            return failure(CoreAudioSourcePhase::History, INVALID_ARGUMENT);
        }
    }
    let report = match state.pending_report() {
        Some(report) => report,
        None => match ops.render_pending(state, frames) {
            Ok(report) if state.pending_report() == Some(report) && report.frames == frames => {
                report
            }
            Ok(_) => {
                ops.publish_generated(state, None);
                return failure(CoreAudioSourcePhase::Render, INVALID_ARGUMENT);
            }
            Err(code) => {
                ops.publish_generated(state, None);
                return failure(CoreAudioSourcePhase::Render, code);
            }
        },
    };
    ops.publish_generated(state, Some(report));
    let (first_frame, end_frame) =
        match checked_native_interval(report, state.admitted_frames(), frames) {
            Ok(interval) if interval == expected_interval => interval,
            _ => return failure(CoreAudioSourcePhase::Extent, INVALID_ARGUMENT),
        };
    let Some(sample_count) = frames.checked_mul(geometry.channels) else {
        return failure(CoreAudioSourcePhase::Extent, INVALID_ARGUMENT);
    };
    let pending_samples = state.pending_samples();
    let Some(samples) = pending_samples.get(..sample_count) else {
        return failure(CoreAudioSourcePhase::Extent, INVALID_ARGUMENT);
    };
    let source_start = pending_samples.as_ptr() as usize;
    let Some(source_end) = pending_samples
        .len()
        .checked_mul(std::mem::size_of::<f32>())
        .filter(|bytes| *bytes <= isize::MAX as usize)
        .and_then(|bytes| source_start.checked_add(bytes))
    else {
        return failure(CoreAudioSourcePhase::Extent, INVALID_ARGUMENT);
    };
    for buffer in buffers {
        // Destination ends were checked before rendering; source overlap is
        // checked against the whole unsent tail so a short delivery cannot
        // overwrite retained PCM needed by a later callback.
        let end = buffer.address + buffer.byte_size as usize;
        if source_start < end && buffer.address < source_end {
            return failure(CoreAudioSourcePhase::Extent, INVALID_ARGUMENT);
        }
    }
    let mut first_channel = 0;
    for buffer in buffers {
        ops.copy_channel_group(*buffer, samples, geometry.channels, first_channel);
        first_channel += buffer.channels as usize;
    }
    // Every native group is already written. An unexpected commit refusal
    // must quarantine this owner rather than replay an uncertain prefix.
    if state.admit(frames).is_err() {
        state.mark_admission_uncertain();
        return failure(CoreAudioSourcePhase::Admission, INVALID_ARGUMENT);
    }
    let frames = frames as u32; // Native byte_size bounds frames to UInt32.
    ops.delivered(report, first_frame, frames, end_frame);
    CoreAudioSourceStep::Delivered {
        report,
        first_frame,
        frames,
        end_frame,
    }
}

#[cfg(test)]
#[path = "coreaudio_source_callback_fixtures.rs"]
pub(crate) mod fixtures;
