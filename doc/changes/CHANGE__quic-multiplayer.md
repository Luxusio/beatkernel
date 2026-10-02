# Common QUIC multiplayer transport

The user selected QUIC for multiplayer. The native application replaces its TCP
connection owner with one Quinn adapter shared by Windows, Linux and macOS.
The existing framed protocol, compatibility identity, readiness, clock probes,
software-start agreement, progress validation and final-prefix acknowledgement
remain common. There is no implicit TCP fallback or OS-specific protocol copy.

The existing network worker owns a current-thread Tokio runtime, endpoint and
one reliable ordered bidirectional stream. Short cancellation-safe reads/writes
drive the runtime; a quiet iteration also drives it rather than parking all
QUIC tasks. Setup polls the same pinned future with one deadline and stop flag.
The joining side writes identity first so the remotely accepted stream becomes
visible. Bounded flow windows, stream/handshake counts, no datagrams, finite
idle timeout and keepalive retain finite resource/lifecycle limits.

Host options are `--mp-cert PATH --mp-key PATH`; join uses
`--mp-ca PATH --mp-server-name NAME`. Shared CLI extraction and advanced Settings
use these exact fields. Drafts may omit required partners, but real startup
rejects incomplete or incompatible credentials. Regular PEM/DER files are
bounded to 1 MiB each. TLS 1.3, the application ALPN and certificate/server-name
validation are explicit; there is no insecure verifier or early-data mode.
TLS authenticates the server, not joining players or their reported scores.

Every connected-loop exit attempts bounded send-stream finish/drain before
endpoint close and retains the original logical error when one already exists.
Local writes and transport receipt do not imply remote application consumption.
The existing final-prefix peer ACK keeps its existing semantics and does not
silently require both peers to have final reports.

Quinn/Tokio are native-only application dependencies. The dependency-free core
and BMS adapter remain unchanged. Browser-only builds do not include native QUIC;
their native transport entry points fail explicitly. Browser multiplayer still
needs WebTransport over HTTP/3 and a compatible session endpoint. A raw QUIC ALPN
peer is not automatically a WebTransport server. The user selected this browser
transport direction explicitly; API/capability checks precede connection, and
controls/results retain reliable ordered streams. WebTransport implementation
and datagram optimization remain future work. Authored source remains MIT;
new published dependency notices are retained in the application's notice file.

## Verification status

Locked source checks succeeded for the Linux workspace, native headless app and
three WASM library configurations (graphics, browser, browser audio). Final
workspace/headless all-target checks also compiled six credential/parser/TLS
fixture groups and three ignored real native QUIC loopback tests. Final focused
host and browser-audio checks succeeded. Logs are under `target/ac180-*`.

Windows GNU and macOS checks stop at the new cryptography dependency's C build:
matching cross compiler/SDK tools are unavailable. An earlier Windows dependency
check used the inherited Linux compiler and produced an ELF object; its exit zero
is excluded from Windows evidence. No native cross-platform QUIC success is
claimed from that check.

The ignored integration tests use real public Multiplayer owners and require
explicit certificate/key/CA/server-name environment variables, as documented in
`doc/kernel/REQ__bms-competition.md`. Missing prerequisites fail when explicitly
run. They cover start/progress/final acknowledgements, certificate-name rejection
and incompatible identities. Assertions, certificate reads, socket/TLS/QUIC/
WebTransport execution and formal review/QA remain deferred. No runtime or
acceptance PASS is claimed. The full player Goal remains active.
