# Player loading latency

This increment is in progress. The PCM foundation now supports explicit cold
sharing while preserving deep clones, independent mutable extraction and
per-SampleId logical capacity. Focused development verification passed 66 tests
across `pcm_sharing`, `pcm_clone`, `audio_pcm`, `audio_mixer`, `section_restart`
and `audio_rt_alloc`. The shared and copied banks produced identical actual
Mixer output; the tested render interval performed no allocation, reallocation
or deallocation. This is scoped development evidence, not full player QA or a
comparative latency measurement.

The common preparation candidate now uses one cooperative path for immutable
original reuse, fresh replay/section validation and resource-level visual work.
Portable integration verification passed 147 tests with zero failures and two
existing ignored actual-codec tests requiring explicit tool configuration.
These include 24 new reuse/control tests, existing preparation, selected-file
planning, replay and movie-registration/frame-supply regressions. The first
desktop-enabled run passed 35 existing tests and seven new tests, then failed
three visual fixtures because their selected-file inventories omitted the
chart. Registering genuine chart bytes corrected those fixtures; production
source validation was preserved. Complete logs remain under
`target/wf/player-loading-latency/`. Image regressions passed 26 tests, source
admission passed 10, mine assets passed five, and selected-source preparation
passed nine. Filtered unit groups overlap; these counts are not a unique total.
The test-only compatibility bank loader retains deep-clone semantics. The real
pipeline's older alias test now checks shared immutable pointers, unchanged
logical bytes/IDs/read/decode counts, independent mutable extraction and
independent uncached custom-decoder preparations; its corrected case passed.

The native helper lane owns player/native-chart attachment APIs; the dependent
desktop lane owns production `Desktop` cache propagation through initial,
replay and retry workers. Browser/native integration, measurements and required
independent review/QA remain unfinished. WBS remains 90/193 verified.

Integration exposed two common API gaps: dependency lookup must transfer the
same parsed original into preparation, and consuming a prepared value must not
force cache publication before fresh derived validation. Additive owned-plan
and revocable-admission APIs address these gaps in the source candidate. Their
first integration execution passed as recorded below; the subsequent inventory
correction and additional browser cases await fresh execution. Native
admission follows fresh section, lane and policy validation; native visual
registration remains a separate uncached stage. Browser admission follows full
prepared-value validation. The actual browser runner's held-read polling now
owns its deadline rather than leaving a live loop after a raced timeout; syntax
validation passed, and real browser execution remains pending.

The first continuation integration run completed 83 tests with zero failures:
32 common reuse/control, 10 connected native preparation, five portable browser
preparation, nine dependency/inventory, and 27 existing preparation regressions.
This proves the tested deferred-admission and shared-plan cases. The subsequent
equal-revision inventory-owner correction and five additional browser cases
await a fresh run; new WASM compilation and generated-export execution remain
separate pending checks. Native partial-catalog and production Desktop cache
propagation are now in development, not covered by those 10 helper tests.

The new browser-feature WASM library type check then exited zero, including the
inventory-owner token and incremental preparation/export API. It reported ten
existing feature-dependent app warnings and three platform cadence warnings;
this is compiler evidence, not generated-binding or browser execution. Browser
progress/Cancel integration and native partial-catalog work proceed on disjoint
paths. Required full fresh export, integration, GUI and ordered review/QA gates
remain pending.

The complete immutable baseline is recorded in
`target/wf/player-loading-latency/baseline-e421b59/manifest.json`; all 91
artifacts were independently hash-checked before source activation. Capturing
these artifacts does not constitute a baseline timing trial.

The fresh inventory/catalog integration run passed 93 tests: nine dependency
inventory, ten browser preparation, ten native preparation, five streamed
catalog, 27 preparation regressions and 32 reuse/control cases. The compiled
native unit executable separately passed seven catalog and three preparation
attachment cases. Desktop loading fixtures initially passed four and failed
two because they omitted the real initial `begin_selection` transition and
conflated existing navigation blur with font publication. Independent source
inspection confirmed those fixture preconditions; correcting them preserved
cache, selected-path and focus assertions. The retry passed all six cases.
These 109 distinct cases are development verification, not independent QA.

Fresh browser release WASM and bindings were generated. The first four-target
Node run executed 252 tests, with 66 passing and 186 failing. Most failures
came from the older Window VM import whitelist rejecting the real keyboard
helper; additional failures exposed missing encoding globals and invalid
settings/section/replay fixture setup. The authentic replay export fixture now
stops its captured runtime before export and declares the rank required by its
explicit policy. Its five generated-export tests passed against current WASM
SHA256 `fed1cea17c59e51d22c12237ec8be864b4dc1f121049b05761b5d068f2fd6ddc`.
After fixture corrections, all 178 Window host cases passed. The earlier full
247-case run already passed the other 69 Worker/loading-state cases; together
with five export cases, all 252 distinct targeted Node cases now have passing
execution evidence. The final 247 were not rerun as one combined invocation.
The retained-preview correction preserves stale presentation/status fencing
and explicitly checks progress visibility and usable Cancel controls.
The current actual
Chromium functional runner passed all 11 steps with ten screenshots and zero
page errors. It observed catalog availability before source reads, real
BMS/WAV preparation, progress while a selected File read was held, trusted
keyboard cancellation before that read settled, retained preview, recovery,
cache reuse, library replacement, keyboard catalog navigation and pending-page
retirement. Pending and cancelled screenshots were directly inspected. Owned
browser and Xvfb exited zero; server and owned directories were cleaned up.
Evidence is under `target/wf/player-loading-latency/current-browser-development/`.
The gameplay WASM hash above and Worker SHA256
`3da44e6b07d784ad1296304a97918e54ccad2b162647f7181044ade5de8d1013`
identify the executed artifacts. This is development verification; the required
independent qa-browser lens still follows formal review. No comparative
five-trial latency result or independent full-task PASS is claimed.

