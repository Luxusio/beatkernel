# Graphical recorded playback

Records Preview now offers Watch in the single winit/wgpu app, with recorded
native audio and the existing lane/score view. Actual ordered judging operations
advance from reported native presentation, preserve the recorded practice start
and stop at the recorded prefix. Watching projects only output settings, resolves
missing output defaults without keyboard discovery, leaves accepted live settings
unchanged, and supports cancellation and cleanup-gated retry of the same record.
Optional seconds truncates diagnostically; omission drains operations/commands/PCM
through native presentation. Source fixtures are authored/compiled only; actual
GUI/audio/input/file acceptance and independent review/QA remain deferred.

## Known ceiling

ASIO recorded output has no validated output-zero epoch association, so omitted
seconds rejects before native resources and explicit seconds provides diagnostic
audio without visual progress. Other backends wait for usable reported
presentation; no render/UI/wall-clock fallback is invented. Existing replay
codec/PCM/queue/voice limits and native late/configuration errors still apply.
Source compatibility does not establish acoustic timing or complete the player.
