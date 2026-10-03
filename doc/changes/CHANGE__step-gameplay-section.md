# Stepped live section ownership

AC-202 extends the existing stepped live owner to carry an immutable
original-song section start. The original zero-start constructor remains the
compatibility path. New callers prepare a fresh chart/bank through the existing
section_start::prepare_at helper before creating the stepped owner; the owner
does not select or copy PCM again. Original absolute object targets remain the
judge timeline, with initial transport position start minus preroll.

Selected BGM targets subtract the section start once before the existing feeder
adds output origin and preroll once. Section metadata propagates into the actual
capture and competition header using the existing bounded replay profile.
Activation and output presentation discipline retain the same initial section
anchor. Completion continues to require actual original judge deadlines and
Mixer/presentation drain rather than an elapsed timer.

This portable owner is the first dependency of browser section playback.
Browser preparation/bindings and Window/Worker launch remain to be connected;
this change alone does not make browser section Play available. Authored genuine
Rust fixtures and planned compilation do not establish acoustic synchronization
or gapless restart. Test execution, application/browser/device/audio execution,
formal review and QA remain deferred.


Six genuine Rust fixture groups were authored, covering original PCM frame
selection, once-only output mapping, section capture/reconstruction/identity,
activation and drift continuity, invalid setup, zero-start parity and natural
drain. Scoped rustfmt and whitespace checks completed. Cargo check reached
exit0 for workspace/all-targets, headless app/all-targets, WASM browser library
and WASM browser-audio library. Existing platform render-cadence dead-code
warnings remain on WASM. These checks compile the fixture bodies but do not
execute their assertions or verify browser/audio/device behavior.
