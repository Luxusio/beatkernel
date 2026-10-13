/** Production live-speed acceptance probe; authoring does not constitute QA.
 * Fresh main/audio WASM + generated bindings must already exist.
 * PUPPETEER_MODULE=target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core \
 * CHROMIUM=/usr/bin/chromium node app/web/live-speed.browser.mjs
 * LIVE_SPEED_OUT and LIVE_SPEED_PORT select ignored evidence / an owned port.
 * Observers forward original arguments/results. No clock, ACK, PCM, input
 * producer, completion, or gameplay metadata is substituted.
 */
import assert from "node:assert/strict";
import { createServer } from "node:https";
import { readFile, writeFile, mkdir, stat } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { X509Certificate, createHash } from "node:crypto";
import { createRequire } from "node:module";
import { resolve, dirname, extname, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const out = resolve(process.env.LIVE_SPEED_OUT ?? resolve(root, "target/wf/live-speed-browser/development-1"));
const port = Number(process.env.LIVE_SPEED_PORT ?? 8148);
const audioWasmFile = resolve(root, "app/web/audio-pkg/beatkernel_bms_runtime_bg.wasm");
const audioWasmURL = `https://127.0.0.1:${port}/app/web/audio-pkg/beatkernel_bms_runtime_bg.wasm`;
assert.ok(Number.isInteger(port) && port >= 1024 && port <= 65535);
assert.ok(relative(root, out).startsWith(`target${sep}wf${sep}`));
const required = ["cold-refusal", "1/2", "1/1", "2/1", "3/2", "saved-record", "quantized-end", "independent-replay", "opponent-match", "opponent-refusal", "nonzero-section", "seek-restart", "touch-local", "stop-retry", "responsive", "clean-console-network"];
const evidence = { kind: "development-production-live-speed", required, checks: [], cases: [], console: [], network: [],
  audioWasmResponses: [], audioWasmResponseOverflow: false, ceilings: [
  "Chromium software output/GPU proves software execution, not acoustic latency or physical devices.",
  "The actual UI always supplies 100 ms preroll and end > start; zero output frames cannot be selected here. Actual zero-grid owner/Mixer coverage is separate. This runner selects a genuine 1 ns interval instead.",
  "CDP supplies actual browser keyboard/touch events, not physical hardware timing; local coverage uses distinct keyboard/touch aggregates only.",
] };
const json = value => JSON.stringify(value, (_, x) => typeof x === "bigint" ? String(x) : x, 2);
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
const sleep = ms => new Promise(done => setTimeout(done, ms));
async function bounded(promise, ms, label) {
  let timer;
  try { return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(Error(`${label} deadline`)), ms); })]); }
  finally { clearTimeout(timer); }
}
const check = (id, detail) => evidence.checks.push({ id, passed: true, detail });
let server, browser, page, worker, renderer;

function wav(frequency, seconds) {
  const hz = 48000, count = hz * seconds, bytes = Buffer.alloc(44 + count * 2);
  bytes.write("RIFF"); bytes.writeUInt32LE(bytes.length - 8, 4); bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(hz, 24); bytes.writeUInt32LE(hz * 2, 28); bytes.writeUInt16LE(2, 32); bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36); bytes.writeUInt32LE(count * 2, 40);
  for (let i = 0; i < count; i++) bytes.writeInt16LE(Math.round(Math.sin(i * 2 * Math.PI * frequency / hz) * 4096), 44 + i * 2);
  return bytes;
}
async function serve() {
  await mkdir(out, { recursive: true });
  await writeFile(resolve(out, "music.wav"), wav(880, 3));
  await writeFile(resolve(out, "key.wav"), wav(220, 0.025));
  await writeFile(resolve(out, "live.bms"), "#TITLE Actual live speed acceptance\n#BPM 120\n#TOTAL 320\n#WAV01 music.wav\n#WAV02 key.wav\n#00001:01\n#00011:00020000\n");
  const key = resolve(out, "key.pem"), cert = resolve(out, "cert.pem");
  const result = spawnSync("openssl", ["req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-keyout", key, "-out", cert, "-days", "1", "-subj", "/CN=localhost", "-addext", "subjectAltName=IP:127.0.0.1,DNS:localhost"], { encoding: "utf8", timeout: 10000 });
  assert.equal(result.status, 0, result.stderr);
  const certificate = new X509Certificate(await readFile(cert));
  server = createServer({ cert: await readFile(cert), key: await readFile(key) }, async (request, response) => {
    try {
      const url = new URL(request.url, "https://localhost");
      if (url.pathname === "/favicon.ico") { response.writeHead(204); response.end(); return; }
      let file = resolve(root, `.${decodeURIComponent(url.pathname)}`);
      const path = relative(root, file);
      assert.ok(path !== ".." && !path.startsWith(`..${sep}`));
      if ((await stat(file)).isDirectory()) file = resolve(file, "index.html");
      response.writeHead(200, { "Content-Type": ({ ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".html": "text/html", ".css": "text/css" })[extname(file)] ?? "application/octet-stream",
        "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp",
        "Cache-Control": file === audioWasmFile ? "no-cache" : "no-store" });
      response.end(await readFile(file));
    } catch (error) { evidence.network.push({ url: request.url, serverError: String(error) }); response.writeHead(404); response.end(); }
  });
  await new Promise((done, reject) => { server.once("error", reject); server.listen(port, "127.0.0.1", done); });
  return createHash("sha256").update(certificate.publicKey.export({ type: "spki", format: "der" })).digest("base64");
}

