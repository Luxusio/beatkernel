# Linux native acquisition and output (Phase 12)

Linux native I/O belongs only in beatkernel-platform::linux. The final app
selects input node paths and ALSA endpoint names explicitly; no default endpoint
is invented. Callers assign runtime DeviceId and ClockDomainId identities. IDs
remain fixed for the opened handle; reconnect requires a fresh runtime identity.
The native ABI implementation supports Linux x86_64 and aarch64; other targets
return UnsupportedTarget rather than guessing ioctl layouts.

Evdev opens read-only nonblocking and requires EVIOCSCLOCKID(CLOCK_MONOTONIC).
Kernel event timestamps become checked integer nanoseconds with native code,
backend namespace and acquisition sequence. Standard keyboard keys use the
existing linux_evdev_key canonicalizer; other buttons and relative/absolute axes
retain type/code-qualified native identities. Axis samples use the core f32
payload in native units; very large i32 values can lose precision in conversion.
EVIOCGID, name, unique ID and capability queries describe the actual device.

SYN_DROPPED is not a synthetic key release. Acquisition reports a loss barrier,
discards through the next SYN_REPORT, queries current keys and absolute axes,
and requires explicit caller acknowledgment of the snapshot before forwarding
new events. The snapshot cannot recover missing relative deltas or the exact
number of lost events. Hosts cancel/reconcile held gameplay state at this barrier.
Unknown evdev event types are ignored, not mislabeled as canonical input.

Hidraw reads derive Input report lengths and numbering from the descriptor,
with a 16 KiB report cap independent from the kernel's 4096-byte descriptor cap.
A sentinel byte detects reports exceeding that declared bound. Output/Feature
report IDs do not cause unnumbered Input payloads to lose their first byte.
The full payload is retained, with numbered Input IDs stored separately. Descriptor and native device metadata are
available to a DeviceAdapter; no generic interpretation of vendor payloads is
invented. Hidraw exposes userspace monotonic acquisition time, not a device
hardware timestamp. Backend/meta explicitly identify this distinction.

ALSA dynamically loads libasound.so.2; missing libraries or required symbols are
explicit failures without a build-time development-library requirement. Endpoint,
format, buffer frames and period frames are supplied by the caller. Exact is the
default; explicit size rounding opt-in permits ALSA's nearest period/buffer only,
with applied settings exposed. Rate, channel count and encoding never fall back.
Speaker channel masks are unsupported and must be unspecified. The backend uses
RW_INTERLEAVED and nonblocking snd_pcm_writei with finite snd_pcm_wait wakeups;
it does not claim mmap, realtime scheduler priority, or a guaranteed latency.

A dedicated worker owns ALSA handles, Mixer and preallocated render/conversion
buffers. Opening/configuration/allocation precede start. Start/stop are explicit;
stop requests wakeup and joins before teardown, including on Drop. Submission
handles short writes and EAGAIN without rerendering or losing pending frames.
EPIPE xruns and ESTRPIPE suspend failures are counted and terminal: callers must
explicitly reopen/reconstruct audio state instead of silently resetting its frame
timeline. The worker does not allocate or take application blocking locks in its
render/write path; ALSA's internal implementation is not certified realtime-safe.

Telemetry distinguishes submitted frames, mixer scheduling cursor, actual xrun
signals, native errors and a measured CLOCK_MONOTONIC observation. The mixer
clock domain/origin remain caller-provided; a scheduling frame cursor is not a
physical presentation clock. No automatic mapping from native input time to
output time or physical latency estimate is claimed. Hardware execution,
benchmarks, and formal QA remain deferred by user instruction (2026-09-30).

The separate [ALSA timing contract](REQ__alsa-timing.md) adds native status,
signed playback delay and an associated monotonic status timestamp. A worker-owned
preallocated status container and bounded atomic publication keep those fields
coherent independently of the aggregate counters. Submitted minus valid native
delay supplies an explicitly estimated sound-frame position; prepared/nonrunning
status and invalid delay cannot create that estimate. Native timestamps and the
userspace query interval remain separate. These observations do not prove acoustic
latency or automatically calibrate input. The separate
[Linux BMS composition](../kernel/REQ__bms-linux-native.md) uses explicitly estimated
native sound-frame/host pairs with the same bounded continuous presentation
discipline as the Windows sample. Its settings, input loss policy and native
execution limitations are documented independently.

The separate [mixer render report](REQ__alsa-render-telemetry.md) exposes the last
successful core render outcome and its cumulative command execution counters.
It is retained after output stop/failure for diagnostics. Rendering, queue
admission and native frame submission are distinct; this report does not prove
audible output or provide a fresh presentation clock. Native timing and aggregate
counter coherence remain unchanged.

Primary ABI/API references:
- [Linux input UAPI](https://github.com/torvalds/linux/blob/master/include/uapi/linux/input.h)
- [Linux input event synchronization](https://www.kernel.org/doc/html/latest/input/event-codes.html)
- [Linux hidraw UAPI](https://github.com/torvalds/linux/blob/master/include/uapi/linux/hidraw.h)
- [Linux hidraw semantics](https://www.kernel.org/doc/html/latest/hid/hidraw.html)
- [ALSA PCM API](https://www.alsa-project.org/alsa-doc/alsa-lib/group___p_c_m.html)
- [ALSA PCM declarations](https://github.com/alsa-project/alsa-lib/blob/master/include/pcm.h)
