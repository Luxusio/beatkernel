# Browser menus across playback

The gameplay owner suspends a visible menu when it receives play-start. The
Window must retire its corresponding menu token, geometry, pending editor and
gesture state at that dispatch. Stop, completion or a refused dispatched start
does not implicitly resume the suspended menu. A late reply cannot reopen it.

Audio or input setup that fails before play-start is dispatched does not suspend
the business menu; retain its existing usable owner. Explicit menu-open creates
the fresh menu lifetime and carries the current roster/opponents/fields.

After playback, idle player/source controls use their current local model when
there is no active menu. Increasing the player count must enable explicit
source acquisition/assignment without sending actions to an old suspended
token or silently reverting to one player. Reopening the menu imports that
current roster and accepts only the new owner's identity.

Keep source menu action validation, monotone action IDs, input acquisition,
audio authority, gameplay/replay identity and graphics feedback unchanged.
Verify actual Window messages/controls and real menu/play/idle/reopen journeys.

## Development evidence — 2026-10-09

Three focused lifecycle cases passed (live/replay retirement, pre-dispatch
audio refusal, dispatched refusal/natural completion); the complete Window
suite passed 165 tests. A current-source Chromium development run opened the
menu, completed live playback, changed the idle roster to two players, acquired
keyboard/touch sources, replayed the complete six-second record and explicitly
reopened the menu. The new menu generation2 imported players[1,2] from the
current roster. No stale-menu-owner error occurred. Browser/server exited;
logs, screenshots and actual messages are ignored artifacts under
`target/wf/browser-menu-after-playback/development`.
The complete Node suite also passed 791 tests with zero failures, cancellations
or skips. The attempted independent formal reviewer allocation still returned
the host's agent thread limit; no independent verdict was produced.

Independent review and QA remain required. The host previously refused agent
allocation at its thread limit; development evidence is not a substitute for
those results. Keep the full player Goal active and do not infer whole-screen
or whole-product completion from this lifecycle correction.
