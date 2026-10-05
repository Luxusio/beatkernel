# Portable archive set publication port

Whole-roster and member archive staging, duplicate detection, write ordering and
first-error selection belong to shared business policy, not the filesystem
adapter. A statically dispatched port supplies opaque comparable destinations,
pure destination preparation and exclusive-create effects with an associated
error type. The shared module imports no native gameplay/cohort, filesystem,
path, renderer, clock or network implementation. Test destinations may be small
integers and errors may be opaque values; OS paths and boxed native errors are
not business policy requirements.

Policy completely validates/encodes the original archive before destination
preparation, prepares the whole destination and all original-member projections
and payloads, and rejects duplicate destinations before the first write.
Destination preparation is contractually effect-free. Preparation refusal means
zero writes. After preparation succeeds, every destination is attempted in whole
then original-member order, with the first exact associated storage error retained.
Historical values remain distinct from live CompletedPlayResult evidence.

The native adapter composes this shared policy with original OsString path
construction and exclusive file creation/write/flush. Its existing callback API
and solo behavior stay compatible. Native destination/storage errors are unwrapped
to preserve the original boxed error identity; policy errors retain their kind.
The three native local composition roots continue using this single adapter.
Generic dispatch introduces no per-note allocation, virtual calls or locks;
bounded staging allocations occur after play cleanup, with no measured
zero-overhead or universal allocation-free claim.

Independent deferred fixtures inject non-path destinations and opaque errors,
exercise 1–64 original IDs, duplicate and late-preparation refusal with zero
writes, invalid whole archive refusal before adapter access, linear payloads,
all-write attempts and original error retention. Existing native callback/path
fixtures remain unchanged. Assertions and formal review/QA are deferred; after
both writers stop, scoped formatting and the exact four compile-only checks
establish compilation only.

## Known ceiling

Port implementations must honor effect-free preparation and exclusive create.
Cross-file writes remain nontransactional and crash durability is unproven.
Actual native filesystem/platform acceptance, complete remaining IO-layer
separation and performance measurements remain unfinished.
Allocator refusal is represented by a typed error but has not been fault-injected
or executed in this increment.

## Implementation evidence

Both paired writers returned terminal `Writes STOPPED` before scoped formatting
and compilation. Five deferred pure-port groups were authored. Their comparable
destinations implement no Clone, and associated errors implement no Debug,
Display or Error. Existing native callback/path fixtures were left unchanged.
Scoped formatting and diff whitespace checks succeeded; the exact four
compile-only checks all exited zero. No assertions, filesystem/device/browser
applications, generated bindings, benchmarks or formal review/QA ran.
