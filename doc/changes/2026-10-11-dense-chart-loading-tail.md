# Dense-chart browser loading tails — 2026-10-11

The existing large-audio benchmark has two chart notes and an 8 MiB WAV. It
cannot establish preparation cost for a dense chart. This increment extends
the existing strict browser runner with a separate dense-only workload, using
20,000/100,000 actual BMS objects and one small shared WAV. Existing genuine
finite replay and full/capture measurement paths remain separate.

Contract: [loading readiness](../ui/REQ__loading-readiness.md).
Base: `c93221c`. Final measured source is `e44af95`; actual browser/CLI and scoped
UX acceptance passed after the failed development attempts described below.
The runner must prove exact prepared notes/samples, fresh cold and same-owner
warm preparation, correlated readiness/submitted frame, honest unavailable
historical metrics, trusted input timing and complete owned-resource cleanup.
No production optimization or measured speedup is claimed before evidence.

Runner implementation: `62b20ac`; diagnostic preservation: `b426c1d`.
Syntax/diff checks and three launch-free refusals (unknown mode, dense+capture,
fewer than five trials) passed. Development1 failed on the first baseline
20k trial: selected preparation completed, but its positive matching renderer
geometry was not observed within 30 seconds. No successful measurement group
is claimed. Server/Xvfb and browser cleanup completed; original observations
remain under `target/wf/browser-dense-chart-loading-tail/development-1/`.
Development2 adds only failed snapshot/screenshot capture and rethrows the
original error; it does not weaken the timeout, add retries or change product code.

Development2 reproduced and identified the actual cause: the baseline prepared
20,000 notes/one sample, then refused the view with `playfield exceeds 2048 visible
notes`. The original fixture placed 10,000 objects in each measure, mixing total
chart preparation with concurrent visible density. The revised workload retains
20,000/100,000 total notes but uses 100 objects per measure across 200/1,000 measures
(000–999). This separates total preparation from the per-view cap without changing
production limits or lowering the total workload. Both failed runs and the inspected
failure screenshot remain preserved. Concurrent visible-density support remains a
separate full Goal obligation. Correction committed as `e44af95`; final independent
DEEP code review PASS precedes the fresh qa-browser/qa-cli/scoped UX cycle.
The actual runner completed exit0 and generated a valid report. Independent browser
QA, CLI, scoped UX and final independent documentation review returned PASS.
No receipt-backed close is claimed.

## Actual measured result

Evidence: `target/wf/browser-dense-chart-loading-tail/qa-browser-1/` contains
`observations.json`, `trials.json`, `report.json`, `acceptance-audit.json` and 40
screenshots. Eight groups contain five observations each: baseline/current ×
20k/100k × cold/warm. Twenty fresh processes each prepare cold then retry on the
same accepted library/cache. Exact prepared notes/sample counts and request-correlated
positive submitted geometry pass. All current warm original parse/compile/decode
counters are zero; historical missing phase/counters remain null.

Current timings in milliseconds, nearest-rank p50/p95 (five samples per row):

| Notes / preparation | Prepared p50 | Prepared p95/max | Submitted frame p50 | Submitted frame p95/max |
| --- | ---: | ---: | ---: | ---: |
| 20k cold | 131.705 | 140.600 | 142.355 | 152.830 |
| 20k warm | 15.240 | 16.740 | 16.835 | 26.455 |
| 100k cold | 246.445 | 288.425 | 276.450 | 318.315 |
| 100k warm | 33.115 | 40.815 | 52.560 | 54.225 |

The current 100k largest indivisible advance is 90.265ms cold and 20.080ms warm.
This is a Worker-local maximum, not proof that one particular internal function
consumed it. Actual input observations comprise 39 during preparation and one after
readiness; all 40 are trusted. No redraw sample or audio-ready event is fabricated.
These are measurements of existing runtime behavior, not a speedup produced by the
measurement-only commits. Cold preparation is not instantaneous; further optimization
requires finer attribution of the measured long advances.

CLI scratch `qa-cli-1/` independently validates the actual report and its eight
groups, fixture/config/artifact provenance, unavailable historical phases and current
warm counters. Existing standalone report-consumer tests pass 8/8; syntax/diff and
three launch-free refusal cases pass. Its initial default-cc standalone compilation
failed because that linker is missing; explicit existing host-cc linking succeeded.
Initial failed build output is preserved in the tool transcript; the scratch build
log holds the successful corrected run. No app/Rust/WASM production source changed.

Scoped independent UX directly inspected all eight variant/density/temperature
screenshots and raw observations; exact chart metadata, usable preview and honest
input ordering passed. Native/narrow/full-product UX is outside that inspection.
All owned browser/server/Xvfb processes joined and profile/cache removed; resource
absence is checked. Twenty existing favicon404 console/server requests are preserved;
page/Worker/render/selection and non-favicon HTTP errors are zero. An all-errors-zero
claim would be inaccurate.

Fixture SHA256: `09addf9b97d840b8684fa923fc8409fbee99a8266b7284332e731101a354ceb2`.
Config SHA256: `99fb7cfaa4e78aa900d8ba3e68f235f32374defab9f0c020d9280ffbfb945b6b`.
Both densities share the 19,244-byte WAV SHA256
`93a8d4c65f1e0aecc5804b33994b8f129cce475374da551b939f52f80b0030a7`.

## Reproduction

Use the existing repository toolchain environment and installed Puppeteer module.
The baseline is immutable `e421b59`, not the immediate parent of this measurement-only
commit. Current Rust/WASM is unchanged by this task; the runner hashes served current
assets independently and records its source revision.

```sh
source target/toolchain/env.sh
LOADING_LATENCY_MODE=dense \
LOADING_LATENCY_TRIALS=5 \
LOADING_LATENCY_OUT=target/wf/browser-dense-chart-loading-tail/qa-browser-1 \
LOADING_LATENCY_BASELINE=target/wf/player-loading-latency/baseline-e421b59 \
LOADING_LATENCY_SOURCE_REVISION=e44af95c83fe6dcb5f630a83fd594c3762bf5b25 \
EXPECTED_WASM_SHA256=2fbf27acd8f3d9a95eb33c7b1077ec491fe2fc8e87b820681f5402feb7ecbc3a \
PUPPETEER_MODULE=target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core \
node app/web/tests/loading-latency.mjs
python3 tools/loading_report.py \
  target/wf/browser-dense-chart-loading-tail/qa-browser-1/trials.json \
  --output target/wf/browser-dense-chart-loading-tail/qa-browser-1/report.json
```

Use a fresh output directory for another run so original failed/successful observations
are retained. The shell example describes the software workload, not physical audio or
input measurement. It never constructs a synthetic recording or historical counter.

## Known ceiling

Software Chromium/SwiftShader/Xvfb measurements do not establish physical input,
audio latency or monitor scanout. An input observed after preparation readiness
does not establish input acquisition during preparation. Individual parser,
compiler, codec and finalize calls may remain indivisible; their actual tail
must be measured. Two bounded dense fixtures do not prove all-chart performance,
whole WBS13.08 or whole-player completion. WBS remains 91/193 (47.15%).
Warm preparation still exports independent JS PCM for actual consumers; absent
Rust copy/retained-byte counters stay unavailable. Baseline absent preparation
phase/job counters cannot produce a historical delta. Natural work has no read
gate; Puppeteer/observer overhead remains in the software measurements.
Existing long settings/small-text layout and dense single-lane overlap remain UX
backlog; narrow layouts and full product accessibility were not reinspected here.
The benchmark server's existing favicon404 requests are preserved and classified;
handling that noise is deferred rather than added to the product scope.
