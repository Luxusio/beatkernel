# Owned startup profile before renderer initialization

## Preparation and owner boundary

Previously --profile loaded and decoded the bounded file before EventLoop.
Move that CPU operation and the original CLI overlay rules to the existing
NativeCatalog::spawn_prepared owner. Native values use explicit CLI native
overrides excluding chart; presentation uses explicit CLI display overrides.
The literal chart/library/font selection stays outside profile content.

Create the native window immediately with loading status in its title. While
the startup owner is pending, create no renderer, GPU instance, selection/font
job or native gameplay owner. Fence settings/navigation/start; nonrepeat Escape
and OS close cancel while focus, resize, suspend and occlusion remain routed.
The renderer must use the fully prepared backend and presentation, with no
temporary renderer using defaults before actual profile completion.

Real joined success installs complete Options into eligible Selection and then
starts existing selection/font preparation and renderer initialization. Hidden,
suspended or occluded UI retains its owned result; closing consumes/discards.
Errors are fatal and join cleanup instead of continuing with defaults. Final
close/exit accounts for startup too. No-profile startup retains existing paths.

## Evidence and limits

Both producer and independent author returned actual terminal STOPPED finals.
Actual run creates the owned startup profile job; resumed creates the window
and defers the renderer. Source checks confirm collect_startup joins the complete
Options, applies backend/presentation/native overlays, initializes its renderer
and then begins selection. Input/hit/navigation/draw guards cover pending startup;
Escape precedes the usual ui_ready gate. About-to-wait polls this owner without
drawing, close/exit cancel and join it, and eligible unocclusion can initialize a
previously deferred renderer without replacing the window.

Four independent deferred groups were added in desktop_startup_fixtures.rs.
They use actual v1/v2 encoded files and real owned-thread/channel gates for CLI
precedence and literal selection; pending input/GPU/Start and direct/font/library
continuation; hidden/suspended/occluded retention and close/Escape/Drop after
real preparation; missing/foreign/malformed/72 KiB+1/nonregular profile fatal
refusal without default selection. No existing fixture groups were removed.
These fixtures have not been executed and open no window/device in source.

Scoped rustfmt on both changed Rust files exited0. Exactly four compile-only
checks after both terminal STOPPED finals exited0: workspace all-targets with
WebTransport (43600), headless all-targets WebTransport (6423), WASM browser
library (7356) and WASM browser-audio library (20487). Existing three WASM
cadence dead-code warnings remain. Source whitespace inspection reported no
diagnostics. Compilation is not evidence of assertion outcomes or native
window/GPU behavior.

Cancellation
is cooperative and cannot forcibly interrupt an OS filesystem call or GPU
initialization. Actual native window loading appearance, profile I/O timing,
renderer choice, close/restart and device behavior require later acceptance.
Scoped formatting and four compile-only checks follow both terminal STOPPED
finals; tests/assertions, app/device/runtime and formal review/QA remain deferred.
The full BMS player Goal/task stays open.
