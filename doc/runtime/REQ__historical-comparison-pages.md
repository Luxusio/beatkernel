# Historical comparison details

Associated historical v3 comparisons appear in native Records and browser
historical detail rendering. Associate the whole validated archive with the exact
original replay header and player ID before copying metadata. RecordPreview
retains comparison availability separately from recomputed replay-prefix scores.
Use immutable shared retained data; failed association clears historical result,
score and comparisons together while keeping a valid replay prefix usable.

Preserve three states: unavailable legacy comparison metadata, an attached row
with no selected comparison, and an attached snapshot (possibly empty). Keep
existing score-only constructors compatible. Display historical comparison
availability explicitly; never convert unavailable to zero or invent peer data.

Existing historical page navigation covers score/grade pages first, then one
page per saved ghost and one page for a stored network snapshot. Known empty
comparison metadata has one explicit empty page. Legacy unavailable metadata
adds no pages, preserving existing score-only page bounds. At most 4096 grades
with four rows per page plus eight ghosts and one peer page means 1033 pages.
Keep existing browser grade-page bridge names for compatibility, but user labels
describe record details. Window still handles controls only; Worker owns geometry.

Saved pages retain own/other label, exact u64 counters, signed i64 recorded-until
prefix time (or unavailable), and an explicit saved replay operation-prefix
caption. Peer pages retain connection status, optional exact progress counters
and song nanoseconds, explicitly self-reported and not a verified final ranking.
All stored values remain separate from local completion authority. Preserve
readability without overlapping metadata, navigation or Back controls.

Prepare bounded comparison-page geometry on historical load, then reuse shared
packets on identical frames and page changes. Native cache identity includes
comparison data as well as score/result. Cheap page counts and selected-page
composition use the same common policy in native controller, UI and browser.
Bad page requests refuse atomically; same-page updates remain no-ops. Changing
records/reassociating/closing details invalidates the appropriate cache and resets
the existing page frontier. No new navigation stack or lifecycle authority.

Extend the existing browser bounded pager to 1033 with original owner/RPC/idle
fencing unchanged. Do not parse or render archive data on Window. Existing DOM IDs
and Worker command names remain compatible. Author independent association,
common geometry/page, native retained UI/controller and browser boundary fixtures;
migrate only affected old initializers/bounds/captions without weakening assertions.

Assertions, JS parsing, applications and runtime/browser QA remain deferred.
Scoped Rust formatting and four sequential compile-only checks run after both
writers stop. These do not prove rendered browser/native hardware behavior,
international font shaping or measured allocation/latency performance. Room-wide
metadata omitted by current archives remains unavailable. Full Goal stays active.
