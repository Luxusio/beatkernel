# Explicit live Windows BMS ASIO output

`windows_bms` now accepts optional explicit ASIO output through the existing
physical-input, binding, judging, BGM, keysound and accepted-operation replay
capture loop. Driver CLSID/view, output channels, multimedia timer declaration
and three assessed error bounds are required. The current driver rate defines
the Mixer format; unsupported selections reject without device/rate replacement.
ASIO preferred/exact frame buffers stay distinct from WASAPI mode/period options.

Startup obtains finite rendered-block presentation calibration. Shared QPC
brackets refresh multimedia timer anchors on the control thread, and actual
ASIO observations enter continuous Unknown-quality correction. Short buffers
can produce a coarse timer plateau: a newer block with unchanged host midpoint
waits without updating freshness. Native regressions/faults and stale actual
progress terminate. Software prepared frames schedule keysounds separately from
audible output. Pre-origin inputs are counted/excluded without changing timestamps.

A hidden driver sysref HWND is retained through stream teardown, separately
from the focused input window. Bounded driver message servicing also runs during
startup. All error exits retain the existing stream/input/window cleanup and
save only an actual accepted-operation replay prefix when capture is enabled.

Nine CLI fixtures are authored/compiled only. Rust 1.98.1 locked workspace,
default Windows GNU and optional target-only Windows Rust source checks passed.
The optional source check leaves Cargo's SDK feature inactive and does not
compile/link C++; actual SDK/MSVC ABI, installed drivers, fixture execution,
sound and physical latency remain unverified. Reviews and QA remain deferred.
MIT source/default builds and GPLv3 SDK-combined distribution remain in effect.
Full-plan acceptance is still outstanding; the runtime Goal remains active.
