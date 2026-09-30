use super::{JudgeEvent, JudgeOutcome, JudgeProfile, JudgeStage, MissReason};
use crate::{
    chart::CompiledChart,
    input::{
        AxisMode, ButtonState, EventMeta, GameInputEvent, PhysicalControlId, PhysicalInputEvent,
        PointerMode, Position2, TouchPhase,
    },
    interaction::InputOwner,
    time::ClockPoint,
};

/// Canonical tagged, length-prefixed little-endian encoding, never Debug text.
pub(crate) struct Encoder(Vec<u8>);
impl Encoder {
    pub(crate) fn new(schema: &[u8]) -> Self {
        let mut out = Self(Vec::new());
        out.bytes(schema);
        out
    }
    pub(crate) fn finish(self) -> Vec<u8> {
        self.0
    }
    pub(crate) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    pub(crate) fn u64(&mut self, v: u64) {
        self.0.extend(v.to_le_bytes());
    }
    pub(crate) fn i64(&mut self, v: i64) {
        self.0.extend(v.to_le_bytes());
    }
    pub(crate) fn i128(&mut self, v: i128) {
        self.0.extend(v.to_le_bytes());
    }
    pub(crate) fn bytes(&mut self, v: &[u8]) {
        self.u64(v.len() as u64);
        self.0.extend(v);
    }
    pub(crate) fn option<T>(&mut self, v: Option<T>, write: impl FnOnce(&mut Self, T)) {
        if let Some(v) = v {
            self.u8(1);
            write(self, v);
        } else {
            self.u8(0);
        }
    }
    pub(crate) fn physical(&mut self, v: PhysicalControlId) {
        match v {
            PhysicalControlId::HidUsage { usage_page, usage } => {
                self.u8(0);
                self.u32(u32::from(usage_page));
                self.u32(u32::from(usage));
            }
            PhysicalControlId::Native { backend, code } => {
                self.u8(1);
                self.u32(backend.0);
                self.u32(code);
            }
            PhysicalControlId::Vendor { namespace, code } => {
                self.u8(2);
                self.u32(namespace.0);
                self.u32(code);
            }
        }
    }
    pub(crate) fn owner(&mut self, v: InputOwner) {
        self.u64(v.source.0);
        self.physical(v.physical);
        self.u32(v.game_control.0);
    }
    pub(crate) fn point(&mut self, v: ClockPoint) {
        self.u32(v.domain.0);
        self.i64(v.timestamp.as_nanos());
    }
    pub(crate) fn meta(&mut self, v: EventMeta) {
        self.u64(v.source.0);
        self.i64(v.timestamp.as_nanos());
        self.u32(v.clock_domain.0);
        self.u64(v.sequence);
        self.option(v.native, |out, native| {
            out.u32(native.backend.0);
            out.option(native.code, Self::u32);
            out.option(native.timestamp, Self::point);
        });
        self.option(v.original_clock_point, Self::point);
    }
    fn position(&mut self, v: Position2) {
        self.u32(v.x.to_bits());
        self.u32(v.y.to_bits());
    }
    pub(crate) fn input(&mut self, v: &GameInputEvent) {
        self.u32(v.game_control.0);
        self.meta(*v.physical.meta());
        match &v.physical {
            PhysicalInputEvent::Button(v) => {
                self.u8(0);
                self.physical(v.control);
                self.u8(match v.state {
                    ButtonState::Down => 0,
                    ButtonState::Up => 1,
                    ButtonState::Repeat => 2,
                });
            }
            PhysicalInputEvent::Axis(v) => {
                self.u8(1);
                self.physical(v.control);
                self.u32(v.value.to_bits());
                self.u8(match v.mode {
                    AxisMode::Absolute => 0,
                    AxisMode::Relative => 1,
                });
            }
            PhysicalInputEvent::Touch(v) => {
                self.u8(2);
                self.physical(v.control);
                self.u64(v.contact.0);
                self.u8(match v.phase {
                    TouchPhase::Down => 0,
                    TouchPhase::Move => 1,
                    TouchPhase::Up => 2,
                    TouchPhase::Cancel => 3,
                });
                self.position(v.position);
                self.option(v.pressure, |out, v| out.u32(v.to_bits()));
            }
            PhysicalInputEvent::Pointer(v) => {
                self.u8(3);
                self.physical(v.control);
                self.position(v.position);
                self.u8(match v.mode {
                    PointerMode::Absolute => 0,
                    PointerMode::Relative => 1,
                });
            }
            PhysicalInputEvent::Pose(v) => {
                self.u8(4);
                self.physical(v.control);
                for value in [
                    v.position.x,
                    v.position.y,
                    v.position.z,
                    v.orientation.x,
                    v.orientation.y,
                    v.orientation.z,
                    v.orientation.w,
                ] {
                    self.u32(value.to_bits());
                }
            }
            PhysicalInputEvent::RawHidReport(v) => {
                self.u8(5);
                self.option(v.report_id, Self::u8);
                self.bytes(&v.data);
            }
            PhysicalInputEvent::Custom(v) => {
                self.u8(6);
                self.u32(v.namespace.0);
                self.u32(v.type_id);
                self.bytes(&v.payload);
            }
        }
    }
    pub(crate) fn result(&mut self, v: JudgeEvent) {
        self.u64(v.object.0);
        match v.stage {
            JudgeStage::Instant => self.u8(0),
            JudgeStage::HoldHead => self.u8(1),
            JudgeStage::HoldTail => self.u8(2),
            JudgeStage::Custom(id) => {
                self.u8(3);
                self.u32(id);
            }
        }
        match v.outcome {
            JudgeOutcome::Hit { grade, delta } => {
                self.u8(0);
                self.u32(grade.0);
                self.i64(delta.as_nanos());
            }
            JudgeOutcome::Miss { reason } => {
                self.u8(1);
                self.u8(match reason {
                    MissReason::HeadTimeout => 0,
                    MissReason::TailTimeout => 1,
                    MissReason::EarlyRelease => 2,
                    MissReason::RejectedInput => 3,
                });
            }
        }
        self.i64(v.at.as_nanos());
        self.option(v.input, Self::meta);
    }
    pub(crate) fn chart(&mut self, v: &CompiledChart) {
        self.u32(v.ticks_per_beat());
        self.u32(v.initial_bpm().numerator());
        self.u32(v.initial_bpm().denominator());
        self.u64(v.objects().len() as u64);
        for object in v.objects() {
            self.u64(object.id.0);
            self.i64(object.time.start.as_nanos());
            self.option(object.time.end, |out, v| out.i64(v.as_nanos()));
            self.u32(object.interaction.0);
            self.u32(object.visual.0);
            self.option(object.audio, |out, v| out.u32(v.0));
            self.bytes(&object.metadata.0);
        }
        self.u64(v.bpm_changes().len() as u64);
        for marker in v.bpm_changes() {
            self.i64(marker.time.as_nanos());
            self.u32(marker.bpm.numerator());
            self.u32(marker.bpm.denominator());
        }
        self.u64(v.stops().len() as u64);
        for marker in v.stops() {
            self.i64(marker.time.as_nanos());
            self.i64(marker.duration.as_nanos());
        }
        self.u64(v.scroll_changes().len() as u64);
        for marker in v.scroll_changes() {
            self.i64(marker.time.as_nanos());
            self.i64(marker.velocity.numerator());
            self.u32(marker.velocity.denominator());
        }
    }
    pub(crate) fn profile(&mut self, v: &JudgeProfile) {
        self.i64(v.input_offset().as_nanos());
        self.u64(v.windows().len() as u64);
        for window in v.windows() {
            self.u32(window.grade.0);
            self.i64(window.early.as_nanos());
            self.i64(window.late.as_nanos());
        }
    }
}

/// Fixed FNV-1a 64-bit divergence diagnostic, not a security primitive.
pub(crate) fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |state, byte| {
        (state ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
