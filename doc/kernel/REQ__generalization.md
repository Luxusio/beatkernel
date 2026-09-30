# Generic interaction patterns

Phase 11 implements configurable Repeated, Tracking and Composite evaluators
without game-name branches in core. Examples provide knob, dual-contact arc,
roll, fret/trigger chord, pointer and pose fixtures as caller configurations.
All use the same JudgeEngine in live play and replay.

Repeated consumes fresh Down transitions within a positive ranged object,
tracks source/control ownership across Up and Repeat, and completes after a
configured minimum count or reports a timeout. Tracking consumes typed axis,
touch, pointer or pose samples against a finite uniformly parameterized path.
Relative axes/pointers accumulate displacement explicitly. A touch interaction
locks its contact/device/control identity; another contact cannot finish it.
Configured tolerance and maximum sample gap are explicit policies, not a
promise of physical sensor precision. Cancellation or failed coverage misses.

Composite tracks held logical prerequisites and grades a fresh trigger using
the existing timing policy. Evaluators can declare additional logical input
controls, routed by the judge in addition to the primary rule destination.
Composite remains armed before the target so prerequisite transitions are
observed, preserving source identities and optional same-device ownership.

Snapshots capture all these states and canonical bytes. Unsupported custom
evaluators still fail explicitly. Configuration validation runs before engine
construction; callbacks never parse game names or native key codes. Fixtures
and compile checks accompany implementation; formal verification is deferred.
