# Cross-platform solo pause

Windows WASAPI shared/exclusive and macOS CoreAudio solo sessions now join the
Linux ALSA F9 pause flow. Non-network live owners announce capability only after
native setup. They use the common presentation-boundary model, freeze/resume
Transport, schedule keysounds on the original playback grid, reconcile paused
key levels and rebuild discipline from the cumulative paused-frame gap.

Windows keeps pumping native messages during pending acknowledgements and
stores at most65536 Raw Input events before judge admission. Original normalized
timestamps are QPC acquisition receipts, not hardware key times. Foreground
packet cleanup, close messages and device changes retain their native handling.
Message backlog fences automatic judge advancement. macOS retains original
normalized HID timestamps and parks at most4096 post-resume events until the
collector is empty; synthesized releases precede those events. Overflow and
chronology/native failures terminate with existing stop/join/close handling and
the valid capture prefix. Boundary interpolation remains physically Unknown.

Authored fixtures cover exact pause boundaries, host-domain rejection, bounded
HID parking and preserved provenance. A portable composition fixture connects
the real command queue, Mixer, SoloRuntime, Transport, NativePause, PauseKeyboard,
LiveReplayCapture and ReplaySession: paused PCM is silent, playback frames freeze,
resume releases keep provenance, later keysound samples use the playback grid,
and captured replay reconstructs the same judge state. These fixtures are
compiled only under the user's deferred execution policy; they do not establish
native or all-product acceptance.

ASIO, local groups, network competition and replay Watch still expose no pause
capability. Local-cohort shared pause, competition policy, replay controls,
practice loops, browser integration and native/GUI/replay acceptance remain open.
