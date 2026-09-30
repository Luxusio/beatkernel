# Explicit ASIO clock-source controls

ASIO control source now exposes bounded SDK clock-source enumeration and exact
selection before stream construction, separately from rate-zero external sync.
SDK-free metadata retains raw names, actual indices, current flags and input
channel/group associations, rejecting malformed reports and capacity overflow.
Selection checks actual reported membership, preserves native errors and never
changes another source or rate implicitly. Time-info clock/rate-change flags
fault running streams for explicit reconstruction. Native timestamp decoding
supports SDK integral and high/low representations. Eight portable fixtures
are authored; execution and SDK/C++ native acceptance remain deferred. The full
runtime Goal stays active; live ASIO host presentation and deferred full-plan
acceptance remain outstanding. MIT defaults and GPLv3 SDK-combined distribution
remain unchanged.

Rust 1.98.1 locked host workspace and default Windows GNU/macOS platform and
sample all-target checks passed. Target-only optional Windows Rust source
checks passed without activating the Cargo SDK feature or compiling/linking
C++. These checks establish Rust type compatibility only; actual SDK ABI,
driver controls and native output remain unverified.
