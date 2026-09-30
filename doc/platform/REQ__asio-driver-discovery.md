# Windows ASIO driver registration discovery

ASIO host selection begins with explicit registry discovery, separate from
opening a driver or proving output support. Steinberg's [host example](https://github.com/audiosdk/asio/blob/main/host/pc/asiolist.cpp)
identifies this registry location and its CLSID/Description metadata; no example
implementation is incorporated here. The Windows platform module exposes
`enumerate_asio_drivers(view, limits)` for `HKLM\SOFTWARE\ASIO`. No SDK or
SDK-derived code is incorporated by this registration-only path. Project-authored
source and non-SDK builds retain the [MIT distribution policy](REQ__asio-distribution.md).

The caller selects Native, Bits32 or Bits64 registry view; discovery does not
merge architectures or choose a default driver. Each registration retains its
name, optional Description and canonical nonzero CLSID with the selected view.
Names and descriptions preserve Unicode. CLSIDs use brace-wrapped uppercase
hexadecimal with exact UUID separators. A registration proves an installed
registry entry, not driver loadability, process-bitness compatibility, device
availability, buffer support or successful ASIO output.

Limits are validated before native access: driver count 1..4096 and REG_SZ value
storage 2..32768 UTF-16 units, including the terminating NUL. Defaults are 256
drivers and 4096 units. Fixed subkey storage follows Windows' 255-unit key-name
limit. Configured bounds and fallible buffer/result reservations precede native
reads; registry-reported lengths do not authorize unbounded allocation. Nonempty
names reject NUL and path separators. Description is optional; CLSID is required.
REG_SZ reads require a valid UTF-16 extent and final NUL inside the returned
byte count; missing termination is rejected rather than scanning beyond it.
Malformed strings/types, inaccessible entries, capacity exhaustion and concurrent
query errors are explicit, rather than silently returning a selected subset.
An absent ASIO root yields an empty list. Results are sorted by registration name
and canonical CLSID; discovery is not a transactional registry snapshot.

Registry handles are scoped owners and are closed on all exits. The implementation
uses read-only Win32 calls; it does not initialize COM, load DLLs, alter registry
entries or obtain an audio clock. Microsoft documents [enumeration extents](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regenumkeyexw)
and [value types and returned byte extents](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regqueryvalueexw).

The `asio_inspector` platform example requires `--view native|32|64` when
enumerating; optional count/value caps remain explicit. Its portable help path
does not enumerate or open audio. Constructor, identity and limit fixtures are
authored and compiled only. Native enumeration, fixture execution and independent
review remain deferred. SDK-combined bridge builds and actual driver streaming
are subsequent required ASIO work, not claimed by this discovery API.

Rust 1.98.1 locked all-target checks passed for the host workspace and the Windows
GNU platform target. The seven registration fixtures, three private returned-
extent fixtures and two inspector parser fixtures compiled without execution.
Initial Windows compilation identified a missing scoped native-FFI lint allowance;
the registry module now declares that boundary like existing Windows modules.
