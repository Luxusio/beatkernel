# Performance and deterministic testability

## Product objective

BeatKernel's lasting product objective is to build the world's lightest and
fastest high-performance rhythm-game player core and player. This is a target,
not an achieved ranking. Comparative claims require published workloads,
hardware, configurations, versions, measurements and reproducible evidence.
Correct timing, input provenance and deterministic replay remain requirements
when optimizing latency, CPU, memory, startup and distribution size.

## Layer boundary

The user explicitly requires complete separation of UI Layer, Business Layer
and Native IO Layer with natural dependency injection. These are dependency and
ownership boundaries, not a requirement for three additional crates.

UI owns retained presentation and translates user actions into typed commands.
It consumes read-only business state/results and must not perform judgment,
device policy or direct driver/network/storage operations. Business owns
validation, judgment, state transitions, completion, replay and protocol policy.
Native IO owns device/OS access and physical effects; browser IO adapters have
the equivalent acquisition/effect role. Business defines narrow interfaces and
evidence values it needs; UI and IO adapt to those contracts. Business cannot
import concrete UI or IO implementations to choose or instantiate them.

The application composition root supplies concrete ports at construction or
explicit operation boundaries. Tests inject deterministic implementations
through those same entry points. Avoid hidden globals, thread-local service
lookup or platform branches as substitutes for injection. Legacy player
thread-local control/publication access belongs only in the outer compatibility
bridge; fully injected gameplay entry points receive
explicit command/publication ports. Remaining concrete adapters still require
audit against this rule. Backend conversion stays in IO while backend-independent
presentation/time algorithms belong in the domain.
UI lifecycles and back-stack decisions remain testable independently of graphics.
IO lifecycle/cleanup failures cannot fabricate business completion evidence.

The kernel and shared game policy must be testable without an operating-system
device, real clock, sleep, filesystem, network, renderer or audio driver.
Platform adapters acquire original evidence and perform effects. Business code
validates evidence and decides transitions through explicit ports. A port must
permit deterministic values, failures and admission limits in tests; moving an
OS call behind a module name alone does not satisfy this rule.

Clock domains and physical input/output provenance cannot be substituted with
test/control time. Native and browser adapters use common business rules.
Use static dispatch or direct value inputs where sufficient; do not add heap
allocation, per-note virtual dispatch or new crates solely to enforce layering.
Real-time audio callbacks must not gain allocation, blocking I/O or locks.
Bound queues and work, and make overflow, refusal and cancellation explicit.

## Reliability target

The user requests SQLite-grade test stability. Adopt the relevant methods from
the [SQLite testing documentation](https://www.sqlite.org/testing.html):
independent harnesses, fault injection, boundary and generated cases, coverage,
regression cases and recovery testing. This is a testing strategy and long-term
quality target, not a claim of equivalent existing coverage or reliability.

Each layer needs an independent reference or invariant rather than assertions
that repeat its implementation. Pure domain tests cover time arithmetic,
ordering, judgment, gauge, replay, scope and lifecycle transitions. Port tests
inject clock faults, queue admission/refusal, partial progress, cancellation,
storage failures and reconnects. Adapter contract tests check original evidence
and error propagation; real-platform tests separately cover drivers, graphics,
browser workers and network interoperability. Generated failures retain their
seed and inputs as reproducible regressions. Crash/recovery tests must verify
record integrity once persistence paths are connected.

Compilation is not executed test evidence. Test count alone is not a quality
metric. Track covered transitions, branches, failure sites, invariant coverage,
repeatability and platform/configuration gaps. Do not mark an unexecuted suite
or an unmeasured performance target as passed.

## Performance evidence

Measure input-to-judgment and presentation latency distributions (including tail
and maximum), audio underruns, frame deadlines, CPU, memory and allocations.
Use sparse and dense charts, long sessions, restart/pause, solo/local/online and
native/browser configurations. Record backend, buffer, display and workload
parameters. Set numerical budgets from recorded baselines and product latency
requirements; arbitrary numbers or average FPS do not establish superiority.

## Current implementation boundary

Separation is unfinished. Core runtime processing telemetry currently defaults
to a native `Instant` clock; deterministic tests must select its existing
`RuntimeProcessingClock::Disabled` or scripted `External` mode. Telemetry timing
is distinct from judgment timing. Native pump wall-clock deadlines and waits use
an explicit control port. The fully injected solo/cohort entry points also
receive a business-owned host for commands, publication and typed diagnostics;
the outer compatibility bridge selects legacy player/system adapters. Shared
pumps now receive a business-owned device contract with an associated injected
presentation implementation. The actual
bounded observation ring, freshness, drift/phase calculation and continuous
transport correction now belong to the pure core presentation estimator; the
platform wrapper validates native source metadata and delegates calculation.
Gameplay sessions and resume reconstruction use the presentation port; native
trait specialization and platform port implementation live in the compatibility
bridge. See [the presentation port](REQ__gameplay-presentation-port.md) and
[the estimator boundary](REQ__pure-presentation-estimator.md).
Their generic sessions now accept business-owned solo/group competition observers; concrete
native specializations live in the compatibility bridge. The native competition
implementations still own network, preparation/storage and terminal
diagnostics. Their outer solo/cohort setup waiting now delegates to a shared
business start gate with explicit readiness/polling and opaque control-clock
ports; network release time stays separate. Group network publication cadence
and underlying room-adapter waiting still use native time. See
[the start gate](REQ__competition-start-gate.md). Competition display payloads,
projection and 50ms cadence
now live in a pure policy using an injected presentation host; native UI and
display-clock effects live in its bridge. Actual native publication delegates
to that policy, with explicit host-injected observation/publication entry points.
This does not make the remaining native network owners pure. See
[the display boundary](REQ__competition-presentation-port.md).
Those adapters, file/network boundaries and adapter coverage require continued
audit and separation. See [the competition ports](REQ__gameplay-competition-ports.md)
and [the host boundary](REQ__native-gameplay-host.md) for
its exact scope and remaining effects. Native completion now returns typed
solo/per-member results through injected hosts and retains an atomic registered
roster table in player snapshots. Result rendering, browser delivery and archive
integration remain unfinished. Test execution, hardware QA and comparative benchmarks remain deferred
under the user's existing verification instruction; no quality target is
considered achieved by authoring fixtures or passing compile-only checks.
