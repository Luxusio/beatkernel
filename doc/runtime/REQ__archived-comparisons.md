# Historical comparison snapshots

Completed historical archives may retain a bounded, original-player-ID keyed
comparison table. These are display-only snapshots of saved replay operation
prefixes and optionally peer-reported network progress. They never establish
completion, a final ranking, trusted remote judgment or clock authority.

Keep existing scoreless v1 and scored v2 encodings byte compatible. A separately
tagged v3 extension stores the comparison table only when explicitly attached.
Legacy decoding reports comparison availability as absent. An attached table
covers the whole original roster, including explicit None rows for no selected
comparison; preserve the difference between unavailable and known empty data.
Member projection validates the whole archive before retaining the selected
original ID's comparison row. Never renumber IDs.

Attachment accepts borrowed snapshots, stages a complete owned bounded copy and
commits only after all validation succeeds. Reject duplicates, unknown/missing
IDs, more than eight ghosts per member, labels exceeding 256 UTF-8 bytes or 64
characters or containing controls, malformed counters and invalid peer progress.
Preserve signed i64 prefix timestamps, u64 counters and opaque u32 player IDs.
Reject empty labels; attachment validates the original bounded label then copies
only its final slash/backslash-separated basename. Stored labels must be nonempty
display basenames without either separator. Exact framing and discriminants
must be specified in the change document after implementation. Decode bounds
precede allocation and all tags, lengths, UTF-8 and trailing bytes are validated.
The existing 5 MiB archive envelope limit applies to the whole extension.

Actual Step solo/local export gets a cold optional comparison-aware path while
existing score-only exports preserve v2 behavior. Actual BrowserGame and
BrowserLocalGame completion export attaches the current Rust-owned saved-HUD
snapshots before capture consumption; JavaScript cannot supply completion proof.
Whole completed-roster/capture checks and refusal ordering remain intact. No
per-frame archival copying or new native effects are introduced.

Author independent codec, malformed-data, roster/projection, atomic attachment
and actual Step completed-export fixtures. Assertions remain deferred; scoped
formatting and four sequential compile-only checks run after both paired writers
stop. Native solo/cohort comparison attachment now follows
[native completion association](REQ__native-archived-comparisons.md). Historical
comparison detail pages now follow
[the shared detail contract](REQ__historical-comparison-pages.md). Actual
browser/native execution remains unproven. Existing score/timing pages remain
available before the new comparison pages. Full BMS player Goal remains
active; formal review and required browser/CLI/desktop QA are still outstanding.
