# EXBMP definition integration

Integrates the parser foundation from `wf/bms-bga-fix` (`2192146`) into the
current application base `0ecaa74`. Sources remain MIT; no dependency or crate
is added.

Selected EXBMP definitions retain a path in the existing BMP namespace and
four exact ARGB bytes in `BmsChart::image_argb`. Definitions use existing
resource radix, duplicate policy, conditional selection and physical limits.
LastWins plain BMP removes preceding ARGB metadata. Source parsing performs
no asset IO and changes no gameplay scheduling or judging policy.

The durable contract is [BMS adapter requirements](../kernel/REQ__bms-adapter.md).
Five adapter fixtures cover byte/path retention, BASE62, malformed grammar,
all BMP/EXBMP duplicate combinations and source-seeded/gameplay identity.
Current integration verification:

- `cargo test -p beatkernel-bms --locked`: 76 passed, 0 failed, exit 0.
- Actual `load_bms` example: valid EXBMP and zero-byte/BMP00 fixtures exit 0;
  alpha 256, shared duplicate and missing argument fixtures exit 1 with the
  expected source-line or usage diagnostic.
- Workspace all-target check with `beatkernel-bms-runtime/webtransport`: exit 0.
- Browser release build and wasm-bindgen package generation: exit 0.
- Independent code/security reviews: scoped PASS, no findings. Independent
  CLI QA: scoped PASS with actual command execution.
- Independent browser QA: scoped interactive PASS. Actual File/DataTransfer
  BMS/PNG/WAV preparation produced 1 note, 1 sound, 1 image; Worker WebGPU
  rendered the distinctive image; trusted Play/KeyZ and AudioWorklet completed
  with Hits 1 / Misses 0 / Combo 1. Alpha 256 was refused at source line 6
  while preserving the previous prepared preview.

Evidence is in ignored `target/wf/exbmp-adapter-test.log`,
`exbmp-qa-cli-entrypoints.json`, `exbmp-qa-cli-workspace.log`,
`exbmp-browser-build.log`, `exbmp-qa-browser-fresh-cache.log` and corresponding
preview/live/invalid PNG screenshots. Browser fetched WASM SHA256:
`7507a467de9101047d454d47987cbbaf51ed9f3276ea248756b5e05da029012b`.
The browser opened before package regeneration initially cached the older
Worker WASM and refused EXBMP. Clearing the browser cache and disabling cache
before reload resolved this; current artifact identity was checked explicitly.
Runtime exceptions: none; favicon 404 and zero-instance draw warning observed.

These actual scoped results do not establish whole-task receipt attestation or
full-scope desktop/browser acceptance. Task verification/close is not attempted.

Known ceiling: application EXBMP transparency/color matching is unfinished.
Retained ARGB bytes do not imply that the existing renderer applies those
effects. Full player, device, networking and full-scope QA remain unfinished.