The loading report consumer and its eight actual CLI tests now pass. Its first
run passed four and failed four because fixture trial IDs were reused between
groups. The corrected fixture assigns globally unique IDs and adds explicit
duplicate-ID refusal coverage; the rerun passed eight with zero failures.
Nearest-rank boundaries, five-trial minimum, request/event correlation,
readiness ordering, artifact/configuration mismatch, cold/warm ownership and
unavailable baseline measurements are covered. These synthetic CLI correctness
fixtures are explicitly not player performance trials. Logs are
`loading-report-development.log` and `loading-report-retry1.log` in the task
evidence directory. The integrated preparation/catalog source and focused
functional tests are committed as `acac653`; measurement runners and independent
full-task review/QA remain in progress.

The first complete paired browser measurement passed all 30 owned Chromium
processes and 70 records: seven scenarios with five before and five after
observations. The strict report consumer accepted the actual records. Evidence
is `browser-latency-development-1/{trials,observations,report}.json`; the runner
serves the complete frozen baseline tree and hash-checks each served artifact.
Times begin at the observed Worker request, excluding browser/page bootstrap.
Diagnostic File gates, trusted CDP input, screenshot and observer costs are part
of this controlled software workload; results do not establish physical latency.

The first run exposed a repeat-preparation regression despite actual warm
codec and original preparation parse/compile counters being zero. Fresh derived
replay, invisible and mine validation work is outside these original counters
and still executes when required. Prepared p50 in milliseconds was
20.085→39.055 for retry, 15.305→34.805 for section and 31.495→50.495 for replay.
Warm preview improved 43.100→39.880, while cold large-audio readiness was nearly
unchanged at 423.520→422.980. Tail changes are retained in the full report;
these results cannot support a blanket speedup claim. Repeated zero-delay timer
yield overhead is the working hypothesis. The source candidate now uses one
preparation-owned MessageChannel, retains the Ready task boundary, and closes
both ports on completion/cancellation. Actual Worker tests passed 67/67 and a
scoped group passed 28/28 (overlapping counts). The first full Worker run failed
two old mock fixtures that awaited only Promise jobs; waiting for actual task
delivery preserved their assertions and the rerun passed. Current functional
browser verification again passed all 11 steps on Worker SHA256
`dfd31da8a70989471a07d38c8f85a3336b560a150435427ed39c43937a75b369`;
comparative remeasurement remains pending.

The current-only native fixture passed one parent test containing five fresh
test processes, actual X11 windows and ten PNG captures. Font preparation was
held while real XTest keys, resize events and renderer submissions were
observed. Raw records separate renderer setup, catalog admission, submitted
usable frame, delivered key/resize and font publication. Prepared/audio
readiness is explicitly null. These records under
`native-connected-development-1/` are structural native evidence, not
before/after comparison. The preserved current production binary has SHA256
`dc309ac7b84b57816cbb71d6bbad9d9980a2d9f45f8aa5d88f216693a602415f`.
Native comparative preparation/output measurements and final independent
review/QA still remain unfinished.

The MessageChannel correction is committed as `0057f84`. The complete second
paired browser run again passed 30 processes and all 70 records; the strict
report accepted them. `browser-latency-development-2/` preserves raw events,
artifact hashes, 70 screenshots and the complete report. Worker hash is
`dfd31da8a70989471a07d38c8f85a3336b560a150435427ed39c43937a75b369`;
gameplay/audio WASM remain unchanged from the freshly built artifacts.

Prepared-resource times in milliseconds, before→after, each five samples:

| Workload | p50 | p95/max |
| --- | --- | --- |
| Warm preview | 41.270→15.095 | 61.245→23.220 |
| Warm retry | 20.280→7.360 | 22.500→8.045 |
| Section | 15.305→4.935 | 17.290→11.960 |
| Replay | 33.480→22.680 | 37.595→25.600 |
| Cold large audio | 435.610→452.365 | 506.675→476.910 |
| Library/content replacement | 380.445→387.005 | 420.015→437.855 |

Catalog's first observed submitted usable frame p50 was 74.020→72.575ms,
with p95/max 78.745→76.010ms. With five observations nearest-rank p95 equals
the maximum. Warm decoded-original reuse recorded zero codec and original
preparation parse/compile counters in every current warm scenario. Fresh
derived/replay validation work is outside these counters; historical internal counters remain
unavailable. The correction removes the observed repeat-preparation regression,
but cold/replacement medians and some audio-setup tails do not improve. The
controlled File gate and probe overhead remain included; no universal speedup,
page-startup performance or physical-output conclusion follows from this run.
While large-media acquisition was held, current main-thread key dispatch p50
was 0.415ms (p95/max 0.610ms), compared with 0.420ms (0.545ms) before. The
resize-to-renderer-geometry acknowledgement median rose 7.890→18.340ms while
p95/max fell 28.795→18.830ms. This is submitted-geometry acknowledgement, not
physical display latency; the new UI also retains a visible preview where the
old preparation route hid its canvas. These costs remain visible in the report.

The preserved native binaries passed a separate catalog comparison: five fresh
processes per variant, ten owned windows and ten captures. The same external
X11 observer required Down to change selection and Up to restore it before
admitting a usable-frame observation. Raw records and sampling costs live under
`native-paired-catalog-development-2/`. Baseline binary SHA256 is
`235816a648aac5500ea248456948627560020f0ec9fc69d6ba9cdf759148288c`;
current binary SHA256 is
`dc309ac7b84b57816cbb71d6bbad9d9980a2d9f45f8aa5d88f216693a602415f`.
Fixture bytes, observer hash and arguments matched across variants.

Observed native catalog times in milliseconds, before→after, five samples:

| Observation | p50 | p95/max |
| --- | --- | --- |
| Candidate selected frame | 1100.183→1164.501 | 1252.206→1177.613 |
| First proven usable frame upper bound | 1126.157→1184.415 | 1278.341→1196.943 |

