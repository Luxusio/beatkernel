# Retain mixer state after native output retirement

Core exposes a static stopped-mixer recovery port. Native streams implement it
so confirmed worker join/callback retirement can return the
original software Mixer once rather than dropping its PCM/voices/queue state.
Existing stop diagnostics and report/cadence access remain independent.
Stopped streams now retain the recovered mixer and its PCM/queue assets until
take or stream drop; callers ending playback should drop the stopped stream
promptly when recovery is unnecessary. This also preserves the command consumer
for handoff rather than treating stop as queue destruction.

## Scope and evidence limits

Recovery never performs stop/reopen or claims an audible playback fence. Device
queues may contain rendered-but-unheard audio, and new device clocks need explicit
origin/epoch mapping. Runtime transition controls, failed-open rollback, browser
worklet transfer and different-rate/grid/capacity adaptation remain subsequent
work. Actual device hot swap and gapless/acoustic behavior are not established.

Both paired writers returned terminal stop reports. Eight fixture groups are
authored: three generic Mixer/port, two actual planar-renderer and three ALSA
stop/join/recovery cases. ALSA cases construct only memory workers and use real
software rendering/telemetry; they do not open NativePcm or acquire native clocks.
Authored thread paths remain unexecuted. Thread streams retain owned
mixer results after ordinary return; callback streams require successful retirement.
ASIO diagnostic failures remain separate from successful callback close proof.
Rendering after planar-renderer take refuses before modifying destinations.
After both terminal writer reports, scoped formatting and whitespace checks
completed. Workspace/all-targets with WebTransport, no-default-features
WebTransport/all-targets, WASM browser/library and WASM browser-audio/library
compile-only checks each exited zero in sequence. Host checks compile the
generic/planar/ALSA fixture children; WASM library checks cover common production
code without tests. Existing unused-code warnings remain. No assertions or
authored threads executed. Assertions, native threads in fixtures, hardware/runtime and formal
review/QA remain deferred. Host checks cover Linux/common source; Windows/macOS/
ASIO SDK branches require later target and device acceptance. Full Goal stays
active and unproven.
