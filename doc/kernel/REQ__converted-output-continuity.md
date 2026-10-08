# Exact converted output continuity

Status: selected prerequisite contract; implementation/acceptance pending.

The original Mixer source format and source identity remain immutable. One
portable owner must retain the Mixer, converter, exact next rational source
position, past kernel history and pulled-but-unconsumed PCM together across
cold target-rate/channel/buffer changes. Recovering only the Mixer cursor does
not recover samples already pulled into the converter.

Retarget prepares every fallible validation, allocation, capacity and coefficient
change before commit. Refusal preserves the original owner, voices, queue,
phase, counters and subsequent PCM. Exact arithmetic is bounded and checked;
representation overflow refuses without drift. Existing quality presets and
channel policy remain unchanged. Required history survives equal-rate segments;
an inherited fraction or pending PCM cannot take an incompatible direct path.

Rendering uses fixed prepared storage and static dispatch, without callback
allocation/reallocation/deallocation or locks. Source RenderReport facts stay
separate from target callback extents/cursors, source consumption position and
pulled frontier. Kernel width is not measured presentation latency. Source
render refusal cannot generically roll back source-side effects.

Continuity may use additive cold setup while existing fixed-rate constructors
keep their behavior. Zero target extent may invoke the existing generic source
callback with an empty slice for its report; converter phase/cursors and source
pull extent remain inert. Arbitrary callback-side effects are not rollbackable.

An explicit target-held primitive writes bounded silence without consuming
source commands or advancing converter source phase/history. Lookahead remains
available after release. Held target facts do not establish native pause ACK,
Mixer pause adoption, source execution or endpoint presentation. Ordinary
source paused reports never prove cached target PCM is silent. Native pause,
end, start and two-grid evidence mapping remain later required integration.

Verification includes independent piecewise rational/PCM oracles, fixed
transition-boundary partition comparisons, repeated fractional and same-rate
retargets, sinc history transitions, capacity/matrix changes, zero extent,
overflow/allocation refusal and retry equivalence. Actual Mixer/queue fixtures
must exercise ownership transfer, command preservation, cached nonzero PCM
with pause requests, held retarget/resume and honest finite-end facts. Callback
allocation instrumentation covers the changed rendering paths.

This prerequisite does not enable unequal native rates. Native retirement and
failure ports must later carry the complete owner; ALSA, WASAPI, CoreAudio and
ASIO need actual target-grid evidence, pause/end/publication integration and
supported-host validation. Hardware gaps, acoustic latency and performance
claims require measured evidence. BK019 and the full player Goal remain open.