The usable-frame median rose about 58ms and the maximum fell about 81ms.
These two-chart software-Vulkan startup observations include process launch,
input probes, X11 sampling and observer overhead. OS page/shader caches were
not controlled. They do not prove native preparation/output readiness or a
universal startup improvement.

The first paired attempt failed when a discovered X11 window disappeared
before `X_QueryTree` (`BadWindow`). Its log is retained as
`native-paired-catalog-development-1.log`. The helper now skips only a
`BadWindow` matching the transient discovery target; all other errors and
errors while sampling, entering input or capturing a confirmed owned window
remain fatal. Fresh compilation and the ten-process retry passed. The fixture
is registered in `desktop.rs` and remains an opt-in ignored test.

A second, explicitly bounded native ALSA-null replay diagnostic with
`--preroll-ns 0 --seconds 1` exited after 84 renders and 21,504 submitted
frames, with zero xruns/failures. It admitted zero commands and retained song
position zero for the empty capture. This verifies the bounded software-output
path, not natural replay completion or physical presentation. The prior
20-second timeout remains preserved alongside
`native-null-replay-bounded-development.log`.

## Known ceiling

The native preparation comparison passed ten fresh preserved production
processes, each with a cold launch and a real F5 retry after the joined evdev
ioctl error. `native-paired-preparation-development-2/` contains ten raw records
and twenty owned-window captures. Exact configuration/fixture hashes matched;
every process produced exactly two correlated preparation markers. The actual
16 MiB mono WAV has four logical aliases (128 MiB decoded PCM), with explicit
44.1 kHz mono output and `--channel-policy exact`.

Input-to-prepared-log observed upper bounds in milliseconds, before→after:

| Workload | p50 | p95/max |
| --- | --- | --- |
| Cold large native audio | 2216.706→2236.450 | 2353.370→2599.100 |
| Same-owner native F5 retry | 2253.290→1878.084 | 2408.079→2073.233 |

Cold median/tail regressed in this workload, while retry improved. These times
include OS input delivery, fresh derived completion/compile and stdout
observation, not codec-only durations or output readiness. All native trials
intentionally end with an actual mandatory `/dev/null` evdev ioctl failure;
`playable=false` and `audio_ready=null` are retained.

The first native preparation attempt failed before any marker because omitted
channel-policy was added after the launch was pinned. Its actual error frame
and 126.65-second observation timeout remain under
`native-paired-preparation-development-1/`. The comparator now supplies the
same complete explicit configuration to both preserved binaries and rejects
that exact pin error immediately. Source inspection confirmed the ordering
defect also affects ordinary native default resolution; its production
correction and independent regression tests are in progress, not yet verified.
The first compile attempt also found a test-only private glyph reference;
using the existing public font atlas fixed it, and subsequent native fixture
compilation passed. All failed logs are retained.

Known ceiling: native preparation markers measure observed upper bounds only;
they do not prove audio readiness. Native section/replay/replacement comparison
and actual native presentation/output acceptance remain unfinished.

The production default-resolution correction subsequently passed nine
`session_launch_preparation` cases and five real worker/TLS/publication cases.
It resolves actual native options on the game worker before pinning or invoking
the platform launcher. Requested lineage remains immutable, while one accepted
retry template preserves resolved device/output settings and recording bases.
Discovery failure can recover on a later F5; accepted retries do not rediscover.
Identity and explicit-option drift, conflicting accepted publication,
recording/bookmark/loop/rebase/member projection and cancelled discovery are
covered by the independent test author. The first worker run passed four cases
and failed a fixture that expected normal cancellation to return an error;
the corrected test retains cancellation/Finished and no-pin/no-native checks,
and all five passed on rerun. Both logs remain preserved.

Fresh production binary SHA256
`bef6dad979eb0cb73aec94da5b8d5841653512b00a1eab3c8bf9a0ea6b39ec6d`
passed the actual current-only missing-channel-policy GUI proof: one owned
process, real Enter and F5, exactly two preparation markers and two joined
mandatory-ioctl error frames. Its records under
`native-default-resolution-gui-development-1/` deliberately keep
`audio_ready=null`. The preserved manifest records the uncommitted production
source hashes. This proves the repaired preparation/launch boundary, not
successful hardware gameplay, comparative speed or formal QA.

The broader Desktop unit run returned337PASS/2FAIL/11ignored. Both failures
were older whole-catalog/font atomic-readiness expectations that contradict
incremental admission. Their replacement fixtures preserve pending-profile
blocking and prove partial paths/actions, diagnostic contents and
path/query/focus continuity through actual font publication. The fresh full
rerun passed339/failed0/ignored11 in3.44s
(`desktop-default-resolution-full-unit-development-2.log`, exit0);
the original failing suite is retained.

The existing portable SessionLaunch library regressions also passed13/failed0
(`session-launch-existing-development-1.log`, exit0), preserving retry,
bookmark, finite loop, chart seed and member recording behavior.
Whitespace validation passed. Scoped Rust2021 formatting passed for the app
entry point, Desktop, measurement fixture and two new test files. The broader
format check reported pre-existing formatting differences in unchanged test
blocks of SessionLaunch/catalog/startup; no unrelated reformatting was applied.

Fresh current-source WASM browser library type checking passed
(`wasm-default-resolution-development-1.log`, exit0,1m00s). Existing unused
platform/helper warnings remain. This is compilation evidence; generated
bindings and actual browser QA must still use a newly built artifact before
claiming verification of the final candidate.

The fresh combined Worker, Window loading-state/play-host and generated PCM
export Node suites passed254/failed0/skipped0
(`current-loading-node-development-1.log`, exit0,84.13s). These are executed
model/module tests, not the required independent interactive browser QA.

