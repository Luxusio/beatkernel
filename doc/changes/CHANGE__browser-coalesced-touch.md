# Browser coalesced touch movement

The browser input owner must preserve intermediate physical touch movements
exposed by `getCoalescedEvents()`, instead of forwarding only the aggregated
`pointermove`. The common live solo/local host acquires a bounded original
sample list; the existing Worker performs touch projection, lane routing,
judgment and rendering. The contract lives in
[REQ__bms-browser.md](../kernel/REQ__bms-browser.md#coalesced-touch-movement-acquisition).

Use a nonempty coalesced list instead of its parent. Empty or unavailable lists
retain ordinary acquisition. No predicted inputs or extra raw-update listeners
are introduced. Preserve per-sample timestamps and pressure, one held contact,
equal-time acquisition order and shared full-width input sequences. Nondispatched
child offsets are not canvas-relative evidence: acquire CSS coordinates from
the dispatched parent's offset/client anchor and child client coordinates on
the existing untransformed canvas. Cached CSS/backing dimensions remain raw
acquisition metadata; no pointer-time layout read or lane lookup is needed.

Movement acquisition admits at most 256 samples per list into the existing
1024-event pending queue. Complete validation precedes publication: matching
identity, finite values, original chronological order, parent upper timestamp,
accepted watermark and remaining capacity. Failure is explicit, without
truncation or partial input publication. Existing Down, Up, Cancel, capture
ownership, page transition and session cleanup remain the lifecycle authority.

## Source evidence and deferred verification

`samples/bms-runtime/web/main.js::touch` implements the common solo/local
acquisition path. The implementation producer and independent test author
both returned actual terminal STOPPED finals before coordinator checks.
`samples/bms-runtime/web/play-host.test.mjs` adds four independent groups;
the existing 104 groups remain, for 108 total source groups. The new groups
cover original sample forwarding and fallback, malformed whole-list refusal,
256/1024 capacity boundaries and held-contact paging/cancellation/replacement.
The coordinates deliberately use dispatched-parent anchors while child offset
getters are unavailable or invalid. Fixtures also retain equal-time order,
original metadata snapshots and event-driven host ownership.

This is a JavaScript-only source increment. No Cargo check is needed for
unchanged Rust. Tests, JavaScript parsers, browsers, devices, performance runs,
formal reviews and QA have not executed. Task and full-player Goal remain open.

## Known ceiling

Actual browser/device delivery, transformed canvas acquisition, input latency
and cost under dense multi-touch remain unverified. The existing Worker input
chronology and accepted watermark continue to reject delayed prefixes; no new
retimestamping or rollback path is introduced. Verification execution remains
deferred by the user's standing instruction. Source fixtures are preparation
for that later execution, not runtime acceptance or full-player completion.
