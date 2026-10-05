# Completed result archive

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
and exclusive creation, preserving existing files. Write/flush errors can leave
a partial newly created file; flush is not power-loss or crash-atomic durability.
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
exports have no comparison table. Native comparison attachment and historical
comparison UI remain pending. Stored comparisons are display metadata, not
trusted final rankings or additional completion proof.
Actual native completion now also uses score-bearing version 2 under
[the native score association contract](REQ__native-archived-score.md).
Independent deferred tests cover golden
bytes, policy/identity round trips, malformed and later-row cases, integer bounds,
storage refusal and exact call ordering. Assertions, filesystem/device/browser
runtime and crash/recovery acceptance remain deferred under the user's instruction.
