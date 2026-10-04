# Portable browser settings files

The browser player gains explicit Save settings and Load settings controls for
timing, output preferences, audio capacities, section positions and all eighteen
keyboard binding rows. A validated import replaces the complete idle draft;
refusal leaves prior values intact. The versioned browser-specific file format
and ownership contract live in
[REQ__bms-browser.md](../kernel/REQ__bms-browser.md#portable-browser-settings-files).

Worker reads the selected File, decodes strict UTF-8, parses JSON, validates the
complete schema and produces encoded export bytes. Metadata, actual reads and
encoded files are bounded to 16 KiB. Existing timing, output, capacity, section
and physical-key validators remain authoritative. Decimal spellings are
preserved and full binding rows use canonical lane order, including unbound
lanes. Files, native/device profiles, source identities, local assignments,
records, chart selection and network credentials are not part of this format.

Window captures a bounded draft on user action, handles one correlated request
with a ten-second deadline, checks returned metadata before any assignment and
uses browser Blob/download URL APIs for Worker-produced bytes. Settings actions
are fenced while gameplay or retained local source ownership is active, and
pending requests prevent overlapping setup actions. Stale/foreign replies,
changed drafts and failures cannot partially overwrite settings. UI updates
are event-driven; no new gameplay rendering, polling, Window File reads or
Window JSON processing is introduced.

## Source evidence and deferred verification

The actual codec is `samples/bms-runtime/web/settings-profile.mjs`. The actual
page and Worker handlers in `main.js` and `worker.js` use its bounded DTO and
File helpers; `index.html` supplies explicit save/load controls. Independent
deferred fixtures add four codec groups, three actual Host groups and two
actual Worker groups. Existing preview fixtures link the actual settings
module. Source group counts are 4 / 111 / 103 / 14 respectively; they are
authored groups, not executed results.

Both writing lanes reached terminal STOPPED before source inspection and
`git diff --check` (exit 0). This JavaScript-only slice did not execute tests,
parsers, browser/file/device/network flows, Cargo checks or formal review/QA.
The broader player task remains open. The latest input-scope clarification is
already covered by the performance-first browser thread ownership contract:
Window acquires keyboard, touch/pointer, HID, Gamepad and future supported
sources; Worker interprets and renders. No keyboard-only restriction applies.

## Known ceiling

This portable draft format does not select devices or grant permissions.
Playback still uses its actually opened output rate; latency remains the
existing browser hint. A response deadline cannot interrupt a native File read;
Worker retains its operation until actual settlement. Actual browser downloads,
file picker behavior, setup controls and measured input/main-thread cost remain
deferred acceptance. Source fixtures are preparation, not executed acceptance
or full-player completion.
