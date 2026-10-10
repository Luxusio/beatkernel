// Actual production browser loading with a diagnostic File acquisition adapter.
// No engine response, readiness, score or timestamp is fabricated.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { readFile, writeFile, mkdir, rm, mkdtemp, statfs } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve, relative, extname, sep } from "node:path";
import { spawn } from "node:child_process";

const root = process.cwd(), out = resolve(process.env.LOADING_BROWSER_OUT ?? "target/wf/player-code-smell-cleanup/actual-loading-browser");
const expected = process.env.EXPECTED_WASM_SHA256;
assert.match(expected ?? "", /^[a-f0-9]{64}$/, "EXPECTED_WASM_SHA256 must identify the coordinator's current WASM");
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const wasmSha = hash(await readFile("app/web/pkg/beatkernel_bms_runtime_bg.wasm"));
assert.equal(wasmSha, expected);
await mkdir(out, { recursive: true });
const actualWorker = await readFile("app/web/worker.js");
const evidence = { status: "RUNNING", wasmSha, actualWorkerSha256: hash(actualWorker), reads: [], steps: [], errors: [], consoleErrors: [], cleanup: {}, ceilings: [
  "Functional software Chromium/SwiftShader evidence; timings are observations, not a throughput, latency or hardware benchmark.",
  "Pending source reads use the actual UI. While preparation is pending the host disables replacement/selection; late selection fencing is covered by deterministic Worker tests, not an invented GUI control.",
  "Browser page closure is the actual pending-owner retirement exercised here; no physical device/audio timing proof is claimed.",
] };
const delay = ms => new Promise(r => setTimeout(r, ms));
const bounded = (promise, ms, name) => { let timer; return Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(Error(name + " timeout")), ms); })]).finally(() => clearTimeout(timer)); };
let releaseDelayed = false;
const held = new Set();

// Served wrapper imports the exact current production worker module bytes under
// an alias in the same directory. Relative production imports stay unchanged.
const fileProbe = `const original=File.prototype.arrayBuffer;const diagnosticOwner=crypto.randomUUID();
async function log(file,phase,bytes){await fetch('/loading-read?'+new URLSearchParams({owner:diagnosticOwner,name:file.name,size:String(file.size),phase,bytes:String(bytes??0)}),{cache:'no-store'});}
File.prototype.arrayBuffer=async function(){await log(this,'begin');
if(this.name==='unrelated-hung.bin')return new Promise(()=>{});
if(this.name==='broken.wav'){await log(this,'fixture-error');throw new Error('Fixture selected media acquisition failure');}
if(this.name==='delayed.wav')await fetch('/loading-wait',{cache:'no-store'});
const bytes=await original.call(this);await log(this,'end',bytes.byteLength);return bytes;};`;
// Static dependencies keep init messages queued until the production listener
// is installed. Probe evaluation precedes the exact production module.
const wrapper = `import '/app/web/loading-file-probe.js';\nimport '/app/web/loading-actual-worker.js';`;

