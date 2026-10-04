# BMS selectable resource radix

## Intent and boundary

The player loads BMS resources using selected #BASE16/36/62, default36. A late
selected header applies to the whole selected file, not just following lines.
Seeded conditional evaluation occurs once before radix discovery, preserving
original physical caps and line diagnostics. Reject/LastWins is the existing
strict duplicate policy. Invalid selected declarations fail; discarded payload
cannot alter the radix. BASE is parser configuration, not a new runtime field.

Directive names remain case-insensitive. Resource suffixes preserve case and
base62 distinguishes 0A=10, 0a=36 and zz=3843. WAV/BMP/indexed BPM/STOP, LNOBJ,
BGM/notes/long notes/tempo/STOP/visual references and BGA destinations/sources
share that radix. Direct BPM03 and opacity0B..0E stay hexadecimal. Two-character
references still use00 as empty; BMP00 retains initial Poor behavior. Crop
sources retain the explicit one-or-two-digit policy, not historical decimal
source indexing. Generic crop identities expand to3843 without changing signed
coordinate validation. Image preparation admits the matching3844-identity
namespace while byte/dimension/decode/GPU concurrency limits stay bounded.
Audio asset counts remain separately configurable preparation limits; radix
support does not promise unlimited simultaneously decoded assets.

## Compatibility references

The primary reference implementation discovers file-wide radix before decoding
indexed headers/arrays in its
[BMS Library loader](https://github.com/j-son3/bms-library/blob/master/src/com/lmt/lib/bms/BmsLoader.java).
Its [integer conversion definitions](https://github.com/j-son3/bms-library/blob/master/src/com/lmt/lib/bms/BmsInt.java)
specify the digit/uppercase/lowercase ordering. These are compatibility
references, not universal BMS standards. Our bounded parser implementation is
independently written and remains MIT; no new dependency or copied source.

## Source and compile-only evidence

Producer and independent author returned terminal STOPPED before root
verification. Scoped rustfmt covered exactly six changed Rust paths; source
whitespace inspection reported no errors. Eight deferred groups are authored:
six adapter groups with literal IDs/timings/physical lines, plus two application
groups using the actual memory-backed ImageAssets::prepare_from_source path.
These include high identities3840..3843, BMP00, distinct0A/0a, decoded crop/layer
results and unchanged reference/encoded/decoded storage bounds.

The following four compile-only checks completed with exit0:

- cargo check --workspace --all-targets --features beatkernel-bms-runtime/webtransport --locked (session13253)
- cargo check -p beatkernel-bms-runtime --no-default-features --features webtransport --all-targets --locked (session27823)
- cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser --locked (session13949)
- cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser-audio --locked (session14607)

Existing three WASM cadence dead-code warnings remain. No tests/assertions,
apps, browser/generated bindings, native assets/devices/audio/GPU, or formal
review/QA were executed; compatibility/runtime/performance acceptance remains
deferred. The full BMS player task remains open; this slice establishes no
runtime PASS or percentage completion.
