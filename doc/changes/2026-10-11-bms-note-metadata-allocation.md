# BMS note metadata allocation — 2026-10-11

The prior per-note serializer allocated two bytes and grew while writing a
13-byte normal/no-tail or 15-byte tail payload. A small private serializer now
reserves its final size once, retaining the exact version/lane/head/physical-line/
tail flag and optional tail bytes, little-endian order and casts. Parser validation,
public API and ordinary Vec OOM behavior remain unchanged. The adapter still
forbids unsafe code; allocation instrumentation exists only in an integration
test binary that includes the exact private production serializer source.

Base: `6975df944aa1b2863254211a713a7500b864340a`. Before source edits,
92 current-parent served artifacts (8,696,923 bytes) were copied and byte-verified
under `target/wf/bms-note-metadata-allocation/baseline-6975df9/`. The compiled
Rust source from `dea00b1` equals the parent for adapters/crates/app/src and Cargo
manifest/lock paths; these packages passed the preceding actual independent QA.
Baseline browser WASM SHA256:
`0cc7b378a6c1dfee36de3102156fbed638915246299b694d6153ca9ca328aab7`.
Baseline audio WASM SHA256:
`dde5198fb9779d4f1f8c5927ca446f929ea1fcdab8c8ca1a15ebaf46f35453b8`.

Development adapter verification passed 113 tests (108 existing plus five new),
zero failures/ignored. The new tests exercise real allocator positive controls,
unwind-safe tracking, literal 13/15-byte expectations, tail casts and line extrema,
public parse/compile for normal, LNTYPE1 defined/undefined tail, LNOBJ and LNTYPE2,
and JudgeEngine identity equality with literal prior-format reference metadata.
A one-byte metadata mutation changes the identity hash as a negative control.
Tests measure the actual helper at one allocation/zero reallocations; the local
prior-format positive control still grows. Initial fixture formatting differences
were corrected after the development suite; no assertion was weakened. Scoped
Rust2024 formatting is now clean; independent fresh QA remains required.

An exact prior inline serializer was extracted from the parent Git source and
wrapped with parameter-name substitutions only in ignored scratch. A scratch
copy of the fixture includes that old serializer instead of the new helper while
linking the actual current adapter/core. It exits 101 as expected: two tests pass,
three allocation assertions fail with `[1 allocation, 2 reallocations]` versus
the new `[1, 0]` requirement. The public parser test still links the current adapter;
this is an isolated old-builder mechanism check, not an old whole-adapter run.
Evidence: `target/wf/bms-note-metadata-allocation/development-adapter.log` and
`old-builder/{parent-block.txt,metadata.rs,fixture.rs,red.log}` in the same scratch.

Source is committed as `70996ad39b67768857cef59b48895a8343a47cf5`. Browser/audio
release builds and pinned wasm-bindgen 0.2.129 generation both finished successfully
(1m45s / 1m09s, 10/9 existing feature-specific warnings). Current browser WASM:
`ff14d153a90b7022f6ec9043bb652c2d46257d942d96e1bbef7fe25bd5f8eb34`.
Audio WASM remains equal to the baseline hash above. All 92 frozen baseline
artifacts were rechecked and unchanged after the current rebuild.

Independent DEEP code review passed after both defect hunter results were empty.
Independent CLI has freshly passed all 113 adapter tests, formatting and baseline
provenance. Fresh full Node tests passed 931 cases and loading-report tests passed
eight, all with exit zero. Actual paired browser and scoped UX final verdicts
are PASS; CLI and final documentation verdicts are PASS. The initial documentation
review found an incorrect browser-run log path; the actual sibling path was
corrected and independently rechecked before PASS. Actual finals are
non-attesting when runtime-owned receipts are absent. No
end-to-end wall-clock improvement is claimed from allocation counts alone.

## Actual immediate-parent observations

The unchanged dense runner and report consumer exited zero. Eight groups of five
observations produced 40 operations and 40 PNGs, using 20 unique fresh cold
process/library/cache owners with the same owner for each warm retry. Each actual
operation prepared exactly 20,000 or 100,000 notes and one sound, with correlated
positive render geometry. Independent UX viewed all eight index-zero images and
audited all 40 request/ready/frame correlations. Trusted keyboard input arrived
during preparation in 34 operations and after readiness in six; the latter are
not evidence of pending-input responsiveness. This flow previews prepared charts;
it does not play audio or judge the injected keyboard event.

Environment: Chrome 153.0.8010.12, Linux/WSL2, AMD Ryzen 7 9800X3D,
Node 22.23.2, software Vulkan/SwiftShader, owned Xvfb, 1200×1000 viewport.
Fixture SHA256: `09addf9b97d840b8684fa923fc8409fbee99a8266b7284332e731101a354ceb2`.
Config SHA256: `99fb7cfaa4e78aa900d8ba3e68f235f32374defab9f0c020d9280ffbfb945b6b`.
Each legal physical row contains 100 objects; totals were not reduced.

