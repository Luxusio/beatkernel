# BeatKernel Phase 2: canonical physical input

The core `beatkernel::input` module represents physical input independently of
OS native structs, game controls and text characters. `DeviceId` is a caller-
assigned runtime identity, not a persistent fingerprint or authentication token.
Optional name, serial, VID/PID, transport and capabilities describe a device.
Two keyboards with identical descriptors can have distinct runtime IDs.

## Control and event semantics

Physical control IDs carry either a HID page/usage, a backend/native code, or a
vendor namespace/code. Keyboard usages use page 0x07. Unknown codes retain their
native identity rather than being mapped speculatively. Events remain typed:

- Button: Down, Up or Repeat.
- Axis: unmodified value and absolute/relative mode.
- Touch: surface control, ContactId, Down/Move/Up/Cancel, named x/y and optional pressure.
- Pointer: named x/y and absolute/relative mode.
- Pose: named x/y/z position and x/y/z/w quaternion.
- Raw HID: optional report ID plus exact owned payload bytes; the separate report ID is not included in the payload.
- Custom: vendor namespace, type ID and exact owned bytes.

Coordinates, axis units/ranges and pose conventions belong to the adapter/device
coordinate system. No clamping, contact merging or quaternion normalization is
performed. Contact identity is scoped by device and surface; lifecycle policy
belongs to interaction rules, not acquisition. Raw report parsing is adapter-owned.

## Metadata and clock normalization

Every variant carries source DeviceId, integer Timestamp, ClockDomainId and u64
source acquisition sequence. Optional native metadata preserves backend, original
code and native ClockPoint. Separate `original_clock_point` preserves the incoming
clock point during normalization without overwriting deeper native provenance.

The virtual backend has an explicit output clock domain. Same-domain input needs
no mapper. Different domains must pass the caller's ClockMapper; unavailable or
unrepresentable mappings return an error. On successful conversion only normalized
timestamp/domain change, and origin is set only when absent. Existing origin,
native metadata and all semantic payload fields remain unchanged.

## Virtual device queue

Registering a caller-assigned ID succeeds once per backend lifetime. Duplicate
active or retired IDs are rejected. Retirement makes a device unavailable for
future input while preserving previously accepted queued events. No global ID
allocator, wraparound, or reuse policy is hidden inside the backend; callers must
provide distinct session IDs for reconnects.

Events are accepted only from attached sources with nondecreasing sequence values
per device. Equal values are valid because one acquired raw report may fan out to
several semantic events. Decreasing values are rejected; gaps are allowed. FIFO
enqueue order is preserved across devices, equal sequences and equal timestamps.
Timestamp regressions are accepted and never cause queue sorting. Failed source,
sequence or mapping validation leaves the queue and last accepted sequences intact.
Drain returns accepted events in FIFO order; an empty queue is an ordinary result.
`drain_events()` removes the whole queued range: dropping a partially consumed
drain discards its remaining events, following VecDeque semantics. Use `pop()`
when remaining events must stay queued after a partial read.

DeviceAdapter receives device descriptors and raw reports and emits typed events
through PhysicalInputSink. Adapter fixtures demonstrate safe malformed-report
handling and button/axis fanout with unchanged acquisition metadata.

The virtual backend is single-owner fixture/control-thread infrastructure. Device
registration, payload creation and enqueue can allocate. It has no native I/O,
lock-free or real-time audio callback guarantee.

## Platform normalization and verification

Pure normalizers live in `beatkernel-platform` and compile on every host for tests.
Windows mapping accepts make code and None/E0/E1 prefix; unknown IDs encode the
prefix in the upper 16 bits and make code in the lower 16 bits. Linux evdev unknown
codes preserve their full u16 value and a distinct backend namespace. The macOS
HID helper preserves any provided page/usage, including nonkeyboard usages.

The Windows helper consumes complete physical scan codes, not individual Raw Input
packets. Pause is E1 plus 0x1D45 (or assembler-normalized E1 plus 0x45); a lone E1
0x1D header remains Native. Native acquisition must assemble the Pause sequence
before calling this helper. E0 0x46 (Ctrl+Pause/Break) and plain 0x54
(Alt+PrintScreen/SysRq) are explicit physical-key aliases.

Golden fixtures verify letters, numbers, left/right modifiers, keypad/navigation,
F1-F12, international keys, PrintScreen/Pause and unknown controls against literal
USB HID IDs. Additional tests check mapping uniqueness/ranges, two-device identity,
all semantic event variants, contact lifecycle data, raw adapter fanout, ordering,
retirement, validation atomicity and native/origin clock provenance. Existing time/
transport tests remain regressions. Debug/release tests, doctests, fmt, clippy,
documentation and a labeled virtual input-inspector example are required.

Source references: [USB HID usages](https://www.usb.org/sites/default/files/hut1_7.pdf),
[Windows RAWKEYBOARD prefixes](https://learn.microsoft.com/en-us/windows/desktop/api/winuser/ns-winuser-rawkeyboard),
[Windows HID/scan-code table](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-keyboard-input),
[Linux input codes](https://github.com/torvalds/linux/blob/master/include/uapi/linux/input-event-codes.h),
[Linux key transitions](https://docs.kernel.org/input/event-codes.html), and
[Apple HID usages](https://github.com/apple-oss-distributions/IOHIDFamily/blob/main/IOHIDFamily/IOHIDUsageTables.h).

Binding, native acquisition, chart/judge/audio/replay and real hardware validation
remain subsequent ordered phases of the full plan.md goal.

## Known ceiling

Known ceiling: Windows scan 0x2B cannot distinguish HID usages 0x31 and 0x32 and
uses conventional 0x31 — upgrade when native device/layout metadata permits that
distinction. HID-origin input retains its provided usage.
