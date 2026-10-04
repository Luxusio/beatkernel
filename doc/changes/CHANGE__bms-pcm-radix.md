# Full resource namespace in default audio preparation

## Behavior

The shared application exports DEFAULT_BMS_PCM_SAMPLES=3844 for original
prepared PCM identities. Native solo/local, offline and recorded replay callers
use this preset, replacing the original1295 limit. Browser Worker preview,
live/local and replay preparation uses ORIGINAL_PCM_SAMPLES=3844; its Worklet
handoff ceiling PLAY_PCM_SAMPLES=7940 includes the existing4096 selected crossing
BGM suffixes. Rust section preparation already adds that bounded suffix allowance
to caller original limits. No additional tail loop or changed identity/time rule.

A source's nonzero two-character base62 references can name3843 distinct samples;
00 remains empty and unused WAV definitions do not allocate PCM. Numeric high
IDs and total admitted asset count are independent. Canonical asset reuse keeps
existing decode behavior; bank entries remain individually counted. This removes
an old preset count bottleneck without changing64MiB per-asset or256MiB bank
bounds, fixed4096 crossing-voice limit, explicit tighter caller limits or the
kernel's generic65536 asset-count ceiling. No new dependencies or crate.

## Source and compile-only evidence

Independent tests cover actual shared preparation above the old1295 count,
full nonzero namespace and crossing BGM section tails, original keysound IDs,
predecode rejection at explicit low count and independent byte limits. Browser
fixtures check actual preparation call arguments and literal handoff capacities.
Both producer and independent author returned terminal STOPPED before root
checks. Scoped rustfmt covered exactly12 changed Rust paths; diff whitespace
inspection reported no errors. Three new Rust groups exercise real shared
preparation and section selection with tiny fake-decoded memory-backed assets.
Existing browser groups remain28 play-model /97 play-worker /14 worker, with
literal updated preparation arguments,7940 admission and7941 refusal. JavaScript
received source/whitespace inspection only, without a parser or runtime.

Exactly four compile-only checks completed with exit0:

- cargo check --workspace --all-targets --features beatkernel-bms-runtime/webtransport --locked (session8964)
- cargo check -p beatkernel-bms-runtime --no-default-features --features webtransport --all-targets --locked (session14191)
- cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser --locked (session11353)
- cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser-audio --locked (session27934)

The existing three WASM cadence dead-code warnings remain.
No test/JS parser/browser/app/audio/device/generated-binding execution or formal
review/QA is claimed. Full player acceptance remains pending.

The earlier [selectable radix slice](CHANGE__bms-base.md) established parser and
image paths while retaining audio count presets. This follow-up implements the
matching default audio counts; it does not change that earlier evidence tier.
