# Share retained room state projection

Native room actor and browser driver snapshot export connect to one
pure Rust projection. Room metadata and peer prefixes retain immutable Arc data
until their accepted contents/sequence change. Actual session receipts accumulate
without using submitted commands as write or acknowledgement evidence.

Metadata-content revision stays separate from the browser driver's existing
accepted metadata-frame counter. Schedule and terminal authority remain with
their existing owners; projection never consumes startup permission or I/O.
Native dirty notification includes accepted earlier mutations if a later refresh
fails. One flag-aware implementation and a thin result wrapper preserve this
behavior without another projection algorithm.

## Evidence and known ceiling

The source writer returned a terminal stop report. Nine fixture groups are
authored: four pure projection, three actual driver and two memory-stream actor
cases. They reuse the genuine committed protocol setup with only cfg(test)
visibility widening. Coverage includes content/frame revision separation,
unchanged Arc identity, real relayed peer sequences, schedule preservation,
partial exhaustion notification and actual final-frame credit without ACK.
The driver lazily refreshes accepted
state and maps projection allocation/exhaustion into existing typed fatal errors;
the browser conversion still closes on refusal. Native schedule consumption and
clock/setup/drain/cleanup paths remain with their existing owners.
Both paired writers returned terminal stop reports before scoped formatting.
Four sequential compile-only checks exited zero: workspace/all-targets with
WebTransport, no-default-features WebTransport/all-targets, WASM browser/library
and WASM browser-audio/library. Host checks compile Rust fixture children; WASM
library checks cover the actual browser export path without test children.
Existing unused-code warnings remain. Whitespace checks completed; no assertions
were executed. Broader peer rosters and allocation fault injection remain deferred.
Existing browser snapshot DTOs and BigInt domains stay compatible. Actual browser
RoomCompetition and stream actor composition remain pending. Assertions, apps,
formal review and required QA remain deferred; compile checks do not establish
real network/timing, hardware, allocator or SQLite-grade acceptance. Full BMS
player Goal stays active.
