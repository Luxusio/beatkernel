# Windows and macOS input cadence composition

Native inspectors previously printed samples without cadence summaries. Windows
Raw Input inspector now offers explicit keyboard device/usage/state cadence mode,
while `macos_input_cadence` selects an IORegistry device, element cookie and native
scalar value. Both connect actual timestamps to bounded interval and delivery-age
observers, preserve provenance and print summaries after cleanup. Errors, selected
order regressions and removal terminate measurement without reconnect bridging.
Windows QPC receipt intervals remain distinct from IOHID native mach event
intervals. No physical latency or exact missing-event counts are inferred.

Locked Rust 1.98.1 platform all-target checks passed for Windows GNU and macOS
x86_64, and the workspace host check passed. These checks do not link or execute
native acquisition. Examples, tests, high-rate measurements, formal review and
QA remain deferred by user instruction. See the [Windows](../platform/REQ__windows-input-cadence.md)
and [macOS](../platform/REQ__macos-input-cadence.md) contracts.
