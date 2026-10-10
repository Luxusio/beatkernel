# Generic interaction patterns

Phase 11 implements configurable Repeated, Tracking and Composite evaluators
without game-name branches in core. Examples provide knob, dual-contact arc,
roll, fret/trigger chord, pointer and pose fixtures as caller configurations.
All use the same JudgeEngine in live play and replay.

## Original six-style combination acceptance

The additive software fixture must exercise both absolute and relative axis
tracking, two concurrent touch contacts, fresh-press repeated input,
same-device held prerequisites with a fresh trigger, pointer trajectory plus a
separately bound button instant hit, and a caller-owned derived pose interaction.
Existing seven-object builders and the default example route remain compatible.
These are representative combinations, not compatibility with named games.

The derived pose policy belongs to the example and uses existing evaluator
traits. It follows a linear position path from [0,0,0] to [1,1,1] with tolerance
0.001 and a maximum sample gap of 150ns. Its finite quaternion must have squared
norm within 0.001 of one and abs(w) >= 0.9, inclusively; q and -q are equivalent.
An invalid pending sample cannot acquire ownership. An invalid active owner's
position/orientation produces one RejectedInput miss. Wrong payload, device,
physical control or logical destination cannot steal or terminate the owner.
Final success requires a qualifying original input; advancing time alone cannot
manufacture a hit. Inclusive timing limits expire strictly after their boundary.

Both snapshot hooks must preserve the complete immutable criterion, lifecycle,
times, logical control, physical owner and mutable sample history. Verification
requires literal per-style outcomes and provenance as well as live/replay parity
and restoration from an active checkpoint.

Independent DEEP code review and CLI QA on `f3e7c8f` passed: full core 498/0,
release focused 38/38, strict core all-target Clippy and 12 debug/release CLI
checks. Evidence is in `target/wf/qa-cli-six-patterns-01a12342-1/`.
`tests/generalization_six_patterns.rs` names each of the six combinations and
checks their literal outcomes, input metadata, independent negative cases and
active checkpoint reconstruction through Runtime and replay. Both axis modes
produce eight hits, including the pointer-device button instant and derived
pose; the default seven-object route and all 19 legacy tests remain valid.
`tests/derived_pose.rs` independently checks the fixed policy's 12 boundary,
ownership, numeric, input-backed completion and snapshot cases. This verifies
the original six-style software combinations, not physical/game compatibility.

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
and compile checks accompany implementation. Source presence alone does not
establish current verification; scoped execution and independent review evidence
must be recorded separately. The earlier blanket verification deferral is lifted.

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
Independent QA executed all 19 legacy tests in debug and release, including the
contact-rebind cases. Physical contact behavior is not established by this
software execution.
