# Windows local-player composition

Windows now connects distinct keyboard assignments from Settings → Players to
the actual shared-output local RuntimeGroup. Stable positive player IDs and
exact Raw Input interface paths survive profile/CLI import; fresh native device
IDs and handles are resolved at preparation. The game owner acquires all
keyboards in one bounded message pump, while each player keeps independent
judging, score, saved opponents, completion and capture. The existing output
owner supplies one calibrated transport, PCM bank, BGM scheduler and mixer,
with the existing WASAPI shared/exclusive and optional ASIO settings.

The input merger preserves original QPC receipt metadata and stops deadline
advancement while messages remain queued. The advance margin defaults to 2ms
and accepts 0..1s. Pre-origin inputs are counted and skipped without rewriting
timestamps; future/late/regressed input and selected removal stop the group.
Output joins and Raw Input unregisters before every independent replay save
attempt. Committed partial reports remain published/captured, and saved paths
use stable `.p<ID>.bkr` suffixes with the existing no-overwrite boundary.

Known ceiling: 2..64 local players, shared active-voice budget 1..4096, finite
command and merge capacity. A local group cannot currently join the two-peer
network mode; macOS native groups remain pending. A QPC receipt timestamp does
not establish physical actuation time. ASIO reuses the existing feature-gated
owner and still needs SDK/native acceptance; its SDK build was not run here.

Only scoped formatting and source checks were performed during the user's
continuing verification deferral. Device/GUI/audio/replay/file/network execution,
independent formal review and QA remain required before acceptance. The complete
BMS player Goal remains active.
