# Completed result archive

## Complete individual native publication

Native saving must encode before effects, then create an exclusively owned
sibling staging file, write all bytes, flush and synchronize it, close the
handle, and exclusively hard-link it to the requested final path. A write,
flush, sync or publication refusal must never expose a partial final file or
modify an existing regular file/symlink. No direct-final or clobbering fallback
is permitted when links are unsupported.

Generated staging names must use uppercase ASCII hexadecimal 8.3 components
and a checked 44-bit process-separated identity. Exclusive creation handles
collisions with a 32-attempt cap. Before creation, staging candidates must be
excluded when their names could alias the final name through ASCII case,
leading spaces, trailing spaces/periods or Windows stream syntax. Conservative
candidate exclusion applies on every target; it does not rewrite the caller's
final filename. This avoids relying on case-sensitive filesystems or generated
Windows short-name aliases. Native Windows execution remains separate evidence.

The exact generated staging namespace (eight ASCII hexadecimal digits, a dot,
three hexadecimal digits) and its conservative native aliases are reserved.
Requests to publish a final basename in that namespace must return
`io::ErrorKind::InvalidInput` before filesystem effects. This keeps every
accepted final name disjoint from all concurrent publishers' stages, including
after refused cleanup. Normal `.bkr` and `.bkresult` names and other native
basenames remain accepted. This internal filename reservation also applies to
the native replay and generic result-store adapters; existing reserved-name
files remain untouched.

Cleanup owns only a successfully created stage and closes its handle first.
It preserves the exact primary IO error. Cleanup after successful publication
is best effort and cannot turn a committed complete file into a refusal; the
final path is never deleted. Refused cleanup or process termination can leave
an orphan stage. File synchronization does not establish directory-entry
power-loss durability or hostile-directory containment. Archive sets retain
their independent prepare-all/attempt-all/first-error semantics.

This implemented guarantee supersedes the earlier partial-new-file limitation.
Independently executed real-filesystem, public replay and archive-consumer
regressions are recorded in
[the storage change note](../changes/2026-10-10-native-record-publication-integrity.md).
Those software checks do not establish hostile-directory containment,
directory-entry power-loss durability or actual Windows/macOS execution.

A portable versioned archive preserves actual completed live results for the
entire original 1..64 player roster, each player's existing replay setup/header
identity, actual gauge policy, final gauge/outcome and full/practice extent.
Canonical integer serialization retains original IDs and long signed timestamps;
the policy must preserve grade overrides and custom clear/failure thresholds
rather than assuming a default gauge. Decoded records are historical data and
never construct a live CompletedPlayResult or certify trusted replay/remote results.

Version 1 starts with `BKRESULT`, a little-endian u32 version and u32 roster
count. Each row stores its u32 player ID, a u32-sized canonical header-only
replay envelope, u64 initial/clear gauge units, i64 hit/miss deltas, a boolean
failure flag, and a bounded ascending table of u32 grade/i64 delta overrides.
The remaining row stores the scope tag and optional signed nanosecond start/end,
u64 final gauge level, failure tag and outcome tag. Full-song scope has no
start/end payload. Embedded replay setup must have the same original extent.
The envelope must contain no records or calibration metadata. Maximum sizes
are 64 players, 65,536 bytes per envelope and 5 MiB for the entire archive.

The format validates the whole table, unique nonzero player IDs, common extent,
supported version/tags, policy configuration and result consistency before
acceptance. Truncation, trailing data, excessive sizes, unsupported versions,
bad later rows and allocation failure return errors without a partial archive.
Replay identity uses the existing canonical replay header codec; it is not a
cryptographic proof of gameplay or authentication of editable local files.

Saving first validates and encodes the entire archive, then invokes an injected
exclusive-create storage port once. Reading is bounded and validates complete
bytes before returning historical data. Pure policy code performs no file, OS,
clock, database or network access. A separate native file adapter maps safe
single-component keys under an explicit caller-owned directory to bounded reads
and exclusive publication, preserving existing files. Write/flush/sync errors
leave no partial final file. Owned staging cleanup is best effort; file sync
does not establish directory-entry power-loss durability.
The directory must remain caller-owned. Standard-library path checks and open
are separate operations and do not guarantee containment if another actor
concurrently replaces directory entries or the root.

This stage supplies the common format, storage policy and native adapter.
Native solo/local recording now connects actual typed completion to a sidecar
after cleanup and replay save attempts; see
[the native save contract](REQ__native-completed-result-save.md).
Browser source now exports actual stepped completion on the Worker and stores
the opaque archive alongside recordings in IndexedDB; see
[the browser storage contract](REQ__browser-completed-result-storage.md).
Native adjacent-record lookup and browser loaded-result UI now use the shared
exact historical association policy; see
[historical association](REQ__historical-record-association.md) and
[browser historical presentation](REQ__browser-historical-record.md).
Native local publication preserves the whole archive and additionally writes
one-row sidecars beside original member recordings; see
[local association](REQ__native-local-record-association.md). Existing local
files without member sidecars still require explicit association or migration.
Version 2 adds exact historical score/timing details from common Step completion
exports under [the detailed score contract](REQ__archived-score-details.md).
Version-1 archives and legacy scoreless publication retain their original bytes;
missing details remain unavailable. Version 3 optionally retains bounded
original-ID comparison snapshots through
[the comparison archive contract](REQ__archived-comparisons.md). Comparison-aware
Step and browser completion exports retain Rust-owned HUD prefixes; legacy
exports have no comparison table. Selected native comparisons now attach under
[native comparison association](REQ__native-archived-comparisons.md); historical
comparison UI uses [shared detail pages](REQ__historical-comparison-pages.md).
Actual execution remains unproven. Stored comparisons are display metadata, not
trusted final rankings or additional completion proof.
Actual native completion now also uses score-bearing version 2 under
[the native score association contract](REQ__native-archived-score.md).
Existing tests cover golden
bytes, policy/identity round trips, malformed and later-row cases, integer bounds,
storage refusal and exact call ordering. Current native publication software
evidence is tracked in [the storage change note](../changes/2026-10-10-native-record-publication-integrity.md).
Device/browser runtime and crash/recovery acceptance remain separate; the
earlier user instruction to defer verification no longer applies.
