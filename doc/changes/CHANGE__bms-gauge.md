# Common fixed-point BMS gauge observations

A shared BmsGauge consumes actual stepped solo/local and incremental
replay reports. Its bounded, explicit profile handles opaque grade overrides,
signed hit/miss deltas, exact half-percent mine damage, recoverable zero and
latched instant-death/depleted state. Observation validates a whole hazard batch
before committing and performs no hot allocation or floating-point accumulation.

The documented application default is initial 20%, clear threshold 80%, +1% per
successful judged stage and -6% per missed stage, with recoverable zero. It is
BeatKernel policy; TOTAL parsing, historical gauge compatibility and configurable
session capture identity are separate work. Native/HUD publication, clear/fail
completion and actual failure fencing/output cleanup remain unfinished. The
high-level mine admission guard remains in place.

Five independently authored deferred fixture groups cover bounded profiles and
signed extremes, exact damage/failure/atomic batches, real solo audio-failure
prefixes, independent local contact gauges, and capture/replay hold/miss/mine
observations without duplicate display-time application.

Both writers delivered terminal Writes STOPPED before scoped rustfmt. Four
authorized compile-only checks completed with exit 0: workspace all targets with
WebTransport, headless all targets with WebTransport, WASM browser and WASM
browser-audio. git diff --check completed successfully. Existing unused
playfield-wrapper and WASM cadence warnings remain. Assertions were not run.

Assertions, applications, browsers, devices, measured performance and formal
review/QA remain deferred. This slice does not establish overall Goal completion.
