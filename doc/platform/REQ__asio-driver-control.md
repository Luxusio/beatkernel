# ASIO SDK driver control

The optional `beatkernel-platform/asio-sdk` feature enables a Windows native
control bridge compiled against a caller-supplied SDK directory in
`BEATKERNEL_ASIO_SDK_DIR`. Builds never download the SDK. Default builds use no
SDK or C++ compiler. Enabled Windows SDK builds currently require an MSVC target
and MSVC-compatible C++ compiler, including clang-cl. The bridge rejects Windows
GNU targets rather than assuming compatible SDK virtual-member ABI; proving and
supporting that configuration remains future work. Project-authored bridge
source remains MIT; SDK-combined
artifacts follow the [GPLv3 distribution contract](REQ__asio-distribution.md).
Preserve the supplied SDK revision, license files and build inputs for release.

Driver opening uses an explicitly selected registration compatible with process
bitness. The owning thread initializes COM and releases the driver before
balancing COM on that same thread. The control handle cannot move between
threads. Initialization accepts an explicit host system reference because some
drivers require a real host window. A supplied window must belong to the owning
thread and remain alive while the control handle uses it. `AsioControl::open`
is unsafe because the caller owns that lifetime and the trusted native driver.
No driver is selected automatically. Its consuming `close` reports cleanup
failure; Drop also releases the driver before balancing COM.

Queries expose actual channel counts, buffer bounds, current rate, latencies and
channel metadata. Driver failures preserve operation and native code; malformed
reports fail explicitly. Channel names preserve bounded original bytes without
assuming UTF-8, and unknown nonnegative sample type identities remain visible
without claiming conversion support. `AsioChannelInfo::pcm_encoding` explicitly
selects the SDK-free [planar PCM converter](REQ__asio-pcm.md) from that reported
type and rejects unsupported types. Ordinary methods accept only `ASE_OK`;
the SDK's special `ASE_SUCCESS` for future calls is not accepted as success here.
Sample-rate changes and driver control panels require
explicit calls. External clock selection is an explicit request with native
rate zero, distinct from a positive finite Hertz request.

Portable `audio::asio` configuration types require no SDK. Buffer selection
preserves the driver's minimum, maximum, preferred size and granularity.
Positive granularity permits minimum plus integral steps; minus one permits
minimum multiplied by successive powers of two. Zero granularity with distinct
bounds adds no step restriction; equal bounds require zero granularity and an
equal preferred size. Invalid reports are rejected. Exact frame requests never
round or fall back. Native buffer creation remains the final support check.
These semantics use the SDK's [buffer and rate specification](https://github.com/audiosdk/asio/blob/main/common/asio.h).

## Implementation and acceptance boundaries

Control and portable configuration are implemented separately from the
[owned output stream](REQ__asio-stream.md). `AsioStream::prepare` consumes the
control, so callers cannot change rate or show driver UI while its buffers and
Mixer callbacks are active. Driver discovery remains a
[separate SDK-free path](REQ__asio-driver-discovery.md). WASAPI's backend selection
still reports ASIO unavailable; callers use the separate ASIO stream API.

Portable fixtures can be compiled on Linux; default Windows Rust checks do not
compile an enabled SDK bridge. The current local environment lacks a Windows
C++ compiler. Actual Windows SDK feature compilation and native control acceptance remain
pending. Tests, native execution, independent reviews and QA remain deferred
by the user's instruction; no playback or synchronization claim follows from
source or compilation alone.

Rust 1.98.1 host workspace all-target compilation passed for default and locked
all-feature configurations, compiling nine portable fixtures without executing
them. The locked Windows GNU platform all-target check passed for the SDK-free
configuration. Linux all-features does not compile SDK C++ or the target-gated
Rust control wrapper, so it supplies no Windows SDK control acceptance evidence.

A subsequent metadata-only Windows GNU Rust compilation included the target-gated
control and stream Rust source without activating the Cargo SDK feature or linking
C++. This establishes Rust type-checking only. It does not establish enabled GNU
SDK support, MSVC ABI compatibility, actual SDK compilation or native acceptance.
