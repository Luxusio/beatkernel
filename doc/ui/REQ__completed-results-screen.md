# Native completed results screen

The native Results screen displays the first immutable completed result table
published by the actual gameplay owner. It shows each player's original identity,
exact final gauge, outcome, and full-song or practice scope. Practice results must
not be labelled as whole-song clears. Replay prefixes and sessions without proven
completion must not manufacture a completed result from their score or status.

The game owner must finish cleanup before navigation enters Results. Completion
evidence may arrive earlier and survives a later technical cleanup error; the UI
keeps that error separate from the historical gauge outcome. Room results and
their paging remain available alongside the local results.

The UI component consumes explicit data without native I/O or gameplay mutation.
It validates the entire 1..64 player table before accepting it, rejecting missing,
duplicate, foreign identities or mixed scopes. It retains formatted presentation
and geometry between draws, and bounds visible local players to four per page.
No per-note rendering or result reclassification runs on the completed screen.

Independent deferred fixtures cover full/practice/failed outcomes, large original
identities, atomic malformed-table refusal, page bounds, retained composition,
cleanup ordering, and absence of completion for replay/cancelled sessions.
Tests are authored and compiled only under the standing deferred-verification
instruction. Device, GPU, browser, assertion execution and performance acceptance
remain deferred; this requirement is not a completion or performance receipt.
