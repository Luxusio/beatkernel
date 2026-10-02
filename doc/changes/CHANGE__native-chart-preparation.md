# Shared native chart preparation

All six Linux, Windows and macOS solo/local compositions use
`native_chart::prepare_chart` for seeded chart/asset loading, practice slicing
and required lane-binding coverage. The same caller-supplied PCM limits apply to
loading and overlapping BGM suffixes. Windows supplies its negotiated format;
Linux and macOS retain their configured format and channel policy.

Negative starts reject before loading. Binding coverage uses only the sliced
chart, so excluded heads and crossing holds do not require keys. Missing binding
errors now precede completion and warning reporting and use one channel diagnostic.
Absolute song times, parser warnings and original PCM tail selection are preserved.

The existing chart publication and cancellation calls keep their original
positions relative to native input selection and ownership. The helper does not
publish UI state or eagerly prepare images in headless sessions. No new crate,
dependency, native resource operation or callback work is introduced.

## Evidence and remaining work

Five source-only fixture groups cover exact configuration/seed forwarding, zero
section preservation, independent excluded head/hold lanes and retained absolute
targets, real PCM tail selection, negative-start admission and exact loader/section
errors, including the suffix byte budget. They were authored and compiled without
execution.

All five compile-only configurations passed on their first attempt: Linux
workspace/all targets, Windows GNU application/all targets, macOS application/all
targets, headless application/all targets and the WASM graphics library. Dedicated
`target/ac150-{host,windows,macos,headless,wasm}.exit` artifacts each contain zero.
Scoped formatting and diff checks passed. Existing macOS `block` future-compatibility
and WASM cadence warnings remain.

Runtime, device, file, GUI, network and formal acceptance remain deferred.
Remaining native composition and ASIO interval-aware startup still need work;
this extraction does not establish physical playback accuracy or full player
completion.
