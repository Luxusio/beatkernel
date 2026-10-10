# Worker-owned shared retained browser menus

Explicit host menu-motion requests must pass through the CPU Worker's actual
visual owner and RenderClient. The Window captures the current menu token and
submitted generation/content/geometry identity, copies endpoints and correlates
the reply; it does not generate renderer counters or motion time. The CPU Worker
validates both current business and acknowledged renderer identity, queues at
most 64 independent motion requests without merging different targets, and
revalidates each request before dispatch. Stale, malformed, full or retired
requests receive an explicit rejection. Admission replies require an actual
renderer control ACK; submitted pose remains governed by render-geometry
evidence. Editing or navigating while a request is queued cannot apply motion
to a newer owner or leave the host request unresolved.
Both Window and CPU outstanding-request limits include the in-flight request.
Allocate the outbound geometry version immediately before dispatch, after
checking the captured submitted-geometry ticket. Each request settles once,
including retirement and finite host timeout; a late ACK cannot revive it.
Normal per-request rejection does not retire the renderer. Earlier presentation
may make a second request with an older ticket stale; reject it explicitly.
An unavailable or invalid target refused by the renderer's actual motion API
returns a correlated control rejection without changing published geometry or
accepted poses. RenderClient rejects that one request and remains usable; only
actual transport or rendering failure follows the renderer-failure path.

Explicit component motion for Settings, Records, Players and Devices must use
the shared retained views and the actual menu token, with successful-presentation
pose publication. Readonly Results uses a separate generation/content-owned
node-target motion path in the renderer Worker. Animation frames reuse geometry
and do not require business updates or Window rendering. Replacement, paging,
suspension and disposal obey [the shared motion contract](REQ__ui-motion.md).
The remaining-surface extension has independent interactive browser QA on the
current production WASM/Workers: four menu owners, actual host editor
motion/edit/preedit, genuine completed Results, resume/retirement and disposal.
Readonly solo Results without comparisons does not prove connected comparison
switching; CLI fixtures cover that capability separately. Hardware latency and
loading-speed measurements remain unverified.

Reuse the existing LocalRoster as the canonical business owner in the gameplay
Worker. Checked snapshots carry original next-ID allocation state, members and
assignments; removal must not cause ID reuse or regeneration from surviving IDs.
Window reconciles accepted state instead of allocating competing members. Reject
malformed bounds/duplicates/IDs/allocator state before any owner publication.

Status: Worker-local menu ownership and generation-tagged protocols reuse the
existing gameplay/render Worker separation. Independent motion-scope browser QA
and UX pass; this does not establish all permission/device/product flows. See
[the exact acceptance record](../changes/2026-10-10-remaining-component-motion.md).

Composition commits must publish the final editor value through the existing
correlated Worker edit transaction before Enter can apply a field. Intermediate
composition updates must not submit incomplete values, and navigation/disposal
must not apply a stale composition to another screen or field.

An admitted Players roster update must refresh the Window acquisition controls
and status from the same canonical roster. Growing from one to multiple players
must enable discovery when setup is otherwise idle; removal, stale updates and
busy play retain the original ownership/capability gates. Supported assignment
must remain reachable through the ordinary Players flow.

Menu snapshots omit assignments belonging to an inactive or retired acquisition
inventory. Such stale sources cannot authorize play. Successful cleanup retires
absent source assignments atomically in the Worker while preserving player IDs
and still-owned sources. Rediscovery publishes the current acquired inventory
before assignment or start can proceed; one bounded pending/latest publication
orders release and rediscovery across matching source-table/capability ACKs.
Stale owner, screen or generation replies cannot revive retired assignments.

When a Players roster or acquired Devices inventory shrinks, retained selection
and page start are normalized against the actual player/source row count before
publishing the new menu model. Serialized metadata field count is not a row count.
Empty and nonempty shrink retain a valid visible page and cannot fail rendering
after an otherwise admitted roster/inventory update.

Pagination remediation development verification passes the combined app library
2107 tests / fail 0 / ignored 4 (`browser-pagination-app-development.log`),
including actual owner/presentation regressions for Players 21→10 and Devices
20→5→empty while a later page is selected. Domain metadata, cached composition
and stale/malformed refusal remain verified. The current Rust WASM and bindings
also rebuild successfully; actual browser regression and fresh review/QA are
still required.

Settings Apply validates the thirteen scalar menu fields using the same bounded
timing/output/capacity/section rules as full settings profiles. Applying scalars
preserves the currently selected keyboard bindings. Full profile import/export
continues to require its kind/version/exact schema and all eighteen keyboard
lanes; a menu draft must neither fabricate a binding map nor weaken that check.

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
