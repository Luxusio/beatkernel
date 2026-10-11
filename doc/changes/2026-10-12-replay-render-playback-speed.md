# Pitch-changing rational replay output

Status: shared planning, offline rendering and CLI integration are implemented. Coordinator-focused
verification passes25 new integration tests,8 existing replay audio tests,
4 existing owned-Stop regressions and3 existing parser tests. Optimized direct
CLI development artifacts pass43 paths. Independent DEEP code and security
reviews pass with no findings; independent QA passes40 related tests and54
optimized direct CLI paths. This is a pre-close evidence snapshot: final
documentation review and Harness verify/close follow publication of this record.
The new [contract](../kernel/REQ__replay-render-playback-speed.md)
connects existing core Rate/Mixer resampling to replay sound planning and actual
offline CLI output, while preserving recording/judge identity and normal output.

Source-backed merged planning confirmed the current1x integration gap, reuse of
the original sound planner and PCM writer, and three mechanical clarifications:
offline finite metadata remains refused, zero quantized section fences exclude
the rate control too, and the existing audio contract is
`REQ__bms-replay-audio.md`. No extra product approval or new framework is needed.
Planner, renderer and CLI have separate file owners with dependent source/test
pairs; the coordinator owns all compiler and runtime execution.

## Development evidence

Original logs are retained in `target/wf/replay-speed-development-12c65/`.
`planner-first.log` compiled in1m48s and passed8 existing/7 new tests. The failing
new gauge fixture incorrectly used a legacy consumer for policy-aware metadata.
`planner-policy-fixture.log` corrected that consumer but exposed a second
fixture issue: releasing the key at the mine instant removed contact ownership.
The fixture now uses a genuine held Runtime Advance and retains its literal
invisible/mine/Stop assertions. `planner-contact-fixture.log` passed8 existing
and8 new tests, with no production behavior or admission rule weakened.

Before those executions the source endpoint check was strengthened: a scaled
finite fence can have a valid u64 frame count yet an unrepresentable Timestamp.
The planner validates its absolute mapped endpoint and preserves representable
negative-origin cancellation. Tests also pin zero quantized fences, inverse
fractions, offsets/preroll, command ties, original metadata and logical identity.
These are coordinator development results, not independent acceptance.

`renderer-first.log` compiled in1m47s and passed8 existing replay audio tests,
8 rate-planning tests and7 independent renderer tests. Literal nonconstant
PCM at1/2,1,2 and3/2 is identical across block sizes1,2,3,7 and32. The suite
also checks original recording/judge identity, explicit output preroll,
zero/cutoff behavior, actual origin-rate queue refusal and a partial writer
failure with its original RenderReport. No renderer test failed on this run.
`owned-stop-first.log` compiled in1m35s and passed all4 existing owned-Stop
tests, including accepted/unknown diagnostics, fatal legacy logs, cutoff/preroll
and partial queue refusal. No compiler process remains from these commands.

`cli-first.log` compiled in40.36s and passed7 new real-binary tests and3 existing
parser tests. Actual PCM16 WAV/recording fixtures exercise four fractions,
default/reduced parity, unchanged logical hash, separate output/preroll/Hz,
malformed/overflow/duplicate refusal before file access, no-overwrite, origin
queue refusal and existing finite/policy rejection. A read-only advisory audit
found no demonstrated defect, but requested two focused coverage additions:
invisible-only selection without mines/policy and owned-Stop execution at a
non-normal rate. Their verification and optimized direct CLI execution are
recorded below.

`coverage-additions-first.log` passed9 planner and9 renderer tests after both
authors froze. The three added tests verify invisible-only selection, genuine
fatal legacy capture at2x with five actual owned Stop diagnostics, and partial
Stop admission refusal with unchanged original command/PCM prefix. No source
behavior changed for these additions. This is development evidence only.

