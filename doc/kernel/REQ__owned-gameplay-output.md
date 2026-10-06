# Own output replacement once for all gameplay adapters

One statically dispatched business owner connects OutputReplacement to the
existing paused gameplay publication port. It owns the current output, one
pending typed request and any controller or rejected-ready resources. Platform
adapters supply opening, retirement and original timing; they do not duplicate
the replacement state machine. Existing input, clock acquisition and session
owners remain independent.

`GameplayOutputOwner` provides `queue`, `publish_paused`, current-output views,
normal observation/resume/end delegation, a pending query and explicit
`cancel`/`stop`/retirement retry/recovered-Mixer access. Typed requests rejected
by `queue` are returned with their wait policy. Original `ReplacementFailure`
payloads are downcastable through the application error boundary; publication
refusal retains a separate inspectable complete Ready owner. Successful current
retirement can retain the stream for genuine post-join telemetry.

An explicitly queued request starts only through the fully committed paused
hook. Acquire the actual runtime producer hold, retire/recover the current
output, stage and open through OutputReplacement, and poll with caller-supplied
host time. Publish only genuine Ready timing using publish_ready_output. No
automatic resume, implicit retry/fallback or dropping rejected ready ownership.
Retain original failure and cleanup/recovery diagnostics. Reject a second queued
request without silently consuming its typed request.

While waiting for a new output, preserve the last genuine committed output
report and original pause evidence without admitting new-output samples into the
old observer. Keyboard acquisition continues through the existing pump; no
fabricated progress, clock sample or terminal boundary. Once published, all
observation, resume seeding and end evidence must use the current epoch/output.
Successful resume seeding requires an actually accepted latest presentation
pair. A backend returning success without admitting a sample is a refusal,
not permission to commit resumed transport. Preserve the existing live pause,
transport and observer while callers stage the fresh presentation candidate.
Both pumps defer resume requests while replacement is actively holding output;
otherwise the old pause could enter Resuming and stop the eligible publication
polls. Expose this through the default-false business device pending query, with
compatibility forwarding and owner delegation. Do not block normal pause
requests or infer a pending operation merely from an idle queued request.
Expose a separate default-false suspended-output-clock query. Only the actual
owner with no published current output and a Waiting replacement reports it.
During a committed nonterminal Paused boundary, receipt/input timestamps at or
after that boundary do not extrapolate the old output clock. Preserve original
observations and still validate input domain, receipt bounds and chronology.
Older input, active output, queued changes and reply contention keep normal
freshness validation. The replacement controller's explicit wait budget bounds
this state; resume and new Ready publication restore ordinary observation rules.
Finite end projection of interval backends must use original backend evidence,
not the correction midpoint. Native adapters retain thread-affinity and pending
retirement safety. Cancellation/explicit stop retire waiting/current resources
through the existing controller/native APIs and preserve errors.

Wire this owner into Linux solo and cohort device composition so actual ALSA
outputs use the common controller. Preserve startup, final diagnostics, input
shutdown order, original completion/error arbitration and optional output
availability during failed replacement. User-facing live setting commands and
other native adapter compositions remain follow-up integration work.
Each static backend owns its typed output/request. Cross-backend switching can
use a platform adapter enum implementing the same port; the common owner must
not introduce OS-specific branches to achieve that. That enum adapter follows
[output backend switch](REQ__output-backend-switch.md); sample-format changes
are not yet delivered by this owner increment.

Independent tests use real Mixer/queue/PCM and presentation implementations to
cover multi-poll waiting, request and failure ownership, cancellation/retirement,
new epochs, original observation provenance, finite end and both actual pumps.
Author assertions without executing them. After both writer stops, run scoped
Rustfmt and the four sequential compile-only configurations. Reviews, required
browser/CLI/desktop QA, hardware and acoustic validation remain deferred; no
close/PASS or full Goal completion is implied.
