/**
 * Strict actual Main + Worker + AudioWorklet two-host acceptance.
 * Caller owns trusted HTTPS/HTTP3 endpoints, displays and disposable trust dirs.
 * Required: WEBTRANSPORT_PLAY_APP_URL, WEBTRANSPORT_PLAY_ROOM_URL, CHROMIUM,
 * PUPPETEER_MODULE, WEBTRANSPORT_PLAY_HOSTS (JSON array of exactly two objects:
 * {display,profile,xdgDataHome,xdgConfigHome}), WEBTRANSPORT_PLAY_HASHES (path to
 * JSON object mapping app/web/<artifact> to SHA256). Manifest must cover every
 * loaded production JS/MJS/WASM/HTML/CSS artifact, including both WASM packages.
 * Profiles must not exist; trust dirs must already exist. HOME is never changed.
 * Optional WEBTRANSPORT_PLAY_OUT. No caller-supplied browser flags are accepted.
 * Generates the original render-integration long.bms/x.wav fixture; observes
 * production messages without manufacturing progress, starts or completion.
 * Exit 0 requires both natural completions, real hits and actual final ACK/drain.
 * This file writes development evidence only, never Harness receipts.
 */
import assert from "node:assert/strict";
import { readFile, writeFile, mkdir, stat, realpath, mkdtemp, rm } from "node:fs/promises";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { resolve, dirname, relative, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const out = resolve(process.env.WEBTRANSPORT_PLAY_OUT ?? "target/wf/webtransport-player-01a1247a/play");
const evidence = { kind: "strict-actual-two-host-gameplay", passed: false, artifacts: {}, hosts: [], errors: [], cleanup: [] };
const browsers = [], pages = [];
let stopping = false;
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const delay = ms => new Promise(done => setTimeout(done, ms));
const required = name => { assert(process.env[name], `Missing ${name}`); return process.env[name]; };
const within = (base, path) => { const rel = relative(base, path); return rel !== ".." && !rel.startsWith("../") && !isAbsolute(rel); };
const fixedFlags = ["--no-sandbox", "--disable-dev-shm-usage", "--enable-unsafe-webgpu", "--enable-unsafe-swiftshader", "--use-angle=vulkan", "--use-vulkan=swiftshader", "--enable-features=Vulkan", "--disable-vulkan-surface"];
const forbidden = /ignore-certificate|ignoreCertificate|acceptInsecureCerts|serverCertificateHashes|webtransport-developer|force-quic|origin-to-force-quic|autoplay-policy|disable-web-security/i;

export async function runBounded(operation, milliseconds, onStop = () => {}) {
  const token = { stopped: false };
  let timer;
  try {
    return await Promise.race([Promise.resolve().then(() => operation(token)), new Promise((_, reject) => {
      timer = setTimeout(() => { token.stopped = true; onStop(); reject(Error("Acceptance runner deadline exceeded")); }, milliseconds);
    })]);
  } catch (error) { token.stopped = true; onStop(); throw error; }
  finally { clearTimeout(timer); }
}

export function verifyDocument(bytes, expected) {
  assert.match(expected ?? "", /^[a-f0-9]{64}$/);
  assert.equal(hash(bytes), expected, "Actual main navigation document differs from index.html manifest");
}

export function verifyPresentation(probe, renderer) {
  assert(renderer && !renderer.unavailable && !renderer.overflow, "Renderer observation unavailable");
  assert.deepEqual(renderer.errors, []);
  const outgoing = renderer.outgoing ?? [];
  assert(!outgoing.some(x => x.kind === "render-error"), "Actual renderer rejected presentation");
  assert(!probe.messages.some(x => x.direction === "received" && x.kind === "play-completed-results" && (x.error || x.completedResults?.failed)), "Completed Results display failed");
  const requests = probe.messages.filter(x => x.direction === "sent" && x.kind?.startsWith("play-results-"));
  assert(requests.length > 0, "Results RPC not issued");
  for (const request of requests) {
    const reply = probe.messages.find(x => x.direction === "received" && x.kind === "play-reply" && x.playId === request.playId && x.rpcId === request.rpcId);
    assert(reply && !reply.error && reply.result?.kind === "completed-results" && !reply.result.completedResults?.failed, "Results RPC missing or failed");
  }
  const result = renderer.packets.findLast(x => x.kind === 5);
  assert(result, "Actual Results packet not delivered");
  const same = value => value.generation === result.generation && value.content === result.content;
  const room = renderer.packets.findLast(x => x.kind === 6 && same(x));
  assert(room, "Combined room packet not delivered");
  for (const packet of [result, room]) assert(outgoing.some(x => x.kind === "state-ack" && same(x) && x.packetKind === packet.kind && x.sequence === packet.sequence), "Matching Results/room acknowledgement missing");
  assert(outgoing.some(x => x.kind === "drawn" && same(x) && x.mode === "results" && BigInt(x.sequence) >= BigInt(room.sequence)), "Matching combined Results draw missing");
  assert(outgoing.some(x => x.kind === "geometry-ack" && same(x) && x.width > 0 && x.height > 0), "Matching positive geometry acknowledgement missing");
  assert(probe.messages.some(x => x.direction === "received" && x.kind === "render-geometry" && same(x) && x.mode === "results" && x.width > 0 && x.height > 0), "Matching positive Main Results geometry missing");
  return { generation: result.generation, content: result.content };
}

function observation() {
  const probe = globalThis.__roomPlayProbe = { messages: [], inputs: [], visibility: [], worklets: [], overflow: false };
  const append = (list, value) => { if (list.length >= 20000) probe.overflow = true; else list.push(value); };
  const copy = value => JSON.parse(JSON.stringify(value, (_, item) => {
    if (typeof item === "bigint") return String(item);
    if (item instanceof Uint32Array) return Array.from(item);
    if (ArrayBuffer.isView(item)) return { byteLength: item.byteLength };
    if (item instanceof ArrayBuffer) return { byteLength: item.byteLength };
    return item;
  }));
  const ActualWorker = Worker;
  globalThis.Worker = class extends ActualWorker {
    constructor(url, options) {
      super(url, options);
      const source = String(url);
      append(probe.messages, { direction: "created", source });
      this.addEventListener("message", event => append(probe.messages, { direction: "received", source, at: performance.now(), ...copy(event.data) }));
    }
    postMessage(value, transfer) {
      append(probe.messages, { direction: "sent", at: performance.now(), ...copy(value) });
      return super.postMessage(value, transfer);
    }
  };
  const ActualWorklet = AudioWorkletNode;
  globalThis.AudioWorkletNode = class extends ActualWorklet {
    constructor(context, name, options) {
      super(context, name, options);
      append(probe.worklets, { name, sampleRate: context.sampleRate, state: context.state });
      this.port.addEventListener("message", event => append(probe.messages, { direction: "worklet", at: performance.now(), ...copy(event.data) }));
    }
  };
  for (const type of ["keydown", "keyup", "pointerdown"]) document.addEventListener(type, event => append(probe.inputs, { type, trusted: event.isTrusted, code: event.code, at: performance.now() }), true);
  for (const type of ["blur", "focus", "visibilitychange"]) addEventListener(type, () => append(probe.visibility, { type, hidden: document.hidden, at: performance.now() }));
}

// Only reads production renderer port traffic; never writes, holds or dispatches.
function rendererObservation() {
  const probe = globalThis.__roomRenderProbe = { packets: [], outgoing: [], errors: [], overflow: false };
  const actualPost = MessagePort.prototype.postMessage;
  MessagePort.prototype.postMessage = function(value, transfer) {
    if (["state-ack", "drawn", "geometry-ack", "render-error"].includes(value?.kind)) {
      if (probe.outgoing.length >= 20000) probe.overflow = true;
      else probe.outgoing.push(JSON.parse(JSON.stringify(value, (_, item) => typeof item === "bigint" ? String(item) : item)));
    }
    return actualPost.call(this, value, transfer);
  };
  const original = MessagePort.prototype.start, observed = new WeakSet();
  MessagePort.prototype.start = function() {
    if (!observed.has(this)) {
      observed.add(this);
      this.addEventListener("message", event => {
        const value = event.data;
        if (!(value?.packet instanceof Uint8Array)) return;
        if (probe.packets.length >= 20000) { probe.overflow = true; return; }
        try {
          const view = new DataView(value.packet.buffer, value.packet.byteOffset, value.packet.byteLength);
          probe.packets.push({ kind: view.getUint16(6, true), generation: String(view.getBigUint64(8, true)), content: String(view.getBigUint64(16, true)), sequence: String(view.getBigUint64(24, true)), length: value.packet.byteLength });
        } catch (error) { probe.errors.push(String(error)); }
      });
    }
    return original.call(this);
  };
}

async function fixture() {
  const frames = 9600, wav = Buffer.alloc(44 + frames * 2);
  wav.write("RIFF"); wav.writeUInt32LE(wav.length - 8, 4); wav.write("WAVEfmt ", 8);
  wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(48000, 24); wav.writeUInt32LE(96000, 28); wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34);
  wav.write("data", 36); wav.writeUInt32LE(frames * 2, 40);
  for (let i = 0; i < frames; i++) wav.writeInt16LE(Math.round(Math.sin(i * Math.PI * 880 / 48000) * 1200), 44 + i * 2);
  const bms = Buffer.from("#TITLE Integration original input\n#BPM 120\n#TOTAL 320\n#WAV01 x.wav\n#00011:00010101\n#00211:01010101\n#00311:01\n");
  await writeFile(resolve(out, "long.bms"), bms); await writeFile(resolve(out, "x.wav"), wav);
  evidence.fixture = { bms: hash(bms), wav: hash(wav), seed: "0", notes: 8 };
}

