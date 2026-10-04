# Common room client progress receipts

Compose bounded local publication, participant peer prefixes and actual final
receipt ownership with the existing admission/clock/start client and native
timed stream driver. Preserve original observations and one external write-ID
space, including early-response/full-write barriers.

Implemented the portable progress client and composed it with RoomPlayClient
and RoomPlayIo. Upload sequences are assigned only on actual frame admission;
unsent ordinary publications coalesce, final publications remain immutable,
and peer ACK and own aggregate ACK credit require actual full-write receipts.
Genuine commitment activates progress; a pending Commit can stage peer data.
Explicit Leave fences progress immediately while draining its existing exact
outer write receipt before sending Leave.

Independent deferred fixtures add six common-client groups and two RoomPlay
groups, preserving all six existing RoomPlay groups. They cover actual Prepared
rosters, actual clock/start/relay composition, coalescing, refusal atomicity,
early ACKs, pending Commit and Leave during an in-flight final upload. Fixtures
have not been executed. Local receipt completion is not whole-room closure
authority. Browser/gameplay/HUD integration, native app room activation and
coordinated final shutdown remain required. No runtime transport/audio/devices/
browser, generated bindings or formal review/QA acceptance are claimed. The
full Goal remains active.

After both writers stopped, scoped Rust formatting and whitespace checks
completed. Compile-only checks exited zero for workspace/all-targets with
WebTransport, headless WebTransport/all-targets, WASM browser and WASM
browser-audio. Existing WASM cadence dead-code warnings remain; compilation
does not establish fixture or runtime acceptance.