function wav() {
  const count = 9600, b = Buffer.alloc(44 + count * 2);
  b.write("RIFF"); b.writeUInt32LE(b.length - 8, 4); b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(1, 22); b.writeUInt32LE(48000, 24); b.writeUInt32LE(96000, 28);
  b.writeUInt16LE(2, 32); b.writeUInt16LE(16, 34); b.write("data", 36); b.writeUInt32LE(count * 2, 40);
  for (let i = 0; i < count; i++) b.writeInt16LE(Math.round(Math.sin(i * Math.PI * 880 / 48000) * 1200), 44 + i * 2);
  return b;
}
for (const [name, title, sample] of [["short.bms", "Loading genuine short", "shared.wav"], ["delayed.bms", "Loading genuine delayed", "delayed.wav"], ["broken.bms", "Loading selected failure", "broken.wav"], ["replacement.bms", "Loading replacement", "shared.wav"]]) {
  await writeFile(resolve(out, name), `#TITLE ${title}\n#BPM 120\n#WAV01 ${sample}\n#00011:00010000\n`);
}
for (const name of ["shared.wav", "delayed.wav", "broken.wav"]) await writeFile(resolve(out, name), wav());
await writeFile(resolve(out, "unrelated-hung.bin"), Buffer.alloc(8 * 1024 * 1024));

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, "http://localhost");
    if (url.pathname === "/loading-read") {
      evidence.reads.push({ at: Date.now(), ...Object.fromEntries(url.searchParams) }); res.writeHead(204); res.end(); return;
    }
    if (url.pathname === "/loading-wait") {
      if (releaseDelayed) { res.writeHead(204); res.end(); }
      else { held.add(res); res.on("close", () => held.delete(res)); }
      return;
    }
    let file = resolve(root, "." + decodeURIComponent(url.pathname)), from = relative(root, file);
    if (from === ".." || from.startsWith(".." + sep)) throw Error("outside root");
    if (url.pathname.endsWith("/")) file = resolve(file, "index.html");
    const bytes = url.pathname === "/app/web/worker.js" ? Buffer.from(wrapper)
      : url.pathname === "/app/web/loading-file-probe.js" ? Buffer.from(fileProbe)
      : url.pathname === "/app/web/loading-actual-worker.js" ? actualWorker : await readFile(file);
    res.writeHead(200, { "Content-Type": ({ ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".css": "text/css" })[extname(file)] ?? "application/octet-stream",
      "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cache-Control": "no-store" });
    res.end(bytes);
  } catch { res.writeHead(404); res.end(); }
});
function windowObservation() {
  const data = window.__loading = { messages: [], input: [], workers: [] };
  const Native = Worker;
  window.Worker = class extends Native {
    constructor(url, options) {
      super(url, options); data.workers.push(this); this.probeURL = String(url);
      this.addEventListener("message", ({ data: message }) => {
        if (["ready", "catalog", "import-error", "selected", "selection-error", "fatal", "menu-state", "render-geometry", "disposed"].includes(message?.kind)) {
          data.messages.push({ at: performance.now(), ...JSON.parse(JSON.stringify(message, (_, value) => typeof value === "bigint" ? String(value) : ArrayBuffer.isView(value) ? { byteLength: value.byteLength } : value)) });
        }
      });
    }
    postMessage(message, transfer) {
      if (message?.kind === "select") data.messages.push({ at: performance.now(), direction: "sent", kind: message.kind, id: message.id, libraryId: message.libraryId, path: message.path });
      return super.postMessage(message, transfer);
    }
  };
  for (const kind of ["keydown", "keyup"]) document.addEventListener(kind, e => data.input.push({ kind, code: e.code, trusted: e.isTrusted, at: performance.now() }), true);
}
let browser, xvfb, profile, cache;
async function stop(child, name) {
  if (!child) return;
  if (child.exitCode === null && child.signalCode === null) {
    child.kill("SIGTERM");
    try { await bounded(new Promise(r => child.once("exit", r)), 4000, name); }
    catch { child.kill("SIGKILL"); await bounded(new Promise(r => child.once("exit", r)), 4000, name + " kill"); }
  }
  evidence.cleanup[name] = { exitCode: child.exitCode, signal: child.signalCode };
}
try {
  await bounded(new Promise(r => server.listen(0, "127.0.0.1", r)), 5000, "server listen");
  const base = "http://127.0.0.1:" + server.address().port; evidence.base = base;
  const shm = await statfs("/dev/shm"); assert.equal(shm.type, 0x01021994); assert(shm.bavail * shm.bsize >= 24 * 1024 * 1024);
  cache = await mkdtemp("/dev/shm/beatkernel-loading-"); profile = await mkdtemp(resolve(out, "profile-"));
  xvfb = spawn("/usr/bin/Xvfb", ["-displayfd", "3", "-screen", "0", "1400x1200x24", "-nolisten", "tcp"], { stdio: ["ignore", "ignore", "pipe", "pipe"] });
  const display = await bounded(new Promise((r, reject) => { let s = ""; xvfb.stdio[3].on("data", b => { s += b; if (s.includes("\n")) r(":" + s.trim()); }); xvfb.once("error", reject); xvfb.once("exit", c => reject(Error("Xvfb exit " + c))); }), 10000, "Xvfb display");
  const require = createRequire(import.meta.url), puppeteer = require(resolve(process.env.PUPPETEER_MODULE ?? "target/wf/qa-browser-parallel-player-01a119a6-1/deps/node_modules/puppeteer-core"));
  browser = await puppeteer.launch({ env: { ...process.env, DISPLAY: display }, executablePath: "/usr/bin/chromium", headless: false, userDataDir: profile,
    args: ["--no-sandbox", "--enable-unsafe-webgpu", "--enable-unsafe-swiftshader", "--use-angle=vulkan", "--use-vulkan=swiftshader", "--enable-features=Vulkan", "--disable-vulkan-surface", "--disk-cache-dir=" + cache, "--disk-cache-size=16777216"] });
  evidence.browserCommand = browser.process().spawnargs;
  const page = await browser.newPage(); await page.setViewport({ width: 1200, height: 1000 }); page.setDefaultTimeout(30000);
  page.on("pageerror", e => evidence.errors.push(String(e))); await page.evaluateOnNewDocument(windowObservation);
  page.on("console", message => { if (message.type() === "error") evidence.consoleErrors.push(message.text()); });
  await page.goto(base + "/app/web/", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => !document.querySelector("#files").disabled);
  const reads = (name, phase = "begin") => evidence.reads.filter(x => x.name === name && x.phase === phase);
  const snapshot = async name => {
    const file = resolve(out, name + ".png"); await page.screenshot({ path: file, fullPage: true });
    const state = await page.evaluate(() => ({ status: document.querySelector("#status").textContent, error: document.querySelector("#status").dataset.error === "true", title: document.querySelector("#title").textContent,
      chart: document.querySelector("#chart").value, prepareDisabled: document.querySelector("#prepare").disabled, messages: __loading.messages, input: __loading.input }));
    evidence.steps.push({ name, screenshot: file, ...state }); return state;
  };
  const upload = async names => {
    const priorCatalog = await page.evaluate(() => __loading.messages.filter(x => x.kind === "catalog").length);
    await (await page.$("#files")).uploadFile(...names.map(name => resolve(out, name)));
    await page.waitForFunction(n => __loading.messages.filter(x => x.kind === "catalog").length > n
      && !document.querySelector("#prepare").disabled && document.querySelector("#chart").options.length > 0, {}, priorCatalog);
  };
  const begin = async chart => {
    const prior = await page.evaluate(() => __loading.messages.length);
    await page.select("#chart", chart); await page.click("#prepare");
    await page.waitForFunction((n, chart) => __loading.messages.slice(n).some(x => x.direction === "sent" && x.kind === "select" && x.path === chart), {}, prior, chart);
  };
  const prepared = async title => {
    await page.waitForFunction(title => document.querySelector("#status").textContent.startsWith("Chart prepared") && document.querySelector("#title").textContent.includes(title), {}, title);
    await page.waitForFunction(() => {
      const selected = __loading.messages.findLast(x => x.kind === "selected");
      const canvas = document.querySelector("#canvas"), box = canvas.getBoundingClientRect();
      return selected && __loading.messages.some(g => g.kind === "render-geometry" && g.mode === "preview" && g.selectedId === selected.id && g.width > 0 && g.height > 0)
        && !canvas.hidden && box.width > 0 && box.height > 0;
    });
  };
  const initialFiles = ["short.bms", "delayed.bms", "broken.bms", "shared.wav", "delayed.wav", "broken.wav", "unrelated-hung.bin"];
  const importStarted = Date.now(); await upload(initialFiles);
  assert.equal(evidence.reads.length, 0, "catalog must arrive before any selected File.arrayBuffer acquisition");
  const catalog = await snapshot("01-metadata-catalog-no-reads"); assert.match(catalog.status, /3 chart\(s\) loaded/);
  evidence.catalogElapsedMs = Date.now() - importStarted;
  await begin("short.bms"); await prepared("Loading genuine short"); await snapshot("02-selected-real-chart-wav-ready");
  assert.equal(reads("short.bms").length, 1); assert.equal(reads("shared.wav").length, 1);
  assert.equal(reads("shared.wav", "end").length, 1);
  for (const name of ["delayed.bms", "broken.bms", "delayed.wav", "broken.wav", "unrelated-hung.bin"]) assert.equal(reads(name).length, 0);
  await begin("delayed.bms");
  await bounded((async () => { while (!reads("delayed.wav").length || held.size === 0) await delay(20); })(), 10000, "actual pending selected media read");
  assert.equal(held.size, 1); const pending = await snapshot("04-genuine-selected-media-pending");
  assert.match(pending.status, /^Preparing chart/); assert.equal(pending.prepareDisabled, true);
  const inputBefore = await page.evaluate(() => __loading.input.length); await page.keyboard.press("KeyA");
  assert(await page.evaluate(n => __loading.input.slice(n).some(e => e.kind === "keydown" && e.code === "KeyA" && e.trusted), inputBefore));
  releaseDelayed = true; for (const response of held) { response.writeHead(204); response.end(); }
  await prepared("Loading genuine delayed"); await snapshot("05-pending-read-genuine-recovery");
  await begin("broken.bms");
  await page.waitForFunction(() => document.querySelector("#status").dataset.error === "true" && !document.querySelector("#prepare").disabled);
  const failure = await snapshot("06-selected-acquisition-error-retains-preview");
  assert.match(failure.status, /Fixture selected media acquisition failure/); assert(failure.title.includes("Loading genuine delayed"));
  assert.equal(reads("broken.wav", "fixture-error").length, 1);
  await begin("short.bms"); await prepared("Loading genuine short"); await snapshot("07-cached-selected-recovery");
  assert.equal(reads("short.bms").length, 1); assert.equal(reads("shared.wav").length, 1, "same accepted library reuses acquired bytes");
  const beforeReplacementReads = evidence.reads.length;
  await upload(["replacement.bms", "delayed.bms", "shared.wav", "delayed.wav", "unrelated-hung.bin"]);
  assert.equal(evidence.reads.length, beforeReplacementReads, "replacement catalog is also metadata-only");
  await begin("replacement.bms"); await prepared("Loading replacement"); await snapshot("08-accepted-new-library-replacement");
  assert.equal(reads("replacement.bms").length, 1); assert.equal(reads("shared.wav").length, 2, "new accepted library owns its own acquisition");
  // The production menu intentionally takes presentation priority over preview.
  // Exercise its actual keyboard bridge after the final preview readiness check.
  await page.click("#menu-open"); await page.waitForFunction(() => __loading.messages.some(x => x.kind === "menu-state" && x.route === 1));
  await page.waitForFunction(() => {
    const s = __loading.messages.findLast(x => x.kind === "menu-state");
    return s?.route === 1 && __loading.messages.some(g => g.kind === "render-geometry" && g.mode === "menu" && g.menuGeneration === s.menuGeneration && g.screen === s.screen && g.revision === s.revision);
  });
  const beforeKey = await page.evaluate(() => __loading.messages.findLast(x => x.kind === "menu-state").revision);
  await page.keyboard.press("ArrowDown");
  await page.waitForFunction(revision => __loading.messages.some(x => x.kind === "menu-state" && x.route === 1 && BigInt(x.revision) > BigInt(revision)), {}, beforeKey);
  await snapshot("08b-actual-keyboard-catalog-navigation");
  // The UI prevents replacing a preparation mid-read; close the real pending
  // owner, then release its original acquisition and inspect cleanup separately.
  releaseDelayed = false; await begin("delayed.bms");
  await bounded((async () => { while (reads("delayed.wav").length < 2 || held.size === 0) await delay(20); })(), 10000, "replacement pending read");
  await snapshot("09-pending-owner-before-page-close");
  evidence.finalWindow = await page.evaluate(() => ({ messages: __loading.messages, input: __loading.input }));
  assert.equal(reads("unrelated-hung.bin").length, 0, "unrelated never-resolving file was never touched");
  assert(!evidence.finalWindow.messages.some(x => x.kind === "fatal"));
  await page.close(); releaseDelayed = true;
  for (const response of held) { if (!response.destroyed) { response.writeHead(204); response.end(); } }
  evidence.steps.push({ name: "actual-pending-page-retired", unrelatedReads: 0, expectedAcquisitionErrors: reads("broken.wav", "fixture-error").length });
  assert.equal(evidence.errors.length, 0); evidence.status = "PASS";
} catch (error) {
  evidence.status = "FAIL"; evidence.failure = String(error.stack ?? error); process.exitCode = 1;
  if (browser) for (const page of await browser.pages()) {
    try { evidence.failureWindow = await bounded(page.evaluate(() => ({ messages: globalThis.__loading?.messages, input: globalThis.__loading?.input, status: document.querySelector("#status")?.textContent })), 3000, "failure window observation"); await bounded(page.screenshot({ path: resolve(out, "failure.png"), fullPage: true }), 5000, "failure screenshot"); }
    catch (observationError) { evidence.failureObservationError = String(observationError); }
  }
} finally {
  for (const response of held) response.destroy();
  if (browser) await bounded(browser.close(), 6000, "browser close").catch(e => { evidence.cleanup.browserCloseError = String(e); });
  await stop(browser?.process(), "browser"); await stop(xvfb, "xvfb");
  server.closeAllConnections(); await bounded(new Promise(r => server.close(r)), 3000, "server close");
  if (profile) await rm(profile, { recursive: true, force: true }); if (cache) await rm(cache, { recursive: true, force: true });
  evidence.cleanup.server = "closed"; evidence.cleanup.ownedDirectories = "removed";
  await writeFile(resolve(out, "evidence.json"), JSON.stringify(evidence, null, 2));
  console.log("FINAL", evidence.status, evidence.failure ?? "", out);
}
