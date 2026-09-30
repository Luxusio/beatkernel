# ASIO mixer-channel PCM conversion

Added SDK-free conversion of explicit interleaved mixer channels into eighteen
ASIO planar PCM layouts. The converter handles both byte orders, full-width
integers, floats and reduced valid-bit 32-bit containers. It reuses the platform
integer quantizer, validates selected samples and exact extents before writing,
and allocates nothing by construction. DSD/unknown native type identities remain
explicit errors. Optional ASIO channel metadata exposes converter selection from
the reported sample type. Ten portable byte/extent/channel/allocation fixtures
are authored. Locked host-workspace, Windows GNU platform and macOS x86-64 platform
all-target compilation passed; no fixtures, native output, review or QA ran.
SDK-enabled Windows source remains uncompiled locally. ASIO native buffer
creation, callbacks, Mixer delivery and owned stream lifecycle remain required;
the full-plan Goal stays active.
