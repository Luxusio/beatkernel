# Prepared multi-host software-start coordination

The common RoomStartCoordinator composes the existing checked
StartAgreement and ClockFilter estimate types. Construction requires a valid
Prepared room snapshot with 2..64 immutable ordered participant leases and valid
local rosters. Each stream keeps its own estimate, real participant preroll and
one exact in-flight start message. The server reference owns no audio preroll.

After every actual ClockReady receipt and complete local readiness write, the
coordinator chooses one checked song target from the current server time, lead,
maximum participant preroll and maximum uncertainty. Proposal preflight covers
every frozen participant before publishing that target. No Commit may be emitted
until all exact Accepts arrive after their complete proposal writes; whole-room
committed state requires every complete Commit write. This uses common Join
agreements for client conversion, rather than another clock calculation.

Production and six independently authored fixture groups are saved. Fixtures use
actual Prepared Registry snapshots, ClockFilter samples and Join agreements for
2/3/4/64 hosts, with full-width IDs, heterogeneous offsets/prerolls, exact write
barriers, atomic refusal, 20-hour/week clocks, overflow, stop and bilateral
compatibility. No assertions have been executed. Mutations preserve accepted state on unknown leases, invalid echoes/ordering, stale
estimates, clock regressions and arithmetic overflow. Stop fences subsequent
mutators and start-authorizing committed queries while retaining historical
song-target evidence. A complete local write is not a remote application ACK,
and arbitrary partial delivery is not an atomic distributed transaction.

No live transport or gameplay activation has been connected in this milestone.
Actual probe/wire/server/client/Worker/page composition, multi-host progress and
final real ACKs remain required work. Hardware/audio timing, TLS, native/browser
interoperability and ranked authority are not established by a common reducer.
Production and fixture writers actually stopped before scoped Rust formatting.
Tests and runtime execution remain user-deferred. Four locked compile-only
checks exited 0 after that writer barrier: workspace all targets with webtransport
(94742), headless app all targets with webtransport (46615), WASM browser (91062),
and WASM browser-audio (11438). Existing WASM audio cadence dead-code warnings
remain. These checks do not typecheck active Windows/macOS device paths or
establish network, audio, browser or timing acceptance. Required ordered
review/security and browser/CLI/desktop QA remain mandatory before eventual close.
The full player Goal is active and the Harness task remains open/PENDING.
