# Windows native audio control contract

Phase 7 must implement caller-controlled native WASAPI shared and exclusive
output under [the primary goals](../common/REQ__project__primary-goals.md).
This contract is a requirement, not a native playback verification result.
The OS-independent [audio runtime](../kernel/REQ__audio.md) never depends on COM
or Windows device types.

## Device and configuration controls

- Enumerate render endpoint identities, names, states and default-role metadata.
  Open the explicitly requested endpoint and mode. Never silently substitute a
  device, shared/exclusive mode, sample rate, channel layout or encoding.
- Expose mix format and arbitrary caller format probing. A finite displayed
  capability matrix identifies the configurations tested; it must not claim to
  enumerate every supported combination. A closest-format suggestion is not an
  accepted configuration until the caller explicitly chooses it.
- Callers control buffer size and processing period. Accept explicit frame-count
  or duration requests; expose device-default selection as an explicit policy.
  Support independent size/period requests whenever the native mode permits.
  Report units, requested values and actual applied values separately.
- Explicit device-managed buffer selection is also available. IAudioClient3
  computes buffer capacity from the selected processing period; this selection
  accepts that capacity while preserving Exact period negotiation. A numeric
  buffer request still requires an exact applied match or explicit rounding
  permission. Do not silently interpret a numeric size as device-managed.
- Query minimum, maximum, fundamental increment and alignment constraints where
  the API provides them; report unavailable constraints honestly. Do not limit
  callers to fixed presets. Validate rate/channel/container/valid-bit/channel-mask
  combinations and overflow before passing format memory to the OS.
- Hardware-engine buffer duration limits are distinct from processing period
  limits. Preserve both and identify the wake policy used for the buffer query.
  The portable backend period query uses event-driven bounds; opening a timer
  stream queries its timer-specific bounds. Unsupported optional queries leave
  those fields unknown, while device invalidation remains an error.
- Preserve an explicitly requested zero speaker mask as direct output, rather
  than rejecting it or replacing it with an inferred speaker layout. Device
  support must still be probed. Microsoft's [extensible format contract](https://learn.microsoft.com/en-gb/windows-hardware/drivers/ddi/ksmedia/ns-ksmedia-waveformatextensible)
  defines this layout for multichannel port-oriented devices.
- Exact configuration is the default. Explicit caller-opt-in negotiation may
  accept documented supported rounding/alignment; report every change. Unsupported
  combinations return precise constraints and suggested alternatives. Never
  resize or alter a mode without the selected policy authorizing that change.

The user's 2026-09-30 clarification requires adjustable buffers and coverage of
many configurations. Device/API constraints still apply; expose them so the
caller can make the next choice.

## WASAPI modes

- Shared mode exposes the engine mix format and IAudioClient3 period negotiation
  when available, including minimum/maximum and fundamental-frame multiples.
  Legacy shared buffering is an explicitly selected policy, not an invisible
  fallback from a requested modern low-period stream.
- Shared event-driven initialization uses an OS-managed buffer. For an
  independently sized legacy shared buffer, callers explicitly select the
  timer-driven wake policy and a polling interval in whole milliseconds.
  This leaves the engine's device-default processing period unchanged. The
  default wake policy remains event-driven; numeric buffer requests are never
  silently converted to a timer-driven stream. Exclusive output uses events.
  Microsoft's [Initialize contract](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize)
  requires zero buffer and period arguments for shared event initialization.
- Exclusive event-driven initialization requires matching buffer and period.
  Reject conflicting independent requests with that constraint. An OS alignment
  suggestion is returned, or retried only under explicit caller policy. Format
  unavailable, exclusive access disabled, endpoint busy and device invalidated
  remain distinguishable failures.
- An alignment failure's suggested next buffer size is not an alignment
  increment. Return that size as a suggestion; leave the increment unknown
  unless the native API actually reports it.
- Shared wakes fill available frames derived from padding. Exclusive event wakes
  fill the complete required packet. Prefill before starting. Support native
  float32 and PCM16/24/32 conversion with checked format and buffer bounds.

Native integer conversion clamps finite input to [-1, 1], rounds to the nearest
signed valid-bit value with ties away from zero, saturates at the signed limits,
and left-aligns valid bits in the little-endian container. Float32 preserves
finite input values. Validate every input value and the exact interleaved byte
extent before writing any output; conversion never allocates. Channel order is
preserved, including explicit direct output. There is no hidden dithering or
channel remapping.

