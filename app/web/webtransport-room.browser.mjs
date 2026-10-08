/** Real generated-WASM/browser HTTP/3 room acceptance against owned local PKI.
 * Root provides HTTPS app/relay servers, trust profile and bounded server joins.
 * Required env: PUPPETEER_MODULE, CHROMIUM, BEATKERNEL_TEST_BROWSER_URL,
 * BEATKERNEL_TEST_WEBTRANSPORT_URL, BEATKERNEL_TEST_FORBIDDEN_BROWSER_URL,
 * BEATKERNEL_TEST_UNTRUSTED_WEBTRANSPORT_URL. No certificate/trust bypass.
 */
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const required = name => {
  const value = process.env[name];
  assert(value, `Actual WebTransport acceptance requires ${name}`);
  return value;
};
const require = createRequire(import.meta.url);
const puppeteer = require(required("PUPPETEER_MODULE"));
const appURL = required("BEATKERNEL_TEST_BROWSER_URL");
const relayURL = required("BEATKERNEL_TEST_WEBTRANSPORT_URL");
const forbiddenURL = required("BEATKERNEL_TEST_FORBIDDEN_BROWSER_URL");
const untrustedURL = required("BEATKERNEL_TEST_UNTRUSTED_WEBTRANSPORT_URL");
assert.notEqual(new URL(appURL).origin, new URL(forbiddenURL).origin);
const browser = await puppeteer.launch({ executablePath: required("CHROMIUM"), headless: true,
  userDataDir: process.env.BEATKERNEL_TEST_CHROMIUM_PROFILE,
  args: ["--no-sandbox"], timeout: 15000 });
