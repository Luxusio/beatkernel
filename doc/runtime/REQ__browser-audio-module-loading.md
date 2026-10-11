# Browser audio module preparation

Status: selected implementation contract; implementation and independent QA
pending. This does not complete the BMS player or diagnose the recurring
Chromium audio-WASM cancellation.

The existing business Worker owns audio WASM fetch, bounded body storage,
byte copying and compilation. Window requests preparation and receives only a
compiled `WebAssembly.Module`; it does not load, accumulate or compile the
audio binary. Input acquisition and permission gestures remain Window-owned.
AudioWorklet creation still follows the existing user-gesture AudioHost path.

CPU readiness does not wait for audio preparation. File import, chart
preparation and preview remain available while audio is pending or failed;
Play waits for a matching valid Module and all existing play prerequisites.
Optional audio failure uses bounded existing status feedback, without making
the business Worker fatal or fabricating audio availability.

## Request and ownership

After the current Worker reports ready, Window sends one `audio-load` with a
positive safe-integer `id`. Repeated ready cannot start another preparation or
clear an accepted Module. The Worker rejects invalid/pre-ready requests before
I/O or ownership changes. It constructs the fixed same-origin audio package URL;
Window cannot choose the package path.

Worker replies are `audio-module-ready` with the original `id` and actual
`module`, or `audio-module-error` with `id` and a message capped at 4096
characters. Module uses structured clone without a transfer-list entry.
Window accepts only its current ticket's Worker, owner and id, and an actual
Module. Missing, mismatched, stale, duplicate or malformed replies cannot
enable Play or overwrite the current accepted module. Stop/restart retires
the ticket before an old response can publish into a replacement owner.

Only one physical setup operation may run. During setup, identical id is
ignored; another valid id receives a bounded busy error without replacing the
operation or starting more I/O. A successful Module is cached for later valid
requests in the same Worker. Failure permits a fresh explicit request after
the underlying operation settles; it does not retry automatically.
New physical operations consume a monotonic request-id floor. Same or lower
consumed ids cannot restart a failed operation; competing busy requests do not
consume that floor. Cached Modules require no new physical operation.

## Limits and cleanup

Require a successful HTTP response with a body. Validate each reader chunk as
`Uint8Array`, ignore zero-length chunks and refuse accumulated bytes above
64 MiB before allocation/copy. Grow contiguous storage geometrically from at
most 64 KiB, capped at 64 MiB; do not keep an unbounded list of chunk objects
or allocate 64 MiB upfront. Compile precisely the admitted binary extent.
Old/new buffers during growth can transiently total up to 96 MiB; fetch and
engine compilation storage are additional. This is not an aggregate memory
or latency guarantee.

One ten-second deadline covers fetch, reads and compilation. Timeout retires
publication, aborts transport and reports one optional bounded error. Reader
cancellation must not hold cleanup hostage. Clear the timer on terminal paths.
Release a fully consumed reader without canceling it; cancel only unfinished
readers during failed or retired setup.
Compilation cannot be aborted: retain physical capacity until the operation
settles, refuse competing setup during that interval, and never cache or
publish a retired operation's late result.

Dispose and fatal entry retire/abort audio setup before deferred capture,
playback or room cleanup. Preserve those existing cleanup sequences. Retired
owners publish neither late success nor late optional failure.

## Verification

Actual-WASM Worker fixtures cover real Module compilation, response/body/read/
compile failures, malformed/empty chunks, exact/over-limit bytes, duplicate and
competing ids, cached success, explicit retry, publication cloning failure,
timeouts, nonsettling cancellation and disposal/immediate/deferred fatal.
Deferred compile tests prove no second compile after timeout until settlement.

Window fixtures use a genuine empty WASM Module and traps on Window audio fetch
and compile. Check stale ids/Worker/owner, fake modules, duplicate ready/replies,
restart, optional errors, preview usability and Play prerequisites. A fresh
production browser run must observe Worker preparation and module delivery,
zero Window audio preparation, and preserve existing trusted live input,
capture/replay, Display and renderer-acknowledgement journeys.

Keep all strict page, console and network assertions. A reported audio-WASM
abort remains a failure even when application reading and compilation complete.
Worker migration alone is not proof that the browser diagnostic was repaired.

## Known ceiling

Setup can temporarily compete with preview work in the existing business
Worker. Physical input/audio latency, real hardware performance, every browser
engine and the recurring cancellation's root cause remain unestablished.
