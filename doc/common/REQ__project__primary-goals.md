# BeatKernel primary goals

BeatKernel is a low-latency, cross-platform Rust runtime for rhythm games, distributed as a public library under the [MIT license](../../LICENSE).

## Required behavior and boundaries

- Implement the full ordered specification in [plan.md](../../plan.md), including binding, native input, chart compilation, judging, audio scheduling, rendering primitives, replay, and remaining platform backends.
- Keep exactly two initial crates: `beatkernel` for platform-independent primitives and `beatkernel-platform` for OS integration. Dependencies flow from platform to core. Later parser adapters and conditional FFI follow the specification's separate phases.
- Use integer nanoseconds for runtime timestamps. Preserve input source, native metadata, original clock points, and deterministic ordering through normalization and later mapping.
- Keep game-specific rules and OS branches outside core. Avoid allocation, locks, and I/O in real-time audio paths when implemented.
- Preserve generic input capabilities beyond keyboards: axes, touch, pointers, pose, raw HID, and custom payloads.
- Keep the runtime independent of React and other UI frameworks.
- Windows audio output must let the caller select and configure an available native backend and device explicitly. Phase 7 must provide WASAPI shared and exclusive streams, expose supported formats, sample rates, channel layouts, buffer periods and clock/underrun telemetry, and return a specific error when the requested mode or device is unavailable; it must not silently change modes. ASIO is a required optional backend target for compatible installed drivers, with a separately verified licensing and distribution path before its SDK or bindings enter the MIT project. Additional native output APIs can be added through the same platform boundary only with a concrete device/API contract and runtime evidence. The OS-independent mixer and scheduler retain their real-time callback restrictions.

Windows' [stream-management contract](https://learn.microsoft.com/en-us/windows/win32/coreaudio/stream-management) distinguishes WASAPI shared and exclusive modes. Steinberg [publishes the ASIO SDK under GPLv3 or a separate proprietary agreement](https://github.com/audiosdk/asio/blob/main/LICENSE.txt); the project must resolve that choice before incorporating SDK-derived code or shipping an ASIO backend under the project's MIT distribution.

## Current evidence and remaining work

On 2026-09-30 the user instructed that verification be deferred and
implementation continue through the remaining phases. This overrides the
original phase-completion sequencing rule for implementation: pending Windows
audio verification must not block Phases 8 onward. Keep native execution,
independent reviews and final QA as outstanding work; do not mark them passed
or call the full runtime complete. Compile and format checks may accompany
implementation to keep the code buildable. MIT, dependency boundaries,
real-time restrictions and the ASIO licensing condition remain required.

Phases 0–3 are implemented: [time and transport](../kernel/REQ__time-transport.md), [canonical input](../kernel/REQ__canonical-input.md), pure keyboard mapping fixtures for Windows, Linux, and macOS, and [device-aware binding](../kernel/REQ__binding.md). Binding retains owned typed samples, provenance, and ordered logical destinations. [Phase 4 Windows input](../kernel/REQ__windows-input.md) passed independent review and CLI QA. Six native API integration tests and five platform unit tests pass in an isolated Windows Server VM. An earlier inspector build captured device-attributed A Down/Up, provenance and normal/Alt+F4 cleanup through a Hyper-V virtual keyboard; the latest revision passed finite native execution with no acquisitions in a locked guest. Native virtual-device execution does not establish physical hardware latency.

[Phase 5 chart compilation](../kernel/REQ__chart-compiler.md) passed independent review and CLI QA. It provides absolute object start/end times, rational BPM, integer STOPs and separate visual SV markers. Eleven debug/release golden tests cover timing boundaries, deterministic ordering and validation.

