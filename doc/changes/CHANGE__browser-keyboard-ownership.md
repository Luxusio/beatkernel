# Browser keyboard acquisition ownership

The actual Window keyboard handler snapshots original native input fields and
checks its original session ownership after getters and browser callbacks. A
per-session guard discards synchronous nested acquisition and resets in finally.
It validates capacity, acquisition time and sequence before committing pressed
state and an immutable queued event together. Cancellation discards stale input,
and errors from retired ownership cannot stop replacement play. Escape retains
its current-session stop behavior, including preparation and replay; other
replay keyboard events do not enter live acquisition.

Three independent actual Host fixture groups are authored for deferred execution
(119 total, preserving the previous 116). Source
inspection and whitespace checks do not establish browser/device behavior or
performance. Runtime tests, formal review and QA remain deferred; the full
player task remains open.

## Known ceiling

The actual browser/device behavior and performance remain unverified. Full-u64
sequence exhaustion cannot be reached through a bounded run of the public Host
endpoint; these fixtures cover ordinary shared sequences and invalid event
refusal without adding a private session mutation seam. This slice does not
claim an executed exhaustion check.
