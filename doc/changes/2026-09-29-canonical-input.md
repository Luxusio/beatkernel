# Canonical physical input

Phase 2 adds device descriptors and session identity, HID/native/vendor physical
controls, seven typed event variants, adapter/sink interfaces and a virtual FIFO.
Clock normalization preserves original and native provenance; equal acquisition
sequences allow report fanout. Invalid input leaves queue/order state unchanged,
and retired device IDs stay reserved while accepted events remain deliverable.
Pure Windows/Linux/macOS keyboard fixtures converge to HID usages, unknown codes
remain lossless, and a virtual inspector demonstrates separate sources. CI runs
the example. [The contract](../kernel/REQ__canonical-input.md) defines semantics
and verification; device-aware binding and native acquisition follow in plan.md.

## Known ceiling

Known ceiling: Windows scan 0x2B collapses HID usages 0x31/0x32 and conventionally maps to 0x31 — upgrade when acquisition supplies original HID usage.

Known ceiling: Pure normalizers require complete Pause sequences and perform no native acquisition — upgrade when Phase 4 implements packet assembly and native I/O.

The virtual example performs no hardware latency measurement. Native device
verification requires execution on each backend's target operating system.

Known ceiling: virtual queue registration and enqueue may allocate — upgrade
when the runtime needs a bounded queue at a real-time boundary.
