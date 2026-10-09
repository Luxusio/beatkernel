# Windows live output backend selection

Windows solo/local output settings select the target backend before parsing its
configuration, through the existing WindowsRequest and shared replacement owner.
Omitted backend preserves legacy current-backend requests. Prepared PCM rate and
source channel basis remain fixed during this switch.

Device and buffer refer to the target. A first WASAPI target may use an empty
device for OS default. An ASIO CLSID copied into a WASAPI target is refused before
retirement; choose a WASAPI endpoint or clear the field. ASIO requires an explicit
installed canonical CLSID, registry view, output channels, multimedia clock and
caller-supplied error bounds with positive anchor age. SDK-disabled builds refuse
ASIO without touching the output owner. No driver is installed or downloaded.

Backend-specific inactive fields do not affect the selected target. Successful
Apply refreshes the draft from the actual current capability; inactive draft
values are not retained in a separate cache. Available switch fields remain in
the existing bounded keyboard-editable settings surface. Existing request IDs,
pause gates, native clock epochs and unique Mixer recovery remain authoritative.

Invalid target metadata is rejected before retirement. Native open/format failure
uses existing recoverable/pending cleanup without implicit backend fallback.
ASIO selection reuses the existing trusted installed-driver loading policy.

Portable tests and foreign Rust typing establish only their stated scope.
Actual MSVC/ASIO SDK compilation, installed-driver playback, measured latency and
physical cross-backend continuity remain required before hardware acceptance.

## Development evidence

The production target planner is called by WindowsOutputUi shared by solo and
local hosts. WASAPI default discovery happens before retiring the current owner.
The owner records original source channel width and recovers it from the Mixer
when reopening; channel matrices cannot replace that original source basis.

Focused development checks passed Windows binary 59 tests (including ten new
target admission fixtures and two production capability roundtrips), two new
Windows settings schema tests, and common gameplay output regression 103 tests
with three ignored native/environment cases. These are development evidence,
not independent QA or physical driver acceptance. Logs are in
`target/wf/windows-output-backend-switch/`; portable admission fixtures are
`app/src/bin/windows_bms/output_switch_fixtures.rs`.
