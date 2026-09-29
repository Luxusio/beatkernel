---
summary: BeatKernel is a low-latency, cross-platform Rust runtime for rhythm games.
---

# BeatKernel

BeatKernel provides reusable timing, input, audio, and deterministic gameplay runtime primitives. Game-specific rules belong in consumers of the kernel.

The full implementation specification is [plan.md](../../plan.md). [Primary goals](REQ__project__primary-goals.md) record the project constraints. [README](../../README.md) describes the current implementation and commands.

The verified implementation covers phases 0–3: workspace structure, integer time and transport, canonical physical input, virtual device routing, pure platform keyboard normalization, and [device-aware game bindings](../kernel/REQ__binding.md). [Phase 4 Windows input](../kernel/REQ__windows-input.md) is implemented and awaiting independent review/full QA: six native API integration tests and five platform unit tests pass in an isolated Windows Server VM, and the inspector captures source-attributed A Down/Up through native WM_INPUT. This is virtual keyboard input, not physical latency evidence. Subsequent runtime phases and physical benchmarks remain required.
