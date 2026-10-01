# Exact-black BGA Layer preparation

Static channel 07 Layer images now use separately prepared RGBA8 variants:
exactly RGB (0,0,0) becomes transparent before GPU texture filtering. Channel
04 Base and Poor resources keep original pixels, including when canonical
path aliases reference the same file in multiple roles. The cache chooses the
role-specific Arc and deduplicates aliases; colored/no-op variants share their
original buffer and upload. Preparation runs on the game owner before chart
publication, with no per-frame pixel scan or shader changes.

The [BMS command memo author's channel 07 notes](https://hitkey.nekokan.dyndns.info/cmds.htm#07)
describe black transparency and historical implementation differences.
BeatKernel explicitly chooses exact black in RGBA8; near-black pixels and
existing nonblack alpha remain unchanged. Thresholds, EXBMP, Poor activation,
video and other extensions remain unfinished.

Changed unique variants count toward the existing aggregate decoded-byte
limit in addition to retained raw images. A changed image can therefore cost
twice its raw RGBA bytes. Limits are retained-storage admission limits, not a
bound on decoder scratch or total process memory. Atomic preparation failure
does not publish a partial bank. GPU upload on selection changes can still
stall the UI or exceed its separate resource budget.

Authored fixtures cover exact-black/near-black/alpha/source immutability,
aliases, aggregate capacity, separate Base/Layer uploads and native chart
publication. Tests and native GPU execution remain deferred; source compilation
does not establish pixel correctness, timing performance or platform acceptance.

Allowed source checks succeeded for workspace/all-targets, Windows GNU and
macOS all-targets, no-default-features/all-targets, and WASM graphics/library.
Scoped rustfmt and diff checks succeeded. Existing WASM cadence warnings and
the macOS block dependency future-compatibility warning remain. No tests,
applications, device/GPU execution or formal review/QA gates ran.
