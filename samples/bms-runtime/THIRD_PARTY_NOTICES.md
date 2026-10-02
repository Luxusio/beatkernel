# Application dependencies

Project-authored code remains MIT. Direct application dependencies use
the licenses below; distributing a binary must preserve their applicable
notices and licenses. This does not introduce a GPL requirement for builds
without the optional ASIO SDK. Transitive packages retain their own licenses;
these direct notices are not a complete binary license inventory.

## Native QUIC multiplayer

Native targets use Quinn 0.11.12 with Tokio 1.53.1 and rustls/ring TLS, outside
the dependency-free kernel and browser-only WASM builds. Authored source remains
MIT. This introduces no GPL requirement and does not change the optional ASIO
distribution split. Exact published notices for the newly resolved packages
are retained under `third-party/` with versioned filenames.

| Dependency | Selected license | Retained text |
| --- | --- | --- |
| quinn 0.11.12 | MIT | [license](third-party/quinn-0.11.12-LICENSE-MIT.txt) |
| quinn-proto 0.11.19 | MIT | [license](third-party/quinn-proto-0.11.19-LICENSE-MIT.txt) |
| quinn-udp 0.5.16 | MIT | [license](third-party/quinn-udp-0.5.16-LICENSE-MIT.txt) |
| tokio 1.53.1 | MIT | [license](third-party/tokio-1.53.1-LICENSE.txt) |
| rustls 0.23.45 | MIT | [license](third-party/rustls-0.23.45-LICENSE-MIT.txt) |
| rustls-pki-types 1.15.1 | MIT | [license](third-party/rustls-pki-types-1.15.1-LICENSE-MIT.txt) |
| rustls-webpki 0.103.15 | ISC | [license](third-party/rustls-webpki-0.103.15-LICENSE.txt) |
| ring 0.17.14 | Apache-2.0 AND ISC | [umbrella](third-party/ring-0.17.14-LICENSE.txt), [BoringSSL](third-party/ring-0.17.14-LICENSE-BoringSSL.txt), [other bits](third-party/ring-0.17.14-LICENSE-other-bits.txt), [fiat](third-party/ring-0.17.14-fiat-LICENSE.txt) |
| untrusted 0.9.0 | ISC | [license](third-party/untrusted-0.9.0-LICENSE.txt) |
| mio 1.2.3 | MIT | [license](third-party/mio-1.2.3-LICENSE.txt) |
| subtle 2.6.1 | BSD-3-Clause | [license](third-party/subtle-2.6.1-LICENSE.txt) |

New chacha20 0.10.2, cpufeatures 0.3.1, getrandom 0.2.17, lru-slab 0.1.3,
rand 0.10.3, rand_core 0.10.1, rand_pcg 0.10.2, rustc-hash 2.1.3, socket2 0.6.5
and zeroize 1.9.0 use their MIT options; their exact `LICENSE-MIT` texts are
retained with those versioned prefixes. This records the newly resolved native
packages; binary redistributors must still inventory the actual linked graph.

## Other application dependencies

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

## Native text clipboard

The optional desktop dependency arboard 3.6.1 has default features disabled;
only native text clipboard and Wayland data-control support are enabled. Core,
BMS adapter, headless player and browser graphics do not depend on this adapter.
The lockfile adds the following packages without upgrading existing packages.

| Dependency | Selected license | Retained text |
| --- | --- | --- |
| arboard 3.6.1 | MIT | [license](third-party/arboard-3.6.1-LICENSE-MIT.txt) |
| clipboard-win 5.4.1 | BSL-1.0 | [license](third-party/clipboard-win-5.4.1-LICENSE-BSL.txt) |
| error-code 3.4.0 | BSL-1.0 | [license](third-party/error-code-3.4.0-LICENSE-BSL.txt) |
| fixedbitset 0.5.7 | MIT | [license](third-party/fixedbitset-0.5.7-LICENSE-MIT.txt) |
| nom 8.0.0 | MIT | [license](third-party/nom-8.0.0-LICENSE-MIT.txt) |
| os_pipe 1.2.3 | MIT | [license](third-party/os_pipe-1.2.3-LICENSE-MIT.txt) |
| petgraph 0.8.3 | MIT | [license](third-party/petgraph-0.8.3-LICENSE-MIT.txt) |
| tree_magic_mini 3.2.2 | MIT | [license](third-party/tree_magic_mini-3.2.2-LICENSE-MIT.txt), [database licensing explanation](third-party/tree_magic_mini-3.2.2-README.md) |
| wl-clipboard-rs 0.9.4 | MIT | [license](third-party/wl-clipboard-rs-0.9.4-LICENSE-MIT.txt) |
| objc2 0.6.4 | MIT | [upstream licensing declaration](third-party/objc2-0.6.4-dispatch2-0.3.1-LICENSING.md) |
| dispatch2 0.3.1 | MIT | [upstream licensing declaration](third-party/objc2-0.6.4-dispatch2-0.3.1-LICENSING.md) |
| objc2-app-kit 0.3.2 | MIT | [upstream licensing declaration](third-party/objc2-frameworks-0.3.2-LICENSING.md) |
| objc2-core-foundation 0.3.2 | MIT | [upstream licensing declaration](third-party/objc2-frameworks-0.3.2-LICENSING.md) |
| objc2-core-graphics 0.3.2 | MIT | [upstream licensing declaration](third-party/objc2-frameworks-0.3.2-LICENSING.md) |
| objc2-foundation 0.3.2 | MIT | [upstream licensing declaration](third-party/objc2-frameworks-0.3.2-LICENSING.md) |
| objc2-io-surface 0.3.2 | MIT | [upstream licensing declaration](third-party/objc2-frameworks-0.3.2-LICENSING.md) |

Package-provided texts above are copied verbatim. clipboard-win omits its text
from the published archive; the retained BSL text comes from its published VCS
revision [3b27cf2](https://github.com/DoumanAsh/clipboard-win/blob/3b27cf2bfd1adcfa6e0264eb51c1025ddaf0f342/LICENSE).
The objc2/dispatch2 declarations come from published VCS revision
[8852b424](https://github.com/madsmtm/objc2/blob/8852b424193ca41602281b3d7540d7c8ed51e49a/LICENSE.md);
the framework declarations come from
[7b1abfd7](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md).
Those upstream revisions provide a license declaration and Apple SDK provenance
note, but omit a package-specific full MIT grant and copyright text. Preserve
the exact upstream statements; this inventory does not invent that attribution
or establish complete release-packaging license compliance.

tree_magic_mini's optional `with-gpl-data` feature and `tree_magic_db` package
are absent from this dependency resolution. No GPL MIME database is embedded or
redistributed here; any separately supplied system MIME data retains its own
terms. arboard's text write selects an explicit text MIME type. Petgraph artwork
is not distributed by this application. Project-authored code remains MIT and
the separate optional ASIO SDK distribution policy is unchanged.