function observeWindow() {
  const probe = globalThis.__liveSpeedWindow = { messages: [], pcm: [], contexts: [], audioContexts: [], statuses: [], replayFiles: [], fileErrors: [], audioTerminals: [] };
  const safe = value => JSON.parse(JSON.stringify(value, (_, x) => typeof x === "bigint" ? String(x)
    : x instanceof File ? { file: true, name: x.name, size: x.size } : x instanceof Blob ? { blob: true, size: x.size }
      : x instanceof MessagePort ? { port: true } : ArrayBuffer.isView(x) ? Array.from(x) : x));
  const ActualWorker = Worker;
  globalThis.Worker = class extends ActualWorker {
    constructor(url, options) {
      super(url, options); this.probeURL = String(url);
      this.addEventListener("message", event => {
        if (probe.messages.length >= 50000) throw Error("Window observation ceiling");
        probe.messages.push({ direction: "received", url: this.probeURL, data: safe(event.data) });
      });
    }
    postMessage(value, transfer) {
      if (probe.messages.length >= 50000) throw Error("Window observation ceiling");
      probe.messages.push({ direction: "sent", url: this.probeURL, data: safe(value) });
      if (value?.kind === "play-start" && value.replayFile instanceof File) {
        const observed = { playId: String(value.playId), bytes: null }; probe.replayFiles.push(observed);
        value.replayFile.arrayBuffer().then(bytes => { if (bytes.byteLength > 1048576) throw Error("Replay file observation ceiling"); observed.bytes = Array.from(new Uint8Array(bytes)); }).catch(error => probe.fileErrors.push(String(error)));
      }
      return super.postMessage(value, transfer);
    }
  };
  const ActualNode = AudioWorkletNode;
  const ActualContext = AudioContext;
  globalThis.AudioContext = class extends ActualContext {
    constructor(...args) { super(...args); probe.audioContexts.push({ sampleRate: this.sampleRate }); }
  };
  globalThis.AudioWorkletNode = class extends ActualNode {
    constructor(context, name, options) {
      super(context, name, options);
      const index = probe.contexts.length; probe.contexts.push({ name, sampleRate: context.sampleRate });
      this.port.addEventListener("message", event => {
        if (event.data?.kind !== "terminal") return;
        if (probe.audioTerminals.length >= 128) throw Error("Audio terminal observation ceiling");
        probe.audioTerminals.push({ index, diagnostics: safe(event.data) });
      });
      const analyser = context.createAnalyser(); analyser.fftSize = 2048; this.connect(analyser);
      const samples = new Float32Array(analyser.fftSize);
      const acquire = () => {
        if (context.state === "closed") return;
        analyser.getFloatTimeDomainData(samples);
        let peak = 0, crossings = 0;
        for (let i = 0; i < samples.length; i++) { peak = Math.max(peak, Math.abs(samples[i])); if (i && samples[i - 1] <= 0 && samples[i] > 0) crossings++; }
        if (peak > 0.005 && probe.pcm.length < 8192) probe.pcm.push({ index, peak, frequency: crossings * context.sampleRate / samples.length });
        requestAnimationFrame(acquire);
      };
      requestAnimationFrame(acquire);
    }
  };
  addEventListener("DOMContentLoaded", () => new MutationObserver(() => {
    if (probe.statuses.length >= 5000) throw Error("Status observation ceiling");
    probe.statuses.push(document.querySelector("#status").textContent);
  }).observe(document.querySelector("#status"), { childList: true, subtree: true, characterData: true }));
}

async function observeWorker() {
  const probe = globalThis.__liveSpeedWorker = { owners: [], events: [], reports: [], captures: [], pending: 0, errors: [] };
  const bindings = await import("./pkg/beatkernel_bms_runtime.js");
  const safe = value => JSON.parse(JSON.stringify(value, (_, x) => typeof x === "bigint" ? String(x) : ArrayBuffer.isView(x) ? Array.from(x) : x));
  const owners = new WeakMap();
  for (const name of ["BrowserGame", "BrowserLocalGame", "BrowserReplay"]) {
    const proto = bindings[name]?.prototype;
    if (!proto) throw Error(`Actual generated ${name} missing`);
    for (const method of ["admit_output", "observe_presentation", "service_audio", "evaluate_completion", "observe_output", "commands", "acknowledge", "queue_input", "queue_input_blob", "queue_input_blob_on_surface", "queue_input_blob_on_surface_on_page", "configure_touch_regions", "add_saved_opponent"]) {
      const original = proto[method]; if (typeof original !== "function") continue;
      proto[method] = function(...args) {
        if (!owners.has(this)) { owners.set(this, probe.owners.length); probe.owners.push({ name }); }
        const owner = owners.get(this), beforeSong = String(this.song_ns);
        const result = original.apply(this, args);
        if (probe.events.length >= 100000) throw Error("Actual binding observation ceiling");
        probe.events.push({ owner, method, args: safe(args), result: safe(result ?? null), beforeSong, songNs: String(this.song_ns) });
        return result;
      };
    }
  }
  const post = globalThis.postMessage;
  globalThis.postMessage = function(value, ...args) {
    // The production Worker transfers Uint8Array captures; copy the read-only
    // observation before the original post detaches its owning buffer.
    const files = value?.replay instanceof File || value?.replay instanceof Uint8Array ? [{ player: null, replay: value.replay, complete: value.replayComplete }]
      : Array.isArray(value?.replays) ? value.replays : [];
    for (const item of files) if (item.replay instanceof File || item.replay instanceof Uint8Array) {
      const capture = { playId: value.playId, player: item.player, complete: item.complete ?? item.replayComplete, bytes: null };
      probe.captures.push(capture);
      if (item.replay instanceof Uint8Array) { if (item.replay.byteLength > 1048576) throw Error("Capture ceiling"); capture.bytes = Array.from(item.replay); }
      else { probe.pending++; item.replay.arrayBuffer().then(bytes => { if (bytes.byteLength > 1048576) throw Error("Capture ceiling"); capture.bytes = Array.from(new Uint8Array(bytes)); })
        .catch(error => probe.errors.push(String(error))).finally(() => probe.pending--); }
    }
    return post.call(this, value, ...args);
  };
  const descriptor = Object.getOwnPropertyDescriptor(MessagePort.prototype, "onmessage");
  if (!descriptor?.set || !descriptor.get) throw Error("Actual port descriptor missing");
  Object.defineProperty(MessagePort.prototype, "onmessage", { ...descriptor, set(listener) {
    return descriptor.set.call(this, listener === null ? null : function(event) {
      const words = event.data?.report?.words;
      if (words instanceof Uint32Array) { if (probe.reports.length >= 50000) throw Error("Output report ceiling"); probe.reports.push(Array.from(words)); }
      return listener.call(this, event);
    });
  } });
}

