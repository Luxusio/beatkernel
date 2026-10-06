# Dynamic gauge policy and historical records

GaugeProfile now represents living minimum, failure-below threshold and a
strict low-level judgment damage reduction (3/5). Default zero dynamics retain
the existing BeatKernel policy. Source adapter rules can resolve profiles with
an explicit fallback BMS judgment and opaque core grade overrides; numeric
grade ordering is never inferred. Judgment deltas use i128 intermediates,
depletion latches at zero and mines retain existing raw damage/instant-death
semantics rather than inheriting judgment guts.

Historical records preserve these resolved dynamics. Version 4 adds dynamic
profile fields; version 5 also carries comparison snapshots. Default profiles
continue emitting the previous versions 1/2/3 and old decoding supplies zero
dynamics. Profile copies, decoder validation and archive result validation retain
failure/minimum invariants, so replay/record consumers do not silently lose
level-dependent policy.
One fallible complete profile-copy method serves the native, stepped and archive
association boundaries. Stepped solo/local owners accept resolved profiles only
in pristine setup before activation or capture; each local member keeps its
independent policy. Actual completion/archive fixtures cover those paths.

Tests compare all six adapter variants against the common runtime observer,
strict threshold boundaries and wide signed deltas, invalid profiles/grades,
dynamic/comparison archive roundtrips, every truncated prefix and corrupted
dynamic bounds. Independent code and security review passed after correcting
profile copies at native and stepped save boundaries. Independent QA passed:
runtime library 1,596 tests (2 ignored), application main 242, BMS adapter 87,
platform 207 (1 ignored), and input-roster allocation regression 1, with no
failures. Workspace all-targets, browser WASM and Windows/macOS all-targets
source checks each exited 0. CLI help exited 0 and an invalid subcommand exited
1 as expected. The eight new fixtures ran successfully, including actual
native-save helpers and stepped completion/archive paths. Cross-platform
checks used C stubs and establish Rust source compatibility only; they do not
establish SDK bridge, physical driver, HAL or native GUI execution.

This provides resolved policy/recording support, not complete historical player
compatibility. Native/browser/replay defaults, selectable BMS judge windows and
gauge kinds, extra/empty judgments, mine compatibility and capture policy
identity integration remain pending. The full player is unfinished.
