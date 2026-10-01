# Native keyboard metadata discovery

Query keyboard metadata outside UI/game/audio callbacks. Discovery does not
register input callbacks, read event streams, change clocks or seize devices.
Return exact native selection identities separately from ephemeral runtime IDs.
Never select the first row or silently substitute an unavailable device.

Windows uses unregistered Raw Input device enumeration and exact interface
paths. Linux scans bounded event nodes and uses read-only nonblocking metadata
ioctls without changing acquisition clocks; inaccessible candidates remain
visible disabled. macOS enumerates IOKit HID services and primary keyboard/keypad
usages without opening an acquisition manager. Bound traversal and metadata;
reject capacity overflow without truncation. Native provider allocations and
wall time can precede application admission limits.

The app admits at most 1024 entries, 4096 bytes per ID/name/detail and 4 MiB
aggregate text. Display controls flatten; IDs remain exact. Actual native
preparation validates access and attachment independently. Fixture execution,
native discovery and UI lifecycle behavior remain user-deferred.

Linux discovery classifies EV_KEY plus KEY_A, KEY_Z and KEY_ENTER, so dedicated
keypads/controllers outside this subset may need an advanced explicit path.
All directory entries count against a 4096 scan ceiling; complete native names
are limited to 16382 bytes by ioctl encoding. Metadata fd opening can activate
the kernel input device temporarily. ABI follows the
[Linux UAPI](https://github.com/torvalds/linux/blob/master/include/uapi/linux/input.h)
and [evdev implementation](https://github.com/torvalds/linux/blob/master/drivers/input/evdev.c).

macOS matches IOHIDDevice services, with primary Generic Desktop keyboard/keypad
usages. Composite devices may need usage-pair expansion later. Matching dictionary
ownership transfers to IOServiceGetMatchingServices; owned iterator/services and
CF properties are released. Follow the
[Apple IOKit header](https://github.com/apple-oss-distributions/IOKitUser/blob/main/IOKitLib.h),
[implementation](https://github.com/apple-oss-distributions/IOKitUser/blob/main/IOKitLib.c)
and [HID keys](https://github.com/apple-oss-distributions/IOHIDFamily/blob/main/IOHIDFamily/IOHIDKeys.h).
Native CF property allocation precedes Rust text limits.

CoreAudio solo preparation queries
[the default output property](https://developer.apple.com/documentation/coreaudio/kaudiohardwarepropertydefaultoutputdevice)
without changing or opening the stream. This is the media output default,
separate from the system-alert output property. Solo metadata selection does
not prompt for devices; multiple-player assignment uses the same metadata
foundation later. Actual native acceptance remains deferred.
