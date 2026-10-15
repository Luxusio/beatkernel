/** Actual retained runtime-rate integration, developer evidence (not a QA receipt).
 * Run only after the source writers stop and both matching WASM packages exist:
 * EXPECTED_WASM_SHA256=<ui-sha> EXPECTED_AUDIO_WASM_SHA256=<audio-sha> \
 * CHROMIUM=/usr/bin/chromium \
 * PUPPETEER_MODULE=target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core \
 * RUNTIME_RATE_OUT=target/wf/runtime-rate-browser-<unique> \
 * node app/web/runtime-rate.browser.mjs
 * Observers forward original methods/messages/transfers. They never create a
 * clock, PCM block, ACK, replay, control result or completion on behalf of play.
 */
import assert from "node:assert/strict";
import { createServer } from "node:https";
import { createRequire } from "node:module";
import { readFile, writeFile, mkdir, rm, mkdtemp, statfs } from "node:fs/promises";
import { createHash, X509Certificate } from "node:crypto";
import { resolve, dirname, relative, extname, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
const clean = value => JSON.parse(JSON.stringify(value, (_, v) => typeof v === "bigint" ? String(v) : v));
const delay = ms => new Promise(done => setTimeout(done, ms));
async function bounded(promise, ms, label) {
  let timer;
  try { return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(Error(label + " timeout")), ms); })]); }
  finally { clearTimeout(timer); }
}
const expected = process.env.EXPECTED_WASM_SHA256, expectedAudio = process.env.EXPECTED_AUDIO_WASM_SHA256;
assert.match(expected ?? "", /^[a-f0-9]{64}$/, "EXPECTED_WASM_SHA256 identifies the fresh UI package");
assert.match(expectedAudio ?? "", /^[a-f0-9]{64}$/, "EXPECTED_AUDIO_WASM_SHA256 identifies the fresh audio package");
for (const key of ["CHROMIUM", "PUPPETEER_MODULE", "RUNTIME_RATE_OUT"]) assert(process.env[key], `${key} is required`);
const out = resolve(root, process.env.RUNTIME_RATE_OUT);
assert(relative(root, out).startsWith(`target${sep}wf${sep}`), "evidence stays inside ignored target/wf");
const sources = {};
for (const file of ["index.html", "main.js", "worker.js", "renderer-worker.js", "audio-host.mjs", "audio-worklet.js",
  "audio-command-client.mjs", "audio-sample-client.mjs", "native-audio-clock.mjs", "worklet-native-frame-probe.mjs",
  "worklet-encoding.mjs", "play-model.mjs", "render-protocol.mjs", "record-store.mjs", "completed-results-model.mjs",
  "pkg/beatkernel_bms_runtime.js", "pkg/beatkernel_bms_runtime_bg.wasm",
  "audio-pkg/beatkernel_bms_runtime.js", "audio-pkg/beatkernel_bms_runtime_bg.wasm"])
  sources[file] = await readFile(resolve(root, "app/web", file));
assert.equal(sha(sources["pkg/beatkernel_bms_runtime_bg.wasm"]), expected);
assert.equal(sha(sources["audio-pkg/beatkernel_bms_runtime_bg.wasm"]), expectedAudio);
await mkdir(out); // An existing evidence directory must never be reused.
const required = ["fresh-artifacts", "solo-rate-continuation", "pause-pending-resume", "invalid-drafts",
  "completed-variable-record", "record-save-load", "independent-replay", "region-loop-disable-scrub-f5",
  "retired-custody", "local-shared-rate", "fixed-opponent-refusal", "clean-console-network"];
const evidence = { kind: "development-runtime-rate-browser", status: "RUNNING", required, checks: [], snapshots: [],
  wasmSha256: expected, audioWasmSha256: expectedAudio, sources: Object.fromEntries(Object.entries(sources).map(([name, b]) => [name, sha(b)])),
  errors: [], consoleErrors: [], networkErrors: [], networkLifecycle: [], networkOverflow: false, cleanup: {},
  ceilings: ["Actual Chromium/SwiftShader/AudioWorklet execution is not physical-device, acoustic latency or performance proof.",
    "Analyser pitch observations are diagnostic; sample-exact continuation and 44.1/48 kHz rational oracles belong to the paired Rust integration suite.",
    "Trusted keyboard/touch acquisition is browser integration evidence, not physical keyboard/HID acceptance.",
    "Bounded read-only observers allocate and add diagnostic overhead; their timing is not production performance evidence."] };
const check = (id, detail) => evidence.checks.push({ id, detail: clean(detail), passed: true });
function diagnostic(list, value, limit = 1000) { if (list.length < limit) list.push(clean(value)); else evidence.networkOverflow = true; }
function network(kind, value) { diagnostic(evidence.networkLifecycle, { order: evidence.networkLifecycle.length, kind, ...value }, 20000); }
let server, browser, page, business, renderer, xvfb, profile, cache, networkSession, attachedListener;
const workerNetworkSessions = new Map(), workerNetworkTasks = new Set();

function observeWindow() {
  const probe = globalThis.__runtimeWindow = { messages: [], pcm: [], contexts: [], input: [], terminals: [], errors: [], overflow: false };
  const add = (list, value, limit = 40000) => { if (list.length < limit) list.push(value); else probe.overflow = true; };
  const safe = value => JSON.parse(JSON.stringify(value, (_, v) => typeof v === "bigint" ? String(v)
    : v instanceof File ? { actualFile: true, name: v.name, size: v.size }
      : v instanceof Blob ? { actualBlob: true, size: v.size } : v instanceof MessagePort ? { actualPort: true }
        : v instanceof WebAssembly.Module ? { actualModule: true } : ArrayBuffer.isView(v) ? Array.from(v) : v));
  const ActualWorker = Worker;
  globalThis.Worker = class extends ActualWorker {
    constructor(url, options) {
      super(url, options); this.probeURL = String(url);
      this.addEventListener("message", ({ data }) => add(probe.messages, { direction: "received", url: this.probeURL, data: safe(data) }));
    }
    postMessage(value, ...args) {
      if (!value?.kind?.includes("samples")) add(probe.messages, { direction: "sent", url: this.probeURL, data: safe(value) });
      return super.postMessage(value, ...args);
    }
  };
  const ActualNode = AudioWorkletNode;
  globalThis.AudioWorkletNode = class extends ActualNode {
    constructor(context, name, options) {
      super(context, name, options);
      const index = probe.contexts.length;
      add(probe.contexts, { index, name, sampleRate: context.sampleRate, generation: options?.processorOptions?.generation }, 128);
      this.port.addEventListener("message", ({ data }) => { if (data?.kind === "terminal") add(probe.terminals, { index, data: safe(data) }, 128); });
      const analyser = context.createAnalyser(); analyser.fftSize = 2048; this.connect(analyser);
      const samples = new Float32Array(analyser.fftSize);
      const observe = () => {
        if (context.state === "closed") return;
        analyser.getFloatTimeDomainData(samples); let peak = 0, crossings = 0;
        for (let i = 0; i < samples.length; i++) { peak = Math.max(peak, Math.abs(samples[i])); if (i && samples[i - 1] <= 0 && samples[i] > 0) crossings++; }
        if (peak > 0.005) add(probe.pcm, { index, peak, frequency: crossings * context.sampleRate / samples.length }, 12000);
        requestAnimationFrame(observe);
      };
      requestAnimationFrame(observe);
    }
  };
  for (const type of ["keydown", "keyup", "pointerdown", "pointerup", "touchstart", "touchend"])
    document.addEventListener(type, event => add(probe.input, { type, trusted: event.isTrusted, code: event.code, target: event.target?.id }), true);
}

