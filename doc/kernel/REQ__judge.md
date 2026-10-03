# Judge and interaction contract

The core judge consumes compiled objects, caller-defined interaction rules and
unchanged bound input. It produces ordered structured results. Instant and Hold
are the initial evaluators; candidate selection, grading and interaction
evaluation expose Rust extension seams. There are no game-name branches.

## Clock, grading and expiry

The caller supplies a mapped **song timestamp** alongside each `GameInputEvent`.
It first normalizes native input into its host domain, then uses `Transport` to
map host to song time. The judge never derives song time from physical metadata
or overwrites its provenance. Both input and `advance_to` take unoffset mapped
song time and apply `input_offset` exactly once. Positive offset moves input
later. Checked overflow or backward effective time rejects the operation before
any library-owned state, chronology or output mutation. Equal times and negative
preroll are valid. Seek/reverse judging requires later snapshot restoration.

A profile contains nonnegative asymmetric early/late windows, ordered from
narrow to wide and nested on both sides, with unique grade IDs. First matching
window wins; endpoints are inclusive. `JudgeEngine::profile()` exposes an
immutable view of the actual configured profile so host-owned replay metadata
can retain its windows and offset. At target 500 ms with early 20 ms and late
30 ms, 480 ms and 530 ms both hit, while 479,999,999 ns is too early and
530,000,001 ns is too late. Misses expire only **strictly after** their deadline.
`advance_to(530 ms)` followed by an input at 530 ms therefore still allows that
boundary hit. A zero-width window accepts exactly the target time.

The default resolver selects the eligible matching-control pending object with
the smallest absolute timing error, then earlier target, then ObjectId. An
earliest-target implementation and caller replacement are available. Invalid
resolver choices are rejected before mutation. One press selects at most one
start per logical destination; binding fanout can independently hit several
destinations at the same source time and sequence.

## Button ownership and Hold

Held state is keyed by `(DeviceId, PhysicalControlId, GameControlId)`. Repeat and
duplicate Down do not become new presses. Unmatched Up is harmless. A different
device or physical key cannot release the owner of an active Hold.

```text
Instant: Pending -> Hit or Miss -> Completed
Hold:    Pending -> graded head + acquire owner -> Active
           |                                      |
           +-> head timeout -> Completed           +-> tail hit/miss -> Completed
```

Instant objects have no end; Hold objects require an end strictly after start.
Unknown rules, duplicate rule IDs and malformed ranges are configuration errors,
not gameplay misses. A graded Hold head acquires the pressing owner. Owner Up
before the widest tail early boundary breaks it; release inside the inclusive
tail windows grades the tail. Late release or continued holding strictly beyond
the tail late boundary yields a tail miss. There is no regrab or automatic
perfect tail. Head and tail results are separate; scoring consumers combine
them according to their own rules. A terminal stage never emits twice.
If a replacement grading policy rejects an in-window owner release, the tail
ends with `RejectedInput`; a rejected head remains pending until its deadline.

Input-caused results retain original source/timing/native provenance and report
effective song time, ObjectId, stage, grade or miss reason, and signed delta
where meaningful. Timeout results fabricate no input provenance. Simultaneous
timeouts order by deadline, then ObjectId and stage; input results follow caller
input order. Active holds and new-press candidates have separate routing.

## Extension and execution boundary

Public evaluator/active-interaction seams support begin, input, advance and state
inspection. Builtin button evaluators ignore nonbutton input without flattening
its typed payload. A custom evaluator may consume the retained sample. Grading
and candidate policy can be replaced by callers.
`StartEligibility` explicitly separates `ProfileButtonPress` from
`EvaluatorDefined` start routing. Instant/Hold use the former: only fresh Down
inside the widest profile window is eligible. Custom evaluators default to the
latter: their predicate accepts typed samples, including Up or samples outside
the builtin window, through a separate pending-object index. Candidate selection
still chooses at most one accepted pending object per bound input. Completed
objects leave that index. Pending objects whose declared inclusive deadline is
strictly before effective input time are excluded before resolver validation;
expiry cannot consume the selected object and discard a live candidate's input.
Equality remains eligible and invalid selections remain atomic. Pending
interactions advance when their declared
deadline expires; continuously advancing interactions begin Active. A pending
interaction declaring no deadline receives no time callbacks.

