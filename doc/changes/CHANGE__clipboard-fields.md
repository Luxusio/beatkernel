# Shared desktop text clipboard

Search, native settings, profile paths, display values, practice start/end and
record directories share Ctrl+C/X/V on Windows/Linux and Command+C/X/V on macOS.
Logical shortcut characters are consumed before ordinary text insertion.
Additional modifiers are excluded; holding a shortcut does not resubmit work.
Active IME composition keeps keyboard ownership. Copy/cut without a selection
does nothing. These controls edit the existing drafts; Apply/Done retain their
existing roles and copying a path does not access its file.

The portable clipboard transaction owns an immutable editor snapshot. Copy
preserves the exact selected UTF-8 bytes; cut prepares a deletion privately and
publishes it only after successful clipboard write. Settings candidates pass
the current model's byte/control/aggregate limits before publication. Paste
replaces selection without trimming, normalization or implicit newline removal.
Backend, payload or model errors preserve text, cursor and selection and appear
in the existing error region.

The native adapter uses one lazy, persistent worker and capacity-one request
and response channels. It accepts one operation until its reply is collected.
arboard creates, accesses and destroys the native clipboard on that worker.
UI, audio and game threads do not call the OS clipboard. A pending editor change
is cancelled when its screen instance, field, readiness, composition or baseline
editor changes. A later return to the original context cannot revive a cancelled
operation. Submitted clipboard writes may still finish after navigation, but
never delete or paste into a replacement draft. Cancelled replies must drain
before the next request.

While busy, the event loop polls completion at up to 4 ms intervals while
preserving gameplay frame deadlines. Idle menus retain event-driven waiting.
Accepted results update the existing retained field and supplied-font cache.
Navigation and suspension retain native clipboard ownership. Closing stops
submissions, drains the worker and waits for its native owner to be destroyed;
unexpected exit uses the worker's joining destructor.

## Limits and deferred acceptance

- Plain text only. Replies above 4096 UTF-8 bytes are rejected, followed by the
  target's smaller byte limit. The native backend allocates its String before
  that check, so arbitrary external data is not bounded at initial allocation.
- arboard has no hard timeout for these operations. Slow or stalled native calls
  do not occupy the UI thread during operation, but can delay normal close or
  an unexpected-exit join. Timing and native availability remain unverified.
- Linux clipboard ownership stays with the application. Persistence after
  exit depends on the desktop's clipboard manager. Pure Wayland requires the
  supported data-control protocols; X11/XWayland is the other backend path.
  These limits follow the pinned [arboard 3.6.1 documentation](https://docs.rs/crate/arboard/3.6.1/source/README.md).
- Browser clipboard integration, mouse/word/grapheme selection and broader IME
  dialog support remain separate work. Existing scalar editing and font shaping
  limitations remain.

arboard is optional under `desktop`, with image support disabled. The core and
BMS adapter have no clipboard dependency. Newly resolved dependency notices and
exact upstream provenance are in the [application notices](../../app/THIRD_PARTY_NOTICES.md).

Portable transaction, fake-worker and actual desktop routing fixtures are
authored for later execution. They cover selected UTF-8 replacement, private
cut staging, errors and bounds, stale contexts, busy/drain and worker lifetime.
Source compilation is tracked separately; no test, native clipboard, GUI,
device, independent review or QA execution is claimed. The full player Goal and
Harness acceptance remain open.

## Source checks

All five locked checks exited 0 on the first attempt on 2026-10-02:

| Configuration | Command |
| --- | --- |
| Linux host workspace and fixtures | `cargo check --workspace --all-targets --locked` |
| Windows GNU app and fixtures | `cargo check -p beatkernel-bms-runtime --all-targets --locked --target x86_64-pc-windows-gnu` |
| macOS app and fixtures | `cargo check -p beatkernel-bms-runtime --all-targets --locked --target x86_64-apple-darwin` |
| Headless app | `cargo check -p beatkernel-bms-runtime --all-targets --locked --no-default-features` |
| WASM graphics library | `cargo check -p beatkernel-bms-runtime --lib --locked --target wasm32-unknown-unknown --no-default-features --features graphics` |

Local evidence is in `target/ac165-{host,windows,macos,headless,wasm}.log` and
matching `.exit` files. Scoped formatting and diff whitespace checks exited 0.
Existing macOS `block` future-incompatibility and WASM native-cadence warnings
remain. These results establish compilation only, including fixture bodies on
native targets; they do not establish linking, ASIO SDK coverage, clipboard
availability, runtime correctness or performance.
