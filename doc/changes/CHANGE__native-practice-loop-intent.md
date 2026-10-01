# Native practice-loop intent and joined repetition

F11 now preflights a fresh pinned invocation with both marked start and end, cancels/drains/joins the previous owner and starts a finite native session. ALSA, WASAPI shared/exclusive and CoreAudio solo/local owners record an explicit completed endpoint only after the immutable PCM endpoint has been presented and input/resume frontiers have drained. UI repetition requires that exact acknowledgement, successful noncancelled Finished publication and a joined worker. Coalesced UI song time and diagnostic seconds cannot trigger repetition. Every repeat derives endpoints and a new replay filename ordinal from the original invocation, with one preroll. F5 restores the pinned original; cancellation disables repetition; toggling off lets the existing finite section finish without repeating. Failure to preflight retains the current owner. Settings/profile drafts now accept optional --end-ns; AddBinding resolves its schema by flag so adding practice fields cannot create an incorrect row.

Regression fixtures cover original invocation/capture stability, bounded atomic failures, settings/profile roundtrips, binding rows, explicit endpoint publication versus cleanup/cancellation, and UI enable/repeat/join conditions. They are authored and source-compiled only. Runtime tests, native GUI/devices and acoustic acceptance remain deferred.

## Known ceiling

Known ceiling: owner reopening can introduce a gap — upgrade when a continuously open native owner can reset all state with verified endpoint and input/capture semantics.
Known ceiling: native finite ASIO/network modes remain unavailable — upgrade when their actual presentation/participant frontier protocols exist.
Known ceiling: an initial clock observation after the endpoint cannot invent a lower interpolation bracket — upgrade when earlier actual observations can be retained.
