# Switch live output between two static backends

`OutputBackendSwitch<A, B>` implements the existing OutputReplacementBackend
port for two statically injected platform adapters that share one Presentation
type. Both adapters stay owned for the session; `Switched<A, B>` carries the
output, request and error of exactly one side. GameplayOutputOwner and
OutputReplacement are used unchanged, so cross-backend replacement (Windows
WASAPI to ASIO and back is the motivating composition) adds no OS-specific
branch, dyn dispatch, allocation or lock to the common owner or audio callbacks.

The queued request's side selects the opening adapter. Every other lifecycle,
observation, report, end and pause-evidence call follows the side that owns the
output, including adapter overrides such as original interval evidence. The
controller still retires the current side, recovers its unique Mixer, issues one
monotonic epoch sequence across both sides and opens the other side with that
Mixer, so paused cursor, queued commands and publication checks stay common.
`Switched` implements StoppedMixerSource and forwards Display/source unchanged.

Open refusal is retagged to its side without dropping the recovered Mixer, the
pending partial owner or the separate cleanup diagnostic. Retirement/recovery
refusal stays tagged with the current side and keeps that owner pending. There
is no implicit fallback to the other side; a later attempt needs an explicit
request. Both sides currently share one Mixer, so sample rate and format must
match; conversion and hardware acceptance remain follow-up work. Windows uses
its existing WindowsRequest union and native owner rather than this generic
helper, because it retains the HWND and original native observation metadata.
Its target settings contract is documented in
[Windows live backend selection](REQ__windows-output-backend-switch.md).

Linux fixtures drive the actual owner and controller with the real memory Mixer
through two distinct adapter types: a round trip with epochs 1 and 2 and resumed
PCM continuity, second-side open refusal followed by an explicit first-side
request, pending open with cleanup and explicit retirement retry, and current
side retirement refusal that never opens the other side.

Canonical implementation lives in gameplay/output/adapters/switch.rs and depends
on the output port. The public root module is a static re-export; no wrapper
instance, duplicated implementation or additional crate is introduced.
# Explicit channel requests

`RemixedOutputRequest<R>` carries a native request and an optional validated
ChannelMatrix. `RemixedOutputBackend<B>` statically routes strict requests to
the existing open port and explicit matrices to an OutputChannelRemixBackend
extension port. Native adapters implement only their platform API calls; the
domain request and the output owner/controller remain common.

Output/error types are unchanged. Retire/start, epoch, original frame basis,
presentation reports, original end and pause-observation overrides must forward
to the actual backend, including interval-based ASIO evidence. Recoverable and
pending open failures retain the original owner and cleanup error unchanged.
Use the existing owner/controller for pause-boundary replacement and monotonic
epochs; no implicit fallback, device discovery or matrix choice occurs here.

Verify routing and unchanged PCM/Mixer custody through actual owner/controller
memory traces, error/retry and pending cleanup. A separate explicit ALSA null
diagnostic exercises the native adapter's matrix route. These are composition
primitives; UI text/profile/configuration integration, rate conversion and
physical device acceptance remain required before full player completion.
