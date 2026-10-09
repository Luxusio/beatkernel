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

## Verification commands and tolerances

The logical geometry fixtures compare independent local rational/literal
expectations with an absolute tolerance of `1e-12` in their normalized coordinate
units. The app's unclipped local head/tail plus drift uses a `0.001` pixel
tolerance derived from f32 renderer-input arithmetic. Neither tolerance grows
with the twenty-hour/week absolute timestamp; neither is a universal accuracy
guarantee for arbitrary geometry or cancellation inside a queried interval.

Executed development commands, using Rust 1.98.1 from the existing local
`target/toolchain/env.sh`, one compiler job, incremental/debug disabled, and
the cached `target/wf/worklet-chronology-qa-cli-1/cargo` target directory:

```sh
cargo test -p beatkernel --locked --test long_duration_visual lane_local_and_cross_segment_displacement_survive_huge_prior_scroll -- --nocapture
cargo test -p beatkernel --locked --test long_duration_visual -- --nocapture
cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked long_duration_playfield_fixtures -- --nocapture
cargo clippy -p beatkernel --all-targets --locked --no-deps -- -D warnings
cargo test -p beatkernel --locked
```

The first command produced the two deliberately preserved red reproductions
before the final correction. Focused checks subsequently passed eight core
tests and three app tests. Final all-target core Clippy exited zero with warnings
denied. Full-core regression initially timed out during compilation, before
executing tests; the final current-source run is recorded separately in
`target/wf/long-duration-visual-time/core-all-test-final.log`. A compilation
timeout is not a test PASS. Final independent review/QA evidence is recorded
after the applicable commands finish.

## Known ceiling

CPU geometry and bounded virtual schedules do not prove physical GPU rendering,
device latency, wallclock weeklong stability, arbitrary custom callback purity
or world-leading performance. These retain their full player WBS obligations.
