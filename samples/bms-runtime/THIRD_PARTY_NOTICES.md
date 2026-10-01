# Application dependencies

Project-authored code remains MIT. Direct application dependencies use
the licenses below; distributing a binary must preserve their applicable
notices and licenses. This does not introduce a GPL requirement for builds
without the optional ASIO SDK. Transitive packages retain their own licenses;
these direct notices are not a complete binary license inventory.

| Dependency | Selected license | Retained text |
| --- | --- | --- |
| wgpu 27.0.1 | MIT | [license](third-party/wgpu-27.0.1-LICENSE.txt) |
| winit 0.30.13 | Apache-2.0 | [license](third-party/winit-0.30.13-LICENSE.txt) |
| bytemuck 1.24.0 | MIT | [license](third-party/bytemuck-1.24.0-LICENSE.txt) |
| pollster 0.4.0 | MIT | [license](third-party/pollster-0.4.0-LICENSE.txt) |
| floem_reactive 0.2.0 | MIT | [license](third-party/floem-reactive-0.2.0-LICENSE.txt) |
| ab_glyph 0.2.32 | Apache-2.0 | [license](third-party/ab-glyph-0.2.32-LICENSE.txt) |
| encoding_rs 0.8.35 | MIT AND BSD-3-Clause | [MIT](third-party/encoding-rs-0.8.35-LICENSE-MIT.txt), [WHATWG](third-party/encoding-rs-0.8.35-LICENSE-WHATWG.txt) |
| claxon 0.4.3 | Apache-2.0 | [license](third-party/claxon-0.4.3-LICENSE.txt) |

The texts are copied from the pinned published packages. The floem_reactive
package declares MIT but omits the license text; its retained text comes from
`floem-0.2.0/LICENSE` in the published parent project's
[0.2.0 package](https://crates.io/crates/floem/0.2.0).
Only the standalone reactive engine is linked; the Floem window/widget host is
not a dependency. minifb is no longer included by this application.

Claxon includes Copyright 2014 Ruud van Asseldonk; its exact published Apache-2.0
license is retained above. The codec is linked only in the application preparation
layer, and is not a dependency of the core or BMS adapter.
