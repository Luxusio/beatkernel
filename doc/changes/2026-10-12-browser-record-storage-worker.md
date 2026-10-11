# Browser record storage Worker migration

Status: implemented with independent code/security review and full Node tests
passing. Available actual Chromium storage probes pass; the full browser lens
is BLOCKED_ENV for an unavailable eligible trusted WebTransport endpoint. This
task is not complete and does not promote a WBS item.

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

First independent DEEP formal code review at `582b3e4` returned **FAIL**, with
two required corrections: canceled historical replay/archive acquisitions
could overlap, and sparse original-player arrays could bypass save preflight.
The independent security review returned PASS at that source; source changes
still require fresh applicable review before QA. Historical acquisition will
retain one physical slot through actual settlement and explicitly refuse busy
requests while retaining prior presentation. Save roster validation must visit
every array position before reading bodies. Matching contract/test corrections
are implemented. Fresh scoped verification passed **24/24** historical Worker
tests and **14/14** recording admission tests (exit zero, no skips/failures):
`target/wf/record-storage-history-remediation-development-12b87.log` and
`target/wf/record-storage-admission-remediation-development-12b87.log`.
Fresh independent formal code and security reviews then passed at `ee8d658`.
The full independent serial Node suite passed **1096/1096**, zero failures,
cancelled tests, skips or todo, exit zero:
`target/wf/record-storage-qa-cli-12b87-1/node-full.log`.

First independent browser QA returned **FAIL**. The full settings probe passed
its original journeys plus real Worker storage; strict audio request completion
passed in both Puppeteer and Worker CDP. Full render recorded **19/20** checks
passing, but the new local-member-history case omitted enabling its touch source
and timed out before play. Matching production input discovery respects that
checkbox, and the existing successful local-touch case enables it explicitly.
The new case now enables touch through the same public control; retain the
actual failure at `target/wf/record-storage-qa-browser-render-12b87-1/`.
Fresh affected review/browser verification remains required.

After enabling touch through the existing public checkbox, scoped actual
Chromium verification reached **local-member-history PASS** for both original
players `[1,2]`, with exact replay/archive save/load and historical association
assertions. No scenario failure occurred:
`target/wf/record-storage-local-history-development-12b87-1/evidence.json`.
As with earlier selected-case runs, the unchanged full-scope completion gate
exits one for unexecuted scenarios. Full affected browser QA remains required.

Settings evidence: `target/wf/record-storage-qa-browser-settings-12b87-1/`.
Its PASS does not prove permanent repair of the preceding audio abort. Strict
trusted WebTransport remains environment-blocked without an eligible endpoint;
the actual strict entrypoint exited one before launch for missing app URL, and
existing raw unknown-root evidence is preserved. Render local DeveloperMode
bootstrap is explicitly development-only, with no trusted QUIC acceptance claim.

## Final available verification

Independent DEEP code review passed after the public touch-control fixture
correction at `1cf6edf`. Fresh full Chromium render verification then passed
**20/20 checks**, exit zero. Both original local members `[1,2]` retained exact
replay bytes and their identical whole-roster archive through save/load and
historical presentation. Worker native transaction completion and Window
database/body/construction counters of zero were observed. Retained captures
survived Worker termination/reopening, and selected replay remained usable
after deletion. Evidence:
`target/wf/record-storage-qa-browser-render-12b87-2/evidence.json`; raw log:
`/tmp/record-storage-qa-browser-render-12b87-2.log`.

The independent browser lens returned **BLOCKED_ENV**, despite the available
local checks passing: strict WebTransport play still lacks an eligible
system-root endpoint and `WEBTRANSPORT_PLAY_APP_URL`. Existing preflight
evidence reports `QUIC_TLS_CERTIFICATE_UNKNOWN` and
`ERR_CERT_AUTHORITY_INVALID`. Local DeveloperMode room completion does not
substitute for that requirement. The task remains unfinished; no close or
global WBS promotion is justified. Its full serial Node result remains valid
for unchanged product/test sources after the test-only touch-control change.

The render main NetLog ends truncated and is preserved untouched. Complete
peer NetLogs contain teardown/QUIC cancellations, so render PASS is not a
blanket strict-network assertion. The unchanged settings evidence separately
passes its strict request gates, including HTTP 200, Puppeteer completion and
Worker CDP loading completion, with zero page/console/network errors. Preserve
the earlier audio-abort failure; one successful run does not diagnose or
permanently repair it. Evidence artifacts are ignored local outputs, not
committed verification receipts.

Independent scoped browser UX also passed actual save/history/delete/replay,
cancel/reopen retention, keyboard activation and disabled-control focus order.
Desktop 1200x1000 and mobile-width 390x844 screenshots were inspected; mobile
record controls had no horizontal overflow and showed visible keyboard focus.
Evidence: `target/wf/record-storage-ux-browser-12b87-1/`. This is scoped UX,
not a full QA PASS. The supplemental resize inspection reported software
WebGPU device loss while retaining preview and usable records; hardware GPU
behavior is unproven. Existing long settings-page placement remains UX backlog.

## Known ceiling

Browser storage is best effort and can be removed by the browser/user; explicit
download remains available. Missing native terminal events leave cleanup
unproven, with bounded failure rather than fabricated successful release.
Immutable File/Blob handles remove explicit large Window binary operations but
do not prove zero internal browser copies, aggregate memory limits or physical
input/audio latency. Other engines, real devices and the audio cancellation
remain broader Goal verification obligations.
