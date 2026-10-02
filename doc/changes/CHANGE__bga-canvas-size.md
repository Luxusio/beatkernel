# Explicit static BGA canvas dimensions

Selected #CANVASSIZE headers now set the static BGA canvas width and height.
The [author’s translation of the Sonorous proposal](https://hitkey.nekokan.dyndns.info/bmsexts-ja.htm)
describes two positive one-to-four digit decimal fields, last-valid selection,
ignored malformed headers, and top-left image padding/cropping. BeatKernel uses
those header rules, including last valid wins independently of the generic
DuplicatePolicy; invalid active headers produce a warning without erasing a
previous valid size. A checked accessor rejects malformed fabricated metadata.
This is support for a proposal, not a claim of universal format compatibility.

Explicit dimensions apply to ordinary images and #BGA/#@BGA crop planes.
Source pixels are copied without stretching, clipped at the plane and padded
with transparent black. Without the header, ordinary images retain their source
extent and existing crop_canvas retains its 256x256 behavior. Exact-size ordinary
images share original storage. The new crop_canvas_sized API uses real row
stride, output dimensions and the same half-open/clamped-origin crop policy.

Image preparation checks canvas metadata, configured dimension limits and
checked per-image RGBA byte limits before filesystem work. Header dimensions
can be1..9999, but configured bounds and the existing64MiB image ceiling still
apply. Canonical aliases and equal normalized raw/crop transforms share output
Arcs. Retained sources, changed canvases and keyed Layer variants all count toward
the aggregate bank budget; allocation follows admission. Unavailable resource
semantics, crop precedence, initialPoor and native atomic publication remain.

Existing GPU ownership and Scene aspect-fit rendering use produced canvas
extents, preserving its aspect ratio below gameplay. No dependency, shader,
perframe pixel operation or native thread changes were added. Gameplay objects,
keysounds and replay identity remain unchanged when gameplay lines stay fixed.

Authored fixtures cover strict/invalid/duplicate/conditional header selection,
fabricated data and gameplay identity, literal non-square padding/clipping,
extreme/byte bounds, legacy256 compatibility, alias sharing across ordinary and
crop resources, exact-size no-copy accounting, pre-IO bounds, native publication,
cache/Scene aspect fit, actual Runtime STOP/capture/replay/fresh-practice images.
They are prepared for later execution. Allowed source checks succeeded for
workspace/all-targets, Windows GNU/macOS all-targets, no-default-features/all-targets
and WASM graphics/library. Scoped rustfmt and diff checks succeeded. Existing
macOS block future-compatibility and WASM cadence warnings remain.

Known ceiling: native pixels, performance and full proposal conformance remain
unverified; videos and ExtChr do not gain support. Default native preparation
still uses its existing4096 extent and64MiB aggregate/image limits; callers can
select admitted ImageAssetLimits. BM98 inclusive endpoints/overspill and ARGB
color-key extensions remain unfinished. Test/app/native/GPU/device and formal
review/security/QA/verification/close execution remains deferred.
