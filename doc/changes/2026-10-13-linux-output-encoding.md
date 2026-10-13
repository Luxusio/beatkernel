# Linux output encoding implementation and checkpoint history

The user requested frequent coherent commits while development continues.
The prerequisite checkpoint `cf9d3906` stores the shared setting and its tests
before the dependent ALSA and launcher paths. `03c3603d` saves the ALSA request,
capability and owner regressions; `d196991f` saves the live/local/replay cold
requests and their fixtures. Those consumers are connected in the implementation
candidate. `d90fb48b` saves the desktop and shipped CLI regressions. Whole-task
independent review, CLI/desktop QA and broader integration/foreign typing
checks remain pending. The connected author checks below passed. This document
does not close the Harness task or complete WBS05.12/07.09.

Implemented:

- Pure `OutputEncoding` choices: `float32`, `pcm16`, `pcm24`, `pcm32`.
  Exact parsing rejects empty, unknown, case-changed and whitespace-altered
  tokens. The default remains Float32.
- Static adapter mapping to full-width native encodings; unsupported native
  container/valid-bit pairs are refused.
- A Linux-only bounded Settings field appended after the existing schema,
  preserving all previous row IDs. Raw editor drafts remain editable.
- Native profile v1/v2 persistence, overlay, preparation and Watch argument
  projection retain an explicit selection. Omission leaves old arguments and
  profile bytes unchanged.
- Native, remixed and converted paused ALSA requests carry the choice through
  format reconstruction. Capabilities report the exact applied encoding;
  malformed edits and inconsistent applied metadata refuse.
- Actual Linux solo and local startup share the cold request carrying the
  choice, including fixed-speed retained paths. Recorded ALSA startup carries
  it through its converted request. Other replay backends reject explicit
  `--encoding`, including `float32`; omission preserves their existing default.
- Existing production encoder and complete native/converted owner fixtures
  cover exact bytes, incompatible pending-encoding refusal, recovery and retry.
  Source PCM, Mixer storage, selected speed and recording bytes remain separate
  from native storage selection.

The existing generic Settings and Output editors expose the Linux schema;
desktop fixture registration uses that editor without a new UI framework.
On Apply error, the edited draft and prior accepted capability remain; only a
successful applied-capability refresh replaces the draft. Actual window
edit/Apply/refusal/Back/reopen evidence remains pending. Retained-practice
output replacement still explicitly refuses as before and remains a separate
unfinished requirement. Adapter/null success does not establish successful
replacement in shipped retained-practice play.

Existing profiles without `--encoding` remain compatible. New profiles storing
it are rejected by older strict-schema binaries; remove the optional field
through supported editing/export before using an older binary.

Historical prerequisite author verification at `cf9d3906`: **52 passed,
0 failed, 0 ignored**.

| Check | Passed |
| --- | ---: |
| New output encoding settings/profile/encoder integration tests | 13 |
| Existing native speed settings integration tests | 14 |
| Existing `settings::tests` unit tests | 12 |
| Existing `native_defaults::` unit tests | 13 |

Commands used `-p beatkernel-bms-runtime --features desktop,webtransport
--locked -j 2`, with `--test output_encoding_settings --test
native_speed_settings`, `--lib settings::tests`, and `--lib native_defaults::`.
All three commands exited zero. Existing dead-code/unused-import warnings
remain; this is not a claim that all lint checks or the whole suite passed.
The static checkpoint assessment found no concrete source issue. Focused
author checks and that assessment do not replace full-task review or QA.
Checkpoint documentation was independently checked against the source and logs.

Local logs are in ignored `target/wf/linux-output-encoding/ac001-settings.log`,
`ac001-settings-unit.log` and `ac001-native-defaults-unit.log`; they are not
part of the committed artifact. The tests themselves are committed.

Subsequent coordinator verification of the connected consumers:

| Check | Result |
| --- | --- |
| `--lib alsa_ui` | 12 passed, 0 failed, 2 ignored |
| Opted `actual_null_all_encodings_open_start_retire_and_retry_both_complete_owner_types` | 1 passed, 0 failed, 0 ignored |
| `--bin linux_bms output_encoding` | 7 passed, 0 failed, 0 ignored |
| `--bin play_replay_bms output_encoding` | 7 passed, 0 failed, 0 ignored |
| Main binary `output_encoding` (including five desktop journeys and embedded live/replay parsers) | 19 passed, 0 failed, 0 ignored |
| Shipped CLI encoding integration | 4 passed, 0 failed, 1 ignored |
| Opted recorded null CLI playback through primary and standalone, all four encodings | 1 passed, 0 failed, 0 ignored |

These focused commands used `cargo test -p beatkernel-bms-runtime --features
desktop,webtransport --locked -j 2` with the listed target/filter. The opted
null diagnostic used `BEATKERNEL_TEST_ALSA_DEVICE=null` and `--ignored`.
It exercised all four encodings through actual native and converted
open/start/retire/recovery/retry software lifecycles. It does not prove
physical sound, device latency or retained-practice Apply success.

