# Rust toolchain

Use the latest stable Rust release when explicitly upgrading the project.
On 2026-09-30 this is Rust 1.98.1, confirmed by the official release page
https://blog.rust-lang.org/releases/1.98.1/ and rustup distribution metadata.

Pin the selected release in `rust-toolchain.toml`, workspace `rust-version`,
and the CI toolchain action together. The current minimum supported Rust is
1.98.1; CI checks that same version on Linux, Windows and macOS. Development
uses the minimal profile with rustfmt and Clippy. Update these declarations
together on subsequent upgrades; do not automatically advance the compiler
on every build.

Windows GNU cross-checks also need the matching `x86_64-pc-windows-gnu` standard
library, installed with `rustup target add x86_64-pc-windows-gnu`. Native Windows
execution remains a separate check from cross-compilation. Historical Rust
1.83 results document earlier observations and do not attest to the new release.

Verify an upgrade with compiler/tool versions, formatting, strict Clippy,
debug and release tests, documentation, and the Windows target check. Run
independent task review and QA before closing the active Harness task. Leave
user-owned `mise.toml` unchanged; the Rust toolchain file owns this selection.
