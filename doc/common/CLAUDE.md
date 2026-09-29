---
summary: BeatKernel is a low-latency, cross-platform Rust runtime for rhythm games.
---

# BeatKernel

BeatKernel provides reusable timing, input, audio, and deterministic gameplay runtime primitives. Game-specific rules belong in consumers of the kernel.

The full implementation specification is [plan.md](../../plan.md). [Primary goals](REQ__project__primary-goals.md) record the project constraints. [README](../../README.md) describes the current implementation and commands.

The current implementation covers phases 0–3: workspace structure, integer time and transport, canonical physical input, virtual device routing, pure platform keyboard normalization, and [device-aware game bindings](../kernel/REQ__binding.md). Phase 4 native input and subsequent runtime phases remain required.