The first combined binary fixture compile failed with three test type errors:
two `usize::from(channels)` conversions inferred `channels` as `i32`, and
`judge_with_policy` received `CompiledBms` instead of `CompiledChart`. The
fixture was corrected to use `[1u16, 2]` and `source.compile().unwrap().chart`;
the subsequent Linux and replay focused runs above passed. The failed attempt
is retained in ignored `target/wf/linux-output-encoding/ac003-ac004-binaries.log`.
The ordinary pending-owner fixtures now validate compatible reconfiguration
without opening a device; actual compatible open/start/retire/retry remains in
the opted null diagnostic. The portable fixture rerun passed 12 tests with two
ignored (`ac002-alsa-ui-portable-c2.log`).

The replay fixture was initially placed directly in `src/bin`, so Cargo
auto-discovered it as an executable during the connected integration build.
It now lives in `app/src/bin/play_replay_bms/output_encoding_fixtures.rs`, with
both module paths adjusted. After relocation, the focused live/replay binary
rerun passed 14 tests, none failed or ignored
(`ac003-ac004-final-checkpoint.log`).

Successful earlier logs are `ac002-alsa-ui.log`, `ac002-alsa-null.log`,
`ac003-linux-c2.log` and `ac004-replay.log` in the same directory. Existing
warnings remain; these checks do not establish all-lint or full-suite success.

The first connected CLI compile also needed an explicit `Option<String>` type
for its retained original-section comparison. After the fixture relocation,
the main-binary run passed 16 tests and failed three desktop assertions because
the fixture left the channel's cold `Unavailable` publication unread before
installing its supplied paused state. The fixture now consumes and checks that
placeholder first, preserving every paused-owner, scope, launch and draft
assertion; production code is unchanged. The subsequent connected command
(`--bin beatkernel-bms-runtime --test native_output_encoding_cli output_encoding`)
passed the 19 main-binary and four CLI tests above, with one opted CLI test
ignored. The separate opted test passed with
`BEATKERNEL_TEST_ALSA_DEVICE=null` and `--ignored`: primary and standalone
recorded playback each opened all four native encodings with source 48000Hz,
target 44100Hz and speed 2/1, preserving source and capture bytes. This proves
software paths, not physical sound or input timing. Logs are
`ac005-connected-c4.log` and `ac005-recorded-null.log`; failed attempts remain
in `ac005-connected.log`, `ac005-connected-c2.log` and
`ac004-ac005-connected-c3.log`.

Independent DEEP review of `ee44e0ff` found two concrete issues. The appended
encoding row reduced the current supported settings capacity: 64 local players,
18 visible-lane bindings and eight opponents need 129 materialized rows. The
model and retained Settings view now account for that one additional Linux
row together. Other host models retain 128 rows, and raw native/profile records
remain bounded at 128 with the same value and aggregate byte limits. This
addresses current configuration capacity. Both profile versions and actual
129-row view selection/refusal have executable regressions.

The desktop CLI also now parses explicit encoding tokens before profile
overlay can erase an empty token. Real saved v1/v2 startup profiles verify
omission and all four valid overrides; invalid CLI tokens refuse before GUI,
catalog or device access. Blank editor and live drafts retain their intended
semantics.

Coordinator remediation checks passed:

| Check | Result |
| --- | --- |
| Library `settings` filter | 76 passed, 0 failed, 1 ignored |
| Complete main-binary test target | 393 passed, 0 failed, 13 ignored |
| Native speed settings | 14 passed, 0 failed, 0 ignored |
| Shipped CLI encoding tests | 5 passed, 0 failed, 1 ignored |
| Encoding settings, including five new capacity regressions | 18 passed, 0 failed, 0 ignored |

Commands used the same package/features/locked/two-job setup, with `--lib
settings`, then `--bin beatkernel-bms-runtime --test output_encoding_settings
--test native_speed_settings --test native_output_encoding_cli`. The first
connected attempt passed the other targets but failed a new UI fixture's
Scene-revision assertion: composing legitimately clears/replays geometry and
advances the Scene revision. The corrected fixture checks rejection before
composition, then exact geometry/colors/UVs/hits after composition, preserving
every capacity and atomicity assertion. The separate encoding-settings rerun
passed all 18. Logs are `remediation-settings-unit.log`,
`remediation-connected.log` and `remediation-settings-c2.log`. Fresh independent
review and full QA of the repaired candidate remain pending.

Remaining task gates include broader app integration,
portable/foreign typing checks, independent code review, CLI/desktop QA and
final documentation review. Physical sound, device latency and Windows/macOS
encoding support remain unproved or separate obligations. The parent player
Goal remains active; no push is authorized.
