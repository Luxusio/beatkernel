# Native asynchronous catalog startup

`player --library` prepares metadata, normalized catalog search and optional
title-font CPU data on one owned background operation. The native window starts
with an explicit loading selection and bounded latest progress counters. Only a
complete prepared catalog can replace selection entries, search, diagnostics
and font ownership; no partial entry list is published per frame. Catalog work
does not open sound assets.

The synchronous scanner remains available. Cooperative checkpoints surround
traversal, bounded chart reads, parsing and preparation while retaining existing
deterministic ordering, symlink refusal and catalog limits. Closing cancels the
operation and waits for its actual completion; ordinary UI polling never joins
a running worker. Hidden or suspended selection retains completed work until
its own screen is active. Closing results are discarded, and optional font GPU
upload remains on the renderer owner.

## Verification state

Independent deferred fixture source adds six groups: two scanner groups, two
owned-worker groups and two actual Desktop groups. They cover synchronous scan
compatibility, ordering, symlink/byte/chart/depth limits, cancellation without
partial catalogs, latest progress observation, actual thread completion and
joined release, loading launch refusal, atomic search/font/diagnostic install,
hidden/suspended retention, closing discard and visible preparation failure.
The filesystem and thread operations in those fixture bodies have not run.

Both source writers actually stopped before scoped Rust formatting and
whitespace inspection. Workspace/all-targets with WebTransport (session 22400),
native no-default/all-targets with WebTransport (56667), WASM browser (93619) and
WASM browser-audio (11621) each exited 0. WASM retained three existing cadence
dead-code warnings. No tests, applications, filesystem fixture probes, generated
bindings, device execution or formal review/QA have run for this phase. These
are compile-only checks; the full player Goal and Harness task remain open.

## Known ceiling

Cancellation cannot interrupt arbitrary filesystem calls or decoder work.
Unexpected final Drop must join its real owner and may wait for such a call.
Profile loading and direct `--chart` startup remain separate existing paths.
Optional font GPU upload can still require renderer time. The advertised-size
aggregate accounting present in this initial phase is superseded by
[actual catalog read accounting](CHANGE__catalog-read-budget.md), which charges
returned raw bytes and bounds the aggregate detection probe.
Actual native window/filesystem/device behavior and measured responsiveness
remain unverified.
