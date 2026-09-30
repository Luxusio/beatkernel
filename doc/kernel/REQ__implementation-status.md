# Implementation inventory and outstanding acceptance

This inventory maps the ordered [plan](../../plan.md) to source present on
2026-09-30, following native BMS replay integration at commit `6fc523c`.
It records implementation locations, not independent review or phase acceptance.
The user's verification deferral remains in force. Earlier executed evidence in
the [primary goals](../common/REQ__project__primary-goals.md) applies only to the
revisions it describes; subsequent compile checks do not renew that evidence.

Paths below are relative to the repository root.

| Phase | Located source and contract | Outstanding acceptance or boundary |
| --- | --- | --- |
| 0: workspace | Root Cargo manifests, `.github/workflows/ci.yml`, core/platform separation | Current hosted CI execution is not established by local checks. |
| 1: time/transport | `crates/beatkernel/src/time/`, `transport/`; [contract](REQ__time-transport.md) | Current transport fixtures and boundary behavior still need execution. Integer representations do not prove physical synchronization. |
| 2: canonical input | `crates/beatkernel/src/input/`, platform keyboard mapping; [contract](REQ__canonical-input.md) | Current semantic/provenance fixtures need execution; native devices need comparison. |
| 3: binding | `crates/beatkernel/src/input/binding.rs`, binding example; [contract](REQ__binding.md) | Current ordered fanout/device fixtures need execution. Binding edits require caller coordination of held interaction state. |
| 4: Windows input | `crates/beatkernel-platform/src/windows/`, `raw_input.rs`, input-inspector example; [contract](REQ__windows-input.md) | Historical virtual keyboard evidence exists. Current source and physical timestamp/jitter comparisons remain unverified. |
| 5: chart compiler | `crates/beatkernel/src/chart/`, chart example; [contract](REQ__chart-compiler.md) | Historical golden evidence exists; current compiler/BMS changes require executed regressions. |
| 6: judge | `crates/beatkernel/src/judge/`, `interaction/builtin.rs`, judge example; [contract](REQ__judge.md) | Historical judge evidence exists; current profile, routing, ownership and restoration paths need execution. |
| 7: audio | `crates/beatkernel/src/audio/`, platform native audio modules; [core](REQ__audio.md), [Windows](../platform/REQ__windows-audio.md), [ASIO stream](../platform/REQ__asio-stream.md) | WASAPI shared/exclusive and optional ASIO buffer/callback/Mixer stream source exists. Actual SDK compilation, native playback, callback allocation/locking checks and scheduling fixtures remain deferred. ASIO final-host composition and presentation mapping remain pending. |
| 8: integrated loop | `crates/beatkernel/src/runtime/`, `telemetry/`, runtime/native BMS examples; [runtime](REQ__runtime.md), [benchmark](REQ__runtime-benchmark.md) | Current integrated fixtures, percentiles under executed workloads, native drop/underrun behavior and physical latency measurements remain deferred. |
| 9: visual projection | `crates/beatkernel/src/visual/`, external SVG example; [contract](REQ__visual.md) | Lane/Point/Path/Polar/Custom source and reusable frame/index paths exist; fixture execution and renderer QA remain deferred. |
| 10: replay/snapshot | `crates/beatkernel/src/replay/`, judge snapshots, runtime restart/playback, replay examples; [replay](REQ__replay.md), [restart](REQ__section-restart.md), [reverse](REQ__reverse-playback.md) | Current repeated replay/seek hashes, codec failures, reverse policy and native restart output remain unverified. Logical restoration does not guarantee acoustic restart alignment. |
| 11: generalization | `interaction/advanced.rs`, generalization example and fixtures; [contract](REQ__generalization.md) | Axis, dual-contact, repeated, composite, pointer and pose configurations exist. Current fixtures need execution; custom policies are not automatically validated by their trait boundary. |
| 12: Linux | `crates/beatkernel-platform/src/linux/`, `samples/bms-runtime/src/bin/linux_bms.rs`; [composition](REQ__bms-linux-native.md) | evdev/hidraw/ALSA source exists. Native permissions, device mapping equivalence, playback and timing require execution. |
| 13: macOS | `crates/beatkernel-platform/src/macos/`, `samples/bms-runtime/src/bin/macos_bms.rs`; [composition](REQ__bms-macos-native.md) | IOHID/CoreAudio source exists. Native mapping, callback lifecycle, playback and timing require execution. |
| 14: game adapter | `adapters/beatkernel-bms/`, separate `samples/bms-runtime/`; [adapter](REQ__bms-adapter.md), [preparation](REQ__bms-preparation.md), [native replay](REQ__bms-native-replay.md) | Core-only parser/rules adapter and final platform composition exist. Parser/fuzz fixtures, live capture/reconstruction, offline PCM and native replay require executed checks. BMS runtime remains a separate crate. |
| 15: host SDK | Existing Rust embedding API; [conditional status](REQ__sdk-status.md) | C ABI/C# integration is not activated: no concrete host requirement is confirmed. It is not an implemented SDK or an unconditional missing phase. |

## Specific limits retained by the inventory

