# Saved-opponent component for browser gameplay

AC-199 continues the open BMS player task with a portable saved-opponent owner
and actual browser gameplay bindings. Window/Worker selection is following
source work; this milestone does not make the browser feature available yet.

The owner uses the existing Competition and genuine replay reconstruction,
with a pristine local header derived from StepGameplay's actual judge. Capture
is optional. Chart, rules, profile, resolved branch seed and runtime metadata
must match. Own/Other and labels describe selected records, not authenticated
identity or ranked authority. Limits cover eight opponents, a 64 MiB aggregate
encoded budget and labels of 1–256 UTF-8 bytes without control characters.
Rejected admission leaves membership and charged bytes unchanged.

Ghost scores come only from recorded operations through the shared judge.
The actual local song frontier advances their prefixes; crossing a truncated
record's last operation never synthesizes timeout misses or proves completion.
Local input scoring remains in StepGameplay and is not aggregated again.
An explicit reset retains admitted recordings and quota. Backward movement
without reset is rejected. Browser admission ends at activation and releases
the retained parsed source; preparation work stays outside audio processing.

`BrowserGame.add_saved_opponent(encoded, own, label)` admits the actual bytes
while pristine. `saved_opponents()` returns a bounded array on the caller's
control-side request. Snapshots expose bounded labels, kind, genuine counters
and recorded_until using BigInt for integer values. Comparison errors remain
separate from local play and capture completion. Neither UI elapsed time nor
remote scores establish a recorded frontier. Optional comparison state adds
no work to the audio callback.

Independent source fixtures target actual compatibility, encoded admission,
quota atomicity, prefix behavior, monotonic advancement/reset and pristine
header identity. Assertions, browser bindings generation, execution, audio,
formal review and QA remain deferred. The persistent Goal stays active and the
Harness task remains open/PENDING.

After both writers stopped, four genuine compile-only terminal checks returned
exit0: workspace/all-targets, application headless/all-targets, WASM browser/lib
and WASM browser-audio/lib, each `--locked`. Native paths compiled the eight
actual component fixture groups without assertions. WASM retained the three
existing cadence dead-code warnings. Scoped formatting and staged whitespace
produced no diagnostics. These results establish source compatibility only.
