# ASIO planar PCM conversion

The SDK-free `audio::asio` converter selects a mixer channel from interleaved
finite float32 PCM and writes one exactly sized planar native ASIO buffer.
The caller supplies channel count, selected channel and native encoding explicitly.
It allocates nothing and performs no locking, I/O, logging, decoding or device
calls. Device buffer ownership and callback lifetimes remain outside this API.

`AsioPcmEncoding::from_native` recognizes the eighteen SDK PCM types: 16/24/32-bit
signed integer and float32/64 in both byte orders, plus 32-bit containers with
16/18/20/24 valid bits in both byte orders. Native type identity and sample width
remain available. Unknown and DSD type identities produce an explicit error
retaining their numeric identity: a PCM mixer is not a DSD modulator.

Reduced-valid-bit ASIO containers place significant bits at the low end, with
unused high bits zero. This differs from the existing WAVE PCM converter's
left alignment. Format identities follow the [SDK declarations](https://github.com/audiosdk/asio/blob/main/common/asio.h);
the [upstream PortAudio ASIO host](https://github.com/PortAudio/portaudio/blob/master/src/hostapi/asio/pa_asio.cpp)
also shifts full-width integers down for these output types. These sources are
evidence for the format interpretation; their code is not incorporated.

Integer conversion shares the existing converter's quantizer: clamp to [-1,1],
round to nearest with ties away from zero, saturate to signed valid-bit bounds.
Float32 preserves finite values, including signed zero and values outside [-1,1].
Float64 widens float32 exactly. No dither or implicit channel mixing occurs.

Channel count must be nonzero, the channel index valid and input frames complete.
Output length must exactly match frames times native sample width, with checked
arithmetic. Selected-channel values must be finite. Every check precedes any
output write, so errors preserve the destination. Nonfinite values in an
unselected channel do not invalidate this single-channel operation. A valid
selection with empty complete input and empty destination is accepted.

`AsioChannelInfo::pcm_encoding` maps actual driver metadata to this converter's
encoding without claiming support for unknown types or native output. The
[control bridge](REQ__asio-driver-control.md) is consumed by the separate
[stream](REQ__asio-stream.md) when preparing SDK buffers. Its SDK-free
`AsioBlockRenderer` owns the actual Mixer and preallocated scratch, validates all
output planes before Mixer mutation and checks the entire mixed block for finite
values before writing any plane. The last successful Mixer report remains visible
even if subsequent PCM delivery fails. Zero frames or channel-count mismatch is
`InvalidConfiguration`; exceeding the actual Mixer render limit is `Capacity`.

Source and fixtures are SDK-free MIT code. Current tests, native execution,
independent review and QA remain deferred. Compilation alone does not establish
native sample interpretation, callback performance or playback acceptance.

Rust 1.98.1 locked all-target compilation passed for the host workspace and the
platform package targeting Windows GNU and macOS x86-64. Ten new portable
fixtures and the existing platform PCM fixtures compiled without execution.
The optional SDK-gated Windows control wrapper and C++ bridge are not compiled
by these checks. Allocation assertions are authored evidence to execute later,
not measured callback behavior or a current no-allocation acceptance verdict.
Seven additional actual-Mixer renderer fixtures are authored and compiled without
execution; they cover heterogeneous literal planes, preflight atomicity, retained
reports, capacity, contiguous rendering and allocation instrumentation.
