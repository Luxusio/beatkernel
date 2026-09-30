# ASIO source and build distribution policy

The user selected this policy on 2026-10-01: project-authored source remains MIT,
while a build incorporating the ASIO SDK under its GPLv3 option is distributed
under GPLv3 conditions. This selects the SDK's open-source licensing path and
supersedes the previous unresolved-license prerequisite. It does not change the
verification deferral or establish an implemented ASIO backend.

The user clarified that builds without ASIO are distributed under MIT conditions.

| Source or build | Project distribution conditions |
| --- | --- |
| Project-authored source | MIT |
| Build without ASIO SDK incorporation | MIT, retaining applicable third-party license notices |
| Build incorporating the ASIO SDK under GPLv3 | GPLv3 for the combined program, with Corresponding Source and retained component notices |

## Source licenses

Root [LICENSE](../../LICENSE) and workspace package metadata stay MIT for
project-authored Rust, examples, adapters and native bridge source. Original
third-party code retains its own licenses, attribution and copyright notices.
ASIO SDK files and SDK-derived copied/generated code are not relabeled MIT.
The SDK's [license](https://github.com/audiosdk/asio/blob/main/LICENSE.txt) offers
GPLv3 or a proprietary agreement; this project selects GPLv3 for SDK incorporation.
Individual SDK files can carry their own embedded licenses, which must be retained.

This permits downstream users to reuse independently supplied MIT portions under
MIT. It does not permit them to treat the ASIO-combined program as MIT-only.
GNU's [compatibility explanation](https://www.gnu.org/licenses/gpl-faq.en.html#WhatDoesCompatMean)
describes distribution of compatible code combinations under the applicable GPL
version. The [combination guidance](https://www.gnu.org/licenses/license-compatibility.en.html)
also preserves the licenses attached to individual portions.

## Combined build distribution

ASIO SDK incorporation must be an explicit optional build choice, outside the
core's OS-independent dependency boundary. Non-ASIO builds are distributed under
MIT conditions, with applicable third-party notices retained. They do not acquire
GPLv3 conditions merely because the ASIO option exists in the repository. An ASIO
combined build must be labeled and distributed under GPLv3 conditions, including
its GPLv3 license text and retained MIT/third-party notices. The verbatim
[GPLv3 text](licenses/GPL-3.0.txt) accompanies this policy; including that text
does not change the license of project-authored source.

Binary release delivery must provide the applicable Corresponding Source through
a GPLv3-compliant distribution method. For reproducible identification, retain
the exact project revision, SDK revision, any patches and generated source,
necessary build/configuration scripts and instructions, and other source/build
inputs required by GPLv3, subject to its applicable exceptions. Source can be
offered alongside the binary download; an upstream link alone is not the project's
source-delivery contract. GPLv3 obligations apply to the combined program, not
only to an SDK folder. Merely moving a linked backend to another crate or DLL
does not establish a separate work or remove those obligations.

The GPLv3 [Corresponding Source and object-code terms](https://www.gnu.org/licenses/gpl-3.0.html)
govern release delivery. MIT license notices must remain with MIT portions.
No release is claimed compliant solely because Cargo package metadata says MIT,
an optional feature is used, or a GPL license file is present.

## Current implementation status

Default builds incorporate no ASIO SDK. The optional `asio-sdk` feature compiles
an original Windows C++ [driver control bridge](REQ__asio-driver-control.md)
against caller-supplied SDK headers on MSVC targets; those enabled combined
artifacts require GPLv3 distribution conditions. SDK source is not vendored or
downloaded by the build script. Preserve the actual supplied SDK revision and
source, including licenses, when distributing combined artifacts.
No ASIO native output backend is implemented yet.
The separate [Windows driver discovery](REQ__asio-driver-discovery.md) path uses
read-only registry APIs without incorporating SDK code or opening drivers.
There is no ASIO-combined release artifact or executed native-control evidence.
The feature's source/build path does not claim stream support.
WASAPI requests for ASIO now return the existing typed
`BackendUnavailable(Asio)` failure. The old public `AsioLicenseUnresolved` variant
is retained for existing callers, but current WASAPI code no longer emits it.

The policy prerequisite is resolved; remaining ASIO work is native stream implementation,
build/source provenance and actual output acceptance. Future SDK incorporation
must preserve third-party licensing boundaries described here. Native execution,
tests, independent review and QA remain deferred by the existing user instruction.
