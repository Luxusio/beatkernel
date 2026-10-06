# macOS cohort output ownership

Local macOS cohorts now connect observation, pause/resume, finite end and paused
output replacement to the same common output owner/service as solo gameplay.
One static CoreAudio composition factory is shared by both application roots;
startup still uses the original raw stream held inside that owner and the
original MachClock instance.

Nonnetwork local play advertises device/buffer/channel-matrix settings through
the existing typed CoreAudio mapper. Replacement policy, epoch/frame basis,
producer pause hold, Mixer custody and group timing publication remain common
application logic. No second cohort-specific output policy is introduced.
The original full-roster attachment checks, retained input FIFO, player/device
IDs and physical timestamps remain unchanged.

Network startup and observation use the owner but do not advertise manual output
controls. The cohort initializes its output UI within the outcome boundary, so
an initialization error follows normal audio stop, HID close and cohort result
cleanup. Final diagnostics handle a missing current output without skipping
either cleanup. Watch controls and broader format/rate conversion remain pending.

Independent code and security reviews passed, followed by scoped CLI QA:
runtime library 1,584 passed / 2 ignored, main 231 passed, macOS CLI 16 passed,
explicit common finite-endpoint fixture 1 passed, and actual ALSA null settings
diagnostic 1 passed, all with zero failures. Workspace all-targets, WASM browser
library and a fresh macOS application all-targets source check exited 0.
CLI help exited 0; malformed device input and native playback on Linux returned
the expected exit 1.

macOS-specific cohort/mapper fixtures compiled in the cross-source check but
did not execute on Linux. Portable owner/cohort tests and
macOS source checks cannot prove actual CoreAudio/HID device execution or GUI
pause/replacement/acoustic timing. Native macOS acceptance remains unfinished.