function observeRenderer() {
  const probe = globalThis.__liveSpeedRenderer = { frames: [], previews: [], receipts: [], errors: [] };
  const seen = new WeakSet(), start = MessagePort.prototype.start, post = MessagePort.prototype.postMessage;
  MessagePort.prototype.start = function() {
    if (!seen.has(this)) { seen.add(this); this.addEventListener("message", event => {
      const bytes = event.data?.packet; if (!(bytes instanceof Uint8Array)) return;
      try {
        if (probe.frames.length >= 50000 || probe.previews.length >= 5000 || bytes.byteLength < 40) throw Error("Renderer extent ceiling");
        const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), kind = view.getUint16(6, true);
        // Public BKRV PREVIEW carries its original-song point at byte40.
        // Observe it separately from gameplay frames; a reply alone does not
        // establish that the selected preview was genuinely drawn.
        if (kind === 3) {
          if (bytes.byteLength !== 56 || view.getBigUint64(32, true) !== 16n) throw Error("Actual preview extent missing");
          probe.previews.push({ generation: String(view.getBigUint64(8, true)), content: String(view.getBigUint64(16, true)), sequence: String(view.getBigUint64(24, true)), songNs: String(view.getBigInt64(40, true)) });
          return;
        }
        if (kind !== 2) return;
        if (bytes.byteLength < 69 || view.getUint32(53, true) < 1) throw Error("Actual visual frame missing member");
        probe.frames.push({ mode: event.data.mode, generation: String(view.getBigUint64(8, true)), content: String(view.getBigUint64(16, true)), sequence: String(view.getBigUint64(24, true)), songNs: String(view.getBigInt64(61, true)) });
      } catch (error) { probe.errors.push(String(error)); }
    }); }
    return start.call(this);
  };
  MessagePort.prototype.postMessage = function(value, transfer) {
    if (value?.kind === "drawn") { if (probe.receipts.length >= 50000) throw Error("Draw receipt ceiling"); probe.receipts.push({ mode: value.mode, generation: String(value.generation), content: String(value.content), sequence: String(value.sequence) }); }
    return post.call(this, value, transfer);
  };
}

