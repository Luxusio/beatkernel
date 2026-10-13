# Linux player output encoding

Implementation status, 2026-10-13: the shared typed setting, Linux schema,
profile persistence and Watch argument projection are implemented. The actual
solo/local/replay launchers and paused ALSA request/capability mapping remain
pending. The requirements below describe the complete intended behavior;
this prerequisite checkpoint does not enable end-to-end encoding selection.

Linux native Settings and the live/local/replay ALSA launchers accept
`--encoding float32|pcm16|pcm24|pcm32`. Omission uses Float32. PCM choices
mean full-width signed little-endian 16/24/32-bit samples; PCM24 is packed
three-byte storage. ALSA must apply the exact requested encoding or refuse.
Other host settings and non-ALSA replay backends reject an explicit flag.

Keep the setting as a pure typed choice and map it statically at the output
adapter. Reuse existing native encoders and complete output owners. Do not
change source PCM sample rate/channels, Mixer float storage, input timestamps,
selected speed, replay bytes or recording conditions to change native storage.

Incomplete text may remain in the editor. Explicit CLI empty/unknown/case or
whitespace-altered tokens refuse before native open. Blank initial drafts omit
the option; blank live drafts preserve the current exact encoding. Retain
bounded values, duplicate rejection, existing row IDs and keyboard navigation.
Linux defaults/profile overlay/primary preparation and desktop Watch/replay
projection must retain every selected token instead of silently dropping it.

Native, remixed and converted output requests preserve selected encoding while
rebuilding rate/channel/matrix fields. Native/remixed adapters still reject any
explicit rate override; converted adapters support existing target-rate changes.
Report the actual applied encoding in capabilities. Unsupported bit pairs and
inconsistent applied metadata refuse rather than claiming an approximate format.

Pending PCM may not be reinterpreted with another encoding. Both complete owner
types must reject incompatible partial pending blocks before native admission,
preserving recovered Mixer, exact tail/report/frame basis and retry. A later
native-open failure can retire the old stream while retaining its stopped state;
do not claim it stays attached. An error retains the edited UI draft and prior
accepted capability; only successful capability refresh replaces the draft.

Existing shipped retained-practice output replacement remains explicitly
unavailable. This encoding feature does not remove that guard. Verify truthful
refusal in the same generic Output editor; adapter/null replacement success does
not prove active retained replacement or physical sound synchronization.

Existing profiles without the option remain compatible. Profiles saved with it
are rejected by older strict-schema binaries. Before rollback, remove the
optional field through supported profile editing/export for the older version.

Verify pure token/schema/profile/primary forwarding, actual cold requests for
solo/local/replay, exact production encoder bytes, native and converted pending
refusal/retry, and actual GUI edit/Apply/refusal/Back. ALSA null execution proves
software format/open/retire ownership only. Physical input/audio, device latency,
foreign-host encoding choices and full WBS05.12/07.09 remain separate obligations.
