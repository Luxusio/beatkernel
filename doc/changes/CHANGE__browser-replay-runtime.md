# Browser replay runtime components

The BMS application now has a portable nonblocking `StepReplay` owner and WASM
preparation/render bindings for canonical recorded playback. Replay reuses the
existing visual owner, audio plan, rolling feeder and actual output completion;
live and replay share ACK and output-evidence validation. Only genuine output
presentation advances recorded operations. Prefixes retain their recorded end,
section and seed, with no synthesized chart completion or BGM after that end.
One bounded immutable outgoing batch remains held until admission ACK.

`BrowserLibrary.prepare_replay_chart`, `BrowserReplay` and
`BrowserView.draw_replay` reuse selected assets, the shared sample/command ABI
and common renderer. Live pressed highlights now use the same canonical BMS
lane bits as replay, fixing sparse/player-two lane mismatch. Intended behavior
lives in [the browser requirement](../kernel/REQ__bms-browser.md).

Window/Worker file selection and audible launch were subsequently connected in
[the replay host slice](CHANGE__browser-replay-host.md).

Known ceiling: authored fixtures and compile-only checks do not establish
executed playback, generated binding behavior or physical audio timing.
Runtime assertions, browser/audio/hardware verification and formal review/QA
remain deferred; the task and Goal stay open.
