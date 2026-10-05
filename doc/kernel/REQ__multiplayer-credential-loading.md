# Portable QUIC credential preparation

Credential metadata and role validation belong to portable policy. Existing
multiplayer_quic::QuicCredentials remains a compatibility re-export. Policy
imports no filesystem, sockets, TLS implementation or native adapter.

A generic CredentialReadPort borrows original Path keys and returns owned bytes
with an opaque associated error. Complete metadata validation precedes any read.
Host reads certificate then key; join reads CA only and borrows the original
server name. Every response must contain 1..=1048576 bytes. The first validation,
read or byte-bound failure stops later acquisition; original read errors survive
without formatting, cloning or additional trait bounds. Acquired bytes are setup
data and are not copied by policy.

Native QUIC endpoints retain address validation before credential reads and use
the injected preparation path. Filesystem regular-file checks, bounded native
reads, certificate/key decoding, trust validation and socket binding remain in
the native adapter. Pure validation does not prove TLS correctness or file safety.
WebTransport preparation now reuses this reader contract through its
[separate policy](REQ__webtransport-preparation.md); its TLS and socket ownership
remain native.

Deferred fake-reader fixtures cover zero effects on invalid metadata, ordered
original keys, errors, byte bounds and original server-name borrowing. Assertions,
native IO, TLS/network acceptance and formal review/QA remain deferred. Compilation
alone does not establish these behavioral requirements.

Six independent fixture groups are authored, and the four compile-only
configurations exited zero after both writers stopped. See
[the implementation evidence](../changes/CHANGE__multiplayer-credential-loading.md).
