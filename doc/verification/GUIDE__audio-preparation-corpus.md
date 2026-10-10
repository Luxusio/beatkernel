# Audio preparation corpus

The [preparation contract](../kernel/REQ__bms-preparation.md) governs this finite
WAV/FLAC/Vorbis/MP3 corpus. It exercises existing public content dispatch and
whole BMS preparation; it does not introduce additional codecs or change
accepted source-rate conversion.

## Cases and evidence boundaries

| Case | Required observation |
| --- | --- |
| Valid original assets | Known PCM values/extent and original source format through decoder and preparation |
| Different source rates | Assets coexist in the bank; output format does not force source-rate equality |
| Truncation and declared metadata | Concrete framing/length refusal instead of partial success |
| Large metadata | Valid ignored WAV/FLAC/ID3 bodies and parsed Vorbis comments retain the same PCM; malformed declared extents refuse |
| Later compressed failure | Valid preceding frame/packet followed by codec-stage failure, distinct from structural preflight |
| WAV nonfinite sample | Whole payload preflight rejects nonfinite float PCM before allocating output |
| PCM and channel bounds | Exact byte cap accepts, one byte below refuses; mono expansion charges expanded storage |
| Aggregate and aliases | Per-ID PCM copies charge the whole bank budget, including duplicate resource paths |
| Failed later asset | No returned partial bank and an unchanged subsequent valid preparation |
| Actual file reader | Owned sparse file above64MiB refuses before decoder invocation |

The corpus uses original authored fixture generators, not downloaded recordings.
Malformed compressed payloads must retain the correct container/header shape
needed to reach the claimed codec stage. A generic `is_err()` assertion alone
does not distinguish early structural refusal from later decoding failure.

## Execution

Use the installed project toolchain environment and existing host C compiler
wrapper. Keep one compiler owner; retain process handles and wait for terminal
results before retrying. Build prerequisites are separate from test verdicts.

```sh
source target/toolchain/env.sh
export CC="$PWD/target/toolchain/host-cc.sh"
export AR="$PWD/target/toolchain/host/usr/bin/x86_64-linux-gnu-ar"
cargo test -p beatkernel-bms-runtime --no-default-features --test media_corpus --locked -- --test-threads=1
cargo test -p beatkernel-bms-runtime --no-default-features --test media_allocation --locked -- --test-threads=1 --nocapture
```

Allocation measurements belong to a separate integration binary with a
test-only System allocator and allocation-free thread-local counters. Construct
fixtures before measurement and disable counters through a guard even if an
operation unwinds. Count, total requested bytes and largest request include
successful realloc requests as defined by the test; they are not live-byte
peaks. Report exact measured operations and conservative corpus-specific bounds.
Parsed Vorbis metadata/setup storage is separate from owned PCM, while skipped
metadata need not produce proportional decoder allocations.

No result establishes process RSS, allocator overhead, stack/other-thread usage,
arbitrary malformed codec conformance, abort/OOM recovery, native playback or
physical audio timing. WBS08.14 requires actual independent results for its
named scope; authored tests or compilation alone do not satisfy it.

## Current execution status

Development checks under `TASK__bounded-media-malformed-corpus`, base `ab4bc81`:
the corpus author executed15 tests successfully, then tightened the MP3 error
assertion and reran that exact case successfully. The allocation binary ran all
five tests successfully at the current source. Independent review and QA
were pending at that development checkpoint; WBS08.14 was V and progress89/193.
After Rust2021 normalization, the combined development command executed all20
current tests successfully (15 corpus plus5 allocation, failures0/ignored0),
with original output in `target/wf/media-corpus-01a1249d/combined-development.log`.

Observed allocation requests on this host:

| Operation | WAV total/largest bytes | FLAC total/largest bytes | Vorbis total/largest bytes | MP3 total/largest bytes |
| --- | --- | --- | --- | --- |
| Original mono decode | 4/4 | 8/4 | 7947/2104 | 2304/2304 |
| Whole original preparation | 5754/720 | 5758/720 | 13697/2104 | 8054/2304 |
| Declared PCM refusal | 1/1 | 109/40 | 149/41 | 99/40 |

Growing ignored metadata from16 to131072 bytes leaves decode requests unchanged
for WAV/FLAC/MP3. Growing the parsed Vorbis vendor from16 to8192 bytes increases
total requests from7914 to32474 bytes and largest request from2104 to8208 bytes;
PCM remains192 bytes. This explicitly demonstrates metadata costs outside PCM
storage. Named accepted decode/preparation assertions use conservative request
ceilings, not universal codec limits; the source specifies each ceiling.

The intentional unwind test prints its caught panic and passes after confirming
the measurement guard is disabled. Initial corpus build attempts needed explicit
CC/AR paths; a borrow-check error and three invalid fixture/assertion assumptions
were corrected before author success. Author results exist in exec session
streams only. Allocation development original stdout/stderr is preserved at
`target/wf/media-corpus-01a1249d/ac002/development.log`. A fresh checkout must
execute the commands; machine-local logs are not portable acceptance artifacts.

## Independent acceptance — 2026-10-10

Independent DEEP code review PASS at source `255c400` preceded independent CLI
QA. QA ran both new binaries and the existing preparation integration suite in
one invocation: corpus15 + allocation5 + preparation27 =47 PASS, failures0 and
ignored0, exit0. Rust2021 format, baseline diff whitespace and WBS consistency
checks also passed. Raw logs are in
`target/wf/media-corpus-01a1249d/qa-cli/{cargo.stdout.log,cargo.stderr.log,cargo.exit}`.
Allocation figures above match the independent output exactly.

```sh
cargo test -p beatkernel-bms-runtime --no-default-features --test media_corpus --test media_allocation --test preparation --locked -- --test-threads=1 --nocapture
```

This fulfills the finite software preparation corpus and allocation/byte-budget
scope of WBS08.14; its status is D and overall progress90/193. Existing compiler
warnings remain. Full codec conformance, total memory sandbox, native hardware
and whole player acceptance remain outside this evidence. Independent execution
and receipt-backed Harness closure are separate; absent lifecycle attestation
does not become an invented task PASS.
