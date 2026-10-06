# Recover actual browser visual/audio/keyboard smoke with software Vulkan

The initial SwiftShader ANGLE/Vulkan combination exposed a WebGPU adapter but
failed compositor SkSurface/SharedImage initialization. A new isolated Chromium
configuration uses the same Vulkan path for ANGLE and WebGPU:

```
chromium --headless=new --no-sandbox --disable-dev-shm-usage \
  --enable-unsafe-webgpu --enable-features=Vulkan \
  --use-angle=vulkan --use-vulkan=swiftshader \
  --remote-debugging-port=9227 \
  --user-data-dir=/tmp/beatkernel-qa-webgpu-vulkan \
  http://127.0.0.1:8765/
```

The actual qa-browser lens returned PASS for bounded real preparation, preview,
audio and keyboard smoke. Actual generated release WASM/Worker/wgpu displayed
lane rectangles, judgment line and song time and handled preview seeks at 0/2 s.
Trusted CDP Play clicks loaded the real audio package and reached natural completion
(Hits0/Misses1). A second run with 1000 ms early/late windows and trusted KeyZ
Down/Up reached Hits1/Misses0/Combo1 and Solo/complete captured replay.
This broad window proves routing/judgment/capture functionality, not input latency.
No runtime exception, SkSurface/SharedImage failure or device loss was observed
in the recovered configuration. Favicon404 and zero-instance draw warning remain.

Screenshots/logs are ignored local evidence in target/wf/qa-browser-vulkan-*.png
and *-live.log/*-keyboard.log. The healthy browser is session22051/CDP9227 and
server session58260/port8765; failed diagnostic browsers9224/9225/9226 are stopped.
The same Harness run resumed after genuine visual environment recovery.

Formal receipt limitation: task_start reports previous review completion records
cannot pair uniquely with starts. Actual independent finals are non-attesting for
the task close gate; do not repair/replay reviews solely for receipt collection
or self-author receipts. Full CLI/desktop/browser acceptance and remaining feature
integration must still be completed before any task/Goal completion audit.
Physical speakers, device HID/Gamepad, acoustic latency, real WebTransport,
multiplayer and replay playback were outside this smoke. Software GPU output is
not physical GPU performance evidence.
