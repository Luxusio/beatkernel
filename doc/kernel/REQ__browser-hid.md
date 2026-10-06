# Browser HID acquisition boundary

Window owns browser-required permission and actual report acquisition. Retrieve
already authorized devices automatically; request new browser permission only
from an explicit user gesture. Application player assignment is separate from
the browser permission chooser. The optional acquisition component does not
establish playable HID bindings or cross-browser support.

Configure one to sixteen actual HID interfaces and one to 1024 payload bytes
per report. Start adapter identities at or above 3, reserving keyboard/touch
aggregate identities 1 and 2. Device IDs and the host's shared acquisition
sequence retain all unsigned 64 bits; reject exhaustion instead of wrapping. IDs distinguish device objects
without inferring hardware serial numbers. Preserve the event timestamp in the
Window host domain, separate report ID and exact DataView byte slice. Numbered
report payload already excludes the ID; never strip another byte or inject one.
Zero ID means an unnumbered report. Follow the
[official WebHID specification](https://wicg.github.io/webhid/).

Validate metadata and payload bounds before adopting acquisition sequence or
calling consumers. Native report callbacks only acquire bounded snapshots;
canonical encoding and descriptor/control interpretation belong off the main
thread. Do no rendering, DOM geometry queries, fabricated button events or
arrival-time substitution. The browser event timestamp is provenance, not an
inferred physical-device hardware timestamp.

Retain exact ownership through asynchronous discovery, opening, disconnection
and close. Discovery/open operations are serialized and concurrent requests are refused.
Close is terminal and idempotent, detaches listeners immediately,
awaits late openings and closes only interfaces opened by this owner. Partial
setup errors clean the acquired prefix; already externally opened resources
are refused and never closed. Surface cleanup failures. Device disconnection
notifies the host with original event time and source; do not fabricate judge
releases. A stopped owner cannot be revived by a late operation or callback.

Canonical BKPI raw report encoding retains source, acquisition sequence, Window
clock point (domain 0x57494e), native backend 0x57484944, report ID
provenance and exact separate-ID payload. Accept empty reports within configured
bounds. Canonical encoded packets use core BKPI v1 RawHidReport variant 5. Existing
keyboard/touch codecs and ownership remain unchanged. The raw payload length is
an unsigned 64-bit little-endian field, matching the core input codec; the browser
writes it with BigInt precision. With the current native provenance fields, the
payload begins at byte 68 for an unnumbered report and byte 69 for a numbered
report. Independent JS/Rust literals and core round-trip tests verify this wire
layout, including empty and 1024-byte reports. Reports alone have no
logical lane binding; descriptor/profile interpretation, actual page/Worker
forwarding, permission UI and gameplay stop policy remain required integration.
Fixtures and compile checks are source evidence. Device/browser/runtime and
performance acceptance require later execution under the active player Goal.

Permission filters accept at most sixteen nonempty dictionaries. Preserve the
WebIDL filter domains: vendorId is unsigned 32-bit; productId, usagePage and
usage are unsigned 16-bit. Require vendor for product and usagePage for usage.
An empty list is valid, while an empty filter dictionary is refused before the
native chooser. Revalidate active ownership after the external sequence hook
before delivering a report to consumers.

The [common report-profile contract](REQ__hid-profiles.md) supplies bounded
explicit button/axis interpretation through the shared DeviceAdapter/Registry.
Browser interpretation belongs on Worker using this same implementation.
Report profiles do not replace page forwarding or logical gameplay bindings.

## Browser runtime profile setup and input

Prepare at most sixteen distinct full-width HID sources (IDs at least 3) in
one configuration before activation or any gameplay processing. Every profile
physical control must match an existing Any or exact-source constructor binding;
setup never silently changes logical lane bindings. Complete device/profile
validation and typed-event storage reservation precede adoption. Setup is once
per gameplay owner, while keyboard/touch defaults remain unchanged.

An entirely empty setup is valid and installs zero sources without a synthetic
profile. Field or parameter rows without declared devices are invalid. Every
listed device must have a valid common profile with at least one real field.
The eventual host must check actual configured sources before enabling an HID
play session; the runtime setup API does not impose that page policy.

The portable browser setup uses the same common HidProfileAdapter as native
configuration. Both paths reuse the same acquisition-order check: reject sequence
regression, conflicting equal-sequence metadata, clock-domain change and time
regression. Invalid report decoding preserves prior levels and metadata order.
All events from one report retain the same original acquisition provenance.

The WASM binding accepts canonical genuine raw-HID packets using original Window
clock/input budgets. Decode the complete selected report before the first
Runtime call, then send typed events through the existing StepGameplay report,
pressed feedback, keysound and capture path in field order. A valid report with
no emitted transitions still sends its original unbound raw input through Runtime
so acquisition chronology and song advancement remain observed. Never fabricate
keyboard input. Capture retains actual bound typed inputs or advance operations
under existing semantics; it does not claim an additional raw-report archive.

A gameplay failure keeps the committed report prefix, fences the owner and
discards the unprocessed suffix without retry. Reusable bounded event scratch
is restored on success and refusal. API/compiled fixtures do not prove playable
HID: actual Window/Worker forwarding, device/profile UI and session disconnect
handling remain required integration.

## Numeric setup representation

Device rows contain six unsigned words: source low/high, vendor tag/value and
product tag/value. Tags are zero/one, absent values are zero and present hardware
IDs fit u16. Up to sixteen unique sources are allowed.

Field rows contain thirteen unsigned words in order: device index, report tag,
report ID, payload bytes, control kind, namespace/page, code/usage, bit offset,
width, bit order, field kind, flags and axis mode. Each row has two parallel
f32 parameters, scale and offset. The extent must be exact and at most sixteen
times 512 rows. Report tag zero requires ID zero; tag one requires nonzero u8.
Repeated field rows for one report must agree on exact payload length.

Controls reuse the seven-word physical-binding decoder: kind zero is HID usage
with u16 page/usage, one is native backend/code, two is vendor namespace/code.
Bit order zero/one selects least/most significant first. Field kind zero is a
button with zero/one inversion, axis mode zero and zero parameters. Kind one
is an axis with zero/one signedness, mode zero absolute or one relative, and
finite scale/offset. Kind two denotes an empty report and requires unused
control/bit/flags/mode/parameter values to be zero. Empty report rows are
exclusive and cannot duplicate or mix with real fields for that report.

Unused floating-point parameters use canonical positive-zero bits. Axis
parameters may retain either finite signed zero.

The shared profile validator enforces report/field capacities and bit extents.
Reject inconsistent extents, unknown device indices, conflicting IDs, invalid
unused values, unknown control kinds and profiles with unbound controls before
configuration adoption. Original packet report IDs/payloads remain separate
and are not modified by numeric setup.

## Worker setup and mixed gameplay reports

An optional live physical `play-start.hidSetup` contains `bindingWords`,
`deviceWords`, `fieldWords` as Uint32Array and `axisParams` as Float32Array.
Snapshot bounded arrays before asynchronous preparation. An enabled HID setup
requires one to sixteen distinct sources; the combined keyboard/HID constructor
bindings contain at most 256 seven-word rows. Use the numeric profile limits
above. Preserve full-width source/control words and validate actual profiles
with the Rust owner. HID bindings may cover lanes without keyboard bindings.
Replay and legacy input modes refuse HID setup.

Require both Rust HID APIs before consuming the prepared chart. Configure the
actual owner before capture, sample publication or activation. Preparation
reports `hidSourceCount` only when HID setup is supplied; absence omits it.
Configuration failure uses existing terminal
gameplay cleanup and publishes no successful preparation.

Worker preflights the complete bounded mixed keyboard/touch/HID batch before
the first Runtime call. HID events require an admitted source and retain their
original time, shared acquisition sequence, separate report ID and exact
payload in canonical raw packets. Call `input_hid_blob` for HID, `input_blob_at`
for touch and `input_blob` for keyboard. Pre-origin reports are validated then
ignored under the existing policy. Unknown sources or malformed events refuse
the whole batch. Actual gameplay failure preserves only the committed prefix
and terminates the owner without retry or fabricated compensation.

Window permission/profile setup, page launch forwarding and disconnect handling
follow the page contract below. Deferred fixtures and Rust
compilation do not verify JavaScript execution or playable browser HID.

## Page permission, profile launch and cleanup

Provide an optional controller profile file, HID enable control and explicit
browser authorization button. Unsupported browsers disable HID controls.
Selecting a profile enables HID; live HID play discovers already authorized
interfaces automatically without an application device chooser. Replay omits
HID. Permission requests begin synchronously in the explicit button gesture;
the temporary authorization owner joins late opens and closes its own handles.
Only one permission operation may run, and page-generation changes cannot
publish its stale result or revive an owner.

Window only inspects file metadata and actual acquired device metadata. Profile
file reading, UTF-8/JSON parsing, matching and numeric setup belong on Worker.
Profiles select eligible authorized devices automatically. Worker preparation
publishes full-width `hidSources` and their exact count; Window validates them
against the acquired interfaces. A matching device disconnect stops gameplay;
unmatched devices do not supply gameplay reports. Never manufacture releases.
Before admitted-source metadata exists, any acquired interface disconnection
cancels preparation because its passed device inventory may be stale.

HID reports share the existing bounded keyboard/touch acquisition sequence and
pending queue, preserve original timestamps and payloads, and use the existing
input pump. No callback rendering, DOM geometry, serialization or decoding.
Ignore reports outside the playing phase or from stale sessions. Stop detaches
HID listeners immediately and joins pending opens/owned closes, audio and Worker
cleanup. Surface cleanup failures and require reload when ownership is uncertain.

### Controller profile file version 1

A nonempty file is at most 1 MiB and contains strict UTF-8 JSON:
`{version: 1, profiles: [...]}`. There are one to sixteen profiles. Each profile
has optional u16 `vendorId`/`productId`, `bindingWords`, `fieldWords` and
`axisParams`; unknown properties and versions are refused. Matching no profile
ignores that acquired device; matching several refuses ambiguity. At least one
actual device must match. Unused profiles are allowed.

Binding rows have four u32 words: lane, control kind, namespace and code. Field
rows have twelve u32 words: report tag, report ID, payload bytes, control kind,
namespace, code, bit offset, width, bit order, field kind, flags and axis mode.
Each field row has two finite f32 parameters. Limit each profile to 256 binding
rows and 512 field rows. Validate numbers before typed conversion; never wrap
invalid integers or silently accept nonfinite float narrowing.

Worker prefixes fields with actual device indices and builds exact-source
bindings from acquired u64 identities. Combined bindings contain at most 256
rows and fields at most 8192 rows. Use the numeric runtime representation above
and actual Rust profile validation; do not invent a second report decoder.
File acquisition is single-read and validates actual returned extent. Check
session ownership after asynchronous acquisition before constructing gameplay.
Numeric setup and profile-file setup are mutually exclusive.

Source fixtures do not prove actual browser permission, device input, audio or
performance acceptance. Those remain deferred until execution is authorized.
