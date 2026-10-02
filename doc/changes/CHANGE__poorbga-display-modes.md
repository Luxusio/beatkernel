# POORBGA display modes

Selected case-insensitive POORBGA headers now accept exactly 0 (Replace),
1 (Overlay) and 2 (Off), defaulting to Replace. The adapter preserves metadata
and applies existing duplicate and seeded-conditional policies; active malformed
values reject. The checked accessor also rejects fabricated invalid metadata
before preparing the cached typed mode on PlayerChart. Display mode does not
alter gameplay objects, keysounds, timing, grading or replay identity when
gameplay physical line positions remain unchanged.

The [BMS command memo author's POORBGA notes](https://hitkey.nekokan.dyndns.info/cmds.htm)
describe replacement, topmost miss overlay and disabled miss display. Native
solo/local/replay rendering now consumes complete BgaPresentation intent:
Replace uses raw Poor as Base, Overlay keeps Base/Layer and paints raw Poor
last under gameplay, and Off keeps the normal background. The existing 500ms
original-song interval and independent full-prefix miss state remain shared.
Poor alpha is preserved; Layer's exact-black transformation is not applied to
Poor. Unavailable overlays are omitted and explicitly counted as unavailable.

The GPU owner cache now admits up to twelve distinct CPU image Arcs across
four visible views. It reuses raw Base/Poor aliases, retains the existing GPU
slot and byte limits, releases expired selections and bounds failed-upload
retries to union changes. No shader or per-frame decoder/pixel transformation
was added. The old project API exposes only selection state; overlay callers
use select. Legacy cache sync does not infer Poor activation.

Authored fixtures cover parser modes/errors/defaults/duplicates/branches,
unchanged gameplay identity, lifetime/identity bounds, per-member native
selection, raw alias sharing, twelve-resource admission/failure/release, ordered
Scene composition and actual Runtime timeout/capture/replay/native publication
for all modes. Execution and native GPU/performance acceptance remain deferred.
Layer2, opacity/crop extensions and video remain unfinished.

Allowed source checks succeeded for workspace/all-targets, Windows GNU/macOS
all-targets, no-default-features/all-targets and WASM graphics/library. Scoped
rustfmt and diff checks succeeded. Existing WASM cadence and macOS block
future-compatibility warnings remain. No tests/apps/device/GPU/bench or formal
review/security/QA/verification/close gates ran.