async function observeBusiness() {
  const probe = globalThis.__runtimeBusiness = { calls: [], captures: [], loads: [], messages: [], ports: [], setup: [], errors: [], pending: 0, tick: 0, overflow: false };
  const add = (list, value, limit = 40000) => { if (list.length < limit) { if (value && typeof value === "object") value.tick = probe.tick++; list.push(value); } else probe.overflow = true; };
  const safe = value => JSON.parse(JSON.stringify(value, (_, v) => typeof v === "bigint" ? String(v)
    : v instanceof File ? { actualFile: true, name: v.name, size: v.size }
      : v instanceof Blob ? { actualBlob: true, size: v.size } : v instanceof MessagePort ? { actualPort: true }
        : v instanceof WebAssembly.Module ? { actualModule: true } : ArrayBuffer.isView(v) ? Array.from(v) : v));
  // Register receive entry before this observer's first await: sibling static
  // module evaluation can install the product listener while bindings load.
  globalThis.addEventListener("message", ({ data }) => {
    if (data?.kind === "play-retired-recordings-ack") add(probe.messages, { direction: "received", ...safe(data) });
    if (data?.kind === "play-start" && data.replayFile instanceof File) {
      const row = { playId: data.playId, actualFile: true, bytes: null }; add(probe.loads, row, 128); probe.pending++;
      data.replayFile.arrayBuffer().then(bytes => { if (bytes.byteLength > 1048576) throw Error("replay observation ceiling"); row.bytes = Array.from(new Uint8Array(bytes)); })
        .catch(error => add(probe.errors, String(error), 128)).finally(() => probe.pending--);
    }
  });
  const bindings = await import("./pkg/beatkernel_bms_runtime.js"), owners = new WeakMap(); let nextOwner = 0;
  for (const name of ["BrowserGame", "BrowserLocalGame", "BrowserReplay"]) {
    const proto = bindings[name]?.prototype;
    if (!proto) throw Error(`actual generated ${name} missing`);
    for (const method of ["retained_prepare_rate", "retained_prepare_region", "retained_prepare_disable_loop", "retained_prepare_original_restart",
      "retained_prepare_pause", "retained_admission", "retained_observe_presentation", "retained_history", "retained_event", "retained_service",
      "retained_event_ack", "retained_response_words", "retained_history_pin", "retained_capability_words", "take_retired_replay",
      "visual_registration", "queue_input_blob", "queue_input_blob_on_surface_on_page", "configure_touch_regions", "observe_output", "evaluate_completion"]) {
      const original = proto[method]; if (typeof original !== "function") continue;
      proto[method] = function(...args) {
        if (!owners.has(this)) owners.set(this, ++nextOwner);
        const row = { order: probe.calls.length, owner: owners.get(this), name, method, args: safe(args), beforeSong: String(this.song_ns), beforeHits: String(this.hits) };
        try { const result = original.apply(this, args); row.result = safe(result ?? null); return result; }
        catch (error) { row.error = String(error); throw error; }
        finally { row.songNs = String(this.song_ns); row.hits = String(this.hits); add(probe.calls, row); }
      };
    }
  }
  const nativeFetch = globalThis.fetch;
  globalThis.fetch = function(...args) {
    const url = String(args[0]?.url ?? args[0]);
    if (!url.includes("/audio-pkg/beatkernel_bms_runtime_bg.wasm")) return nativeFetch.apply(this, args);
    add(probe.setup, { kind: "fetch-start", url });
    return nativeFetch.apply(this, args).then(response => {
      add(probe.setup, { kind: "fetch-response", status: response.status, contentLength: response.headers.get("content-length") });
      if (response.body) {
        const getReader = response.body.getReader;
        response.body.getReader = function(...readerArgs) {
          const reader = getReader.apply(this, readerArgs), read = reader.read, release = reader.releaseLock, cancel = reader.cancel;
          let bytes = 0; add(probe.setup, { kind: "reader-acquired" });
          reader.read = function(...readArgs) { return read.apply(this, readArgs).then(value => { bytes += value.value?.byteLength ?? 0; add(probe.setup, { kind: "reader-read", bytes, done: value.done }); return value; }); };
          reader.releaseLock = function(...releaseArgs) { add(probe.setup, { kind: "reader-release", bytes }); return release.apply(this, releaseArgs); };
          reader.cancel = function(...cancelArgs) { add(probe.setup, { kind: "reader-cancel", bytes }); return cancel.apply(this, cancelArgs); };
          return reader;
        };
      }
      return response;
    });
  };
  const nativeCompile = WebAssembly.compile;
  WebAssembly.compile = function(bytes, ...args) {
    const byteLength = bytes.byteLength; add(probe.setup, { kind: "compile-start", byteLength });
    return nativeCompile.call(this, bytes, ...args).then(module => { add(probe.setup, { kind: "compile-done", byteLength, actualModule: module instanceof WebAssembly.Module }); return module; });
  };
  const post = globalThis.postMessage;
  globalThis.postMessage = function(value, ...args) {
    if (["play-stopped", "play-error", "play-retired-recordings", "play-retained-result", "records-reply"].includes(value?.kind)) add(probe.messages, safe(value));
    const rows = ["play-stopped", "play-error", "play-retired-recordings"].includes(value?.kind)
      ? value.replays ?? [{ player: 1, replay: value.replay, replayComplete: value.replayComplete }] : [];
    for (const row of rows) {
      if (!(row.replay instanceof File)) continue;
      if (row.replay.size < 1 || row.replay.size > 1048576 || probe.captures.length >= 128) throw Error("capture observation ceiling");
      const item = { kind: value.kind, playId: value.playId, player: row.player ?? 1, programId: String(value.programId ?? ""),
        boundarySerial: value.boundarySerial === undefined ? null : String(value.boundarySerial), complete: row.replayComplete ?? value.replayComplete,
        actualFile: true, size: row.replay.size, bytes: null };
      add(probe.captures, item, 128); probe.pending++;
      row.replay.arrayBuffer().then(bytes => { item.bytes = Array.from(new Uint8Array(bytes)); })
        .catch(error => add(probe.errors, String(error), 128)).finally(() => probe.pending--);
    }
    return post.call(this, value, ...args);
  };
  const nativePortPost = MessagePort.prototype.postMessage;
  MessagePort.prototype.postMessage = function(value, ...args) {
    if (value?.kind?.startsWith("retained-")) add(probe.ports, { direction: "sent", data: safe(value) }, 20000);
    return nativePortPost.call(this, value, ...args);
  };
  const descriptor = Object.getOwnPropertyDescriptor(MessagePort.prototype, "onmessage");
  if (!descriptor?.get || !descriptor?.set) throw Error("actual MessagePort descriptor missing");
  Object.defineProperty(MessagePort.prototype, "onmessage", { ...descriptor, set(listener) {
    return descriptor.set.call(this, listener === null ? null : function(event) {
      if (event.data?.retained) add(probe.ports, { direction: "received", data: safe(event.data) }, 20000);
      return listener.call(this, event);
    });
  } });
}

function observeRenderer() {
  const probe = globalThis.__runtimeRenderer = { records: [], errors: [], overflow: false };
  const record = (direction, value) => {
    if (probe.records.length >= 40000) { probe.overflow = true; return; }
    const row = { order: probe.records.length, direction, kind: value?.kind, mode: value?.mode,
      generation: String(value?.generation ?? ""), content: String(value?.content ?? ""), sequence: String(value?.sequence ?? "") };
    const bytes = value?.packet;
    if (bytes instanceof Uint8Array) {
      if (bytes.length < 40 || bytes.length > 1048576) throw Error("actual renderer packet extent");
      const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      row.packet = { kind: view.getUint16(6, true), generation: String(view.getBigUint64(8, true)),
        content: String(view.getBigUint64(16, true)), sequence: String(view.getBigUint64(24, true)) };
      if (row.packet.kind === 2 && bytes.length >= 69) row.packet.songNs = String(view.getBigInt64(61, true));
    }
    probe.records.push(row);
  };
  const post = MessagePort.prototype.postMessage, start = MessagePort.prototype.start, seen = new WeakSet();
  MessagePort.prototype.postMessage = function(value, ...args) { record("sent", value); return post.call(this, value, ...args); };
  MessagePort.prototype.start = function(...args) {
    if (!seen.has(this)) { seen.add(this); this.addEventListener("message", ({ data }) => { try { record("received", data); } catch (error) { probe.errors.push(String(error)); } }); }
    return start.apply(this, args);
  };
}

