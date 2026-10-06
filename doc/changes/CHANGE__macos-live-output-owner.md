# macOS solo live output ownership

Current inspection showed that Windows/macOS native gameplay still used raw
streams rather than the common replacement owner. This change connects macOS
solo gameplay to GameplayOutputOwner and its existing paused replacement/UI
protocol, using static CoreAudio and remixed adapters. Startup uses the same
raw stream inside that owner; actual stream Start acknowledgement, not a second
adapter-local boolean, determines observation admission.

Solo nonnetwork capability includes device/buffer/channel matrix. Matrix text
selection/preservation/reset is a common domain function shared with ALSA;
the CoreAudio mapper only converts native request fields. Accepted matrix
metadata is retained and canonicalized for replies. Native callback/listener
retirement, epoch/frame basis/end/pause evidence and source Mixer custody remain
the original common controller/adapter paths. Legacy startup and HID acquisition
keep original device identity and timestamps. Network/cohort/Watch capabilities
are not advertised until their owner composition is connected.

Pure common selection fixtures and macOS-specific typed mapper fixtures are
prepared. See [player requirements](../kernel/REQ__bms-player.md). Independent
code and security reviews passed for this change, followed by scoped CLI QA:
runtime library 1,584 passed / 2 ignored, application tests 223 passed, and the
explicit actual ALSA null matrix diagnostic 1 passed, all with zero failures.
Workspace all-targets and WASM browser library checks exited 0. Shipped CLI help
exited 0; unknown-option handling returned the expected exit 1.

macOS application all-target source checking exited 0 using C SDK stubs. The
macOS mapper fixture compiled but was not executed on Linux. Actual CoreAudio
HAL/HID and paused GUI execution require a macOS environment and remain
unverified. Linux portable execution and cross-source checks are not macOS
HAL/HID/full GUI execution. Windows output ownership,
macOS cohort/network/Watch output controls, launch/profile wiring, full-rate
conversion and full player acceptance remain unfinished.

Next Windows migration must retain the existing ASIO host HWND through driver
release, service its sysref messages, and refresh its multimedia-clock anchor.
The current standalone replacement adapter does not compose those application
responsibilities; substituting it directly is not sufficient to preserve the
existing live output behavior. Keep the original interval evidence, fault
checks and source frame basis when connecting common ownership.
