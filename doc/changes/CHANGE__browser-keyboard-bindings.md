# Browser retained keyboard bindings

AC-206 connects editable lane/key choices to the existing Worker and Rust
BrowserGame binding admission. The Window retains bounded select controls with
Unbound and Reset defaults. It validates and clones a live selection before
audio preparation, then uses the same immutable rows for Worker keyPairs,
prepared-lane resolution, the displayed map and KeyboardEvent.code Down/Up
handling. Busy owners lock editing; setup failure and Stop retain the page draft.
Replay ignores live drafts and uses recorded input.

The old default rows and physical IDs remain stable. A frozen bounded catalog
adds known physical code names from the
[W3C vocabulary](https://www.w3.org/TR/uievents-code/), using additive explicit
browser-host source IDs. These are application IDs rather than native OS scan
codes. Escape remains Stop. Unknown/malformed rows, duplicate lanes/bound keys
and missing prepared-lane bindings fail explicitly. Unbound rows add no key
pair. Single-keyboard play needs no device choice. Browser or OS shortcuts can
prevent event delivery, and drafts are not claimed to persist across reload.

No extra per-frame UI owner, clock or judging path is introduced. Capture,
replay and comparison/network identity reuse existing shared logic. Source
fixtures cover boundaries and actual Window/Worker routing, with instrumented
bindings rather than a replacement judge. JavaScript parsing, assertions,
test/application/browser/audio execution, generated bindings, formal review and
QA remain deferred. Rust is unchanged and previous Cargo checks are not
relabeled as JavaScript verification. Goal active; task open/PENDING.
