# Catalog page and wheel navigation

Selection supports PageUp/Down by fifteen visible rows and Home/End within the
current search results. Focused search Home/End still edits the text caret.
Wheel input over a displayed chart row navigates the same original chart
identities. Pixel deltas use the renderer's physical/logical stretch and row
height; positive vertical movement selects earlier rows. Fractional movement
accumulates, with at most fifteen steps per event and no oversized backlog.
Horizontal input leaves vertical remainder alone. Invalid input, route/query/
focus changes, suspension, resize and pointer departure clear accumulation.
Scrolling cancels armed clicks and never emits gameplay input or timestamps.
Consecutive wheel events use retained painter-order hit regions even before
redraw; central click hits stay invalidated until the new rows are composed.

Cursor movement is constant time within the cached search projection; it does
not rescan or allocate a new result list. Retained row dependencies and screen
scope ownership remain in place. Prepared fixtures cover filtered page/edge
clamping, huge jumps, empty results, stable projection identity, text focus,
fractional scroll and lifecycle admission, click cancellation and pixel stretch.
They also cover consecutive scrolling before redraw and the search field's
overlap with the first catalog row.
Fixtures are compiled only; actual GUI and test execution remain deferred.

Known ceiling: fifteen fixed-height rows in the 960x720 logical viewport remain;
responsive layout will need measured dimensions and a shared scroll policy.
