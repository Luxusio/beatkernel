# Retain preparation storage for saved-record competition

Competition prepares saved-opponent updates in retained storage grown during
actual replay admission. Ordinary updates no longer allocate a fresh preparation
vector. Unchanged recorded cursors retain their existing score maps while exact
requested song times still publish on successful whole-owner transactions.
Empty local reports retain local score storage and continue advancing ghosts
through actual recorded operations. Changed prefixes and nonempty local reports
retained their original score staging behavior at this increment.
[The following scalar transaction increment](CHANGE__score-transactions.md)
retains existing grade maps for those forward updates too.

All candidates are prepared before any local/opponent logical state is committed.
A later opponent failure drops staged candidates and leaves the previous scores,
cursors and times intact. Scratch capacity remains available for retries and
reset. Admission grows slots for actual opponents, without preallocating to a
possibly huge configured maximum. Rebuild and loading keep their actual prefix
semantics, including no synthesized misses past a recording frontier.

## Known ceiling

This removes recurring preparation-vector allocation and unchanged-prefix clones.
At this increment, changed ghost prefixes and nonempty local staging could still
clone grade maps; the subsequent scalar transaction increment removes these
forward clones. New grade insertion may still allocate.
Backward prefix rebuild and replay loading are cold reconstruction work. No
throughput, global allocation-freedom or world-leading performance is established.
Saved record UI coverage, broader BMS features and platform acceptance remain work.

Seven independent genuine-owner/reference, atomic failure/retry, frontier and
retained storage fixture groups are authored for deferred execution. They use
actual parsed charts, judge/capture headers and ReplaySession recordings; an
independent clone/fresh-vector reference compares ordered logical updates. Cases
include late admission, backward rebuild/reset, later-opponent score/timing
failure with retry, and actual scratch/unchanged-grade retention. Address/capacity
checks establish storage retention, not a complete allocator audit. Both writers
returned terminal stop reports before scoped Rust formatting. Four sequential
compile-only checks exited zero: workspace/all-targets with WebTransport,
no-default-features WebTransport/all-targets, WASM browser/library and WASM
browser-audio/library. Existing unused-code warnings remain. Host checks compile
the actual owner fixture children; WASM library checks compile the common
owner but not its fixture children. Tests, benchmarks, actual
browser/native acceptance, formal review, required QA, verify and close remain
deferred. The full Goal remains active and unproven.

Cold allocation-fault injection is not implemented or executed in this slice;
ordinary malformed/capacity admission fixtures do not prove allocator failure
behavior. The source retains fallible reservation errors before logical admission.
