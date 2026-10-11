/** Actual production browser replay-speed acceptance probe (not a QA receipt).
 * Generate main/audio WASM first, then:
 * PUPPETEER_MODULE=target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core \
 * CHROMIUM=/usr/bin/chromium node app/web/replay-speed.browser.mjs
 * REPLAY_SPEED_OUT / REPLAY_SPEED_PORT select ignored evidence and an owned port.
 * No responses, clocks, output reports, gameplay records or completion states
 * are manufactured. The probes only retain original messages and tap real PCM.
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
const out = resolve(process.env.REPLAY_SPEED_OUT ?? resolve(root, "target/wf/realtime-replay-speed-browser/development-1"));
const port = Number(process.env.REPLAY_SPEED_PORT ?? 8147);
assert.ok(Number.isInteger(port) && port >= 1024 && port <= 65535);
assert.ok(relative(root, out).startsWith(`target${sep}wf${sep}`), "Evidence must remain in ignored target/wf");
const evidence = { kind: "development-browser-replay-speed", checks: [], cases: [], console: [], network: [],
  required: ["genuine-capture", "1/2", "1/1", "2/1", "3/2", "stop-retry", "immutable-record", "clean-console-network"],
  ceiling: ["Chromium software GPU/audio execution is not physical-device or acoustic timing proof."] };
let server, browser, page;
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
const delay = ms => new Promise(done => setTimeout(done, ms));
const json = value => JSON.stringify(value, (_, x) => typeof x === "bigint" ? String(x) : x, 2);
const check = (id, detail) => evidence.checks.push({ id, passed: true, detail });

function fixtureWav() {
  const hz = 48000, frames = hz * 3, bytes = Buffer.alloc(44 + frames * 2);
  bytes.write("RIFF"); bytes.writeUInt32LE(bytes.length - 8, 4); bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(hz, 24); bytes.writeUInt32LE(hz * 2, 28); bytes.writeUInt16LE(2, 32); bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36); bytes.writeUInt32LE(frames * 2, 40);
  for (let frame = 0; frame < frames; frame++) bytes.writeInt16LE(Math.round(Math.sin(frame * Math.PI * 1760 / hz) * 4096), 44 + frame * 2);
  return bytes;
}

async function serve() {
  await mkdir(out, { recursive: true });
  await writeFile(resolve(out, "speed.wav"), fixtureWav());
  await writeFile(resolve(out, "speed.bms"), "#TITLE Replay speed acceptance\n#BPM 120\n#TOTAL 320\n#WAV01 speed.wav\n#00001:01\n#00011:00010001\n");
  const cert = resolve(out, "cert.pem"), key = resolve(out, "key.pem");
  const created = spawnSync("openssl", ["req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-keyout", key, "-out", cert, "-days", "1", "-subj", "/CN=localhost", "-addext", "subjectAltName=IP:127.0.0.1,DNS:localhost"], { encoding: "utf8", timeout: 10000 });
  assert.equal(created.status, 0, created.stderr);
  const certificate = new X509Certificate(await readFile(cert));
  const spki = createHash("sha256").update(certificate.publicKey.export({ type: "spki", format: "der" })).digest("base64");
  server = createServer({ cert: await readFile(cert), key: await readFile(key) }, async (request, response) => {
    try {
      const url = new URL(request.url, "https://localhost");
      if (url.pathname === "/favicon.ico") { response.writeHead(204); response.end(); return; }
      let file = resolve(root, `.${decodeURIComponent(url.pathname)}`);
      const path = relative(root, file);
      if (path === ".." || path.startsWith(`..${sep}`)) throw Error("Outside repository");
      if ((await stat(file)).isDirectory()) file = resolve(file, "index.html");
      response.writeHead(200, { "Content-Type": ({ ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".html": "text/html", ".css": "text/css" })[extname(file)] ?? "application/octet-stream",
        "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cache-Control": "no-store" });
      response.end(await readFile(file));
    } catch (error) { evidence.network.push({ serverError: String(error), url: request.url }); response.writeHead(404); response.end(); }
  });
  await new Promise((done, reject) => { server.once("error", reject); server.listen(port, "127.0.0.1", done); });
  return spki;
}

// Observations run before main.js. Both subclasses retain the original native
// constructors, messages, transfer lists, contexts and ports.
function observeWindow() {
  const probe = globalThis.__replaySpeedProbe = { messages: [], pcm: [], contexts: [], statuses: [] };
  const safe = value => JSON.parse(JSON.stringify(value, (_, x) => typeof x === "bigint" ? String(x)
    : x instanceof File ? { file: true, name: x.name, size: x.size }
      : x instanceof Blob ? { blob: true, size: x.size }
        : x instanceof MessagePort ? { port: true } : ArrayBuffer.isView(x) ? Array.from(x) : x));
  const ActualWorker = Worker;
  globalThis.Worker = class extends ActualWorker {
    constructor(url, options) {
      super(url, options); this.probeURL = String(url);
      this.addEventListener("message", event => {
        if (probe.messages.length >= 20000) throw Error("Window message observation ceiling");
        probe.messages.push({ direction: "received", url: this.probeURL, at: performance.now(), data: safe(event.data) });
      });
    }
    postMessage(value, transfer) {
      if (probe.messages.length >= 20000) throw Error("Window message observation ceiling");
      probe.messages.push({ direction: "sent", url: this.probeURL, at: performance.now(), data: safe(value) });
      return super.postMessage(value, transfer);
    }
  };
  const ActualNode = AudioWorkletNode;
  globalThis.AudioWorkletNode = class extends ActualNode {
    constructor(context, name, options) {
      super(context, name, options);
      const index = probe.contexts.length;
      probe.contexts.push({ name, sampleRate: context.sampleRate });
      // Passive real-output tap. It neither feeds the game nor changes the
      // original destination. Real process() output alone supplies these PCM.
      const analyser = context.createAnalyser(); analyser.fftSize = 2048; this.connect(analyser);
      const samples = new Float32Array(analyser.fftSize);
      const acquire = () => {
        if (context.state === "closed") return;
        analyser.getFloatTimeDomainData(samples);
        let peak = 0, crossings = 0;
        for (let i = 0; i < samples.length; i++) {
          peak = Math.max(peak, Math.abs(samples[i]));
          if (i && samples[i - 1] <= 0 && samples[i] > 0) crossings++;
        }
        if (peak > 0.005 && probe.pcm.length < 4096) probe.pcm.push({ index, at: performance.now(), peak,
          frequency: crossings * context.sampleRate / samples.length, samples: Array.from(samples.slice(0, 32)) });
        requestAnimationFrame(acquire);
      };
      requestAnimationFrame(acquire);
    }
  };
  addEventListener("DOMContentLoaded", () => {
    new MutationObserver(() => probe.statuses.push({ at: performance.now(), status: document.querySelector("#status").textContent,
      position: document.querySelector("#position").value })).observe(document.querySelector("#status"), { childList: true, subtree: true, characterData: true });
  });
}

// Install before playback setup, not before WASM initialization. Reading a
// capture occurs only in the Worker; original File handles/messages stay intact.
async function observeWorker() {
  if (globalThis.__replaySpeedWorker) return;
  const probe = globalThis.__replaySpeedWorker = { reports: [], observations: [], renders: [], captures: [], pending: 0, errors: [] };
  // Reuse the Worker's already-cached generated binding module. This does not
  // initialize WASM, construct an owner or replace any gameplay/output value.
  const { BrowserReplay } = await import("./pkg/beatkernel_bms_runtime.js");
  const observe = BrowserReplay.prototype.observe_output;
  const owners = new WeakMap(); let nextOwner = 0;
  BrowserReplay.prototype.observe_output = function(words, presented) {
    const result = observe.call(this, words, presented);
    if (!owners.has(this)) owners.set(this, ++nextOwner);
    if (probe.observations.length >= 20000) throw Error("Replay observation ceiling");
    probe.observations.push({ owner: owners.get(this), point: presented === undefined || presented === null ? null : String(presented),
      songNs: String(this.song_ns), completed: result });
    return result;
  };
  const post = globalThis.postMessage;
  globalThis.postMessage = function(value, ...args) {
    if (value?.kind === "play-render-done" && probe.observations.length) {
      probe.renders.push({ playId: value.playId, renderId: value.renderId, observation: probe.observations.length - 1 });
    }
    const result = post.call(this, value, ...args);
    if (value?.replay instanceof File) {
      const capture = { kind: value.kind, playId: value.playId, complete: value.replayComplete, bytes: null };
      probe.captures.push(capture); probe.pending++;
      value.replay.arrayBuffer().then(bytes => {
        if (bytes.byteLength > 1048576) throw Error("Capture observation ceiling");
        capture.bytes = Array.from(new Uint8Array(bytes));
      }).catch(error => probe.errors.push(String(error))).finally(() => probe.pending--);
    }
    return result;
  };
  const descriptor = Object.getOwnPropertyDescriptor(MessagePort.prototype, "onmessage");
  if (!descriptor?.set || !descriptor.get) throw Error("Native MessagePort observation descriptor missing");
  Object.defineProperty(MessagePort.prototype, "onmessage", { ...descriptor,
    set(listener) {
      return descriptor.set.call(this, listener === null ? null : function(event) {
        const words = event.data?.report?.words;
        if (words instanceof Uint32Array) {
          if (probe.reports.length >= 20000) throw Error("Output report observation ceiling");
          probe.reports.push({ at: performance.now(), words: Array.from(words) });
        }
        return listener.call(this, event);
      });
    },
  });
}

// The visible playfield is owned by the renderer Worker, not the disabled
// Window preview-position draft. Retain only real packet headers/song scalars
// and actual renderer draw ACKs; no packet or acknowledgement is injected.
function observeRenderer() {
  if (globalThis.__replaySpeedRenderer) return;
  const probe = globalThis.__replaySpeedRenderer = { packets: [], receipts: [], errors: [] };
  const started = new WeakSet(), start = MessagePort.prototype.start, post = MessagePort.prototype.postMessage;
  MessagePort.prototype.start = function() {
    if (!started.has(this)) {
      started.add(this);
      this.addEventListener("message", event => {
        const message = event.data, bytes = message?.packet;
        if (!(bytes instanceof Uint8Array)) return;
        try {
          if (probe.packets.length >= 20000 || bytes.byteLength < 40) throw Error("Renderer observation extent ceiling");
          const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
          const kind = view.getUint16(6, true);
          if (kind !== 1 && kind !== 2) return;
          const packet = { kind, mode: message.mode, generation: String(view.getBigUint64(8, true)),
            content: String(view.getBigUint64(16, true)), sequence: String(view.getBigUint64(24, true)), songNs: null };
          // render_wire.rs write_frame: 40-byte header, page:u32,
          // lookahead:i64, room_disabled:u8, members:u32, player:u32, song:i64.
          if (kind === 2) {
            if (bytes.byteLength < 69 || view.getUint32(53, true) !== 1) throw Error("Expected actual solo visual frame");
            packet.songNs = String(view.getBigInt64(61, true));
          }
          probe.packets.push(packet);
        } catch (error) { probe.errors.push(String(error)); }
      });
    }
    return start.call(this);
  };
  MessagePort.prototype.postMessage = function(message, transfer) {
    if (["state-ack", "drawn"].includes(message?.kind)) {
      if (probe.receipts.length >= 20000) throw Error("Renderer receipt observation ceiling");
      probe.receipts.push({ kind: message.kind, mode: message.mode, generation: String(message.generation),
        content: String(message.content), sequence: String(message.sequence) });
    }
    return post.call(this, message, transfer);
  };
}

const status = () => page.$eval("#status", x => x.textContent);
async function setField(id, value) {
  await page.$eval(`#${id}`, (x, value) => { x.value = value; x.dispatchEvent(new Event("input", { bubbles: true })); }, value);
}
async function waitTerminal(playId) {
  await page.waitForFunction(id => __replaySpeedProbe.messages.some(x => x.direction === "received"
    && x.data.playId === id && ["play-stopped", "play-error"].includes(x.data.kind)), { timeout: 20000 }, playId);
  await page.waitForFunction(() => document.querySelector("#stop").disabled, { timeout: 20000 });
}
async function currentStart() {
  return page.evaluate(() => __replaySpeedProbe.messages.findLast(x => x.direction === "sent" && x.data.kind === "play-start")?.data);
}
async function snapshot(name, worker, renderer) {
  const window = await page.evaluate(() => ({ ...__replaySpeedProbe, status: document.querySelector("#status").textContent,
    position: document.querySelector("#position").value, speedDisabled: document.querySelector("#replay-speed").disabled }));
  const actualWorker = await worker.evaluate(() => __replaySpeedWorker);
  assert.deepEqual(actualWorker.errors, []);
  const actualRenderer = renderer ? await renderer.evaluate(() => __replaySpeedRenderer) : null;
  if (actualRenderer) assert.deepEqual(actualRenderer.errors, []);
  await page.screenshot({ path: resolve(out, `${name}.png`), fullPage: true });
  const value = { name, window, worker: actualWorker, renderer: actualRenderer };
  evidence.cases.push(value); return value;
}
function word(words, index) { return BigInt(words[index * 2]) | BigInt(words[index * 2 + 1]) << 32n; }

async function run() {
  const spki = await serve();
  const require = createRequire(import.meta.url);
  const puppeteer = process.env.PUPPETEER_MODULE ? require(resolve(process.env.PUPPETEER_MODULE)) : (await import("puppeteer-core")).default;
  evidence.sources = Object.fromEntries(await Promise.all(["main.js", "index.html", "worker.js", "renderer-worker.js", "audio-worklet.js", "play-model.mjs", "pkg/beatkernel_bms_runtime.js", "pkg/beatkernel_bms_runtime_bg.wasm", "audio-pkg/beatkernel_bms_runtime.js", "audio-pkg/beatkernel_bms_runtime_bg.wasm"].map(async file => [file, sha(await readFile(resolve(root, "app/web", file)))])));
  browser = await puppeteer.launch({ executablePath: process.env.CHROMIUM ?? "/usr/bin/chromium", headless: true,
    args: ["--no-sandbox", "--disable-dev-shm-usage", "--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-features=Vulkan", "--use-vulkan=swiftshader", "--ignore-certificate-errors", `--ignore-certificate-errors-spki-list=${spki}`, "--autoplay-policy=no-user-gesture-required"] });
  evidence.ownedBrowserPid = browser.process().pid;
  console.log(`Owned Chromium ${evidence.ownedBrowserPid}; evidence ${out}`);
  page = await browser.newPage();
  const workers = new Map(), installations = [];
  page.on("workercreated", worker => {
    workers.set(worker.url(), worker);
    if (worker.url().endsWith("/worker.js")) installations.push(worker.evaluate(observeWorker));
    if (worker.url().endsWith("/renderer-worker.js")) installations.push(worker.evaluate(observeRenderer));
  });
  page.on("pageerror", error => evidence.console.push({ type: "pageerror", text: String(error) }));
  page.on("console", message => evidence.console.push({ type: message.type(), text: message.text() }));
  page.on("requestfailed", request => evidence.network.push({ stage: evidence.stage, at: Date.now(), url: request.url(), failure: request.failure() }));
  page.on("response", response => { if (response.status() >= 400) evidence.network.push({ url: response.url(), status: response.status() }); });
  await page.setViewport({ width: 1200, height: 1000 }); await page.evaluateOnNewDocument(observeWindow);
  evidence.stage = "initialization";
  await page.goto(`https://127.0.0.1:${port}/app/web/`, { waitUntil: "networkidle0", timeout: 60000 });
  await page.waitForFunction(() => !document.querySelector("#files").disabled, { timeout: 60000 });
  await Promise.all(installations);
  const worker = [...workers.values()].find(x => x.url().endsWith("/worker.js")); assert.ok(worker);
  const renderer = [...workers.values()].find(x => x.url().endsWith("/renderer-worker.js")); assert.ok(renderer);
  assert.equal(await page.evaluate(() => crossOriginIsolated), true);
  assert.equal(await page.$eval("#replay-speed", x => x.value), "1/1");
  assert.equal(await page.$eval("#replay-speed", x => x.maxLength), 40);
  assert.equal(await page.$eval('label[for="replay-speed"]', x => x.textContent.includes("NUM/DEN")), true);
  await (await page.$("#files")).uploadFile(resolve(out, "speed.bms"), resolve(out, "speed.wav"));
  await page.waitForFunction(() => !document.querySelector("#prepare").disabled);
  await page.click("#prepare"); await page.waitForFunction(() => !document.querySelector("#play").disabled, { timeout: 60000 });
  // CPU preparation and GPU presentation are independent owners. Wait for the
  // actual selected preview's draw receipt, not merely enabled audio controls.
  await page.waitForFunction(() => {
    const messages = __replaySpeedProbe.messages;
    const selected = messages.findLast(x => x.direction === "received" && x.data.kind === "selected")?.data;
    const canvas = document.querySelector("#canvas"), extent = canvas.getBoundingClientRect();
    return selected && messages.some(x => x.direction === "received" && x.data.kind === "drawn"
      && x.data.selectedId === selected.id) && !canvas.hidden && extent.width > 0 && extent.height > 0
      && document.querySelector("#status").textContent.startsWith("Chart prepared");
  }, { timeout: 30000 });
  assert.match(await status(), /^Chart prepared/);
  evidence.stage = "genuine-capture";
  await setField("live-end", "2"); await page.click("#record"); await page.click("#play");
  await page.waitForFunction(() => __replaySpeedProbe.messages.some(x => x.direction === "sent" && x.data.kind === "play-start"), { timeout: 30000 });
  const live = await currentStart(); await waitTerminal(live.playId);
  const capture = await snapshot("genuine-capture", worker, renderer);
  const liveTerminal = capture.window.messages.find(x => x.direction === "received" && x.data.kind === "play-stopped" && x.data.playId === live.playId)?.data;
  assert.ok(liveTerminal?.replayComplete, json(liveTerminal));
  assert.equal(liveTerminal.songNs, "2000000000");
  await worker.evaluate(async () => {
    const deadline = performance.now() + 5000;
    while (__replaySpeedWorker.pending) {
      if (performance.now() >= deadline) throw Error("Capture acquisition observation deadline");
      await new Promise(done => setTimeout(done, 10));
    }
  });
  const captured = await worker.evaluate(() => __replaySpeedWorker.captures.find(x => x.complete));
  assert.ok(captured?.bytes?.length > 0); assert.ok(Number(liveTerminal.misses) > 0, "Genuine nonempty Miss capture");
  const original = Buffer.from(captured.bytes), originalHash = sha(original), replayFile = resolve(out, "genuine-record.bkr");
  await writeFile(replayFile, original); check("genuine-capture", { bytes: original.length, sha256: originalHash, misses: liveTerminal.misses });
  await (await page.$("#replay-file")).uploadFile(replayFile);

  for (const [fraction, numerator, denominator, expectedHz] of [["1/2", 1n, 2n, 440], ["1/1", 1n, 1n, 880], ["2/1", 2n, 1n, 1760], ["3/2", 3n, 2n, 1320]]) {
    evidence.stage = fraction;
    await setField("replay-speed", fraction);
    const contextIndex = await page.evaluate(() => __replaySpeedProbe.contexts.length);
    const reportOffset = await worker.evaluate(() => __replaySpeedWorker.reports.length);
    const previousPlayId = (await currentStart()).playId;
    await page.click("#replay-play");
    await page.waitForFunction(previous => __replaySpeedProbe.messages.some(x => x.direction === "sent" && x.data.kind === "play-start"
      && x.data.mode === "replay" && BigInt(x.data.playId) > BigInt(previous)), { timeout: 30000 }, previousPlayId);
    const start = await currentStart(); assert.equal(start.replaySpeed, fraction);
    assert.equal(await page.$eval("#replay-speed", x => x.disabled), true);
    await page.waitForFunction(id => __replaySpeedProbe.messages.some(x => x.direction === "received"
      && x.data.playId === id && x.data.kind === "play-render-done" && x.data.songNs !== null
      && BigInt(x.data.songNs) >= 500000000n && BigInt(x.data.songNs) < 1500000000n), { timeout: 15000 }, start.playId);
    await (await page.$("#canvas")).screenshot({ path: resolve(out, `speed-${fraction.replace("/", "-")}-playing-canvas.png`) });
    await waitTerminal(start.playId);
    const observed = await snapshot(`speed-${fraction.replace("/", "-")}`, worker, renderer);
    const messages = observed.window.messages.filter(x => x.data.playId === start.playId);
    assert.ok(!messages.some(x => x.data.kind === "play-error"), json(messages.filter(x => x.data.kind === "play-error")));
    const metadata = messages.find(x => x.direction === "received" && x.data.kind === "play-reply" && x.data.result?.kind === "prepared")?.data.result;
    assert.ok(metadata); assert.equal(metadata.replaySpeed, fraction);
    assert.equal(metadata.replaySpeedNumerator, String(numerator)); assert.equal(metadata.replaySpeedDenominator, String(denominator));
    assert.equal(metadata.startNs, "0"); assert.equal(metadata.endNs, "2000000000");
    const rate = BigInt(start.rate);
    // Independently declared rational integer oracle, not the production model.
    const durationNs = 100000000n + 2000000000n * denominator / numerator;
    const expectedFrame = (durationNs * rate + 999999999n) / 1000000000n;
    assert.equal(metadata.endFrame, String(expectedFrame));
    const terminal = messages.find(x => x.direction === "received" && x.data.kind === "play-stopped")?.data;
    assert.ok(terminal); assert.equal(terminal.songNs, "2000000000");
    assert.equal(terminal.misses, liveTerminal.misses); assert.equal(terminal.hits, liveTerminal.hits);
    const completed = messages.find(x => x.data.kind === "play-render-done" && x.data.completed);
    assert.ok(completed, "Original output owner genuinely completed");
    const activation = messages.find(x => x.direction === "sent" && x.data.kind === "play-activate")?.data; assert.ok(activation);
    const startFrame = BigInt(activation.startFrame);
    const reports = observed.worker.reports.slice(reportOffset).map(x => x.words);
    assert.ok(reports.some(words => word(words, 0) === 1n && word(words, 6) === 1n
      && word(words, 7) === expectedFrame && word(words, 24) >= startFrame + expectedFrame
      && word(words, 13) === word(words, 14) && word(words, 13) > 0n), "Actual consumed/applied Worklet fence report required");
    const progress = messages.filter(x => x.data.kind === "play-render-done" && x.data.songNs !== null
      && BigInt(x.data.songNs) > 250000000n && BigInt(x.data.songNs) < 1500000000n);
    assert.ok(progress.length >= 2, "At least two genuinely presented intermediate song positions");
    let matched = 0, retained = 0;
    for (const response of progress) {
      const request = messages.find(x => x.direction === "sent" && x.data.kind === "play-render" && x.data.renderId === response.data.renderId)?.data;
      assert.ok(request, "Every response must match its original Window render request");
      const render = observed.worker.renders.find(x => x.playId === start.playId && x.renderId === response.data.renderId);
      assert.ok(render, "Actual binding observation must correlate to the published render response");
      const actual = observed.worker.observations[render.observation];
      assert.equal(actual.songNs, response.data.songNs, "Published progress retains the genuine binding's original-song position");
      const accepted = actual.point !== null ? actual
        : observed.worker.observations.slice(0, render.observation).findLast(x => x.owner === actual.owner && x.point !== null);
      assert.ok(accepted, "Retained progress needs an earlier accepted point on the same actual replay owner");
      if (actual.point !== null) {
        assert.ok(request.timestamp?.contextTime, "Accepted point requires an actual output timestamp");
        const seconds = request.timestamp.contextTime, whole = Math.floor(seconds);
        const contextNs = BigInt(whole) * 1000000000n + BigInt(Math.floor((seconds - whole) * 1000000000));
        const startNs = (startFrame * 1000000000n + rate - 1n) / rate;
        assert.equal(BigInt(actual.point), contextNs - startNs, "Accepted binding point is the exact original presentation association");
        matched++;
      } else retained++;
      // A repeated/regressing/future association is refused by the original
      // Worker policy; its null point retains the previous genuine position.
      // Keep the same strict integer oracle for both new and retained progress.
      const expectedSong = (BigInt(accepted.point) - 100000000n) * numerator / denominator;
      const difference = BigInt(response.data.songNs) - expectedSong;
      assert.ok(difference >= -2n && difference <= 2n, `Presented original song mismatch: ${difference} ns`);
    }
    assert.ok(matched >= 2, "Progress must match genuine reported presentation, not wall time");
    const pcm = observed.window.pcm.filter(x => x.index === contextIndex);
    assert.ok(pcm.length >= 2, "Actual production Worklet emits nonzero PCM");
    assert.ok(pcm.some(x => Math.abs(x.frequency - expectedHz) <= 50), `Expected actual pitch ${expectedHz} Hz: ${json(pcm.map(x => x.frequency))}`);
    assert.ok(observed.window.statuses.some(x => x.status.startsWith("Playing") && x.status.includes(fraction)),
      "Window status exposes selected normalized speed without per-frame DOM progress");
    const cold = observed.renderer.packets.findLast(x => x.kind === 1 && x.mode === "replay");
    assert.ok(cold, "Actual replay visual registration required");
    const visualFrames = observed.renderer.packets.filter(x => x.kind === 2 && x.generation === cold.generation
      && x.content === cold.content && x.songNs !== null && BigInt(x.songNs) > 250000000n && BigInt(x.songNs) < 1500000000n);
    const drawn = visualFrames.filter(frame => observed.renderer.receipts.some(receipt => receipt.kind === "drawn"
      && receipt.mode === "replay" && receipt.generation === frame.generation && receipt.content === frame.content
      && receipt.sequence === frame.sequence));
    assert.ok(new Set(drawn.map(x => x.songNs)).size >= 2, "Visible canvas must genuinely draw multiple advancing original-song frames");
    for (const frame of drawn) assert.ok(progress.some(response => response.data.songNs === frame.songNs),
      `Rendered song position ${frame.songNs} must come from genuine replay progress`);
    assert.equal(observed.window.speedDisabled, false);
    check(fraction, { playId: start.playId, expectedFrame: String(expectedFrame), actualReportCount: reports.length,
      originalSongProgress: matched, retainedOriginalProgress: retained, drawnOriginalSongFrames: drawn.length,
      pcmAcquisitions: pcm.length, expectedPitchHz: expectedHz });
  }

  // Genuine user stop followed by another production session; no injected stop
  // failure or replacement endpoint. Node host cases separately cover failures.
  evidence.stage = "stop-retry";
  await setField("replay-speed", "1/2"); await page.click("#replay-play");
  await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Playing"), { timeout: 30000 });
  const stopped = await currentStart(); await delay(450); await page.click("#stop"); await waitTerminal(stopped.playId);
  await setField("replay-speed", "2/1"); await page.click("#replay-play");
  await page.waitForFunction(id => __replaySpeedProbe.messages.some(x => x.direction === "sent" && x.data.kind === "play-start"
    && BigInt(x.data.playId) > BigInt(id)), { timeout: 30000 }, stopped.playId);
  const retry = await currentStart(); await waitTerminal(retry.playId);
  const final = await snapshot("stop-retry", worker, renderer);
  assert.ok(!final.window.messages.some(x => x.data.playId === retry.playId && x.data.kind === "play-error"));
  assert.ok(final.window.messages.some(x => x.data.playId === retry.playId && x.data.kind === "play-render-done" && x.data.completed));
  const stopRequest = final.window.messages.find(x => x.direction === "sent" && x.data.playId === stopped.playId && x.data.kind === "play-stop");
  assert.equal(stopRequest?.data.completed, false); check("stop-retry", { stopped: stopped.playId, retry: retry.playId });
  const retained = await worker.evaluate(() => __replaySpeedWorker.captures.find(x => x.complete)?.bytes);
  assert.deepEqual(Buffer.from(retained), original); assert.equal(sha(await readFile(replayFile)), originalHash);
  check("immutable-record", { bytes: original.length, sha256: originalHash });
  await delay(100);
  assert.deepEqual(evidence.console.filter(x => ["error", "pageerror"].includes(x.type)), []);
  assert.deepEqual(evidence.network, []); check("clean-console-network", "No page/console errors, failed requests or HTTP errors");
}

try { await run(); }
catch (error) { evidence.failure = { message: String(error), stack: error.stack }; console.error(error); process.exitCode = 1; }
finally {
  evidence.stage = "cleanup";
  if (page && !page.isClosed()) {
    if (evidence.failure) {
      evidence.failureWindow = await page.evaluate(() => {
        const canvas = document.querySelector("#canvas"), rect = canvas?.getBoundingClientRect();
        return { probe: globalThis.__replaySpeedProbe ?? null, status: document.querySelector("#status")?.textContent,
          stylesheets: document.styleSheets.length,
          canvas: canvas ? { hidden: canvas.hidden, width: canvas.width, height: canvas.height,
            cssWidth: rect.width, cssHeight: rect.height } : null };
      }).catch(error => ({ observationError: String(error) }));
      evidence.failureWorkers = [];
      for (const worker of page.workers()) {
        evidence.failureWorkers.push({ url: worker.url(), observation: await worker.evaluate(() => globalThis.__replaySpeedWorker ?? globalThis.__replaySpeedRenderer ?? null)
          .catch(error => ({ observationError: String(error) })) });
      }
      await page.screenshot({ path: resolve(out, "failure.png"), fullPage: true }).catch(error => { evidence.failureScreenshotError = String(error); });
    }
    await page.close().catch(error => { evidence.cleanupError = String(error); process.exitCode = 1; });
  }
  if (browser) await browser.close().catch(error => { evidence.cleanupError = String(error); process.exitCode = 1; });
  if (server) { server.closeAllConnections(); await new Promise(done => server.close(done)); }
  const passed = new Set(evidence.checks.filter(x => x.passed).map(x => x.id));
  evidence.unmet = evidence.required.filter(id => !passed.has(id));
  if (evidence.unmet.length) process.exitCode = 1;
  await mkdir(out, { recursive: true }); await writeFile(resolve(out, "evidence.json"), json(evidence));
  console.log(json({ checks: evidence.checks, unmet: evidence.unmet, evidence: resolve(out, "evidence.json") }));
}
