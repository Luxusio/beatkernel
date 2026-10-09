# Player fault isolation

Accepted local gameplay belongs to the actual BeatKernel Runtime. Optional
comparison publication and UI delivery must not rewrite immutable chart times,
accepted input metadata or judge transitions. A malformed local progress
snapshot remains a validation error; a peer/network failure may disable
comparison while retaining the latest valid local snapshot. Neither is evidence
that gameplay or output completed successfully.

## Separate admission and completion boundaries

Local snapshot admission, wire reservation, completed full-write receipt,
recipient delivery, final acknowledgement and coordinated drain are separate
events. A queued final prefix is not a completed write. A matching early ACK or
drain notice may be staged by the existing protocol, but completion remains
false until its matching write receipt and other required evidence arrive.
Duplicate/stale progress and forged receipts are rejected according to the
existing typed client/relay errors; rejection must leave their accepted state
unchanged. Delivery uses its original captured timestamp, not the later poll
time. A post-publication clock refusal cannot undo the effect already performed;
the caller must disable comparison before retrying that uncertain effect.

## UI and runtime ownership

Portable RoomCompetition coordinates its existing network, UI and runtime ports.
UI publication/reply refusal must remain inspectable and must not manufacture
accepted commands, successful finalization or results. Cancellation and failed
final ACK/drain/cleanup remain distinct from successful completion. Tests must
exercise this production controller; a test-local coordinator or an isolated
ScreenNavigator does not establish actual desktop route/owner cleanup.
For reply delivery, WouldBlock retains the exact pending reply without resending
its network command. A fatal reply error records the UI diagnostic, closes
controls and clears pending UI delivery; it must not admit later UI effects in
that service pass. Immediate local replies and delayed network replies obey
the same error distinction.

## Verification

Connected campaigns use real Runtime reports, actual client/relay wire frames
and the portable room controller. Only I/O effects and clocks are injected.
Bounded scripts pin independent literal event/state/receipt outcomes at each
checkpoint. Repeatability alone is insufficient.

Using Rust 1.98.1 and the existing local compiler environment:

```sh
cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked fault_schedule_fixtures -- --nocapture
cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked
python3 tools/wbs_status.py
```

The first actual campaign run passed six tests and failed one. It reproduced
the immediate fatal reply defect: expected the recorded reply refusal, observed
`ui_error == None`. Evidence is
`target/wf/player-network-ui-fault-schedules/baseline-campaigns.log`. The
controller now distinguishes WouldBlock from fatal immediate reply errors using
its existing delayed-reply policy. The final-source full app library regression
exited zero: 2,172 passed, zero failed and six ignored, including all seven
connected campaign tests. Evidence is
`target/wf/player-network-ui-fault-schedules/app-all-final.log`. Both new fixture
files pass Rust 2021 formatting and the complete change passes diff whitespace
checks. The compile emits the existing browser-menu motion dead-code warning;
the new unused import was removed before the final run. Independent review and
subsequent QA remain separate close gates.

The network campaign has three tests and uses actual Runtime object IDs101–103,
10/20/30ms song targets and Device41 sequences901–903, with literal grade7 and
zero delta. Its separate publication clock is never a judgement clock. Scripts
repeat from fresh owners, with original captures and independently pinned
client/relay states, write IDs, sequence numbers and receipt gates.

The UI campaign has four tests and uses actual Runtime object IDs41/42, exact
100/200ns targets and Device33 sequences1/2. Actual RoomCompetition orchestrates
injected I/O and wait effects; distinct UI request IDs are host-valid. Native
RoomControls owns request-ID validation, while the portable controller validates
network reply correlation. The tests do not claim native UI mailbox coverage.

## Known ceiling

Deterministic portable campaigns do not prove native transport/device behavior,
actual desktop/browser rendering, physical output cessation or acoustic timing.
The remaining allocation, short-write, spawn, panic and native fault matrix in
the [full WBS](../kernel/REQ__bms-player-work-breakdown.md) remains active.
