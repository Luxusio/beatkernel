# Full-plan source coverage and outstanding acceptance

This record applies to the complete [plan](../../plan.md), not a reduced MVP.
The [phase inventory](REQ__implementation-status.md) supplies phase 0–15 source
locations. The 2026-10-01 source inspection additionally checked plan §§1–17,
§§19–24, current manifests, fixtures and public examples. It is an implementation
coverage record, not independent review, executed verification or a release verdict.

Two concrete authored-deliverable gaps were addressed in this continuation:
isolated QPC/mach conversion boundary fixtures for the new real-time clock API,
and an actual `JudgeEngine::with_policies` public custom-policy example.
The existing generalization example/fixtures already represent more than four
distinct game patterns. The larger §13 matrix describes supported primitive
combinations; it does not activate a requirement to ship every listed game.
Phase 15 C/C# SDK remains conditional on confirmed host need.

## Plan §24 completion conditions

Paths in this table are relative to the repository root. “Authored” establishes
source presence only. Behavior requiring execution remains unproven at the
current revision, even where historical earlier-revision results exist.

| Required end state | Current source evidence | Remaining proof |
| --- | --- | --- |
| Core has no direct native OS dependency | `crates/beatkernel/Cargo.toml` has empty dependencies; `src/lib.rs` exports logical modules | Current dependency/build inspection is static evidence; target builds and architecture review still need final verification |
| Platform and game adapter meet only in final app | Platform depends on core; `adapters/beatkernel-bms/Cargo.toml` depends only on core; `app/Cargo.toml` composes them | Current manifests establish declared direction; final adapter-only suite and composition execution pending |
| Windows plus another platform native input/audio operates | Windows Raw Input/WASAPI/optional ASIO, Linux evdev/hidraw/ALSA, macOS IOHID/CoreAudio source and native BMS hosts | Actual current Windows and Linux/macOS acquisition/output; SDK-enabled ASIO C++/MSVC build and device execution pending |
| Standard keys converge to canonical HID identity | `keyboard/` maps; `tests/key_mapping.rs` enumerates convergence and documented aliases | Current fixtures and physical native comparisons unexecuted |
| Separate device binding for identical controls | Core `input/binding.rs`; `tests/binding.rs`, `tests/runtime.rs` | Execute exact/any-device and two-source end-to-end assertions |
| Raw reports become canonical device-adapter events | Platform `input/hid_report.rs`, adapter registry; `tests/hid_reports.rs`, `tests/device_adapters.rs` | Execute malformed/valid adapter and native report paths |
| Instant/Hold/Tracking/Repeated/Composite validated | `interaction/`, `tests/judge.rs`, `tests/generalization.rs` | Authored configurations exist; execute their actual outcomes and ownership boundaries |
| Button/Axis/Touch/Pointer/Pose reach shared runtime | Typed `input/`; runtime/generalization examples and fixtures | Execute typed payload, binding, lifecycle and forwarding checks |
| Transport rate/pause/seek/reverse passes tests | `transport/`; `tests/time_transport.rs` includes seeded anchor oracle | Execute current boundary/property suite; source is not a pass |
| Judge and visual projection remain separate | Independent `judge/` and `visual/`; `examples/runtime_visual.rs` consumes actual Runtime reports | Current projection fixtures and external renderer execution pending |
| RT prohibitions established by audit/benchmark | Mixer/queue and native serialized rendering; `tests/audio_rt_alloc.rs`; direct render-clock diagnostics avoid allocated errors | Current allocation/deallocation/error-path execution, lock/IO audit and native scheduling performance still required |
| Repeated replay produces same judgment | Same JudgeEngine transitions; replay codecs and `tests/replay.rs` | Current repeated result/ordering comparisons and cross-platform limits need execution |
| Snapshot seek hash matches reference playback | Replay snapshots, runtime restart; replay/section_restart/reverse fixtures | Execute arbitrary seek/reference hashes, boundary restores and reverse policies |
| At least four game fixtures without core game/OS branches | `examples/generalization.rs`: axis, dual-contact, repeated, composite, pointer/pose; separate BMS adapter | Execute fixtures and inspect dependency/core boundaries in final review |
| Timing/jitter/underrun telemetry and benchmark binary | `examples/runtime_bench.rs`, native input cadence tools and shared direct render capture; native counters | Benchmark executable source exists; p50/p95/p99/max under actual workloads, 1 kHz inputs, buffer-size xruns/jitter and physical latency remain unmeasured |
| Public examples and crate documentation ready | Core/platform `lib.rs`, README and examples; custom judging extension usage added | Compile evidence exists; current doctest/doc build and example execution still pending |

## Plan §17 authored verification coverage

| Required checks | Authored locations | Evidence boundary |
| --- | --- | --- |
| Time arithmetic/overflow and transport controls | `tests/time_transport.rs`, platform clock-source fixtures | Pure fixtures and source compilation; current execution pending |
| Native key equivalence, source identity and binding selectors | Platform `tests/key_mapping.rs`, core `tests/input.rs`, `tests/binding.rs` | Exhaustive alias/convergence authoring does not establish physical acquisition |
| Judge boundaries, contact lifecycle/rebind, axis normalization | Core `tests/judge.rs`, `tests/generalization.rs`, platform adapter tests; `examples/contact_rebind.rs` | Custom rebind is game-owned; built-in Tracking does not silently transfer contact |
| Chart compilation and snapshot event restoration | Core `tests/chart.rs`, `tests/replay.rs`, `tests/section_restart.rs` | Current golden/hash behavior requires execution |
| Random anchor mapping, serialization roundtrip, key table aliases | Time seeded oracle, `tests/input_codec.rs`, platform exhaustive key mapping | Finite deterministic property corpus authored, not executed at current revision |
| Parser/compiler fuzzing and repeated replay sequence/hash | `adapters/beatkernel-bms/tests/fuzz_corpus.rs` includes text and structured core charts; replay/restart fixtures | Finite seeded corpus is not an unbounded fuzz campaign or executed determinism result |
| Virtual input→binding→judge→audio, two keyboards | `tests/runtime.rs`, runtime/judge examples | Actual pipeline source and assertions exist; execute to prove PCM/result behavior |
| Raw HID→adapter; axis, dual-contact tracking | Platform report/adapter tests and core generalization fixtures | Meaningful fixture assertions authored; current execution pending |
| Seek/reverse consistency and command order/sample offset | Replay/reverse/section restart and `tests/audio_mixer.rs` | Execute reference state, order and PCM comparisons |
| Native timestamp/cadence, high-rate loss/order | Linux/macOS input cadence examples; Windows inspector cadence mode | Tools preserve backend-specific provenance; no 1 kHz source measurements collected |
| Buffer-size underrun and scheduling jitter | Native configured output examples/hosts, shared `audio/cadence.rs` | Actual native counters and direct pre-Mixer capture source exist; no device measurements executed |
| Loopback input-to-audio latency, if equipment available | No configured physical loopback apparatus or result | Conditional hardware evidence absent; software timestamps cannot substitute |

Plan §16 performance figures are development targets, not guarantees. In
particular, zero judge hot-path allocations are not established by current
source compilation. Runtime setup and result handling may allocate under their
documented boundaries. Input cadence and render cadence report different native
clock locations; neither proves physical latency or exact missing-event counts.

The user's verification deferral remains authoritative: do not run fixtures,
examples, native builds/devices, benchmarks, formal review or QA solely because
this table identifies missing evidence. Compile/format checks are allowed.
Retain full acceptance and revisit actual verification when that sequencing
instruction changes. No full completion, PASS or task close is established here.