Fresh Windows GNU all-target Rust type checking of the default-resolution
candidate passed with `desktop,webtransport`
(`windows-default-resolution-development-1.log`, exit0). The cross-check uses
stub native compilers/archive tools: it proves Rust type compatibility, not
Windows linking, SDK behavior or execution. It also exposed the now-unused
legacy private `native_replay` wrapper. A complete source-reference search
confirmed it had no callers after the prepared Desktop routing change, and it
was removed. Final primary-binary checks/builds must include this deletion;
the preceding Windows check still describes its earlier source checkpoint.

Known ceiling: the actual nonempty native ALSA-null replay diagnostic
reconstructed one logical result with the 1.5-second recorded horizon and
opened the exact requested output, but did not naturally complete within its
15-second bound (`native-finite-replay-development-1.log`, exit124). No cause,
successful drain or acoustic conclusion is inferred from this timeout.

A separate three-second diagnostic of the same genuine replay completed with
exit0 (`native-finite-replay-bounded-development-1.log`). Its typed source
report reached `playback_start_frame=60000` and
`playback_end_physical_frame=Some(60000)`, then paused with no pending commands.
The exact ALSA-null stream reported zero xruns, suspends and failures.
This separates source endpoint completion from the unproven native presentation
crossing; the imposed `--seconds 3` stop does not prove natural replay drain.

Read-only route inspection found common executable preparation markers for
finite native sections (`prepared practice section=`) and genuine recorded
playback (`reconstructed logical replay:`). The latter precedes ALSA open;
neither is an audio-ready endpoint. Five paired section/replay observations
remain to be implemented and executed. Neither preserved native UI exposes a
same-process library replacement/rescan command, so restarting an executable
cannot stand in for that comparative owner-replacement requirement. Native
fixture owner-replacement tests and browser replacement measurements retain
their narrower evidence scopes.

The opt-in native finite-section comparator is now an uncompiled candidate:
`desktop::loading_measurement_fixtures::preserved_native_finite_section_five_paired_fresh_process_trials`.
It reuses the existing observer/runner, fixes identical requested start/end
and config hash for both executables, and verifies actual start plus exclusion
of the zero-time note in each preparation marker. Marker-observed end stays
null. Scoped formatting/whitespace checks passed; compilation and ten actual
paired process observations remain pending after the current macOS compiler
lease. No native comparative section measurement is claimed yet.

Inspection of the frozen genuine replay's actual binary setup confirmed chart
seed0, start250000000ns, end1500000000ns, input offset0, one grade1 early/late
window of50000000ns each, no timing/judgment policy and default BeatKernel
gauge. Its v4 profile is ButtonOnly. Native Records preview must receive those
exact settings; the older proposed default150ms window would be incompatible.
The native GUI replay comparator is now a source candidate using unmodified
canonical bytes and actual Records scan/preview/Watch/F5:
`desktop::loading_measurement_fixtures::preserved_native_genuine_replay_five_paired_fresh_process_trials`.
It requires record/chart paths and hashes through
`BEATKERNEL_TEST_NATIVE_REPLAY_RECORD`,
`BEATKERNEL_TEST_NATIVE_REPLAY_RECORD_SHA`,
`BEATKERNEL_TEST_NATIVE_REPLAY_CHART` and
`BEATKERNEL_TEST_NATIVE_REPLAY_CHART_SHA`.
Both new comparators preserve owned failure PNGs before cleanup on navigation
or preparation/join timeouts and detected replay technical errors. Current
scoped formatting and diff checks passed; no compilation or GUI result exists
yet. The original macOS all-target check remains live and owns the heavy lease.

The original macOS all-target `desktop,webtransport` type check subsequently
completed with exit0 in33m02s
(`macos-default-resolution-development-1.log`). The prepared-routing/default
candidate and current primary binary were type checked; Linux-only measurement
fixtures still require their native compilation. Stub native tools mean this
is Rust type evidence, not macOS linking, device behavior or execution.
Existing unused helpers and the `block v0.1.6` future-incompatibility warning
remain visible in the retained log. The heavy lease is now used to compile the
new native finite-section/replay comparators.

The fresh native comparator compilation passed in1m17s
(`native-section-replay-compile-development-1.log`, exit0). The actual finite
section run then passed in64.11s:10 fresh production processes,20 preparation
attempts and20 owned-window PNGs. Both variants used identical config SHA256
`fd0e17d6e267eaa33d4a1e00158fc5aeda142fe7a6be91d1eed73c94fef4bce0`
and fixture hashes. Every marker exposed start250000000ns and excluded the
zero-time head; end1500000000ns is the request, not an observed native endpoint.
Raw records and derived `summary.json` remain under
`native-paired-finite-section-development-1/`.

| Native finite-section preparation upper bound, ms | Baseline p50 / p95=max | Current p50 / p95=max |
|---|---:|---:|
| First Enter |2073.040 /2202.704 |2094.494 /2108.259 |
| Same-owner F5 |2098.019 /2167.288 |1765.338 /1936.588 |

Each group has5 samples. Baseline executable235816a...288c and current
bef6dad...ec6d were preserved unchanged; their manifests retain exact full
hashes/source provenance. Current here predates the subsequent mailbox fix.
The first-run median slightly regressed; the retry median improved. The observer
and mandatory-ioctl joined frames add overhead, OS page caches are uncontrolled,
and no output readiness/natural finite completion/physical timing is claimed.
The current trial4 retry PNG was directly inspected.

The first strict genuine replay comparison failed on baseline trial0 retry
(`native-paired-genuine-replay-development-1.log`, exit101,20.72s). Actual
Records scan and compatible preview succeeded, and both replay reconstruction
markers had the same genuine1.5-second horizon/hash. The owned failure PNG
shows `retry lineage: room controls are busy`; this is not a valid completed
comparative trial. Source inspection found the same native-launch mailbox
WouldBlock-to-fatal-error path in current production. The approved-plan candidate
will defer that poll without waiting or losing the queued invocation, preserve
post-join canonical rebasing and retain genuine poison/lineage failures.
No failed sample is counted in the measurement table, and the immutable
baseline/strict GUI assertions will not be rewritten to hide this failure.

