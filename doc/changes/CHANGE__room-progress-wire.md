# Participant-scoped room progress frames

Extend the shared BKMR wire with upload, participant-labelled peer progress and
exact final acknowledgement identifiers. Preserve the common GroupPrefix schema
and existing admission/clock/start bytes, strict bounds and original u64 domains.

Implementation and four independent deferred wire fixture groups are saved, with
13 groups total and all nine previous groups retained. Both writers stopped
before scoped formatting and whitespace checks; four compile-only Cargo checks
succeeded for workspace/headless WebTransport and WASM browser/browser-audio.
The existing three WASM cadence dead-code warnings remain. No tests,
network/browser/audio/device runtime or review/QA acceptance are claimed. Actual
owner authorization, participant fanout/backpressure, per-recipient application
acknowledgements and native/browser gameplay integration remain required. The
full Goal stays active.
