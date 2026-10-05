# Separate saved-opponent preparation from file IO

Shared saved-opponent loading accepts an opaque generic loader, preflights the
whole requested count, and validates each supplied ReplayFile through the actual
competition reconstruction path. Native path opening and display conversion
stay in the adapter. Original loader errors and already accepted prefixes remain
explicit; no atomic whole-batch or synthesized completion claim is added.

The native outer method retains its public signature and passes borrowed Path
keys through the adapter; its whole-capacity preflight also precedes cold request
allocation. Shared policy receives owned decoded files and labels without
formatting resource keys or adding file/network/clock dependencies. The actual
Competition admission path still checks canonical files and current source
identity. Capacity is the remaining configured value, including existing
opponents, rather than an inferred CLI count.

Actual filesystem/platform behavior, assertions, performance and formal
review/QA remain deferred. Endpoint/credential acquisition and remaining wait
adapters are unfinished; the complete player and complete IO separation are not
claimed. This boundary does not establish allocator fault injection or whole
networked owner construction through pure factories.

Five independently authored deferred groups use actual zero-window ReplaySession
hit recordings and an unrecorded later note. They cover complete capacity/empty
preflight, original opaque key pointers and all codec limits, Own/Other order and
adapter labels, every loader refusal position with original error identity,
seven malformed later-file variants, and active comparison loading across
capture domains at twenty-hour/week extents without synthetic tail misses.
Earlier accepted opponents and actual local score/time are checked separately.
Allocator failures and native reader behavior are not exercised by these groups.

## Compile-only evidence

Both paired writers returned terminal Writes STOPPED before scoped formatting
of the six changed Rust paths and whitespace checking. All four compile-only
configurations exited zero: workspace/all-targets with webtransport, runtime
all-targets with defaults disabled and webtransport, wasm32 library with browser,
and wasm32 library with browser-audio. Existing dead-code warnings remain.

Five fixture groups were compiled, not executed. No assertions, applications,
file/socket sessions, browser/generated WASM, benchmarks, formal review/security,
QA, task verification or task close ran. The full player goal remains active;
required review-before-QA and browser/CLI/desktop close gates remain open.
