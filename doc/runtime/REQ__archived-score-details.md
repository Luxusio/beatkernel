# Exact historical score details

Completed StepGameplay and StepLocalGameplay archive exports shall include each
original player's actual score summary: hit/miss counts, current/maximum combo,
opaque grade counts and exact known-stage timing statistics. Export only after
the existing typed completion is latched and with the original capture identity.
Incomplete, cancelled and consumed-capture paths retain their existing meaning.
Do not infer completion from score, gauge or decoded historical bytes.

Extend BKRESULT with version 2 while continuing to decode version 1. Preserve
version-1 bytes for archives built by the existing scoreless constructor; absence
of details means unavailable, never zero. Version-2 entries append a tagged score
record after the existing outcome. Preserve little-endian exact integers, including
signed/unsigned 128-bit timing sums and i64 delta extrema. Do not store rounded
means or use floating-point conversion. Preserve whole-set and member-sidecar
projection. No partial or duplicate score roster is accepted.

Use public historical ArchivedScore and TimingRecord value types, distinct from
live completion and mutable runtime accumulators. TimingSummary.record() produces
its exact scalar value; TimingRecord.validate() checks bucket sums, empty/nonempty
option shape, ordered extrema/last, sign consistency and achievable magnitude
bounds with checked arithmetic. Reserve one sample for each distinct minimum,
maximum and last value when checking sign-specific counts and sums; these values
cannot be invented outside the observed sample total. ArchivedScore::from_summary
copies existing
ordered grades into a bounded vector and validates counters and timing. Reject
grade sums differing from hits, impossible combo bounds, timing count exceeding
hits, unsorted/duplicate/zero-count grades, and more than 4096 grade entries.
Byte, header and original 64-player bounds remain unchanged. Validation establishes
structural consistency, not authenticity or proof that a chart was played.

Version-2 score payload order is hits, misses, combo, maximum combo (four u64s),
grade-entry count (u32), and ascending (u32 grade, u64 count) pairs. Timing follows
as count, early, late, exact (four u64s), signed sum (i128), absolute sum (u128),
then last, minimum and maximum delta, each with a 0/1 presence tag and optional
i64 value. The leading score tag is 0 for unavailable or 1 for this payload;
refuse other tags and mixed availability across the roster. All numbers use
little-endian fixed widths. Scoreless construction emits version 1; a decoded
uniformly scoreless version-2 value may canonicalize to version 1 on re-encoding.

ResultArchive::from_completed_with_scores(rows, identities, scores) accepts
scores as an exact whole-roster table of (PlayerId, &ScoreSummary); associate by
the original ID, never position. Existing from_completed remains scoreless.
ArchiveEntry.score is Option<ArchivedScore>; for_player retains its exact details.
The common historical presentation shows stored counts and timing when available,
and retains scoreless legacy rendering. Cache geometry once; Window gains no
rendering, parsing or business logic.

## Evidence and known ceiling

Author independent literal version-1/version-2 wire, integer extremes, malformed
shape, roster association, whole/member projection, truncation and actual Step
completion/export fixtures. Do not rely solely on encoder/decoder round trips.
Host and WASM compile checks do not establish runtime behavior. Test execution,
formal review, QA and platform acceptance remain deferred. Actual native
completed-save callers now use explicit score association under
[the native score contract](REQ__native-archived-score.md). Existing scoreless
public helpers retain version 1. Saved opponent comparison snapshots are not
archived by this increment. Cold serialization/decoded values can allocate;
no new per-frame work, global allocation-free claim or authenticity guarantee
is established by this serialization increment.