The mailbox change and its two independently authored deterministic regression
cases passed the actual private preparation suite5/failed0/ignored0
(`native-mailbox-contention-development-1.log`, exit0). The real slot stays
queued while held, then drains the exact prepared retry2/recording lineage once.
Actual poisoned-mutex refusal retains its queued invocation and native-specific
diagnostic. Current production changes only the native-launch poll; room,
practice/output admission and canonical post-join checks remain intact.
A fresh production artifact subsequently built successfully in2m01s
(`native-mailbox-production-build-development-1.log`), SHA256
`525cb58fe434a39d697b6032c85d0f8cfac9eddd878f37ca3ff5ac3084488194`.
The GUI fixture also recompiled successfully in1m22s.

The second strict genuine replay comparison failed after140.67s
(`native-paired-genuine-replay-development-2.log`, exit101). Preserved baseline
trial0 completed its first replay and F5, with two validated reconstruction
markers and owned joined-prefix frames. Current trial0 completed only the first
replay: its F5 timeout PNG reports `retry preflight: unknown native setting
--device`. This is a distinct actual accepted-invocation validation defect,
not another mailbox contention failure. The accepted replay template correctly
uses canonical `--device`, whereas the initial platform settings validator
expects Linux `--alsa`; projecting the accepted template a second time fails.
The scoped correction injects the existing strict canonical replay validator
for prepared retries, retaining the initial requested validator and custom
embedding policy. Both immediate and post-join retry preflight must select from
the actual invocation's prepared state. No completed five-pair distribution is
claimed from this incomplete run, and its failure screenshot remains preserved.

The prepared replay validation correction built successfully in1m16s
(`native-replay-prepared-validator-build-development-1.log`), executable SHA256
`6409cbb94d0e10c4df899341a168052038fe6efe9fb0e17a31034cc54c658254`.
Its manifest matches the current production source hashes. The first focused
test compilation failed on an independently authored fixture's missing boxed
error-to-String conversion; that raw log is preserved. After correcting the
fixture, the native preparation suite passed9/failed0/ignored0 in0.43s
(`native-replay-prepared-validator-tests-development-2.log`; build1m17s).
Four new cases cover real canonical projection/parser across repeated retries,
requested/prepared/custom validator selection, strict canonical rejection
without cancelling the current owner, and actual post-join cleanup publication
rebasing an unresolved preflight into the accepted canonical retry. These are
connected software tests, not physical output or final independent QA evidence.

The third strict genuine replay comparison passed in116.77s
(`native-paired-genuine-replay-development-3.log`, exit0). All10 unique owned
processes completed actual Records scan/preview, genuine first replay, joined
prefix, F5, second matching reconstruction and joined prefix, then bounded
cleanup. Each variant has5 fresh processes,2 attempts each, and matching
config/fixture hashes; the derived summary validates20 actual observations.
Root also inspected current trial4's retry PNG: the owned REPLAY prefix has no
technical failure caption. No earlier failed trial was added to this distribution.

| Genuine native replay input-to-prepared-log observed upper bound (ms) | Baseline p50 / p95 | Current p50 / p95 |
|---|---:|---:|
| First replay | 1417.990 / 1504.561 | 1412.596 / 1457.906 |
| Same-owner F5 retry | 1340.011 / 1404.471 | 1045.707 / 1253.464 |

Here p95 is nearest-rank, hence the maximum of5 observations. Raw evidence and
derived summary are under `native-paired-genuine-replay-development-3/`.
Baseline SHA remains `235816a648aac5500ea248456948627560020f0ec9fc69d6ba9cdf759148288c`;
current SHA is `6409cbb94d0e10c4df899341a168052038fe6efe9fb0e17a31034cc54c658254`.
The workload uses the same genuine finite recording and original large chart;
ALSA null with an imposed3s cutoff proves logical reconstruction/preparation
and joined prefix, not natural replay completion, output-ready or acoustic
latency. Runs are baseline-first, OS page cache is uncontrolled, artifacts are
unoptimized, and input/log/frame observer overhead is included. The current
Desktop subset also passed177/failed0/ignored9 in2.59s after the correction.
The complete fresh parent executable then passed343/failed0/ignored13 in3.52s
(`native-prepared-replay-full-unit-development-1.log`). Opt-in native GUI tests
remain ignored in that ordinary suite and have their separate actual runs above.

### Independent review remediation

The first independent DEEP review of `e421b59..2d65b13` returned three source
findings. Native visual registration used compatibility wrappers that ignored
resource-level cancellation; a newly created movie bank could leave its service
unjoined before Session installation; public uncached preparation copied rejected
raw input before its parser bound. Native images/movies now use their existing
phased jobs with the detached native control and combined bounded Visual counts.
NativeFile movie descriptors remain unchanged. A game-worker-local staged guard
joins an uninstalled bank on failure/cancellation/unwind and transfers cleanup
ownership once to Session on success. UI/render bank destruction stays nonblocking.
The public borrowed-chart path checks the existing8MiB raw parser limit before
reserve/copy, retaining its InvalidData error and message without decoding twice.

Fresh native attachment tests passed9/failed0/ignored0 in0.42s
(`native-visual-cancellation-remediation-development-2.log`; build2m26s).
Connected tests cancel synchronously during actual resource registration and
hold a controlled actual movie service to prove join-before-terminal ordering
and successful Session cleanup transfer. The first run returned8PASS/1FAIL:
a new fixture incorrectly expected Loading instead of legitimate Stopping
during cancellation. Correcting that expectation retains the held-service and
nonterminal assertions; the final snapshot is read once. Its original failure
log remains preserved. No production state rule was changed for that fixture.

Public allocation tests passed7/failed0/ignored0 in0.23s
(`raw-chart-allocation-remediation-development-1.log`; build3m26s).
8MiB+1 and16MiB input reject with no chart-sized duplicate allocation and zero
source/decoder effects; 8MiB-1 and exactly8MiB prepare a genuine referenced WAV
with one resolve/read/decode each. The existing per-thread allocation meter
observes requested allocation sizes, not peak resident memory.

