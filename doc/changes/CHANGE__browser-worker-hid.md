# Worker HID profile setup and gameplay forwarding

The actual gameplay Worker accepts bounded numeric HID setup beside live
physical keyboard or contact input. Constructor bindings preserve exact source
and control words and can cover prepared lanes using HID without keyboard
bindings. The Rust profile owner is configured before preparation publication,
capture or activation. Mixed batches preserve original acquisition metadata and
use the canonical raw-HID encoder and actual HID gameplay entrypoint.

Source implementation and six independently authored deferred fixture groups
are present; all 34 existing Worker groups remain. The four allowed Rust cargo
checks completed with exit code zero, retaining existing WASM cadence warnings.
No Rust source changed in this slice; those checks provide no JavaScript
execution evidence. Whitespace checks completed. JavaScript parsing, tests,
browser/device/runtime execution, generated
bindings, formal review and QA remain deferred. Actual Window permission/profile
UI, launch forwarding and disconnect handling remain required full player work.
