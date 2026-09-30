# ALSA mixer execution reports

`AlsaStream::last_render_report()` exposes the last successful actual core
Mixer.render report through the existing internal portable Telemetry publisher.
No new seqlock or native query is introduced. The report includes cumulative
core execution counters, active voices, pending commands, rendered frame grid,
song position and producer disconnection. Queue admission and judge results are
separate from mixer command execution; voice-full, late-command and other
execution counters do not undo or retry an already accepted judge result.

The worker publishes immediately after successful render, before format
conversion or ALSA submission. A report therefore proves successful core render,
not successful native write, acoustic playback or a current native clock reading.
Failed render publishes no replacement. A previous successful report survives
conversion/write failures, stop, join and terminal failure for diagnostics.
Before the first successful render, during busy publication or after publication
version exhaustion the public result is None. Existing telemetry's bounded
SeqCst read/publication protocol and exhaustion policy are reused unchanged.

An internal AudioStreamSnapshot carries only the RenderReport, Running carrier
status, absent clock and zero default native counters. This carrier is private;
the public method returns only the report, preventing fabricated native fields
from becoming observations. Existing AlsaSnapshot counters remain independently
observed and existing native timing availability/coherence rules are unchanged.
Publication serializes fixed numeric fields into already allocated atomics;
it adds no callback/control locks, allocation, formatting or file/native IO.

Inline fixtures use actual pure Mixer rendering with queued voice overflow,
unknown samples, late commands and producer disconnection. They exercise the
public ALSA facade through a private worker-free owner, including absence before
render, report replacement, preservation across failed render and stop/failure.
Existing shared telemetry fixtures already cover scalar encoding and publication
exhaustion; duplicate field-encoding tests are not added. Fixtures are authored
and compiled, but no tests, native calls, QA or review are executed during the
user's verification deferral.
