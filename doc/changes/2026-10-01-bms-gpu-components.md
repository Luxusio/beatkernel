# Native GPU components for the first BMS application

The BMS app now composes a winit main-thread window with a reusable wgpu
renderer, replacing the initial uncommitted CPU framebuffer implementation.
Geometry/text atoms build note/hold/counter molecules, which build playfield
and scoreboard organisms; the desktop composes these from actual game-owner
snapshots. Native input, judging and audio retain their owners, and close/focus
loss cancel play before another session starts. The graphics feature exposes
portable components and async GPU initialization for later WASM reuse. This is
the first application of the Rust cross-platform rhythm-game engine foundation,
not a completed general engine UI API, full BMS player or browser player.

The [player contract](../kernel/REQ__bms-player.md) owns component construction,
backend/presentation controls, threading, browser boundaries and known ceilings.
Source checks compile authored fixtures without executing them; actual GPU
shader/rendering, lifecycle/input/audio and independent review/QA remain deferred
under the user's explicit sequencing. Authored source remains MIT; dependency
licenses, including winit Apache-2.0, are preserved in application notices.

wgpu is pinned to 27.0.1 after published 30.0.1 failed Windows DX12 source
compilation due to HAL/allocator Windows binding version differences. No backend
was removed and no registry source was patched. Future upgrades must compile
all native targets and the portable WASM renderer before replacing this pin.
