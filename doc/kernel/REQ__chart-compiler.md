# Chart compiler contract

BeatKernel keeps format-specific chart parsing outside the core. `beatkernel::chart`
accepts a simple `SourceChart` and returns an owned, immutable `CompiledChart`
whose object times are absolute song nanoseconds. The two are logical modules
inside the existing core crate, not separate crates.

## Source timeline

- A chart chooses a nonzero number of integer ticks per beat. Source positions
  are nonnegative ticks. The beat-zero song time is zero before any STOP.
- Initial and changed BPM values are positive rational beats per minute. STOP
  lengths are nonnegative integer song nanoseconds. File adapters convert their
  own timing units before constructing a source chart.
- At a tick shared by objects, BPM changes, SV changes, and a STOP, all markers
  and object endpoints receive the time **before** that STOP. The BPM change
  governs intervals *after* that tick; the STOP shifts only later ticks.
- There is at most one BPM, STOP, or SV event per tick. Duplicate object IDs are
  invalid. Source event and object order do not affect the result.
- The compiler maps each tick from the most recent genuine BPM or positive STOP
  boundary with checked wide integer arithmetic, truncating fractional
  nanoseconds toward zero once per segment. Redundant equal-BPM changes and
  zero STOPs do not create rounding boundaries. Overflow is an error, never a
  wrapped or saturated timestamp.

At 120 BPM and 480 ticks per beat, an object at tick 480 has time 500,000,000 ns.
If a 250,000,000 ns STOP occurs there, an object at tick 480 remains at
500,000,000 ns and one at tick 960 moves to 1,250,000,000 ns. If BPM also
changes to 60 at tick 480, the tick-960 object instead lands at 1,750,000,000 ns.

## Compiled objects and lookup

Each source object has an ID, start tick, optional end tick, interaction and
visual IDs, optional audio binding, and owned opaque metadata. The compiler
maps both endpoints independently and rejects a reversed source range. It sorts
compiled objects by start timestamp, then object ID. Time-window lookup returns
a borrowed slice of starts in `[start, end)`; it allocates no memory.
An empty or reversed query window returns an empty slice, and a ranged object
is included only by its start time, not merely by overlap with the window.

SV is a signed rational visual speed. Compiled SV markers have their own
absolute timestamps, but changing SV never changes an object's judge target.
Visual projection and game-specific interpretation belong to later phases.

Invalid resolution, BPM, SV denominator, STOP, beat/range, duplicate markers or
IDs, and arithmetic overflow return a typed error without a partial chart.
Compilation may allocate and must run outside future real-time audio callbacks.
The compiler accepts at most 1,000,000 combined objects and timing/visual
markers, bounding sorting and indexing work. This count does not bound opaque
metadata bytes; untrusted file adapters must enforce their own byte limits.
