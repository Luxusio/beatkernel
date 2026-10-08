# Implementation inventory and outstanding acceptance

The canonical full product TODO/count ledger is the
[BMS player WBS](REQ__bms-player-work-breakdown.md). Run
`python3 tools/wbs_status.py` for leaf counts and per-domain status; the
historical feature survey and a child task's milestones use different scopes.

## Current authority and historical boundary

The feature-level survey at revision `5ddc850` on 2026-10-07 in
[BMS player progress](REQ__bms-player-progress.md) is a historical baseline.
The ongoing [parallel requirements](REQ__parallel-player-requirements.md)
work adds the evidence table below without declaring full-player acceptance.
The sections after that table are historical source observations
from 2026-10-01 and subsequent dated continuations, not a current completion
ledger. In particular, old WAV-only, unavailable-backend or deferred-execution
statements must not override current source and fresh evidence. The user lifted
verification deferral on 2026-10-06; historical deferral text below is superseded.
Neither the old inventory nor the current survey establishes full-player,
native hardware or measured performance acceptance.

## Parallel implementation evidence on 2026-10-09

These are bounded development checks, not full-player acceptance. The current
security review passed; the code review's missing native Desktop component
motion has been implemented and tested, and stale document statements were
corrected. The fresh full code review passed. Independent
review and CLI/browser/desktop QA remain required before task close.

The latest combined app library run (`desktop,webtransport`) passed 2099 tests,
with 0 failures and 4 ignored. Linux binary tests passed 34/0/3, platform library
131/0/1, and the browser Node suite 692/0. Each total belongs to its own command;
they must not be added into a TODO ratio. The pack retains 24 criteria and 17
development milestones; the canonical WBS remains the completion-count ledger.

| Requirement slice | Actual evidence | Remaining integration or proof |
| --- | --- | --- |
| Retained layout and declarative screens | All migrated screen fixtures included in app 2099; stable mounted geometry and partial updates | Full native/browser route interactions and independent GUI QA |
| Mixed-rate target output | App 2099, platform 131; Linux solo/local and network committed-start consumers compile and Linux tests pass | Clock-capable full play, socket-to-driver synchronization, replay/other backend and interval integration |
| Converted output settings and ownership | Actual typed owner/request/reply/recovery fixtures; Linux consumers connected | Actual endpoint changes and hardware presentation/latency |
| Chained same-format Vorbis | Decoder/preparation/Mixer-tail fixtures executed | Mixed-format chaining remains explicitly rejected |
| Selected-policy network admission | Policy/identity/launcher admission fixtures; WBS E8 actual selected cohort production-owner/header routing | Selected peer exchanges and cross-platform physical playback |
| Browser Worker menus and record preview | App fixtures, Node 692; rebuilt WASM and export smoke | All-route editor/permission/record interaction and independent browser QA |
| Component motion | Browser SwiftShader 51 submitted frames (E18); native binary 281 tests (E24), actual X11/llvmpipe Vulkan 22 animation frames and shifted hits/cache/disposal (E25); full code review PASS | Hardware performance and independent UI QA |
| Real transport acceptance | QUIC 5 actual loopback cases (E12), native HTTP/3 room 4 cases with owned relay cleanup (E14) | Browser trusted WebTransport interoperability, socket-to-driver and acoustic synchronization |

The full requirement scope remains in the parallel requirements document and
the original plan acceptance map. Historical timing presets, empty-POOR and
EXBMP/video policies, cross-backend continuity, DDD/port coverage, RT/fuzz/stress,
performance comparisons, OS/device matrix, licensing/release and final docs
remain open wherever direct acceptance evidence is absent. Conditional C ABI/C#
SDK activation still requires a concrete host requirement.

## Historical source inventory

