# Audio preparation malformed-file and allocation corpus

The new cross-codec integration corpus verifies WAV/FLAC/Vorbis/MP3 through
public decoding, memory-backed BMS preparation and actual filesystem loading.
Cases cover valid source-rate differences, exact and insufficient PCM bounds,
channel expansion, aggregate/alias charges, large metadata, truncation and
later decoder failures after valid preceding data. A failed later asset returns
no partial PreparedBms and subsequent valid preparation retains its original
result. Actual sparse oversized input refuses before decoder invocation.

A separate integration binary measures successful allocation request count,
total bytes and largest request on the operation's thread. Fixtures are created
before measurement; allocator hooks forward to System and use scalar TLS only.
An unwind guard disables instrumentation even after a caught failure. Accepted
decodes/preparations and early declared-cap refusals are measured for all four
codecs. Large ignored metadata does not scale decoder allocations, while parsed
Vorbis metadata has an explicitly measured separate storage cost.

[The preparation contract](../kernel/REQ__bms-preparation.md) and
[reproduction/evidence guide](../verification/GUIDE__audio-preparation-corpus.md)
preserve those exact limits. Stale single-stream-only Vorbis prose is reconciled
with the existing same-format chained-stream implementation. Production code,
codec policies and dependencies are unchanged.

Development evidence: corpus15/0 then final strengthened MP3 case1/0; allocation
5/0 with original log preserved. The final combined development run after format
normalization passes20/0/0ignored with original log preserved. Final independent
review and QA remain pending.
WBS08.14 remains V; total89/193. Neither these request statistics nor authored
tests establish peak RAM, arbitrary codec conformance, hardware audio, whole
player completion or receipt-backed Harness closure.
