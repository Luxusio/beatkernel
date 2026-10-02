# Native multiplayer preparation barrier

Selected solo multiplayer sessions now wait for bilateral preparation before
native audio starts. Each native owner opens its assets, PCM schedule, output
and input first, then asks the networking worker to send one readiness frame.
The application proceeds only after the compatible peer's readiness is received
and its own complete readiness frame is written. BKMP wireversion3 adds an empty
ready frame; earlier versions1/2 are incompatible. Progress and terminal data
cannot precede readiness, while existing terminal-prefix acknowledgements retain
their semantics. Offline and saved-record-only sessions gain no startup wait.

The common competition barrier runs on the game owner inside the existing
cleanup-owned native outcome, immediately before Linux ALSA, Windows
WASAPI/optionalASIO or macOS CoreAudio start. The main graphical event loop and
audio callback do not wait. Cancellation, input loss/removal, decoding failure,
incompatible peers and the existing setup deadline abort through native cleanup.
Bounded input/message draining during the wait preserves foreground Windows
Raw Input cleanup and macOS HID/evdev failure handling. Waiting events never
enter judging, replay capture or audio commands and retain no invented timestamps.

## Known ceiling

This barrier confirms preparation; output clocks are still local. A common
scheduled start time, offset/delay estimation, preroll agreement and measured
physical synchronization remain to implement. Deadlines and native polling are
software controls subject to scheduling; they are not measured latency guarantees.
Network pause, local network groups, authentication/ranking and automatic network
practice loops remain unsupported. Protocol readiness/final-ordering and native
owner regression fixtures are authored for later execution. Source compilation
does not establish native or socket behavior; execution and formal acceptance
remain deferred by the user within the full active BMS player goal.
