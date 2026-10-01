# Native audio output metadata selection

BeatKernel remains a Rust cross-platform rhythm-game engine; the single BMS app
is its first composition. This increment builds a portable catalog before the
native query adapter and desktop view, consistent with bottom-up components.

Audio Devices queries WASAPI endpoint metadata, ASIO registry registrations,
ALSA output/duplex PCM hints or CoreAudio output metadata on the serialized
settings worker. Discovery opens no output stream and instantiates no ASIO
object. ASIO registry view is explicit and registry discovery requires no SDK.
WASAPI non-active endpoints remain visible but unselectable.

Selection is explicit: Up/Down or row click selects, Enter/Use Device updates
only the backend's exact draft device ID. Apply changes the next session.
Backend/ASIO-view mismatch, invalid rows and disabled devices reject selection
without changing the draft. Refresh replaces the catalog without selecting a
row; Back/Escape preserves the settings draft. Previous/Next and PageUp/PageDown
also expose pages containing only disabled entries. A pending worker blocks
other settings operations; close drains it.

The portable catalog rejects more than 1024 entries, 4096 UTF-8 bytes per ID,
label or detail, or 4 MiB aggregate text without truncation. Display controls
are flattened; native IDs are preserved. Providers may allocate native lists
before admission; discovery metadata is not a configuration certificate.
ALSA ownership, filtering and its initial allocation ceiling are documented in
[the platform contract](../platform/REQ__alsa-discovery.md).

Scoped formatting and source compilation are the permitted checks. Fixture
execution, actual discovery, GUI/device behavior, formal reviews and QA remain
deferred; this source increment does not complete the full player goal.

Source checks succeeded on 2026-10-01: workspace all-targets on Linux, BMS app
all-targets for Windows GNU and macOS x86_64, headless app all-targets, and the
WASM graphics library. Existing WASM cadence dead-code and macOS `block 0.1.6`
future-incompatibility warnings remain. No fixtures or native queries ran.
