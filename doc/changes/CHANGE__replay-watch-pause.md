# Native Replay Watch pause

Replay Watch now connects F9/Pause to the same native/mixer boundary coordinator
as live play on ALSA, WASAPI shared/exclusive and CoreAudio. Actual output/host
clock pairs grant capability; source-only progress cannot fabricate a host
association. ASIO retains its existing unavailable presentation/pause limitation.

The output owner requests the sole producer's pause, waits for actual native
presentation, commits one frozen recorded prefix and stops idle progression.
Pending and paused phases fence feeder admission and completion. Resume subtracts
the cumulative paused-frame gap and advances only actual recorded operations.
Start/preroll apply once, equal-time operations preserve ordinal order, and
feeder credit stays on the mixer playback grid. Native failures, cancellation,
stop and final physical/core diagnostics retain their previous handling.

Desktop controls use actual owner capability for live play and Watch; pending,
retry, cancellation and cleanup remain fenced. Recorded files are unchanged.
No keyboard, network or recording is acquired, and live F7/F8 bookmarks stay
unavailable during Watch. Optional seconds remains a wall cutoff including pause.

Authored pure model fixtures cover coalesced boundaries, frozen prefixes,
fractional frame rounding, cumulative gaps and atomic overflow/domain errors.
A composed actual Mixer/queue/ReplayPause/ReplayVisual fixture preserves silent
pause blocks, plays a queued sound on its original playback frame after resume,
and matches complete recorded judge results/hash without duplicate operations.
Feeder-grid and UI-capability fixtures cover the integration rules. All fixtures
are compiled only; device, GUI and timing execution remain deferred. ASIO/network
pause, loops, browser integration and full native/GUI/replay acceptance remain open.
