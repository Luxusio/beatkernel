# Linux output encoding prerequisite checkpoint

The user requested frequent coherent commits while development continues.
This checkpoint stores the shared setting and its tests before implementing
the dependent ALSA and launcher paths. It does not close the Harness task,
complete a WBS item or claim selectable PCM output works end to end.

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

Pending: actual solo/local/replay cold requests, paused native/remixed/converted
ALSA request mapping and applied capabilities, connected CLI/desktop coverage,
independent full-task review and QA. Until those consumers are connected,
the new draft field is preparation infrastructure and an explicit selection
is not accepted by the shipped launchers. Retained-practice output replacement
continues to be a separate unfinished requirement.

Existing profiles without `--encoding` remain compatible. New profiles storing
it are rejected by older strict-schema binaries; remove the optional field
through supported editing/export before using an older binary.

Focused author verification: **52 passed, 0 failed, 0 ignored**.

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

Physical sound, device latency and Windows/macOS encoding support are outside
this checkpoint. The parent player Goal remains active; no push is authorized.
