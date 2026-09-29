# BeatKernel primary goals

BeatKernel is a low-latency, cross-platform Rust runtime for rhythm games, distributed as a public library under the [MIT license](../../LICENSE).

## Required behavior and boundaries

- Implement the full ordered specification in [plan.md](../../plan.md), including binding, native input, chart compilation, judging, audio scheduling, rendering primitives, replay, and remaining platform backends.
- Keep exactly two initial crates: `beatkernel` for platform-independent primitives and `beatkernel-platform` for OS integration. Dependencies flow from platform to core. Later parser adapters and conditional FFI follow the specification's separate phases.
- Use integer nanoseconds for runtime timestamps. Preserve input source, native metadata, original clock points, and deterministic ordering through normalization and later mapping.
- Keep game-specific rules and OS branches outside core. Avoid allocation, locks, and I/O in real-time audio paths when implemented.
- Preserve generic input capabilities beyond keyboards: axes, touch, pointers, pose, raw HID, and custom payloads.
- Keep the runtime independent of React and other UI frameworks.

## Current evidence and remaining work

Phases 0–2 are implemented: [time and transport](../kernel/REQ__time-transport.md), [canonical input](../kernel/REQ__canonical-input.md), and pure keyboard mapping fixtures for Windows, Linux, and macOS. Platform modules remain metadata stubs; the input inspector uses virtual devices. Binding and all later runtime phases remain unfinished.

Git was initialized locally on 2026-09-29 after these files had been implemented. Initial commits record the existing implementation and its documentation, rather than reconstructing historical development commits.

## Verification

The executable verification configuration is [the Harness manifest](../harness/manifest.yaml). It runs formatting, strict Clippy, workspace tests in debug and release, both virtual examples, and public API documentation. CI also declares Linux, Windows, macOS, and Rust 1.83 checks; local verification alone does not establish execution on all CI hosts or physical hardware.

In the current workspace, Rust 1.83 is available through `/tmp/beatkernel-cargo/bin`, `RUSTUP_HOME=/tmp/beatkernel-rustup`, and `CARGO_HOME=/tmp/beatkernel-cargo`. Other environments need a working Rust 1.83 or newer toolchain with rustfmt and Clippy.
