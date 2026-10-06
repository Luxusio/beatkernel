# FLAC metadata body skipping

The preparation contract requires unused FLAC tags and artwork to be skipped.
Claxon 0.4.3 `read_vorbis_comment: false` still parses comment bodies before
throwing them away. Consequently an irrelevant malformed comment rejected
otherwise valid source PCM, and unused comments could allocate tag storage.

The application decoder now checks the entire metadata chain using headers and
bounded slices, then calls the public codec parser only for the first STREAMINFO
block. Remaining bodies are neither parsed nor copied. STREAMINFO must be first
and unique; type 127, duplicate comment blocks, truncated headers and out-of-range
body extents reject. Existing source-format, PCM-budget, chronology and frame CRC
validation remains in place. Unknown reserved metadata types retain skip behavior.

This runs during asset preparation, outside playback callbacks. Codec frame
scratch remains separate from the PCM output cap; no native-device or exhaustive
codec conformance claim is made.

Verification:

- Before the change, focused FLAC tests: 6 passed, 1 failed (unused comment).
- After the change, focused FLAC tests: 8 passed, 0 failed. Added coverage includes
  ignored comment/art/application/reserved bodies and strict chain structure.
- Whole library: 1471 passed, 94 failed. Compared with the preceding viewport
  baseline (1469 passed, 95 failed), there are no new failing test names; the
  comment failure is resolved and one regression test is added.

- Preparation integration: 26 passed, 1 failed, the same pre-existing tagged MP3
  timing test; FLAC preparation cases pass.

- Workspace all-target check with webtransport: exit 0.
- WASM browser library check: exit 0. Existing dead-code warnings remain.

The Harness task remains open. Independent review and required QA receipts,
including browser QA, remain required before task close.
