# Native finite local cohorts

Windows WASAPI shared/exclusive and macOS CoreAudio local 2..64 sessions now accept `--end-ns` with the same immutable audio and logical end as solo sessions. One audio owner and Transport serve independent judges, scores and replay captures. Actual native output/host clock crossing, all acquisition sources drained, a real committed InputMerger frontier at/past the terminal boundary, every member at the logical end and no pending resume are required to finish. A candidate watermark alone cannot complete the session. Earlier inputs remain globally ordered; input at/after the native endpoint cannot generate gameplay. Manual pause/release reconciliation preserves original timestamp metadata and short resume gaps. Finite sessions retain incomplete prefixes and do not force remaining notes into full scores. Unlimited completion and cleanup retain their previous behavior.

Portable parent mode fixtures are updated, and native local tests cover 2/3/4/64 sparse and maximum IDs, all-member progress, backlog/resume/commit guards and actual ordered merger pre-end/equal-end/future input. Existing shared real Mixer/RuntimeGroup/pause/native-end/capture/replay fixtures remain the model composition layer. These tests are authored and source-compiled only; actual hardware, GUI, Raw Input/HID and acoustic acceptance remain deferred.

## Known ceiling

Known ceiling: graphical loops still consume observed positions and can overshoot or leave a restart gap — upgrade when native finite intent and joined owner restart are wired through the UI.
Known ceiling: finite ASIO and network competition remain explicitly rejected — upgrade when their actual presentation and shared participant frontier protocols are implemented.
Known ceiling: initial native observation after an already-presented endpoint lacks a lower interpolation bracket and fails explicitly — upgrade when early actual native observations can be retained.
