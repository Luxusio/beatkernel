# BMS WAV00 plans and stepped playback

MineSoundPlan compiles actual mine IDs and prepares optional WAV00 bindings only
for nonfatal markers when WAV00 is defined. It retains source WAV gain and one
reusable voice per logical mine lane above gameplay, BGM and press-sound voices.
Capacity/timing validation remains required when sound is absent; fatal-only or
absent WAV00 sources invent no sample, voice or sound. Fatal sound omission is
documented in the [BMS mine contract](../kernel/REQ__bms-mines.md).

Local preparation remaps within-lane aliases into disjoint member voices.
RuntimeGroup preflights the exact roster and all voice collisions before
installing hazard timelines; successful press and hazard configurations reserve
their voices so reversed configuration order also refuses conflicts atomically.
StepGameplay installs actual solo/local plans after press sounds, including the
delegated StepLocalGameplay setup, and refuses missing audible WAV00 PCM.

An optional versioned mine-sound fingerprint extends the common source sound
identity with semantic IDs/control/time/gain/sample selection. Resource paths and
remapped output voices are excluded; absent audible mine sound preserves the old
identity exactly. Existing source-aware capture/replay validation reuse that
identity. SongCompletion includes audible mine PCM tails in preparation extent
while preserving actual mixer and output-drain requirements.

Five independently authored deferred fixture groups cover parsed plans and voice
exhaustion, capture identity, atomic group installation in either order, actual
solo/local reports and failure prefixes, and PCM tails/output completion.
Scoped rustfmt and git diff --check completed successfully. Compile-only checks
completed with exit 0 for workspace all targets with WebTransport, headless all
targets with WebTransport, WASM browser, and WASM browser-audio. An initial type
inference error in the semantic identity plan was corrected before rerunning the
failed workspace check. Existing unused playfield-wrapper and WASM cadence
warnings remain. Assertions, browsers, applications and devices were not run.

Guarded file preparation still refuses mine charts. Native/replay/offline sound
installation, WAV00 asset loading and complete gauge/fatal-stop handling remain
unfinished. No runtime/device/browser/performance acceptance, formal review/QA or
overall Goal completion is claimed.
