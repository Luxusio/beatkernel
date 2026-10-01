# Shared Linux finite playback

Linux local 2..64 playback now accepts the same --end-ns option as solo playback.
The owner preflights frame/clock representation, configures one finite mixer and
the same original-song boundary on every private runtime, and observes actual
native presentation alongside manual pause/resume. Player IDs remain independent
from assigned device IDs, with one Transport, audio producer and sample bank.

Completion requires every input source to drain, the actual committed global
merger frontier to reach the terminal native host boundary, no pending resume,
and every member to reach the original logical end. A backlogged keyboard blocks
advancement and completion. Pre-boundary events retain original ordering and
provenance; input at/after the terminal boundary cannot enter gameplay. Already
queued later input cannot prolong the finite session once earlier input is
drained. Partial reports, independent scores/captures, cancellation and native
stop/join/save cleanup retain their existing behavior; no unfinished notes or
full scores are fabricated.

Portable fixture source composes the real mixer, NativePause, NativeEnd,
RuntimeGroup, InputMerger, keyboard reconciliation, capture and ReplaySession
for 2/3/4/64 members with sparse/MAX player IDs and separately assigned devices.
It retains a queued keysound through a short resume, fences exact-end input,
blocks on backlog, preserves synthetic-release provenance and compares recorded
incomplete-prefix hashes. These fixtures are authored/compiled only.

Known ceiling: Windows/macOS finite ownership, UI practice-region intent and
restart, gapless playback, network endpoint/pause policy, ASIO presentation,
browser/full controls and native GUI/device/acoustic acceptance remain required.
Startup observation after an already-presented endpoint still lacks a valid
lower clock bracket and fails explicitly. Native host interpolation has Unknown
physical accuracy. Execution and formal close gates remain user-deferred.
