# Browser HID acquisition boundary

Window owns browser-required permission and actual report acquisition. Retrieve
already authorized devices automatically; request new browser permission only
from an explicit user gesture. Application player assignment is separate from
the browser permission chooser. The optional acquisition component does not
establish playable HID bindings or cross-browser support.

Configure one to sixteen actual HID interfaces and one to 1024 payload bytes
per report. Start adapter identities at or above 3, reserving keyboard/touch
aggregate identities 1 and 2. Device IDs and the host's shared acquisition
sequence retain all unsigned 64 bits; reject exhaustion instead of wrapping. IDs distinguish device objects
without inferring hardware serial numbers. Preserve the event timestamp in the
Window host domain, separate report ID and exact DataView byte slice. Numbered
report payload already excludes the ID; never strip another byte or inject one.
Zero ID means an unnumbered report. Follow the
[official WebHID specification](https://wicg.github.io/webhid/).

Validate metadata and payload bounds before adopting acquisition sequence or
calling consumers. Native report callbacks only acquire bounded snapshots;
canonical encoding and descriptor/control interpretation belong off the main
thread. Do no rendering, DOM geometry queries, fabricated button events or
arrival-time substitution. The browser event timestamp is provenance, not an
inferred physical-device hardware timestamp.

Retain exact ownership through asynchronous discovery, opening, disconnection
and close. Discovery/open operations are serialized and concurrent requests are refused.
Close is terminal and idempotent, detaches listeners immediately,
awaits late openings and closes only interfaces opened by this owner. Partial
setup errors clean the acquired prefix; already externally opened resources
are refused and never closed. Surface cleanup failures. Device disconnection
notifies the host with original event time and source; do not fabricate judge
releases. A stopped owner cannot be revived by a late operation or callback.

Canonical BKPI raw report encoding retains source, acquisition sequence, Window
clock point (domain 0x57494e), native backend 0x57484944, report ID
provenance and exact separate-ID payload. Accept empty reports within configured
bounds. Canonical encoded packets use core BKPI v1 RawHidReport variant 5. Existing
keyboard/touch codecs and ownership remain unchanged. Reports alone have no
logical lane binding; descriptor/profile interpretation, actual page/Worker
forwarding, permission UI and gameplay stop policy remain required integration.
Fixtures and compile checks are source evidence. Device/browser/runtime and
performance acceptance require later execution under the active player Goal.

Permission filters accept at most sixteen nonempty dictionaries. Preserve the
WebIDL filter domains: vendorId is unsigned 32-bit; productId, usagePage and
usage are unsigned 16-bit. Require vendor for product and usagePage for usage.
An empty list is valid, while an empty filter dictionary is refused before the
native chooser. Revalidate active ownership after the external sequence hook
before delivering a report to consumers.

The [common report-profile contract](REQ__hid-profiles.md) supplies bounded
explicit button/axis interpretation through the shared DeviceAdapter/Registry.
Browser interpretation belongs on Worker using this same implementation.
Report profiles do not replace page forwarding or logical gameplay bindings.
