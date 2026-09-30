# Source chart compilation

BeatKernel now compiles nonnegative integer beat ticks, rational BPM changes
and nanosecond STOPs into immutable absolute song timestamps. Both object
endpoints are compiled, bindings and metadata are preserved, and a borrowed
half-open time-window lookup uses the sorted object array. Same-beat objects
use the pre-STOP timestamp; redundant BPM changes and zero STOPs preserve
rounding anchors. Separate SV markers never alter judge targets. Eleven golden
tests cover timing, ordering, range endpoints, validation and overflow in debug
and release, and the chart example demonstrates a point and a range. The
[chart contract](../kernel/REQ__chart-compiler.md) defines the observable rules.

Known ceiling: Source beats are nonnegative integer ticks under a u32 resolution
— extend the source representation when a concrete format adapter requires
negative chart positions or subdivisions that cannot fit that resolution.
File parsing, judging, visual projection and audio remain later Goal phases.
