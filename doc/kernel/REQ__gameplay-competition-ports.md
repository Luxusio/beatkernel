# Injected gameplay competition observers

Shared solo/cohort policy cannot depend on concrete `LiveCompetition` or
`NativeGroupCompetition` implementations. Business-owned solo and group ports
observe borrowed actual runtime reports or validated ordered member progress,
and receive completion only after existing genuine output completion guards.
Port observation is fallible; completion marking is infallible as in existing
native owners. Neither successful publication, cancellation, input closure nor
diagnostic timeout establishes completion. Observe committed prefixes before
reporting technical failure and preserve original observer error evidence.

Use generic business sessions and per-player state over the competition types,
with explicit no-op implementations for disabled competition. No per-note trait
objects, boxed observer ownership, new crates or allocation solely for injection.
The generic session may contain optional observers; concrete IO selection lives
in the native compatibility bridge, where legacy owners implement the ports.
Keep the original NativeGameplaySession, NativeCohortSession and PlayerState
public names as compatibility aliases to their native specializations. Existing
public entry point and field-literal calls must continue to compile. Explicit
injected entry points also accept generic sessions containing test observers.

Solo observes gauge, capture and competition before UI publication and sound
fencing in the existing order. Local observes every actual member before the
whole group's progress, then UI publication and fencing. No later observer
refusal may erase gauge/capture/score updates, earlier member observations,
queued audio, Stop ownership or original report errors. Numeric failure still
freezes only its actual member. Technical errors retain their existing wrappers.
The group callback receives the actual stable member order and original IDs,
never fabricated aggregate judgments or remote results submitted to local judges.
On genuine cohort completion, notify each populated member observer in state
order and then the shared group observer. Every member uses the same established
shared output-completion barrier; early numeric fencing alone cannot trigger a
member marker. Each owner run returns immediately after notifying completion,
so no marker is repeated within that run. Native local ghost observers have no
per-member network endpoint; their delegated completion remains a no-op there.

Existing native IO implementations retain setup, sockets, native timing,
publication cadence and cleanup behavior outside policy. This increment removes
the common loop's concrete dependency; it does not yet separate all internals of
those IO/comparison implementations or platform presentation-discipline types.

Independent deferred fixtures connect populated fake solo and group observers,
virtual clock/waits and an explicit host to actual portable Runtime/Mixer paths.
Cover ordered observations, member-specific and shared errors preserving complete
committed evidence, full/partial Stop admission, deterministic repeatability and
completion markers withheld on cancellation, closure, timeout or technical error.
Existing assertions remain unchanged. Fixture execution, hardware/network tests,
formal review/QA and comparative performance measurements remain deferred.
