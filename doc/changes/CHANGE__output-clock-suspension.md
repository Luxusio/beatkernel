# Bound paused replacement without extrapolating a retired clock

The output owner exposes clock suspension only when no current output is
published and its replacement controller is Waiting. A default-false gameplay
port carries this state, including compatibility forwarding and actual Linux
solo/cohort composition. Queued requests and reply contention do not suspend an
active clock.

Both pumps use the common host validator: at a fully committed nonterminal
Paused cutoff, timestamps at/after that cutoff need no extrapolation from the
retired output. Existing domain, chronology, receipt bounds, merger ordering and
input delivery checks stay active. Earlier input and all active/resuming clocks
retain normal freshness validation. No pair timestamp is rewritten, observation
is fabricated or transport advanced; the replacement controller still owns its
explicit wait deadline and recovery.

Two actual pump regressions with a shorter observation age failed Stale before
the validator integration and passed afterward; their original input/capture/
gauge/roster/PCM/history assertions are reused. Added checks retain active-clock
staleness/domain refusal, and an actual controller timeout test verifies the
original observer/report plus recovered paused Mixer. The full library suite
executed: 1462 passed / 100 failed, all failure names present in the supplied
baseline. Workspace all-targets WebTransport, WASM browser/browser-audio and Windows GNU/macOS
all-target Rust checks exited zero. Cross-checks use C stubs and do not prove
native ABI, callback or device execution. Full native/acoustic acceptance and
independent review/QA remain outstanding.
