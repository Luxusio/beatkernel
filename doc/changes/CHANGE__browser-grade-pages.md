# Connect browser historical grade paging

The current increment connects Window paging controls to the Worker-owned Rust
historical grade presentation. Shared pure host-model validation bounds page
metadata, tracks one pending command and accepts only its matching receipt.
Window handles control metadata and lifecycle; archive statistics and cached
canvas geometry remain in Rust on the Worker.

Successful page changes preserve the historical binding and replay selection.
Stale identities, duplicate requests, invalid pages and busy phases refuse before
publication. Same-page requests do not call the setter or schedule a new draw.
Timeout or inconsistent metadata clears an uncertain historical display while
keeping replay selection usable; late responses cannot replace a new selection.

## Known ceiling

Source routing and fifteen independent fixtures are authored: six pure pager
groups, five actual Worker VM groups and four actual main DOM/Worker/storage
groups. Existing assertions remain. Scripted compatibility updates add the grade
binding API, three control elements and the unavailable clear receipt already
returned by production Worker. Disposal/redraw and post-failure diagnostics were
extended in existing new groups without removing assertions. Both source and
independent-author lanes, including corrections, returned terminal stop reports.
They use scripted binding and browser endpoints, not generated WASM or real GPU/
IndexedDB execution. This increment changes only JavaScript/HTML; earlier four
Rust compile checks for the unchanged grade adapter do not verify these routes.
JavaScript parsing, Node/assertions, browser/native acceptance, formal review,
required QA, verify and close remain deferred. Text/whitespace inspection is
available and git diff whitespace checks exited zero. No script parsing or
assertion execution was performed, and unchanged Rust checks were not repeated.
Measured performance, saved comparison archival and the full player
acceptance remain unfinished, and the Goal stays active and unproven.
