# Browser Worker preparation fixtures

Worker result/history/archive tests must reach successful preparation before
asserting behavior owned by gameplay or recording. The fake generated-WASM
library follows the current production preparation surface; the real Worker,
numeric helpers and renderer/room ownership boundaries remain under test.

## Required preparation seam

1. Declare file paths and exact byte lengths without eagerly reading them.
2. Hydrate a declared file only when the Worker acquires its bytes. Check
   declared length, library lifetime and single hydration.
3. Begin the appropriate incremental chart, section, policy or replay job.
   Preserve each fixture's constructor argument mapping and replay bytes.
4. Advance through at least one not-ready turn, then readiness. Deliver the
   continuation on a queued `MessageChannel` host task, independently of
   Promise microtasks and deliberately held renderer/room ACKs.
5. Take the prepared owner once. Preserve cancellation, job/library cleanup
   and existing no-resurrection/late-transfer assertions.

A real incremental job can report the same pending asset path across processing
stages after its bytes have been supplied. Reuse acquired bytes while advancing
the job; do not require pending paths to be unique. At Ready, assert each
required asset was hydrated exactly once. This keeps genuine binding tests
aligned with production without weakening the declaration or length checks.

`app/web/play-worker.test.mjs` supplies the existing queued-task pattern.
Its flush tracks both queued tasks and a set of reserved delivery promises.
Concurrent flushers must join deliveries already removed from the queue;
otherwise setup can appear finished while another flusher still owns work.
Delivery drains microtasks without recursively awaiting its own reservation.
Both success and rejection remove the exact reservation, and rejection stays
visible to waiting flushers.

## Meaningful assertions

Successful import, accepted library, selection and preparation are explicit
preconditions. Keep original capture bytes, finalization, refusal, archive
ordering, member identity, stale transfer and free-count assertions. Do not
silence failures by weakening these assertions or by making preparation
synchronous. Preparation progress alone does not prove result completion.

Sample transfer fixtures must expose the current generated
`BrowserSample.take_pcm_array()` surface, retaining one-shot consumption,
original PCM contents and cleanup counters. An old `take_pcm()` fake can hide
the actual transfer refusal behind a later missing sample result; assert the
sample RPC error and result before using its kind.

Binding tests with actual generated WASM complement these Worker fixtures;
actual finite recording/save/comparison still requires interactive browser
acceptance. Node fixture passes do not prove GPU, physical input or acoustic
timing. Commands and observed results belong in the task change ledger.
