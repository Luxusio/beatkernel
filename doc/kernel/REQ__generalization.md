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

## Game-owned contact reacquisition fixture

The `contact_rebind` core example supplies two custom sustain policies through
the existing evaluator traits: `Locked` and `AfterRelease { grace }`. This is a
unit-square touch region fixture, separate from the built-in Tracking path.
It adds no backend lifecycle synthesis or new kernel rebind API.

A fresh Down within the profile head window acquires device, physical surface,
logical destination and contact. Move cannot acquire or transfer ownership, and
another Down cannot steal an active contact. A normal early Up fails the locked
policy; the other policy retains the original device/surface/destination while
waiting for a new Down with the same or a different contact ID. Reacquisition is
inclusive at release time plus positive grace. Another device, surface or
logical destination cannot rebind. Wrong-contact Move/Up/Cancel is ignored.
An acquired contact's Cancel or invalid/out-of-region position fails explicitly;
there is no contact to cancel while detached.

Owner Up in the tail window produces a single `Custom(0)` result using the
existing grading policy; releasing earlier may enter the configured grace.
Advancing alone cannot complete a sustain as a hit. Head, detached-grace and tail
deadlines expire strictly after their inclusive limit using wide arithmetic.
Detached grace expiry and missing tail release produce TailTimeout. This fixture
does not establish continuous path coverage between observations.

The custom snapshot schema includes policy/grace, target times, lifecycle,
device/surface/logical owner, acquired contact and detached release time. Existing
JudgeEngine snapshots and ReplaySession restoration retain those actual states.
The generalization test source covers lifecycle boundaries, identity isolation,
cancellation, region validation, snapshot differences and replay/seek parity.
The example and fixtures are compiled only; execution and independent review
remain deferred.