Windows desktop/webtransport all-target Rust type checking passed in8m11s on
`2d65b13` before this remediation. The subsequent macOS check was deliberately
stopped at its exact owned process tree after these verified source findings
made its pending result obsolete (`macos-prepared-replay-development-1.log`,
exit143); it did not PASS. Fresh foreign-target checks, WASM artifacts, the
second DEEP discovery/formal review and ordered independent QA remain required.
These fixes and focused development results do not advance WBS or close the task.

The second full DEEP review of `293ed9b` found one further catalog ownership
race: the worker may finish after a pending poll but before a separate
completion check. Releasing that owner at completion discards its unconsumed
font or error result. The correction distinguishes consumed results from
finished threads and releases a metadata-drained owner only after its final
result has been consumed. Connected tests hold a real worker across the pending
poll, complete it before the actual release decision, then require real font
or explicit error publication on the next collection. Discovery has completed
both allowed cycles; the corrected full diff still requires a fresh DEEP
formal review and ordered independent CLI/browser/desktop QA.
The native catalog suite passed8/failed0/ignored0 in0.36s
(`catalog-result-lib-development-1.log`). The actual Desktop loading suite
passed8/failed0/ignored0 in0.59s (`catalog-result-desktop-development-1.log`),
including both new deterministic font/error publication regressions.

Browser and browser-audio release builds on `293ed9b` passed in1m43s and1m22s,
respectively. Matching wasm-bindgen0.2.129 generated both current packages.
`current-browser-293ed9b-artifact-provenance.json` records95 artifact hashes,
including gameplay WASM `a0f0d6ea40243b389197a579a98766afcca7dca97777c0b933112c8bfee842e7`
and audio WASM `775ed808697e545183a6b88f3b22bec0dbbd0cd04f76671185b7e70ac15d4b13`.
These are development build/provenance results, not independent browser QA.

Full DEEP code review and documentation review subsequently passed on
`dbc44f8`. Independent browser QA passed actual headed functional interaction,
five generated-WASM export tests and all30paired processes/70records/70PNGs.
Its current artifacts still match the95-entry browser manifest; later source
changes are native-only. Evidence is `qa-browser-final-1/` and
`qa-browser-finite-final-1/report.json`. All owned browser/display/server
processes terminated. The observed favicon404 requests are preserved and are
not application resource or runtime failures. Browser loading UX also passed
direct inspection of all10fresh functional screenshots.

Independent desktop QA on the preserved `dbc44f8` production executable
(`current-native-catalog-result/`, SHA256
`1898a3e45a1b80ec8700478aa7872099654d2dc1b1d459ed7bd15fc9f5aa3858`)
returned FAIL: progress at y105 overprints the existing practice caption at
y102. Two fresh actual processes reproduce unreadable stage/resource/cancel
text in `qa-desktop-dbc44f8-1/pending-current-snaps/current-2.png` and
`manual-cancel/actual-pending.png`. Independent desktop UX confirmed this
scoped feedback defect. The six actual native GUI batches all passed their
respective catalog/original/finite/replay/missing-default/held-font assertions;
they do not turn the overall QA failure into PASS. Manual real Return→pending,
Escape→Stopping→joined cancellation, Return→recovered catalog and clean exit
also passed without a completed-preparation marker after cancellation.
`observed-statistics.json` preserves the software upper bounds and all owned
processes/display were released. The correction reserves a separate loading
progress row while retaining practice instructions, then requires a pure scene
overlap regression, fresh DEEP formal review and affected desktop/UX QA.
Final CLI/foreign-target checks wait for corrected source; WBS remains90/193.

The first pure loading-row regression build failedE0624: native Desktop tests
are in the application binary, while Scene's existing rectangle slice was
library-crate-private. No tests ran and its sequential full-parent/production
build stages were not executed. After inspecting existing public APIs, the
minimal observation correction makes the existing Rectangle type and borrowed
Scene::rectangles accessor public. The accessor body, representation, bounds,
storage ownership and rendering are unchanged; no geometry copy, allocation,
mutable storage access or additional test feature is introduced. Its contract
describes static ordered logical quads and excludes timed instance expansion
and physical presentation. This allows the actual Desktop composition test
to inspect glyph/control footprints without GPU IO. Fresh library/WASM build
provenance and the final independent review remain required; old browser QA
retains its exact immutable artifact/source scope until equivalence or current
verification is established. The original failed build log remains preserved.

On final source `e436fcf`, the corrected actual Desktop composition regression
passed with all9 loading tests (0 failures, 0 ignored); the full parent executable
passed346 tests (0 failures, 13 ignored). Native production compilation passed
in1m37s. The preserved final executable in `current-native-loading-row/` has
SHA256 `7a9c4cc5fc39d762ab18ccddb8198efa9c5eec81212d76bd50a2d63195df1a90`
and93,629,136 bytes. The fresh full DEEP independent code review passed with
no findings; discovery was exhausted and no further hunters were run. These
development results and code review do not replace affected desktop/UX or
independent CLI/foreign-target verification.

Fresh browser and browser-audio release builds on that same source passed
in1m51s and1m14s, followed by successful generation of both bindings packages.
`current-browser-e436fcf-artifact-provenance.json` compares all95 artifact
hashes and lengths against the immutable `293ed9b` browser manifest: every
artifact is byte-identical. Thus the already completed actual browser QA and
browser UX retain their browser-artifact scope after the additive Scene API
change. This is artifact equivalence, not a new browser execution or a
hook-owned completion receipt. The original desktop QA FAIL remains attached
to its old executable; fresh affected desktop/UX and final CLI checks remain
pending. WBS remains90/193.

