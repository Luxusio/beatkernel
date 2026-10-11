# BMS parser single lookup — 2026-10-11

The prior Reject duplicate policy searched the definition tree with
`contains_key` and then searched again with `insert`. This increment uses the
existing entry API for Reject and retains ordinary insert for LastWins. It must
preserve exact duplicate line/label, stored canonical keys, replacement values,
validation order and existing chart/record identities.

Base: `b6b8d134c605f207344705e8f4bcc7f6ac046ec1`. Before source edits, the
immediate parent browser dependency tree was frozen into
`target/wf/bms-parser-single-lookup/baseline-b6b8d13/`: 92 artifacts, 8,696,220
bytes, each copied and byte-verified. Its browser/audio hashes match the prior
actual QA packages. Compiled Rust source equivalence to `318aef8` was verified
for adapters/crates/app/src and Cargo manifest/lock paths; the parent also contains
later JavaScript menu/layout changes, which are frozen rather than inferred.

Adapter whole-suite verification passed 108 tests, including four new private
fixtures. Formatting and diff checks pass. An isolated exact prior helper body
linked against the actual public adapter error types produced the expected result:
three semantic tests passed and the fewer-comparisons test failed (both paths
performed 3,242 comparisons). The scratch fixture replaces its crate-private error
constructor with identical public-field construction solely to link that isolated
reference; production/test source is unchanged by the reference run.
Current tests pass all four cases, without depending on exact standard-library
comparison counts. Initial fixture formatting differences were corrected before
source freeze; no assertion was weakened.

Source committed `dea00b1`. Current browser/audio release builds and installed
pinned wasm-bindgen0.2.129 generation succeeded (browser 1m44s, audio 1m04s).
Current browser WASM SHA256:
`0cc7b378a6c1dfee36de3102156fbed638915246299b694d6153ca9ca328aab7`.
Audio WASM remains:
`dde5198fb9779d4f1f8c5927ca446f929ea1fcdab8c8ca1a15ebaf46f35453b8`.
Builds report 10/9 existing feature-specific warnings; no zero-warning claim.
Independent DEEP code review passed; its final is non-attesting while the
runtime-owned receipt is absent. Actual paired measurements and CLI checks below
are complete; independent CLI, browser, scoped UX and documentation final
verdicts are PASS. Runtime-owned verification remains separate; no closed-task
verdict is claimed.
The previous 90.265ms Parse→Compile advance is not proof that all its time is
spent in the definition helper. Warm derived-copy/finalize is a separate cost.

## Actual immediate-parent comparison

The unchanged dense runner completed 8 groups × 5 observations = 40 records,
using 20 fresh cold process/library/cache owners and the same actual owner for
each warm operation. All operations prepared exactly 20,000 or 100,000 notes,
one sound, and correlated positive render acknowledgements. The 40 screenshots
were retained; independent scoped UX viewed the eight index-zero representative
screenshots and passed. Trusted keyboard input arrived during preparation in 36
operations and after readiness in four; the latter cannot prove responsiveness
during preparation. This was preview preparation, not audio playback or judging.

Environment: Chrome 153.0.8010.12, Linux/WSL2, AMD Ryzen 7 9800X3D,
Node 22.23.2, software Vulkan/SwiftShader, owned Xvfb, 1200×1000 viewport.
Fixture SHA256: `09addf9b97d840b8684fa923fc8409fbee99a8266b7284332e731101a354ceb2`.
Config SHA256: `99fb7cfaa4e78aa900d8ba3e68f235f32374defab9f0c020d9280ffbfb945b6b`.
There are 100 objects per legal physical BMS row; note totals were not reduced.

All timings below are milliseconds. Each cell is p50 / p95 / max over five
observations, using nearest-rank quantiles (so p95 equals max here).

