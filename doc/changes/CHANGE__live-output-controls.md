# Connect paused live output settings to the native owner

A capability-driven live-audio child screen retains Play and sends a bounded,
correlated output request through a statically injected control port. Linux
solo/cohort decode output-only settings on the native owner thread and use the
existing common replacement/publication path. The UI receives applied settings
only after real Ready output and clock publication; admission is pending.

`OutputControls` owns bounded request IDs and settlement independently of
`PlayerViewer`/`PlayerPublisher`. `GameplayOutputUi<U: OutputUiPort>` connects
requests to the existing owner and retains replies through contention.
`NativeAlsaOutputUi` converts the output-only request and projects actual applied
configuration. The `LiveAudio` route retains its Play parent and has its own
retained view, draft, text target and request binding. F2 opens it only from an
acknowledged supported pause whose desired state is still paused. Support and
pending checks use scalars; actual capability/settings copies are cold actions.

The pure command state is separate from PlayerViewer/Publisher synchronization
and native request conversion. Idle checks use a scalar atomic; command locks,
text and geometry allocations remain outside the audio callback. Screen/request
identities are separate so late replies cannot overwrite a different child
draft. Back preserves the same paused Play instance; cancellation and owner
completion settle accepted requests.

Linux preparation raises only the Mixer's scalar legal render bound, while
actual native scratch remains sized to the opened period. Output-only preflight
preserves format/grid and rejects unrelated settings before native retirement.

Independent fixtures: 19 tests in four files (pure/Player channel 7, native
solo/cohort pump and Linux translation 5, Desktop child lifecycle 4, screen
lifecycle 3). The existing IME-area fixture's exhaustive field match gained an
unreachable `LiveOutput` arm; its assertions are unchanged. After scoped
rustfmt, all four compile-only configurations (workspace all-targets with
webtransport, runtime no-default webtransport all-targets, wasm32 browser lib,
wasm32 browser-audio lib) exited 0 with only the pre-existing unused warnings,
and `git diff --check` was clean. This is compile evidence only: no assertion
ran. All assertions, browser/desktop/device execution, formal reviews and
required QA remain deferred. Other OS composition, cross-backend switching,
blocking foreign-call isolation and acoustic/performance acceptance remain
unfinished full-Goal work.
