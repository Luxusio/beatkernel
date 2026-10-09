# Continuous layer fuzz campaigns

An isolated `fuzz/` package calls actual public production APIs for physical
input/replay/room codecs, BMS parsing and chart compilation. Standard
cargo-fuzz/libFuzzer supplies mutation, coverage guidance, corpus minimization
and failing-input minimization. Existing finite corpus tests remain independent
regressions; their presence does not establish continuous fuzz execution.

## Isolation and oracles

Product Rust1.98.1, production manifests/lockfile/default features and MIT source
remain unchanged. The non-shipping harness has its own workspace and lockfile;
stable tests exercise shared checkers without libFuzzer. Optional `fuzzing`
binaries use nightly2026-10-08, cargo-fuzz0.13.2, libfuzzer-sys0.4.13 and
arbitrary1.4.2. App dependency disables default graphics features. No target
initializes devices, sockets, assets or UI; no validation is disabled under
`cfg(fuzzing)`, and panics/sanitizer findings are never suppressed.

Accepted input/replay decoding must preserve raw floating-point bits through a
canonical encode/decode/re-encode fixed point. Ordinary value equality is
insufficient for NaN and signed zero. Independent known-vector tests include
NaN payloads and both zero signs. Rejected malformed/over-limit data is a
legitimate result, not a fuzz failure. Room typed values and canonical bytes
must round-trip; successful decoding grants no room membership or authority.

BMS uses UTF-8 text with deterministic seeds0 and73, repeated parse/compile
results and explicit chart/BGM/BGA/opacity/invisible/mine compilation. Invalid
UTF-8 is outside this parser's string interface. Accepted overflow/compile
errors remain legitimate. No asset IO or legacy text-decoder coverage is claimed.

Structured compiler inputs frequently reach valid compilation and also include
overflow, negative stops, duplicate IDs/markers, reversed holds and zero
resolution. Checks cover canonical results/errors, source identity/metadata,
ordering and valid permutation/grid properties. Independent constant-tempo
integer expectations supplement repeatability; repeating the same algorithm
alone does not establish correctness.

Structured inputs require the fixed18-byte header. Subsequent scalar generation
uses arbitrary1.4.2's zero-padding behavior; declared metadata byte spans remain
strict. This is a fuzz-generator schema, not a new production chart format.

## Campaign budgets

Input bytes65536 and opaque payload4096; replay bytes65536,64 records and4096
header bytes with nested input limits. Room bytes131072 includes the actual
maximum valid Join frame65808 bytes. BMS text16384 bytes,256 lines,1024 bytes
per line,256 items, resolution65536. Structured compiler bytes4096,64 objects,
16 markers per family and64 metadata bytes per object. These are campaign
budgets, not replacements for public production limits.

One local compiler or fuzz target runs at a time. Normal compiler/test commands
use240s watchdog and8GiB virtual address limit. AddressSanitizer executables
must not inherit that address limit: their shadow layout maps16+TB of virtual
space. Fuzz runs instead use RSS limit1024MiB, per-input timeout2s, explicit
max_len, local10s/CI60s campaign budget and an outer90s watchdog. Build first,
then run without compiler overlap. Timeouts and unavailable tools are incomplete
evidence, never sanitizer PASS. See the [LLVM limitation](https://clang.llvm.org/docs/AddressSanitizer.html#limitations).

## Seeds, failures and continuous execution

Real encoders generate reproducible seeds for every physical input, replay
operation, room message and nested start message family, including raw float
bits/provenance and maximum room Join. BMS/compiler seeds reach documented
successful and refused branches. Seed files use create-new admission and refuse
overwrites/symlink destinations; a diagnosed partial seed directory may remain
after failure. Generated corpora/artifacts are separate from committed source.

Seed generation uses an exclusively new output root and standard create-new
files. It operates in caller-controlled scratch directories; concurrent parent
directory replacement is outside this development tool's contract.

Continuous CI runs five independent targets with fail-fast disabled and preserves
corpora, original failure bytes, target/commit/tool versions/limits/commands and
logs even on failure. A workflow file proves configuration only; an external
campaign is proven by its actual execution results. Original failed bytes must
remain available alongside minimized reproductions.

An additional `harness-probe` feature exposes an isolated controlled-failure
binary solely to verify real artifact capture, reproduction and tmin. It is
excluded from ordinary production-target campaigns and is not a manufactured
production defect. Actual cmin/re-execution of real target corpora and actual
probe reproduction/tmin are required verification, not promises.

WBS13.03 remains W until its original continuous/corpus/minimization scope is
proven. All193 player requirements remain; fuzz counts alone cannot guarantee
SQLite-level reliability or physical/native/full-player completion.
