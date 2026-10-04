# Shared local member preparation

Prepare actual local member configurations from the shared resolved input plan,
one BindingMap per member, the existing PreparedBms, an explicit judge profile
and BMS input mode. Validate setup before constructing independent existing core
judges. Reserve actual BGM voices and use the existing VoiceAllocator to remap
key-sound voices deterministically across members while preserving SampleIds,
same-member replacements and the original PCM bank.

Native cohort preparation calls this same builder with its existing keyboard
maps and ButtonOnly mode. Native attachment constraints, recording paths,
capture/completion and opponent loading remain host-owned. The common builder
contains no file/device/platform API and creates no new judge or PCM copy.

Coverage scans source notes once and reuses the at most eighteen distinct lanes
for every member. Five independently authored deferred fixture groups exercise
MemoryFiles/WavDecoder preparation, real three/four-member RuntimeGroup and
Mixer over the shared original bank, contact mode versus ButtonOnly, invalid
setup and namespace boundaries, same-member voice replacements, and actual
native cohort parity including recording identities and legacy constraints.

Scoped Rust formatting and staged whitespace inspection completed cleanly.
Four compile-only configurations completed with exit zero: workspace all
targets, headless app all targets, wasm32 browser library and wasm32 audio
library. Existing three platform cadence dead-code warnings remain on WASM.
Tests/assertions, generated bindings, app/browser/device/audio/network runs,
formal reviews and QA were not executed under the user's verification deferral.

This implements the shared prepared-member layer needed by a nonblocking
browser group owner. It does not establish browser device assignment,
multi-field presentation or playable local multiplayer. Full runtime/device
acceptance and required ordered reviews and browser/CLI/desktop QA remain pending.
