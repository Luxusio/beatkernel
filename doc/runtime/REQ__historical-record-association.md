# Historical record association and native preview

A pure shared policy associates a decoded whole result archive with the exact
canonical ReplayHeader of a selected recording. An explicit original player ID
must identify a matching row. Without an explicit ID, exactly one matching row
is required; zero matches or multiple matching rows refuse association. A file
name, row position, replay prefix, score or gauge threshold cannot select a
player or create live completion evidence. Returned archive data stays historical
and does not construct CompletedPlayResult. Header consistency is not
authentication of editable recordings or proof of gameplay completion.

Native record preview decodes canonical section setup and reconstructs only
actual accepted operations, supporting original finite/unlimited extents while
checking current draft profile, seed, start/end and input mode. It never adds a
synthetic end advance or miss. Preview retains the exact optional end and prefix
statistics independently of stored final gauge/outcome. Existing unlimited
recordings remain compatible; incompatible drafts refuse the prefix.

After a valid prefix is reconstructed, the native metadata worker may inspect
the adjacent `.bkresult` sidecar appended to the whole selected replay filename.
Absent sidecars leave the normal prefix preview usable. Bounded non-symlink
regular-file reads and complete archive decoding precede pure association.
Corrupt, oversized, mismatched or ambiguous sidecars retain the valid prefix and
an explicit archive diagnostic; no historical row is accepted partially.

The retained Records UI displays associated historical player ID, full/practice
scope, outcome and exact gauge units separately from recomputed prefix scores.
It caches this presentation and keeps filesystem/decoding out of rendering.
Catalog/preview data declarations live in a pure record model; the UI imports
those values instead of the filesystem adapter module. Existing adapter exports
may retain compatibility while native composition roots select effect methods.
No completion claim is inferred from a stored boolean or replay extent.

Automatic adjacent lookup supports directly associated recording sidecars.
Native local saves preserve a whole-roster sidecar at the configured base path
and now publish adjacent one-row sidecars beside original member recordings;
see [the local association contract](REQ__native-local-record-association.md).
Those files carry explicit original IDs and exact headers. Existing local
recordings without adjacent member sidecars still require explicit archive/player
selection or migration. The UI must not guess IDs from member filenames.
Browser loaded-record
presentation now uses the same matcher through a separate Worker-owned historical
binding; see [its contract](REQ__browser-historical-record.md). Version-2 score
and timing preservation and common historical presentation are governed by
[the detail contract](REQ__archived-score-details.md). Native score export uses
[explicit completed score association](REQ__native-archived-score.md).
Comparison archival, actual browser/filesystem/
GPU acceptance and race-free directory containment remain unfinished.
Native Records now retains associated final score metadata separately from its
prefix and exposes a cached detail subview under
[the stored detail contract](REQ__native-record-details.md). Full grade-table
presentation and saved comparison archival remain pending.

Independent deferred fixtures cover exact header fields and original IDs,
ambiguous/missing rows, finite draft equality, long extents, recorded-prefix-only
scores, historical display provenance and retained updates. No tests or real
device/browser/filesystem/GPU execution run under the standing instruction.
Scoped formatting and the exact four compile-only checks completed after both
writers' terminal stop. All four checks exited zero; the eight fixture groups
remain unexecuted. Compile evidence cannot establish execution or authentication.
