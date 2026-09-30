# ALSA native status timing

The ALSA worker owns one preallocated `snd_pcm_status_t` from successful open
through teardown. It enables timestamp mode ENABLE (1) and type MONOTONIC (1),
applies software parameters, reloads current parameters and verifies both exact
values. Missing symbols, rejected settings or readback mismatch reject open;
there is no fallback clock. Status storage is allocated before Start and freed
on every owner return path before unloading libasound.

The ABI uses the signatures and enum values in ALSA's official
[pcm.h](https://github.com/alsa-project/alsa-lib/blob/master/include/pcm.h).
Status state, signed delay, available frames and native high-resolution timestamp
come from one successful `snd_pcm_status` result. The
[status API](https://www.alsa-project.org/alsa-doc/alsa-lib/group___p_c_m___status.html)
provides signed delay in frames. Its high-resolution timestamp uses a
[timespec](https://www.alsa-project.org/alsa-doc/alsa-lib/group___global.html).
The existing backend restricts native ABI support to Linux x86_64/aarch64,
where native long/time_t are signed 64-bit values.

`AlsaStream::timing_snapshot()` returns a separately coherent optional
`AlsaTimingSnapshot`. It includes raw native state integer (retaining unknown
future values), signed delay, available frames, raw seconds/nanoseconds, the
same-worker submitted frame count, verified timestamp mode/type and explicit
CLOCK_MONOTONIC query-start/query-finish points. This is not a coherence claim
for existing `AlsaSnapshot` aggregate counters, render state or lifecycle.
Native PREPARED/other successful states may produce raw timing with no played
estimate while the writer is running. Before Start, after stop, on terminal
failure, during publication or transient query failure timing is unavailable.

Optional `native_timestamp` requires nonnegative seconds, nanoseconds in
0..1,000,000,000, representable checked nanoseconds, a nonzero value and matching
ordered query-bracket domains. The raw timespec is retained even when invalid.
It is the ALSA timestamp, never a userspace receipt timestamp or midpoint. ALSA
may update that stamp before the userspace call bracket; it need not lie inside
the bracket. Callers must independently evaluate timestamp age. Quality is
always Unknown: configured monotonic units imply no acoustic or numeric accuracy
bound.

`estimated_played_frames = submitted_frames - delay_frames` is available only
for native RUNNING (3), nonnegative delay no larger than the submitted count,
and a valid associated native monotonic timestamp. This is a checked software
estimate from native status, not acoustic Exact, absolute hardware position or
permission to treat queued-but-unsubmitted mixer frames as played.

The worker retains the last successful timing observation between status queries,
including ordinary buffer-full write EAGAIN and interrupted wait operations. Its
original native and call timestamps are never refreshed by those operations;
callers assess age. The worker invalidates timing immediately before querying
status and on terminal/stop/unwind. Status EAGAIN/EINTR publishes unavailability;
no prior valid record is reused after a failed status query. Other native status failures
are terminal and preserve existing EPIPE xrun/ESTRPIPE suspend accounting.
Only the worker calls status APIs. The caller reads atomic fields through a
SeqCst version protocol, retries at most 64 times, and returns None if busy.
There are no caller locks, allocations or native calls. The publication sequence
does not wrap: exhaustion permanently disables publication. Stop-side clearing
occurs after join, retaining the single-publisher invariant; the stop flag also
suppresses caller reads immediately.

Pure fixtures are authored for checked timestamp/delay extremes, state gating,
metadata preservation, timestamp-before-bracket semantics, unavailable/busy
publication, replacement and invalidation. No tests or native ALSA calls are
executed during the user's verification deferral. Formatting and compilation
are implementation checks and do not establish native timing accuracy.
