# Canonical physical input binary codec

`beatkernel::input::{encode_event, decode_event}` provides a deterministic,
versioned little-endian blob for one complete PhysicalInputEvent. It does not
normalize clocks, interpret native codes, validate sensor coordinates, or change
physical input semantics. Device/source identity, acquisition sequence, normalized
clock point, native acquisition metadata and earliest original point remain exact.
A replay/container format can length-prefix the complete blob without borrowing
private judge hash encoders or relying on Debug text.

Callers explicitly construct CodecLimits with maximum encoded and opaque payload
bytes. Encoded capacity must hold the six-byte header and fit a Vec byte extent;
payload limit must not exceed encoded limit. There is no implicit unlimited or
default allocation policy. Encode/decode enforce encoded limits, independent
payload bounds, representable u64/usize lengths, checked cursor addition and
try_reserve_exact allocation failures. Truncated payloads are rejected before
allocating their declared size. Codec operations are control-thread work, outside
native/audio callbacks; they are safe Rust with no dependencies.

Version 1 wire layout (all integer fields little endian):

- Header: four bytes `BKPI`, then u16 version `1`, then u8 event variant.
- Variant tags: Button=0, Axis=1, Touch=2, Pointer=3, Pose=4, RawHID=5, Custom=6.
- Common EventMeta: source u64, timestamp i64 nanoseconds, clock domain u32,
  sequence u64, optional NativeEventMeta, optional original ClockPoint.
- Every option: u8 absent=0 or present=1, followed by its value when present.
- NativeEventMeta: backend u32, optional code u32, optional ClockPoint.
- ClockPoint: domain u32, timestamp i64 nanoseconds.
- Control: u8 HidUsage=0 then page u16/usage u16; Native=1 then backend u32/code
  u32; Vendor=2 then namespace u32/code u32.
- Button payload: control then state u8 Down=0, Up=1, Repeat=2.
- Axis payload: control, value f32 bits, mode u8 Absolute=0, Relative=1.
- Touch payload: control, contact u64, phase u8 Down=0/Move=1/Up=2/Cancel=3,
  position x/y f32 bits, optional pressure f32 bits.
- Pointer payload: control, position x/y f32 bits, mode u8 Absolute=0/Relative=1.
- Pose payload: control, position x/y/z f32 bits, quaternion x/y/z/w f32 bits.
- RawHID payload: optional report ID u8, payload length u64, exact payload bytes.
- Custom payload: namespace u32, type ID u32, length u64, exact payload bytes.

Each f32 uses its raw IEEE u32 bits, including negative zero, infinity and every
NaN payload. No quaternion normalization or finite-value rejection is applied at
this uninterpreted physical-input boundary. Use byte identity or to_bits when
checking NaN preservation; ordinary PartialEq treats NaN as unequal.

Decoding rejects unknown magic/version, invalid event/control/state/mode/phase
and option tags, truncated records, oversized/adversarial lengths, and any trailing
bytes. It consumes exactly one blob, with no hidden fallback to another schema.
Round-trip and adversarial fixtures cover all variants, all control forms,
metadata options, signed timestamp extremes and nonfinite float bit preservation.
Fixtures are authored and compile-checked during implementation; execution and
formal QA remain deferred by user instruction on 2026-09-30.
