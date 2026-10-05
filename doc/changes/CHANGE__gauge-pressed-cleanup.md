# Failed gauge pressed feedback cleanup

Native solo/local report publication clears a failed player's display-only
button/contact ownership and pressed lane mask after complete batch validation.
Full replay publication cannot revive a failed mask from a stale nonzero absolute
mask. Later failed reports cannot reacquire owners. Browser Worker solo/local
observation uses the actual stepped owner's gauge and also clears on committed
technical-error prefixes. Recoverable zero and healthy members retain ownership.

ReplayVisual clears its display owners after the actual recorded operation
produces gauge failure, while continuing to reconstruct later legacy recorded
judge operations. Equal targets remain idempotent. Core judge ownership, actual
input/capture prefixes, wire identity and shared audio are unchanged; no release
input or Window gameplay/rendering work is added.

Five independently authored fixture groups cover native solo/local publication,
legacy replay continuation and actual BrowserGame/BrowserLocalGame observation.
They include simultaneous audio/capture failure prefixes, high-width physical
source/contact identity, healthy member ownership, recoverable zero, stale replay
masks and invalid batch atomicity. Both writers delivered terminal Writes STOPPED
before root formatting or compilation. Assertions and browser/device execution
remain deferred. The two browser cfg(test) groups are also not compiled by the
authorized WASM --lib checks; no browser-fixture compilation is claimed.

Scoped formatting and whitespace checks succeeded. Four authorized compile-only
checks exited zero: workspace/all-targets WebTransport, headless/all-targets
WebTransport, WASM browser and WASM browser-audio. Existing unused-code warnings
remain in audio cadence and the playfield progress helper. Native/replay fixture
source was compiled by the host all-targets checks, without executing assertions.

Audio stop/drain, clear/fail
completion and high-level mine admission remain separate unfinished work.