This inventory maps the ordered [plan](../../plan.md) to source present on
2026-10-01, including native BMS replay, live ASIO composition and the
subsequent Runtime-driven visual example.
The [unified BMS competition application](REQ__bms-competition.md) additionally
implements own/other saved opponents and two-peer TCP progress exchange within
the existing app crate. The main binary dispatches all modes; native play owns
input/judging on a game thread, with separate native output and network owners.
The product UI uses winit/wgpu, with actual chart, score and local-player
snapshots. Competition prefixes are connected to that same bridge. Display
drafts and version 2 player profiles include GPU backend, presentation, FPS and
lookahead; version 1 loads with display defaults. Native Linux, Windows and macOS local
groups compose independent player runtimes with one shared output. Peer progress
is unauthenticated with independent local starts. Source checks exist;
actual socket exchange, gameplay and fixture execution remain deferred.
Graphical retry source retains accepted invocation state and starts a fresh native
session after cleanup, with independent recording stems. Configured fresh practice starts now retain original targets and select overlapping
BGM suffixes. Section recordings now store their original-song start and restore
logical replay, matching ghosts and audio preparation from original assets in
source. Settings Records now discovers direct saved-record files and previews
their actual recorded prefixes against the current chart/profile/section on
the serialized metadata worker, before adding own/other ghosts to the draft.
Live scrubbing/loops and acoustic restart acceptance remain unfinished.
It records implementation locations, not independent review or phase acceptance.
The [full-plan acceptance evidence](REQ__plan-acceptance-evidence.md) maps every
§24 completion condition and §17 verification category to current source and
remaining proof, preserving the original scope.
The user lifted the verification deferral on 2026-10-06. Earlier executed evidence in
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
| 7: audio | `crates/beatkernel/src/audio/`, platform native audio modules; [core](REQ__audio.md), [Windows](../platform/REQ__windows-audio.md), [ASIO stream](../platform/REQ__asio-stream.md) | WASAPI shared/exclusive and optional ASIO buffer/callback/Mixer stream source exists. ASIO recorded and live BMS host source, finite presentation mapping and continuous correction are implemented. Actual SDK compilation, native playback, callback allocation/locking checks and scheduling fixtures remain deferred. |
| 8: integrated loop | `crates/beatkernel/src/runtime/`, `telemetry/`, runtime/native BMS examples; [runtime](REQ__runtime.md), [benchmark](REQ__runtime-benchmark.md), native selected-input cadence for [Linux](../platform/REQ__linux-input-cadence.md), [Windows](../platform/REQ__windows-input-cadence.md), [macOS](../platform/REQ__macos-input-cadence.md) | Native acquisition source now feeds explicit selected-signal interval summaries with backend-specific timestamp meanings. Current integrated fixtures, percentiles under executed workloads, native drop/underrun behavior and physical latency measurements remain deferred. |
| 9: visual projection | `crates/beatkernel/src/visual/`, external SVG examples including `runtime_visual`; [contract](REQ__visual.md) | Lane/Point/Path/Polar/Custom source and reusable frame/index paths exist. The Runtime-driven composition uses actual reports with four lanes and a path; example/fixture execution and renderer QA remain deferred. |
| 10: replay/snapshot | `crates/beatkernel/src/replay/`, judge snapshots, runtime restart/playback, replay examples; [replay](REQ__replay.md), [restart](REQ__section-restart.md), [reverse](REQ__reverse-playback.md) | Current repeated replay/seek hashes, codec failures, reverse policy and native restart output remain unverified. Logical restoration does not guarantee acoustic restart alignment. |
| 11: generalization | `interaction/advanced.rs`, generalization example and fixtures; [contract](REQ__generalization.md) | Axis, dual-contact, repeated, composite, pointer and pose configurations exist. Current fixtures need execution; custom policies are not automatically validated by their trait boundary. |
| 12: Linux | `crates/beatkernel-platform/src/linux/`, `app/src/bin/linux_bms.rs`; [composition](REQ__bms-linux-native.md) | evdev/hidraw/ALSA source exists. Native permissions, device mapping equivalence, playback and timing require execution. |
| 13: macOS | `crates/beatkernel-platform/src/macos/`, `app/src/bin/macos_bms.rs`; [composition](REQ__bms-macos-native.md) | IOHID/CoreAudio source exists. Native mapping, callback lifecycle, playback and timing require execution. |
| 14: game adapter | `adapters/beatkernel-bms/`, separate `app/`; [adapter](REQ__bms-adapter.md), [preparation](REQ__bms-preparation.md), [native replay](REQ__bms-native-replay.md) | Core-only parser/rules adapter and final platform composition exist. Parser/fuzz fixtures, live capture/reconstruction, offline PCM and native replay require executed checks. BMS runtime remains a separate crate. |
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
Explicit native replay seconds is a wall cutoff including preroll. Omitted
seconds now finishes the actual recorded prefix and PCM through native
presentation drain, without synthetic missing-tail judging or acoustic proof.
ASIO recorded output currently requires explicit diagnostic seconds because its
output-zero presentation epoch is not established. See native replay limits.

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
linking C++. Actual SDK/MSVC build and native playback remain outstanding.
Presentation mapping and live-input host composition were implemented in the
later source continuations below. This does not enable GNU SDK builds.

