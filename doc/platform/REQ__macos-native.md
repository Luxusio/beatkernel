# macOS native acquisition and output

The macOS platform layer acquires physical input and renders PCM; live input and replay use the same core JudgeEngine through the application composition root. The platform layer does not judge charts or know game profiles.

IOHIDManager runs on its creating thread's retained CFRunLoop. Its boxed callback context stays allocated until callbacks are unregistered and the manager is unscheduled/closed. Device enumeration returns native registry identity plus a session DeviceId; reconnects receive new session IDs. Input value callbacks preserve HID usage page/usage, raw mach timestamp and integer value/cookie in a native envelope, and emit canonical button/axis events with explicit mach nanosecond ClockPoint provenance. Typed axes use f32 as required by the public event model; exact native integer values remain in the envelope. Unknown layouts are counted/reported, not converted into fake contacts or poses. Input Monitoring denial and callback/native failures are explicit errors. Polling and event queues belong on the owner/gameplay thread, outside audio callbacks.

MachClock caches mach_timebase_info and one explicit initialization origin. Absolute mach ticks, absolute nanoseconds and origin-relative host time are separate representations/domains. IOHIDValue timestamps and CoreAudio host timestamps use the mach absolute-time base, which stops during system sleep; no relation to continuous/wall clocks is silently inferred.

CoreAudio uses explicit AudioDeviceID, exact requested nominal sample rate/channel count/buffer frames, and separately reports native applied settings. There is no default-device replacement or WASAPI shared/exclusive emulation. Each actual output stream virtual ASBD must be packed native-endian float32 linear PCM with exactly one frame per packet; other encodings/flags are rejected. Both interleaved and noninterleaved buffer layouts are validated from native stream configuration and again in the IOProc. Preallocated interleaved mixer scratch is distributed across validated native buffers, without callback allocation, locks, decoding or property queries. Dynamic format/layout changes are reported as callback failures requiring explicit reopening.

IOProc state lives in a stable boxed allocation with guarded UnsafeCell storage. Only the callback accesses mutable mixer/scratch while active; the control thread reads scalar atomic telemetry. Native property listeners invalidate output on rate/buffer/format/layout changes without querying properties in the callback. Lifecycle stops the exact IOProc, destroys registration, and waits for an in-flight callback to quiesce before releasing the mixer/queue/sample assets. If native shutdown fails, callback context is deliberately retained instead of risking dangling pointers. Device-global rate/buffer properties are requested explicitly and read back; native drivers may reject or asynchronously apply them, in which case exact mismatch returns an error instead of substituting settings. No exclusive ownership or atomic global-device reconfiguration is promised.

Telemetry preserves actual AudioTimeStamp host ticks and sample-frame bits/validity flags. Presentation ClockPoints come from the callback's output timestamp with an explicit native mach domain and cached integer conversion; an immutable Mixer output-domain/frame-grid origin is supplied by the app. The native presentation point and logical frame-grid point are reported together so the application can explicitly calibrate their relation; HAL startup time is never silently equated to the mixer origin. These are observed clock representations, not a zero-latency or calibrated DAC timing claim. Native permission, link/runtime behavior and hardware timing remain unverified until the user resumes verification.

ABI sources: Apple's [IOHIDManager header](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDManager.h), [IOHIDValue header](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDValue.h), [IOHIDElement header](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDElement.h), [AudioDeviceIOProc documentation](https://developer.apple.com/documentation/coreaudio/audiodeviceioproc), [IOProc registration](https://developer.apple.com/documentation/coreaudio/audiodevicecreateioprocid(_:_:_:_:)), and [Core Audio ASBD layout](https://developer.apple.com/library/archive/documentation/MusicAudio/Conceptual/CoreAudioOverview/CoreAudioEssentials/CoreAudioEssentials.html).

## Explicit timestamped raw IOHID reports

`HidInput::open` keeps the scalar value path. `open_reports` or
`open_with_options(HidInputOptions::Reports { .. })` selects a separate raw-only
path and never registers scalar input-value callbacks. Raw mode descriptors set
`raw_hid=true`; default scalar descriptors do not claim acquired raw reports.
`pop_report` drains a distinct bounded queue. Native envelopes retain actual report
type, full uint32 report ID, exact callback bytes, arrival mach ticks and canonical
metadata derived from that supplied timestamp/device session identity. No current
receipt-time clock read substitutes for an unavailable acquisition timestamp.

Apple's [IOHIDManager.h](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDManager.h#L434-L448)
marks timestamped manager registration available from macOS 10.15. The exact
[IOHIDBase.h callback ABI](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDBase.h#L103-L111)
is context pointer, signed IOReturn, sender pointer, uint32 report type, uint32 ID,
mutable byte pointer, signed pointer-sized CFIndex length, uint64 arrival timestamp.
The [device implementation](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDDevice.c#L1787-L1813)
forwards the originating IOHIDDeviceRef as sender; the manager propagates registration
to its devices. Callback buffers are borrowed and copied before return. They are
never retained or interpreted as IOHIDValueRefs.

Registration resolves `IOHIDManagerRegisterInputReportWithTimeStampCallback` only
for explicit raw mode via `dlsym`, using Apple's [RTLD_DEFAULT definition](https://github.com/apple-oss-distributions/dyld/blob/main/include/dlfcn.h#L88-L93).
IOKit is already linked for manager ownership, so the resolved function stays live.
The new symbol has no static link reference; unavailable symbol yields explicit
`HidError::Unsupported`. Scalar default acquisition gains no macOS 10.15 symbol
dependency. [Manager implementation](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDManager.c#L1125-L1146)
stores and propagates a null callback for unregistration. Owner-thread shutdown
unschedules the runloop and unregisters the cached timestamped callback before
closing/releasing the manager and boxed context.

Queue slots are capped at 65,536 and raw report bytes at 1 MiB with a 64 MiB
queue-slots-times-byte-limit ceiling. Full queues, oversized/invalid native reports,
allocation failures and timestamp failures are explicit counters/errors. This is
an off-audio owner-thread copying path; no allocation-free callback claim is made.
Removal retires per-device IDs; previously queued reports retain acquisition identity.

`HidReport::to_raw_report` requires caller-selected `NativeReportLayout` and a byte
cap, delegating the portable normalization helper. Separate-ID payloads and validated
leading-ID payloads remain explicit choices. The actual native bytes/ID/type remain
available independently; no report-prefix or vendor control mapping is guessed.
The callback is an input-report API, but the envelope still preserves actual type;
hosts must select acceptable native report types before vendor adapter routing.
The optional example raw CLI chooses layout explicitly, prints native/canonical
representations and does not automatically decode vendor controls. Compile checks
alone do not verify native report delivery, permissions, buffers or runtime linkage.
