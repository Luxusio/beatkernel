# Gameplay presentation dependency injection

The actual common solo and local-cohort gameplay loops must depend only on a
business-owned presentation contract using core clock/transport values. Their
production policy cannot import platform PresentationDiscipline or native
presentation snapshot/error types, instantiate a native estimator, or choose a
backend. Define device acquisition/effect contracts in the business layer with
an associated presentation implementation; static dispatch ties each device to
its injected presentation owner. Use the same production entry point for pure
memory devices and actual native adapters.

The presentation port supplies the latest accepted pair, quality, freshness
validation, continuous transport update, and construction of a fresh estimator
with explicit stream/playback/host/song origins at resume. Preserve existing
resume chronology, current correction configuration, original native reseeding
and input/pause/end evidence. Reconstruction is explicit through the injected
presentation implementation; common loops cannot fabricate native counters.
The pure core estimator implements the port without IO or native references.
The port also exposes a cold optional epoch getter and explicit output rebind
under [output clock epochs](REQ__output-clock-epochs.md). Existing custom owners
default to unsupported refusal; core/native adapters implement the same contract.
This supplies a timing transition interface, not live device/PCM handoff.
Same-stream resume now follows
[resume clock identity](REQ__resume-clock-identity.md): stage a new owner with
the actual configuration and unchanged epoch before original native reseeding.
Seeding must leave accepted evidence before clocks are replaced, following
[resume commit order](REQ__resume-commit-order.md).

Keep original NativeGameplayDevice, NativeGameplaySession, NativeCohortSession
and run_* compatibility APIs at their existing public paths. The outer bridge
owns their concrete platform specialization, native trait and business-trait
adapter. Native platform errors remain the original errors wrapped by existing
result types. Existing native adapters and fixture assertions remain unchanged.
No extra crate/dependency or per-note virtual dispatch is required.

Independent deferred fixtures exercise the port and actual solo/cohort pumps
using only the core estimator, memory device evidence, injected controls and
host/competition ports. Explicitly disable core processing telemetry's native
clock. Check original pair chronology, stale/refused observations and failed
updates, completion versus cancellation, ordered member publication, and
pause/resume construction/reseeding with distinct playback origin. Do not
replace original observation/input timestamps with control time. Cases use
independent expected traces and failure injection; compile-only checks do not
prove assertion success.

This removes the presentation type dependency from common gameplay policy;
concrete competition internals, processing telemetry defaults, other IO
boundaries and full adapter coverage still require work. Test execution,
formal review/QA, hardware and performance verification remain deferred under
the user's standing verification instruction. Full player completion and
SQLite-equivalent reliability are not established by this increment.
