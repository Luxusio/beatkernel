# Finite ASIO presentation calibration

ASIO presentation observations retain the Mixer rate and frame-zero origin.
Two same-stream observations can establish a finite output/host affine relation
using interval midpoints. Interval width, output-grid quantization and permitted
extrapolation contribute to the reported observation error. Residual drift is
explicitly caller-assessed; unknown drift retains Unknown quality. Host mapping
and transport creation enforce finite validity, including frame-zero admission.

The continuous presentation discipline accepts ASIO observations separately
from WASAPI counters and generic supplied pairs. It preserves block/rate identity,
rejects inconsistent grid metadata and overlap/regression, and does not refresh
freshness for a duplicate block. Midpoint correction retains Unknown quality.

Four calibration and five discipline fixtures are authored and compiled only.
Locked Rust 1.98.1 workspace, optional target-only Windows Rust source and
default macOS platform/sample all-target checks passed. The Windows source
check leaves Cargo's SDK feature inactive and does not compile or link C++.
Native SDK/driver acceptance, live BMS host composition, fixture execution,
independent reviews and QA remain outstanding. The complete runtime objective
remains active. Original source and non-ASIO builds remain MIT; SDK-combined
artifacts retain the separate GPLv3 distribution policy.
