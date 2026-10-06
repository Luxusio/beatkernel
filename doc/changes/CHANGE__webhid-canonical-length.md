# Match browser raw HID packets to the core BKPI payload length

The browser encoder used a u32 payload length for RawHidReport, whereas core
BKPI v1 requires a little-endian u64. Core decoding interpreted four payload
bytes as the high length bytes and rejected ordinary numbered reports. For
short or empty reports the packet itself was truncated. This affected the
production Worker input boundary, not only fixture expectations.

encodeRawHidEvent now writes the payload length using setBigUint64 and reserves
all eight bytes. Payload starts at byte 68 for the existing unnumbered provenance
layout and byte 69 for numbered reports. Original source/sequence/timestamp,
backend/report ID metadata and exact payload bytes remain unchanged. Existing
1024-byte acquisition bounds and detached-buffer rejection are retained; no
report ID byte is stripped from a WebHID payload.

JS and Rust independent literal fixtures now describe the same 73-byte numbered
packet. The Rust fixture decodes it with the actual core codec, re-encodes it
exactly and tests truncated packets, invalid tags/domains and byte/payload caps.
Both languages cover zero/nonzero IDs, empty/max-size reports and full-width
source/sequence/time values. Worker integration payload-offset assertions follow
the corrected wire layout. Core admission rules are unchanged.

This is Node application and Rust codec evidence. Browser permissions, physical
HID devices and required formal browser QA remain separate unfinished work.
The broad Harness task remains open with independent review/QA pending.

Verification (2026-10-06):

- JS physical-input tests: 11 passed, 0 failed.
- Actual Rust browser HID codec fixtures: 2 passed, 0 failed (both previously
  failed against the malformed wire layout).
- Full runtime library: 1496 passed, 72 failed versus the preceding 1494/74.
  Exactly the 2 HID failures were resolved; no new failing names.
- Worker tests under Node experimental VM modules: 84 passed, 25 failed. An
  isolated archive of the unmodified preceding HEAD reports the same counts and
  exact failing names; no Worker regression was introduced.
- HID-focused Worker tests: 7 passed, 1 existing failure about touch-setup ordering
  before HID configuration. The mixed numbered/zero-ID canonical blob path passes.

Worker verification initially omitted the required experimental VM flag and
failed during module loading; rerunning with the file's documented command
executed the tests above. The 25 existing Worker failures remain full-Goal work.
