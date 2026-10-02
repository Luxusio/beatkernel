# Shared native local-cohort gameplay

Local multiplayer on ALSA/evdev, CoreAudio/IOHID and WASAPI/Raw Input now enters one `native_cohort::run_cohort` owner over the existing `RuntimeGroup` and `InputMerger`. The shared loop handles ordered input admission, pause/resume boundaries, lagged deadlines, player reports, replay capture, scores, competition and completion. Linux and macOS continue acquiring input while awaiting native pause/resume acknowledgements. Any native input backlog holds the cohort judgment frontier. Each player retains an independent judge and capture while transport, BGM and native audio output remain shared.

Windows solo and cohort sessions reuse the same native adapter and message reader. The adapter retains accepted WASAPI snapshots across resume rather than changing observation sources. Linux retains fair bounded collection from every evdev owner; macOS retains native attachment and HID-loss checks. Native shutdown precedes independent create-new replay saves on all exit paths.

Finite sections require an actual native endpoint plus the committed cohort input frontier and every logical prefix. Full-song completion requires each player’s actual completion evidence and drained native/merged input. Missing completion metadata cannot silently terminate a cohort. Original timestamps, native payloads and partial group reports are retained.

## Known ceiling

Native setup/discovery and per-player resource preparation/cleanup still have platform composition. ASIO bounded-interval startup and SDK/device execution remain incomplete. Source fixtures and cross-target compilation do not prove physical latency, driver behavior, desktop presentation or real multiplayer execution. User-requested execution/formal review/QA deferral remains active.

After all writers stopped, `cargo check` passed for workspace/all targets, Windows GNU/all targets, macOS/all targets, no-default-features/all targets, and the WASM library with graphics/no default features. Native-only unused-import/feature cleanup was followed by successful rechecks of the four affected native/all-target configurations. Final exit artifacts are `target/ac142-{host,windows,macos,headless}-2.exit` and `target/ac142-wasm.exit`, each 0. Scoped formatting and diff checks passed. The existing macOS dependency future-compatibility and WASM cadence dead-code warnings remain.

Six source fixture groups exercise the actual RuntimeGroup, Mixer, capture and replay reconstruction: equal-time multi-device input/backlog, acquisition during pause acknowledgement and resume provenance, finite all-member prefixes, partial audio-failure captures, merger/lag boundaries, and sparse player identities/replay paths. These fixtures compiled but were not executed. Formal review and QA remain pending.
