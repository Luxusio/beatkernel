//! Complete retained Source delivery shared by native ASIO and portable tests.
//! Native memory access remains in the adapter; this step validates the whole
//! planar write before any plane is touched and admits each prefix exactly once.
use super::AsioPcmEncoding;
use crate::audio::{
    source_history_retirement_mailbox::SourceHistoryRetirementRequest,
    source_output_retirement::{SourceRetirementDecision, SourceRetirementSlot},
    AsioPlanarLayout, NativeOutputLayout, NativeOutputState,
};
use beatkernel::audio::RenderReport;
use std::sync::atomic::{AtomicU32, Ordering};

const INVALID_ARGUMENT: i32 = 2;
const MAX_PLANES: usize = 32;

/// Numeric native metadata, never permission to dereference its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AsioPlaneDescriptor {
    pub(crate) channel: u32,
    pub(crate) encoding: AsioPcmEncoding,
    pub(crate) byte_size: usize,
    pub(crate) address: usize,
}
impl Default for AsioPlaneDescriptor {
    fn default() -> Self {
        Self {
            channel: 0,
            encoding: AsioPcmEncoding::Float32Lsb,
            byte_size: 0,
            address: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AsioPlaneGeometry {
    pub(crate) frames: usize,
    pub(crate) channels: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsioSourcePhase {
    Quarantined,
    Permit,
    Publication,
    Configuration,
    Layout,
    Pending,
    History,
    Render,
    Extent,
    Pcm,
    Admission,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AsioSourceFailure {
    pub(crate) phase: AsioSourcePhase,
    pub(crate) code: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsioSourceStep {
    Delivered {
        report: RenderReport,
        first_frame: u64,
        frames: u32,
        end_frame: u64,
    },
    Failure(AsioSourceFailure),
}

/// Callback-local, single-use permission for the exact serialized owner. The
/// native preflight stores this value, then fill takes it after event capture.
#[derive(Debug)]
#[must_use]
pub(crate) struct AsioSourcePermit {
    owner: usize,
}

#[derive(Debug)]
pub(crate) enum AsioSourcePreflight {
    Continue(AsioSourcePermit),
    Retire,
    Failure(AsioSourceFailure),
}

/// The only retirement decision. It precedes even native clock acquisition and
/// raw diagnostic publication. A later request is served on the next callback.
pub(crate) fn source_callback_preflight(
    state: &NativeOutputState,
    slot: &SourceRetirementSlot,
    retirement_geometry: &AtomicU32,
) -> AsioSourcePreflight {
    match slot.decision_with(state, || {
        let frames = retirement_geometry.load(Ordering::Relaxed) as usize;
        if frames == 0 {
            state.pending_frames() == 0
        } else {
            state.pending_frames().is_multiple_of(frames)
        }
    }) {
        SourceRetirementDecision::Continue => AsioSourcePreflight::Continue(AsioSourcePermit {
            owner: state as *const NativeOutputState as usize,
        }),
        SourceRetirementDecision::Retire => AsioSourcePreflight::Retire,
        SourceRetirementDecision::Quarantined => AsioSourcePreflight::Failure(AsioSourceFailure {
            phase: AsioSourcePhase::Quarantined,
            code: INVALID_ARGUMENT,
        }),
    }
}

/// Checks exact channel order, encoding, full planar extents and pairwise
/// disjointness. Actual native allocation validity remains the adapter's duty.
pub(crate) fn validate_plane_descriptors(
    expected: &AsioPlanarLayout,
    planes: &[AsioPlaneDescriptor],
    expected_frames: usize,
) -> Result<AsioPlaneGeometry, AsioSourcePhase> {
    let invalid = AsioSourcePhase::Layout;
    let channels = usize::from(expected.pcm().channels());
    if !(1..=MAX_PLANES).contains(&planes.len()) || planes.len() != channels || expected_frames == 0
    {
        return Err(invalid);
    }
    let mut frames = None;
    for (index, (plane, &(channel, encoding))) in planes.iter().zip(expected.channels()).enumerate()
    {
        let width = encoding.bytes_per_sample();
        if plane.channel != channel
            || plane.encoding != encoding
            || plane.address == 0
            || plane.byte_size == 0
            || plane.byte_size > isize::MAX as usize
            || !plane.byte_size.is_multiple_of(width)
        {
            return Err(invalid);
        }
        let count = plane.byte_size / width;
        if count != expected_frames
            || u32::try_from(count).is_err()
            || frames.is_some_and(|frames| frames != count)
        {
            return Err(invalid);
        }
        frames = Some(count);
        let end = plane.address.checked_add(plane.byte_size).ok_or(invalid)?;
        for earlier in &planes[..index] {
            let earlier_end = earlier
                .address
                .checked_add(earlier.byte_size)
                .ok_or(invalid)?;
            if plane.address < earlier_end && earlier.address < end {
                return Err(invalid);
            }
        }
    }
    Ok(AsioPlaneGeometry {
        frames: frames.ok_or(invalid)?,
        channels,
    })
}

/// Delivery coordinates refer to the original block's positively admitted
/// prefix, never the Mixer's newer generated frontier.
pub(crate) fn checked_native_interval(
    report: RenderReport,
    admitted: usize,
    frames: usize,
) -> Result<(u64, u64), AsioSourcePhase> {
    let invalid = AsioSourcePhase::Extent;
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

/// Statically supplied native operations. Copies cannot fail: all planes and
/// the entire interleaved prefix have been checked before the first copy.
pub(crate) trait AsioSourceOps {
    fn check_configuration(&mut self) -> Result<(), i32>;
    fn buffers(&mut self) -> Result<&[AsioPlaneDescriptor], i32>;

    /// Checks every publication/prepared-frame/fill counter before beginning.
    /// An error means no publication began. Success gets exactly one end call.
    fn begin_publication(&mut self, frames: u32) -> Result<(), i32>;
    fn end_publication(&mut self);
    fn copy_plane(
        &mut self,
        plane: AsioPlaneDescriptor,
        samples: &[f32],
        channels: usize,
        channel: usize,
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

fn failure(phase: AsioSourcePhase, code: i32) -> AsioSourceStep {
    AsioSourceStep::Failure(AsioSourceFailure { phase, code })
}

fn validate_prefix_samples(samples: &[f32]) -> Result<(), AsioSourcePhase> {
    if samples.iter().any(|sample| !sample.is_finite()) {
        Err(AsioSourcePhase::Pcm)
    } else {
        Ok(())
    }
}

fn admit_delivery(state: &mut NativeOutputState, frames: usize) -> Result<(), AsioSourceFailure> {
    if state.admit(frames).is_err() {
        state.mark_admission_uncertain();
        Err(AsioSourceFailure {
            phase: AsioSourcePhase::Admission,
            code: INVALID_ARGUMENT,
        })
    } else {
        Ok(())
    }
}

/// Consumes the preflight permission without reading the retirement slot again.
/// Metadata is copied to fixed local storage before any mutable adapter call.
pub(crate) fn source_callback_fill(
    state: &mut NativeOutputState,
    permit: AsioSourcePermit,
    ops: &mut impl AsioSourceOps,
    completed_history: &mut u64,
) -> AsioSourceStep {
    if permit.owner != state as *const NativeOutputState as usize {
        return failure(AsioSourcePhase::Permit, INVALID_ARGUMENT);
    }
    if state.admission_is_uncertain() {
        return failure(AsioSourcePhase::Quarantined, INVALID_ARGUMENT);
    }
    if let Err(code) = ops.check_configuration() {
        return failure(AsioSourcePhase::Configuration, code);
    }
    let Some(NativeOutputLayout::AsioPlanar(layout)) = state.layout() else {
        return failure(AsioSourcePhase::Layout, INVALID_ARGUMENT);
    };
    let planes = match ops.buffers() {
        Ok(planes) => planes,
        Err(code) => return failure(AsioSourcePhase::Layout, code),
    };
    let geometry = match validate_plane_descriptors(layout, planes, state.max_frames()) {
        Ok(geometry) => geometry,
        Err(phase) => return failure(phase, INVALID_ARGUMENT),
    };
    let mut descriptors = [AsioPlaneDescriptor::default(); MAX_PLANES];
    let count = planes.len();
    descriptors[..count].copy_from_slice(planes);
    let planes = &descriptors[..count];
    let frames = geometry.frames;
    let pending = state.pending_frames();
    if pending != 0 && (pending < frames || !pending.is_multiple_of(frames)) {
        return failure(AsioSourcePhase::Pending, INVALID_ARGUMENT);
    }
    let expected_interval = match state.pending_report() {
        Some(report) => checked_native_interval(report, state.admitted_frames(), frames),
        None => {
            let first = state.mixer().output_frame_basis().start_physical_frame();
            u64::try_from(frames)
                .ok()
                .and_then(|frames| first.checked_add(frames))
                .map(|end| (first, end))
                .ok_or(AsioSourcePhase::Extent)
        }
    };
    let expected_interval = match expected_interval {
        Ok(interval) => interval,
        Err(phase) => return failure(phase, INVALID_ARGUMENT),
    };
    // Descriptor validation bounds this positive count to u32.
    if let Err(code) = ops.begin_publication(frames as u32) {
        return failure(AsioSourcePhase::Publication, code);
    }
    let result = fill_in_publication(
        state,
        ops,
        completed_history,
        geometry,
        planes,
        expected_interval,
    );
    ops.end_publication();
    result
}

fn fill_in_publication(
    state: &mut NativeOutputState,
    ops: &mut impl AsioSourceOps,
    completed_history: &mut u64,
    geometry: AsioPlaneGeometry,
    planes: &[AsioPlaneDescriptor],
    expected_interval: (u64, u64),
) -> AsioSourceStep {
    if let Some(request) = ops.history_request() {
        if state
            .apply_source_history_retirement(request, completed_history)
            .is_err()
        {
            ops.publish_generated(state, None);
            return failure(AsioSourcePhase::History, INVALID_ARGUMENT);
        }
    }
    let frames = geometry.frames;
    let report = match state.pending_report() {
        Some(report) => report,
        None => match ops.render_pending(state, frames) {
            Ok(report) if state.pending_report() == Some(report) && report.frames == frames => {
                report
            }
            Ok(_) => {
                ops.publish_generated(state, None);
                return failure(AsioSourcePhase::Render, INVALID_ARGUMENT);
            }
            Err(code) => {
                ops.publish_generated(state, None);
                return failure(AsioSourcePhase::Render, code);
            }
        },
    };
    ops.publish_generated(state, Some(report));
    let (first_frame, end_frame) =
        match checked_native_interval(report, state.admitted_frames(), frames) {
            Ok(interval) if interval == expected_interval => interval,
            _ => return failure(AsioSourcePhase::Extent, INVALID_ARGUMENT),
        };
    let Some(sample_count) = frames.checked_mul(geometry.channels) else {
        return failure(AsioSourcePhase::Extent, INVALID_ARGUMENT);
    };
    let pending_samples = state.pending_samples();
    let Some(samples) = pending_samples.get(..sample_count) else {
        return failure(AsioSourcePhase::Extent, INVALID_ARGUMENT);
    };
    let source_start = pending_samples.as_ptr() as usize;
    let Some(source_end) = pending_samples
        .len()
        .checked_mul(std::mem::size_of::<f32>())
        .filter(|bytes| *bytes <= isize::MAX as usize)
        .and_then(|bytes| source_start.checked_add(bytes))
    else {
        return failure(AsioSourcePhase::Extent, INVALID_ARGUMENT);
    };
    for plane in planes {
        // Compare against the whole retained tail, so delivering one prefix
        // cannot overwrite PCM intended for a later native callback.
        let end = plane.address + plane.byte_size;
        if source_start < end && plane.address < source_end {
            return failure(AsioSourcePhase::Extent, INVALID_ARGUMENT);
        }
    }
    if let Err(phase) = validate_prefix_samples(samples) {
        return failure(phase, INVALID_ARGUMENT);
    }
    for (channel, plane) in planes.iter().copied().enumerate() {
        ops.copy_plane(plane, samples, geometry.channels, channel);
    }
    // All native planes have been written. An unexpected admission refusal
    // must irreversibly quarantine this owner rather than replay a prefix.
    if let Err(error) = admit_delivery(state, frames) {
        return AsioSourceStep::Failure(error);
    }
    let frames = frames as u32;
    ops.delivered(report, first_frame, frames, end_frame);
    AsioSourceStep::Delivered {
        report,
        first_frame,
        frames,
        end_frame,
    }
}

#[cfg(test)]
#[path = "source_callback_fixtures.rs"]
pub(crate) mod fixtures;
