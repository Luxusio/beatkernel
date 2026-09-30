# ASIO direct cadence and allocation-free clock diagnostics

ASIO gains opt-in explicit shared-QPC render cadence with actual successful block
identity and B priming excluded. Clock-free preparation remains available. Live
and recorded BMS hosts opt in and print drained summaries. QPC/mach diagnostic
queries now use allocation-free native sampling and checked conversion, correcting
potential error-path allocations from rich `io::Error` construction in the prior
WASAPI/CoreAudio cadence path. Missing timing remains explicitly unavailable.

The new memory-backed ASIO fixture is authored and compile-checked, not executed.
Locked Rust 1.98.1 default workspace, default Windows GNU/macOS platform and
optional ASIO Windows Rust-source checks passed. The optional cfg check activates
Rust source only; actual SDK C++/MSVC ABI build, linking, native output, allocation
audit, tests, review and QA remain deferred. See the
[cadence boundary](../platform/REQ__native-render-cadence.md).