// A bounded independent reader: no production decoder/encoder supplies these
// expected source conditions, original input facts or variable cut positions.
function decodeRecord(bytes) {
  const b = Buffer.from(bytes); assert(b.length > 0 && b.length <= 1048576); let p = 0;
  const take = n => { assert(Number.isSafeInteger(n) && n >= 0 && p + n <= b.length, "record extent"); return b.subarray(p, p += n); };
  const u64 = () => take(8).readBigUInt64LE(), i64 = () => take(8).readBigInt64LE();
  const blob = () => { const n = u64(); assert(n <= 1048576n); return take(Number(n)); };
  assert.equal(take(8).toString(), "BKREPLAY"); assert.equal(take(4).readUInt32LE(), 1);
  take(4); const domain = take(4).readUInt32LE(); u64(); const fields = [blob(), blob(), blob(), blob()];
  const calibration = take(1)[0]; assert(calibration <= 1); if (calibration) blob();
  const count = u64(); assert(count <= 20000n); const records = [];
  for (let i = 0n; i < count; i++) {
    assert.equal(u64(), i); const songNs = i64(), tag = take(1)[0]; assert(tag <= 1); let input = null;
    if (tag === 0) {
      const lane = take(4).readUInt32LE(), event = blob(); let q = 0;
      const e = n => { assert(q + n <= event.length); return event.subarray(q, q += n); };
      const option = read => { const present = e(1)[0]; assert(present <= 1); if (present) return read(); return null; };
      assert.equal(e(4).toString(), "BKPI"); assert.equal(e(2).readUInt16LE(), 1);
      const variant = e(1)[0], source = e(8).readBigUInt64LE(), normalizedNs = e(8).readBigInt64LE(), normalizedDomain = e(4).readUInt32LE(), sequence = e(8).readBigUInt64LE();
      option(() => { e(4); option(() => e(4)); option(() => e(12)); });
      const original = option(() => ({ domain: e(4).readUInt32LE(), ns: e(8).readBigInt64LE() }));
      assert.equal(normalizedDomain, domain); assert(original, "original HOST provenance must survive capture");
      input = { lane, variant, source, normalizedNs, sequence, original };
    }
    records.push({ ordinal: i, songNs, input });
  }
  assert.equal(p, b.length);
  const options = fields[3], v1 = Buffer.from("bms-speed-setup/v1:"), v2 = Buffer.from("bms-speed-setup/v2:");
  let initial = [1n, 1n], cuts = [], inner = options;
  if (options.subarray(0, v1.length).equals(v1) || options.subarray(0, v2.length).equals(v2)) {
    const variable = options.subarray(0, v2.length).equals(v2); let q = v1.length;
    assert(options.length >= q + (variable ? 24 : 20));
    const n = options.readUInt32LE(q); q += 4; initial = [options.readBigInt64LE(q), options.readBigUInt64LE(q + 8)]; q += 16;
    if (variable) {
      const count = options.readUInt32LE(q); q += 4; assert(count > 0 && count <= 4096);
      for (let i = 0; i < count; i++) {
        assert(q + 48 <= options.length);
        const unsigned = options.readBigUInt64LE(q) | options.readBigUInt64LE(q + 8) << 64n;
        cuts.push({ whole: BigInt.asIntN(128, unsigned), numerator: options.readBigUInt64LE(q + 16), denominator: options.readBigUInt64LE(q + 24), rate: [options.readBigInt64LE(q + 32), options.readBigUInt64LE(q + 40)] }); q += 48;
      }
    }
    assert.equal(options.length, q + n); inner = options.subarray(q);
  }
  return { fields, records, initial, cuts, inner };
}
const word = (words, index) => BigInt(words[index * 2]) | BigInt(words[index * 2 + 1]) << 32n;
function capability(words, program) {
  assert.equal(words.length, 16); assert.equal(word(words, 0), 1n); assert.equal(word(words, 1), BigInt(program));
  assert(word(words, 6) > 0n && word(words, 7) > 0n);
  return { generation: word(words, 2), revision: word(words, 5), rate: [word(words, 6), word(words, 7)] };
}
function eventWords(words, program) {
  assert.equal(words.length, 80); assert.equal(word(words, 0), 1n); assert.equal(word(words, 2), BigInt(program)); assert(word(words, 3) > 0n);
  return { tag: word(words, 1), serial: word(words, 3), id: word(words, 4), generation: word(words, 5), iteration: word(words, 6),
    physical: word(words, 7), playback: word(words, 8), revision: word(words, 10), rate: [word(words, 15), word(words, 16)] };
}

