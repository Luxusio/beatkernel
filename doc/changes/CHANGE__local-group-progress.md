# Whole-cohort progress snapshots

The shared local gameplay owner exposes its actual ordered member progress,
including independent song frontiers and cumulative counts. A browser control
snapshot serializes exact eleven-word rows without sampling a clock or changing
judgment, audio, transport or capture. Retained prefixes remain observable
after failure. Snapshot allocation occurs on the control path, outside input
and audio callbacks.

The portable payload codec validates one to 64 positive unique PlayerIds,
full-width times/counters, an exact expected sequence, fixed roster order and
valid monotonic member prefixes. Every row is validated before a group can be
accepted; one malformed member rejects the whole payload. The bounded schema
contains a final-prefix flag and fits within 2828 bytes at the maximum roster.
The common scalar progress validator supplies score semantics.

The codec owns payload bytes only. Existing BKMP v6 framing/session, clock/start
agreement and complete-write/final-ACK ownership remain scalar. Group setup
and remote roster negotiation, framed group transport, one shared start/output
mapping, target HUD dispatch and Window/native callers remain unfinished.
No payload flag is evidence of completed writing or acknowledgement, and the
browser page still refuses local network combinations.

Deferred fixtures and scoped compilation are source evidence. They do not
execute generated WASM bindings or establish device/network acceptance,
measured latency, formal review/QA or full-player completion.

Four deferred groups were added: three in multiplayer_group_fixtures and one
in browser_local_saved_fixtures (now four groups total). The codec fixtures use
independent literal bytes/words and cover 64-member limits and whole-payload
refusal. The runtime fixture reads genuine independent member frontiers before
and after a chronology failure and compares retained hashes and exported member
capture bytes against an identical actual cohort.

Scoped Rust formatting and whitespace checks completed. Workspace all-targets,
headless runtime all-targets, browser WASM library and browser-audio WASM library
Cargo checks each completed with exit code zero. Three existing platform
cadence dead-code warnings remain in the WASM checks. No assertions, generated
binding calls, browser/native application or network scenario were executed.
