# Retained native room Results

Native room Results now retains every prepared remote host/player row after the
network owner joins, rather than losing all but the last visible page. One
immutable bounded archive captures the actual accepted prefixes and cached text
after the post-join poll. Player snapshots share that archive through Arc.
Results paging projects at most four rows on a user event, while rendering
continues to borrow the cached page. No network thread or protocol authority is
kept alive to browse scores.

The joined snapshot may contain a newer core-accepted prefix than the previously
disconnected live HUD displayed. A private retention path preserves that prefix
with the original identity, monotonicity and finality checks; ordinary live HUD
updates after disconnect still refuse. Retention does not reconnect gameplay or
clear the network error.

Missing prefixes, final-prefix flags and actual diagnostics stay distinct.
Cancelled or failed play may expose retained reported prefixes without becoming
a verified result or ranking. Seal, Ready and Leave remain closed; browsing
archived pages cannot reopen controls. The first Results page preserves live
selection; invalid pages refuse atomically, and replacement games own fresh
archives. Archive presentation failure must preserve local judgment and the
actual joined network outcome.

Independent deferred fixture source adds six groups: two immutable archive
groups, two actual controller/join groups and two actual Desktop control groups.
They cover 4,032 remote rows/1,008 pages, full-width qualified identity, partial
pages, unavailable presentation, actual common-client/relay-accepted prefixes
retained after join and prior disconnect, genuine protocol-derived drain evidence
with separate cleanup failure, local paging and replacement isolation. Existing
controller fixture helpers are reused through a child module; their two leading
inner-doc comments became ordinary comments for include wiring, with no assertion
change. The fixture sources have not been executed.

Both source writers actually stopped before scoped rustfmt and whitespace
inspection. Workspace/all-targets with WebTransport (session 62467), native
no-default/all-targets with WebTransport (61432), WASM browser (14373) and WASM
browser-audio (46052) each exited 0. WASM retained three existing cadence
dead-code warnings. These are compile-only checks. No tests, applications,
browser/device/network runs, generated bindings, formal reviews, QA,
task verification or close were executed.

## Known ceiling

Live native GUI, TLS/device, generated browser bindings and measured performance
acceptance remain unverified. Reported room prefixes are presentation data and
do not establish ranked authority. The full player Goal and Harness task stay
open, with no review/QA/runtime PASS or close claimed.
