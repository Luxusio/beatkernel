# Publish replacement output without restarting gameplay

Solo and local-cohort gameplay use one business-owned, statically dispatched
device port for paused output publication. Existing adapters default to no
replacement. Platform selection, native opening and retirement remain adapter
effects; neither pump imports platform output types or chooses backend policy.

Only an acknowledged, committed pause before terminal rendering is eligible.
The adapter must stage all fallible checks before publishing output ownership,
pause state, presentation observer and configuration together. A ready output
retains its pause hold until publication; refusal returns the original ready
owner and hold without changing live clocks. A successful transaction leaves
audio paused, with explicit coordinated resume through the existing pump.

Require a strictly newer matching pause/presentation epoch, the original sample
grid and host domain, genuine accepted presentation and paused render evidence,
and the unchanged frozen playback position. Update output and playback origins
used by subsequent resume reconstruction while preserving the original logical
song origin, transport history and deadline. Finite completion must discard old
output observation brackets while preserving its original frame grid, startup
prefix and finite playback endpoint; rendered/emitted endpoints cannot be reset.

The pump must retain acquired/pending input, keyboard reconciliation, roster and
original IDs, capture, score, gauge, BGM cursor, competition and stop barriers.
No rebuilding the session or per-note dynamic dispatch, allocation or locks.
No native/UI wiring or acoustic accuracy claim follows from a default port.

`GameplayOutputContext` borrows the existing presentation, pause, configuration
and finite-completion owner. `GameplayDevice::publish_paused_output` defaults to
false. `publish_ready_output` consumes the candidate only on successful
publication into an empty output slot; `ReadyPublicationFailure` returns its
original error and complete ready candidate on refusal. The native compatibility
port forwards this operation without adding an OS policy to the pumps.

The context also borrows the actual solo/cohort runtime pause control. Adapters
acquire a hold from that producer only when a replacement is requested, through
the existing cold hold operation. An unrelated queue's hold, an alias of the
runtime, or a hold acquired before initial playback cannot substitute for this
ownership path. A small business enum selects the existing runtime delegation;
the default device hook does not acquire a hold.

Independent fixtures exercise refusal ownership, genuine memory-rendered timing,
finite completion across epochs and both actual pumps. Assertions, hardware,
browser, formal reviews and QA remain deferred. Four sequential compile-only
configurations follow both writer stops. The full player Goal remains active.
