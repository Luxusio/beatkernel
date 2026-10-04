# Browser mouse and pen acquisition

The page gains optional mouse/pen input and event-driven button-to-lane choices.
An owned Pointer Events component captures original samples and mask transitions
from the canvas, then the page forwards admitted data through the existing
bounded mixed queue. Worker setup and common physical input processing remain
authoritative. Solo automatically uses its configured aggregate channels; local
players select distinct sources through the existing roster. The contract is
[REQ__bms-browser.md](../kernel/REQ__bms-browser.md#window-mouse-and-pen-acquisition-and-launch).

Mouse and pen source numbers use the same session allocator as HID/Gamepad.
The page snapshots configured rows before launch and verifies the actual
prepared admission before forwarding. Original timestamps and native pointer
codes survive; mask edges become genuine Pointer Button events. Held masks are
aggregated per type so one pointer cannot release another pointer's held button.
Capture loss and cancellation release tracked buttons once. Close detaches the
owner without publishing synthetic events. Fatal cleanup follows existing
ownership rules. Main performs acquisition and event-driven setup DOM work;
Worker owns encoding, interpretation, judgment, replay and gameplay rendering.

The [W3C Pointer Events specification](https://www.w3.org/TR/pointerevents3/)
defines buttons mask ordering, chorded pointer changes and implicit capture
release. The implementation uses those acquisition semantics rather than
compatibility mouse events or a keyboard disguise.

## Source evidence and deferred checks

The actual `pointer-input.mjs` owner acquires dispatched positions and aggregate
button transitions. `main.js` supplies the shared session allocators, snapshots
36 fields, sends numeric Worker setup, checks exact admission and appends a
complete acquired prefix before pumping the existing queue. Discovery retains
and transfers ownership; stop/release paths detach and join cleanup. `index.html`
exposes optional input and event-driven controls. Existing `data-touch-input`
styling suppresses direct-manipulation gestures, independently of actual touch
admission. The current live canvas also suppresses its native context menu so
mapped right/barrel input does not open a menu over gameplay.

Independent deferred fixtures add four component groups and three Host groups.
The actual module is linked in `play-host.test.mjs`, whose 111 existing groups
remain intact for a total of 114. Component coverage includes mask/chord ordering,
aggregated pens, once-only release, full identities, the 33-DTO/64-held-pointer
limits, source/sequence/native failure and old callback cleanup. Host fixtures
cover actual setup/queue/local subset, frozen choices, admission refusal,
1024-pending capacity, chronology, replay exclusion, lifecycle cleanup and
context-menu ownership including canvas replacement. Scripted APIs/Worker
replies remain substitutes, not actual browser or judgment evidence.

Both writing lanes, including the scoped context-menu follow-up, reached actual
terminal STOPPED before final source inspection and `git diff --check` (exit 0).
This JavaScript-only slice ran no tests, Node/parser, Cargo checks, browser/app/
device/network operation, formal review or QA. Source group counts are 4 and
114; execution is deferred. Full Goal/task remain active and required browser
QA still precedes eventual close.

## Known ceiling

The two Window channels aggregate mouse and pen types. They do not identify
individual physical mice/pens or prove that hardware for each type exists.
Pointer acquisition in this slice captures dispatched absolute samples; retained
coalesced histories, pointer-lock relative acquisition, pressure and tilt remain
additional work. Browser/device execution, OS behavior, actual replay/competition
results and measured latency remain deferred. Portable settings version 1 keeps
its explicit keyboard-only binding schema. Source fixtures cannot establish
full player completion or browser performance acceptance.