`release-build-first.log` built the current optimized CLI in54.85s. The retained
public capture generator, PCM16 WAV fixtures and `direct_matrix.py` exercised
43 direct paths: four speeds across three blocks, default/reduced ratios,
preroll/cutoff, bounded queue refusal, no-overwrite,20 invalid fractions,
duplicate/missing values and help. `direct-results.json`, per-path stdout/stderr
and `artifact.json` preserve exact argv/exits and output identities. The original
replay bytes and hash `0x024a319d09f2b448` remained unchanged; successful PCM is
checked against literal floats. The CLI artifact is2795488 bytes with SHA256
`de1d18991102e6e6a9bbfba3f962f4ad0a724a50b72679381f30d6cf77698b19`.

Scoped formatting and `git diff --check` pass. `scoped-clippy.log` exits0 in1m02s
for the app library and CLI, with277 warnings. This is not a warning-free or
`-D warnings` result: warnings include the reused large ReplayAudioError return
type on planner helpers and unrelated app code. No error boxing, new framework
or hot-path allocation was added to suppress those warnings. A tracked-doc
frontmatter search found no dependency notes needing invalidation; the generic
freshness walker was not used because it would read the forbidden handoff file.

## Independent review

Source commit `57a61fd` received actual `code_review_speed_12c65_1` PASS with
zero findings, following fresh correctness and contract/test hunters (both
returned empty candidate arrays). Depth was recomputed as DEEP for public and
durable contracts, rational mapping, actual Mixer PCM and CLI resource/error/IO
boundaries. `security_review_speed_12c65_1` independently returned PASS with
zero findings. Neither reviewer executed commands. QA follows these finals;
this review evidence does not replace independent command execution.

## Independent QA

After both actual review PASS finals, `qa_cli_speed_12c65_1` returned PASS for
source `57a61fd` and documentation HEAD `b9587c3`. Fresh focused runs passed
planner9, renderer9, CLI7, original replay audio8, owned-Stop4 and parser3
tests (40 total, zero failures). Scoped formatting, optimized build, WBS
validation and diff checks also passed. WBS remains91/193,47.15%;05.13 is open.

Evidence lives in `target/wf/replay-speed-qa-12c65-1/`: `checks.json` and
`check-0.log` through `check-6.log` retain exact commands/exits; `qa_direct.py`,
`direct-results.json`, per-path outputs and `artifact.json` preserve54 optimized
direct executions. Literal PCM covers four speeds across blocks1,2,7,32,
default/reduced parity, separate device Hz, output preroll/cutoff, maximum
fraction components, malformed/duplicate values, output collision and queue
refusal. Maximum-denominator and preroll mapping overflow leave zero-byte new
files and return `Overflow`; none prints success. Replay bytes and judge hash
remain unchanged. The artifact SHA256 and size match the development build.

The first direct QA oracle incorrectly equated maximum numerator/denominator
with exactly1/2. The selected rational rate is slightly below1/2, retaining an
extra keysound and BGM tail sample. `direct-first.log` and
`direct-first-results.json` preserve that failure; the corrected independent
literal oracle passes in `direct.log`. No product source or admission rule was
changed. All command handles are terminal, including session20198 exit0.
Warnings remain; these results do not assert strict lint or physical acceptance.

Next queued Goal child is `TASK__realtime-replay-playback-speed`: integrate
selected rate through actual common StepReplay/browser and native replay
pause/presentation owners, preserving original-song visual/judge time. Source
survey found separate1x endpoint/presentation paths; changing readheads alone
cannot complete those paths. Runtime switching, live gameplay and retained
practice still require further integration. The full player objective is active.

## Known ceiling

Known ceiling: 오프라인 pitch-changing 배속만 제공 — real-time/native/browser 속도 제어 작업에서 확장.

Offline pitch-changing speed does not establish real-time/native/browser rate
controls, physical delivery or pitch-preserving DSP. Original WBS05.13 and the
full193-leaf player Goal remain unfinished until their whole requirements have
actual evidence.
