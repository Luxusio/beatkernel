# Output format converter integration

Ports the converter and twelve integration fixtures from old-base branch
`wf/format-convert` (`7b51062`) into current base `796ec6c`. Current frame-basis
and mixer-handoff exports are preserved; no dependency or crate is added.

ChannelMatrix validates explicit gains and provides layout-agnostic defaults.
FormatConverter preallocates source-window/kernel storage at setup. Linear and
bounded windowed-sinc kernels use exact rational phase across callback sizes;
render performs no allocation, lock or dynamic dispatch. The source callback
returns its original report in the source frame grid.

This port replaces silent saturation of frame cursors with checked refusal
before source invocation. A new unit regression covers pass-through,
channel-only and rate-conversion overflow paths. Kernel half-width is explicitly
not a measurement of pulled-frame lead or native latency. A source error
preserves converter phase/retained history, but cannot undo source side effects
or caller output writes. See [audio requirements](../kernel/REQ__audio.md).

An additional integration regression demonstrates actual source progress for
48kHz→8kHz and 24kHz→48kHz conversion while the nominal linear kernel width
remains one. The current converter integration suite has thirteen tests, plus
the internal cursor-overflow unit test and two public doctests.

Verification for the current integration (actual process exits 0):

- Latest complete core suite: 345 passed, 0 failed, including thirteen converter
  integration fixtures, the cursor-overflow unit and two new doctests.
- Runtime library with webtransport: 1,574 passed, 0 failed.
- Workspace all-target check with runtime/webtransport and WASM browser library
  check: passed. Existing dead-code warnings remain.
- Independent code/security reviews: scoped PASS, no findings. Independent
  CLI/library QA: scoped PASS at test-suite depth; no separate converter CLI
  was added or simulated.

Fixtures exercise actual Mixer 44.1kHz mono→48kHz stereo reports/PCM, rational
linear oracles, callback partition equivalence, bounded sinc DC/band-limited
tone and above-Nyquist attenuation, invalid buffers/source errors and extreme
setup bounds. Test-local allocator instrumentation requires zero allocation,
reallocation and deallocation on the exercised callback paths. These tests do
not establish every quality/performance configuration or physical output timing.

Ignored evidence logs: `target/wf/format-converter-qa-cli-{core,runtime,workspace,wasm}.log`.
Actual scoped results do not establish full-task receipt attestation. No task
verify/close or full Goal completion is attempted.

Native output owners still refuse mixer/device format mismatches. Converter
ownership across streams, exact presentation mapping, buffered pause/end
fencing and native UI/backend integration remain required. Passing a source
report through the converter does not convert it into device presentation or
drain evidence. Device execution, full player QA and performance specifications
are not established by this foundation.
