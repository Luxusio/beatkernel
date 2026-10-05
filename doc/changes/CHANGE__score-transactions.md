# Prepare score transactions as scalar plans

Local and changed saved-opponent scores now prepare private scalar counter and
timing plans over borrowed judgment batches. After the whole transaction
validates, commit to the original grade maps rather than cloning them. Ordinary
forward observations retain existing grade storage for nonempty local reports
and advancing opponent prefixes. All public score/competition APIs remain intact.

The common preflight preserves timing-before-score errors within each batch and
original batch order across ghost prefixes. Exact cumulative grade checks avoid
rejecting unrelated saturated grades. Plans live inside the exclusive owner
transaction and expose no public deferred-commit API. Cold-sized update storage
is reused; a later refusal clears staging without publishing any owner changes.
Backward rebuild and late admission keep their original recorded-prefix meaning.

## Known ceiling

New grade insertion, replay loading and successful cold reset/backward rebuild
can allocate. Near-overflow grade validation can rescan earlier borrowed batches.
Retaining node addresses is not a global allocator audit. No fault-injected OOM
guarantee, measured throughput or world-leading performance is established.
Broader BMS features, UI integration and platform acceptance remain unfinished.

Seven independent actual-owner/reference, cumulative-limit, batch-error-order and
forward-storage fixture groups are authored for later execution. Genuine two-hit
ReplaySession recordings exercise advancing owners; numeric faults are explicitly
injected into admitted state for boundary and refusal cases. Existing score
fixture assertions remain intact; the retained-update fixture changes only its
scratch pointer type erasure to match the private plan representation. Both
writers returned terminal stop reports before scoped Rust formatting.
Four sequential compile-only checks exited zero: workspace/all-targets with
WebTransport, no-default-features WebTransport/all-targets, WASM browser/library
and WASM browser-audio/library. Existing unused-code warnings remain. Host checks
compile the test children; WASM library checks compile the common implementation
without its test children. These checks do not execute assertions.
Tests, fault injection, allocator profiling, benchmarks,
actual browser/native acceptance, formal review, required QA, verify and close
remain deferred. The full BMS player Goal stays active and unproven.
