# Long-duration visual time

Twenty-hour and one-week compiled chart extents retain integer target and
Runtime song times independently of rendering cadence. Virtual traversal,
pause and genuine replay restoration verify logical history; late placement
alone does not prove this property or a physical wallclock soak.

Lane displacement represents the scroll integral between the current song
time and each head/tail. Unrelated large earlier scroll history must not erase
an ordinary local displacement. Float arithmetic remains visual only; fixtures
use independent integer/rational local oracles with explicit local tolerances.
Exact arbitrary rational visual arithmetic is not promised.

The public Rust projector regression reproduced distance zero instead of one
third after a valid SV of one billion followed by one third at the twenty-hour
horizon. A same-segment-only correction passed that case but still reproduced
zero instead of negative one third across the next marker. Logs are preserved
in `target/wf/long-duration-visual-time/{baseline-red,crossing-red}.log`.

The correction stores per-segment local areas in a compact setup-time range-sum
tree of two f64 nodes per scroll segment, replacing cumulative positions.
This tree is allocated only when a Lane binding exists; other projection kinds
do not pay for a range index they cannot use.
Same-segment queries subtract i128 times directly; crossed queries combine two
boundary pieces and the indexed sum of complete interior segments. Song segment
lookup is reused for each lane head/tail. Setup and memory remain O(S), queries
O(log S) or better, with no frame-time allocation or crossed-marker scan. This
excludes irrelevant history from arithmetic; it does not promise exact sums
when large positive/negative contributions cancel within the requested range.

Any demonstrated correction preserves indexed object and scroll queries,
O(log S) or better scroll work, and prepared caller-owned frame storage without
per-frame marker scans. Reverse/decreasing Runtime continuation retains its
existing restore-required refusal; backward history uses actual replay seek.

The app's PlayfieldCache is a separate renderer-input boundary. Its bounded
local epoch, i128 time subtraction, head/tail coordinates and drift are checked
across longextents, pauses, epoch rebases and backward seeks. This does not
establish that the app consumes the core VisualProjector.

## Known ceiling

CPU geometry and bounded virtual schedules do not prove physical GPU rendering,
device latency, wallclock weeklong stability, arbitrary custom callback purity
or world-leading performance. These retain their full player WBS obligations.
