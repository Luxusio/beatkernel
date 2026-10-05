# Common gauge HUD

A common gauge_hud molecule now draws actual native and Worker-owned gauge
snapshots with rectangle/text atoms. The integer-derived percentage truncates to
two decimals; a 16-byte stack label and wide-integer fill calculation add no
per-frame label allocation or gauge clone. Text and color distinguish GAUGE,
READY, DEAD and EMPTY, including recoverable zero. Malformed bounds and endpoint
arithmetic are rejected before drawing, and text remains component-clipped.

Native solo and browser Worker solo/replay use the same sidebar strip. Each local
member borrows its own gauge in the existing header, clipping the neighboring
judge label without moving playfields, touch geometry or comparison reservations.
Browser preview passes no gauge and creates no fictitious live state. Drawing
does not judge, advance replay or mutate gauge/score. No Window JS, DOM gauge or
new polling/render loop was introduced.

Four independently authored deferred fixture groups cover fixed-point label/bar
boundaries, status words/colors and read-only repeated draws, malformed bounds and
clipping, and 64-member page layouts with independent gauges and unchanged field/
touch/comparison bounds. Both writers delivered terminal Writes STOPPED before
scoped rustfmt and compile-only checks. Assertions were not run.

Workspace/all-target WebTransport, headless/all-target WebTransport, WASM browser
and WASM browser-audio each completed with exit 0. Scoped rustfmt and git diff
--check succeeded. Existing unused playfield-wrapper and WASM cadence warnings
remain. Compilation does not establish GPU visual or timing acceptance.

Known ceiling: threshold readiness is not song-clear evidence. Actual gauge
failure fencing, clear/fail completion, configurable live capture policy and
high-level mine admission remain unfinished. Browser/native GPU/device execution,
performance measurements, assertions, formal review and QA remain deferred.
