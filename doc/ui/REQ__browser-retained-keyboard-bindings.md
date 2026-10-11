# Retained browser keyboard bindings

Status: implemented with independent CLI and functional browser evidence;
strict browser acceptance remains unfinished because the audio-WASM request
records `net::ERR_ABORTED`. See the
[acceptance record](../changes/2026-10-11-browser-retained-keyboard-bindings.md).
This status does not waive any verification requirement below.

The Worker-owned Settings screen exposes the existing eighteen keyboard lanes
after the sixteen scalar/policy fields. Indices 16–33 identify lanes
11,12,13,14,15,16,17,18,19,21,22,23,24,25,26,27,28,29 (hexadecimal), in that
order. Labels identify lanes and hints explain physical `KeyboardEvent.code`;
empty text means unbound. All rows remain reachable through retained paging,
ordinary keyboard/pointer navigation and the existing accessible editor.
No virtual DOM or per-frame binding conversion is introduced.

Initial acquisition publishes bounded raw strings, including unfinished scalar
values and duplicate bindings, so the user can repair them. Apply validates the
whole 34-field draft before any scalar, policy or binding effect/mirror write.
Unsupported codes, Escape, duplicate nonempty codes, partial shapes and bounds
violations refuse the transaction and preserve the draft and accepted bindings.
Errors identify available lane/index/code context. Editing alone never changes
live input. Thirteen- and sixteen-field legacy Apply preserve existing bindings.

While any Settings ancestor owns a draft, original DOM binding selectors and
reset are disabled and reset gestures are refused. Ancestry transitions refresh
these controls; leaving that owner restores ordinary standalone acquisition.
Pending profile operations retain their existing lock. Accepted bindings are
snapshotted once before playback; gameplay input performs no form reads, sort
or conversion. Empty bindings receive no guessed fallback.

Practice inherits 13/16/34 fields; Done changes only section indices 11/12.
Players/Devices navigation preserves the parent's bindings and selection.
Full profiles retain version 1 and eighteen explicit validated rows, with
canonical ordering. Retained profile admission keeps the existing T0/T1/T2
identity and revision fences. A 16-field replacement of a 34-field parent is
refused atomically even when selection is before row16; legacy parents still
accept their compatible replacement. Stale or partial receipts cannot publish
keys. Post-admission uncertainty uses existing joined retirement and raw
restoration; cancelled/disposed owners cannot revive a candidate.

Verification must cover literal indices/key IDs, invalid final-row atomicity,
legacy compatibility, late-row selection, child returns, profile cancellation
and ownership restoration. Real browser acceptance additionally edits lane11
to KeyA (ID19), observes prepared [17,19], proves old KeyZ does not acquire an
event for that lane, and observes actual KeyA Down/Up, nonzero judgment and those
inputs in captured replay bytes. Playing that capture must be independent of
later live binding edits. Separate row33 editing proves late-page reachability.
Miss-only completion or DTO roundtrip does not prove working input.

Strict page, console, network and audio checks remain required. The previously
unresolved audio-WASM cancellation diagnostic remains unfinished; this feature
does not waive it. Rebuild matching WASM/proxies after Rust changes or rollback.
Physical OS IME, screen-reader completeness and device/GPU latency require
separate evidence; no performance benchmark is claimed here.
