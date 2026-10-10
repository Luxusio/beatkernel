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
decode/parse/compile counters being zero. Prepared p50 in milliseconds was
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
the maximum. Warm decoded-original reuse recorded zero codec/parse/compile
work in every current warm scenario; historical internal counters remain
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
73.415→76.235ms p95/max. Every current warm trial again recorded zero
codec/parse/compile work. These are workload-specific observations including
the same acquisition gates and added input/frontier observers, not universal
startup, physical latency or full-player completion claims. Development
execution remains separate from required independent review and GUI QA.

Fresh Windows GNU Rust type checking passed for `desktop,webtransport`, all
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
