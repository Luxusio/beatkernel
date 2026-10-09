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
