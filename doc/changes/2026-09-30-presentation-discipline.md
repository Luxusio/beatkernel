# Ongoing presentation observations

The Windows BMS composition uses progressing WASAPI device/QPC observations
throughout playback instead of retaining a short startup slope as its whole-song
rate. A bounded off-thread observer estimates drift over a declared observation
span and applies bounded phase correction using continuous Transport rate changes.
Startup observations still establish the output-zero origin; historical transport
segments remain available for acquired input, and the judge is not reset. Duplicate
device positions do not refresh progress; stale clocks, resets and excessive drift
or phase errors fail explicitly. Mapping quality remains Unknown without a hardware
error bound. Observation storage is bounded, while Transport history may grow with
successful control updates. The section-restart example creates a fresh observer
for each independently selected original-PCM cue. Synthetic fixtures are authored for later execution; native
playback, test execution, independent review and QA remain deferred.

Locked workspace all-target and Windows-target platform/sample all-target compile
checks passed on Rust 1.98.1. These compile checks do not establish the authored
fixtures' results or native playback behavior. The full original Goal stays active.
