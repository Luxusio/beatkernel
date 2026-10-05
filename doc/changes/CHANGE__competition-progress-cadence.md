# Inject competition progress publication time

Separate group progress publication time from native clock acquisition and UI
presentation. Shared cadence policy uses an injected monotonic clock and exact
integer time; only successful publication with a valid completion sample
commits the publication interval. Local judge progress remains committed when
optional comparison fails. Solo song-time and room-controller cadence remain
their original independent policies.

NativeGroupCompetition now supplies observe_with_ports with a generic progress
clock alongside the existing presentation host. Its default wrapper uses a
copied native clock adapter with the same per-owner origin. The shared policy
tracks every valid observed sample separately from the committed publication
completion time, so a clock regression during a suppressed interval is refused.
Clock and publication errors retain their unconstrained associated error values.
The policy requires no allocation, lock, dynamic dispatch or native clock access.

Seven independently authored deferred groups cover all 16 gates, exact interval
boundaries, week/above-2^53/u64::MAX values, slow-effect completion anchoring,
publication refusal, pre/post clock failure and pre/post regression including
suppressed observation history. They assert original borrowed member data and
opaque error identity without requiring Error/Display/Debug/Clone bounds.

Both paired writers returned terminal Writes STOPPED before scoped formatting
and compilation. Assertions and runtime/platform/performance acceptance remain
deferred. Endpoint lifecycle, start/cleanup ownership and some prefix allocations
remain native outer work. Group endpoint integration and real clock acquisition
are not exercised by these pure fixtures. No full player completion or formal
review/QA acceptance is claimed.

## Compile-only evidence

Scoped formatting covered only the five changed Rust paths, followed by
whitespace checking. All four compile-only commands exited zero: workspace
all-targets with webtransport, runtime all-targets with defaults disabled and
webtransport, wasm32 library with browser, and wasm32 library with browser-audio.
Existing dead-code warnings remain. No assertions, native/browser applications,
generated WASM execution, network sessions, benchmark, formal review/security,
QA, task verification or task close ran. The full player goal remains active.
