# Native static BGA ownership and playfield composition

All seven actual native publication sites (Linux/macOS/Windows solo and local
plus replay) now call publish_native_chart. Attached owners resolve and prepare
referenced static images after their actual chart preparation and before judge
report loops, and atomically publish exact chart, local roster and one immutable
CPU bank. Registration is validated before image IO; failures do not partially
register data. Unattached audio-only commands retain no-image-IO behavior.
Old chart-only publication APIs remain compatible. Results/pause/cancellation
snapshots share the same bank; fresh sessions start empty. This preparation
phase may occur after platform device acquisition, but never in native audio
callbacks or input/report processing loops.

The UI owner reads the exact visible member song timestamps. A four-view/eight
resource cache deduplicates Arc pixel aliases across IDs and members, releases
unselected GPU textures before replacements, reuses steady selections and
keeps upload failures blank without repeating work every frame. Failed uploads
retry when the desired image union changes. Resource removal errors retain
remaining ownership for retry. Native Renderer implements the same owner trait
used by authored cache fixtures. CPU resource count is independent of current
GPU texture slots. Device texture extent/byte limits remain authoritative;
an image that cannot be uploaded contributes a visible unavailable count.

Solo/local playfields compose black, centered aspect-fit base, then layer
using a dark tint below notes. Straight alpha is preserved. Existing note
progress, pressed bands, feedback, labels and judgement line retain their
painter order; the image viewport ends at the judgement line. Undefined base
is black and unavailable layer has no sprite. Legacy organism wrappers keep
plain lanes. A generic BACKGROUND UNAVAILABLE caption reports visible failures.
Poor selection is not displayed without a defined miss activation policy.

Navigation away from play/results, session retry/replacement and exit clear
live GPU ownership. Renderer destruction/suspend discards IDs only after the
renderer is dropped; subsequent resume reuploads from retained CPU data.
Surface recreation with the same device keeps valid resources. UI frame
queries do no file reads or decoding. Selection changes can upload on the UI
thread and can stall presentation; no native latency/performance acceptance
is claimed. Prefetch/streaming improvements, video, additional raster formats,
legacy black-key transparency and poor overlay activation remain pending.

Authored fake-owner cache fixtures cover aliases, steady reuse, removal-before-
replacement, bank changes, failed-upload retry, bounds, cleanup and retained
ownership on errors. Real Scene fixtures cover fit/clipping/tint/order, invalid
frame preflight and sparse/MAX-ID64-member visible paging. Root native publisher
fixtures cover atomic rejected preparation, one shared bank, lifecycle/fresh
sessions and unattached no-IO compatibility. Actual Runtime input/capture/
codec/reconstruct/replay STOP/practice fixture now publishes a real image bank
and checks selected pixels/shared bank on the original song clock. Explicit
pause acknowledgement changes force the coalescing bridge without sleeps;
this fixture does not execute or prove native-device pause. Pure desktop
member-clock/page fixtures check paused and backwards projection.

All fixtures are authored for later execution. Actual native GPU rendering,
device/driver behavior and full player acceptance remain unverified.

Source check evidence: initial host workspace/all-targets, Windows GNU app/
all-targets, macOS app/all-targets, app no-default/all-targets and WASM graphics/
lib all exit0. A final Desktop-only composition fixture adds calls to both
legacy and actual background paths, valid/invalid sprite admission and blank
unavailability caption. Its host/Windows/macOS all-target compilation also
exits0; no-default/WASM production sources did not change after initial checks.
Scoped formatting and whitespace checks exit0. No test/app/GPU/device execution
or formal review/QA occurred.
