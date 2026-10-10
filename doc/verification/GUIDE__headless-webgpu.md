# Headless WebGPU verification on Linux

Use a real current WASM package and a dedicated loopback server/browser profile.
Software rendering can verify functional presentation; it does not establish
physical GPU performance or native audio latency. Run only one compiler or
heavy browser verification owner at a time. Do not apply compiler virtual
address limits to Node, WebAssembly or Chromium.

## Verified Vulkan software configuration

The 2026-10-10 Chromium probe used these arguments:

```text
--no-sandbox
--enable-unsafe-webgpu
--enable-unsafe-swiftshader
--use-angle=vulkan
--use-vulkan=swiftshader
--enable-features=Vulkan
--disable-vulkan-surface
```

The ANGLE/Vulkan and compositing configuration follows the
[Chrome headless WebGPU guidance](https://developer.chrome.com/blog/supercharge-web-ai-testing).
The software Vulkan driver option follows the
[Chromium SwiftShader documentation](https://chromium.googlesource.com/chromium/src/+/main/docs/gpu/swiftshader.md).
Verify the actual adapter and drawing in the current browser build; accepting
launch options or detecting `navigator.gpu` alone is insufficient.

With current WASM SHA-256
`5d947a3f11cf8cf1589f9210d3f9d9dda0073bac5ac2c0f73221dfa9b4626358`,
the bounded development probe observed a Google/SwiftShader adapter, a healthy
five-second preview, one full-page screenshot, six-second completed play and
results/history draw acknowledgements. It observed no device loss, application error or
SharedImage error. This is development setup evidence; independent browser QA
is recorded separately. Subsequent independent QA found a visually blank
historical canvas even with matching draw acknowledgements and this setup.
Thus the configuration removes the observed device-loss failure but does not
yet establish visible application presentation.

## Diagnose actual presentation failures

An earlier `--use-angle=swiftshader` configuration produced
`Could not find SharedImageBackingFactory` and `Unable to create shared image`
for a WebGPU swap-chain image, followed by device loss. Instrumentation installed
before device creation observed the loss before any screenshot and recorded no
application `GPUDevice.destroy()` or canvas `unconfigure()` call. That evidence
does not prove the browser's exact internal destruction mechanism. The changed
Vulkan configuration repaired the observed failure in the bounded probe.

Retain browser stderr, actual renderer messages, adapter/configuration and
timestamps before changing application code. Keep genuine device loss and
render acknowledgement errors visible. A historical `available=true` response
does not prove a rendered historical screen; wait for matching actual `drawn`
evidence and inspect visible content in the screenshot and page controls.
Use a viewport-only capture after settling geometry and paint to distinguish
actual blank output from full-page screenshot resizing. A draw acknowledgement
alone proves submission, not pixels appearing in the browser compositor.

Each changed setup gets a bounded verification attempt, not an indefinite flag
permutation loop. Own and close the browser/server/profile. Preserve failed
attempt evidence; rerun independent QA after a substantiated setup correction.
Do not relabel development traces as formal QA.

Diagnostic artifacts are ignored and machine-local:
`target/wf/browser-record-result-continuity/gpu-diagnostic` and
`target/wf/browser-record-result-continuity/gpu-vulkan-remediation`.

## Isolate the browser from application rendering

Before changing a scene or renderer to repair blank output, verify a minimal
known-color WebGPU clear with the same browser configuration, independently
of the application. Compare a main-thread canvas with a canvas transferred to
a Worker. Wait for GPU submission completion and inspect the expected colors.

On 2026-10-10 a bounded minimal control under the configuration above submitted
a red main-thread clear and a green Worker clear. Both submissions completed
without device loss or GPU errors, but screenshot center pixels were black
and page-background RGB(17,17,17), respectively. No player or Rust renderer
was loaded. Thus this setup cannot currently supply reliable visible-content
acceptance for the player. GPU texture readback was not performed, so the
precise internal rendering/compositing cause remains unproven. Exit zero
reports diagnostic execution, not visual PASS. The browser, server and profile
were cleaned up. Preserve these controls alongside the original failed QA:
`target/wf/browser-record-result-continuity/minimal-webgpu/{run.mjs,evidence.json,pixel-check.json,viewport.png}`.

## Headed comparison under an owned virtual display

On 2026-10-10 a bounded development comparison at source `0c48454` ran
`/usr/bin/Xvfb -displayfd 3 -screen 0 1024x768x24 -nolisten tcp` and Chromium
with `headless: false`. It retained the Vulkan/SwiftShader configuration above;
the exact merged Chromium launch arguments are in the evidence artifact.
Allocate the display automatically, use a private browser profile and loopback
server, and terminate only processes owned by the comparison.

The direct main-thread red canvas and transferred-Worker green canvas both
completed submission. Copying the actual current textures into aligned GPU
readback buffers produced RGBA `[255,0,0,255]` and `[0,255,0,255]`; viewport
screenshot center samples produced RGB `[255,0,0]` and `[0,255,0]`, respectively.
The screenshot was also visually inspected. Both adapters reported
Google/SwiftShader and `rgba8unorm`; row stride was 1,024 bytes. No application,
device-loss or GPU validation error was observed. Browser/Xvfb stderr remains
available, including unrelated system-service/keymap diagnostics.

`node target/wf/browser-record-result-continuity/headed-known-color-20261010/run.mjs`
exited zero after explicit texture and visible-color assertions. The owned
Chromium and Xvfb exited zero, the server closed and the profile was removed.
The fixture, screenshot, pixel checks and launch/cleanup evidence are under
that ignored directory; regenerate these diagnostics if artifacts are absent.
GPU completion, texture readback and visible presentation remain distinct
observations. This passing headed control supplies a local prerequisite for
connected player QA; it does not establish why the earlier headless control
failed. Rebuild current WASM and perform independent actual record/results
navigation and visible historical-screen QA before accepting that player flow.

### Known ceiling

Known ceiling: Known-color control contains no player/WASM — independent connected browser QA required before AC-004 acceptance.

## Connected headed player observation — 2026-10-10

Independent QA repeated the known-color control successfully before loading
the current player packages. For bounded startup diagnosis, preload the exact
production HTML/JS/MJS/CSS/WASM bytes before launching the owned loopback server
and browser, and log request durations. Keep product initialization deadlines,
clock checks and replay completion criteria unchanged. Artifacts are under
`target/wf/qa-browser-records-headed-20261010/`.

In `timed-preloaded-flow`, preload took 169 ms and each 4,823,503-byte WASM
response completed in 3 ms. CPU readiness arrived approximately 9,586 ms after
initialization, within the product ten-second guard. Actual finite capture,
save/use and visible historical provenance/score with next/back pages passed.
The subsequent replay assertion selected a nonterminal progress notification.
The corrected `timed-preloaded-terminal-flow` requires the current play ID
and `completed === true`, retaining the same 25-second QA observation limit.
It failed the original product initialization guard before play, despite WASM
HTTP responses completing in 2–3 ms. CPU readiness was absent at disposal.

Fast memory-served responses exclude handler file-read latency as the sole
explanation for the latter failure; they do not locate the remaining cause.
Investigate Worker startup and WASM initialization stages before choosing a
fix. No further setup-only retries or timeout extensions establish acceptance.
The full connected browser verdict remains FAIL; replay/local-player acceptance
and hardware/performance claims are not supplied by the passing history subset.
All runs terminated and their owned browser/display/server/profile resources
were cleaned up. Earlier failed headless evidence remains valid for its setup.
