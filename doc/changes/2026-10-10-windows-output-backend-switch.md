# Windows live output backend selection

Paused Windows solo/local settings now select WASAPI or optional ASIO through
the existing shared replacement owner. Target configuration is validated before
retirement, inactive backend fields remain inert, and original PCM rate/channel
basis and Mixer recovery are preserved. Invalid copied ASIO CLSIDs cannot select
a WASAPI endpoint; clearing a cross-switch target device selects the OS default.
See the [contract and exact development checks](../kernel/REQ__windows-output-backend-switch.md).

## Known ceiling

Actual ASIO SDK/MSVC compilation, installed-driver playback and physical
cross-backend latency/continuity remain unverified; they require the real Windows
SDK and device environment. Portable fixtures and non-ASIO foreign Rust typing
do not establish those results. Whole-player WBS completion remains separate.
