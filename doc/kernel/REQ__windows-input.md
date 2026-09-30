# Windows Raw Input acquisition

This contract extends [canonical input](REQ__canonical-input.md) and
[device-aware binding](REQ__binding.md). Phase 4 must implement native Windows
keyboard/HID acquisition and an inspector. Its completion requires actual
Windows execution and device-attributed input evidence; portable packet fixtures
alone do not establish that. Later runtime/audio/platform phases and physical
latency benchmarks remain required by the original design.

## Ownership and identity

The application owns its window, message pump and process-wide Raw Input
registration. Constructing the acquisition backend never registers a class.
Explicit registration checks for existing matching classes, deduplicates the
requested usages and refuses takeover. The 4,096-class limit applies to distinct
usages after deduplication; repeated entries do not consume class capacity.
The application serializes registration
operations and keeps its current-process/current-thread window alive until
cleanup. The application grants exclusive ownership of those classes through
the guard's lifetime, closing it before any ownership transfer. Windows exposes
no registration token: identical same-window re-registration is not detectable.
Cleanup rechecks class, target and observable flags and removes only unchanged registrations,
with a null target; detectably changed successor registrations remain intact.
Windows may omit `RIDEV_DEVNOTIFY` from registered-class queries; presence or
absence of that bit is accepted, while other flag differences preserve successors.
Changing only that unobservable notification bit cannot be detected either.
Overlapping page-wide registrations also cause conflict refusal.

Each nonzero native device handle gets an independent, monotonic runtime ID.
Identical descriptions do not merge devices. Removing a device clears its held
keys and pending scan assembly; reconnecting allocates a fresh ID. A zero or
unregistered handle cannot silently become a shared fictitious physical source.
Already-produced events remain owned and valid after removal.

## Packet validation and reports

Safe portable code decodes literal Win32/Win64 layouts by checked little-endian
slices. Every header and keyboard field remains available for inspection.
Malformed sizes, unsupported types and incomplete payloads return explicit errors.
The declared packet size equals the supplied buffer length and cannot exceed
1 MiB. Native buffers are DWORD-aligned; packet bytes are never cast into native
union references.

HID size and count must be positive, their product must fit the declared payload,
and count cannot exceed 4,096. Native trailing union/alignment padding is allowed
but never emitted as report payload. Reports retain fixed-size boundaries and
acquisition order. `report_id: None` means no separate ID was extracted: `data`
contains the whole opaque wire report, including any embedded ID. It does not
claim that the descriptor lacks IDs. An adapter may extract a known ID separately,
using `Some(id)` with that byte excluded from the resulting payload.

Each accepted native packet increments a checked per-device acquisition sequence
once; HID fanout shares that sequence and receipt time. Validation, clock or
sequence failures leave sequence, held keys and assembly state unchanged.
Device count is bounded at 4,096; native string queries at 32,768 UTF-16 characters;
query/snapshot retries at three. Enumeration uses the actual returned count.
Device interface paths and serials are not default inspector output or friendly
names; any displayed device-provided name is escaped.

## Keyboard semantics

Known complete scan codes reuse the existing canonical HID table. Backend IDs
1–3 keep their current meanings. Raw Input native provenance uses BackendId(4)
and the complete `(flags << 16) | make_code`. Unknown flags or ambiguous prefixes
use that namespace for physical fallback, excluding break from physical identity.
Scan-zero VKey fallback uses BackendId(5), also excluding break; it does not use
text or layout to guess a physical key.

Make produces Down, a held same-source/control make produces Repeat, and break
produces Up. VKey 255 is intentionally filtered with visible batch status.
Keyboard overrun is an explicit rejected-packet diagnostic. Rejected packets can
be followed by valid input without consuming ordering or state.