[Phase 6 judging](../kernel/REQ__judge.md) implements Instant/Hold evaluators, caller rule registration, configurable inclusive asymmetric windows, replaceable candidate/grading policies and ordered results with retained input provenance. Inputs and advances use explicitly mapped song time with the profile offset applied once. Hold heads acquire device/physical-control/logical-control ownership; only owner release can grade a tail, and early release or timeout produces an explicit miss. Repeat and duplicate Down do not create fresh builtin presses. Explicit start eligibility keeps builtin button/profile limits separate from custom evaluator predicates. Library validation errors preserve state; trusted infallible callbacks have a separate panic/side-effect boundary. Setup and dispatch/results may allocate; judging is single-owner and forward-only.

The Phase 6 baseline passed twenty-three targeted judge tests in debug and release,
and strict core all-target Clippy. Custom regressions cover accepted Up, typed axis
samples outside builtin windows, exclusion of overdue pending candidates,
inclusive declared deadlines and invalid-selection atomicity. The four-control
console offers help, a deterministic synthetic fixture and timestamped stdin
routed through virtual canonical input, binding and Transport. Its baseline CLI
checks covered ten stdin error/boundary/EOF cases and invalid UTF-8 diagnostics.
These earlier checks do not verify subsequent implementation changes.

Phases 8–10 now have [integrated runtime](../kernel/REQ__runtime.md),
[visual projection](../kernel/REQ__visual.md) and [logical replay](../kernel/REQ__replay.md)
source and examples. Live recording consumes admitted RuntimeReport operations
without a second judge. [Section restart](../kernel/REQ__section-restart.md)
prepares a frame-selected PCM suffix and fresh output owners; native buffer reset
and observed first-presentation clock mapping remain host responsibilities.
Generic Phase 11 interaction source and Phase 12/13 native Linux/macOS backend
source are implemented with build checks. Current tests, independent reviews,
native audio execution and physical timing evidence remain deferred. Phase 14
now includes the [bounded BMS adapter](../kernel/REQ__bms-adapter.md) and
[offline final composition sample](../kernel/REQ__bms-sample.md); the adapter
depends only on core, while the executable composes core/platform/adapter.
[Phase 15's conditional SDK](../kernel/REQ__sdk-status.md) is not activated without
a concrete C/C# host requirement. The portable runtime benchmark exists but
has not been executed. ASIO licensing/implementation, deferred verification and
physical restart synchronization evidence remain outstanding. The full runtime
Goal is not complete.

Logical visual source now includes Polar and validated caller-defined projection,
and reverse replay keysounds expose normal-head, reversed-sample and mute policies
using dedicated output owners. The Windows runtime example connects original-WAV
frame selection to an observed WASAPI position/QPC relation rather than assuming a
wall-clock read marks presentation. See [presentation composition](../platform/REQ__wasapi-presentation.md)
and [reverse playback](../kernel/REQ__reverse-playback.md). Native section timing,
new fixtures and formal review remain unverified.

The portable [device adapter registry](../platform/REQ__device-adapters.md)
owns per-device adapter instances, explicit connected/retired identities and
bounded raw-report fanout. Equal acquisition metadata permits multiple reports
from one native packet without losing source sequence. The separate
[interval jitter collector](../kernel/REQ__telemetry.md) observes caller-supplied
clock points against a declared nominal period; generated benchmark observations
are labeled synthetic. Neither surface establishes native timing measurements.

macOS input now has an explicitly selected raw-report mode using a dynamically
resolved timestamped IOHID callback. It retains original bytes/IDs/mach arrival
ticks and exposes declared report-framing conversion; default scalar value
acquisition remains separate. Apple-target compile checks cover this source,
while native linking, permissions and device reception remain unverified. The
[bounded BMS/compiler corpus](../kernel/REQ__bms-fuzz.md) supplies authored
mutation and metamorphic fixtures; creation does not establish fuzz execution
or replace pending broader fuzzing/QA evidence.

The final BMS sample additionally has [native Windows composition](../kernel/REQ__bms-native.md)
with real keyboard input, actual BMS rules/timing/WAV assets and WASAPI. Its
explicit startup preroll shifts automatic BGM and the output/song origin together;
compiled targets and input offset stay separate. The shared
[preparation library](../kernel/REQ__bms-preparation.md) has a WAV default and an
injected off-thread decoder boundary. No additional codec support is claimed by
that boundary. Portable workspace and Windows-target compile checks cover the
source; actual playback and long-run device-clock stability remain unverified.

Native BMS composition now connects an ongoing
[presentation observer](../platform/REQ__presentation-discipline.md) to the existing
Transport. It estimates output/host drift over retained observations and spreads
bounded phase corrections through continuous positive rate changes, preserving
past transport segments and judge chronology. Repeated/stalled device positions
do not extend freshness. Stale progress, clock resets and excessive disagreements
require explicit failure/resynchronization rather than silently changing origin.
The observer's storage is bounded; retained Transport history can grow off-thread.
The Windows section-restart example creates a fresh observer for each applied cue.
Neither source establishes a hardware accuracy bound or replaces deferred native
execution, tests and independent review.

Linux ALSA source additionally exposes [coherent native timing observations](../platform/REQ__alsa-timing.md)
separately from its independent aggregate counters. It requests monotonic native
timestamps explicitly and retains raw status/delay and query intervals. Estimated
sound-frame position requires valid running status and delay; terminal/unavailable
queries invalidate publication. Native execution and acoustic accuracy remain
unverified. The separate [Linux-native BMS composition](../kernel/REQ__bms-linux-native.md)
connects actual evdev input and ALSA output to the same shared preparation and
Runtime/JudgeEngine. Native sound-frame/host observations remain explicitly
estimated, feed continuous bounded transport correction, and do not fabricate
Windows clock metadata. Input loss stops rather than guessing lost events; native
execution and synchronization accuracy remain unverified.

ALSA additionally exposes the [last successful mixer report](../platform/REQ__alsa-render-telemetry.md)
with core cumulative late/execution rejection counters. Linux native diagnostics
print it after stop/join, including error cleanup. A rendered block remains
different from native submission and audible output; retaining this report does
not extend native clock freshness or establish physical playback evidence.

CoreAudio exposes the same [successful mixer report](../platform/REQ__coreaudio-render-telemetry.md)
contract through callback-owned publication and a final retained report after
callback quiescence. Native example cleanup prints these core execution counters.
This adds no native playback evidence and does not yet provide a macOS BMS sample.

Git was initialized locally on 2026-09-29 after these files had been implemented. Initial commits record the existing implementation and its documentation, rather than reconstructing historical development commits.

Phase 7 audio is under implementation: [core audio](../kernel/REQ__audio.md)
provides finite PCM preload, bounded scalar SPSC commands and deterministic
scheduled mixing. [Windows audio](../platform/REQ__windows-audio.md) provides
native WASAPI enumeration/probing, shared engine/legacy event or explicit timer
operation, exclusive event operation, requested/applied configuration and
worker-owned clock telemetry. On Rust 1.98.1, coordinator checks pass 178 tests
including doctests in each of debug and release on Linux, and strict Clippy
passes on Linux and for the Windows platform target. Windows cross-compilation
passes. These checks are not independent audio review/QA or successful native
playback evidence. Successful native shared/exclusive execution remains
required; the retained Windows VM previously reported zero render endpoints.
ASIO licensing remains unresolved, and full Phases 7–15 remain required.

## Verification

The executable verification configuration is [the Harness manifest](../harness/manifest.yaml). It runs formatting, strict Clippy, workspace tests in debug and release, transport, binding, chart, input-inspector and judge help/fixture examples, and public API documentation. Timestamped judge stdin also receives task-specific CLI QA. CI declares Linux, Windows and macOS checks using Rust 1.98.1; local verification alone does not establish execution on all CI hosts or physical hardware.

Contributors need a working Rust 1.98.1 or newer toolchain with rustfmt, Clippy and a host linker. The [toolchain guide](../build/GUIDE__rust-toolchain.md) records the pinned compiler policy. Task-local isolated toolchain paths are environment-specific and are not a project installation contract.
