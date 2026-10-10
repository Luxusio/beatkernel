# Native completed-result saving

Individual replay and whole/member sidecar files must follow the
[complete native publication contract](REQ__completed-result-archive.md#complete-individual-native-publication).
The final name receives complete synchronized bytes through exclusive linking;
write/flush/sync/link refusal preserves existing targets without a partial final
file. Group writes remain nontransactional and retain all-attempts/first-error
ordering. Best-effort owned-stage cleanup never deletes a final or masks an
original error. This intended strengthening replaces the partial-file limitation
below; its implementation and independent verification are in progress.

Windows, macOS and Linux solo/local application entry points use the same
completed-result finalization policy. Only actual typed pump completion, or
completion retained in a typed publication error, can supply archive results.
Cancellation, setup refusal, bounded-duration cutoff and a valid replay prefix
cannot manufacture completion. The original actual gauge policy and each
capture's pristine replay header are associated with the complete roster.
Actual native solo/local roots now include their independent exact score/timing
summaries in version 2 under [the native score contract](REQ__native-archived-score.md).
Legacy scoreless helper APIs continue emitting version 1.

When replay recording is enabled and actual completion exists, save one whole
roster result sidecar next to the configured base replay path after native
output/input cleanup attempts. Solo uses its replay path; local uses the
configured base path rather than inferring roster position from individual
destinations. Append `.bkresult` to the entire base filename. All replay saves
are attempted before archive publication. Local publication additionally stages
one-row archives beside each original member recording using the ID in the
archive, with no filename-derived ID inference; see
[the local association contract](REQ__native-local-record-association.md).
Every staged whole/member sidecar write is attempted before returning the first
exact publication error. Earlier gameplay or cleanup errors
retain precedence, but cannot erase actual completion or skip the archive
attempt. Archive validation/save refusal returns an error when no earlier error
exists. Prefix recordings remain available without a completion sidecar.

Business finalization receives explicit save callbacks, performs no OS calls,
and validates the complete table before external archive publication. Native
roster owners are projected to bounded borrowed player/capture/profile values;
the pure association policy does not receive concrete competition, network or
device owners. If recording was enabled and actual completion exists, absent
member captures are errors rather than silently reclassified disabled recording.
All allocation/encoding for saving occurs after the realtime pump and cleanup
attempts; generic callback injection adds no per-note dynamic dispatch.
Native filesystem publication remains an outer exclusive-create adapter and supports
the original path's native filename without lossy conversion. Existing files
are preserved. Each native file is published only after complete staging,
file synchronization and handle closure. Best-effort staging cleanup,
directory-entry power-loss durability and group transactionality remain
explicit limitations.

Capture setup must preserve the actual original start and optional end in its
canonical header. Unlimited captures retain their existing compatibility
encoding. Browser persistence source now has its
[own storage contract](REQ__browser-completed-result-storage.md).
Native adjacent record lookup and browser loaded-result presentation now use the
shared exact association policy. Local recordings saved before adjacent member
sidecars still require explicit association or migration. Existing
fixtures cover real pristine finite capture,
typed completion/error retention, original roster associations, cancelled/prefix
refusal, cleanup/error precedence and save ordering. Current native filesystem
publication software evidence is tracked in
[the storage change note](../changes/2026-10-10-native-record-publication-integrity.md).
Device/browser execution and Windows/macOS runtime remain separate acceptance;
the earlier verification deferral no longer applies.
