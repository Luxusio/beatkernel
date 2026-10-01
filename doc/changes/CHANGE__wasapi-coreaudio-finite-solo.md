# WASAPI and CoreAudio finite solo playback

Windows WASAPI shared/exclusive and macOS CoreAudio solo commands accept an unsigned `--end-ns` strictly after `--start-ns`. The checked section/preroll mapping configures the actual PCM endpoint once. The mixer silences the suffix at that exclusive frame, runtime judging caps at the original logical end, and actual native output/host clocks acknowledge presentation. Completion additionally waits for native keyboard collection to drain and pending resume reconciliation. Earlier input retains its original timestamps; terminal input cannot generate gameplay beyond the prefix. Remaining notes are not forced complete, and replay capture retains an incomplete session prefix. Unlimited sessions retain full-song completion and stop/join/capture cleanup remains authoritative.

Portable parser/mapping/frontier and real mixer/pause/end model fixtures are authored for later execution. Source compilation does not establish WASAPI/CoreAudio, GUI, HID, Raw Input or physical/acoustic acceptance. Tests and native execution remain deferred by the user.

## Known ceiling

Known ceiling: finite Windows/macOS local groups, ASIO and network owners remain rejected — upgrade when those owners share immutable endpoints and actual native/drained input acknowledgements.
Known ceiling: graphical practice loops still restart from observed UI positions and can overshoot or leave a gap — upgrade when the native finite intent is passed through owner restart/join lifecycle.
Known ceiling: an initial observation already past the endpoint has no native lower interpolation bracket and fails explicitly — upgrade when initial native observations can be retained before that crossing, without fabricated timestamps.
