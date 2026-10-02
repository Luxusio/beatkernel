# Shared native judging and capture preparation

Linux, Windows and macOS solo sessions and the common local-cohort setup now use
`native_judge` for the grade-one profile, optional full-song completion, capture
limits and pristine replay initialization. The core `JudgeEngine` constructor
remains the existing common engine. Cohorts still create one profile and clone it
for independent members.

Early/late windows, input offset, referenced PCM, output domain, section start and
chart seed retain their original values. Finite sessions skip full-song completion;
disabled recording skips unused recording validation. Enabled recording keeps the
existing header/input codec bounds and caller byte/record limits.

Preparation remains at its existing lifecycle stages. Solo completion precedes
publication, profile creation follows publication/cancellation, and capture starts
inside the cleanup-protected outcome after native owners open. Cohort pure member
preparation still precedes opponent loading. Native binding selectors, voice IDs,
recording paths, competition setup and cleanup behavior are unchanged.

## Evidence and remaining work

Five source-only fixture groups cover asymmetric judge boundaries and both offset
signs, PCM-derived completion with actual mixer reports and a presentation boundary,
disabled recording and exact enabled limits, legacy replay bytes and original
seed/start/domain, and actual Runtime reports reconstructed through replay. They
were authored and compiled without execution.

All five first compile checks passed: Linux workspace/all targets, Windows GNU
application/all targets, macOS application/all targets, headless application/all
targets and WASM graphics library. Dedicated
`target/ac152-{host,windows,macos,headless,wasm}.exit` artifacts contain zero.
Scoped formatting and diff checks passed; existing macOS `block` future-compatibility
and WASM cadence warnings remain.

Actual device, file, graphics, network and formal acceptance remain deferred.
Remaining native resource and solo finalization policy, ASIO interval startup and
broader player/browser functionality still require work.
