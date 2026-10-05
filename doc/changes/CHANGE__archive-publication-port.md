# Portable archive set publication port

Move archive staging and publication order from the native filesystem module
into shared business policy with opaque destinations and associated errors.
Native path construction and exclusive writes adapt to a statically dispatched
port, and existing native callbacks preserve their original boxed error identity.

## Known ceiling

This separates one storage policy path; complete IO-layer separation remains
unfinished. Real native file/platform acceptance, performance measurements,
cross-file transactions and crash durability are not established. Test execution
and formal review/QA remain deferred.
Allocator refusal is represented but has not been fault-injected or executed.

## Implementation evidence

result_archive_publication defines ResultArchivePublicationPort with comparable
opaque destinations, effect-free destination preparation, exclusive create and
an unconstrained associated error. publish_archive_set validates/encodes the
whole archive before adapter access and stages every unique destination/payload
before writes. Every prepared write is attempted and the first storage error is
returned. No native module, filesystem, path, clock, renderer, boxed error or
dynamic dispatch is imported by this policy module.

NativeArchivePublication supplies original OsString paths and the existing
callback; publish_cohort_sidecars unwraps original destination/storage errors
without replacing their boxed identity. Existing native source callers and
native callback/path fixtures retain their signatures and behavior contracts.

Both writers stopped before scoped formatting and diff whitespace checks.
Five independent deferred fixture groups cover 1–64 rosters, exact linear
payloads, invalid whole archives before adapter access, early/late preparation
errors, duplicate tokens with zero writes, all storage failure subsets and exact
first opaque error retention. All four compile-only checks exited zero:
workspace/all-targets with webtransport; runtime/all-targets without defaults
with webtransport; wasm32 library with browser; and wasm32 library with
browser-audio. Existing dead-code warnings remain. No fixture assertions,
filesystem/platform/browser apps, benchmarks or formal review/QA ran.