Modeled Pause assembly is per source: E1 make 0x1d with Pause VKey 0x13 becomes
pending and emits no semantic event. The next accepted matching keyboard packet
must have make 0x45, VKey 0x13, and flags no-prefix or E1 without break. It completes
Pause using the completion packet's sequence/time/native fields. An unrelated
NumLock must not complete it. Accepted unmatched/filtered packets clear pending;
repeated headers establish new pending state. Other sources do not consume it.
Removal clears pending. Complete E1 0x45/0x1d45 uses the existing table. Pause
makes are untracked pulses, each Down; no Up is fabricated. Actual breaks remain
Up. Original packets remain inspectable when semantic output is pending/filtered.
These modeled fixtures are distinct from captured native evidence.

## Clock provenance and cleanup

QPC is sampled when acquisition begins, before native packet reading and device
queries. Frequency is cached at initialization. QPC counter/frequency conversion
uses checked i128 arithmetic and integer nanoseconds, truncating nonnegative
absolute values. Native time is converted absolute QPC time; normalized host time
is that value minus the converted initialization origin, in an explicit distinct
output domain. Thus rounding is the difference of converted absolute points.
Negative elapsed values are allowed by the pure mapping; invalid frequency,
counter, domain or i64 range fails explicitly.

Optional original `MSG.time` is retained separately as message-posted milliseconds
since system start, modulo 2^32. It is queue metadata, not a hardware timestamp
or a QPC-domain point; no value is fabricated when the caller lacks a message.
Canonical timing uses the explicit QPC receipt mapping.

Raw Input does not expose a hardware event timestamp. The inspector labels QPC
as **receipt time**, prints raw counter/frequency plus native and normalized
clock points, device ID, complete native keyboard fields, canonical control and
sequence. It makes no hardware-latency claim. Every foreground WM_INPUT reaches
DefWindowProc exactly once, including failed acquisition/decoding/mapping, before
an error propagates. Initialization/control-loop errors fail after resource
cleanup; recoverable packet diagnostics stay visible while input continues.
Acquisition, report ownership and console output may allocate; these APIs are
outside the audio real-time callback.

## Inspector and verification

The console example supports help, an explicitly synthetic `--fixture`, bounded
integer `--seconds` from 1 to 3,600 (default 10), and explicit `--hid page:usage`
collections in decimal or `0x` hexadecimal. Fixture mode rejects native options.
Native mode
uses an app-owned Windows target and finite message loop; non-Windows native mode
returns a clear unsupported error. Both normal and error exits unregister owned
classes and destroy the window. A stateless close callback posts quit instead of
destroying the window before guard cleanup, including synchronously sent close.
The inspector's bounded polling loop is a diagnostic, not a latency benchmark.
It prints the same descriptor fields for devices found initially, announced by
arrival notification, or first attached while acquiring a packet, once per
runtime device ID. This output excludes interface paths and serials.
HID output shows length and at most 32 preview bytes; the acquisition retains
the complete reports. No frontend framework is required.

Portable tests cover both layouts, lengths and caps, complete native fields,
opaque byte batches, identity/reconnect/exhaustion, make/repeat/break/Pause,
clock rounding and failed-retry atomicity, and two-device binding integration.
Windows Rust 1.83 all-target compilation, native API/error/lifecycle tests,
inspector execution and device-attributed WM_INPUT Down/Up are separate evidence
tiers. Guest boot, RDP or injected text alone proves none of the physical input or
latency claims. Required independent code/security review precedes CLI QA.

Primary references: [RAWINPUTHEADER](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputheader),
[RAWKEYBOARD](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawkeyboard),
[RAWHID](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawhid),
[registration](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerrawinputdevices),
[WM_INPUT cleanup](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-input),
[device enumeration](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getrawinputdevicelist),
[device information](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getrawinputdeviceinfow),
and [QPC](https://learn.microsoft.com/en-us/windows/win32/sysinfo/acquiring-high-resolution-time-stamps).
Queue metadata follows [MSG](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-msg)
and [GetMessageTime](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmessagetime).