async function wait(page, predicate, timeout = 20000, ...args) {
  await page.waitForFunction(predicate, { timeout }, ...args);
  const errors = await page.evaluate(() => __roomPlayProbe.messages.filter(x => x.direction === "received" && (["play-error", "fatal", "play-presentation-unavailable"].includes(x.kind) || x.kind === "play-room" && ["closed", "display-unavailable"].includes(x.event?.kind))));
  assert.equal(errors.length, 0, JSON.stringify(errors));
}
async function snapshot(page, index, label) {
  const record = { label, index, status: await page.$eval("#status", x => x.textContent), probe: await page.evaluate(() => __roomPlayProbe), renderer: [] };
  for (const worker of page.observedWorkers) {
    if (!worker.url().endsWith("/renderer-worker.js")) continue;
    try { record.renderer.push(await worker.evaluate(() => globalThis.__roomRenderProbe)); }
    catch (error) { record.renderer.push({ unavailable: String(error) }); }
  }
  evidence.hosts.push(record);
  await page.screenshot({ path: resolve(out, `host-${index}-${label}.png`), fullPage: true });
  return record;
}

async function main() {
  const legacyTrust = resolve(homedir(), ".pki/nssdb");
  try { await stat(legacyTrust); throw Error("Legacy HOME NSS database exists; isolated XDG trust cannot be assumed"); }
  catch (error) { if (error.code !== "ENOENT") throw error; }
  const app = new URL(required("WEBTRANSPORT_PLAY_APP_URL")), room = new URL(required("WEBTRANSPORT_PLAY_ROOM_URL"));
  assert.equal(app.protocol, "https:"); assert.equal(room.protocol, "https:");
  assert(!app.username && !app.password && !room.username && !room.password);
  assert.match(room.pathname, /^\/rooms\/[A-Za-z0-9_-]+$/);
  const hosts = JSON.parse(required("WEBTRANSPORT_PLAY_HOSTS"));
  assert.equal(hosts.length, 2);
  for (const field of ["display", "profile", "xdgDataHome", "xdgConfigHome"]) {
    assert(hosts.every(host => typeof host[field] === "string" && host[field].length > 0), `Two ${field} values required`);
    assert.equal(new Set(hosts.map(host => field === "display" ? host[field] : resolve(host[field]))).size, 2, `${field} must be independent`);
  }
  for (const host of hosts) {
    for (const field of ["xdgDataHome", "xdgConfigHome"]) {
      assert((await stat(host[field])).isDirectory());
      host[field] = await realpath(host[field]);
    }
    try { await stat(host.profile); throw Error("Browser profile already exists"); } catch (error) { if (error.code !== "ENOENT") throw error; }
    assert(!within(resolve(host.profile), out), "Evidence directory must be outside browser profile");
    for (const field of ["xdgDataHome", "xdgConfigHome"]) assert(!within(resolve(host.profile), host[field]), "Caller trust directories must be outside owned profile");
  }
  for (const field of ["xdgDataHome", "xdgConfigHome"]) assert(!within(hosts[0][field], hosts[1][field]) && !within(hosts[1][field], hosts[0][field]), `${field} trust directories overlap`);
  const manifest = JSON.parse(await readFile(required("WEBTRANSPORT_PLAY_HASHES"), "utf8"));
  for (const name of ["index.html", "main.js", "worker.js", "renderer-worker.js", "audio-worklet.js", "pkg/beatkernel_bms_runtime.js", "pkg/beatkernel_bms_runtime_bg.wasm", "audio-pkg/beatkernel_bms_runtime.js", "audio-pkg/beatkernel_bms_runtime_bg.wasm"]) assert(manifest[`app/web/${name}`], `Missing artifact hash: ${name}`);
  for (const [path, expected] of Object.entries(manifest)) {
    assert(path.startsWith("app/web/") && within(resolve(root, "app/web"), resolve(root, path)) && /^[a-f0-9]{64}$/.test(expected));
    assert(within(await realpath(resolve(root, "app/web")), await realpath(resolve(root, path))), `Artifact symlink escapes app/web: ${path}`);
    assert.equal(hash(await readFile(resolve(root, path))), expected, `Stale current artifact ${path}`);
    evidence.artifacts[path] = expected;
  }
  evidence.configuration = { app: app.href, room: room.href, hosts, flags: fixedFlags, originalDeadlines: true };
  const puppeteer = createRequire(import.meta.url)(resolve(required("PUPPETEER_MODULE")));
  await fixture();
  for (let i = 0; i < 2; i++) {
    assert(!stopping, "Acceptance deadline expired");
    const host = hosts[i];
    host.ownedCache = await mkdtemp("/dev/shm/beatkernel-room-play-");
    if (stopping) { await rm(host.ownedCache, { recursive: true }); throw Error("Acceptance stopped before launch"); }
    let browser;
    try {
      browser = await puppeteer.launch({ executablePath: required("CHROMIUM"), headless: false, userDataDir: resolve(host.profile), env: { ...process.env, DISPLAY: host.display, XDG_DATA_HOME: resolve(host.xdgDataHome), XDG_CONFIG_HOME: resolve(host.xdgConfigHome) }, args: [...fixedFlags, `--disk-cache-dir=${host.ownedCache}`, "--disk-cache-size=16777216", `--log-net-log=${resolve(out, `network-${i}.json`)}`], timeout: 30000, protocolTimeout: 60000 });
    } catch (error) {
      // A failed launch gives no joinable handle; leave its profile/cache for
      // caller inspection rather than remove resources of an unproved process.
      evidence.cleanup.push({ host: i, launchFailed: true, profile: host.profile, cache: host.ownedCache, error: String(error) });
      throw error;
    }
    browser.ownedHost = host;
    browsers.push(browser);
    if (stopping) { await browser.close(); throw Error("Acceptance deadline expired during launch"); }
    const args = browser.process().spawnargs;
    assert(!args.some(arg => forbidden.test(arg)), "Browser command contains forbidden trust/audio override");
    evidence.configuration.hosts[i].pid = browser.process().pid;
    evidence.configuration.hosts[i].actualArgs = args;
    const page = await browser.newPage(); pages.push(page); page.observedWorkers = []; page.pending = [];
    page.setDefaultTimeout(20000);
    page.artifactReads = [];
    page.on("response", response => {
      const url = new URL(response.url());
      const base = new URL("./", app);
      if (url.origin !== app.origin || !url.pathname.startsWith(base.pathname) || !/\.(?:html|js|mjs|wasm|css)$/.test(url.pathname)) return;
      const path = `app/web/${decodeURIComponent(url.pathname.slice(base.pathname.length))}`;
      page.artifactReads.push((async () => {
        assert(manifest[path], `Loaded artifact omitted from manifest: ${path}`);
        assert.equal(hash(await response.buffer()), manifest[path], `Executed artifact mismatch ${path}`);
      })().catch(error => evidence.errors.push({ host: i, kind: "artifact", error: String(error) })));
    });
    page.on("pageerror", error => evidence.errors.push({ host: i, kind: "pageerror", error: String(error), stack: error.stack }));
    page.on("console", message => { if (message.type() === "error") evidence.errors.push({ host: i, kind: "console", error: message.text() }); });
    page.on("workercreated", worker => {
      page.observedWorkers.push(worker);
      if (worker.url().endsWith("/renderer-worker.js")) page.pending.push(worker.evaluate(rendererObservation).catch(error => { evidence.errors.push({ host: i, kind: "worker-observation", error: String(error) }); }));
    });
    await page.evaluateOnNewDocument(observation);
    await page.setViewport({ width: 1200, height: 1000 });
    const navigation = await page.goto(app.href, { waitUntil: "networkidle0", timeout: 60000 });
    assert(navigation && navigation.ok(), "Actual main navigation failed");
    verifyDocument(await navigation.buffer(), manifest["app/web/index.html"]);
    await wait(page, () => !document.querySelector("#files").disabled, 60000);
    await Promise.all(page.pending);
    assert(await page.evaluate(() => crossOriginIsolated && isSecureContext && document.hasFocus() && !document.hidden));
    // Retrieve via Chromium's ordinary trusted TLS path, compare deployed bytes
    // to the caller manifest and current local artifacts, never intercept them.
    for (const [path, expected] of Object.entries(manifest)) {
      const url = new URL(path.slice("app/web/".length), app);
      const actual = await page.evaluate(async url => {
        const response = await fetch(url, { cache: "no-store" });
        if (!response.ok) throw Error(`Artifact HTTP ${response.status}: ${url}`);
        return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", await response.arrayBuffer()))).map(x => x.toString(16).padStart(2, "0")).join("");
      }, url.href);
      assert.equal(actual, expected, `Deployed artifact mismatch ${path}`);
    }
    await (await page.$("#files")).uploadFile(resolve(out, "long.bms"), resolve(out, "x.wav"));
    await wait(page, () => !document.querySelector("#prepare").disabled);
    await page.select("#chart", "long.bms");
    assert.equal(await page.$eval("#seed", x => x.value), "0");
    await page.click("#prepare"); await wait(page, () => document.querySelector("#status").textContent.startsWith("Chart prepared"));
    await page.click("#record"); await page.click("#multiplayer"); await page.select("#multiplayer-mode", "room");
    await page.$eval("#multiplayer-url", (x, url) => { x.value = url; x.dispatchEvent(new Event("input", { bubbles: true })); }, room.href);
  }
  // Genuine gestures authorize each original AudioHost; distinct X displays
  // retain independent focus under the unchanged product blur stop policy.
  for (const page of pages) await page.click("#play");
  await Promise.all(pages.map(page => wait(page, () => __roomPlayProbe.messages.some(x => x.kind === "play-room" && x.event?.kind === "snapshot" && x.event.snapshot.phase === 0 && x.event.snapshot.members.length === 2))));
  const collecting = await Promise.all(pages.map(page => page.evaluate(() => __roomPlayProbe.messages.findLast(x => x.kind === "play-room" && x.event?.kind === "snapshot").event)));
  assert.notEqual(collecting[0].participant, collecting[1].participant);
  assert.deepEqual(collecting[0].snapshot.members, collecting[1].snapshot.members);
  const creator = collecting.findIndex(event => event.participant === event.snapshot.members[0].participant);
  assert(creator >= 0);
  await wait(pages[creator], () => !document.querySelector("#room-seal").disabled);
  await pages[creator].click("#room-seal");
  for (const page of pages) { await wait(page, () => !document.querySelector("#room-ready").disabled); await page.click("#room-ready"); }
  await Promise.all(pages.map(page => wait(page, () => document.querySelector("#status").textContent.startsWith("Playing"), 30000)));
  await Promise.all(pages.map((page, index) => snapshot(page, index, "playing")));
  // Eight original tap notes span 0.5..6 seconds. Repeated genuine acquisitions
  // tolerate scheduler jitter; every hit must still come from original judging.
  await Promise.all(pages.map(async page => {
    const started = Date.now();
    while (Date.now() - started < 7500) {
      if (await page.evaluate(() => __roomPlayProbe.messages.some(x => ["play-stopped", "play-error"].includes(x.kind)))) break;
      await page.keyboard.down("z"); await delay(12); await page.keyboard.up("z"); await delay(18);
    }
  }));
  await Promise.all(pages.map(page => wait(page, () => __roomPlayProbe.messages.some(x => x.kind === "play-stopped") && !document.querySelector("#play").disabled, 30000)));
  await Promise.all(pages.map(async page => {
    const renderer = page.observedWorkers.find(worker => worker.url().endsWith("/renderer-worker.js"));
    assert(renderer, "Actual renderer missing");
    const until = Date.now() + 20000;
    for (;;) {
      assert(!stopping, "Acceptance stopped awaiting Results presentation");
      try { verifyPresentation(await page.evaluate(() => __roomPlayProbe), await renderer.evaluate(() => __roomRenderProbe)); break; }
      catch (error) { if (Date.now() >= until) throw error; await delay(25); }
    }
  }));
  const results = await Promise.all(pages.map((page, index) => snapshot(page, index, "completed")));
  const terminals = results.map(record => {
    const probe = record.probe, received = probe.messages.filter(x => x.direction === "received");
    assert.equal(probe.overflow, false);
    assert.equal(received.filter(x => x.kind === "play-room" && x.event?.kind === "start").length, 1);
    assert.equal(probe.messages.filter(x => x.direction === "sent" && x.kind === "play-start").length, 1);
    assert.equal(probe.messages.filter(x => x.direction === "sent" && x.kind === "play-activate").length, 1);
    const stops = probe.messages.filter(x => x.direction === "sent" && x.kind === "play-stop");
    assert.equal(stops.length, 1); assert.equal(stops[0].completed, true);
    const prepared = received.find(x => x.kind === "play-room" && x.event?.kind === "snapshot" && x.event.snapshot.phase === 2);
    assert(prepared && prepared.event.snapshot.members.length === 2 && prepared.event.snapshot.members.every(x => x.prepared));
    assert.deepEqual(prepared.event.snapshot.members.map(x => ({ participant: x.participant, players: x.players })), collecting[0].snapshot.members.map(x => ({ participant: x.participant, players: x.players })));
    assert.equal(received.filter(x => x.kind === "play-stopped").length, 1);
    const terminal = received.find(x => x.kind === "play-stopped");
    assert(terminal.completedResults?.proof);
    assert.equal(terminal.replays?.length, 1);
    assert(terminal.replays[0].replayComplete && terminal.replays[0].replay?.byteLength > 0);
    assert.equal(terminal.replays[0].replayError, null);
    assert(terminal.completedArchive?.byteLength > 0);
    assert.equal(terminal.replayError, null); assert.equal(terminal.archiveError, null);
    assert.equal(terminal.completedResultsError, null);
    assert.equal(terminal.room.error, null);
    for (const field of ["finalQueued", "finalWritten", "finalAcknowledged", "localComplete"]) assert.equal(terminal.room[field], true, field);
    assert.equal(terminal.room.finalDrain, "complete");
    assert.equal(terminal.roomResults.failed, false);
    assert(BigInt(terminal.hits) > 0); assert.equal(BigInt(terminal.hits) + BigInt(terminal.misses), 8n);
    assert(probe.inputs.some(x => x.type === "keydown" && x.code === "KeyZ" && x.trusted));
    assert(probe.inputs.some(x => x.type === "keyup" && x.code === "KeyZ" && x.trusted));
    assert.equal(probe.worklets.length, 1);
    assert(received.some(x => x.kind === "play-room" && x.event?.kind === "score-pages" && x.event.pages === 1));
    const start = received.find(x => x.kind === "play-room" && x.event?.kind === "start");
    assert(!probe.visibility.some(x => x.at >= start.at && x.at <= terminal.at && (x.type === "blur" || x.hidden)));
    verifyPresentation(probe, record.renderer[0]);
    return terminal;
  });
  for (let i = 0; i < 2; i++) {
    const own = terminals[i], other = terminals[1 - i];
    assert.equal(own.room.participant, collecting[i].participant);
    assert.equal(own.room.peers.length, 1);
    const peer = own.room.peers[0]; assert.equal(peer.participant, other.room.participant);
    assert(peer.finalPrefix && BigInt(peer.sequence) > 0n); assert.equal(peer.words.length, 11);
    assert.equal(peer.words[0], collecting[1 - i].snapshot.members.find(x => x.participant === other.room.participant).players[0]);
    const u64 = offset => BigInt(peer.words[offset]) | BigInt(peer.words[offset + 1]) << 32n;
    for (const [field, offset] of [["hits", 3], ["misses", 5], ["combo", 7], ["maxCombo", 9]]) assert.equal(u64(offset), BigInt(other[field]), `Remote ${field} differs from actual other-host completion`);
  }
  for (const page of pages) await Promise.all(page.artifactReads);
  assert.deepEqual(evidence.errors, []);
}