function fixtureWav() {
  const hz = 48000, frames = hz * 10, bytes = Buffer.alloc(44 + frames * 2);
  bytes.write("RIFF"); bytes.writeUInt32LE(bytes.length - 8, 4); bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(hz, 24); bytes.writeUInt32LE(hz * 2, 28); bytes.writeUInt16LE(2, 32); bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36); bytes.writeUInt32LE(frames * 2, 40);
  for (let frame = 0; frame < frames; frame++) bytes.writeInt16LE(Math.round(Math.sin(frame * Math.PI * 1760 / hz) * 2048), 44 + frame * 2);
  return bytes;
}
async function startServer() {
  const chart = "#TITLE Runtime rate actual browser\n#BPM 120\n#TOTAL 320\n#WAV01 runtime.wav\n#00001:01\n#00011:00010001\n#00111:00010001\n#00211:00010001\n#00311:00010001\n";
  const wav = fixtureWav(); await writeFile(resolve(out, "runtime.bms"), chart); await writeFile(resolve(out, "runtime.wav"), wav);
  evidence.fixture = { chart: sha(chart), wav: sha(wav) };
  const cert = resolve(out, "cert.pem"), key = resolve(out, "key.pem");
  const created = spawnSync("openssl", ["req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes",
    "-keyout", key, "-out", cert, "-days", "1", "-subj", "/CN=localhost", "-addext", "subjectAltName=IP:127.0.0.1,DNS:localhost"], { encoding: "utf8", timeout: 10000 });
  assert.equal(created.status, 0, created.stderr);
  const certBytes = await readFile(cert), spki = createHash("sha256").update(new X509Certificate(certBytes).publicKey.export({ type: "spki", format: "der" })).digest("base64");
  server = createServer({ cert: certBytes, key: await readFile(key) }, async (request, response) => {
    try {
      const pathname = decodeURIComponent(new URL(request.url, "https://localhost").pathname);
      let file = resolve(root, "." + pathname); const from = relative(root, file);
      if (from === ".." || from.startsWith(".." + sep)) throw Error("outside root");
      if (pathname === "/favicon.ico") { response.writeHead(204); response.end(); return; }
      if (pathname.endsWith("/")) file = resolve(file, "index.html");
      const name = relative(resolve(root, "app/web"), file);
      const bytes = pathname === "/app/web/worker.js" ? Buffer.from("import './runtime-rate-business-observer.js';\nimport './runtime-rate-actual-worker.js';\n")
        : pathname === "/app/web/runtime-rate-actual-worker.js" ? sources["worker.js"]
          : pathname === "/app/web/runtime-rate-business-observer.js" ? Buffer.from(`await (${observeBusiness.toString()})();`)
            : pathname === "/app/web/renderer-worker.js" ? Buffer.from("import './runtime-rate-render-observer.js';\nimport './runtime-rate-actual-renderer.js';\n")
              : pathname === "/app/web/runtime-rate-actual-renderer.js" ? sources["renderer-worker.js"]
                : pathname === "/app/web/runtime-rate-render-observer.js" ? Buffer.from(`(${observeRenderer.toString()})();`)
                  : sources[name] ?? await readFile(file);
      if (name.endsWith(".wasm")) network("server-wasm-body", { name, byteLength: bytes.byteLength, sha256: sha(bytes) });
      response.writeHead(200, { "Content-Length": String(bytes.byteLength), "Content-Type": ({ ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".css": "text/css" })[extname(file)] ?? "application/octet-stream",
        "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cache-Control": "no-store" }); response.end(bytes);
    } catch (error) { diagnostic(evidence.networkErrors, { serverError: String(error), url: request.url }); response.writeHead(404); response.end(); }
  });
  await bounded(new Promise((done, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", done); }), 5000, "HTTPS listen");
  evidence.base = `https://127.0.0.1:${server.address().port}`;
  return spki;
}
async function installNetworkObservation() {
  networkSession = await page.createCDPSession(); await networkSession.send("Network.enable");
  attachedListener = event => {
    const task = (async () => {
      let session;
      try {
        session = networkSession.connection().session(event.sessionId); assert(session, "attached target session missing");
        if (event.targetInfo.type !== "worker") return;
        assert(workerNetworkSessions.size < 16, "Worker target observation ceiling");
        const requests = new Map(), listeners = [];
        const target = { targetId: event.targetInfo.targetId, targetURL: event.targetInfo.url, sessionId: event.sessionId };
        const on = (name, callback) => { session.on(name, callback); listeners.push([name, callback]); };
        workerNetworkSessions.set(event.sessionId, { session, listeners });
        on("Network.requestWillBeSent", value => {
          if (requests.size >= 1000) { evidence.networkOverflow = true; return; }
          requests.set(value.requestId, value.request.url);
          network("worker-cdp-request", { ...target, requestId: value.requestId, url: value.request.url });
        });
        on("Network.responseReceived", value => {
          if (value.response.status >= 400) diagnostic(evidence.networkErrors, { ...target, url: value.response.url, status: value.response.status });
          network("worker-cdp-response", { ...target, requestId: value.requestId, url: value.response.url, status: value.response.status, mimeType: value.response.mimeType });
        });
        on("Network.loadingFinished", value => { network("worker-cdp-finished", { ...target, url: requests.get(value.requestId), ...value }); requests.delete(value.requestId); });
        on("Network.loadingFailed", value => { const detail = { ...target, url: requests.get(value.requestId), ...value }; diagnostic(evidence.networkErrors, detail); network("worker-cdp-failed", detail); requests.delete(value.requestId); });
        await bounded(session.send("Network.enable"), 5000, "Worker Network enable"); network("worker-cdp-enabled", target);
      } catch (error) { diagnostic(evidence.errors, "Worker network observation: " + String(error)); }
      finally { if (session) try { await bounded(session.send("Runtime.runIfWaitingForDebugger"), 5000, "attached target resume"); } catch (error) { diagnostic(evidence.errors, String(error)); } }
    })();
    workerNetworkTasks.add(task); task.finally(() => workerNetworkTasks.delete(task));
  };
  networkSession.on("Target.attachedToTarget", attachedListener);
  await bounded(networkSession.send("Target.setAutoAttach", { autoAttach: true, waitForDebuggerOnStart: true, flatten: true }), 5000, "Worker autoattach");
  page.on("pageerror", error => diagnostic(evidence.errors, String(error)));
  page.on("console", message => { if (message.type() === "error") diagnostic(evidence.consoleErrors, message.text()); });
  page.on("requestfailed", request => { const detail = { url: request.url(), resourceType: request.resourceType(), failure: request.failure() }; diagnostic(evidence.networkErrors, detail); network("page-failed", detail); });
  page.on("response", response => { if (response.status() >= 400) diagnostic(evidence.networkErrors, { url: response.url(), status: response.status() }); });
}

const messages = () => page.evaluate(() => __runtimeWindow.messages);
const businessState = () => business.evaluate(() => __runtimeBusiness);
const field = (id, value) => page.$eval(`#${id}`, (element, value) => { element.value = value; element.dispatchEvent(new Event("input", { bubbles: true })); element.dispatchEvent(new Event("change", { bubbles: true })); }, value);
async function toggle(id, value) { if (await page.$eval(`#${id}`, element => element.checked) !== value) await page.click(`#${id}`); }
async function snapshot(name) {
  await bounded((async () => { while (await business.evaluate(() => __runtimeBusiness.pending)) await delay(20); })(), 5000, "File observation settlement");
  const value = { name, window: await page.evaluate(() => __runtimeWindow), business: await businessState(), renderer: await renderer.evaluate(() => __runtimeRenderer) };
  assert.equal(value.window.overflow, false); assert.equal(value.business.overflow, false); assert.equal(value.renderer.overflow, false);
  assert.deepEqual(value.window.terminals, []); assert.deepEqual(value.window.errors, []); assert.deepEqual(value.business.errors, []); assert.deepEqual(value.renderer.errors, []);
  await writeFile(resolve(out, `${name}.json`), JSON.stringify(clean(value), null, 2));
  await page.screenshot({ path: resolve(out, `${name}.png`), fullPage: true });
  await (await page.$("#canvas")).screenshot({ path: resolve(out, `${name}-canvas.png`) });
  evidence.snapshots.push({ name, file: `${name}.json` }); return value;
}
async function start(button = "play") {
  // Keep startup CDP results bounded while the real250ms arm/activate deadline
  // is pending. The complete original log remains available to later snapshots.
  const offset = await page.evaluate(() => __runtimeWindow.messages.length); await page.click(`#${button}`);
  await page.waitForFunction(offset => __runtimeWindow.messages.slice(offset).some(row => row.direction === "sent" && row.data.kind === "play-start"), {}, offset);
  const request = await page.evaluate(offset => __runtimeWindow.messages.slice(offset).find(row => row.direction === "sent" && row.data.kind === "play-start").data, offset);
  const wait = async phase => {
    await page.waitForFunction(({ id, offset, phase }) => {
    const rows = __runtimeWindow.messages.slice(offset).filter(row => row.direction === "received" && row.data.playId === id);
    const failure = rows.find(row => row.data.kind === "play-error" || row.data.kind === "play-reply" && row.data.error != null);
    const stopped = rows.find(row => row.data.kind === "play-stopped");
    if (failure || stopped) return true;
    if (phase === "prepared") return rows.some(row => row.data.kind === "play-reply" && row.data.result?.kind === "prepared");
    if (!document.querySelector("#status").textContent.startsWith("Playing")) return false;
    if (phase === "playing") return true;
    const rendered = rows.findLast(row => row.data.kind === "play-render-done");
    const actual = rendered?.data.retainedCapability;
    if (actual == null) return false;
    if (!Array.isArray(actual) || actual.length !== 16) return true;
    return !document.querySelector("#runtime-controls").hidden
      && !document.querySelector("#runtime-rate-apply").disabled
      && !document.querySelector("#runtime-pause").disabled;
    }, {}, { id: request.playId, offset, phase });
    // A Puppeteer polling exception can become pageerror while polling keeps
    // running. Settle on the actual correlated receipt, then report in Node.
    const receipts = await page.evaluate(({ id, offset }) => {
      const rows = __runtimeWindow.messages.slice(offset).filter(row => row.direction === "received" && row.data.playId === id);
      return {
        failure: rows.find(row => row.data.kind === "play-error" || row.data.kind === "play-reply" && row.data.error != null)?.data ?? null,
        stopped: rows.find(row => row.data.kind === "play-stopped")?.data ?? null,
        prepared: rows.find(row => row.data.kind === "play-reply" && row.data.result?.kind === "prepared")?.data.result ?? null,
        readiness: rows.findLast(row => row.data.kind === "play-render-done") ?? null,
      };
    }, { id: request.playId, offset });
    if (receipts.failure) throw Error(`Actual play ${request.playId} failed while awaiting ${phase}: ${JSON.stringify(receipts.failure)}`);
    if (receipts.stopped) throw Error(`Actual play ${request.playId} stopped before ${phase}: ${JSON.stringify(receipts.stopped)}`);
    if (phase === "retained-ready") {
      const actual = receipts.readiness?.data.retainedCapability;
      assert(Array.isArray(actual) && actual.length === 16, "Actual Started render reply has malformed retained capability");
    }
    return receipts;
  };
  const prepared = (await wait("prepared")).prepared;
  assert(prepared);
  await wait("playing");
  let readiness = null;
  if (request.mode === "live") {
    assert.equal(request.retained, true); assert.equal(prepared.retained, true); assert.equal(prepared.endFrame, undefined); assert(BigInt(prepared.programId) > 0n);
    assert.equal(prepared.capability, null, "cold preparation cannot claim actual Started permission");
    readiness = (await wait("retained-ready")).readiness;
    assert(readiness); capability(readiness.data.retainedCapability, prepared.programId);
  }
  return { request, prepared, offset, readiness };
}
async function terminal(session, complete = true) {
  await page.waitForFunction(id => __runtimeWindow.messages.some(row => row.direction === "received" && row.data.playId === id && ["play-stopped", "play-error"].includes(row.data.kind)), { timeout: 45000 }, session.request.playId);
  const rows = (await messages()).slice(session.offset).filter(row => row.data.playId === session.request.playId);
  const stopped = rows.find(row => row.direction === "received" && ["play-stopped", "play-error"].includes(row.data.kind)).data;
  assert.equal(stopped.kind, "play-stopped", JSON.stringify(stopped));
  await page.waitForFunction(() => document.querySelector("#stop").disabled && !document.querySelector("#local-count").disabled && !document.querySelector("#live-speed").disabled);
  if (complete) assert(rows.some(row => row.data.kind === "play-render-done" && row.data.completed === true), "actual completed render/drain required");
  return stopped;
}
async function requirePlaying(session, label) {
  const terminal = await page.evaluate(({ offset, id }) => __runtimeWindow.messages.slice(offset).find(row => row.direction === "received" && row.data.playId === id
    && ["play-error", "play-stopped"].includes(row.data.kind)), { offset: session.offset, id: session.request.playId });
  if (terminal) throw Error(`Actual play ${session.request.playId} ended while awaiting ${label}: ${JSON.stringify(terminal.data)}`);
}
async function pollPlaying(session, realm, predicate, value, label, timeout = 20000) {
  const deadline = Date.now() + timeout;
  do {
    const ready = await realm.evaluate(predicate, value);
    await requirePlaying(session, label); // Report the real receipt in Node, never inside a Puppeteer predicate.
    if (ready) return;
    await delay(20);
  } while (Date.now() < deadline);
  throw Error(`${label} timeout for actual play ${session.request.playId}`);
}
async function progress(session, ns, offset = 0) {
  await pollPlaying(session, page, ({ id, ns, offset }) => __runtimeWindow.messages.slice(offset).some(row => row.direction === "received" && row.data.playId === id && row.data.kind === "play-render-done" && row.data.songNs != null && BigInt(row.data.songNs) >= BigInt(ns)), { id: session.request.playId, ns: String(ns), offset }, `song progress ${ns}ns`);
}
async function draft(session, action, values = {}, trigger) {
  const offset = await page.evaluate(() => __runtimeWindow.messages.length);
  const selector = `#${{ rate: "runtime-rate-apply", pause: "runtime-pause", region: "runtime-region", "disable-loop": "runtime-disable-loop", "restart-original": "runtime-restart-original" }[action]}`;
  await pollPlaying(session, page, selector => { const control = document.querySelector(selector); return !control.disabled && control.getClientRects().length > 0; }, selector, `${action} control visible and enabled`);
  if (action === "rate") await field("runtime-rate", values.fraction);
  if (action === "region") { await field("runtime-start", values.start); await field("runtime-end", values.end ?? ""); await toggle("runtime-loop", values.repeat ?? false); }
  await pollPlaying(session, page, selector => !document.querySelector(selector).disabled, selector, `${action} control enabled`);
  if (trigger) await trigger(); else await page.click(selector);
  await pollPlaying(session, page, ({ offset, action }) => __runtimeWindow.messages.slice(offset).some(row => row.direction === "sent" && row.data.kind === "play-retained" && row.data.action === action), { offset, action }, `${action} actual request`);
  const request = await page.evaluate(({ offset, action }) => __runtimeWindow.messages.slice(offset).find(row => row.direction === "sent" && row.data.kind === "play-retained" && row.data.action === action).data, { offset, action });
  await pollPlaying(session, page, request => __runtimeWindow.messages.some(row => row.direction === "received" && row.data.kind === "play-reply" && row.data.playId === request.playId && row.data.rpcId === request.rpcId), request, `${action} admission reply`);
  const reply = await page.evaluate(request => __runtimeWindow.messages.find(row => row.direction === "received" && row.data.kind === "play-reply" && row.data.playId === request.playId && row.data.rpcId === request.rpcId).data, request);
  return { request, reply, offset };
}
async function committed(session, action, values = {}, trigger) {
  return settleAdmission(session, action, await draft(session, action, values, trigger));
}
async function waitBusiness(session, predicate, value, label) {
  await pollPlaying(session, business, predicate, value, label, 15000);
}
async function settleAdmission(session, action, admission) {
  assert.equal(admission.reply.error ?? null, null, JSON.stringify(admission.reply));
  const pending = admission.reply.result; assert.equal(pending.kind, "retained-pending"); assert.equal(pending.action, action);
  const before = capability(pending.capability, session.prepared.programId);
  await pollPlaying(session, page, ({ id, playId }) => __runtimeWindow.messages.some(row => row.direction === "received" && row.data.kind === "play-retained-result" && row.data.playId === playId && row.data.id === id), { id: pending.id, playId: session.request.playId }, `${action} actual committed result`);
  const result = await page.evaluate(({ playId, id }) => __runtimeWindow.messages.find(row => row.direction === "received" && row.data.kind === "play-retained-result" && row.data.playId === playId && row.data.id === id).data, { playId: session.request.playId, id: pending.id });
  const event = eventWords(result.event, session.prepared.programId), after = capability(result.capability, session.prepared.programId);
  assert.equal(event.id, BigInt(pending.id)); assert.equal(result.action, action);
  assert(["applied", "noop", "paused", "resumed"].includes(result.outcome), JSON.stringify(result));
  await waitBusiness(session, ({ serial, program }) => __runtimeBusiness.calls.some(row => row.method === "retained_event_ack" && row.args[0] === serial)
    && __runtimeBusiness.ports.some(row => row.direction === "sent" && row.data.kind === "retained-event-ack" && row.data.serial === serial && row.data.programId === program), { serial: String(event.serial), program: String(session.prepared.programId) }, "committed event ACK");
  // Select the same original diagnostic rows in their owning Worker instead of
  // copying all previous sessions while a short finite region is still live.
  const observed = await business.evaluate(({ serial, program }) => {
    const probe = __runtimeBusiness, word = (words, index) => BigInt(words[index * 2]) | BigInt(words[index * 2 + 1]) << 32n;
    const held = probe.calls.find(row => row.method === "retained_event" && row.args[0]?.length === 80 && word(row.args[0], 2) === BigInt(program) && word(row.args[0], 3) === BigInt(serial));
    return {
      held,
      service: held && probe.calls.find(row => row.method === "retained_service" && row.result === serial && row.tick > held.tick),
      localAck: held && probe.calls.find(row => row.method === "retained_event_ack" && row.args[0] === serial && row.tick > held.tick),
      audioAck: held && probe.ports.find(row => row.direction === "sent" && row.data.kind === "retained-event-ack" && row.data.serial === serial && row.tick > held.tick),
      wire: probe.ports.find(row => row.direction === "received" && row.data.retained?.event && word(row.data.retained.event, 3) === BigInt(serial) && word(row.data.retained.event, 2) === BigInt(program)),
    };
  }, { serial: String(event.serial), program: String(session.prepared.programId) });
  const { held, service, localAck, audioAck, wire } = observed;
  assert(held, "original committed event must come from actual Rust ingestion"); assert.deepEqual(held.args[0], result.event);
  assert(service && localAck && audioAck); assert(service.tick < audioAck.tick && audioAck.tick < localAck.tick, "service -> real Worklet ACK -> local latch ACK");
  assert(wire, "actual Worklet poll event required"); assert.deepEqual(wire.data.retained.event, result.event);
  assert.equal(wire.data.report?.words?.length, 58); assert.equal(wire.data.retained.metadata.length, 12);
  const history = wire.data.retained.history; assert(history.length >= 20 && (history.length - 20) % 10 === 0); assert.equal(word(history, 0), 1n); assert.equal(word(history, 1), BigInt(session.prepared.programId)); assert.equal(history.length, 20 + Number(word(history, 9)) * 10);
  assert(event.physical <= word(history, 7), "cut cannot exceed actual pulled frontier");
  return { ...admission, result, event, before, after };
}

async function invalidDraft(session, action, values, pattern) {
  const before = await business.evaluate(() => __runtimeBusiness.calls.length), request = await draft(session, action, values);
  assert.equal(request.reply.result ?? null, null); assert.match(request.reply.error ?? "", pattern);
  const rows = await page.evaluate(offset => __runtimeWindow.messages.slice(offset), request.offset);
  assert(!rows.some(row => row.data.kind === "play-retained-result" && row.data.playId === session.request.playId), "invalid draft cannot manufacture Applied");
  assert(!rows.some(row => ["play-error", "play-stopped"].includes(row.data.kind) && row.data.playId === session.request.playId));
  const calls = await business.evaluate(before => __runtimeBusiness.calls.slice(before), before);
  assert(!calls.some(row => row.method === "retained_event_ack"), "invalid draft cannot acknowledge a nonexistent event");
  await pollPlaying(session, page, () => !document.querySelector("#runtime-rate-apply").disabled, null, "control recovery after refused draft");
  return request;
}
async function acquireKeyboard(session, until = 800000000n) {
  // The literal fixture has its first lane11 tap at0.5s. One genuine Down/Up
  // near that note supplies both capture records without layering a ten-second
  // keysound on every progress poll. Keep the long sample and real voice limit.
  await progress(session, 475000000n);
  await page.keyboard.down("z");
  try { await delay(8); } finally { await page.keyboard.up("z"); }
  await progress(session, until);
}
async function saveAndSelect(name, session) {
  // Retained play deliberately leaves the custodian selector unselected. Pick
  // this actual Program's final current File, never an earlier retired prefix.
  const currentKey = `${session.prepared.programId}:current:1`;
  await page.waitForFunction(key => !document.querySelector("#captured-replay").disabled
    && [...document.querySelector("#captured-replay").options].some(option => option.value === key), {}, currentKey);
  const choice = await page.$eval("#captured-replay", (select, key) => [...select.options].find(option => option.value === key)?.textContent, currentKey);
  assert.match(choice ?? "", /complete/, "explicitly selected current File must be a genuine complete capture");
  await page.select("#captured-replay", currentKey);
  assert.equal(await page.$eval("#captured-replay", select => select.value), currentKey);
  const offset = (await messages()).length;
  await page.waitForFunction(() => !document.querySelector("#records-save").disabled); await page.click("#records-save");
  await page.waitForFunction(offset => __runtimeWindow.messages.slice(offset).some(row => row.data.kind === "records-reply" && row.data.action === "save" && row.data.error === null), {}, offset);
  const saved = (await messages()).slice(offset).find(row => row.data.kind === "records-reply" && row.data.action === "save").data;
  const id = saved.result.metadata.id; assert(Number.isSafeInteger(id) && id > 0);
  await page.waitForFunction(id => [...document.querySelector("#records").options].some(option => option.value === String(id)), {}, id);
  await page.select("#records", String(id)); await page.click("#records-use");
  await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Saved replay selected.") && !document.querySelector("#replay-play").disabled);
  const loaded = (await messages()).slice(offset).find(row => row.data.kind === "records-reply" && row.data.action === "load" && row.data.error === null)?.data;
  assert(loaded?.result?.replayFile?.actualFile, "original stored File must return through actual Worker load");
  check(name, { recordId: id, capturedTuple: currentKey }); return id;
}
function assertCut(cut, numerator, denominator, expectedRate) {
  const whole = numerator / denominator, remainder = numerator % denominator;
  assert(numerator >= 0n, "this fixture changes only after preroll");
  assert.equal(cut.whole, whole);
  assert.equal(cut.numerator * denominator, remainder * cut.denominator);
  assert.deepEqual(cut.rate, expectedRate);
}
function elapsed(words) { return BigInt.asIntN(128, word(words, 20) | word(words, 21) << 64n); }
function assertRetiredCustody(state, session, minimum, roster) {
  const sent = state.business.messages.filter(row => row.kind === "play-retired-recordings" && row.playId === session.request.playId);
  assert(sent.length >= minimum, `at least ${minimum} actual retired cohorts required`);
  const keys = new Set();
  for (const cohort of sent) {
    assert.deepEqual(cohort.players, roster); assert.equal(cohort.replays.length, roster.length);
    const accepted = state.business.messages.find(row => row.direction === "received" && row.kind === "play-retired-recordings-ack" && row.playId === cohort.playId
      && row.programId === cohort.programId && row.boundarySerial === cohort.boundarySerial && row.accepted === true);
    assert(accepted && accepted.tick > cohort.tick, "actual positive Window custody must follow the complete cohort");
    const ack = state.business.ports.find(row => row.direction === "sent" && row.data.kind === "retained-event-ack" && row.data.programId === cohort.programId && row.data.serial === cohort.boundarySerial && row.tick > cohort.tick);
    assert(ack && ack.tick > accepted.tick, "Worklet ACK must wait for real Window File custody");
    for (const row of cohort.replays) {
      assert.equal(row.replayComplete, false); assert(row.replay?.actualFile && row.replay.size > 0); assert.equal(row.replayError, null);
      const key = `${cohort.programId}/${cohort.boundarySerial}/${row.player}`; assert(!keys.has(key)); keys.add(key);
      const captured = state.business.captures.find(item => item.playId === cohort.playId && item.boundarySerial === cohort.boundarySerial && item.player === row.player);
      assert(captured?.bytes && captured.complete === false); decodeRecord(captured.bytes);
    }
  }
  return [...keys];
}
function assertFreshDraws(state, minimum) {
  const rows = state.renderer.records, registrations = rows.filter(row => row.packet?.kind === 1 && ["live", "local"].includes(row.mode));
  assert(registrations.length >= minimum, "fresh actual attempts need new chart registrations");
  for (const registration of registrations) {
    const frames = rows.filter(row => row.packet?.kind === 2 && row.packet.generation === registration.packet.generation && row.packet.content === registration.packet.content);
    for (const frame of frames) assert(registration.order < frame.order, "new registration precedes interpreting a reset progress page");
    // A short autonomous iteration can be superseded before display. The
    // explicit restart below separately waits for a genuine new draw.
  }
}

async function runJourneys() {
  await toggle("touch-input", false); await toggle("record", true); await toggle("runtime-retained", true);
  await field("live-speed", "1/2"); await field("live-start", "0"); await field("live-end", "6");
  evidence.stage = "solo-variable";
  const solo = await start(); await progress(solo, 200000000n); await acquireKeyboard(solo);
  const badRate = await invalidDraft(solo, "rate", { fraction: "0/1" }, /rate|positive|speed|fraction|numerator/i);
  const badEnd = await invalidDraft(solo, "region", { start: "0.25", end: "1", repeat: false }, /end|scrub|region|endpoint/i);
  check("invalid-drafts", { rate: badRate.reply.error, endpoint: badEnd.reply.error });
  const noop = await committed(solo, "rate", { fraction: "2/4" });
  assert.equal(noop.result.outcome, "noop"); assert.deepEqual(noop.after.rate, [1n, 2n]);
  assert.equal(noop.after.generation, noop.before.generation); assert.equal(noop.after.revision, noop.before.revision + 1n);
  const changed = await committed(solo, "rate", { fraction: "2/1" });
  assert.equal(changed.event.tag, 2n); assert.deepEqual(changed.before.rate, [1n, 2n]); assert.deepEqual(changed.after.rate, [2n, 1n]);
  assert.equal(changed.after.generation, changed.before.generation); assert.equal(changed.after.revision, changed.before.revision + 1n);
  await delay(200);
  const paused = await committed(solo, "pause"); assert.equal(paused.result.outcome, "paused");
  // The actual Pause owns this frozen song interval while diagnostic JSON and
  // screenshots are collected; artifact I/O must not consume the finite test.
  const rateObservation = await snapshot("solo-rate-applied");
  assert(rateObservation.window.pcm.some(row => Math.abs(row.frequency - 440) <= 50), "actual original half-rate pitch");
  assert(rateObservation.window.pcm.some(row => Math.abs(row.frequency - 1760) <= 60), "actual new double-rate pitch");
  check("solo-rate-continuation", { actual: changed.result, diagnosticPitch: [440, 1760] });
  const queued = await draft(solo, "rate", { fraction: "3/2" }); assert.equal(queued.reply.result?.kind, "retained-pending");
  const pendingCap = capability(queued.reply.result.capability, solo.prepared.programId); assert.deepEqual(pendingCap.rate, [2n, 1n]);
  await delay(150);
  assert(!(await messages()).some(row => row.data.kind === "play-retained-result" && row.data.playId === solo.request.playId && row.data.id === queued.reply.result.id), "paused admission cannot fabricate an applied cut");
  assert.equal(await page.$eval("#runtime-pause", element => element.disabled), false, "Resume stays available for the admitted pending rate");
  const resumed = await committed(solo, "pause"); assert.equal(resumed.result.outcome, "resumed");
  const afterPause = await settleAdmission(solo, "rate", queued); assert.deepEqual(afterPause.after.rate, [3n, 2n]);
  assert.equal(afterPause.after.generation, changed.after.generation);
  check("pause-pending-resume", { paused: paused.result, resumed: resumed.result, actualRate: afterPause.result });
  const stopped = await terminal(solo), completed = await snapshot("solo-variable-complete");
  assert(BigInt(stopped.hits) > 0n, "actual trusted key must hit a fixture note");
  const capture = completed.business.captures.find(row => row.playId === solo.request.playId && row.boundarySerial === null && row.complete === true);
  assert(capture?.bytes, "naturally completed current attempt must have its actual complete File");
  const original = Buffer.from(capture.bytes), decoded = decodeRecord(original); assert.deepEqual(decoded.initial, [1n, 2n]); assert.equal(decoded.cuts.length, 2);
  const firstE = elapsed(changed.result.event), secondE = elapsed(afterPause.result.event), firstQNumerator = firstE - 100000000n;
  assertCut(decoded.cuts[0], firstQNumerator, 2n, [2n, 1n]);
  assertCut(decoded.cuts[1], firstQNumerator + (secondE - firstE) * 4n, 2n, [3n, 2n]);
  const inputs = decoded.records.filter(row => row.input); assert(inputs.length >= 2);
  const acquired = completed.window.messages.filter(row => row.direction === "sent" && row.data.kind === "play-step" && row.data.playId === solo.request.playId).flatMap(row => row.data.events ?? []);
  for (const record of inputs) assert(acquired.some(event => String(event.sequence) === String(record.input.sequence) && String(event.hostNs) === String(record.input.original.ns)), "captured input must retain its original trusted Window acquisition");
  await writeFile(resolve(out, "variable-source.bkr"), original);
  check("completed-variable-record", { sha256: sha(original), initial: decoded.initial, cuts: decoded.cuts, inputs: inputs.length, stopped });
  const recordId = await saveAndSelect("record-save-load", solo);
  await field("replay-speed", "1/1"); evidence.stage = "independent-replay";
  const replay = await start("replay-play"); assert.notEqual(replay.request.retained, true); assert.equal(replay.request.replaySpeed, "1/1");
  const replayStopped = await terminal(replay), replayState = await snapshot("variable-record-replay");
  assert.equal(replayStopped.hits, stopped.hits); assert.equal(replayStopped.misses, stopped.misses);
  const loaded = replayState.business.loads.find(row => row.playId === replay.request.playId); assert(loaded?.bytes); assert.deepEqual(Buffer.from(loaded.bytes), original);
  assert.equal(sha(await readFile(resolve(out, "variable-source.bkr"))), sha(original));
  check("independent-replay", { recordId, originalSha256: sha(original), sourceInitial: decoded.initial, sourceCuts: decoded.cuts, selectedOutput: "1/1", stopped: replayStopped });

  evidence.stage = "regions-original-restart";
  await field("live-start", "0.5"); await field("live-end", "8"); await field("live-speed", "1/2");
  const regions = await start(); await progress(regions, 650000000n);
  await committed(regions, "rate", { fraction: "2/1" });
  const loop = await committed(regions, "region", { start: "1", end: "2", repeat: true }); assert.deepEqual(loop.after.rate, [2n, 1n]);
  const loopCaptureOffset = await business.evaluate(() => __runtimeBusiness.messages.length);
  await waitBusiness(regions, ({ offset, id }) => __runtimeBusiness.messages.slice(offset).some(row => row.kind === "play-retired-recordings" && row.playId === id), { offset: loopCaptureOffset, id: regions.request.playId }, "autonomous real loop capture");
  const autonomous = await business.evaluate(({ offset, id }) => __runtimeBusiness.messages.slice(offset).find(row => row.kind === "play-retired-recordings" && row.playId === id), { offset: loopCaptureOffset, id: regions.request.playId });
  await waitBusiness(regions, ({ serial, after }) => __runtimeBusiness.calls.some(row => row.method === "retained_event_ack" && row.args[0] === serial && row.tick > after), { serial: autonomous.boundarySerial, after: autonomous.tick }, "autonomous loop custody and ACK");
  const disabled = await committed(regions, "disable-loop"); assert.deepEqual(disabled.after.rate, [2n, 1n]);
  const scrub = await committed(regions, "region", { start: "0.25", end: "", repeat: false }); assert.deepEqual(scrub.after.rate, [2n, 1n]);
  const registrationCount = await renderer.evaluate(() => __runtimeRenderer.records.filter(row => row.packet?.kind === 1 && row.mode === "live").length);
  const restarted = await committed(regions, "restart-original", {}, async () => { await page.click("#canvas"); await page.keyboard.press("F5"); });
  assert.equal(restarted.event.tag, 3n); assert.deepEqual(restarted.after.rate, [1n, 2n]); assert(restarted.after.generation > scrub.after.generation);
  assert.equal(BigInt.asIntN(64, word(restarted.result.event, 23)), 500000000n, "F5 restores the actual original section start");
  await progress(regions, 600000000n, await page.evaluate(() => __runtimeWindow.messages.length));
  await pollPlaying(regions, renderer, count => {
        const rows = __runtimeRenderer.records, registration = rows.filter(row => row.packet?.kind === 1 && row.mode === "live")[count];
        return registration && rows.some(row => row.kind === "drawn" && row.generation === registration.packet.generation && row.content === registration.packet.content);
  }, registrationCount, "actual F5 fresh chart draw", 10000);
  await page.click("#stop"); await terminal(regions, false);
  const regionState = await snapshot("regions-f5-retired-custody"); assertFreshDraws(regionState, registrationCount + 1);
  const retired = assertRetiredCustody(regionState, regions, 4, [1]);
  check("region-loop-disable-scrub-f5", { loop: loop.result, disabled: disabled.result, scrub: scrub.result, restarted: restarted.result });
  check("retired-custody", { tuples: retired, prefixes: retired.length });

  evidence.stage = "local-shared-rate";
  if (!(await page.$eval("#local-count", element => element.closest("details").open))) await page.click("details.binding-editor:has(#local-count) > summary");
  await toggle("touch-input", true); await field("local-count", "2"); await field("live-start", "0"); await field("live-end", "4");
  await page.waitForFunction(() => !document.querySelector("#local-discover").disabled); await page.click("#local-discover");
  await page.waitForFunction(() => document.querySelector("#local-source-1")?.options.length >= 3 && document.querySelector("#local-source-2")?.options.length >= 3);
  await page.select("#local-source-1", "1"); await page.select("#local-source-2", "2");
  const local = await start(); await progress(local, 200000000n); await page.keyboard.press("z");
  const bounds = await business.evaluate(() => __runtimeBusiness.calls.findLast(row => row.name === "BrowserLocalGame" && row.method === "configure_touch_regions")?.args[2]);
  assert(Array.isArray(bounds) && bounds.length === 4, "actual local touch region required");
  await page.$eval("#canvas", element => element.scrollIntoView({ block: "center" }));
  const canvas = await page.$eval("#canvas", element => { const r = element.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; });
  const point = { x: canvas.x + canvas.width * (bounds[0] + bounds[2]) / 2 / 960, y: canvas.y + canvas.height * (bounds[1] + bounds[3]) / 2 / 720, id: 1 };
  const touch = await page.createCDPSession();
  try { await touch.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [point] }); await delay(40); await touch.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] }); }
  finally { await touch.detach(); }
  const localRate = await committed(local, "rate", { fraction: "2/1" }); assert.equal(localRate.after.generation, localRate.before.generation);
  await committed(local, "pause"); await committed(local, "pause");
  await committed(local, "region", { start: "0.25", end: "", repeat: false });
  const localRestart = await committed(local, "restart-original"); assert.deepEqual(localRestart.after.rate, [1n, 2n]);
  await progress(local, 200000000n, await page.evaluate(() => __runtimeWindow.messages.length)); await page.click("#stop"); await terminal(local, false);
  const localState = await snapshot("local-runtime-controls"); assertRetiredCustody(localState, local, 2, [1, 2]);
  const localCaptures = localState.business.captures.filter(row => row.playId === local.request.playId && row.boundarySerial !== null);
  const twoMembers = localCaptures.filter(row => row.boundarySerial === localCaptures[0]?.boundarySerial); assert.equal(twoMembers.length, 2);
  for (const row of twoMembers) { const file = decodeRecord(row.bytes); assert.deepEqual(file.initial, [1n, 2n]); assert.equal(file.cuts.length, 1); assert.deepEqual(file.cuts[0].rate, [2n, 1n]); }
  assert(localCaptures.some(row => decodeRecord(row.bytes).records.some(record => record.input?.variant === 2 && record.input.source === 2n)), "actual assigned touch provenance remains in its member capture");
  assert(localCaptures.some(row => decodeRecord(row.bytes).records.some(record => record.input?.variant === 0 && record.input.source === 1n)), "actual assigned keyboard provenance remains in its member capture");
  check("local-shared-rate", { rate: localRate.result, restart: localRestart.result, captures: localCaptures.map(row => ({ player: row.player, serial: row.boundarySerial, bytes: row.size })) });

  evidence.stage = "fixed-opponent-refusal";
  await field("local-count", "1"); await toggle("touch-input", false); await field("live-start", "0"); await field("live-end", "2"); await field("live-speed", "1/2");
  const fixed = await start(); await terminal(fixed); const fixedState = await snapshot("fixed-source-record");
  const fixedCapture = fixedState.business.captures.find(row => row.playId === fixed.request.playId && row.complete === true);
  assert(fixedCapture?.bytes); assert.equal(decodeRecord(fixedCapture.bytes).cuts.length, 0);
  await saveAndSelect("fixed-opponent-record-save-load", fixed); await page.click("#opponents-add");
  await page.waitForFunction(() => document.querySelector("#opponents-list").children.length === 1);
  const competition = await start(); await progress(competition, 200000000n);
  const refusal = await draft(competition, "rate", { fraction: "2/1" }); assert.match(refusal.reply.error ?? "", /opponent|competition|fixed|condition/i);
  assert.equal(refusal.reply.result ?? null, null);
  await progress(competition, 400000000n);
  assert.equal(await page.$eval("#opponents-list", element => element.children.length), 1, "fixed competition must never silently drop the opponent");
  await terminal(competition); const competitionState = await snapshot("fixed-opponent-rate-refusal");
  const competitionCapture = competitionState.business.captures.find(row => row.playId === competition.request.playId && row.complete === true); assert(competitionCapture?.bytes);
  assert.equal(decodeRecord(competitionCapture.bytes).cuts.length, 0);
  check("fixed-opponent-refusal", { error: refusal.reply.error, selectedOpponents: 1, originalRate: "1/2" });
  await page.click("#opponents-clear");
}

