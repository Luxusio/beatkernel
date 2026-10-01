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
| lewton 0.10.2 | MIT | [license](third-party/lewton-0.10.2-LICENSE.txt) |
| ogg 0.8.0 (container reader) | BSD-3-Clause | [license](third-party/ogg-0.8.0-LICENSE.txt) |
| tinyvec 1.13.3 (codec dependency) | MIT | [license](third-party/tinyvec-1.13.3-LICENSE.txt) |
| byteorder 1.5.0 (codec dependency) | MIT | [license](third-party/byteorder-1.5.0-LICENSE.txt) |
| nanomp3 0.2.0 | MIT | [license](third-party/nanomp3-0.2.0-LICENSE-MIT.txt) |
| nanomp3-core 0.2.0 (codec dependency) | MIT | [license](third-party/nanomp3-core-0.2.0-LICENSE-MIT.txt) |

The texts are copied from the pinned published packages. The floem_reactive
package declares MIT but omits the license text; its retained text comes from
`floem-0.2.0/LICENSE` in the published parent project's
[0.2.0 package](https://crates.io/crates/floem/0.2.0).
Only the standalone reactive engine is linked; the Floem window/widget host is
not a dependency. minifb is no longer included by this application.

Claxon includes Copyright 2014 Ruud van Asseldonk; its exact published Apache-2.0
license is retained above. The codec is linked only in the application preparation
layer, and is not a dependency of the core or BMS adapter.

Lewton and the newly resolved codec dependencies above are linked in application
preparation only. Their exact published license texts are retained; Ogg's BSD
notice remains applicable to redistribution. These codecs introduce no GPL
dependency. Project-authored source remains MIT.

The MP3 packages use the selected MIT terms above. Only their scalar MPEG
Layer III decoder and metadata functions are linked, without the optional
SIMD, Layer I/II, allocation-backed readers or native C build. Their exact
published MIT notices are retained. Core and BMS adapter dependencies do not
include these application codecs.

## Static image preparation codecs

The image dependency is pinned with default features disabled and only BMP,
PNG and JPEG enabled. The following new resolved package texts are retained
from their published packages; previously resolved dependencies keep their
existing notices. Main project code remains MIT.

| Dependency | Selected license | Retained text |
| --- | --- | --- |
| adler2 2.0.1 | MIT | [license](third-party/adler2-2.0.1-LICENSE-MIT.txt) |
| byteorder-lite 0.1.0 | MIT | [license](third-party/byteorder-lite-0.1.0-LICENSE-MIT.txt) |
| crc32fast 1.5.2 | MIT | [license](third-party/crc32fast-1.5.2-LICENSE-MIT.txt) |
| fdeflate 0.3.7 | MIT | [license](third-party/fdeflate-0.3.7-LICENSE-MIT.txt) |
| flate2 1.1.10 | MIT | [license](third-party/flate2-1.1.10-LICENSE-MIT.txt) |
| image 0.25.10 | MIT | [license](third-party/image-0.25.10-LICENSE-MIT.txt) |
| miniz_oxide 0.8.9 | MIT | [license](third-party/miniz_oxide-0.8.9-LICENSE-MIT.txt) |
| miniz_oxide 0.9.1 | MIT | [license](third-party/miniz_oxide-0.9.1-LICENSE-MIT.txt) |
| moxcms 0.8.1 | BSD-3-Clause | [license](third-party/moxcms-0.8.1-LICENSE.txt) |
| png 0.18.1 | MIT | [license](third-party/png-0.18.1-LICENSE-MIT.txt) |
| pxfm 0.1.30 | BSD-3-Clause | [license](third-party/pxfm-0.1.30-LICENSE.txt) |
| simd-adler32 0.3.10 | MIT | [license](third-party/simd-adler32-0.3.10-LICENSE.txt) |
| zlib-rs 0.6.8 | Zlib | [license](third-party/zlib-rs-0.6.8-LICENSE.txt) |
| zune-core 0.5.3 | MIT | [license](third-party/zune-core-0.5.3-LICENSE-MIT.txt) |
| zune-jpeg 0.5.15 | MIT | [license](third-party/zune-jpeg-0.5.15-LICENSE-MIT.txt) |

The miniz_oxide packages also retain their upstream umbrella LICENSE files
beside the selected MIT text, including provenance for the original miniz
implementation. This list covers new packages introduced by this phase, not
a complete inventory of all transitive packages in every platform binary.
