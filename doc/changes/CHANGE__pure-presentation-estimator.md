# Pure presentation estimation

Presentation drift and phase calculation previously lived alongside WASAPI/ASIO
source validation in the platform discipline. The core time layer now owns the
bounded clock-pair ring, freshness, update chronology and continuous transport
correction. The platform compatibility adapter retains source validation and
original error variants, forwarding accepted observations to the actual core
algorithm. No additional crate or dependency was introduced.

Supplied duplicate output does not refresh freshness. Checked native progress
can retain an unchanged quantized nanosecond when actual counters advance;
ASIO equal coarse host midpoint still awaits progress without metadata updates.
The adapter owns latest source identity while the single ring contains only
clock pairs. Cloning reserves the configured ring capacity before copying pairs.

Independent deferred fixtures cover four core groups and three adapter groups:
rational drift/phase/clamping, retention and chronology, rejected-operation
preservation, signed limits, WASAPI quantization, malformed source evidence,
ASIO midpoint waits and source mixing. Existing assertions remain unchanged.
After both writers stopped, scoped rustfmt and git diff --check completed.
All four root compile-only checks exited zero: workspace/all-targets with
webtransport; runtime/all-targets without defaults with webtransport; WASM
browser library; WASM browser-audio library. Existing dead-code warnings
remain. Tests, hardware timing, formal review/QA and benchmarks remain
unexecuted; compilation does not establish assertion success.

Common BMS sessions and device interfaces still reference the platform wrapper;
a subsequent presentation port migration and further concrete competition/IO
separation are required. This change does not establish full layer separation,
SQLite-equivalent reliability or a comparative performance ranking.
