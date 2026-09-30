# Owned device adapter registry

`beatkernel_platform::input` composes native raw acquisition with the core
`DeviceAdapter` and `PhysicalInputSink` interfaces. It does not read OS devices,
interpret unknown vendor bytes, normalize clocks, or apply gameplay policy.
Hosts explicitly register adapter factories, attach owned `DeviceDescriptor`s,
route borrowed acquired reports, and remove disconnected devices.

Every attachment constructs independent owned adapter candidates using registered
factories and invokes their existing `accepts` method. Exactly one match selects
that instance; multiple matches fail explicitly, independent of registration
order. Zero matches attach an unhandled raw device. Factories and adapter callbacks
are trusted mutable control-thread code: allocations, side effects, arbitrary
execution time and panics are their responsibility. Rejected selection or report
batches do not roll back callback/factory mutations. No real-time claim is made.
Registration identifiers are unique. Registry edits are refused while any device
is connected, so an attachment's deterministic routing cannot become stale.

Limits bound registrations, connected devices, lifetime identities, report/event
payload bytes, combined descriptor string bytes, and emitted events per report.
All limits have finite implementation ceilings. Identity history includes connected
and retired IDs: attachment reserves a slot, so removal never fails for retirement
capacity. IDs can arrive in arbitrary numeric order. Retired IDs cannot reconnect;
a new attachment needs a new globally unique acquired identity and obtains a fresh
adapter instance. Removal drops its per-device instance and descriptor. Host-held
bindings or gameplay held states require host lifecycle coordination separately.

Reports must name the explicit attached device, and the descriptor must advertise
raw HID acquisition. Per attachment, raw sequence numbers never decrease,
clock domain stays fixed and timestamps never decrease (equal timestamps are
valid). Equal sequence requires exact full acquisition metadata equality: native
packets can fan out distinct reports with the same acquisition ordinal. Devices
interleave independently; no global ID/sequence/time order is required. Checks
occur before callback. An admitted report records acquisition order before callback,
including an invalid emitted batch; pre-callback rejection does not update it.
Unhandled reports also record acquisition order. The core report has no packet
subordinal, so exact retransmissions cannot be distinguished from valid fanout
and are not deduplicated. Hosts own retransmission policy and must not assume a
failed callback batch can safely be retried. Reports route in received emission
order, including equal-meta native packet fanout.
Routing preserves the host's borrowed original report on both
success and failure. `Unhandled` requests an explicit host raw path; `Handled`
contains the complete accepted canonical batch, possibly empty. An adapter emitting
nothing does not implicitly fall back to guessing vendor semantics. Callers may
also retain the raw report alongside handled events for diagnostics or storage.

Every output must retain the report's *entire* `EventMeta` exactly, including source,
clock domain, timestamp, acquisition sequence, native provenance and original clock
point. Fanout uses equal acquisition metadata. Invalid metadata is rejected rather
than overwritten. All floating coordinates, pressure and quaternion components must
be finite; raw/custom payloads must fit the byte limit. Capability bits are descriptive,
so emitted kinds are not rejected for a missing semantic capability bit. Raw reports
emitted by an adapter are returned as data and never recursively routed.

The collecting sink bounds storage even if a trusted callback attempts excessive
fanout. Invalid or over-capacity emission withholds the entire batch with a concrete
emission index/reason; it never exposes a misleading partial batch. Callback mutation
still persists. Decoder-specific malformed report behavior belongs to the adapter;
the core trait has no decoder-error return, so zero emissions is not proof that a
report was valid. Authored vendor fixture/example decode actual bytes and then use
canonical `BindingMap`; tests and examples are not executed in this deferred-QA lane.

## Explicit native report framing

The portable `input::hid_report::normalize_report` boundary converts borrowed
native bytes and a separate integer report ID into the canonical report payload.
The caller chooses SeparateId when native bytes contain only payload, or LeadingId
when numbered reports include their ID byte. The latter requires an exact matching
nonzero prefix and strips exactly one byte; native ID zero denotes an unnumbered
report and leaves all bytes untouched in either mode. No byte-pattern heuristic
selects framing, because an ordinary payload can begin with the same byte as its ID.
IDs above 255 are rejected rather than truncated. The configured native-byte bound
includes any prefix and must be in 1..=16 MiB; the canonical copy reserves storage
fallibly. Empty unnumbered payloads remain raw data for decoder-specific validation.
The entire supplied EventMeta is copied without clock remapping or provenance edits.

Native envelopes may retain the original complete bytes beside the normalized
report. macOS raw acquisition uses this explicit framing conversion; Apple exposes
report ID, bytes, length and arrival timestamp separately in its callback ABI.
Sources: [Apple IOHIDBase header](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDBase.h)
and [IOHIDManager header](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDManager.h).
