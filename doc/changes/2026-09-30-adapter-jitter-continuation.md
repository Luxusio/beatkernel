# Device adapter registration and interval jitter

Added a portable platform-owned registry around the existing core DeviceAdapter
contract. Host-owned descriptors and acquired reports select a unique registered
factory and independent mutable adapter per connected device. Limits bound
registrations, devices, lifetime identities and report/emission storage. Disconnect
drops the adapter instance; retired runtime identities are not reused. Typed
fanout retains the entire acquisition metadata and rejects malformed batches.
Callback side effects are trusted and are not rolled back on rejected output.

Native Windows HID packets may contain multiple reports with the same acquisition
sequence. Registry chronology therefore permits equal sequence with identical
metadata and routes these reports in received order. It rejects regression and
conflicting equal-sequence metadata instead of renumbering native acquisition.

Added a separate interval-jitter collector using explicit ClockPoint observations,
positive nominal period and finite retention. It reports signed interval error
and absolute-deviation percentiles, with wide arithmetic for the full timestamp
span. Domain/chronology errors preserve the baseline; discontinuities require an
explicit reset. The offline benchmark labels generated cadence as synthetic.

Portable workspace all-targets and focused telemetry/platform compile checks
passed. Fixtures and examples are authored for later execution. Formal reviews, QA,
native runs and timing measurements remain deferred under the user's sequencing
instruction; compilation alone is not completion evidence. The full Goal remains
active, including ASIO and all outstanding verification.
