# Conditional SDK and optimization status

Phase 15 in plan.md makes a C ABI/host SDK conditional on a confirmed need.
No concrete C or C# embedding host, ownership contract or distribution target
has been requested. The current public embedding surface is the Rust core and
native platform crates with examples. A C ABI is therefore not activated in this
implementation continuation; this is the existing condition, not cancellation
of a requested host integration. Future activation needs explicit handle
ownership, callback/thread and error-buffer contracts and a real host example.

Performance improvements require measurements. Runtime processing percentiles
and native output counters are available; unknown physical timing is retained
as unknown. Do not claim optimization gains from compilation or integer time
representation. Required callback audits, latency/jitter/underrun benchmarks,
independent review and native device measurements remain deferred under the
user's 2026-09-30 verification sequencing instruction.
