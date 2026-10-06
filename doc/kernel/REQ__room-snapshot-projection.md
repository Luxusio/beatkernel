# Shared retained room snapshot projection

One unconditional pure Rust projection refreshes RoomSnapshot from the actual
accepted RoomPlayClient state. Both native RoomNetworkActor and the actual browser
RoomClientDriver/BrowserRoomClient snapshot export use it. The helper performs no
transport, clocks, UI, timers or schedule consumption. No new crate is required.

Retain exact participant ID, room phase/deadline, original ordered members/player
IDs, prepared flags and frozen-roster-ordered peer prefixes. Copy room data only
when metadata changes and peer data only when sequence changes; otherwise retain
the same immutable Arc allocations. Historical receipt bits accumulate only from
the actual session. Metadata revision changes only for changed room contents,
never peer traffic or polling. Checked exhaustion refuses before publishing a new
room allocation. Preserve accepted earlier evidence if a later projection fails.

Native actor uses the same changed notification and keeps its existing setup,
Leave/drain/clock/cleanup policy. Its genuine schedule remains supplied by its
existing RoomPlayIo start path. Projection never changes schedule or terminal
outcome fields. Browser driver gains a lazy retained snapshot accessor over its
actual live session; failure latches the existing typed facade failure and does
not manufacture new protocol acceptance. Its public revision() remains the old
accepted Admitted/Snapshot frame counter; distinguish that counter from retained
RoomSnapshot's metadata-content revision. Preserve peer materialization tokens,
write receipts, close behavior and one-shot schedule consumption.

When the existing driver take_start path returns a genuine schedule, retain that
same value in its cached snapshot; metadata refresh must neither take nor restore
start permission. No prepared roster or borrowed snapshot grants playback startup.
BrowserRoomClient.snapshot converts the retained room value into the existing
JS DTO shape and exact integer/BigInt domains. Failure to allocate/convert still
closes the binding as before. No JS command, Window rendering or async completion
contract changes. Browser/full RoomCompetition and stream actor integration remain
subsequent work; async writes must not be modeled as synchronous completion.

Author independent pure projection and actual driver/actor cases using accepted
protocol evidence, unchanged Arc identity, exact/full roster/sequence domains,
receipt accumulation, revision separation/exhaustion and schedule preservation.
Test-only helper visibility may be widened under cfg(test) to reuse the existing
committed protocol fixture without copying its assertions or setup. Assertions,
runtime, JS parsing, formal review and QA remain deferred. Scoped formatting and
four sequential compile-only checks follow both paired writers stopping. No real
network, hardware, allocation profiling or SQLite-grade stability is established.
Full BMS player Goal remains active.
