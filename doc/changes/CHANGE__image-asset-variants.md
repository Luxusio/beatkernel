# Missing static image filename variants

The shared static image bank now resolves a genuinely missing BMP/PNG/JPG/JPEG
reference through bounded same-stem extension variants. Filesystem and browser
selected-file sources use the same policy: literal first, original recognized
family first, then remaining BMP/PNG/JPG/JPEG families, all ASCII extension
case combinations with at most 40 candidates. Extensionless references use the
default family order; unsupported suffixes stay literal-only. Directory and
Unicode stem spelling remain unchanged. Decoding still follows signatures,
and damaged or unsupported existing files never trigger another search.

This supersedes the literal-only image lookup described in
[initial static preparation](CHANGE__static-image-preparation.md). General
`Exact` defaults and audio family priority stay intact. Referenced IDs, crop
dependencies and initial Poor images use the existing resolved-key cache and
retained pixel budgets. Canonical aliases decode once and share their raw,
cropped and keyed variants. Original chart references and gameplay/replay
identity stay unchanged. Actual native publication and browser preparation
already call this shared bank, so no additional platform-specific search or
per-frame decoding is introduced.

An existing dangling reference previously returned NotFound and could become
an ordinary unavailable image. After directory-entry presence is established,
canonicalization NotFound now returns InvalidData. Both image and audio lookup
reject that reference without variant fallthrough; ordinary absent references
retain their existing missing behavior. Escaping links, directory candidates,
permission errors and resource limits keep their rejection paths. Containment
assumes a trusted static filesystem and does not provide race-free sandboxing.

Five independent fixture groups are authored for later execution, covering bounded case
and family order, filesystem/selected-file parity, scope and literal priority,
directory/dangling/escaping failures, actual image-bank decoding, unavailable
literals, alias/crop/Layer sharing, original identity and budgets. Existing
audio dangling-error fixtures are updated for the explicit error kind.

After both source and fixture writers returned terminal STOPPED, scoped
Rust formatting and whitespace checks finished successfully. The four locked
compile-only checks also finished with exit 0: workspace/all targets with
WebTransport, no-default WebTransport/all targets, WASM browser/lib, and WASM
browser-audio/lib. Existing WASM cadence unused-code warnings remain. No tests,
browser, audio, device or formal review/QA execution was performed.

## Known ceiling

Tests and actual browser/native image rendering remain deferred. Compilation
does not establish correct decoded pixels, rendered blending or performance.
Video, EXBMP/ARGB color policies, ExtChr and the remaining player requirements
are still unfinished. This slice does not complete the whole Goal or provide
review/QA PASS evidence; browser QA remains required before eventual close.
