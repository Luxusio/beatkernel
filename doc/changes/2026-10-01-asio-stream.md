# ASIO owned buffers and Mixer delivery

The optional Windows ASIO stream source now consumes an explicit native control
and actual Mixer, maps each Mixer channel to a distinct native output, resolves
exact buffer sizes, prepares SDK double buffers and primes B before one explicit
start. Callback admission/serialization and terminal cleanup keep native buffer
and Rust context lifetimes aligned. Reset/rate/buffer notifications require
explicit reconstruction; software prepared frames and raw native clock data stay
separate. The SDK-free block renderer preallocates scratch and preflights every
plane and the entire mixed block. Seven independently authored fixtures compile
without execution. Locked Rust 1.98.1 host workspace, SDK-free Windows GNU and
macOS platform all-target checks passed; metadata-only Windows Rust compilation
also type-checked optional wrapper source without building/linking SDK C++.
Actual SDK/MSVC ABI compilation, native playback, final-host composition,
presentation mapping, tests, independent reviews and QA remain outstanding.
Project-authored source and SDK-free builds remain MIT; SDK-combined artifacts
retain the documented GPLv3 distribution obligations. The full runtime Goal
remains active and the Harness task remains open.