async function stopOwned(child, name) {
  if (!child) return;
  if (child.exitCode === null && child.signalCode === null) {
    const exited = new Promise(done => child.once("exit", done)); child.kill("SIGTERM");
    try { await bounded(exited, 4000, name); } catch { child.kill("SIGKILL"); await bounded(exited, 4000, name + " kill"); }
  }
  evidence.cleanup[name] = { exitCode: child.exitCode, signal: child.signalCode };
}
try {
  const spki = await startServer();
  const shm = await statfs("/dev/shm"); assert.equal(shm.type, 0x01021994); assert(shm.bavail * shm.bsize >= 24 * 1024 * 1024);
  cache = await mkdtemp("/dev/shm/beatkernel-runtime-rate-"); profile = await mkdtemp(resolve(out, "profile-"));
  xvfb = spawn("/usr/bin/Xvfb", ["-displayfd", "3", "-screen", "0", "1400x1200x24", "-nolisten", "tcp"], { stdio: ["ignore", "ignore", "ignore", "pipe"] });
  const display = await bounded(new Promise((done, reject) => {
    let value = ""; xvfb.stdio[3].on("data", bytes => { value += bytes; if (value.includes("\n")) /^\d+$/.test(value.trim()) ? done(":" + value.trim()) : reject(Error("invalid Xvfb display")); });
    xvfb.once("error", reject); xvfb.once("exit", code => reject(Error("Xvfb exited " + code)));
  }), 10000, "Xvfb display");
  const puppeteer = createRequire(import.meta.url)(resolve(root, process.env.PUPPETEER_MODULE));
  browser = await puppeteer.launch({ executablePath: process.env.CHROMIUM, env: { ...process.env, DISPLAY: display }, headless: false, userDataDir: profile, timeout: 30000,
    args: ["--no-sandbox", "--enable-unsafe-webgpu", "--enable-unsafe-swiftshader", "--use-angle=vulkan", "--use-vulkan=swiftshader", "--enable-features=Vulkan",
      "--disable-vulkan-surface", "--ignore-certificate-errors-spki-list=" + spki, "--disk-cache-dir=" + cache, "--disk-cache-size=16777216"] });
  evidence.browserCommand = browser.process().spawnargs;
  page = await browser.newPage(); page.setDefaultTimeout(30000); await page.setViewport({ width: 1200, height: 1000, hasTouch: true });
  page.on("workercreated", worker => { if (worker.url().endsWith("/worker.js")) business = worker; if (worker.url().endsWith("/renderer-worker.js")) renderer = worker; });
  await page.evaluateOnNewDocument(observeWindow); await installNetworkObservation();
  await page.goto(evidence.base + "/app/web/", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => !document.querySelector("#files").disabled); assert(await page.evaluate(() => crossOriginIsolated));
  assert(business && renderer, "both real production Workers required");
  await (await page.$("#files")).uploadFile(resolve(out, "runtime.bms"), resolve(out, "runtime.wav"));
  await page.waitForFunction(() => !document.querySelector("#prepare").disabled); await page.click("#prepare");
  await page.waitForFunction(() => !document.querySelector("#play").disabled);
  check("fresh-artifacts", { ui: expected, audio: expectedAudio, sourceFiles: Object.keys(sources).length });
  await runJourneys();
  evidence.stage = "strict-final-audit";
  const final = await snapshot("final");
  const setup = final.business.setup, byteLength = sources["audio-pkg/beatkernel_bms_runtime_bg.wasm"].byteLength;
  const fetches = setup.filter(row => row.kind === "fetch-start"); assert.equal(fetches.length, 1, "actual Worker must load the owned audio module once");
  assert.equal(setup.filter(row => row.kind === "reader-read" && row.done && row.bytes === byteLength).length, 1, "actual complete audio body EOF");
  assert.equal(setup.filter(row => row.kind === "reader-release" && row.bytes === byteLength).length, 1);
  assert.equal(setup.filter(row => row.kind === "reader-cancel").length, 0);
  assert.equal(setup.filter(row => row.kind === "compile-done" && row.byteLength === byteLength && row.actualModule).length, 1);
  const bodies = evidence.networkLifecycle.filter(row => row.kind === "server-wasm-body" && row.name === "audio-pkg/beatkernel_bms_runtime_bg.wasm");
  assert.equal(bodies.length, 1); assert.equal(bodies[0].byteLength, byteLength); assert.equal(bodies[0].sha256, expectedAudio);
  const requests = evidence.networkLifecycle.filter(row => row.kind === "worker-cdp-request" && row.url?.endsWith("/audio-pkg/beatkernel_bms_runtime_bg.wasm"));
  assert.equal(requests.length, 1);
  assert(evidence.networkLifecycle.some(row => row.kind === "worker-cdp-finished" && row.requestId === requests[0].requestId && row.sessionId === requests[0].sessionId), "actual audio response needs strict Worker Network completion");
  assert(!evidence.networkOverflow);
  assert.deepEqual(evidence.errors, []); assert.deepEqual(evidence.consoleErrors, []); assert.deepEqual(evidence.networkErrors, [], "no exclusions, including ERR_ABORTED");
  for (const [name, bytes] of Object.entries(sources)) assert.equal(sha(await readFile(resolve(root, "app/web", name))), sha(bytes), `source changed during execution: ${name}`);
  check("clean-console-network", "original strict page/Worker network and owned audio body/EOF/module identity all clean");
  evidence.status = "PASS";
} catch (error) {
  evidence.status = "FAIL"; evidence.failure = String(error.stack ?? error); process.exitCode = 1;
  if (page && !page.isClosed()) {
    try { evidence.failureWindow = await bounded(page.evaluate(() => ({ probe: __runtimeWindow, status: document.querySelector("#status")?.textContent })), 3000, "failure Window observation"); await page.screenshot({ path: resolve(out, "failure.png"), fullPage: true }); }
    catch (observed) { evidence.observationError = String(observed); }
  }
  if (business) try { evidence.failureBusiness = await bounded(businessState(), 3000, "failure business observation"); } catch (observed) { evidence.businessObservationError = String(observed); }
  if (renderer) try { evidence.failureRenderer = await bounded(renderer.evaluate(() => __runtimeRenderer), 3000, "failure renderer observation"); } catch (observed) { evidence.rendererObservationError = String(observed); }
} finally {
  const cleanup = async (name, action) => { try { await action(); } catch (error) { (evidence.cleanup.errors ??= []).push({ name, error: String(error) }); evidence.status = "FAIL"; process.exitCode = 1; } };
  if (networkSession && attachedListener) networkSession.off("Target.attachedToTarget", attachedListener);
  await cleanup("Worker setup settlement", () => bounded(Promise.all([...workerNetworkTasks]), 10000, "Worker setup settlement"));
  for (const { session, listeners } of workerNetworkSessions.values()) {
    for (const [name, callback] of listeners) session.off(name, callback);
    await cleanup("Worker network detach", () => bounded(networkSession.send("Target.detachFromTarget", { sessionId: session.id() }), 3000, "Worker detach"));
  }
  workerNetworkSessions.clear();
  if (networkSession) await cleanup("Worker autoattach disable", () => bounded(networkSession.send("Target.setAutoAttach", { autoAttach: false, waitForDebuggerOnStart: false, flatten: true }), 3000, "Worker autoattach disable"));
  if (browser) await cleanup("browser close", () => bounded(browser.close(), 6000, "browser close"));
  await cleanup("browser process", () => stopOwned(browser?.process(), "browser")); await cleanup("Xvfb", () => stopOwned(xvfb, "xvfb"));
  if (server) await cleanup("HTTPS", async () => { server.closeAllConnections(); await bounded(new Promise(done => server.close(done)), 3000, "HTTPS close"); });
  for (const owned of [profile, cache]) if (owned) await cleanup("owned directory", () => rm(owned, { recursive: true, force: true }));
  for (const name of ["key.pem", "cert.pem"]) await cleanup(name, () => rm(resolve(out, name), { force: true }));
  const passed = new Set(evidence.checks.map(row => row.id)); evidence.unmet = required.filter(id => !passed.has(id));
  if (evidence.unmet.length || evidence.networkErrors.length || evidence.errors.length || evidence.consoleErrors.length) { evidence.status = "FAIL"; process.exitCode = 1; }
  await writeFile(resolve(out, "evidence.json"), JSON.stringify(clean(evidence), null, 2));
  console.log(JSON.stringify({ status: evidence.status, checks: evidence.checks.map(row => row.id), unmet: evidence.unmet, failure: evidence.failure, evidence: resolve(out, "evidence.json") }, null, 2));
}
