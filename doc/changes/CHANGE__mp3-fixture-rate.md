# Correct the original MPEG-2 fixture sample-rate index

The shared original silent MP3 generator promised MPEG-2 at 24,000 Hz and wrote
192-byte 64 kbps frames, but its header byte 0x80 encoded sample-rate index zero
(22,050 Hz). The application correctly computed 208-byte frames from that header
and rejected synchronization at the next boundary. This broke five positive
codec tests and the default tagged MP3 preparation integration.

Use header byte 0x84 (rate index one) for the promised 24,000 Hz fixture. The
variable-bitrate/padding fixture likewise uses 0x96 for its 241-byte, 80 kbps,
24,000 Hz padded frame. Production rate tables, strict frame checks, decoded
limits and tagged timing policies are unchanged; expected PCM/timing assertions
are retained. Malformed frame/tag tests now start from genuinely valid frames.

Focused codec tests: before 1 passed, 5 failed; after 6 passed, 0 failed.
Default preparation integration: 27 passed, 0 failed (previously 26/1).
Full runtime library: 1479 passed, 89 failed (previously 1474/94). The five resolved
failure names are exactly the MP3 positives; no new failing names were found.
This is synthetic codec/application verification, not exhaustive MP3 conformance
or native sound acceptance. The broad task remains open with review/QA pending.

Workspace all-target webtransport check: exit 0; existing library warnings remain.
