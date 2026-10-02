# Original-song BGA channel opacity

The adapter accepts hexadecimal channels 0B/0C/0D/0E as independent Base,
Layer, Layer2 and Poor opacity markers. Typed alpha events and schedules share
the checked visual grid, acquisition ordinals, BPM/STOP rescaling, seeded
selection and source caps with image events, while using separate duplicate
namespaces. Combined image/opacity counts are bounded for fabricated charts too.
Gameplay objects, BGM scheduling and replay identity remain unchanged when
physical gameplay lines stay fixed.

The [BMS command memo](https://hitkey.nekokan.dyndns.info/cmds.htm) documents
these four channels and the hexadecimal range. BeatKernel explicitly uses
direct byte alpha/255: default255, 00 rest, 01 means1/255, FF means1.
There is no interpolation or inferred historical threshold/rescaling. This is
an implementation policy, not a claim of complete Nanasi/ARGB conformance.

Prepared four-channel indexes answer the exact original-song prefix without
forward cursors, allocation or image work. PlayerChart and full BgaPresentation
carry opacity through live/local/replay publication. Replace uses Poor opacity
for its substituted Base; Overlay preserves independent normal and Poor alpha;
Off retains normal alpha. Selection-only compatibility APIs cannot carry opacity.

The texture cache retains image Arc identity independently of alpha, so alpha
changes require neither upload nor release. Scene uses existing RGBA vertex
color and shader multiplication, preserving image alpha, exact-black layer
preparation, clipping, tint, aspect fit and painter order beneath gameplay.
Opaque sprite callers remain compatible. No shader layout or dependency changed.

Authored fixtures cover hex/rest/duplicate/conditional/cap/identity/STOP timing,
default/equal-time/pause/backward/extreme-time queries, sixteen-resource cache
reuse during alpha-only changes, clipped alpha/tint/UV/invalid geometry, ordered
four-role rendering, member clocks and actual Runtime capture/replay/native
publication with all Poor modes. Fixtures are prepared for later execution;
actual blending pixels, native devices and platform performance remain unverified.
ARGB RGB/color-key, crop and video remain unfinished.

Allowed source checks succeeded for workspace/all-targets, Windows GNU/macOS
all-targets, no-default-features/all-targets and WASM graphics/library. Scoped
rustfmt and diff checks succeeded. Existing macOS block future-compatibility
and WASM cadence warnings remain. Tests/apps/native GPU/device/bench and formal
review/security/QA/task verification/close remain deferred.