Frame requests compare against applied frame counts. Duration requests require
exact rational frame duration under Exact policy; unrepresentable durations
return a supported frame suggestion. Opt-in negotiation may round upward to a
supported fundamental multiple within device bounds. Unknown native bounds
remain unknown, and a suggested size does not prove the device will initialize.

These mode constraints follow Microsoft's [stream management](https://learn.microsoft.com/en-us/windows/win32/coreaudio/stream-management)
and [exclusive stream](https://learn.microsoft.com/en-us/windows/win32/coreaudio/exclusive-mode-streams)
contracts. Requested configuration probing follows [IsFormatSupported](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-isformatsupported).

## Thread, resource and failure ownership

- A dedicated worker owns its COM apartment, interfaces, render events and
  MMCSS registration. Allocate all mixer and conversion storage before Start;
  setup, error formatting and COM teardown remain outside buffer filling.
- The caller may select MMCSS priority or explicitly disable registration.
  Unsupported or failed priority setup is reported rather than silently applied.
  Start acknowledges native startup. Stop is terminal for that stream instance
  and joins its worker; starting again requires explicitly opening a new stream.
- Fill paths use fixed numeric error/status reporting, including native failure
  exits. Audit projected COM error construction for allocation; use narrow
  documented raw FFI calls where necessary. Every acquired buffer has exactly
  one corresponding release on the owning thread.
- Startup failures release partial resources before returning failure. Shutdown
  wakes and stops the worker, joins it, then releases assets. Signal failures
  require a bounded recovery path; worker panic is reported by join. No detached
  callback may outlive its mixer or sample storage.
- Associate the owned render event immediately after successful event-driven
  initialization, before later validation can fail. Cleanup never releases an
  initialized event-driven client while withholding its required event handle.
- Device loss or render failure produces explicit terminal status. Reopening or
  selecting another configuration is the caller's action, not automatic fallback.

## Clock and telemetry

- Preserve raw audio-clock position, frequency, QPC association and reading
  quality. Position units are not assumed to be sample frames. IAudioClock's
  reported QPC time uses 100 ns units; map it explicitly to the existing host
  clock origin rather than treating it as a raw QueryPerformanceCounter tick.
  See [GetPosition](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclock-getposition).
- Report submitted frames, actual buffer/period, padding, stream latency,
  callback/deadline failures, scheduling lateness, rejected commands and native
  error separately. Label suspected starvation as inferred. Do not advertise an
  authoritative hardware underrun count when the render API does not supply one.
- Submitted PCM and progressing clock prove stream activity; virtual execution
  does not prove sound heard, physical-device latency or input-to-audio latency.
- Off-thread telemetry reads make a bounded attempt to obtain one coherent
  worker publication. A collision reports telemetry unavailable explicitly;
  placeholder zero counters are not measurements. Lifecycle status remains
  separately observable, and callers can retry outside the render path.

## ASIO and other output APIs

ASIO remains a required optional backend for compatible installed drivers after
its licensing/distribution path is verified. No SDK-derived source, bindings or
binary enters this MIT project merely because ASIO is requested. The existing
[licensing condition](../common/REQ__project__primary-goals.md) remains in effect.
Report unsupported/unresolved backends specifically. Additional native APIs need
concrete capability and runtime contracts through the same platform boundary.

## Verification and current environment evidence

Pure tests cover format packing, widths/layouts, exact/closest responses, buffer
and period requests, shared negotiation and exclusive alignment policies. Native
tests must execute finite shared and exclusive streams with explicit settings,
successful submissions, clock progress and clean stop/reopen, plus actionable
device/configuration failures. Compile checks and unsupported-mode errors do not
prove successful playback.

A bounded planning probe on 2026-09-30 found AudioSrv and AudioEndpointBuilder
running in the retained Windows Server Hyper-V guest, but successful native
MMDevice enumeration returned zero render endpoints across all states. PnP and
registry inventories corroborated this. The guest was stopped normally with no
driver/settings changes. It can test enumeration and absent-device behavior;
successful output still requires a suitable endpoint. This is an environment
limitation, not a reduction of the implementation or verification requirements.

A further bounded probe reused the existing Windows Sandbox on 2026-09-30.
The provider accepted start, but guest command transport timed out before any
endpoint inventory was returned. Owner status only described recorded state,
so it did not establish guest readiness or audio capability. The Sandbox was
stopped normally afterward with provider exit code zero. Its endpoint support
remains unknown; this attempt supplied no native playback evidence.