The independent browser QA's finite comparison is separate from the earlier
development measurements below. Its validated report is
`qa-browser-finite-final-1/report.json`; each variant/group contains five
samples, with nearest-rank p95 equal to the sample maximum. Observed
milliseconds before→after:

| Workload | p50 | p95/max |
| --- | --- | --- |
| Catalog first submitted usable frame | 75.155→71.165 | 112.340→79.600 |
| Library/content replacement preparation | 452.410→409.600 | 475.635→437.665 |
| Nonempty finite replay preparation | 42.315→32.270 | 65.630→35.575 |
| Section preparation | 14.975→4.200 | 20.160→13.375 |
| Cold large audio preparation | 398.085→438.885 | 437.345→485.215 |
| Warm preview preparation | 44.785→13.880 | 48.635→14.220 |
| Warm retry preparation | 18.950→8.065 | 19.735→9.305 |

Cold large-audio preparation regressed in this independent observed batch;
neither its median nor its tail supports a universal speedup claim. These
five-sample observations include controlled acquisition gates, input/frontier
observers and software rendering acknowledgments. They exclude browser/page/
WASM startup and establish neither physical output latency nor instantaneous
loading. Warm zero counts still cover original preparation parse/compile and
codec work only; fresh derived/replay validation remains outside those counters.

Fresh affected native GUI verification on the final `7a9c` executable passed
all six helper batches: catalog26.63s, original preparation82.64s, finite
section90.77s, genuine replay129.23s, missing-channel policy11.93s and held-font
catalog22.75s. The first four batches each compare five preserved-baseline
and five current processes; preparation/section/replay require first and joined
retry markers. Evidence lives separately in `qa-desktop-e436fcf-2/`, including
`observed-statistics.json`; it is not pooled with the earlier failed GUI attempt.
External observed milliseconds before→after:

| Native workload | p50 | p95/max |
| --- | --- | --- |
| Catalog first usable frame | 1456.325→1443.880 | 1587.385→1512.115 |
| Original cold preparation | 2769.703→2620.241 | 2955.742→3033.089 |
| Original same-owner retry | 2656.080→2334.377 | 2749.466→2451.725 |
| Finite-section cold preparation | 2895.639→2912.822 | 3569.848→3657.382 |
| Finite-section same-owner retry | 2704.983→2652.059 | 3492.844→3244.570 |
| Genuine replay cold reconstruction | 1548.051→1410.114 | 2113.472→1490.328 |
| Genuine replay same-owner retry | 1409.897→1177.063 | 2108.657→1222.100 |

Original cold tail and finite-section cold median/tail regressed in this batch.
These external upper bounds include input delivery, process/stdout observation
and software presentation costs. They are not parse/codec-only times or evidence
of physical/acoustic readiness. The held-font fixture uses the fresh test-parent
executable and retains separate provenance from the production executable.
The visible `manual-current/actual-pending.png` shows the final status, actual
stage/count/ESC instruction, practice caption and Cancel in separate readable
rows. Final manual interaction/cleanup and independent CLI verification remain
pending at this checkpoint.

The subsequent independent desktop QA2 and matching UX2 both returned actual
PASS for the changed desktop software/UI scope. Manual production PID1713707,
window8388610 received real MCP Return, Escape, Down, Up and Return: pending
preparation was visible, selection recovered, keyboard highlight changed, and
re-entry followed by the expected null-output/input technical error recovered
to selection again. Actual Exit clicking terminated the owned application with
exit0. All owned runner/application helpers terminated; the existing display
server was left untouched. `desktop-evidence.json` and the fresh manual PNGs
retain correlation. These actual finals are non-attesting when hook-owned
receipts are absent and do not authorize task closure. Independent CLI and
foreign-target checks remain pending; full player Goal and WBS remain unfinished.

Known ceiling: fresh manual Escape may have occurred after the preparation
marker, so this run does not prove cancellation before Ready or observation of
Stopping. It also did not capture the initial absent-snapshot state. Earlier
actual cancellation evidence remains associated with its original executable;
the pure current composition regression covers absent-snapshot and Loading
geometry. Neither is relabeled as fresh interactive evidence for the other.

Known ceiling: the manual FIFO attempt was refused by production regular-file
admission and is retained separately from successful GUI evidence. Failed new
Xvfb startup, focus-command and MCP-key invocations are not successful input or
server-ownership evidence.

Independent CLI verification passed66 focused PCM/mixer/restart/real-time
allocation tests. The broader app command then stopped at its library tests:
2365 passed,2 failed,6 ignored. Both failures reproduced individually on that
fresh compiled test executable. `invisible_source_fixtures.rs` still required
distinct sample payload pointers for canonical aliases, contradicting this
increment's approved immutable alias sharing. Its connected fixture must be
updated while retaining logical limits, unique acquisition/decode and independent
public deep-clone semantics. The unchanged cached Records detail test separately
refused a zero logical extent during suspended update; an actual archived
`e421b59` baseline run is pending to establish its ownership. Neither failure
is concealed or counted as a passing test. The original and reproduction logs
are retained under `qa-cli-final-1/`; integration/bin targets built before this
failure are not claimed executed.

Actual production CLI smoke passed12 invocations: primary/mode help, invalid
mode/zero-duration refusal, real bounded BMS+WAV rendering and repeated-output
refusal. The32000-byte output matched exact expected mono PCM values and
remaining silence; the repeated destination was refused without changing its
hash. `cli-smoke.json` records executable/fixture/output hashes and command
outputs. This narrow success does not replace the failed app suite, remaining
Node/Linux/foreign checks or required fresh formal review after the fixture
correction. Full player Goal and WBS remain unfinished.