// Independent bounded reader for the public BKREPLAY/BKPI wire. Reading saved
// bytes never alters or re-encodes them; the production replay owner validates
// the complete reconstruction when the UI plays these exact bytes below.
function decodeRecord(bytes) {
  const b = Buffer.from(bytes); let p = 0;
  const take = n => { assert.ok(Number.isSafeInteger(n) && n >= 0 && p + n <= b.length); const x = b.subarray(p, p += n); return x; };
  const u64 = () => take(8).readBigUInt64LE(), i64 = () => take(8).readBigInt64LE();
  const blob = () => { const n = u64(); assert.ok(n <= 1048576n); return take(Number(n)); };
  assert.equal(take(8).toString(), "BKREPLAY"); assert.equal(take(4).readUInt32LE(), 1);
  take(4); const domain = take(4).readUInt32LE(); u64();
  const fields = [blob(), blob(), blob(), blob()];
  const calibration = take(1)[0]; assert.ok(calibration <= 1); if (calibration) blob();
  const count = u64(); assert.ok(count <= 100000n); const records = [];
  for (let i = 0n; i < count; i++) {
    const ordinal = u64(), songNs = i64(), kind = take(1)[0]; assert.ok(kind <= 1);
    let input = null;
    if (!kind) {
      const lane = take(4).readUInt32LE(), event = blob(); assert.equal(event.subarray(0, 4).toString(), "BKPI"); assert.equal(event.readUInt16LE(4), 1);
      let q = 7;
      const source = event.readBigUInt64LE(q); q += 8;
      const normalizedNs = event.readBigInt64LE(q); q += 8;
      const normalizedDomain = event.readUInt32LE(q); q += 4;
      const sequence = event.readBigUInt64LE(q); q += 8;
      let nativeNs = null;
      const nativeTag = event[q++]; assert.ok(nativeTag <= 1);
      if (nativeTag) { q += 4; const code = event[q++]; assert.ok(code <= 1); if (code) q += 4; const timestamp = event[q++]; assert.ok(timestamp <= 1); if (timestamp) { q += 4; nativeNs = event.readBigInt64LE(q); q += 8; } }
      const originalTag = event[q++]; assert.equal(originalTag, 1, "Normalized capture retains original HOST point");
      const originalDomain = event.readUInt32LE(q); q += 4; const originalNs = event.readBigInt64LE(q);
      assert.equal(normalizedDomain, domain); assert.equal(originalDomain, 0x57494e);
      if (nativeNs !== null) assert.equal(nativeNs, originalNs);
      input = { lane, variant: event[6], source, sequence, normalizedNs, originalNs, nativeNs };
    }
    records.push({ ordinal, songNs, input });
  }
  assert.equal(p, b.length); return { fields, records };
}
function captureSection(decoded) {
  // This fixture selects button-only finite playback at3/2 with the default
  // gauge. Read the original public setup bytes independently of prepared
  // metadata; no replay owner or re-encoding supplies this timing oracle.
  const speedPrefix = Buffer.from("bms-speed-setup/v1:"), outer = decoded.fields[3];
  assert.deepEqual(outer.subarray(0, speedPrefix.length), speedPrefix);
  assert.ok(outer.length >= speedPrefix.length + 20);
  const innerLength = outer.readUInt32LE(speedPrefix.length);
  assert.equal(outer.length, speedPrefix.length + 20 + innerLength);
  assert.equal(outer.readBigInt64LE(speedPrefix.length + 4), 3n);
  assert.equal(outer.readBigUInt64LE(speedPrefix.length + 12), 2n);
  const options = outer.subarray(speedPrefix.length + 20), prefix = Buffer.from("bms-judge-profile/v4:");
  assert.deepEqual(options.subarray(0, prefix.length), prefix);
  assert.ok(options.length >= prefix.length + 40);
  const seed = options.readBigUInt64LE(prefix.length), startNs = options.readBigInt64LE(prefix.length + 8), endNs = options.readBigInt64LE(prefix.length + 16);
  const windows = options.readBigUInt64LE(prefix.length + 32);
  assert.ok(windows <= 128n); assert.equal(options.length, prefix.length + 40 + Number(windows) * 20);
  return { seed, startNs, endNs };
}
const word = (words, index) => BigInt(words[index * 2]) | BigInt(words[index * 2 + 1]) << 32n;
async function field(id, value) { await page.$eval(`#${id}`, (element, value) => { element.value = value; element.dispatchEvent(new Event("input", { bubbles: true })); element.dispatchEvent(new Event("change", { bubbles: true })); }, value); }
async function toggle(id, checked) { if (await page.$eval(`#${id}`, x => x.checked) !== checked) await page.click(`#${id}`); }
async function start(button = "play") {
  const offset = await page.evaluate(() => __liveSpeedWindow.messages.length);
  await page.click(`#${button}`);
  await page.waitForFunction(offset => __liveSpeedWindow.messages.slice(offset).some(x => x.direction === "sent" && x.data.kind === "play-start"), { timeout: 30000 }, offset);
  return page.evaluate(offset => __liveSpeedWindow.messages.slice(offset).find(x => x.direction === "sent" && x.data.kind === "play-start").data, offset);
}
async function terminal(id) {
  await page.waitForFunction(id => __liveSpeedWindow.messages.some(x => x.direction === "received" && x.data.playId === id && ["play-stopped", "play-error"].includes(x.data.kind)), { timeout: 25000 }, id);
  // Stop is disabled during closing too. Await actual idle controls before
  // changing the next session's roster or acquiring another input owner.
  await page.waitForFunction(() => document.querySelector("#stop").disabled
    && !document.querySelector("#local-count").disabled
    && !document.querySelector("#touch-input").disabled
    && !document.querySelector("#live-speed").disabled, { timeout: 25000 });
}
async function snapshot(name) {
  await worker.evaluate(async () => { const deadline = performance.now() + 5000; while (__liveSpeedWorker.pending) { if (performance.now() > deadline) throw Error("Capture observation deadline"); await new Promise(done => setTimeout(done, 10)); } });
  const result = { name, window: await page.evaluate(() => __liveSpeedWindow), worker: await worker.evaluate(() => __liveSpeedWorker), renderer: await renderer.evaluate(() => __liveSpeedRenderer) };
  assert.deepEqual(result.window.audioTerminals, [], "Actual Worklet terminal failures must fail acceptance, including during cleanup");
  assert.deepEqual(result.worker.errors, []); assert.deepEqual(result.renderer.errors, []);
  evidence.cases.push(result); await page.screenshot({ path: resolve(out, `${name}.png`), fullPage: true }); return result;
}
function session(result, id) { return result.window.messages.filter(x => x.data.playId === id); }
function metadata(messages) { const x = messages.find(x => x.direction === "received" && x.data.kind === "play-reply" && x.data.result?.kind === "prepared")?.data.result; assert.ok(x); return x; }
function assertCompletion(result, request, reportOffset, endNs, n, d, startNs = 0n) {
  const messages = session(result, request.playId); assert.ok(!messages.some(x => x.data.kind === "play-error"), json(messages));
  const meta = metadata(messages), rate = BigInt(request.rate);
  const duration = 100000000n + (endNs - startNs) * d / n, frame = (duration * rate + 999999999n) / 1000000000n;
  assert.equal(meta.startNs, String(startNs)); assert.equal(meta.endNs, String(endNs)); assert.equal(meta.endFrame, String(frame));
  const stopped = messages.find(x => x.direction === "received" && x.data.kind === "play-stopped")?.data;
  assert.ok(stopped); assert.equal(stopped.songNs, String(endNs));
  assert.ok(messages.some(x => x.data.kind === "play-render-done" && x.data.completed && x.data.pendingInputs === 0 && !x.data.commandsPending), "Natural completion requires real drained input and commands");
  const activation = messages.find(x => x.direction === "sent" && x.data.kind === "play-activate")?.data; assert.ok(activation);
  assert.ok(result.worker.reports.slice(reportOffset).some(words => word(words, 0) === 1n && word(words, 6) === 1n && word(words, 7) === frame
    && word(words, 24) >= BigInt(activation.startFrame) + frame && word(words, 13) === word(words, 14) && word(words, 13) > 0n), "Actual Worklet endpoint/counters required");
  return { messages, meta, stopped, frame };
}
async function acquireKeyboardUntil(id, songNs) {
  const deadline = Date.now() + 12000;
  let covered = false;
  try {
    while (Date.now() < deadline) {
      await page.keyboard.down("z"); await sleep(8); await page.keyboard.up("z"); await sleep(12);
      covered = await page.evaluate(({ id, songNs }) => {
        const observation = __liveSpeedWindow.messages.findLast(x => x.direction === "received"
          && x.data.playId === id && x.data.kind === "play-render-done");
        return observation?.data.songNs != null && BigInt(observation.data.songNs) >= BigInt(songNs);
      }, { id, songNs: String(songNs) });
      if (covered) break;
    }
  } finally { await page.keyboard.up("z"); }
  assert.ok(covered, "Genuine section progress must cover the keyboard interval before its deadline");
}
async function actualSection(name) {
  const contextIndex = await page.evaluate(() => __liveSpeedWindow.contexts.length);
  const reportOffset = await worker.evaluate(() => __liveSpeedWorker.reports.length);
  const frameOffset = await renderer.evaluate(() => __liveSpeedRenderer.frames.length);
  const receiptOffset = await renderer.evaluate(() => __liveSpeedRenderer.receipts.length);
  const request = await start();
  assert.equal(request.liveSpeed, "3/2"); assert.equal(request.startNs, "250000000"); assert.equal(request.endNs, "2000000000");
  await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Playing"), { timeout: 30000 });
  await acquireKeyboardUntil(request.playId, 750000000n);
  await terminal(request.playId);
  const result = await snapshot(name), completion = assertCompletion(result, request, reportOffset, 2000000000n, 3n, 2n, 250000000n);
  assert.equal(result.window.contexts.length, contextIndex + 1, "Each section owns a fresh actual Worklet context");
  assert.equal(completion.meta.liveSpeed, "3/2"); assert.ok(BigInt(completion.stopped.hits) > 0n);
  const captured = result.worker.captures.find(x => x.playId === request.playId && x.bytes);
  assert.ok(captured?.complete, "Actual natural section completion must produce a complete capture");
  const decoded = decodeRecord(captured.bytes), capturemeta = captureSection(decoded), inputs = decoded.records.filter(x => x.input);
  assert.equal(capturemeta.startNs, 250000000n); assert.equal(capturemeta.endNs, 2000000000n); assert.equal(capturemeta.seed, BigInt(request.seed));
  assert.ok(inputs.length >= 2); assert.ok(inputs.some(x => x.songNs >= 450000000n && x.songNs <= 550000000n));
  const acquired = completion.messages.filter(x => x.direction === "sent" && x.data.kind === "play-step").flatMap(x => x.data.events);
  for (const record of inputs) {
    const expected = 250000000n + (record.input.normalizedNs - 100000000n) * 3n / 2n;
    assert.ok(record.songNs - expected >= -2n && record.songNs - expected <= 2n, `Original section captured timing mismatch ${record.songNs - expected} ns`);
    assert.equal(record.input.variant, 0); assert.equal(record.input.source, 1n);
    assert.ok(acquired.some(x => String(x.sequence) === String(record.input.sequence) && String(x.hostNs) === String(record.input.originalNs)), "Section capture preserves actual Window keyboard provenance");
  }
  assert.ok(result.window.pcm.some(x => x.index === contextIndex && Math.abs(x.frequency - 1320) <= 50), "Actual nonzero-section PCM must retain3/2 pitch");
  const progress = completion.messages.filter(x => x.data.kind === "play-render-done" && x.data.songNs !== null && BigInt(x.data.songNs) > 350000000n && BigInt(x.data.songNs) < 1800000000n);
  const receipts = result.renderer.receipts.slice(receiptOffset);
  const drawn = result.renderer.frames.slice(frameOffset).filter(x => progress.some(y => y.data.songNs === x.songNs)
    && receipts.some(y => y.mode === "live" && y.generation === x.generation && y.content === x.content && y.sequence === x.sequence));
  assert.ok(new Set(drawn.map(x => x.songNs)).size >= 2, "This actual section must draw advancing original-song positions");
  const bytes = Buffer.from(captured.bytes), sha256 = sha(bytes);
  await writeFile(resolve(out, `${name}.bkr`), bytes);
  return { request, result, contextIndex, sha256, frame: String(completion.frame), inputs: inputs.length, hits: completion.stopped.hits, drawn: drawn.length };
}

