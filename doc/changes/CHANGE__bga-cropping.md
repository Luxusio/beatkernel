# Prepared static BGA cropping

Selected #BGA and #@BGA headers now define typed crop resources independently
of original BMP paths. The [BMS command memo](https://hitkey.nekokan.dyndns.info/cmds.htm)
and its [author’s BMSE crop notes](https://hitkey.nekokan.dyndns.info/bmse_help_full/bga.html)
describe source coordinates, destination placement and varying player behavior.
BeatKernel explicitly uses half-open corners and a fixed256x256 transparent RGBA
canvas, clamps negative source origins before placement, and clips source and
destination. There is no inclusive endpoint, canvas spill, tiling or complete
historical compatibility claim. Signed i32 coordinates and #@BGA additions are
checked; source IDs use one/two ASCII base36 digits without guessed decimal mode.

Crop definitions win over same-ID BMP resources and can supply initial Poor00.
Crop sources always refer to original BMP files, so self-reference and swapped
IDs do not create recursive graphs. Headers share existing conditional/duplicate
policies; BMP and crop namespaces remain separate. Source gameplay/audio/replay
identity remains unchanged when physical gameplay lines remain fixed.

Preparation gathers displayed IDs plus their original dependencies under one
reference limit, loads contained literal files through canonical alias sharing,
and propagates missing/unsupported/invalid source reasons without BMP fallback.
Raw source buffers remain retained and charged. Identical normalized definitions
on canonical aliases share one prepared canvas. Fixed crop bytes are admitted
before allocation; original raw, unique crops and changed Layer variants all
count toward the aggregate bank budget. Layer black key applies after cropping;
Base/Poor preserve produced RGBA alpha. Unused definitions do not open files.

Existing native atomic chart/bank publication, texture cache, aspect-fit Scene,
opacity and original-song timelines consume the prepared resources directly.
No perframe crop/decoder/pixel work, dependency, thread or shader was added.

Authored fixtures cover parser sugar/duplicates/seed/identity/overflow,
literal pixels/alpha/negative/extreme clipping, self/swap dependency semantics,
canonical crop aliases, initialPoor00, unavailable sources, aggregate admission,
pre-IO fabricated validation, cache ownership/Scene and actual Runtime STOP,
capture/replay/native publication/fresh-practice selections. Fixtures remain
prepared for later execution; native pixels and performance are unverified.

Known ceiling: fixed256canvas/half-open/clamped policy only; custom canvas size,
BM98 inclusive endpoints/overspill and historical source-index variants remain
unsupported. ARGB RGB/color-key, video and full native acceptance remain pending.
Allowed source checks succeeded for workspace/all-targets, Windows GNU/macOS
all-targets, no-default-features/all-targets and WASM graphics/library. Scoped
rustfmt and diff checks succeeded. Existing macOS block future-compatibility
and WASM cadence warnings remain. Tests/apps/native/device/GPU and formal
review/security/QA/verification/close remain deferred.
