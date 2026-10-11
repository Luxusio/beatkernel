# Browser record storage Worker migration

Status: implementation in progress; independent code/security and full QA are
pending. This task is not complete and does not promote a WBS item.

The selected [ownership contract](../runtime/REQ__browser-record-storage.md)
moves existing record-store operations and replay/archive binary work into the
existing gameplay Worker. Window retains immutable File/Blob handles, bounded
metadata controls and the download bridge. Database version and canonical
recording formats remain unchanged. The reviewed plan includes explicit
opening/transaction terminal evidence and cancellation without releasing a
still-live physical operation.

Foundation and local development checks passed **41 tests / zero failures**:
actual RecordsStore source, shared immutable capture admission and exact local
member/budget validation. Evidence:
`target/wf/record-storage-foundations-development-12b87.log` (28 passed) and
`target/wf/record-storage-local-development-12b87.log` (41 passed). These are
focused coordinator development checks, not independent full acceptance.

The first Window integration check actually completed **256 tests / 245 passed /
11 failed**, exit one. Its seven new RPC cases passed. A new fixture clone walker
reacquired accessor values after structured cloning, producing four observation
reads instead of the original required one. Ten historical cases used an old
ascending fake catalog; production RecordsStore lists newest first. The fixture
correction must preserve single acquisition, exact original record selection,
member/archive bytes and replacement semantics. Original failure evidence:
`target/wf/record-storage-window-development-12b87.log`. After those fixture
corrections, the same two suites passed **256/256**, zero failures, exit zero:
`target/wf/record-storage-window-fixed-development-12b87.log`. This is scoped
development verification, not independent QA.

The first Worker integration development check completed **306 tests / 293
passed / 13 failed**, exit one:
`target/wf/record-storage-worker-development-12b87.log`. Twelve new protocol
cases mistakenly initialized an already initialized fixture; one duplicate
buffer case expected bytes from a different fixture branch. Corrections must
retain readiness checks, exact capture bytes and duplicate-buffer refusal.
After fixture correction, the same five Worker suites passed **306/306**, zero
failures, exit zero:
`target/wf/record-storage-worker-fixed-development-12b87.log`. All thirteen
new storage protocol tests passed, including delayed/missing native cleanup,
metadata-before-read admission and cancellation without stale mutation.
This remains scoped development evidence. Production
browser integration and actual immutable File cloning/lifetime remain pending.
Node endpoints explicitly model immutable File handoff because Node cloning can
yield Blob; this is not proof of Chromium behavior. Local ignored evidence under
`target/wf/` is not included in commits.

Independent pre-review auditing then identified overlapping uncancelable menu
preview body reads and missing direct replay/archive cancellation and raw
archive-alias assertions. A bounded preview acquisition slot and focused
regressions are being added. The prior 306-test pass predates this correction;
its evidence is retained. The two new menu/alias tests then passed (exit zero):
`target/wf/record-storage-menu-alias-development-12b87.log`. A focused final run
also included the original delayed-chart acquisition regression and passed
**3/3**, exit zero:
`target/wf/record-storage-menu-alias-final-development-12b87.log`.

The first scoped production runtime smoke reached genuine completion,
Worker save/load and historical presentation, delete and retained replay.
Its new lifecycle path reached a fresh Worker and retained capture save, but
the probe tried to screenshot an unprepared, invisible canvas and failed.
Actual exit one is retained in
`target/wf/record-storage-runtime-smoke-12b87-1.log` and its sibling evidence
directory. Correct the lifecycle-state observation before fresh verification;
this is neither full browser QA nor an acceptance pass.

After correcting that observation, a second scoped smoke reached all four
selected checks: genuine completion, history, replay and
record-cancel-reopen-retention. No scenario failure occurred; the fresh Worker
saved retained File/Blob handles and Window work counters stayed zero.
Evidence: `target/wf/record-storage-runtime-smoke-12b87-2/evidence.json`.
The existing probe deliberately retains the unexecuted full-suite checks as
unmet and exits one for selected-case runs. This scoped result therefore does
not constitute full browser acceptance or waive the strict audio gate.

The preceding audio task retains a strict production-browser failure: HTTP 200
and successful body consumption/compilation followed by `net::ERR_ABORTED`
without Worker CDP completion. The new storage migration neither diagnoses nor
waives it. Preserve the original strict diagnostics in affected production
probes and record any new run's actual outcome.

## Known ceiling

Browser storage is best effort and can be removed by the browser/user; explicit
download remains available. Missing native terminal events leave cleanup
unproven, with bounded failure rather than fabricated successful release.
Immutable File/Blob handles remove explicit large Window binary operations but
do not prove zero internal browser copies, aggregate memory limits or physical
input/audio latency. Other engines, real devices and the audio cancellation
remain broader Goal verification obligations.
