# Browser native frame diagnostic

This opt-in diagnostic compares native a-rate parameter edges with actual callback extents while the production processor remains in setup phase0. It adds no product clock and does not resolve the earlier strict browser chronology FAIL.

## Run

Use the existing generated audio WASM and installed Chromium/Puppeteer. No dependency installation or Rust rebuild is required for this fixture.

```bash
PUPPETEER_MODULE="$PWD/target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core" \
CHROMIUM=/usr/bin/chromium WORKLET_NATIVE_FRAME_PROBE=1 \
WORKLET_CHRONOLOGY_OUT="$PWD/target/wf/browser-native-frame-diagnostic/local" \
WORKLET_CHRONOLOGY_PORT=8161 \
node app/web/worklet-chronology.browser.mjs

node --experimental-vm-modules --test app/web/worklet-native-frame-probe.test.mjs \
  app/web/audio-host.test.mjs app/web/audio-worklet.test.mjs
```

Choose an unused owned port and a unique output directory. The runner owns its HTTPS server, browser, pages, contexts and private probe channels. Evidence records source/WASM SHA, Chromium version, applied sample rate, actual callback extents, marker targets/edges, refusals and cleanup. Default and `WORKLET_CHRONOLOGY_REQUIRE_COLD=1` chronology acceptance remain separate.

## What constitutes an initial anchor

Start a fresh context/parameter with native default0 and exactly one first `setValueAtTime(1,time)` event. No previous reset, parameter-value assignment, generation or cursor is admissible. For intended onset T and native rate F, `markerTime` uses `(T-0.25)/F` only when the represented double, multiplied by F, remains strictly inside (T-1,T) and rounds upward to T.

A full native Float32Array must contain an exact zero prefix and one suffix with one interior edge k. For a known actual onset T the callback base is T-k. Constant arrays, index0, nonbinary values, mismatched lengths and unsafe arithmetic refuse. This spatial calculation cannot authenticate an arbitrary scheduled onset: the fresh first-event prerequisite belongs to the caller.

The second independent marker must agree with the first base plus every actual observed callback extent. A contradiction fails and preserves both raw candidates; the runner never rebases. Raw global `currentFrame` is a separate observation, never the oracle. Two successful observations establish only their measured interval.

## Why the scheduling prerequisite matters

Native late automation is clamped. With a preceding reset, floating multiplication can turn the clamped quantum boundary into the following sample:

- `(1664/44100)*44100 = 1664.0000000000002`, whose ceiling is1665.
- `(896/48000)*48000 = 896.0000000000001`, whose ceiling is897.

An interior edge at index1 can therefore appear even when the original scheduled onset was missed. The pure oracle deliberately cannot detect that prerequisite violation. The initial native anchor requires a fresh single first event; previous-reset cases are refused separately.

## Native evidence and remaining limits

The pinned Chromium sources are commit971a7443b0c9b0a9b2860529b33331b76077ec62, corresponding to the tested Chrome153.0.8010.12 artifact:

- `BaseAudioContext::UpdateWorkletGlobalScopeOnRenderingThread` publishes the Worklet frame only after a successful graph try-lock. A failed lock leaves the published global frame unchanged. [Primary source](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/base_audio_context.cc).
- `RealtimeAudioDestinationHandler` can skip graph pulling while still advancing the destination frame. Thus a local callback counter does not prove uninterrupted native chronology across all lifecycle states. [Primary source](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/realtime_audio_destination_handler.cc).
- `AudioParamHandler::CalculateTimelineValues` uses the destination sample frame; first-event handling, late-event clamping and sample rounding explain both the independent marker and its admission constraint. Native parameter generation remains ongoing work when attached; it is not a zero-cost product fix. [Primary source](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/audio_param_handler.cc).
- The Web Audio rendering-loop contract is distinct from implementation evidence and browser-specific measurements. [Web Audio specification](https://www.w3.org/TR/webaudio/#rendering-loop).

The earlier actual failure recorded PROCESS status6, owner phase2, current161408, expected161536 and block128 without injected omissions. The native try-lock branch is a plausible mechanism, not an instrumented proof of the exact lock collision in that run. Neither a delayed DOM suspend event nor a first raw frame proves an exact callback origin.

A source-possible lifecycle counterexample remains: when embedding policy restricts media-playback-while-not-visible, native interruption and automatic restart can complete before queued statechange notifications run. Those notifications carry no prior-state snapshot; the current Host handler reads only the current state and can see running on both. This is conditional source evidence, not a reproduced failure or a claim about every top-level page hide. [Pinned AudioContext source](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/audio_context.cc), [current Host handler](../../app/web/audio-host.mjs).

Real observations apply only to measured rates, extents, contexts and generated WASM bytes. They do not prove cross-browser chronology, future callback continuity, presentation/acoustic timing, successful gameplay, replay/input integrity or high-load performance. Production chronology checks, report coordinates and ownership remain unchanged.

See [recorded change evidence](../changes/2026-10-13-browser-native-frame-diagnostic.md) for actual results and independent review/QA status.