Tracking's built-in touch policy acquires one device/control/contact and keeps
that identity. Cancellation or broken coverage misses; another contact cannot
finish the interaction. The plan assigns reacquisition/rebind policy to the
interaction/game layer, not the native backend. Custom evaluator hooks provide
that boundary, but there is no built-in configurable contact transfer policy or
executed rebind-policy fixture claimed here. Native lifecycle/provenance
preservation and game-level rebinding must be assessed separately.

Live play and logical replay use the same JudgeEngine transitions. Recorded BMS
audio uses hit-stage sound selection shared with live publication and the actual
Mixer, but does not reproduce historical device delays or dropped commands.
Finite native replay duration is wall time including preroll, not an automatic
end-of-song drain. See the replay audio and native replay contracts for limits.

The default asset decoder supports WAV. An injected off-thread decoder is an
extension point, not evidence of bundled FLAC/OGG/MP3 support. Native output source
currently includes WASAPI, ALSA and CoreAudio; it does not establish support for
every native output API or every installed device configuration. ASIO remains a
required optional backend. Its baseline licensing prerequisite is superseded by
the source/build policy below; native output acceptance remains pending.

## Source continuation on 2026-10-01

The game-owned `contact_rebind` example and generalization fixtures now cover
fixed-contact sustain versus same-device/surface reacquisition after normal
release with an inclusive grace. They use existing custom evaluator and
snapshot/replay hooks. This fills the authored custom lifecycle/rebind fixture
gap identified above; built-in Tracking still keeps its original contact.
See the [generalization contract](REQ__generalization.md) for its region,
ownership, cancellation and deadline limits. Fixture execution is still pending.
Rust 1.98.1 `cargo check --workspace --all-targets --locked` passed after this
continuation, compiling the example and twelve new fixture tests without running
them. Native platform source was unchanged; cross-target checks were not repeated.

## Source/build licensing decision on 2026-10-01

The user approved keeping project-authored source MIT and distributing ASIO SDK
combined builds under GPLv3 conditions. The [distribution contract](../platform/REQ__asio-distribution.md)
records third-party license preservation and Corresponding Source obligations.
This resolves the baseline policy prerequisite in the Phase 7 row. It does not
claim an ASIO backend or release artifact: WASAPI currently rejects ASIO as an
unavailable backend. The full-plan task has resumed to apply this decision;
verification remains deferred.

ASIO driver registration discovery source now provides explicit Windows registry
views and bounded canonical driver identities through a separate
[inspector/API](../platform/REQ__asio-driver-discovery.md). It incorporates no SDK
and opens no audio. This is progress toward ASIO selection, not an implemented
ASIO stream or executed device evidence.

ASIO continuation adds SDK-free `audio::asio` buffer/rate validation and an
optional Windows [SDK driver control bridge](../platform/REQ__asio-driver-control.md).
Exact sizes preserve reported bounds and granularity without implicit rounding;
external clock is distinct from a positive finite Hertz request. The SDK feature
uses a supplied SDK and C++ toolchain, retaining the default MIT build boundary.
Control queries and explicit hardware configuration are separate from the
owned buffer/callback stream described below. The current local Windows
C++ compilation path is unavailable; source does not establish native acceptance.
Nine portable configuration fixtures are authored; execution remains deferred.

SDK-free [planar ASIO PCM conversion](../platform/REQ__asio-pcm.md) now extracts
explicit interleaved mixer channels into eighteen native PCM layouts. It shares
integer quantization with the existing platform PCM converter while preserving
ASIO's distinct low-valid-bit container alignment. Endian, channel, finite-value,
exact-extent and allocation fixtures are authored, with execution still deferred.
The separate stream uses this converter for native buffer filling; no native
playback acceptance is claimed.

[ASIO stream source](../platform/REQ__asio-stream.md) now provides actual SDK
double-buffer preparation, callback admission/serialization, explicit channel
routing, actual Mixer delivery, B priming, one-time start and terminal cleanup.
The stable Rust callback context outlives native callback drain and driver release.
Raw native clock observations remain distinct from prepared software frames.
Seven actual-Mixer planar fixtures are authored and compile without execution.
Default Rust 1.98.1 host workspace and Windows GNU/macOS platform all-target
checks passed. A metadata-only Windows GNU compilation additionally type-checked
optional control/stream Rust source without activating Cargo SDK compilation or
linking C++. Actual SDK/MSVC build, native playback, presentation mapping and
final-host composition remain outstanding. This does not enable GNU SDK builds.

## Build evidence and completion boundary

After the native replay slice, Rust 1.98.1 locked all-target checks passed for the
host workspace and for platform/BMS packages targeting `x86_64-pc-windows-gnu`
and `x86_64-apple-darwin`. These are compilation checks; they do not establish
linking, native execution, ARM64 support, device output or fixture success.

The remaining full-plan acceptance includes current regression execution,
deterministic replay/seek comparisons, native Windows output and at least one
other native platform, callback audit, measured latency/jitter/drop/underrun
behavior, independent reviews and final QA. These activities remain deferred by
the user's sequencing instruction. ASIO implementation must follow the selected
GPLv3 combined-build distribution policy. No full completion verdict
follows from this source inventory, and the full-plan task remains open.
