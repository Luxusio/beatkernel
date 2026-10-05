# Native stored record details

RecordPreview keeps reconstructed accepted-prefix ScoreSummary separate from
historical archive data. Add historical_score: Option<Arc<ArchivedScore>>;
copy an associated version-2 score once on the metadata worker, then retain
immutable shared data. Preserve all opaque grade entries and exact timing.
Absent/version-1 score metadata means unavailable, never an empty score inferred
from the reconstructed prefix. Exact header and original PlayerId association
remain required. Failed association or score-copy preparation clears both
historical attachments, reports archive_error and retains the usable prefix.

Use the same score-copy association logic for explicit from_file_with_archive
and adjacent sidecar lookup. Prepare the score before publishing historical
identity/result. Do not rerun archived final statistics or create live completion
from historical data. Existing decoded historical records remain untrusted.

The Records controller adds a stored-detail subview under the current parent
screen lifetime. A DETAILS button (ControlId 66) and nonrepeat D while list-focused
open only a valid selected historical preview with no pending metadata operation.
Set directory focus inactive, cancel gestures/IME composition as appropriate and
invalidate old hits. Show stored provenance, original player, full/practice extent,
outcome, gauge, final counters and exact timing sums/extrema when present. Version-1
details retain historical metadata and explicitly unavailable score statistics.

The subview shows only its back control (66), which returns to the same catalog
selection/page without popping the Records navigation entry. Escape/back also
return to the catalog first; the next back follows normal navigation. Ignore
catalog actions, text edits, paging, watching and opponent mutations while details
are visible. Selection/directory replacement and pending metadata requests clear
detail mode. Disposing the parent releases cached geometry/shared metadata; no
second navigation stack, gameplay owner, IO resource or independent modal life
is introduced. Invalid/stale/pending detail frames refuse before UI mutation.

HistoricalRecordPresentation.from_record(value, score) builds the same cached
pure historical geometry as browser display, without reading files, parsing
replays or decoding archive bytes. The Records subview reuses it. Build geometry
only on changed associated metadata or changed immutable score identity; stable
frames reuse packets without grade-vector copies, string formatting or decoding.
Cache the complete detail packet, including background/back button. Detail-only
hover/pressed changes update that packet without repainting hidden catalog nodes.
RecordPreview still retains full grades even though the current detail screen
focuses on totals/timing; scrolling grade-table UI remains future work.

Keep the catalog's existing controls and paging. Compress preview text rows within
the existing 514..571 area to avoid overlap with secondary controls at 575 and
opponent labels at 576/592. DETAILS fits the unused 304..414 secondary-row area.
Business/metadata acquisition remains outside UI, and browser Window gains no
rendering or business logic.

## Evidence and known ceiling

Author independent actual ReplaySession/prefix association, v1/v2 and original-ID
fixtures, retained geometry/cache and hit isolation fixtures, and actual desktop
controller back/invalidation/pending/input cases using injected data without OS
applications. Keep old assertions; default initializer compatibility and the
existing timing-glyph coordinate expectation may change to match the deliberately
compressed layout. Preserve its exact glyph filtering, repaint and cache checks;
the independent author owns that coordinate migration after the source writer
stops. Source and compile checks do not prove browser/GPU,
filesystem, runtime, performance or complete lifecycle acceptance. Assertions,
formal review, required QA, verify and close remain deferred. Cold metadata copies
and initial detail geometry may allocate; no global allocation-free claim is made.
Full grade-table paging, saved comparison archival and platform acceptance remain
unfinished, and the full player Goal stays active.
