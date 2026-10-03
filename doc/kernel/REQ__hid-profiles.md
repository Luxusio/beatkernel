# Shared HID report profiles

Declare device report interpretation once using the common DeviceAdapter and
AdapterRegistry boundary, independent of native or browser acquisition. Explicit
profiles select optional vendor/product identity and require raw HID capability.
Each attached device receives independent retained decoder state. This does not
guess USB descriptors or claim every HID device has the same report format.

A profile contains one to 256 distinct separate report IDs (None for unnumbered,
nonzero byte IDs for numbered) and one to 256 total fields. Report payload extent
is exact, zero to 1024 bytes. Individual empty report field lists are allowed.
All output physical controls are unique across the profile. Different controls
may deliberately read overlapping bits. Fields use one to 64 bits fully inside
the declared payload and specify bit order explicitly. Least-significant-first
counts from the first byte low bit and places the first stream bit at value bit
zero. Most-significant-first counts from the high bit and places the first stream
bit at the field value's highest bit. Preserve cross-byte and 64-bit fields.

Buttons interpret nonzero as pressed with optional inversion. First false levels
do not invent Up; first true emits Down. Later changed levels emit Down/Up and
unchanged levels emit nothing. Absolute axes emit first or changed finite core
f32 values; relative axes emit every report, including repeated/zero deltas.
Signed values use declared-width two's complement. Explicit finite scale/offset
sets axis units; use wide intermediate arithmetic and reject nonfinite core f32
results. Axis conversion retains core float precision, not exact 64-bit integers.

Validate all selected report fields before emitting or committing levels. Wrong
length, invalid converted values and a different attached source refuse the
whole report without changing state. Unknown IDs produce no events or source
adoption. Reset clears decoder state/source without synthesizing judge releases.
All derived events retain full original report metadata, including equal sequence
for fanout, clocks and native provenance; never fabricate keyboard controls.

Fixed bounded decoder state/scratch avoid allocation of decoder storage during
report interpretation. Caller sinks/registry may allocate separately. Typed
decoding returns refusal details; the DeviceAdapter callback safely emits nothing
for malformed reports. Valid state commits before infallible sink publication;
caller panics or registry emission-cap refusal do not promise decoder rollback.

Existing native and browser acquisition must share this implementation when
configured. Actual page/Worker forwarding, profile selection and logical lane
bindings still require integration. Fixtures and compile checks cannot establish
actual device, browser, driver, runtime or performance acceptance.
