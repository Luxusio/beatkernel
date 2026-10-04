# Native application room routing

An explicit `--mp-room URL` selects the existing multi-host WebTransport room
owner independently of bilateral `--mp-webtransport` and raw QUIC host/join.
Origin and CA trust remain required. Mixed modes, a guessed room role and host
credentials refuse. Native settings expose this option and replace the entire
transport family when overlaying another mode.

`NativeCompetitionNetwork` composes the existing bilateral owner or actual
`NativeRoomCompetition` over its dedicated network thread. Both `LiveCompetition`
and `NativeGroupCompetition` construct it through their existing preparation
paths. Existing platform gameplay/output/cleanup adapters therefore share the
room implementation. Room mode requires an attached graphical player publisher
before acquiring credentials/endpoints; headless launch cannot wait for absent
lobby controls. Room readiness comes from explicit UI actions and the actual
committed schedule, with no automatic Seal/Ready. Qualified room score pages use
the existing player UI bridge rather than a legacy one-peer score projection.

The backend stores natural completion proof initially false. Only the existing
native finite-output/input completion gate or genuine whole-song completion
gate marks it, for solo and complete local cohorts. UI state, `Result::Ok`,
cancellation and wall-time expiration provide no such proof. Final delivery uses
that proof and the actual ordered local prefix; unplayed/cancelled cleanup joins
without fabricating final/drain receipts. Nonblocking request-stop revokes room
authority and signals the network owner; final cleanup retains the joined
outcome and separates network failure from cleanup failure.

The user's browser input clarification is already documented in
`doc/kernel/REQ__bms-browser.md`, under Performance-first browser thread
ownership: Window acquires keyboard, touch/pointer, HID and Gamepad input while
Worker owns mapping, judgment and gameplay rendering. No duplicate architecture
rule or browser rendering loop is introduced by this native routing phase.

Independent deferred fixture source adds six groups: two application routing and
headless/feature admission groups, two native/profile transport replacement
groups, and two genuine common gameplay completion groups. Completion fixtures
use the real native gameplay/Mixer/capture owner and an already-joined failed
room acquisition; they do not fabricate a committed schedule or drain success.
Successful room protocol evidence remains the subject of earlier deferred
owner/controller fixtures. All fixture execution remains pending.

Both writers actually stopped before scoped rustfmt and whitespace inspection.
Workspace/all-targets with WebTransport (session 39510), native no-default
all-targets with WebTransport (72638), WASM browser (4026) and WASM browser-audio
(26848) each exited 0. WASM retained three existing cadence dead-code warnings.
These are compile-only checks. No runtime tests, browser/device/network execution,
formal reviews, QA, verification or task close are authorized in this phase.
Closed Results still retains only its selected score page; an immutable final
score archive is remaining work. Full Goal and Harness task remain open.
