# Browser audible replay host

The browser host adds one local recording selection and a separate Play replay
action for the prepared matching chart. Both Window and Worker bound metadata
before the Worker acquires the file once; canonical preparation supplies its
actual seed and section. Replay uses the same samples, audio commands, ACKs,
armed start, output observations and joined Stop cleanup as live gameplay.
Recorded score and pressed state reach the common renderer through genuine
output presentation. Replay accepts no live input or recapture. Its natural
end is labeled Recorded replay ended, preserving interrupted prefixes without
claiming the whole chart completed. Live recording/download remain available.
The intended behavior is documented in
[the browser requirement](../kernel/REQ__bms-browser.md).

Known ceiling: this is source integration with authored deferred host fixtures.
Generated WASM bindings, JavaScript execution, browser/GPU/audio playback,
physical timing, formal review and QA remain unexecuted. Unsupported or missing
output presentation cannot advance replay operations or establish natural
completion; explicit Stop remains available. The task and Goal stay open.
