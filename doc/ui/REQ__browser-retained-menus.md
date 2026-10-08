# Worker-owned shared retained browser menus

Reuse the existing LocalRoster as the canonical business owner in the gameplay
Worker. Checked snapshots carry original next-ID allocation state, members and
assignments; removal must not cause ID reuse or regeneration from surviving IDs.
Window reconciles accepted state instead of allocating competing members. Reject
malformed bounds/duplicates/IDs/allocator state before any owner publication.

Status: Worker-local menu ownership and generation-tagged protocols are implemented
and have development fixtures. Existing gameplay/render Worker separation is
reused. All-route interaction, editor/permission flows and independent browser QA
remain pending.

Browser menu navigation and business drafts belong to the gameplay owner. The
render Worker instantiates local shared retained views/navigator lifecycle and
owns Scene/GPU; no Rc, runtime, PCM bank or Scene crosses Workers. Bounded
generation-tagged immutable view models, semantic actions and geometry ACKs
preserve exact route/action correlation and stale-message refusal.

Menu setup/changed-model packets support the existing 64-member roster and
1,024-source catalog without silent truncation: at most 8,192 fields, 4,096
UTF-8 bytes per field and 4 MiB aggregate including framing. Validate all count,
length and cumulative arithmetic before state/paint publication. Larger packets
refuse explicitly. Full metadata is sent only when its model changes, outside
acquired-input callbacks; unchanged frames reuse retained state.

Selection, Settings, Practice and Back first prove the shared owner; completion
requires Records, Players, Devices, Display and Results with actual supported
actions/capabilities. Preserve drafts, editor state, suspend/resume/dispose,
cancellation and clear unavailable-native capability behavior. Navigation must
not recreate preparation/audio/capture or silently mutate game input policy.

Records previews cross to the renderer as read-only accepted-prefix values:
original path/section, counters, a validated TimingRecord and genuine optional
historical score/comparison/result metadata. The renderer must not rebuild a
live TimingSummary, ScoreSummary, replay runtime or completion authority from
those scalars. Native and frozen preview updates use the same retained Records
view and action gates. Invalid timing, class, history or selected-path association
refuses before publication; an archive association error keeps the valid prefix.
Pure model validation must compile without graphics/browser features and reuse
the same domain validators as the existing visual consumers.

Window retains original keyboard/touch/HID acquisition and necessary user
gesture/file/permission/IME/clipboard bridges. Required event-driven setup/final
DOM is permitted by the existing boundary ADR; no blanket DOM removal or heavy
rendering returns to Window. Preserve stalled-renderer gameplay/audio/input
isolation, bounded coalescing, submitted touch geometry and joined stop.

Actual browser interaction/screenshots, stale actions, Back/cancel, resize and
editor/lifecycle tests prove menu parity. Existing chart-render probes alone
cannot prove these menu workflows or performance/physical input guarantees.
