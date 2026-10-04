# Native room controls and retained score presentation

The existing player latest-state channel now carries Arc-shared room metadata
and a cached selected score page of at most four qualified rows. New portable
RoomPresentation and action/request/reply types contain no native protocol or
judgment authority. Metadata updates copy the bounded roster only on actual
change; ordinary player publication copies Arc references. Hidden peer updates
do not rebuild the selected page, and diagnostic text is cached behind Arc.

PlayerViewer queues Seal, Ready, Leave and Page intents. One capacity of 16
covers queued, processing and unread replies together; busy requests refuse
without consuming an identity. The native competition owner maps UI identities
to separate real network command identities and preserves correlated admission
or refusal results. Page requests have local controller results and do not
invent network receipts. Contention retains dirty publication/replies without
resending protocol commands or blocking local player publication. Cancellation
wins, pending requests settle explicitly, and fresh channels isolate replacement
sessions from old viewers. Controller polling services the bridge during lobby,
commit waiting and gameplay; finishing closes controls before joining.

Desktop controls 90..94 use actual snapshot capabilities and pending-request
state. Seal requires the first actual participant, Ready requires the actual
Frozen/unprepared local member, and Leave/page requests respect closing state.
The display changes page only after an actual owner result. Replay, retry,
cancelled and joined owners disable live room controls. The header uses room
controls in place of inapplicable practice controls. A clipped x=0..600,
y=646..720 score footer leaves existing local page controls at x>=620; note
and input field geometry remain unchanged. Renderer inputs remain cached display
data, with no new game loop, dependency, crate or operating-system protocol.

Independent deferred fixture source adds seven groups: three player/portable
presentation groups, two actual Desktop groups, and two native controller bridge
groups. They cover bounds, contention/cancellation/session isolation, host/player
identity and paging, clipped Scene geometry, actual controls, separate IDs,
stale refusal and startup cancellation. Fixtures have not been executed.

Both writers actually stopped before scoped rustfmt and whitespace inspection.
The initial workspace/all-targets check found one fixture reference to a
nonexistent `fixtures` module; it now uses the actual sibling
`tests::lifecycle_fixture`. The affected workspace/WebTransport check then exited
0. Runtime no-default/all-targets with WebTransport, WASM browser and WASM
browser-audio also exited 0. The two WASM checks retained three existing cadence
dead-code warnings. No tests, parsers, applications, generated bindings,
device/network runs, formal reviews, QA, task verification or close were executed.

Known ceiling: native app room-mode selection and actual solo/cohort room-session
instantiation are still pending; existing bilateral application routes remain.
Closed Results retains only its selected score page, with room controls closed.
Browsing all final score pages requires an explicit retained-result ownership
phase. Live UI, transport/browser/device and performance acceptance remain
unverified. Full Goal and Harness task remain open; no runtime PASS is claimed.
