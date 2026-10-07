# Domain modules inside the single BMS application

## Application directory

The production player belongs in the root `app/` directory. The user selected
this on 2026-10-07 because the application is not a sample. Move the former
`samples/bms-runtime` tree and update Cargo membership, relative dependencies,
test/build paths and current documentation links together. Keep one application
crate with its existing package, library and executable identities; directory
naming does not require extra crates or a public API rename. Core and native
libraries remain under `crates/`, and the BMS format adapter under `adapters/`.
Do not keep a legacy sample-path alias or recovery copy.

On 2026-10-06 the user approved organizing the application by domain to improve
maintenance, independent testing and separation of concerns. Keep the existing
core/platform/BMS-adapter/app crate boundaries and organize the app internally.
DDD describes domain ownership; hexagonal ports describe effect boundaries. They
are complementary. Do not add repositories, dynamic dispatch, per-note objects,
heap allocations or locks merely to fit an architectural template.

The intended app contexts are gameplay, content, competition and settings, with
presentation and native effects in adapters. Migrate cohesive groups gradually,
preserving behavior and existing public paths with compile-time re-exports.
Existing app modules are not all migrated or perfectly isolated yet.

The first group is `gameplay/output`:

- `domain/control.rs`: output capabilities, requests, replies and bounded command
  state. No platform calls, Player synchronization or rendering.
- `ports.rs`: statically supplied output lifecycle and command/reply contracts.
- `application/`: output replacement, ownership and correlated request handling.
  Uses domain/ports and shared timing primitives; does not call Player or native
  backends directly.
- `adapters/`: Player command synchronization plus ALSA/WASAPI/CoreAudio/ASIO
  lifecycle, request conversion and original observation projection. Platform
  cfg gates belong here and in the composition root.

Legacy root modules remain thin re-exports of the same types and implementations;
they contain no second state machine or forwarding runtime objects. The new
domain path is canonical for internal dependencies. Presentation remains in
`ui/` and the desktop composition root. Wider gameplay timing/session and other
context migrations remain follow-up work.

Keep existing fixture assertions and genuine Mixer/controller/pump evidence.
Move or register fixtures beside their owning implementation without changing
their meaning to obtain green results. Compare failures against the known
baseline after accounting for changed module prefixes. Run host and WASM checks,
focused output tests, the main executable tests and available cross-target Rust
type checks. Rust-only stub cross-checks do not establish native ABI/hardware
acceptance. Full independent review/QA remains required before task close.

One Rust crate does not by itself enforce every sibling dependency direction.
Explicit ports and implementation imports must maintain the boundary; folder
names alone are not evidence of complete hexagonal isolation or zero overhead.
