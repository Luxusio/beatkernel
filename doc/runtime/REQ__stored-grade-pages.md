# Bounded stored grade pages

HistoricalRecordPresentation displays every stored opaque grade/count pair in
ascending order, four entries per page. Preserve exact u32 grade IDs and u64
counts, including zero/MAX IDs and counts beyond floating-point precision.
Do not invent friendly judge names or infer missing metadata from replay prefixes.
Unavailable legacy scores show STORED GRADES UNAVAILABLE; present empty score
tables show STORED GRADES EMPTY. Both have one display page, with no grade rows.
At most 4096 grades produce 1024 pages. This changes presentation, not archive bytes.
Stored comparison metadata now adds at most nine detail pages after those grade
pages under [historical comparison details](REQ__historical-comparison-pages.md).
Legacy unavailable metadata adds no pages. Common page accessors and native
navigation now cover the combined detail sequence, bounded at 1033 pages.

Add GRADE_ROWS_PER_PAGE=4, grade_page(), grade_page_count() and
set_grade_page(page)->Result<bool,String> to the common presentation. Same-page
requests return false and retain packets. Out-of-range or usize::MAX pages refuse
before mutation. Prepare the new grade leaf packet before publishing its page.
Base metadata, timing packets and stored grade vector remain retained. Stable
composition appends cached packets without grade scans, copies or formatting.
GeometrySnapshot may cheaply clone its immutable Arc-backed rectangles/batches;
cloning must not copy the underlying vectors.

The common grade leaf uses a heading near y514 and four rows at 538/554/570/586.
Native detail controls use 66 Back at (754,620,176,34), 67 Previous at
(430,575,140,34), and 68 Next at (580,575,164,34). Catalog DETAILS stays at
(304,575,110,34). Render/hit availability must agree: bounds admit only enabled
controls, with Previous absent on the first page and Next absent on the last.
No control may cover grade text. Legacy/empty/one-page details admit only Back.

RecordsDraft and RecordsFrame add grade_page:usize. Entering/leaving details,
selection/directory changes and accepted metadata replacement reset page zero.
Direction arrows and PageUp/PageDown navigate stored pages only while details
are visible; Home/End select first/last. Clamp at boundaries without wrapping or
changing the catalog selection/page. Catalog-grade controls remain isolated from
detail controls. Invalid/stale/pending detail frames and invalid pages refuse
before changing current cache/mode. Detail hover/pressed changes reuse the cached
grade leaf without repainting hidden catalog packets.

Native detail staging prepares a grade packet and the complete composed packet
before committing page/cache state; fallible work cannot leave a half-updated
visible page. Do not reconstruct HistoricalRecordPresentation or copy all grades
on every page action. Reuse pure helpers to compose base and prepared grade packets.

BrowserHistoricalRecord exposes grade_page, grade_pages and set_grade_page(u32)
through the same common logic. Its rendering remains Worker-owned. Interactive
Worker message routing and Window controls now follow
[the browser paging contract](REQ__browser-grade-pages.md); generated binding
and real browser acceptance remain unproven.

## Evidence and known ceiling

Author pure exact grade ordering/boundary/empty/legacy/stable-cache fixtures,
actual retained Records render/hit/cache/paging cases, and desktop controller
keyboard/back/reset cases. Preserve old assertions, migrating only default page
initializers and the deliberately relocated detail Back hit coordinates. Public
historical fixture values must not call the private live-completion constructor
from the desktop binary crate. No tests, JS parsing, apps or hardware execution;
four compile-only checks follow both paired writers stopping. Assertions, formal
review, required QA, runtime acceptance, verify and close remain deferred.
Cold first presentation and page leaf builds may allocate; existing initial
grade copy remains. Comparison archival/detail rendering is source-integrated;
browser/native execution, room-wide metadata and platform acceptance remain
unfinished, and the full player Goal stays active.
