# BMS invisible input-sound voice and sample plan

The application converts the adapter's exact invisible song-time selections
into the portable core input-sound timeline. A dedicated voice for each lane
keeps fallback presses separate from gameplay and background audio; later
fallback presses on that lane reuse its voice. The plan retains original
resource identities and exposes their sorted unique sample set for subsequent
asset admission. It performs no file or device work.

The existing local VoiceAllocator also supports input-sound markers, preserving
intentional voice reuse and rejecting exhaustion without partial mutation.
Independent fixture code is prepared separately for deferred execution.

Four independent fixture groups cover parsed timing/resource/gain behavior,
actual fresh-press/hit precedence, invalid setup/namespace boundaries and
consecutive local-player remap/atomic exhaustion. Both writers stopped before
scoped Rust formatting and compile-only checks. Whitespace checks and workspace
all-targets, headless WebTransport all-targets, WASM browser and WASM
browser-audio cargo checks each exited zero. The WASM configurations retain
three existing unused cadence-code warnings. Tests were not executed.

## Remaining integration

Actual BMS preparation still rejects nonempty invisible selections. Enabling
playback requires sample loading, solo/local runtime installation, replay
identity and actual replay audio reconstruction. Section starts must retain the
latest selection before their start rather than erase earlier invisible data.
Invisible-only touch play also needs explicit contact ownership configuration;
it cannot rely on the existence of a visible touch-judged note.

Test execution, native/browser/device/audio behavior, performance and formal
review/QA remain deferred. This prerequisite does not establish full playback.
