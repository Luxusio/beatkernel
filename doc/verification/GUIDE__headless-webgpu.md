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

## Bounded Worker startup stage diagnostic — 2026-10-10

A single development probe at `275255c` instrumented only server-delivered
Worker entry responses. Production files and generated main/audio packages
remained unchanged. Fixed-cap diagnostic messages did not clear readiness
guards; wrappers returned original fetch/instantiation promises. Both Workers
became ready under the original guards: gameplay in 9,467.3 ms and renderer
in 4,587.4 ms after construction.

Gameplay fetch-to-response headers took 8,216.5 ms; renderer took 3,968.4 ms.
Streaming instantiation took 21.6/22.9 ms and renderer view creation 18.2 ms.
Streaming time includes body transfer, validation, compilation and
instantiation; it is not compile-only evidence. Gameplay's request reached
the server about 8,214.7 ms after fetch invocation, then handling took 2 ms.
Thus response acquisition dominated this observed run. Browser scheduling or
queueing underneath that delay remains unmeasured, and the earlier failed
run's cause remains unproven. Do not introduce compilation sharing or extend
deadlines on the assumption that duplicate compilation caused the timeout.

Worker entry markers execute after static imports, so constructor-to-entry
includes module acquisition, dependency evaluation and scheduling. The probe
is diagnostic evidence, not connected acceptance. Inspect
`target/wf/browser-wasm-startup-stages-20261010/{run.mjs,evidence.json,stage-summary.json}`
for transformations, original/served hashes, timing and cleanup. Node exited
zero; owned browser/display/server/profile resources were cleaned up.

### Known ceiling

Known ceiling: Browser network scheduling cause unmeasured — upgrade when a bounded network trace is authorized

## HTTP cache trace — 2026-10-10

A subsequent single startup diagnostic at `5a9861c` recorded Chromium's
default NetLog without Worker CDP attachment. Game Worker request 294 received
HTTP 200 headers at tick 143753307, then spent 176 ms in
`HTTP_CACHE_DOOM_ENTRY` and 8,230 ms in `HTTP_CACHE_CREATE_ENTRY` before response
delegation at tick 143761713. Proxy selection was DIRECT, a socket was reused
immediately, and the local response handler finished in 2 ms. Streaming
instantiation took 21.7 ms. This identifies a cache interval in this run;
it does not locate the previous run's pre-server delay or establish one cause
for every failed initialization.

The next bounded setup comparison may use a unique owned cache directory on
`/dev/shm` with `--disk-cache-dir` and a 16 MiB `--disk-cache-size` budget.
Check available tmpfs capacity, preserve the original browser profile location,
packages and product deadlines, and remove only the comparison's own cache
directory after process exit. Chromium defines the cache-directory switch in
its [official switch declarations](https://github.com/chromium/chromium/blob/main/chrome/common/chrome_switches.h).
The startup-only comparison below verifies this local setup; it does not
change application behavior or establish connected acceptance.

Exact mapped events and the report are under
`target/wf/browser-wasm-startup-network-20261010/`. Node exited zero; owned
Chromium/Xvfb exited, the server closed and the profile was removed. Startup
diagnosis remains separate from connected acceptance.

### Known ceiling

Known ceiling: prior pre-server delay and renderer post-header delivery delay are not uniquely localized — upgrade when their own event evidence identifies the interval.

## Owned tmpfs cache comparison — 2026-10-10

One subsequent comparison used a unique `mkdtemp` cache directory on the
verified `/dev/shm` tmpfs (64 MiB available before launch), adding only
`--disk-cache-dir=<owned-directory>` and `--disk-cache-size=16777216` to the
same launch setup. Profile location, production packages, diagnostic markers
and ten-second product guards were unchanged. Actual HTTP/Code Cache files
under that directory confirmed its use.

Game `HTTP_CACHE_CREATE_ENTRY` fell from 8,230 ms to 9 ms; fetch-to-JavaScript
headers fell from 8,453.8 ms to 24.4 ms. Renderer headers took 37.4 ms.
WASM initialization and view creation completed within existing guards and
the chooser enabled. This supports the bounded local cache setup correction;
it does not prove every previous timeout had the same cause.

Evidence is under `target/wf/browser-wasm-startup-tmpfs-cache-20261010/`.
Node exited zero. Chromium required SIGKILL during bounded owned cleanup;
Xvfb exited zero, the server closed and profile/cache directories were removed.
Retain this cleanup distinction rather than reporting graceful browser exit.
Connected QA must use original production Worker bytes without diagnostic
transformation, the same exact WASM packages and unchanged completion criteria.

### Known ceiling

Known ceiling: startup-only diagnostic does not verify gameplay or visible history — upgrade when independent connected browser QA runs.

## Independent connected acceptance with the corrected local cache

Fresh independent browser QA at `5a9861c` used original production Worker
bytes, unchanged main/audio packages and product deadlines, plus the owned
16 MiB tmpfs cache. It returned PASS after six-second capture, save/use without
reprepare, visibly inspected stored history and next/back pages, current-play
six-second replay with exact `1/4/0/1` score parity and preserved timing, and
local two-player keyboard/touch stop. Seven screenshots were visually inspected.
There were no console, page or Worker errors. Node, Chromium and Xvfb exited
zero; the server closed and owned profile/cache directories were removed.

Evidence is under `target/wf/qa-browser-records-tmpfs-cache-20261010/`.
This fulfills the continuity flow's browser lens on this local setup, rather
than promoting the preceding diagnostic controls to acceptance. Earlier failed
artifacts and their unproven causes remain preserved. Other browsers, physical
audio/performance, the earlier timing task and the full Goal remain separate.
