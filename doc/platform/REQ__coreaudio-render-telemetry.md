# CoreAudio mixer execution reports

`CoreAudioStream::last_render_report()` returns the last successful actual core
Mixer.render report using the existing internal portable Telemetry publisher.
It reports cumulative core execution counters, rendered frame grid, song
position, active voices, pending commands and producer disconnection. Queue
admission, judge results, core rendering and native/audible delivery are separate
outcomes. Execution rejection counters do not undo or retry accepted judging.

The existing guarded native callback is the exclusive publisher. Its RenderState
owns the version counter; Context owns the already allocated atomic publisher.
A successful core render is published immediately before copying PCM to native
buffers. Failed render does not publish a replacement. The callback adds no
allocation, locks, native queries, formatting, logging or file IO. The private
AudioStreamSnapshot carrier has absent clock and default zero native counters;
the facade returns only RenderReport, preventing synthetic native observations
from being exposed.

Before the first successful render, during busy publication or after version
exhaustion the result is None. Successful reports are diagnostic history, not a
fresh native presentation association or acoustic-delivery guarantee. Existing
CoreAudio aggregate/presentation telemetry keeps its prior coherence semantics.

Successful stop first disables/stops the IOProc, removes its property listeners,
destroys registration and waits for all in-flight callback/listener accesses to
quiesce. It then copies the final successful render report before releasing the
Context. Later reads return that retained report. Stop failure retains the
existing Context and registrations for retry; reads continue through the bounded
atomic publisher. No callback-lifetime or failure-path ownership guarantee is
weakened, and repeated stop preserves the final report.

Existing portable Telemetry fixtures cover serialization, bounded availability
and publication exhaustion. Existing actual Mixer fixtures cover command
execution rejection/late/disconnection reports; these are reused as authored
coverage rather than duplicated field-encoding tests. Constructing an actual
MachClock/native callback context is platform work, so native callback execution
verification remains explicitly deferred. Scoped formatting and Apple-target
library/test compilation are allowed; no tests, native calls, linking, review or
QA are executed during the user's verification deferral.
