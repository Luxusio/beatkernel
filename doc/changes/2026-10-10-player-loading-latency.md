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

The complete immutable baseline is recorded in
`target/wf/player-loading-latency/baseline-e421b59/manifest.json`; all 91
artifacts were independently hash-checked before source activation. Capturing
these artifacts does not constitute a baseline timing trial.

## Known ceiling

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
