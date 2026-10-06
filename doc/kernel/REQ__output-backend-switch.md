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
match; conversion, the Windows composition/UI request mapping and hardware
acceptance remain follow-up work.

Linux fixtures drive the actual owner and controller with the real memory Mixer
through two distinct adapter types: a round trip with epochs 1 and 2 and resumed
PCM continuity, second-side open refusal followed by an explicit first-side
request, pending open with cleanup and explicit retirement retry, and current
side retirement refusal that never opens the other side.

Canonical implementation lives in gameplay/output/adapters/switch.rs and depends
on the output port. The public root module is a static re-export; no wrapper
instance, duplicated implementation or additional crate is introduced.
