# Strict browser WebTransport room acceptance

Use current main/audio WASM, the existing HTTP/3 relay command and separate
focused Chromium processes for actual Main gameplay. Preserve product deadlines,
real user Play gestures, input policies and production bytes. The existing
`app/web/webtransport-room.browser.mjs` proves direct protocol behavior with
constructed rows; it does not prove natural gameplay completion.

Prepare owned HTTPS app and forbidden-Origin servers, an allowed-origin relay
in `serve-multiplayer --group-hosts 2` mode and an unrelated-CA negative relay.
Serve exact app assets with correct MIME and COOP/COEP headers. Caller-owned
servers and displays outlive the test runner; terminate only owned resources.
Keep ephemeral keys ignored and never print their content.

Use installed Chromium/OpenSSL/NSS libraries. A private `XDG_DATA_HOME/pki/nssdb`
can isolate local NSS trust if the legacy home NSS database is absent; verify
that prerequisite and never repurpose HOME or change user/system trust.
[Chromium's NSS path selection](https://chromium.googlesource.com/chromium/src/crypto/+/refs/heads/main/nss_util.cc)
describes this lookup. Confirm ordinary HTTPS acceptance and unrelated-CA refusal.

HTTPS success is insufficient for WebTransport. Chromium's
[QUIC parameters](https://chromium.googlesource.com/chromium/src/net/+/refs/heads/main/quic/quic_context.h)
and [proof verifier](https://chromium.googlesource.com/chromium/src/net/+/refs/heads/main/quic/crypto/proof_verifier_chromium.cc)
distinguish a known system root from a user-installed CA. Preserve one bounded
actual WebTransport acquisition attempt and NetLog. An unknown-root rejection
is an environment prerequisite failure, not permission to add certificate-ignore,
SPKI-ignore, developer mode, forced-QUIC origins or injected certificate hashes.
Keep trusted acceptance unfinished when no eligible endpoint is available.

Two natural completions, genuine keyboard judging and both raw final write/ACK/
drain receipts are required. Passive Worker observations are diagnostic evidence;
screenshots must be inspected separately. Full WBS12.04 also requires native/
browser interoperability. Do not infer physical synchronization from software
start schedules or promote protocol-only tests to gameplay acceptance.

## Current bounded preparation evidence — 2026-10-10

At source `072a21a`, installed Chromium153 and NSS libraries prepared a private
XDG NSS store without changing HOME or system trust. Ordinary HTTPS accepted
the imported CA and refused an otherwise valid unrelated CA with
`CERT_AUTHORITY_INVALID`. An actual cached native relay ran with group-hosts2.
The unchanged production WebTransportChannel refused actual stream acquisition
as transport/remote, with an opening-handshake error. QUIC NetLog source236
records certificate unknown/CERTIFICATE_VERIFY_FAILED; the ordinary accepted
certificate chain has cert_status0 and is_issued_by_known_root=false. The trace
does not expose an explicit -380 event, so that exact error code is not claimed.

This is a bounded environment failure, not a successful protocol/gameplay test.
No certificate bypass or additional launch permutation followed. Existing
room-owner/transport Node tests pass47/0; they do not replace actual HTTP/3.
Preflight Node exited0 to report its diagnostic result, not acceptance PASS.
Owned Chromium3034860 and relay3034858 exited0, HTTPS servers closed and profile/
configuration directories were removed. Browser.close exceeded its observation
deadline before bounded owned process termination completed; graceful close is
not claimed. The private NSS database and certificates remain ignored evidence.

Artifacts are machine-local at `target/wf/webtransport-player-01a1247a/`:
`prepare.py`, `preflight.mjs`, `preflight-evidence.json`, `netlog.json` and compact
trust summaries. The initial NSS preparation used an unavailable exported symbol;
the corrected existing PK11_ImportCert/CERT_ChangeCertTrust APIs succeeded.
This setup correction did not change production transport or trust policy.
Actual strict room gameplay needs an eligible system-trusted HTTP/3 endpoint;
the prepared actual-play runner remains unexecuted until that prerequisite holds.

## Runnable production-page fixture

Run `node app/web/webtransport-play.browser.mjs` after the strict endpoint
prerequisite is satisfied. Required environment variables are:

- `WEBTRANSPORT_PLAY_APP_URL`: trusted production app navigation URL.
- `WEBTRANSPORT_PLAY_ROOM_URL`: trusted HTTP/3 room URL.
- `CHROMIUM` and `PUPPETEER_MODULE`: installed executable and module paths.
- `WEBTRANSPORT_PLAY_HOSTS`: JSON array of exactly two hosts, each with
  `display`, `profile`, `xdgDataHome` and `xdgConfigHome`.
- `WEBTRANSPORT_PLAY_HASHES`: path to a JSON map from `app/web/<artifact>`
  to its current SHA256. `WEBTRANSPORT_PLAY_OUT` optionally selects evidence output.

Profiles must initially be absent and physically disjoint, including through
symlink aliases; trust/config directories must already exist. The runner owns
the new profiles and two private 16MiB tmpfs caches and removes them after
owned browser processes terminate. Caller-owned endpoints and displays remain
the caller's responsibility. Actual navigation response bytes are hash-checked,
including directory URLs; Results require correlated RPC/state ACK/draw and
positive geometry. A late operation cannot turn an expired deadline into PASS.

Independent CLI QA at source `9ce2670` reports 68/68 tests, zero failures/skips:

```sh
node --experimental-vm-modules --test app/web/webtransport-play.test.mjs app/web/room-owner.test.mjs app/web/multiplayer-transport.test.mjs
```

This includes21 new portable gate cases and47 existing owner/transport cases.
Both new scripts pass `node --check`. Actual entrypoint execution without the
required app URL exits1 before browser launch and preserves failure evidence.
Independent browser QA inspected raw NetLog and returned BLOCKED_ENV; it
launched no new browser. These checks do not establish protocol or gameplay
acceptance. Both natural completions and native/browser interoperability remain
outstanding.