The archived `e421b59` Records test subsequently reproduced the same failure
at fixture line478, exit101 (0passed/1failed/2359filtered), after3m24s
compilation. It is proven preexisting; suspended cached-detail updates remain
an actual full-goal repair obligation for the queued screen-ownership cleanup,
not a fixed or relaxed requirement. CLI QA1 returned actual FAIL for the obsolete
alias assertion and incomplete remaining scope. The corrected alias fixture
now additionally requires both canonical alias pairs to share immutable payload,
public deep cloning to remain independent, unique extraction to preserve its
pointer, and shared extraction/mutation to leave all originals unchanged.
Current-source development execution and a fresh formal review are still pending.
The baseline compile reused and overwrote a cached library-test filename;
current-source rebuilding is required before that artifact can be reused.

The corrected current-source alias regression subsequently passed1 test
(0failed/0ignored/2372filtered) after a fresh3m11s build, in0.35s. The same
fresh compiled executable then passed all4 invisible-source fixtures
(0failed/0ignored/2369filtered), in0.49s. Logs are
`alias-contract-development-1.log` and `invisible-source-development-1.log`.
These development results repair the obsolete assertion; fresh formal review
and independent remaining CLI verification are still required. The preexisting
Records failure remains open in the queued full-goal cleanup request.

Known ceiling: native GUI QA2 uses the existing live `:99` current-display
transport. Its attempted new Xvfb startup failed because the display listener
already existed; no ownership of that server or server cleanup is claimed.
Only owned runner/application processes may be terminated.

The strengthened browser capture prerequisite passed in one actual preserved
baseline process. `browser-finite-capture-development-1/observations.json`
records two admitted inputs, 118 advances, 120 durable records and a finite
1,500,000,000ns horizon. The live owner naturally completed with
`replayComplete=true` and `completedResults.proof=true`; the same process then
prepared and naturally completed that actual replay. Replay ownership does not
export the live recording-completion proof, and none was invented. The frozen
2,439-byte recording has SHA256
`17a5e5f36f1f96f372fa4bcbf1e72f4265f2964c62076171983b09277d12c0e6`.
Its prerequisite status is `CAPTURE_VALIDATED`, with no paired trials report.
The subsequent full comparison passed all 30 processes and 70 records, with
70 screenshots, zero page errors and ten naturally completed replay owners.
`browser-finite-paired-development-1/` preserves raw observations and the strict
report, which exited zero. All replay trials use the exact frozen bytes. Server
and owned Xvfb closed successfully. New fixture hash
`ec9e491c01e783641152d77dc0ed7e56c3c20720b9fc9480ab688def162dfd8e`
and configuration hash
`3c75ccfe82dc8c58be24eea096c7b32371357963d25d65ecbabbbebf99673afc`
distinguish this stronger workload from the earlier empty-capture measurements;
their statistics are not pooled.

Prepared-resource times in milliseconds, before→after, five samples per group:

| Workload | p50 | p95/max |
| --- | --- | --- |
| Warm preview | 44.615→13.725 | 49.670→15.960 |
| Warm retry | 19.990→9.695 | 20.545→10.045 |
| Section | 15.020→4.270 | 15.505→4.650 |
| Nonempty finite replay | 42.430→35.815 | 54.985→40.765 |
| Cold large audio | 439.950→429.190 | 456.625→451.855 |
| Library/content replacement | 422.235→394.525 | 485.210→399.150 |

Catalog first submitted usable frame was 71.885→66.360ms p50 and
73.415→76.235ms p95/max. Every current warm trial again recorded zero codec
and original preparation parse/compile counters. Fresh derived/replay validation
work remains outside these counters. These are workload-specific observations including
the same acquisition gates and added input/frontier observers, not universal
startup, physical latency or full-player completion claims. Development
execution remains separate from required independent review and GUI QA.

Before the production default-resolution correction, fresh Windows GNU Rust
type checking passed for `desktop,webtransport`, all
targets, with default features disabled. The command exited zero after
7m55s; `windows-loading-current.log` retains warnings and compiler completion.
C/C++ archive stubs make this type evidence only, not Windows SDK linking,
actual window/input/audio execution or physical-device evidence. macOS and
the new native preparation comparator still need fresh execution.

Known ceiling: the first two measured browser replay fixtures were genuine incomplete
captures stopped during startup, with no accepted inputs and no recorded horizon.
It covers replay preparation and audio setup, not nonempty replay execution,
judgement equivalence or typical full-song recording cost. A separate native
ALSA-null diagnostic reconstructed it and opened the exact requested output,
but did not naturally finish within its 20-second observation bound (exit124);
this is not successful native replay drain or acoustic proof.

Known ceiling: browser measurement `audio_ready` observes the correlated Worker
`play-reply` result `audio-ready` (audio setup readiness), not the first armed
AudioWorklet presentation frontier or physical speaker output. The actual UI
play journey still requires AudioWorklet startup before Stop.

Known ceiling: 보고서는 producer의 관측·hash·correlation 일관성을 검증하며, 실제 artifact 바이트나 물리적 지연을 독립 측정하지 않습니다.

Known ceiling: cached consumer는 독립 source/compiled 값을 복제하지만 query continuation은 같은 immutable parsed Arc를 재사용합니다.

Known ceiling: Arc metadata는 inventory 생성 시 한 번 할당됩니다.

Known ceiling: cold borrowed encoded bytes는 owned job/cache 저장소로 복사됨 — warm exact bytes는 추가 payload 복사 없이 재사용.

The cold browser copy can overlap the library's encoded storage within the
separately bounded cache budget. This does not claim whole-process memory is
limited to that budget or that acquisition requires no copies.

Known ceiling: Arc metadata is a cold allocation whose OOM cannot be recovered
as AudioError; empty assets incur this allocation too — upgrade when a supported
fallible ownership primitive can preserve the payload allocation and runtime
compatibility.

Known ceiling: shared infallible `into_samples` copies its payload and uses
infallible Vec allocation — use `try_into_samples` when the caller must report
payload allocation failure.

Known ceiling: one parse, compile, codec or filesystem operation can remain
indivisible — measure its tail and introduce finer processing only when actual
workload evidence requires it; phase yields do not promise instant cancellation
inside such an operation.
