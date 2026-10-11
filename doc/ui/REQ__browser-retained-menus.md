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

Settings Apply validates thirteen legacy scalar fields or sixteen complete
scalar/policy fields using the same bounded
timing/output/capacity/section rules as full settings profiles. Applying scalars
preserves the currently selected keyboard bindings. Full profile import/export
continues to require its kind/version/exact schema and all eighteen keyboard
lanes; a menu draft must neither fabricate a binding map nor weaken that check.

The complete Settings draft keeps indices 0–12 unchanged and appends numerical
timing preset, rank precedence and gauge at indices 13–15. Apply checks the whole
draft before any acquisition mirror changes. Legacy thirteen-field Apply leaves
existing policy untouched. Loading an old full profile without policy restores
the default triple: empty preset, `rank-first`, `beatkernel`. Empty preset with
`defexrank-first` and `beatkernel` preserves that latent precedence text even
though the runtime policy is absent. Existing profile encoding omits policy only
for the exact default triple; no timing algorithm or profile version changes.

Settings keyboard row/page movement must reach all fields using ordinary focus
transitions. Up/Down, Home/End, PageUp/PageDown and Tab/ShiftTab submit existing
field-selection intents; Escape returns to the original parent. Native editors,
composition, modified keys, repeats and active gameplay retain input ownership.
Browser Settings shows native-only Add Binding and Profile Path as unavailable,
excludes their hits and refuses direct unsupported actions. Native capabilities
remain enabled. Players Done and Back return to the same Settings parent,
preserving its policy draft, selection, canonical roster and acquisition owner.

Full profile loading while Settings or its retained child is open must not leave
an older retained draft that can overwrite the loaded profile on the next Apply.
One existing settings operation owns a bounded candidate. It publishes correlated
pending state, validates the original menu token and complete acquisition draft,
and admits only the matching candidate. Admission is a commitment boundary:
ordinary cancellation and the file-acquisition deadline apply before admission.
The Worker atomically replaces the original Settings draft and retained Practice
section values, clears pending and publishes the resulting token before the
final response. Window mirrors the already validated candidate only for that
accepted owner/result token. Parent/child identity, route, selection, other
drafts and roster stay intact. Held file reads remain owned until settlement;
cancelled consumers cannot revive or accumulate candidates. After admission,
a lost or malformed acknowledgement requires joined owner retirement/reset
before ordinary use resumes; it cannot claim rollback while retaining an
uncertain committed owner. Invalid files, stale tokens and pre-admission draft
changes preserve the previous accepted configuration. Loading, refusal and
correction use existing pending/error surfaces and keep values readable.
The original acquisition form may contain unfinished invalid timing or duplicate
keyboard choices. A valid full profile must repair that form without requiring
the old draft to pass profile validation. Preserve its complete bounded strings
for drift checks and uncertain-result restoration; strictly validate all values
and eighteen bindings of the incoming profile before admission.
Saving publishes correlated pending/completion state without replacing the
retained draft. Cancellation before admission remains possible when the pending
receipt has not arrived; it must match that operation's original token. If an
admitted load requires owner reset, accepted acquisition values are restored
before use resumes; the existing reset can release chart and roster owners.

Verify current production Workers/WASM with trusted keyboard policy edits,
Apply, Back/reopen, Practice and Players Done, full profile/binding roundtrip,
and genuine finite selected-policy playback with observed audio progress.
Injected composition events are bridge evidence, not physical OS IME evidence.

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

## Actual Records selection and relative opponents

Records must connect the selected genuine recording's accepted prefix, timing
and optional historical/comparison values to its read-only retained surface.
Add Own and Add Other require a valid preview of that selected record and
available capacity. Their accepted entry retains the exact stored source key,
original File handle and explicit kind; the selected Own/Other counters and
painted action gates reflect the same canonical Worker selection.

An admitted complete menu snapshot without a preview must remove the previous
prefix/history display and disable Add, even if its selected key matches the
previously painted record. Intermediate screen updates may be omitted; the
latest complete state remains authoritative. Membership-based Remove stays
available. Partial presentation updates and complete same-key updates carrying
a valid preview preserve their existing details state.

The browser keeps source keys unique. Repeating Own or adding the same key as
Other refuses atomically and preserves the prior selection; this does not
change the native draft's separate duplicate-path policy. Existing eight-entry
and64MiB bounds remain. Selection metadata must not eagerly acquire file bytes.

Remove Own and Remove Other need exact selected-key membership of that kind,
even after changing rows has invalidated the current preview. Removal does not
delete or reopen the stored recording and preserves unrelated selected records.
Absent/wrong-kind removal is unavailable. Clear removes selected opponents only.
Back/reopen preserves the canonical opponent draft and the original Settings
parent; later valid actions must not restore a removed entry accidentally.

When Records owns ordinary keyboard input, ArrowUp/Down/Home/End select the
current row and Escape returns to its original Settings parent, including an
empty catalog. Repeats, composition, modified keys, native text/select editors
and active gameplay retain their existing input ownership. Same-row boundary
keys do not publish redundant actions. Keyboard requests use current menu/action
identity and cannot target a replaced Window or menu revision.

Scan after an actual catalog change must make current stored rows available in
the retained screen while preserving the literal selection if it still exists.
Asynchronous acquisition/publication uses the existing current-menu token,
selected-path and replacement fences; a stale result cannot revive a retired
screen or replace the accepted prefix.

Verify with ordinary trusted canvas/keyboard actions52/53/54/60/61, at least two
genuine stored recordings, nonzero selected counters and actual history pixels
correlated with Worker state. Changing rows then returning without Preview proves
membership-based removal. A prior DOM opponent chooser journey alone cannot
prove this retained connection. Protocol/codec/quota fixtures separately prove
hostile/stale/late/boundary branches; GUI success is not physical latency,
screen-reader completeness or whole-player acceptance.
