# Windows and macOS shared local pause

Windows WASAPI shared/exclusive and macOS CoreAudio local 2..64-player sessions
now join Linux ALSA's shared F9 pause flow. Each owner uses one native boundary
coordinator, Transport and audio producer, with independent per-device key levels
and per-player judge, score, capture and ghost comparison. Sparse player IDs and
u32::MAX still map through their freshly resolved keyboard attachment identities.

Windows continues its Raw Input message pump during pending acknowledgements.
The existing bounded 65536-entry InputMerger parks original QPC receipt events;
close, attachment changes and foreground packet cleanup retain native handling.
macOS continues HID polling and attachment/loss checks while pending, retaining
original normalized native timestamps until collector admission resumes. All
sources must drain before pause commits one shared judge boundary. Paused idle
periods add no scoring inputs, deadline commits or repeated capture records.

Resume waits for the configured lag frontier to reach the acknowledged native
boundary, drains the globally ordered paused prefix, reconciles releases through
the actual RuntimeGroup and capture path, then admits original post-boundary
events. A short pause cannot trip a spurious merger frontier regression while
lag catches up. Keysound scheduling uses validated mixer playback reports,
discipline uses the cumulative paused-frame gap, and completion waits for input
reconciliation. Partial errors retain completed member reports before whole-group
native cleanup and replay save attempts.

Platform fixtures cover pending Windows receipt order/native provenance,
paused-new-key suppression, immediate pause/short-resume lag, actual merger
regression, and macOS fresh registry/device ownership for sparse/max player IDs.
Existing shared composition fixtures cover 2/3/4/64 real RuntimeGroup/Mixer/
InputMerger/capture/replay pipelines. Fixtures are authored and compiled only;
native audio, device, GUI and timing acceptance remains unexecuted.

ASIO still advertises no pause capability and retains its existing validated
presentation constraints. Network and replay Watch pause policies, loops,
browser integration and full native/GUI/replay acceptance remain required work.
