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

Phases 0–3 are implemented: [time and transport](../kernel/REQ__time-transport.md), [canonical input](../kernel/REQ__canonical-input.md), pure keyboard mapping fixtures for Windows, Linux, and macOS, and [device-aware binding](../kernel/REQ__binding.md). Binding retains owned typed samples, provenance, and ordered logical destinations. [Phase 4 Windows input](../kernel/REQ__windows-input.md) passed independent review and CLI QA. Six native API integration tests and five platform unit tests pass in an isolated Windows Server VM. An earlier inspector build captured device-attributed A Down/Up, provenance and normal/Alt+F4 cleanup through a Hyper-V virtual keyboard; the latest revision passed finite native execution with no acquisitions in a locked guest. Native virtual-device execution does not establish physical hardware latency.

[Phase 5 chart compilation](../kernel/REQ__chart-compiler.md) passed independent review and CLI QA. It provides absolute object start/end times, rational BPM, integer STOPs and separate visual SV markers. Eleven debug/release golden tests cover timing boundaries, deterministic ordering and validation.

[Phase 6 judging](../kernel/REQ__judge.md) implements Instant/Hold evaluators, caller rule registration, configurable inclusive asymmetric windows, replaceable candidate/grading policies and ordered results with retained input provenance. Inputs and advances use explicitly mapped song time with the profile offset applied once. Hold heads acquire device/physical-control/logical-control ownership; only owner release can grade a tail, and early release or timeout produces an explicit miss. Repeat and duplicate Down do not create fresh builtin presses. Explicit start eligibility keeps builtin button/profile limits separate from custom evaluator predicates. Library validation errors preserve state; trusted infallible callbacks have a separate panic/side-effect boundary. Setup and dispatch/results may allocate; judging is single-owner and forward-only.

Twenty targeted judge tests pass in debug and release, and strict core all-target Clippy passes. Two custom-start regressions cover accepted Up and typed axis samples outside builtin windows. The four-control console offers help, a deterministic synthetic fixture and timestamped stdin routed through virtual canonical input, binding and Transport. Targeted console checks cover help, repeated fixture output and ten stdin error/boundary/EOF cases; an example unit test covers line-numbered invalid UTF-8 errors. Full Phases 7–15 remain required: audio, integrated loop, projection, replay/seek/reverse restoration, generalization, native Linux/macOS, parsers, FFI and optimization. Linux/macOS native modules currently remain metadata stubs. The full runtime Goal is not complete.

Git was initialized locally on 2026-09-29 after these files had been implemented. Initial commits record the existing implementation and its documentation, rather than reconstructing historical development commits.

## Verification

The executable verification configuration is [the Harness manifest](../harness/manifest.yaml). It runs formatting, strict Clippy, workspace tests in debug and release, transport, binding, chart, input-inspector and judge help/fixture examples, and public API documentation. Timestamped judge stdin also receives task-specific CLI QA. CI declares Linux, Windows, macOS, and Rust 1.83 checks; local verification alone does not establish execution on all CI hosts or physical hardware.

Contributors need a working Rust 1.83 or newer toolchain with rustfmt, Clippy and a host linker. Task-local isolated toolchain paths are environment-specific and are not a project installation contract.