const pages = [];
try {
  const page = await browser.newPage(); pages.push(page);
  page.setDefaultTimeout(15000);
  await page.goto(appURL, { waitUntil: "domcontentloaded", timeout: 15000 });
  const result = await page.evaluate(async address => {
    const wasm = await import("/app/web/pkg/beatkernel_bms_runtime.js");
    await wasm.default();
    const { BrowserRoomOwner } = await import("/app/web/room-owner.mjs");
    if (!isSecureContext || typeof WebTransport !== "function") throw Error("Actual secure browser WebTransport unavailable");
    const insist = (value, message) => { if (!value) throw Error(message); };
    const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
    const wait = async (label, predicate) => {
      const deadline = performance.now() + 10000;
      while (!predicate()) { if (performance.now() >= deadline) throw Error(`Timed out: ${label}`); await delay(5); }
    };
    const now = () => BigInt(Math.floor(performance.now() * 1000000));
    const owned = [];
    const observations = [];
    function identity(seed) {
      const library = new wasm.BrowserLibrary(4, 65536, 131072, 256);
      let game;
      try {
        const wav = new Uint8Array(44 + 64 * 2), view = new DataView(wav.buffer);
        const text = (offset, word) => wav.set(new TextEncoder().encode(word), offset);
        text(0, "RIFF"); view.setUint32(4, wav.length - 8, true); text(8, "WAVEfmt ");
        view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
        view.setUint32(24, 48000, true); view.setUint32(28, 96000, true);
        view.setUint16(32, 2, true); view.setUint16(34, 16, true); text(36, "data"); view.setUint32(40, 128, true);
        library.add_file("x.wav", wav);
        library.add_file("x.bms", new TextEncoder().encode("#TITLE Actual transport\n#BPM 120\n#WAV01 x.wav\n#00011:01\n"));
        const prepared = library.prepare_chart("x.bms", 48000, 2, seed, 65536, 131072, 4);
        game = new wasm.BrowserGame(prepared, 0n, 100000000n, 100000000n, 100000000n, 0n, new Uint32Array([0x11, 4]));
        return game.competition_identity();
      } finally { game?.free(); library.free(); }
    }
    const canonical = identity(41n), different = identity(42n);
    insist(canonical.length > 0 && different.some((byte, i) => byte !== canonical[i]), "Actual seed identities must differ");
    async function open(caseName, players, preroll, bytes = canonical, signal) {
      const events = { starts: [], progress: [], closed: [], snapshots: [] };
      const session = wasm.BrowserRoomClient.new_with_start(bytes, new Uint32Array(players), preroll);
      const owner = await BrowserRoomOwner.open(`${address}_${caseName}`, {
        session, now, signal, setupTimeoutMs: 10000, ioTimeoutMs: 10000,
        onSnapshot: snapshot => events.snapshots.push(snapshot),
        onStart: schedule => events.starts.push(schedule),
        onProgress: prefix => events.progress.push(prefix),
        onClose: error => events.closed.push({ code: error.code, operation: error.operation, message: String(error) }),
      });
      owned.push(owner);
      return { owner, events };
    }
    function words(players, tick) {
      const rows = new Uint32Array(players.length * 11);
      players.forEach((player, index) => {
        const at = index * 11;
        rows[at] = player;
        for (const [offset, value] of [[1, tick * 1000000000n], [3, tick * 4n], [5, tick], [7, tick], [9, tick * 2n]]) {
          rows[at + offset] = Number(value & 0xffffffffn); rows[at + offset + 1] = Number(value >> 32n);
        }
      });
      return rows;
    }
    function exact(actual, expected) { return actual.length === expected.length && actual.every((value, i) => value === expected[i]); }
    async function cohort(caseName) {
      const a = await open(caseName, [11, 12], 10000000n);
      await wait("first actual admission", () => a.owner.snapshot?.members.length === 1);
      insist(a.events.starts.length === 0, "Collecting must not schedule start");
      const b = await open(caseName, [21, 22], 25000000n);
      const peers = [a, b];
      await wait("complete collecting roster", () => peers.every(peer => peer.owner.snapshot?.members.length === 2));
      for (const peer of peers) {
        insist(peer.owner.snapshot.phase === 0, "Actual roster starts Collecting");
        insist(exact(peer.owner.snapshot.members[0].players, new Uint32Array([11, 12])) && exact(peer.owner.snapshot.members[1].players, new Uint32Array([21, 22])), "Roster order or player identity changed");
      }
      let refused = false; try { b.owner.requestSeal(); } catch { refused = true; }
      insist(refused, "Second participant must not seal");
      a.owner.requestSeal();
      await wait("frozen room", () => peers.every(peer => peer.owner.snapshot?.phase === 1));
      a.owner.requestReady();
      await wait("first full Ready write", () => peers.every(peer => peer.owner.snapshot.members[0].prepared));
      insist(peers.every(peer => peer.events.starts.length === 0), "Single Ready must not commit start");
      b.owner.requestReady();
      await wait("actual clock and committed start", () => peers.every(peer => peer.events.starts.length === 1));
      peers.forEach((peer, i) => {
        const start = peer.events.starts[0];
        insist(peer.owner.snapshot.phase === 2 && peer.owner.snapshot.deadlineNs === null, "Prepared snapshot invalid");
        insist(start.songTargetNs - start.targetNs === [10000000n, 25000000n][i], "Committed original preroll changed");
        insist(start.uncertaintyNs <= 100000000n, "Start admitted unbounded clock uncertainty");
      });
      return peers;
    }
    try {
      const peers = await cohort("complete");
      for (let tick = 1n; tick <= 2n; tick++) {
        for (const [index, peer] of peers.entries()) {
          await wait("original publication cadence", () => peer.owner.progressDue(false));
          insist(peer.owner.publishProgress(words(index === 0 ? [11, 12] : [21, 22], tick)), "Ordinary progress not admitted");
        }
        await wait("ordered exact remote prefix", () => peers.every((peer, i) => peer.events.progress.some(prefix =>
          prefix.sequence === tick && !prefix.finalPrefix && exact(prefix.words, words(i === 0 ? [21, 22] : [11, 12], tick)))));
      }
      peers.forEach((peer, i) => insist(peer.owner.publishProgress(words(i === 0 ? [11, 12] : [21, 22], 3n), true), "Final progress refused"));
      const receipts = await Promise.all(peers.map(peer => peer.owner.waitForLocalCompletion()));
      for (const [index, receipt] of receipts.entries()) {
        insist(receipt.localFinalWritten && receipt.localFinalAcknowledged && receipt.complete && !receipt.drainComplete, "Final writes/ACKs differ from drain evidence");
        insist(peers[index].owner.peerFinalAckWritten(peers[1 - index].owner.participant), "Peer final ACK was not fully written");
        insist(peers[index].events.progress.some(prefix => prefix.sequence === 3n && prefix.finalPrefix && exact(prefix.words, words(index === 0 ? [21, 22] : [11, 12], 3n))), "Final exact prefix lost");
        insist(exact(peers[index].events.progress.map(prefix => Number(prefix.sequence)), [1, 2, 3]), "Progress sequence duplicated/reordered");
      }
      const drained = await Promise.all(peers.map(peer => peer.owner.drain()));
      insist(drained.every(receipt => receipt.drainComplete && receipt.complete), "Genuine drain ACK missing");
      await Promise.all(peers.map(peer => peer.owner.close()));
      observations.push("room-roster-ready-clock-start-ordered-progress-finals-drain");

      const cancellation = await cohort("cancel");
      await cancellation[0].owner.leave();
      await wait("actual whole-room cancellation", () => cancellation.every(peer => peer.owner.closed));
      insist(cancellation.every(peer => !peer.owner.receipts.drainComplete), "Cancellation fabricated drain completion");
      await Promise.all(cancellation.map(peer => peer.owner.close()));
      observations.push("leave-cohort-cancellation-joined-owner-close");

      const honest = await open("identity", [1], 0n);
      await wait("honest identity admission", () => honest.owner.snapshot?.members.length === 1);
      let rejected;
      try { rejected = await open("identity", [9], 0n, different); }
      catch (error) { insist(["transport", "closed", "core"].includes(error.code), "Identity test failed outside actual transport refusal"); }
      if (rejected) {
        await wait("real identity rejection", () => rejected.owner.closed);
        insist(rejected.owner.participant === 0n && rejected.events.starts.length === 0, "Incompatible setup entered room/start");
        insist(rejected.events.closed.length > 0 && rejected.events.closed.every(error => error.code !== "timeout"), "Identity timeout is not explicit transport refusal");
      }
      insist(honest.owner.snapshot.members.length === 1 && !honest.owner.closed, "Foreign identity changed honest roster");
      await Promise.all([honest.owner.close(), rejected?.owner.close()]);
      observations.push("actual-game-seed-identity-refusal");

      const controller = new AbortController();
      const abortable = await open("abort", [1], 0n, canonical, controller.signal);
      await wait("abortable admission", () => abortable.owner.snapshot?.members.length === 1);
      controller.abort();
      await abortable.owner.close();
      insist(abortable.owner.closed && !abortable.owner.receipts.complete, "Abort did not fence and join owner");
      observations.push("original-abort-signal-joined-close");
      return observations;
    } finally {
      const results = await Promise.allSettled(owned.map(owner => owner.close()));
      if (results.some(result => result.status === "rejected")) throw Error("Actual browser owner cleanup did not join successfully");
    }
  }, `${relayURL}_${process.pid}`);
  assert.equal(result.length, 4);

  async function refused(sourceURL, transportURL, label) {
    const rejection = await browser.newPage(); pages.push(rejection);
    await rejection.goto(sourceURL, { waitUntil: "domcontentloaded", timeout: 15000 });
    const refusal = await rejection.evaluate(async address => {
      const { WebTransportChannel } = await import("/app/web/multiplayer-transport.mjs");
      let channel;
      try {
        channel = await WebTransportChannel.open(address, { setupTimeoutMs: 5000, ioTimeoutMs: 5000 });
        return { opened: true };
      }
      catch (error) { return { opened: false, operation: error.operation, code: error.code }; }
      finally { if (channel) await channel.close(); }
    }, transportURL);
    assert.equal(refusal.opened, false, `${label} acquired an HTTP/3 stream`);
    assert.equal(refusal.code, "transport", `${label} failed outside actual transport admission`);
    assert(["open", "remote"].includes(refusal.operation), `${label} failed outside opening-handshake/remote transport refusal`);
    await rejection.close();
  }
  await refused(forbiddenURL, `${relayURL}_${process.pid}_forbidden`, "Forbidden browser Origin");
  await refused(appURL, `${untrustedURL}_${process.pid}_trust`, "Untrusted relay certificate");
  console.log(JSON.stringify({ kind: "actual-http3-browser-room", checks: [...result, "forbidden-Origin", "untrusted-TLS"], passed: true }));
} finally {
  await Promise.allSettled(pages.map(page => page.close()));
  await browser.close();
}
