# Saved recording section compatibility

## Observable behavior

A recording made during a finite section must be usable as a saved opponent
when the player selects that same section again. Solo and local multiplayer
use the original chart coordinates, start, configured end, chart seed, input
mode, selected timing/judgment/gauge policy and original member identity.
Open-ended recordings likewise match open-ended setup.

A different finite end, or a finite/open mismatch, must continue to produce
the existing typed `IncompatibleSetup` refusal. Refusal must preserve already
accepted opponents, charged bytes, replay prefix, current score and usable
selection. Existing bounded replay decoding and shared local quotas remain
mandatory; full-width player IDs must not be truncated.

## Header ownership and compatibility

The saved-comparison header must equal the header decoded from genuine
`configure_capture` / `take_replay` bytes for the same pristine owner.
Querying this metadata must neither enable or consume recording nor process
judgment. Existing pristine, member, limits and failed-owner gates remain.

The public `competition_header` remains the normalized network identity basis:
its section end stays absent because finite competition carries that end in
the separate network envelope. Network identity bytes and the rejection of
double-carried endpoints remain unchanged. Saved comparisons use a separate
crate-visible recording projection of the existing canonical header builder.
No replay schema migration or relaxed compatibility comparison is required.

## Verification

- Rust Step regressions compare genuine decoded captures and literal metadata
  for solo/local, finite/open, policy/default and button/contact setups.
- Genuine-byte admission tests check matching setup and atomic mismatches
  after an existing accepted opponent.
- Generated WASM binding tests exercise both browser adapters, original member
  IDs and the shared eight-record quota.
- Actual browser acceptance completes finite recording, saving, same-section
  own comparison, history/replay and normal navigation; a stopped prefix test
  alone is insufficient evidence of completion.
- Existing finite/contact network identity regressions remain unchanged.

Implemented in `7fb6a5a`, with fixture coverage in `1282130`. Independent code,
CLI and interactive browser acceptance passed for
`TASK__player-record-worker-regressions`, including finite Own and Other
completion, persistence, mismatch recovery and replay. Task closure remains
subject to Harness-owned attestation. This requirement does not establish
whole-player completion or physical audio/input acceptance.