async function run() {
  const spki = await serve(), require = createRequire(import.meta.url);
  const puppeteer = process.env.PUPPETEER_MODULE ? require(resolve(process.env.PUPPETEER_MODULE)) : (await import("puppeteer-core")).default;
  evidence.sources = Object.fromEntries(await Promise.all(["main.js", "index.html", "worker.js", "renderer-worker.js", "audio-host.mjs", "audio-worklet.js", "native-audio-clock.mjs", "worklet-native-frame-probe.mjs", "play-model.mjs", "pkg/beatkernel_bms_runtime.js", "pkg/beatkernel_bms_runtime_bg.wasm", "audio-pkg/beatkernel_bms_runtime.js", "audio-pkg/beatkernel_bms_runtime_bg.wasm"].map(async name => [name, sha(await readFile(resolve(root, "app/web", name)))])));
  browser = await puppeteer.launch({ executablePath: process.env.CHROMIUM ?? "/usr/bin/chromium", headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage", "--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-features=Vulkan", "--use-vulkan=swiftshader", "--ignore-certificate-errors", `--ignore-certificate-errors-spki-list=${spki}`, "--autoplay-policy=no-user-gesture-required"] });
  evidence.ownedBrowserPid = browser.process().pid; console.log(`Owned Chromium ${evidence.ownedBrowserPid}; evidence ${out}`);
  page = await browser.newPage(); const installations = [];
  page.on("workercreated", value => { if (value.url().endsWith("/worker.js")) { worker = value; installations.push(value.evaluate(observeWorker)); } if (value.url().endsWith("/renderer-worker.js")) { renderer = value; installations.push(value.evaluate(observeRenderer)); } });
  page.on("pageerror", error => evidence.console.push({ type: "pageerror", text: String(error) }));
  page.on("console", message => evidence.console.push({ type: message.type(), text: message.text() }));
  page.on("requestfailed", request => evidence.network.push({ stage: evidence.stage, url: request.url(), failure: request.failure() }));
  page.on("response", response => {
    if (response.status() >= 400) evidence.network.push({ url: response.url(), status: response.status() });
    if (response.url() === audioWasmURL) {
      if (evidence.audioWasmResponses.length >= 4) { evidence.audioWasmResponseOverflow = true; return; }
      evidence.audioWasmResponses.push({ url: response.url(), at: Date.now(), status: response.status(),
        cacheControl: response.headers()["cache-control"] ?? null, fromCache: response.fromCache(),
        fromServiceWorker: response.fromServiceWorker(), resourceType: response.request().resourceType() });
    }
  });
  await page.setViewport({ width: 1200, height: 1000, hasTouch: true }); await page.evaluateOnNewDocument(observeWindow);
  evidence.stage = "initialization";
  await page.goto(`https://127.0.0.1:${port}/app/web/`, { waitUntil: "networkidle0", timeout: 60000 });
  await page.waitForFunction(() => !document.querySelector("#files").disabled, { timeout: 60000 }); await Promise.all(installations);
  assert.ok(worker && renderer); assert.equal(await page.evaluate(() => crossOriginIsolated), true);
  assert.equal(await page.$eval("#live-speed", x => x.value), "1/1"); assert.equal(await page.$eval("#live-speed", x => x.maxLength), 40);
  assert.match(await page.$eval('label[for="live-speed"]', x => x.textContent), /NUM\/DEN/);
  await (await page.$("#files")).uploadFile(resolve(out, "live.bms"), resolve(out, "music.wav"), resolve(out, "key.wav"));
  await page.waitForFunction(() => !document.querySelector("#prepare").disabled); await page.click("#prepare");
  await page.waitForFunction(() => !document.querySelector("#play").disabled && __liveSpeedWindow.messages.some(x => x.direction === "received" && x.data.kind === "drawn"), { timeout: 60000 });
  await toggle("touch-input", false); await toggle("record", true); await field("live-end", "2");
  const contexts = await page.evaluate(() => __liveSpeedWindow.audioContexts.length);
  await field("live-speed", "0/1"); await page.click("#play");
  await page.waitForFunction(() => document.activeElement?.id === "live-speed", { timeout: 5000 });
  assert.equal(await page.evaluate(() => __liveSpeedWindow.audioContexts.length), contexts); check("cold-refusal", "Invalid fraction focuses field before actual AudioContext creation");
  let saved, savedHash, savedTerminal;
  for (const [fraction, n, d, pitch] of [["1/2", 1n, 2n, 440], ["1/1", 1n, 1n, 880], ["2/1", 2n, 1n, 1760], ["3/2", 3n, 2n, 1320]]) {
    evidence.stage = fraction; await field("live-speed", fraction);
    const contextIndex = await page.evaluate(() => __liveSpeedWindow.contexts.length), reportOffset = await worker.evaluate(() => __liveSpeedWorker.reports.length), eventOffset = await worker.evaluate(() => __liveSpeedWorker.events.length);
    const request = await start(); assert.equal(request.liveSpeed, fraction);
    await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Playing"), { timeout: 30000 });
    assert.equal(await page.$eval("#live-speed", x => x.disabled), true);
    // Real browser acquisition with ordinary repeated Down/Up. This is not a
    // precision keyboard scheduler: original normalized captured facts below
    // are the timing oracle. Repeated acquisitions cross the real note window.
    const deadline = Date.now() + 10000 + Number(750000000n * d / n / 1000000n);
    let covered = false;
    try {
      while (Date.now() < deadline) {
        await page.keyboard.down("z"); await sleep(8); await page.keyboard.up("z"); await sleep(12);
        covered = await page.evaluate(id => {
          const observation = __liveSpeedWindow.messages.findLast(x => x.direction === "received"
            && x.data.playId === id && x.data.kind === "play-render-done");
          return observation?.data.songNs != null && BigInt(observation.data.songNs) >= 750000000n;
        }, request.playId);
        if (covered) break;
      }
    } finally { await page.keyboard.up("z"); }
    assert.ok(covered, "Actual original-song progress must cover the keyboard acquisition interval before its bounded deadline");
    await terminal(request.playId); const result = await snapshot(`live-${fraction.replace("/", "-")}`);
    const { messages, meta, stopped, frame } = assertCompletion(result, request, reportOffset, 2000000000n, n, d);
    assert.equal(meta.liveSpeed, fraction); assert.equal(meta.liveSpeedNumerator, String(n)); assert.equal(meta.liveSpeedDenominator, String(d));
    assert.ok(BigInt(stopped.hits) > 0n, "Actual keyboard acquisition must judge the fixture note");
    const captured = result.worker.captures.find(x => x.playId === request.playId && x.bytes); assert.ok(captured?.complete);
    const decoded = decodeRecord(captured.bytes), inputs = decoded.records.filter(x => x.input); assert.ok(inputs.length >= 2);
    assert.ok(inputs.some(x => x.songNs >= 450000000n && x.songNs <= 550000000n), "Captured original inputs must cover the fixture note window");
    const acquired = messages.filter(x => x.direction === "sent" && x.data.kind === "play-step").flatMap(x => x.data.events);
    for (const record of inputs) {
      const expected = (record.input.normalizedNs - 100000000n) * n / d;
      assert.ok(record.songNs - expected >= -2n && record.songNs - expected <= 2n, `Original captured timing mismatch ${record.songNs - expected} ns`);
      assert.equal(record.input.variant, 0); assert.equal(record.input.source, 1n);
      assert.ok(acquired.some(x => String(x.sequence) === String(record.input.sequence) && String(x.hostNs) === String(record.input.originalNs)), "Captured provenance must match original Window acquisition facts");
    }
    const prefix = Buffer.from("bms-speed-setup/v1:");
    if (n === d) assert.ok(!decoded.fields[3].subarray(0, prefix.length).equals(prefix));
    else { assert.deepEqual(decoded.fields[3].subarray(0, prefix.length), prefix); assert.equal(decoded.fields[3].readBigInt64LE(prefix.length + 4), n); assert.equal(decoded.fields[3].readBigUInt64LE(prefix.length + 12), d); }
    const events = result.worker.events.slice(eventOffset), commands = events.filter(x => x.method === "commands" && x.result).flatMap(x => x.result.commands);
    if (n !== d) { const prefix = commands.findIndex(x => x.kind === 2 && x.value === String(n) && x.denominator === String(d)); assert.ok(prefix >= 0); assert.equal(commands[prefix].at, "0"); assert.ok(commands.findIndex(x => x.kind === 0) > prefix, "Actual origin rate precedes all sounds"); }
    const music = commands.find(x => x.kind === 0 && x.sample === "1"); assert.ok(music); assert.equal(music.at, "100000000");
    const keys = commands.filter(x => x.kind === 0 && x.sample === "2"); assert.ok(keys.length > 0, "Actual judged keysound commands required");
    for (const command of keys) assert.ok(events.some(x => x.method === "service_audio" && x.args[1] === command.at), "Keysound stays at actual raw service output audio_at, without song-rate multiplication");
    const pcm = result.window.pcm.filter(x => x.index === contextIndex); assert.ok(pcm.some(x => Math.abs(x.frequency - pitch) <= 50), `Actual Worklet pitch ${pitch} Hz required`);
    const progress = messages.filter(x => x.data.kind === "play-render-done" && x.data.songNs !== null && BigInt(x.data.songNs) > 100000000n && BigInt(x.data.songNs) < 1800000000n);
    // Dynamic packets inherit the registered mode; the actual draw receipt
    // identifies the mode that the renderer presented for this exact packet.
    const drawn = result.renderer.frames.filter(x => progress.some(y => y.data.songNs === x.songNs) && result.renderer.receipts.some(y => y.mode === "live" && y.generation === x.generation && y.content === x.content && y.sequence === x.sequence));
    assert.ok(new Set(drawn.map(x => x.songNs)).size >= 2, "Actual renderer must draw advancing original-song progress");
    assert.ok(result.window.statuses.some(x => x.startsWith("Playing") && x.includes(fraction)));
    assert.equal(await page.$eval("#live-speed", x => x.disabled), false);
    if (fraction === "3/2") { saved = Buffer.from(captured.bytes); savedHash = sha(saved); savedTerminal = stopped; await writeFile(resolve(out, "source-3-2.bkr"), saved); }
    check(fraction, { playId: request.playId, frame: String(frame), inputs: inputs.length, hits: stopped.hits, rawKeysounds: keys.length, pitch, actualDrawnFrames: drawn.length });
  }
  evidence.stage = "saved-record"; await page.click("#records-save");
  await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Recording saved.") && !document.querySelector("#records").disabled, { timeout: 15000 });
  const recordId = await page.$eval("#records", x => [...x.options].find(option => option.value !== "")?.value); assert.ok(recordId);
  await page.select("#records", recordId); await page.click("#records-use");
  await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Saved replay selected.") && !document.querySelector("#replay-play").disabled, { timeout: 15000 });
  check("saved-record", { id: recordId, sourceSpeed: "3/2", sha256: savedHash });
  evidence.stage = "quantized-end"; await field("live-speed", "2/1"); await field("live-end", "0.000000001");
  const quantizedOffset = await worker.evaluate(() => __liveSpeedWorker.reports.length), quantized = await start(); await terminal(quantized.playId);
  assertCompletion(await snapshot("quantized-end"), quantized, quantizedOffset, 1n, 2n, 1n); check("quantized-end", "Actual 1 ns original interval closes only after genuine preroll output fence");
  // The selected replay comes through the actual IndexedDB saved-record flow;
  // the exported original bytes are retained separately for immutable hash proof.
  evidence.stage = "independent-replay"; await field("replay-speed", "1/2");
  const replayContext = await page.evaluate(() => __liveSpeedWindow.contexts.length), replayOffset = await worker.evaluate(() => __liveSpeedWorker.reports.length), replay = await start("replay-play"); await terminal(replay.playId);
  const replayResult = await snapshot("independent-replay"), replaySession = assertCompletion(replayResult, replay, replayOffset, 2000000000n, 1n, 2n);
  assert.deepEqual(replayResult.window.fileErrors, []);
  const selectedBytes = replayResult.window.replayFiles.find(x => x.playId === String(replay.playId))?.bytes; assert.ok(selectedBytes); assert.equal(sha(Buffer.from(selectedBytes)), savedHash, "Actual saved replay launch retains original capture bytes");
  assert.equal(replaySession.meta.replaySpeed, "1/2"); assert.equal(replaySession.stopped.hits, savedTerminal.hits); assert.equal(replaySession.stopped.misses, savedTerminal.misses);
  assert.ok(replayResult.window.pcm.some(x => x.index === replayContext && Math.abs(x.frequency - 440) <= 50), "Independent replay output uses selected 1/2 pitch, not recorded 3/2");
  assert.equal(sha(await readFile(resolve(out, "source-3-2.bkr"))), savedHash); check("independent-replay", { source: "3/2", output: "1/2", sha256: savedHash });
  evidence.stage = "opponent-match"; await field("live-end", "2"); await field("live-speed", "3/2"); await page.click("#opponents-add");
  await page.waitForFunction(() => document.querySelector("#opponents-list").children.length === 1);
  const match = await start(); await terminal(match.playId); const matched = await snapshot("opponent-match"), matchMessages = session(matched, match.playId);
  assert.equal(metadata(matchMessages).opponentCount, 1); assert.ok(!matchMessages.some(x => x.data.kind === "play-error")); check("opponent-match", "Canonical recorded 3/2 admitted for selected 3/2");
  evidence.stage = "opponent-refusal"; await field("live-speed", "2/1"); const refused = await start(); await terminal(refused.playId);
  const refusal = session(await snapshot("opponent-refusal"), refused.playId); assert.ok(refusal.some(x => x.data.kind === "play-error")); assert.ok(!refusal.some(x => x.data.kind === "play-activate"));
  check("opponent-refusal", "Different source conditions refuse before activation"); await page.click("#opponents-clear");
  evidence.stage = "nonzero-section";
  await field("live-start", "0.25"); await field("live-end", "2"); await field("live-speed", "3/2");
  const section = await actualSection("nonzero-section");
  check("nonzero-section", { playId: section.request.playId, startNs: "250000000", endNs: "2000000000", frame: section.frame, inputs: section.inputs, hits: section.hits, drawn: section.drawn, sha256: section.sha256 });
  evidence.stage = "seek-restart";
  const seekOffset = await page.evaluate(() => __liveSpeedWindow.messages.length);
  const previewOffset = await renderer.evaluate(() => __liveSpeedRenderer.previews.length);
  const previewReceiptOffset = await renderer.evaluate(() => __liveSpeedRenderer.receipts.length);
  const selectedId = await page.evaluate(() => __liveSpeedWindow.messages.findLast(x => x.direction === "received" && x.data.kind === "selected")?.data.id);
  assert.ok(Number.isSafeInteger(selectedId) && selectedId > 0);
  await page.waitForFunction(() => !document.querySelector("#position").disabled && !document.querySelector("#seek").disabled, { timeout: 15000 });
  await field("position", "1.25"); await page.click("#seek");
  await page.waitForFunction(({ offset, selectedId }) => {
    const messages = __liveSpeedWindow.messages.slice(offset), request = messages.find(x => x.direction === "sent" && x.data.kind === "seek")?.data;
    return request?.selectedId === selectedId && messages.some(x => x.direction === "received" && x.data.kind === "position"
      && x.data.id === request.id && x.data.selectedId === selectedId && x.data.ns === "1250000000");
  }, { timeout: 15000 }, { offset: seekOffset, selectedId });
  const seekMessages = await page.evaluate(offset => __liveSpeedWindow.messages.slice(offset), seekOffset);
  const seekRequest = seekMessages.find(x => x.direction === "sent" && x.data.kind === "seek")?.data;
  assert.ok(seekRequest); assert.equal(seekRequest.selectedId, selectedId); assert.equal(seekRequest.ns, "1250000000");
  assert.ok(!seekMessages.some(x => x.direction === "received" && x.data.kind === "seek-error" && x.data.id === seekRequest.id));
  const previewDraw = await renderer.evaluate(async ({ previewOffset, receiptOffset }) => {
    const deadline = performance.now() + 10000;
    while (performance.now() < deadline) {
      const probe = __liveSpeedRenderer, receipt = probe.previews.slice(previewOffset).find(frame => frame.songNs === "1250000000"
        && probe.receipts.slice(receiptOffset).some(draw => draw.mode === "preview" && draw.generation === frame.generation && draw.content === frame.content && draw.sequence === frame.sequence));
      if (receipt) return receipt;
      await new Promise(done => setTimeout(done, 10));
    }
    throw Error("Actual sought preview draw deadline");
  }, { previewOffset, receiptOffset: previewReceiptOffset });
  await page.waitForFunction(({ selectedId, generation, content }) => __liveSpeedWindow.messages.some(x => x.direction === "received"
    && x.data.kind === "drawn" && x.data.selectedId === selectedId && x.data.mode === "preview"
    && String(x.data.generation) === generation && String(x.data.content) === content), { timeout: 10000 }, { selectedId, ...previewDraw });
  assert.equal(await page.$eval("#position", x => x.value), "1.25");
  await snapshot("seek-preview");
  // Preview seeking is a separate operation. Restart the explicit selected
  // section, rather than silently using the preview position as its origin.
  assert.equal(await page.$eval("#live-start", x => x.value), "0.25");
  assert.equal(await page.$eval("#live-end", x => x.value), "2");
  const restart = await actualSection("seek-restart");
  assert.ok(restart.request.playId > section.request.playId); assert.ok(restart.contextIndex > section.contextIndex);
  assert.equal(restart.frame, section.frame); assert.equal(sha(await readFile(resolve(out, "nonzero-section.bkr"))), section.sha256);
  check("seek-restart", { selectedId, seekId: seekRequest.id, previewNs: previewDraw.songNs, previewDraw, firstPlayId: section.request.playId, restartedPlayId: restart.request.playId, frame: restart.frame, inputs: restart.inputs, firstContext: section.contextIndex, restartedContext: restart.contextIndex, sha256: restart.sha256 });
  await field("live-start", "0");
  evidence.stage = "touch-local";
  if (!await page.$eval("#local-count", element => element.closest("details").open)) {
    await page.click("details.binding-editor:has(#local-count) > summary");
  }
  await toggle("touch-input", true); await field("local-count", "2");
  await page.waitForFunction(() => document.querySelector("#local-source-1") && document.querySelector("#local-source-2")
    && !document.querySelector("#local-discover").disabled, { timeout: 15000 });
  await page.click("#local-discover");
  await page.waitForFunction(() => document.querySelector("#local-source-1")?.options.length >= 3 && document.querySelector("#local-source-2")?.options.length >= 3, { timeout: 15000 });
  await page.select("#local-source-1", "1"); await page.select("#local-source-2", "2"); await field("live-speed", "3/2");
  const local = await start(); await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Playing"), { timeout: 30000 });
  await page.waitForFunction(id => __liveSpeedWindow.messages.some(x => x.direction === "received"
    && x.data.playId === id && x.data.kind === "play-render-done"
    && x.data.songNs != null && BigInt(x.data.songNs) >= 200000000n), { timeout: 15000 }, local.playId);
  await page.keyboard.press("z");
  await page.$eval("#canvas", element => element.scrollIntoView({ block: "center" }));
  const canvas = await page.$eval("#canvas", x => { const r = x.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; });
  const cdp = await page.createCDPSession();
  const bounds = await worker.evaluate(() => __liveSpeedWorker.events.findLast(x => x.method === "configure_touch_regions" && __liveSpeedWorker.owners[x.owner].name === "BrowserLocalGame")?.args[2]);
  assert.ok(Array.isArray(bounds) && bounds.length === 4, "Read actual admitted one-lane local touch geometry");
  const point = { x: canvas.x + canvas.width * (bounds[0] + bounds[2]) / 2 / 960, y: canvas.y + canvas.height * (bounds[1] + bounds[3]) / 2 / 720, id: 1 };
  assert.ok(await page.evaluate(point => point.x >= 0 && point.y >= 0 && point.x < innerWidth && point.y < innerHeight, point), "Actual touch must land inside the visible browser viewport");
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [point] }); await sleep(40); await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] }); await cdp.detach();
  await terminal(local.playId); const localResult = await snapshot("touch-local"), localMessages = session(localResult, local.playId);
  assert.ok(!localMessages.some(x => x.data.kind === "play-error"), json(localMessages)); assert.equal(metadata(localMessages).liveSpeed, "3/2");
  assert.equal(await page.$eval("#local-results", x => x.children.length), 2);
  const actualLocal = localResult.worker.events.filter(x => localResult.worker.owners[x.owner].name === "BrowserLocalGame");
  assert.ok(actualLocal.some(x => x.method === "queue_input_blob"), "Actual local keyboard acquisition required");
  assert.ok(actualLocal.some(x => x.method === "queue_input_blob_on_surface_on_page"), "Actual local touch acquisition required");
  const memberCaptures = localResult.worker.captures.filter(x => x.playId === local.playId && x.bytes && x.complete); assert.equal(memberCaptures.length, 2);
  assert.ok(memberCaptures.some(x => decodeRecord(x.bytes).records.some(record => record.input?.variant === 2 && record.input.source === 2n)), "The actual member capture must contain bound touch provenance");
  check("touch-local", "Distinct actual keyboard and touch aggregates enter one generated local owner");
  await field("local-count", "1"); await toggle("touch-input", false);
  evidence.stage = "stop-retry"; await field("live-speed", "1/2"); const interrupted = await start(); await page.waitForFunction(() => !document.querySelector("#stop").disabled); await sleep(350); await page.click("#stop"); await terminal(interrupted.playId);
  await field("live-speed", "2/1"); const retry = await start(); await terminal(retry.playId); const retryMessages = session(await snapshot("stop-retry"), retry.playId);
  assert.ok(!retryMessages.some(x => x.data.kind === "play-error")); assert.ok(retryMessages.some(x => x.data.kind === "play-render-done" && x.data.completed)); check("stop-retry", { interrupted: interrupted.playId, retry: retry.playId });
  evidence.stage = "responsive";
  for (const width of [390, 768, 1200]) { await page.setViewport({ width, height: 1000, hasTouch: true }); await sleep(150); await page.screenshot({ path: resolve(out, `viewport-${width}.png`), fullPage: true }); assert.ok(await page.$eval("#live-speed", x => x.getBoundingClientRect().width > 0)); }
  check("responsive", [390, 768, 1200]); await sleep(100);
  assert.deepEqual(await page.evaluate(() => __liveSpeedWindow.audioTerminals), [], "Final acceptance requires no actual Worklet terminal failure");
  assert.equal(evidence.audioWasmResponseOverflow, false);
  assert.equal(evidence.audioWasmResponses.length, 1, "One actual fresh Worker audio WASM response required");
  const [audioWasmResponse] = evidence.audioWasmResponses;
  assert.equal(audioWasmResponse.status, 200); assert.equal(audioWasmResponse.cacheControl, "no-cache");
  assert.equal(audioWasmResponse.fromCache, false); assert.equal(audioWasmResponse.fromServiceWorker, false);
  assert.deepEqual(evidence.console.filter(x => ["error", "pageerror"].includes(x.type)), []); assert.deepEqual(evidence.network, []); check("clean-console-network", "No exclusions, including ERR_ABORTED");
}
try { await run(); }
catch (error) { evidence.failure = { message: String(error), stack: error.stack }; console.error(error); process.exitCode = 1; }
finally {
  evidence.stage = "cleanup";
  if (page && !page.isClosed()) {
    if (evidence.failure) {
      evidence.failureWindow = await page.evaluate(() => ({ probe: globalThis.__liveSpeedWindow ?? null, status: document.querySelector("#status")?.textContent })).catch(error => ({ error: String(error) }));
      evidence.failureWorkers = [];
      for (const item of page.workers()) evidence.failureWorkers.push({ url: item.url(), observation: await item.evaluate(() => globalThis.__liveSpeedWorker ?? globalThis.__liveSpeedRenderer ?? null).catch(error => ({ error: String(error) })) });
      await page.screenshot({ path: resolve(out, "failure.png"), fullPage: true }).catch(error => { evidence.screenshotError = String(error); });
    }
    await bounded(page.close(), 10000, "Owned page cleanup").catch(error => { evidence.cleanupError = String(error); process.exitCode = 1; });
  }
  if (browser) await bounded(browser.close(), 10000, "Owned Chromium cleanup").catch(error => { evidence.cleanupError = String(error); process.exitCode = 1; const owned = browser.process(); if (owned && owned.exitCode === null) owned.kill("SIGKILL"); });
  if (server) { server.closeAllConnections(); await bounded(new Promise(done => server.close(done)), 5000, "Owned HTTPS cleanup").catch(error => { evidence.cleanupError = String(error); process.exitCode = 1; }); }
  const passed = new Set(evidence.checks.filter(x => x.passed).map(x => x.id)); evidence.unmet = required.filter(x => !passed.has(x)); if (evidence.unmet.length) process.exitCode = 1;
  await mkdir(out, { recursive: true }); await writeFile(resolve(out, "evidence.json"), json(evidence)); console.log(json({ checks: evidence.checks, unmet: evidence.unmet, evidence: resolve(out, "evidence.json") }));
}
