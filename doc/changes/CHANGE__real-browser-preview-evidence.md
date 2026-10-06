# First actual generated-WASM browser verification

After the full Node web suite reached 508/0, both browser and browser-audio
libraries built in release mode for wasm32-unknown-unknown (exit 0). Matching
wasm-bindgen-cli 0.2.129 generated ignored web/pkg and web/audio-pkg artifacts.
The local test server uses COOP/COEP headers at http://127.0.0.1:8765/.

The actual qa-browser lens ran against the reviewed UI HEAD 83d7c43. Default MCP
Chromium had navigator.gpu but requestAdapter returned null. A separate software
WebGPU Chromium obtained an adapter and initialized the actual WASM/Worker/wgpu.
Actual File/DataTransfer import of original BMS and 8 kHz WAV produced QA Original,
1 note, 1 sound, last note 2 s and accepted preview seeks at 0/2 s. Local count
1->2, invalid65 preserving2, return1 auto-input controls, and saved opponent
stable ID4 / Removed player4 retention and removal all worked in actual DOM.
The opponent file was selection-only data, not a valid replay execution test.

The lens final verdict was BLOCKED_ENV for visual output: canvas captures were
white and Chromium logged SkSurface/SharedImage initialization failures. This
is an environment diagnosis, not proof of a product renderer defect. Rendering,
audio playback, physical HID and WebTransport acceptance remain unproven.
The audio package was generated after that smoke and was not exercised by it.

Actual screenshots reside under ignored target/wf/qa-browser-software-*.png.
The local server session58260 and dedicated Chromium session45679/CDP9224 remain
owned temporary verification processes for continued work. GPU configuration
recovery and a fresh real-browser pass are required; Node tests are not a
substitute. No full task/Goal PASS or close is claimed.
