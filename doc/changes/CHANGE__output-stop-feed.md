# Feed mapped output Stops

BgmFeeder::from_output_commands now prepares output-domain Play and Stop without
remapping origin/preroll; BgmFeeder::new remains song-time Play-only. SetRate/Seek,
nonfinite Play gain and targets before output origin still reject. Sorting keeps
timestamp plus original ordinal, including same-frame Play/Stop ordering.

The existing feed path applies credit, chronology, horizon and budget to both
kinds, retaining original queue refusal and accepted prefix. admitted_stops()
retains only Stop callbacks that returned success and remains unchanged by
refusal or credit retirement. BgmFeedReport's public shape and strict render
validators remain unchanged. Callback success is distinct from Worklet ACK,
actual mixer execution, silence and full output completion.

Three independently authored groups use real command queue/Mixer output for
same-time Play/Stop versus Stop/Play behavior, sparse full-width voice identity,
unknown_stops diagnostics, exact Stop refusal and explicit retry without duplicated
accepted commands, retained counters after credit retirement, negative output
origin/subframe mapping and configuration/chronology/command-kind rejection.
Both writers delivered terminal Writes STOPPED before scoped formatting and the
four authorized compile-only checks. No test assertions, browser/device execution,
strict render-validator change or physical completion is claimed. Replay/offline
gauge-failure audio planning and actual completion evidence are still unfinished.

Scoped two-file formatting and whitespace checks succeeded. Four authorized
compile-only checks exited zero: workspace/all-targets WebTransport,
headless/all-targets WebTransport, WASM browser and WASM browser-audio. Existing
unused-code warnings remain in audio cadence and the playfield progress helper.
The host checks compile these fixtures but do not execute their assertions.