async function execute() {
await mkdir(out, { recursive: true });
try {
  await runBounded(main, 240000, () => { stopping = true; });
  assert(!stopping && !evidence.failure);
  evidence.passed = true;
}
catch (error) {
  evidence.failure = { message: String(error), stack: error.stack };
  for (let i = 0; i < pages.length; i++) await snapshot(pages[i], i, "failure").catch(error => evidence.errors.push({ kind: "failure-snapshot", error: String(error) }));
} finally {
  stopping = true;
  for (const browser of browsers) {
    const child = browser.process();
    let closeError = null;
    try { await Promise.race([browser.close(), delay(10000).then(() => { throw Error("Browser close timeout"); })]); }
    catch (error) { closeError = String(error); if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL"); }
    if (child.exitCode === null && child.signalCode === null) await Promise.race([new Promise(done => child.once("exit", done)), delay(3000)]);
    const joined = child.exitCode !== null || child.signalCode !== null;
    evidence.cleanup.push({ pid: child.pid, joined, closeError });
    if (joined) {
      for (const path of [browser.ownedHost.profile, browser.ownedHost.ownedCache]) {
        try { await rm(resolve(path), { recursive: true, force: true }); }
        catch (error) { evidence.cleanup.push({ path, removalError: String(error) }); evidence.passed = false; }
      }
    }
    if (!joined || closeError) evidence.passed = false;
  }
  await writeFile(resolve(out, "evidence.json"), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify({ passed: evidence.passed, failure: evidence.failure, cleanup: evidence.cleanup, output: out }));
  if (!evidence.passed) process.exitCode = 1;
}
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await execute();
