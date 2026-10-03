# Browser bounded audio capacities

AC-208 exposes retained output limits for command queue, voices, pending
commands, maximum render frames and per-render command budget. Live and replay
capture one immutable selection before opening audio. AudioHost receives that
selection; the Worker independently admits min(256, queue capacity) as its
per-owner command batch limit and uses it for the actual game's command query
and returned batch validation. Legacy callers retain the 256-command default.

Sequence correlation and rejected-prefix handling are unchanged. Smaller batches
do not retry commands or grow buffers. Initial BGM must fit the queue before
the armed start, and actual workload or callback size can exceed a chosen limit
and fail explicitly. These controls select application capacities, not native
device buffers or guaranteed browser callback sizes. Existing Rust browser
report limits bound maximum render frames and pending commands to 4096.

Independent fixtures are authored for bounded admission, immutable Window
snapshots, actual Worker live/replay command queries and acknowledgements.
JavaScript parsing, tests, browser/audio execution, generated bindings, formal
review and QA remain deferred. Rust is unchanged, so prior compiler checks do
not validate these JavaScript paths. Full Goal active; task open/PENDING.
