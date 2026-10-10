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

## Known ceiling

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