| Workload | Ready before | Ready after | Submitted usable before | Submitted usable after | Max-step before | Max-step after |
|---|---|---|---|---|---|---|
| 20k cold | 172.575 / 174.590 / 174.590 | 169.940 / 177.905 / 177.905 | 183.620 / 198.980 / 198.980 | 188.455 / 200.675 / 200.675 | 24.755 / 37.420 / 37.420 | 24.530 / 25.510 / 25.510 |
| 20k warm | 13.675 / 28.830 / 28.830 | 10.915 / 15.755 / 15.755 | 27.825 / 30.825 / 30.825 | 25.055 / 29.830 / 29.830 | 3.215 / 5.575 / 5.575 | 2.985 / 3.220 / 3.220 |
| 100k cold | 265.415 / 268.890 / 268.890 | 269.195 / 277.395 / 277.395 | 296.750 / 301.580 / 301.580 | 298.310 / 317.870 / 317.870 | 76.465 / 89.675 / 89.675 | 71.490 / 76.885 / 76.885 |
| 100k warm | 35.825 / 39.580 / 39.580 | 38.225 / 39.405 / 39.405 | 51.485 / 62.260 / 62.260 | 49.280 / 60.345 / 60.345 | 16.165 / 18.120 / 18.120 | 17.585 / 19.950 / 19.950 |

The observed 100k cold maximum indivisible advance decreased from 89.675 to
76.885 ms. Both maxima first appear at stage `compile`, with parses=1 and
compiles=0: they belong to the completed Parse→Compile advance, not the compiler
operation. This does not isolate the helper's own duration. Overall 100k cold
ready/submitted medians increased; no end-to-end wall-clock speedup is established.
The remaining file acquisition, scheduler/render submission and warm finalize
costs are separate. Mechanism tests establish fewer comparisons independently
of these noisy wall-clock observations.

Both variants actually expose begin/max-step and parse/compile/read/decode job
counters. The unchanged runner's generic ceiling “Baseline has no preparation
phase/job counters” is historical text and is superseded for this immediate-parent
comparison by the raw observations. Separate parse/compile/decode duration fields,
Rust retained/copy bytes, physical audio-ready and redraw observations remain
unavailable/null; none is inferred from counters. Cold jobs parse/compile/decode
once; warm jobs do none of those and reuse the actual accepted cache.

Independent CLI checks passed 108 adapter tests, 931 Node tests using the fresh
bindings, eight loading-report fixtures, scoped formatting, source/artifact
provenance and an actual consumer rerun equal to the browser report. All 90 served
artifacts outside the changed browser binding/WASM pair equal the frozen parent.
Preserved QA setup mistakes were a guessed WASM filename and a nonexistent Cargo
package name; corrected checks passed. Scratch audits also initially assumed
virtual observer resources mapped directly to filesystem paths, and that warm
`canonical_asset_reads` had to be zero. The actual runner mapping was used instead;
this cumulative canonical counter remains one, while warm file-read, parse,
compile and decode jobs are zero and shared-assets is one. Original failed audit
outputs were retained. An initially overbroad zero-server-error assertion
encountered the expected favicon failures. All 20 `/favicon.ico` 404s
were retained and classified; product/Worker/render/nonfavicon errors were zero.
No failed browser measurement attempt or weakened product assertion was needed.
The server closed, Xvfb exited zero, owned runner/Xvfb PIDs were absent and all
20 owned browser profiles/caches were removed.

Actual evidence (ignored scratch artifacts, available in this checkout):

- `target/wf/bms-parser-single-lookup/qa-browser-1/{trials.json,observations.json,report.json,independent-audit.json}`
- `target/wf/bms-parser-single-lookup/qa-cli-1/{rust-all.log,node-all.log,loading-tests-corrected.log,provenance.json,independent-report.json,old-helper-fresh.log}`
- `target/wf/bms-parser-single-lookup/baseline-b6b8d13/manifest.json`

## Known ceiling

Software Chromium/SwiftShader/Xvfb trials do not prove physical input/audio latency
or monitor scanout. Exact comparison counts are standard-library-dependent, so
tests require fewer aggregate comparisons rather than a fixed count. Five trial
quantiles describe this workload/environment only. Parser/codec/finalize calls
may remain indivisible; retained-cache paths and all-chart performance remain
separate. Full player/WBS13.08 and release are unfinished; WBS remains 91/193.
The scoped UX review does not cover transient Cancel behavior, narrow-screen
layout or full accessibility; the existing long control page and small text are
broader UI backlog. A missing runtime-owned receipt is not a closed-task PASS.
The optional generic health command printed 0.0/10 because its unconfigured shell
cannot find Cargo (`command -v cargo` exits 1); it suppresses the component stderr.
That result is not a product regression verdict or a fresh full-workspace test.
The scoped QA commands above explicitly load the approved local toolchain.
