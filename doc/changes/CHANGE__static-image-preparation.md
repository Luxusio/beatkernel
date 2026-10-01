# Bounded static visual preparation

The application now has explicit portable image decoding and asset preparation
APIs for BMP, PNG and JPEG. image0.25.10 is pinned with only these codecs enabled
and default features disabled. Codec code remains outside core/BMS adapter.
Raw RgbaImage and its existing64MiB size check are portable; GPU texture IDs
and mutation helpers remain graphics-only. Project code remains MIT and new
codec dependencies retain their exact permissive MIT/BSD/Zlib license texts
in the application's third-party directory.

ImageDecodeLimits validates positive bounds before codec work. It bounds input
length, dimensions, native decoded bytes and RGBA8 output; upstream limits are
set before pixel decoding, followed by checked dimensions/output preflight.
Decoding uses signatures, independent of file extensions, produces straight
RGBA8 without resizing/EXIF rotation, and preserves PNG alpha. Defaults are
4096px extents and64MiB encoded/native/output bytes. Configurable ceilings are
16384px and64MiB per buffer. These are not hard process-memory guarantees: some
upstream scratch limits are non-strict and native/RGBA/encoded buffers can
coexist during conversion. No allocation failure interception claim.

ImageAssets::prepare opens only referenced BGA images plus defined BMP00.
An explicit preparation call keeps image work out of audio-only load_prepared,
audio/input callbacks and frame queries. Paths use literal exact contained
resolution with no extension substitutions. Canonical aliases share an Arc
and decode once; retained-byte accounting counts their pixels once. Missing,
undefined, unsupported or malformed raster references retain explicit blank
reasons. Unsafe/escaping/nonfile/access failures and decode/reference/aggregate
limits reject the whole bank; no partially prepared bank escapes. Static
filesystem assumption is the same as existing audio lookup, not race-free
sandboxing. Invalid roots reject before lookup.

Defaults allow1296 referenced IDs and64MiB unique retained pixels; the bank
budget can increase to256MiB independently of the GPU's concurrent texture
slots. Image selection continues to use the existing exact original-song BGA
state. The caller borrows decoded pixels after preparation; deleting files
later does not change retained data. CPU bank clones share pixels; no per-note
signals or per-frame decoding. These APIs are not yet installed or displayed
in the native UI. GPU upload/cache lifetime and composition remain next work.
Videos, GIF/WebP/other raster formats, black-key layer transparency and
miss-triggered poor-overlay policy are still pending.

Authored codec fixtures cover original BMP padding and orientations, PNG alpha,
JPEG dimensions,16-bit native versus RGBA budgets, truncation and configurable
limits. A real prepared-audio/parsed-BGA filesystem fixture covers Unicode
paths, exact-budget deduplication, missing/undefined/unsupported/corrupt/unused
resources and retained pixels after source deletion. Additional fixtures cover
aggregate/reference/encoded limits, unsafe/directory/symlink escape and
transactional rejection without changing an existing bank. Fixtures are
authored for later execution, not executed acceptance evidence.

Upstream limit semantics are inspected in the [pinned reader source](https://github.com/image-rs/image/blob/v0.25.10/src/io/image_reader_type.rs)
and [limit source](https://github.com/image-rs/image/blob/v0.25.10/src/io/limits.rs).

Source checks: host workspace/all-targets, Windows GNU app/all-targets, macOS
app/all-targets, app no-default/all-targets and WASM graphics/lib all finished
with exit0. Scoped formatting/whitespace checks exit0. Existing dependency
warnings remain; no tests, GUI/GPU/device execution or formal review/QA.