The native replay host now selects ASIO explicitly through the sample's optional
SDK feature, exact CLSID/view and distinct output channel mapping. It reuses the
checked replay/JudgeEngine audio plan, actual Mixer and rolling command feeder,
retaining a hidden same-thread HWND through stream teardown. Eight portable CLI
fixtures are authored. A coherent buffer observation now pairs each successful
callback's copied raw event with the actual rendered Mixer block; post-create
driver latency remains available. These are inputs for future live presentation
mapping, not normalized host time or physical playback evidence.

Bounded clock-source metadata and actual SDK control source now enumerate/select
explicit internal/external indices before stream construction, with preserved
raw names and associated input metadata. Eight portable fixtures are authored.
Time-info clock/rate change flags require reopening. Actual SDK C++ compilation
and driver control execution remain unverified; this does not complete live ASIO
host timing or deferred native acceptance.

Windows multimedia timer acquisition now retains actual shared QPC brackets.
Finite modular relations and ASIO presentation observations pair actual rendered
Mixer frame identity with native switch time and explicit output-latency bounds.
Two-observation calibration enforces rate/origin/grid identity and finite mapping
validity, accounting for interval width and extrapolation. Continuous discipline
admits ASIO blocks separately from WASAPI counters and supplied pairs, with no
duplicate freshness updates. Unknown residual drift and rolling midpoint accuracy
remain Unknown; no acoustic accuracy follows from compilation.

The Windows live BMS sample now composes explicitly selected optional ASIO
through the same physical input, binding, JudgeEngine/Runtime, BGM admission and
accepted-operation replay capture as WASAPI. It queries actual driver rate and
selected channels, requires explicit multimedia timer/error assumptions, refreshes
finite timer anchors and feeds ASIO presentation discipline. Coarse timer plateaus
wait without refreshing progress. The hidden driver window remains alive until
stream close/drain; startup driver messages are serviced with bounded work.
Nine portable CLI fixtures are authored and compiled only. Source composition
does not establish actual SDK/MSVC build, device compatibility, native output
or physical input-to-sound timing.

## Build evidence and completion boundary

After the native replay slice, Rust 1.98.1 locked all-target checks passed for the
host workspace and for platform/BMS packages targeting `x86_64-pc-windows-gnu`
and `x86_64-apple-darwin`. These are compilation checks; they do not establish
linking, native execution, ARM64 support, device output or fixture success.

ALSA's [direct render-worker cadence](../platform/REQ__alsa-render-cadence.md)
now captures actual pre-Mixer monotonic timestamps with successful frame identity
and summarizes a finite prefix after worker join. The Linux native audio example
prints this alongside applied sizes and actual xrun counters. Startup fill bursts
remain included. WASAPI and CoreAudio now reuse shared direct pre-Mixer capture
at their QPC/mach boundaries. Optional ASIO also captures actual callback renders
using an explicit shared QPC clock; default clock-free preparation remains
available. These source slices do not establish hardware results.

The remaining full-plan acceptance includes current regression execution,
deterministic replay/seek comparisons, native Windows output and at least one
other native platform, callback audit, measured latency/jitter/drop/underrun
behavior, independent reviews and final QA. These activities remain deferred by
the user's sequencing instruction. ASIO implementation must follow the selected
GPLv3 combined-build distribution policy. No full completion verdict
follows from this source inventory, and the full-plan task remains open.


## Graphical recorded playback source slice

The single graphical app now connects compatible Records previews to Watch,
reusing the live lane/score view, cancellation and cleanup-gated F5 lifecycle.
An incremental validated ReplayVisual applies actual ordered record operations;
record prefix results remain distinct from full-song completion. Output-only
settings projection resolves default endpoints on the game owner without input
discovery; recorded profile/start are authoritative and live configuration is
unchanged. WASAPI/ALSA/CoreAudio presentation drives the UI bridge; render/UI/wall
time never substitutes for unavailable native presentation. Natural replay ends
only after actual operations and PCM drain. ASIO's recorded-host epoch remains
unestablished, so natural mode rejects and explicit seconds is diagnostic audio
without graphical progress. Source fixture authorship and compilation are not
executed GUI/native/audio acceptance; deferred independent gates remain open.
