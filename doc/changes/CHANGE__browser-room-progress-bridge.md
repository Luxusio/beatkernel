# Browser room progress bridge

Expose common participant progress and local receipt status through the WASM
room client and Worker-owned channel adapter, using the existing bounded member
word format and actual read/write observations. No Window rendering loop or
parallel network owner is introduced.

BrowserRoomClient now publishes exact member words through RoomPlayClient,
retains one accepted peer identity until its DTO is consumed, and exposes
separate final write, aggregate ACK and local completion status. Metadata
revision remains independent of progress traffic. A facade failure cannot
report active local completion.

BrowserRoomOwner bounds and copies words before entering WASM, retains private
Prepared roster copies before exposing setup callbacks, and drains the accepted
peer token after each successful read. Receipt callbacks run only when one of
three local booleans changes. Peer ACK queries are on demand; I/O does not scan
all peers across the WASM boundary. Publication wakes the existing writer and
the lazy completion waiter rejects on cancellation without sending Leave.

Independent deferred Owner fixtures add five groups, sixteen total; all eleven
existing groups remain. Worker fixture mocks expose the added binding methods
while preserving all eighty-four groups. The fixtures cover bounded ownership,
full-width integers, pending-start peer data, callback mutation isolation,
real write/ACK barriers, completion without closure and joined cancellation.
They have not been executed or parsed.

Known ceiling: Application gameplay publication, multi-host competitive HUD and
coordinated room shutdown remain required; local completion is not permission
to close the whole room. Generated bindings, runtime/browser/audio/device/TLS
acceptance and formal review/QA are unverified. The full player Goal remains
active.

After both writers stopped, scoped Rust formatting and whitespace checks
completed. Compile-only checks exited zero for workspace/all-targets with
WebTransport, headless WebTransport/all-targets, WASM browser and WASM
browser-audio. Existing WASM cadence dead-code warnings remain. Compilation
does not establish JavaScript fixture, generated-binding or runtime acceptance.
