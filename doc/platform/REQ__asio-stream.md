# ASIO native output stream

The optional Windows MSVC SDK stream consumes an explicitly opened
[ASIO control](REQ__asio-driver-control.md), the actual core Mixer, selected output
channel indices and an exact/preferred buffer request. Mixer sample rate must
match the driver; opening never changes it silently. Each mixer channel maps to
one distinct selected output, retaining heterogeneous native PCM types. All
buffers and callback state are allocated before native start.

ASIO callbacks contain no host context argument. The bridge therefore permits
one active buffer session per process and explicitly rejects another. A session
reservation lasts through native driver release. SDK buffer descriptors and
callbacks remain stable until disposal/release; the Rust mixer context outlives
callback quiescence. Callback admission increments a global reader count before
reading the owner pointer. Closing detaches that pointer, drains admitted readers
outside rendering, then stops, disposes and releases the driver before freeing
context. Concurrent/reentrant renderer entry faults without blocking or aliasing
the Mixer. Native output regions must be nonnull, representable and disjoint.

Buffer B is primed before start; the first index-zero callback fills buffer A
while B may play, following the [SDK start and callback contract](https://github.com/audiosdk/asio/blob/main/common/asio.h).
The stream starts once; stop is terminal. Reopening requires explicit caller
reconstruction rather than replaying a partially consumed mixer implicitly.
Start and preparation failures complete cleanup before releasing Rust context.

The SDK-free `AsioBlockRenderer` shares the actual Mixer and
[planar converter](REQ__asio-pcm.md). It validates every destination before Mixer
mutation, renders one block and validates the entire mixed scratch buffer before
writing any output plane. It retains the successful Mixer report separately
from native buffer preparation. Render allocates nothing, performs no blocking
locks, I/O, logging or decoding. Prepared frames describe native buffer writes,
not audible output or physical synchronization.

Reset, resync, latency, buffer-size and actual rate changes require explicit
reopening. Callbacks only record notifications/faults; they never launch UI,
reconfigure, stop or reopen. Overload remains a reported native diagnostic.
Time-info callbacks copy native sample position, raw system nanoseconds and
flags while valid. Legacy callbacks query actual sample position when available.
An unavailable observation remains absent; raw ASIO time is not QPC or a
normalized host timestamp. Native clock publication is bounded and coherent;
counter exhaustion reports unavailable rather than wrapping. Presentation mapping
and physical output acceptance remain separate requirements.

This source slice does not add outputReady optimization or hot buffer resizing.
WASAPI's backend still rejects ASIO requests; the separate `AsioStream` API is
the native path. SDK-combined artifacts follow the [GPLv3 distribution policy](REQ__asio-distribution.md);
default SDK-free builds remain MIT. Local SDK-enabled Windows compilation,
actual callbacks/playback, allocation assertions, independent review and QA
remain unverified/deferred. Source and default compilation do not establish
native output acceptance.

`AsioStream::prepare(control, mixer, channels, request)` consumes the actual
opening-thread control and Mixer, returning a ready stream. Call `start` once,
read `snapshot` off the render callback on that owner thread, then `stop`.
Stop is terminal and repeatable. Diagnostics and cleanup errors do not bypass
driver release/drain; fatal native faults are returned as `ReopenRequired`.
Setup and start failures retain the first operation error while attempting full
cleanup. Final native diagnostics are cached before close; subsequent software
snapshots retain the last actual Mixer report after callback drain. That cached
event is a pre-close observation, not a final audible position.

Rust 1.98.1 locked host workspace all-target and SDK-free Windows GNU/macOS
platform all-target checks passed, compiling seven authored renderer fixtures
without executing them. A separate metadata-only Windows GNU Rust invocation
included optional control/stream Rust source without activating Cargo's SDK
feature/build script or linking C++. It type-checks that Rust source only and
does not enable GNU SDK builds or verify actual MSVC/C++/SDK ABI compatibility.
Real SDK builds still require supplied headers and MSVC or clang-cl. Final-host
ASIO composition and presentation mapping remain implementation work.