Library-owned fallible validation completes before state updates. Evaluator
transitions are infallible; callback panics and external side effects are outside
the library's atomic-error guarantee. The judge uses setup indexes, temporal
slices and deadline processing rather than rescan all completed chart objects
on every input/frame. Setup and results can allocate: this is a single-owner
gameplay-thread API, not an RT audio callback, and has no measured latency claim.

The console example offers `--help`, a labeled synthetic `--fixture`, and a
timestamped `--stdin` stream routed through virtual canonical input, four-control
bindings and explicit Transport mapping. Each nonblank line is
`host_ns lane down|up|repeat`, with lanes 1..4 and nondecreasing signed i64 host
nanoseconds. Host origin 1,000,000,000 ns maps to song zero; earlier timestamps,
malformed fields, unknown lanes and states fail with a line number and nonzero
exit. Equal timestamps are accepted. Blank lines are ignored. EOF advances to
at least song 1,600,000,001 ns, beyond the fixed chart's widest deadlines, so
unplayed stages produce explicit misses. The fixed chart has Instant on lane 1
at 500 ms, Hold on lane 2 from 500 to 1,500 ms, and Instant on lanes 3 and 4 at
1,000 and 1,500 ms. Grade 1 uses inclusive +/-20 ms and grade 2 +/-100 ms;
offset is zero. Results print song time, stage/outcome and original host/source/
sequence provenance; timeout results say `input=none`.
It is not native keyboard acquisition. File
parsers, native/audio integration, tracking and snapshot-based seek/reverse now
have separate source implementations listed in the
[phase inventory](REQ__implementation-status.md). This judge example does not
establish their current acceptance.

## Public policy composition example

`examples/custom_judge.rs` uses `JudgeEngine::with_policies` with actual virtual
canonical keyboard input, exact-device binding and Transport conversion. At song
510 ms, eligible targets at 500/520 ms illustrate default closest-target tie
selection versus custom later-target priority. Its custom grading keeps profile
window eligibility and labels early input 70, on-time/late 71. Actual returned
JudgeEvents and subsequent unselected-target expiry are printed by both engines;
the example does not manufacture output or add game-specific core branches.
These stateless custom policies retain unsupported default snapshot hooks, so
the example does not claim checkpoint/replay support. Existing custom resolver
and grading fixtures remain the behavior tests; execution of both fixtures and
this example stays deferred.


## Opt-in button/contact press interactions

Offer explicit press instant/hold evaluators for genuine button and touch events.
Existing button-only evaluators keep their behavior, eligibility tags and
canonical snapshot bytes. A touch owner includes source, physical surface,
logical destination and full contact identity. Only the owning contact may
release a hold. Move does not create a fresh press; repeated Down cannot consume
another note until the matching Up or Cancel. Cancel terminates an active hold
as RejectedInput and cannot become a graded release.

Fresh button/contact presses use the profile-window start index, not a scan of
all pending objects. Touch held ownership belongs in deterministic snapshots,
clones and restoration. New press interactions and contact-enabled judge states
have explicit versioned identities; legacy-only engine bytes remain unchanged.
Contact state serialization is deterministic regardless of hash-map insertion
order. Shared live/replay processing retains unchanged physical metadata.

These evaluators are opt-in primitives. BMS rule selection, versioned setup
metadata, browser contact acquisition and application controls still require
integration before playable touch support is complete.

Mixed charts retain eligibility isolation: fresh touch input enters only
contact-enabled profile-press candidates, while fresh buttons may enter both
button-only and contact-enabled candidates. Evaluator-defined predicates retain
their existing routing contract. This does not turn all pending interactions
into a custom scan.
