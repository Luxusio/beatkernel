/**
 * Actual production Worker -> RenderWorker -> WebGPU dense regression.
 * Build fresh browser WASM first; the coordinator supplies its recorded hash.
 * From the repository root:
 * source target/toolchain/env.sh
 * EXPECTED_WASM_SHA256=<fresh-build-sha256> CHROMIUM=/usr/bin/chromium \
 * PUPPETEER_MODULE=target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core \
 * DENSE_BROWSER_OUT=target/wf/dense-playfield-browser \
 * node app/web/dense-playfield.browser.mjs
 * HTTP on owned loopback port uses COOP same-origin / COEP require-corp.
 * Owned Xvfb, Chromium profile and /dev/shm cache are removed in finally.
 * The renderer wrapper observes exact production bytes and forwards every real
 * call/event unchanged. It never injects a packet, ACK, readiness or timestamp.
 * Raw wire/GPU bytes, correlated ACKs and screenshots are development evidence;
 * screenshots do not establish complete primitive membership. CPU fixtures
 * independently establish geometry semantics. This is functional SwiftShader
 * evidence, not physical GPU, audio, latency, throughput, or universal OOM proof.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { readFile, writeFile, mkdir, rm, mkdtemp, statfs } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve, dirname, relative, extname, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const out = resolve(root, process.env.DENSE_BROWSER_OUT ?? "target/wf/dense-playfield-browser");
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const expected = process.env.EXPECTED_WASM_SHA256;
assert.match(expected ?? "", /^[a-f0-9]{64}$/, "EXPECTED_WASM_SHA256 must identify the coordinator's fresh WASM");
const wasm = await readFile(resolve(root, "app/web/pkg/beatkernel_bms_runtime_bg.wasm"));
assert.equal(hash(wasm), expected, "current production WASM differs from the recorded build");
const actualRenderer = await readFile(resolve(root, "app/web/renderer-worker.js"));
const actualWorker = await readFile(resolve(root, "app/web/worker.js"));
await mkdir(out, { recursive: true });
// A BPM60 measure spans4s, only half fits the production2s lookahead. BPM120
// makes this entire measure simultaneously visible without changing settings.
const count = 2304;
const fixture = `#TITLE Dense actual GPU\n#BPM 120\n#LNTYPE 1\n#WAV01 tone.wav\n#00051:${"0101".repeat(count)}\n#00012:${"01".repeat(count)}\n#000D3:${"1E".repeat(count)}\n`;
const fixturePath = resolve(out, "dense.bms");
await writeFile(fixturePath, fixture);
// Genuine PCM sample selected through the public file input. Undefined WAV01
// is a parser error; this valid tone gives the real preparation path its asset.
const samplePath = resolve(out, "tone.wav"), frames = 2400;
const sample = Buffer.alloc(44 + frames * 2);
sample.write("RIFF"); sample.writeUInt32LE(sample.length - 8, 4); sample.write("WAVEfmt ", 8);
sample.writeUInt32LE(16, 16); sample.writeUInt16LE(1, 20); sample.writeUInt16LE(1, 22);
sample.writeUInt32LE(48000, 24); sample.writeUInt32LE(96000, 28); sample.writeUInt16LE(2, 32); sample.writeUInt16LE(16, 34);
sample.write("data", 36); sample.writeUInt32LE(frames * 2, 40);
for (let i = 0; i < frames; i++) sample.writeInt16LE(Math.round(Math.sin(i * Math.PI * 880 / 48000) * 1200), 44 + i * 2);
await writeFile(samplePath, sample);
const evidence = { status: "RUNNING", wasmSha256: expected,
  workerSha256: hash(actualWorker), rendererSha256: hash(actualRenderer), fixtureSha256: hash(fixture), sampleSha256: hash(sample),
  fixture: { bpm: 120, holds: count, taps: count, mines: count, fullWindowPrimitives: 11520 },
  steps: [], errors: [], consoleErrors: [], httpErrors: [], faviconErrors: [], cleanup: {}, ceilings: [
    "Observed real production registration, preview packets, GPU uploads/draw calls and successful correlated ACKs on local Chromium SwiftShader.",
    "CPU fixture geometry oracles remain independent; screenshot inspection alone cannot prove all primitives are present.",
    "No physical GPU, real audio/input judging, performance benchmark, allocator OOM recovery or multiplayer-page acceptance claim.",
  ] };
const bounded = (promise, ms, name) => {
  let timer;
  return Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(Error(`${name} timeout`)), ms); })])
    .finally(() => clearTimeout(timer));
};

// Executed before importing the unmodified production renderer. Bounded raw
// observations avoid an unbounded diagnostic owner during a failing run.
function rendererObservation() {
  const data = globalThis.__denseGPU = { records: [], overflow: false, adapter: null };
  const add = record => { if (data.records.length < 512) data.records.push(record); else data.overflow = true; };
  const clean = value => JSON.parse(JSON.stringify(value, (_, v) => typeof v === "bigint" ? String(v) : v));
  const summary = message => {
    const value = {};
    for (const key of ["kind", "operationId", "generation", "content", "sequence", "packetKind", "geometryVersion", "mode", "page", "width", "height", "message"])
      if (message?.[key] !== undefined) value[key] = clean(message[key]);
    if (message?.packet instanceof Uint8Array) {
      if (message.packet.byteLength > 1024 * 1024) throw Error("dense observation packet ceiling exceeded");
      value.bytes = Array.from(message.packet);
    }
    return value;
  };
  const originalPost = MessagePort.prototype.postMessage;
  MessagePort.prototype.postMessage = function(message, ...args) {
    add({ direction: "sent", ...summary(message) });
    return originalPost.call(this, message, ...args);
  };
  const originalStart = MessagePort.prototype.start, observed = new WeakSet();
  MessagePort.prototype.start = function(...args) {
    if (!observed.has(this)) {
      observed.add(this);
      this.addEventListener("message", ({ data: message }) => add({ direction: "received", ...summary(message) }));
    }
    return originalStart.apply(this, args);
  };
  const requestAdapter = GPU.prototype.requestAdapter;
  GPU.prototype.requestAdapter = async function(...args) {
    const adapter = await requestAdapter.apply(this, args);
    if (adapter) {
      const info = adapter.info;
      data.adapter = { vendor: info?.vendor, architecture: info?.architecture, device: info?.device,
        description: info?.description, isFallbackAdapter: adapter.isFallbackAdapter,
        maxBufferSize: adapter.limits.maxBufferSize };
    }
    return adapter;
  };
  const buffers = new WeakMap(), bindings = new WeakMap();
  let nextBuffer = 0;
  const createBuffer = GPUDevice.prototype.createBuffer;
  GPUDevice.prototype.createBuffer = function(descriptor) {
    const buffer = createBuffer.call(this, descriptor);
    const record = { id: ++nextBuffer, label: descriptor.label, size: descriptor.size };
    buffers.set(buffer, record);
    if (record.label === "BeatKernel retained note buffer") add({ kind: "note-buffer", ...record });
    return buffer;
  };
  const writeBuffer = GPUQueue.prototype.writeBuffer;
  GPUQueue.prototype.writeBuffer = function(buffer, offset, source, dataOffset = 0, size) {
    const retained = buffers.get(buffer);
    if (retained?.label === "BeatKernel retained note buffer") {
      const unit = source.BYTES_PER_ELEMENT ?? 1;
      const bytes = ArrayBuffer.isView(source)
        ? new Uint8Array(source.buffer, source.byteOffset, source.byteLength) : new Uint8Array(source);
      const start = dataOffset * unit, length = size === undefined ? bytes.length - start : size * unit;
      if (length > 1024 * 1024) throw Error("dense observation upload ceiling exceeded");
      add({ kind: "note-upload", buffer: retained.id, offset, bytes: Array.from(bytes.subarray(start, start + length)) });
    }
    return writeBuffer.apply(this, arguments);
  };
  const setVertexBuffer = GPURenderPassEncoder.prototype.setVertexBuffer;
  GPURenderPassEncoder.prototype.setVertexBuffer = function(slot, buffer, ...args) {
    if (slot === 0) bindings.set(this, buffers.get(buffer));
    return setVertexBuffer.call(this, slot, buffer, ...args);
  };
  const draw = GPURenderPassEncoder.prototype.draw;
  GPURenderPassEncoder.prototype.draw = function(vertices, instances = 1, firstVertex = 0, firstInstance = 0) {
    const buffer = bindings.get(this);
    if (buffer?.label === "BeatKernel retained note buffer") add({ kind: "note-draw", buffer: buffer.id, vertices, instances, firstVertex, firstInstance });
    return draw.apply(this, arguments);
  };
}

function windowObservation() {
  const data = globalThis.__denseWindow = { messages: [], input: [], overflow: false };
  const add = (list, value) => { if (list.length < 512) list.push(value); else data.overflow = true; };
  const Native = Worker;
  globalThis.Worker = class extends Native {
    constructor(url, options) {
      super(url, options);
      this.addEventListener("message", ({ data: message }) => {
        if (["selected", "render-geometry", "position", "fatal", "render-error", "selection-error", "seek-error", "catalog"].includes(message?.kind))
          add(data.messages, JSON.parse(JSON.stringify(message, (_, value) => typeof value === "bigint" ? String(value) : value)));
      });
    }
  };
  for (const kind of ["keydown", "keyup"]) document.addEventListener(kind, event =>
    add(data.input, { kind, code: event.code, trusted: event.isTrusted }), true);
}

// Independent reader of the existing BKRV v1 registration/preview wire; no
// parser or geometry result is supplied back to the application.
function packet(bytes) {
  const b = Buffer.from(bytes); assert.equal(b.subarray(0, 4).toString(), "BKRV");
  assert.equal(b.readUInt16LE(4), 1); assert.equal(Number(b.readBigUInt64LE(32)), b.length - 40);
  const header = { kind: b.readUInt16LE(6), generation: String(b.readBigUInt64LE(8)),
    content: String(b.readBigUInt64LE(16)), sequence: String(b.readBigUInt64LE(24)) };
  if (header.kind === 3) return { ...header, song: b.readBigInt64LE(40), lookahead: b.readBigInt64LE(48) };
  if (header.kind !== 1) return header;
  let at = 40;
  const u32 = () => { const v = b.readUInt32LE(at); at += 4; return v; };
  const i64 = () => { const v = b.readBigInt64LE(at); at += 8; return v; };
  for (let i = 0; i < 2; i++) { const n = Number(b.readBigUInt64LE(at)); at += 8 + n; }
  const lanes = u32(); assert(lanes > 0 && lanes <= 18); at += lanes;
  i64(); at++; if (b[at++]) at += 2;
  const notes = [], mines = [], noteCount = u32(); assert.equal(noteCount, 4608);
  for (let i = 0; i < noteCount; i++) { at += 8; const lane = u32(), start = i64(), end = b[at++] ? i64() : null; notes.push({ lane, start, end }); }
  const mineCount = u32(); assert.equal(mineCount, count);
  for (let i = 0; i < mineCount; i++) { at += 8; const lane = u32(), time = i64(); at += 2; mines.push({ lane, time }); }
  return { ...header, notes, mines };
}
// Wire packets and GPU uploads both retain raw bytes. Only incoming production
// packet messages have BKRV framing; never send GPU instance bytes to this reader.
const isWirePacket = record => record.direction === "received" && record.kind === "packet" && Array.isArray(record.bytes);
function membership(chart, song, lookahead) {
  // The existing Scene playfield keeps an inclusive 150ms trailing window for
  // recently passed heads/mines. Compute membership linearly from actual wire
  // endpoints instead of assuming the visible lower bound equals song time.
  const lower = song - 150_000_000n, upper = song + lookahead;
  const notes = chart.notes.filter(n => n.start <= upper && (n.end ?? n.start) >= lower);
  const mines = chart.mines.filter(n => n.time >= lower && n.time <= upper);
  const holds = notes.filter(n => n.end !== null).length;
  return { holds, taps: notes.length - holds, mines: mines.length, primitives: notes.length + holds * 2 + mines.length };
}
function uploadCounts(bytes) {
  const b = Buffer.from(bytes); assert.equal(b.length % 32, 0);
  const result = { body: 0, tail: 0, head: 0, mine: 0 };
  for (let at = 0; at < b.length; at += 32) {
    const kind = b.readFloatLE(at + 16);
    if (kind === 0) result.body++;
    else if (kind === 1) result.tail++;
    else { assert.equal(kind, 2); if (Math.abs(b.readFloatLE(at + 20) - 0xef / 255) < 0.00001) result.mine++; else result.head++; }
  }
  return result;
}

const probeSource = `(${rendererObservation.toString()})();`;
const server = createServer(async (request, response) => {
  try {
    const pathname = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    let file = resolve(root, "." + pathname), from = relative(root, file);
    if (from === ".." || from.startsWith(".." + sep)) throw Error("outside root");
    if (pathname.endsWith("/")) file = resolve(file, "index.html");
    const bytes = pathname === "/app/web/renderer-worker.js"
      ? Buffer.from("import './dense-observer.js';\nimport './dense-actual-renderer.js';\n")
      : pathname === "/app/web/dense-observer.js" ? Buffer.from(probeSource)
        : pathname === "/app/web/dense-actual-renderer.js" ? actualRenderer : await readFile(file);
    response.writeHead(200, { "Content-Type": ({ ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".css": "text/css" })[extname(file)] ?? "application/octet-stream",
      "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cache-Control": "no-store" });
    response.end(bytes);
  } catch { response.writeHead(404); response.end(); }
});
let browser, xvfb, profile, cache, renderer, page;
async function stop(child, name) {
  if (!child) return;
  if (child.exitCode === null && child.signalCode === null) {
    const exited = new Promise(r => child.once("exit", r)); child.kill("SIGTERM");
    try { await bounded(exited, 4000, name); }
    catch { child.kill("SIGKILL"); await bounded(exited, 4000, name + " kill"); }
  }
  evidence.cleanup[name] = { exitCode: child.exitCode, signal: child.signalCode };
}
try {
  await bounded(new Promise((r, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", r); }), 5000, "server listen");
  const base = `http://127.0.0.1:${server.address().port}`; evidence.base = base;
  const shm = await statfs("/dev/shm"); assert.equal(shm.type, 0x01021994); assert(shm.bavail * shm.bsize >= 24 * 1024 * 1024);
  cache = await mkdtemp("/dev/shm/beatkernel-dense-"); profile = await mkdtemp(resolve(out, "profile-"));
  xvfb = spawn("/usr/bin/Xvfb", ["-displayfd", "3", "-screen", "0", "1400x1200x24", "-nolisten", "tcp"], { stdio: ["ignore", "ignore", "ignore", "pipe"] });
  const display = await bounded(new Promise((r, reject) => { let text = ""; xvfb.stdio[3].on("data", b => { text += b; if (text.includes("\n")) { if (/^\d+$/.test(text.trim())) r(":" + text.trim()); else reject(Error("Invalid owned Xvfb display number")); } }); xvfb.once("error", reject); xvfb.once("exit", c => reject(Error("Xvfb exit " + c))); }), 10000, "Xvfb display");
  const require = createRequire(import.meta.url);
  const puppeteer = require(resolve(root, process.env.PUPPETEER_MODULE ?? "target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core"));
  browser = await puppeteer.launch({ env: { ...process.env, DISPLAY: display }, executablePath: process.env.CHROMIUM ?? "/usr/bin/chromium", headless: false, userDataDir: profile, timeout: 30000,
    args: ["--no-sandbox", "--enable-unsafe-webgpu", "--enable-unsafe-swiftshader", "--use-angle=vulkan", "--use-vulkan=swiftshader", "--enable-features=Vulkan", "--disable-vulkan-surface", "--disk-cache-dir=" + cache, "--disk-cache-size=16777216"] });
  evidence.browserCommand = browser.process().spawnargs;
  page = await browser.newPage(); page.setDefaultTimeout(30000);
  await page.setViewport({ width: 1200, height: 1000 }); await page.evaluateOnNewDocument(windowObservation);
  page.on("workercreated", worker => { if (worker.url().endsWith("/renderer-worker.js")) renderer = worker; });
  page.on("pageerror", error => evidence.errors.push(String(error)));
  const favicon = url => { try { return new URL(url).pathname === "/favicon.ico"; } catch { return false; } };
  page.on("console", message => {
    if (message.type() === "error") (favicon(message.location().url) ? evidence.faviconErrors : evidence.consoleErrors).push(message.text());
  });
  page.on("response", response => {
    if (response.status() >= 400) (favicon(response.url()) ? evidence.faviconErrors : evidence.httpErrors).push({ url: response.url(), status: response.status() });
  });
  await page.goto(base + "/app/web/", { waitUntil: "domcontentloaded", timeout: 30000 });
  await page.waitForFunction(() => !document.querySelector("#files").disabled);
  assert(await page.evaluate(() => crossOriginIsolated));
  await (await page.$("#files")).uploadFile(fixturePath, samplePath);
  await page.waitForFunction(() => !document.querySelector("#prepare").disabled && document.querySelector("#chart").options.length > 0);
  await page.select("#chart", "dense.bms"); await page.focus("#prepare"); await page.keyboard.press("Enter");
  await page.waitForFunction(() => __denseWindow.messages.some(x => ["fatal", "render-error", "selection-error"].includes(x.kind))
    || __denseWindow.messages.some(x => x.kind === "selected") && __denseWindow.messages.some(x => x.kind === "render-geometry" && x.mode === "preview" && x.width > 0 && x.height > 0));
  const preparationError = await page.evaluate(() => __denseWindow.messages.find(x => ["fatal", "render-error", "selection-error"].includes(x.kind)));
  assert(!preparationError, `actual preparation failed: ${JSON.stringify(preparationError)}`);
  assert(renderer, "real renderer Worker was not created");
  const observed = () => bounded(renderer.evaluate(() => __denseGPU), 5000, "renderer observation");
  let lastRecord = 0, chart;
  async function snapshot(name, song, warmed = false) {
    // Production drawn sequence is emitted only after successful draw_visual.
    const deadline = Date.now() + 30000;
    let gpu, preview, records;
    while (Date.now() < deadline) {
      gpu = await observed(); records = gpu.records.slice(lastRecord);
      const renderError = gpu.records.find(r => r.kind === "render-error");
      assert(!renderError, `${name}: actual renderer failed: ${JSON.stringify(renderError)}`);
      preview = records.filter(isWirePacket).map(r => ({ record: r, parsed: packet(r.bytes) })).findLast(r => r.parsed.kind === 3 && r.parsed.song === song);
      if (preview && records.some(r => r.kind === "drawn" && r.generation === preview.parsed.generation && r.content === preview.parsed.content && r.sequence === preview.parsed.sequence)) break;
      await new Promise(r => setTimeout(r, 20));
    }
    assert(preview, `${name}: actual preview packet missing`);
    assert(records.some(r => r.kind === "drawn" && r.generation === preview.parsed.generation && r.content === preview.parsed.content && r.sequence === preview.parsed.sequence), `${name}: actual successful presentation ACK missing`);
    chart ??= gpu.records.filter(isWirePacket).map(r => packet(r.bytes)).find(r => r.kind === 1 && r.generation === preview.parsed.generation && r.content === preview.parsed.content);
    assert(chart, "genuine cold registration missing");
    const wanted = membership(chart, song, preview.parsed.lookahead);
    const draws = records.filter(r => r.kind === "note-draw"); assert(draws.some(r => r.vertices === 6 && r.instances === wanted.primitives && r.firstInstance === 0), `${name}: complete real GPU draw missing`);
    const uploads = records.filter(r => r.kind === "note-upload");
    if (warmed) { assert.equal(uploads.length, 0, "unchanged warmed geometry must reuse uploaded storage"); assert.equal(records.filter(r => r.kind === "note-buffer").length, 0); }
    else {
      const upload = uploads.findLast(r => r.bytes.length === wanted.primitives * 32); assert(upload, `${name}: complete GPU upload missing`);
      assert.deepEqual(uploadCounts(upload.bytes), { body: wanted.holds, tail: wanted.holds, head: wanted.holds + wanted.taps, mine: wanted.mines });
    }
    assert.equal(gpu.overflow, false); const window = await page.evaluate(() => __denseWindow); assert.equal(window.overflow, false);
    const geometry = window.messages.findLast(r => r.kind === "render-geometry" && r.mode === "preview");
    assert(geometry && gpu.records.some(r => r.kind === "geometry-ack" && r.generation === geometry.generation && r.content === geometry.content && r.geometryVersion === geometry.geometryVersion && r.width === geometry.width && r.height === geometry.height), "public geometry must correlate with genuine positive renderer ACK");
    const screenshot = resolve(out, name + ".png"); await page.screenshot({ path: screenshot, fullPage: true });
    evidence.steps.push({ name, song: String(song), lookahead: String(preview.parsed.lookahead), wanted, geometry, drawnSequence: preview.parsed.sequence, screenshot, recordStart: lastRecord, recordEnd: gpu.records.length });
    lastRecord = gpu.records.length; return { gpu, wanted };
  }
  const cold = await snapshot("01-cold-dense", 0n);
  assert.deepEqual(cold.wanted, { holds: count, taps: count, mines: count, primitives: 11520 });
  assert.equal(chart.notes.filter(n => n.end !== null).length, count);
  assert.equal(chart.notes.filter(n => n.end === null).length, count);
  assert(chart.notes.every(n => n.start >= 0n && (n.end ?? n.start) < 2_000_000_000n));
  assert(chart.mines.every(n => n.time >= 0n && n.time < 2_000_000_000n));
  assert(cold.gpu.records.some(r => r.kind === "note-buffer" && r.size >= 11520 * 32));
  assert(cold.gpu.records.filter(r => r.kind === "note-buffer").length > 4, "dense draw must grow one of four initial retained buffers");
  async function seek(seconds, name, warmed = false) {
    await page.focus("#position"); await page.keyboard.down("Control"); await page.keyboard.press("KeyA"); await page.keyboard.up("Control");
    await page.keyboard.type(seconds); await page.keyboard.press("Enter");
    await page.waitForFunction(ns => __denseWindow.messages.some(r => r.kind === "position" && r.ns === ns), {}, String(BigInt(Math.round(Number(seconds) * 1e9))));
    return snapshot(name, BigInt(Math.round(Number(seconds) * 1e9)), warmed);
  }
  await seek("0", "02-warmed-dense", true);
  await seek("0.25", "03-seek-dense");
  await page.evaluate(() => { document.querySelector("#viewport").style.width = "640px"; document.querySelector("#viewport").style.height = "480px"; });
  await page.waitForFunction(() => __denseWindow.messages.some(r => r.kind === "render-geometry" && r.mode === "preview" && r.width === 640 && r.height === 480));
  // Resize produces a real geometry ACK; subsequent public seek requests a
  // preview-state sequence and a warmed draw at the resized field geometry.
  await seek("0", "04-resized-dense");
  await seek("0", "05-resized-warmed", true);
  evidence.window = await page.evaluate(() => __denseWindow); evidence.gpu = await observed();
  assert(evidence.window.input.some(r => r.kind === "keydown" && r.code === "Enter" && r.trusted));
  assert(!evidence.window.messages.some(r => ["fatal", "render-error", "selection-error", "seek-error"].includes(r.kind)));
  assert(!evidence.gpu.records.some(r => r.kind === "render-error"));
  assert.equal(evidence.errors.length, 0); assert.equal(evidence.consoleErrors.length, 0); assert.equal(evidence.httpErrors.length, 0);
  evidence.status = "PASS";
} catch (error) {
  evidence.status = "FAIL"; evidence.failure = String(error.stack ?? error); process.exitCode = 1;
  if (renderer) try { evidence.gpu = await bounded(renderer.evaluate(() => __denseGPU), 3000, "failure renderer"); } catch (e) { evidence.rendererObservationError = String(e); }
  if (page) try { evidence.window = await bounded(page.evaluate(() => __denseWindow), 3000, "failure window"); await bounded(page.screenshot({ path: resolve(out, "failure.png"), fullPage: true }), 5000, "failure screenshot"); } catch (e) { evidence.windowObservationError = String(e); }
} finally {
  if (browser) await bounded(browser.close(), 6000, "browser close").catch(e => { evidence.cleanup.browserCloseError = String(e); });
  for (const [child, name] of [[browser?.process(), "browser"], [xvfb, "xvfb"]]) try { await stop(child, name); } catch (e) { evidence.cleanup[name + "Error"] = String(e); process.exitCode = 1; evidence.status = "FAIL"; }
  server.closeAllConnections(); await bounded(new Promise(r => server.close(r)), 3000, "server close").catch(e => { evidence.cleanup.serverCloseError = String(e); process.exitCode = 1; evidence.status = "FAIL"; });
  for (const directory of [profile, cache]) if (directory) await rm(directory, { recursive: true, force: true });
  evidence.cleanup.server = server.listening ? "still listening" : "closed";
  evidence.cleanup.ownedDirectories = "removed";
  await writeFile(resolve(out, "evidence.json"), JSON.stringify(evidence, (_, value) => typeof value === "bigint" ? String(value) : value, 2));
  console.log("FINAL", evidence.status, evidence.failure ?? "", out);
}
