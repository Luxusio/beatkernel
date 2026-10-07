# Browser input isolation and note-page transfer primitive

Status: independent code, security and documentation review plus CLI and
interactive Chromium QA passed for this child. The broader player Goal remains active.
The first formal code review found a cancellation gap in the pre-existing
Gamepad owner: after a native getter closes the owner, later native fields could
still be read before publication was suppressed. Acquisition now checks the
existing closed/revision invariant after each native read and exits immediately
on invalidation. Direct owner and actual Window regressions verify early and
middle-slot cancellation, zero subsequent native reads/source or sequence
allocation/publication, and unchanged getter-once behavior when eligible.
The fresh focused Node run passes all 139 tests; the subsequent complete web
Node run passes all 540 tests, with no failures or skipped tests. Fresh independent
review verified the remediation without further findings.

Window input callbacks now send acquired data without synchronously polling
Gamepads. The existing 8 ms cadence performs eligible polling before input
dispatch, preserving owner/reentrancy/pending-tick guards and original sampling
metadata. Source discovery and preflight polls remain unchanged. A focused Node
run passed 135 tests and exposed one new-fixture setup sequence assumption; the
corrected named reentrancy test then passed. The fresh full Node run passes all
537 tests with no failures or skipped tests.
No measured latency superiority or delivery guarantee is claimed.

`NoteProgress` exposes allocation-free borrowed changed-page enumeration against
a retained acknowledged snapshot. Exact chart allocation is checked; unchanged
shared directories yield immediately without scanning pages. Changed directories
compare bounded page pointers and expose current packed states, valid extent and
derived completed count in order. The existing scalar `last_miss` remains
separate. Six new fixtures use 8,194 genuinely compiled unique objects and real
judge events, including an actual hold release. All 13 focused note-progress
tests pass. No existing progress behavior, global chart IDs, receiver/importer,
serialization format or WASM wire contract was introduced.
The fresh app library/bin run with desktop and WebTransport passes 2,183 tests
with zero failures and two existing ignored tests. Fresh independent CLI QA
also confirms these results and successful browser WASM type checking.
Current browser WASM type checking also passes; it does not prove hardware or
complete renderer-worker isolation.

Interactive Chromium QA recorded 13 trusted keyboard/touch/capture callbacks
with zero Gamepad reads, 321 eligible cadence reads, two setup/preflight reads
and one QA environment probe, and eight acquired DTOs preserving original
timestamps and metadata.
Holding an actual Worker ACK suppressed polling and retained queued input until
release. Stop, restart and saved-prefix replay preserved ownership; a software
polling exception exercised explicit cleanup. Desktop/mobile screenshots were
inspected. Evidence is local under
`target/wf/browser-input-isolation/qa-browser-renew-1/`; physical HID/Gamepad
acceptance and a latency guarantee are not established. The 900 ms post-start
wait used by this scoped QA does not resolve the parked startup timing failure.

The selected renderer boundary reuses immutable chart data and COW pages with
one bounded snapshot in flight and cumulative changes since the last complete
ACK. Actual separate renderer Worker integration for all presentation modes is
queued as `TASK__browser-render-worker`. This primitive and ADR do not themselves
move rendering out of the gameplay Worker. Existing audio-authority startup and
late-touch QA failures remain parked, unwaived and unresolved.