All table values are milliseconds, p50 / p95 / max using nearest-rank quantiles.
There are five observations per cell, so p95 equals max for this sample size.

| Workload | Ready before | Ready after | Submitted usable before | Submitted usable after | Max-step before | Max-step after |
|---|---|---|---|---|---|---|
| 20k cold | 145.350 / 165.330 / 165.330 | 147.840 / 162.435 / 162.435 | 169.305 / 177.185 / 177.185 | 163.085 / 180.120 / 180.120 | 24.435 / 32.935 / 32.935 | 22.485 / 24.080 / 24.080 |
| 20k warm | 14.440 / 17.180 / 17.180 | 14.615 / 29.610 / 29.610 | 26.070 / 33.745 / 33.745 | 18.085 / 31.620 / 31.620 | 3.380 / 5.970 / 5.970 | 3.510 / 4.685 / 4.685 |
| 100k cold | 262.680 / 283.005 / 283.005 | 255.630 / 293.780 / 293.780 | 293.275 / 314.360 / 314.360 | 279.950 / 327.855 / 327.855 | 78.275 / 90.860 / 90.860 | 69.690 / 88.410 / 88.410 |
| 100k warm | 35.200 / 38.185 / 38.185 | 35.350 / 48.575 / 48.575 | 50.965 / 53.590 / 53.590 | 49.425 / 55.620 / 55.620 | 16.855 / 17.385 / 17.385 | 16.440 / 21.700 / 21.700 |

The 100k cold ready median decreased 7.050 ms, but p95/max increased 10.775 ms.
Warm ready medians did not improve, and several tail observations increased.
These mixed results do not establish a universal or statistically reliable
end-to-end speedup. The allocation mechanism improvement is independently proven.
The 100k cold maximum advance decreased 90.860→88.410 ms; both first appear at
stage `compile`, parses=1/compiles=0, so belong to the completed Parse→Compile
advance. This is not isolated compiler or serializer duration. The current 100k
warm maximum is 21.700 ms, first appearing at `ready` with parses/compiles=0.

Both variants actually expose begin/max-step durations and job counters. Cold
parses/compiles/decodes are one; warm file reads/parses/compiles/decodes are zero
and shared assets is one. Warm canonical_asset_reads remains one as a cumulative
original/shared-asset counter, not a new file read. Actual raw observations
supersede the runner's historical “Baseline has no preparation phase/job counters”
ceiling for this immediate-parent comparison. Individual parse/compile/decode/
dependency-query durations, Rust copy/retained-byte counters, pending-frame,
physical audio-ready and redraw observations remain unavailable/null.

Twenty favicon-only 404s were retained; product/page/Worker/render and nonfavicon
HTTP errors were zero. The server closed, Xvfb exited zero, and the browser owner
verified owned runner/Xvfb/Chromium absence and profile/cache removal. Audit setup
corrections so far were the CLI manifest-key assumption (`files` versus actual
`artifacts`) and a mistaken zero warm canonical-read expectation; original outputs
were preserved, with no product assertion or raw observation changed. The CLI
auditor also corrected an input-latency assumption: latency observations retain
all 40 trusted inputs, while timing is separately classified as 34 during
preparation and six after-ready. After-ready samples are never pending-input proof.
The independent CLI consumer reproduced the report and verified served artifact
bytes, identical observer wrapper/probe hashes and fixture/config provenance.

Evidence in this checkout's ignored scratch:

- `target/wf/bms-note-metadata-allocation/qa-browser-1/{trials.json,observations.json,report.json,browser-audit.json,stage-first-appearance.json}`
- `target/wf/bms-note-metadata-allocation/qa-browser-1-run.log`
- `target/wf/bms-note-metadata-allocation/qa-cli-1/browser-audit.log`
- `target/wf/bms-note-metadata-allocation/ux-browser-1/review.txt`
- `target/wf/bms-note-metadata-allocation/baseline-6975df9/manifest.json`

## Known ceiling

Helper measurements do not mean the entire parser allocates once. Ordinary
infallible Vec OOM behavior is preserved; no forced OOM abort experiment was run.
The 64-bit native line-extrema branch was executed; 32-bit encoding assertions
are prepared but not yet executed. Actual software browser measurements cannot
prove physical input/audio latency or monitor scanout. The unchanged runner's
historical baseline-counter ceiling is explicitly superseded above, keeping unavailable
individual phase/memory observations null. Full player, all-chart performance,
release and WBS13.08 remain unfinished; WBS stays 91/193.
Long controls, small text, narrow-screen layout/accessibility and transient Cancel
behavior remain broader UI work, beyond the scoped eight-image UX review.
Missing runtime-owned receipts cannot be supplied by these actual direct finals.
