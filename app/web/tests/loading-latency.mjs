// Coordinator-run, paired software browser measurements. Production responses,
// replay bytes and submitted frames are observed; none are synthesized here.
// EXPECTED_WASM_SHA256=<fresh hash> node app/web/tests/loading-latency.mjs
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { readFile, writeFile, mkdir, readdir, mkdtemp, rm, statfs } from "node:fs/promises";
import { createHash, randomUUID } from "node:crypto";
import { resolve, relative, extname, sep } from "node:path";
import { spawn } from "node:child_process";
import { cpus, platform, release } from "node:os";

const root = process.cwd();
const out = resolve(process.env.LOADING_LATENCY_OUT ?? "target/wf/player-loading-latency/browser-latency");
const baseline = resolve(process.env.LOADING_LATENCY_BASELINE ?? "target/wf/player-loading-latency/baseline-e421b59");
const trials = Number(process.env.LOADING_LATENCY_TRIALS ?? 5);
assert(Number.isInteger(trials) && trials >= 5 && trials <= 20, "at least five bounded comparable trials are required");
const expected = process.env.EXPECTED_WASM_SHA256;
assert.match(expected ?? "", /^[a-f0-9]{64}$/, "identify the coordinator-built current WASM explicitly");
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
const baselineManifest = JSON.parse(await readFile(resolve(baseline, "manifest.json"), "utf8"));
assert.match(baselineManifest.source_revision, /^[a-f0-9]{7,40}$/);
assert(Object.keys(baselineManifest.artifacts).length >= 91, "complete frozen dependency tree required");
// Check every frozen artifact, including files not requested in a given trial.
for (const [name, artifact] of Object.entries(baselineManifest.artifacts)) {
  const path = resolve(baseline, name), from = relative(baseline, path);
  assert(!from.startsWith("..") && !from.startsWith(sep));
  const bytes = await readFile(path);
  assert.equal(bytes.length, artifact.bytes, name + " frozen length");
  assert.equal(sha(bytes), artifact.sha256, name + " frozen hash");
}
assert.equal(sha(await readFile(resolve(root, "app/web/pkg/beatkernel_bms_runtime_bg.wasm"))), expected);
const sourceRevision = process.env.LOADING_LATENCY_SOURCE_REVISION;
assert.match(sourceRevision ?? "", /^[a-f0-9]{7,40}$/, "LOADING_LATENCY_SOURCE_REVISION identifies current source; hashes also include uncommitted bytes");
await mkdir(out, { recursive: true });

const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function bounded(promise, ms, name) {
  let timer;
  return Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(Error(name + " timeout")), ms); })])
    .finally(() => clearTimeout(timer));
}
async function until(predicate, ms, name) {
  const deadline = performance.now() + ms;
  while (!predicate()) {
    const remaining = deadline - performance.now();
    if (remaining <= 0) throw Error(name + " timeout");
    await delay(Math.min(20, remaining));
  }
}

// Identical external wrappers on both builds. This retains the real File API,
// adds a controlled wait before one real acquisition, and observes public calls.
// Static dependency evaluation installs probes before the unmodified Worker.
const workerWrapper = Buffer.from('import "./latency-file-probe.mjs";\nimport "./latency-actual-worker.js";\n');
const fileProbe = Buffer.from(`import * as runtime from './pkg/beatkernel_bms_runtime.js';
const trial=new URL(self.location.href).searchParams.get('latencyTrial');
const fileRead=File.prototype.arrayBuffer;
const notify=(kind,data)=>self.postMessage({kind:'latency-observation',observation:kind,...data});
File.prototype.arrayBuffer=async function(){
 const begin=performance.now();notify('read-begin',{name:this.name,size:this.size,workerAt:begin});
 if(this.name==='unrelated-hung.bin')await new Promise(()=>{});
 if(this.name==='large.wav')await fetch('/latency-gate?'+new URLSearchParams({trial}),{cache:'no-store'});
 try{const result=await fileRead.call(this);notify('read-end',{name:this.name,size:this.size,bytes:result.byteLength,durationMs:performance.now()-begin});return result;}
 catch(error){notify('read-error',{name:this.name,message:String(error)});throw error;}
};
for(const name of ['take_pcm_array','take_pcm']){
 const original=runtime.BrowserSample?.prototype[name];if(typeof original!=='function')continue;
 runtime.BrowserSample.prototype[name]=function(...args){const begin=performance.now();const value=original.apply(this,args);
 notify('pcm-export',{method:name,bytes:value.byteLength,durationMs:performance.now()-begin});return value;};
}
for(const name of ['referenced_asset_paths','replay_referenced_asset_paths']){
 const original=runtime.BrowserLibrary?.prototype[name];if(typeof original!=='function')continue;
 runtime.BrowserLibrary.prototype[name]=function(...args){
  const beforeParse=this.query_parse_count,beforeCompile=this.query_compile_count,begin=performance.now();
  const result=original.apply(this,args);
  notify('dependency-query',{durationMs:performance.now()-begin,
   parses:typeof beforeParse==='bigint'?Number(this.query_parse_count-beforeParse):null,
   compiles:typeof beforeCompile==='bigint'?Number(this.query_compile_count-beforeCompile):null});return result;};
}
`);

// Observers use one Window monotonic clock. Worker-local phase duration is a
// local difference; absolute Worker timestamps are never subtracted from Window.
function observeWindow() {
  const state = window.__latency = { messages: [], sent: [], inputs: [], marks: [], workers: [], replays: [] };
  const clone = data => JSON.parse(JSON.stringify(data, (_, value) => typeof value === "bigint" ? String(value)
    : ArrayBuffer.isView(value) ? { byteLength: value.byteLength } : value));
  const Native = window.Worker;
  window.Worker = class extends Native {
    constructor(url, options) {
      const actual = new URL(url, location.href);
      if (actual.pathname.endsWith("/worker.js")) actual.searchParams.set("latencyTrial", new URL(location.href).searchParams.get("trial"));
      super(actual, options); state.workers.push(this);
      this.addEventListener("message", ({ data }) => {
        if (!data || typeof data.kind !== "string") return;
        const at = performance.now();
        if (data.kind === "play-stopped" && data.replay instanceof Uint8Array) state.replays.push({ playId: data.playId, bytes: Array.from(data.replay), at });
        if (["ready", "catalog", "selected", "selection-error", "selection-progress", "play-preparation-progress", "play-reply", "play-stopped", "play-error",
          "fatal", "render-geometry", "drawn", "render-error", "menu-state", "menu-error", "latency-observation", "audio-ready"].includes(data.kind)) {
          // PCM payloads and render packets are never copied by this observer.
          state.messages.push({ ...clone(data), at });
        }
      });
    }
    postMessage(message, transfer) {
      if (["import", "accept-library", "select", "play-start", "play-stop", "resize", "menu-open", "menu-select"].includes(message?.kind)) {
        state.sent.push({ at: performance.now(), kind: message.kind, id: message.id, libraryId: message.libraryId,
          playId: message.playId, rpcId: message.rpcId, path: message.path, width: message.width, height: message.height });
      }
      return super.postMessage(message, transfer);
    }
  };
  document.addEventListener("keydown", event => state.inputs.push({ at: performance.now(), eventAt: event.timeStamp,
    code: event.code, trusted: event.isTrusted }), true);
  const observer = new MutationObserver(() => {
    const last = state.messages.findLast(message => message.kind === "catalog");
    const chart = document.querySelector("#chart"), prepare = document.querySelector("#prepare");
    if (last && chart?.options.length && !prepare?.disabled && !state.marks.some(mark => mark.kind === "catalog-selectable" && mark.id === last.id)) {
      state.marks.push({ kind: "catalog-selectable", id: last.id, at: performance.now() });
    }
  });
  observer.observe(document, { childList: true, subtree: true, attributes: true });
}

function wav(frames, polarity = 1) {
  const bytes = Buffer.alloc(44 + frames * 2);
  bytes.write("RIFF"); bytes.writeUInt32LE(bytes.length - 8, 4); bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(48000, 24); bytes.writeUInt32LE(96000, 28); bytes.writeUInt16LE(2, 32); bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36); bytes.writeUInt32LE(frames * 2, 40);
  for (let i = 0; i < frames; i++) bytes.writeInt16LE(polarity * Math.round(Math.sin(i * Math.PI * 880 / 48000) * 1200), 44 + i * 2);
  return bytes;
}
const config = { trials, viewport: { width: 1200, height: 1000 }, largeEncodedBytes: 8 * 1024 * 1024 + 44,
  rate: 48000, channels: 2, seed: "0", sectionStart: "0.25", sectionEnd: "1.5", heldInput: "KeyZ", gate: "external File acquisition before real arrayBuffer",
  renderer: "software Vulkan SwiftShader", browser: "/usr/bin/chromium", display: "owned Xvfb 1400x1200x24",
  clock: "Window performance.now; Worker phase durations use only local differences", request: "actual production Worker postMessage",
  presentation: "actual RenderClient render-geometry ACK after RendererWorker draw; visible canvas required",
  input: "trusted CDP keyboard event acquisition; excludes physical device/OS latency", warmth: "same actual accepted BrowserLibrary/cache, output rate fixed" };
const configHash = sha(JSON.stringify(config));
const fixtures = {
  "bootstrap/short.bms": Buffer.from("#TITLE Benchmark bootstrap\n#BPM 120\n#RANK 2\n#WAV01 short.wav\n#00011:01010000\n"),
  "bootstrap/short.wav": wav(9600),
  "original/large.bms": Buffer.from("#TITLE Benchmark large\n#BPM 120\n#RANK 2\n#WAV01 large.wav\n#WAV02 large.wav\n#00011:01020000\n"),
  "original/large.wav": wav(4 * 1024 * 1024),
  "original/unrelated-hung.bin": Buffer.alloc(1024 * 1024),
  "replacement/large.bms": Buffer.from("#TITLE Benchmark changed content\n#BPM 121\n#RANK 2\n#WAV01 large.wav\n#WAV02 large.wav\n#00011:01020000\n"),
  "replacement/large.wav": wav(4 * 1024 * 1024, -1),
};
const fixtureManifest = {};
for (const [name, bytes] of Object.entries(fixtures)) {
  const file = resolve(out, "fixtures", name); await mkdir(resolve(file, ".."), { recursive: true }); await writeFile(file, bytes);
  fixtureManifest[name] = { sha256: sha(bytes), bytes: bytes.length };
}
const fixtureHash = sha(JSON.stringify(fixtureManifest));
const artifacts = { before: new Map(), after: new Map() };
for (const [name, value] of Object.entries(baselineManifest.artifacts)) {
  if (name.startsWith("web/")) artifacts.before.set(name.slice(4), { file: resolve(baseline, name), sha256: value.sha256 });
}
async function currentTree(directory, prefix = "") {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const name = prefix + entry.name;
    if (entry.isDirectory()) {
      if (!["tests", "node_modules"].includes(entry.name)) await currentTree(resolve(directory, entry.name), name + "/");
    } else if (/\.(?:html|css|js|mjs|wasm|woff2?|ttf|png|svg)$/.test(name) && !name.endsWith(".test.mjs")) {
      const file = resolve(directory, entry.name); artifacts.after.set(name, { file, sha256: sha(await readFile(file)) });
    }
  }
}
await currentTree(resolve(root, "app/web"));
for (const variant of ["before", "after"]) {
  for (const name of ["index.html", "main.js", "worker.js", "renderer-worker.js", "pkg/beatkernel_bms_runtime.js", "pkg/beatkernel_bms_runtime_bg.wasm"]) {
    assert(artifacts[variant].has(name), variant + " required artifact " + name);
  }
  assert([...artifacts[variant].keys()].some(name => name.startsWith("audio-pkg/") && name.endsWith(".wasm")), variant + " full audio dependency tree");
}
const artifactManifest = variant => Object.fromEntries([
  ...[...artifacts[variant]].map(([name, artifact]) => ["/" + variant + "/" + (name === "worker.js" ? "latency-actual-worker.js" : name), artifact.sha256]),
  ["/" + variant + "/worker.js", sha(workerWrapper)], ["/" + variant + "/latency-file-probe.mjs", sha(fileProbe)],
  ["/" + variant + "/", artifacts[variant].get("index.html").sha256],
]);
const records = { schema_version: 1, workload_id: "production-browser-loading-paired", path: "browser",
  environment: { platform: platform(), kernel: release(), cpu: cpus()[0]?.model, logical_cpus: cpus().length, node: process.version,
    config, fixture_manifest: fixtureManifest, baseline_manifest_sha256: sha(await readFile(resolve(baseline, "manifest.json"))),
    ceilings: ["Software Chromium/SwiftShader/Xvfb only; no physical audio/input or world-fastest claim.",
      "Render geometry ACK proves submitted usable geometry, not physical monitor scanout.",
      "Warm preparation still exports independent JS PCM for actual consumers; Rust copy/retained-byte counters absent from public Worker remain unavailable.",
      "External read gates are identical diagnostic controls; held-duration and Puppeteer/observer overhead belong to both measured workloads.",
      "Baseline has no preparation phase/job counters; unavailable historical work is never inferred from current counters."] },
  fixture_sha256: fixtureHash, config_sha256: configHash, groups: [] };
const detail = { status: "RUNNING", fixtureManifest, config, trials: [], cleanup: {}, errors: [] };
const gates = new Map(), pendingResponses = new Set(), loaded = new Map();
const headers = { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cache-Control": "no-store" };
const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, "http://localhost");
    if (url.pathname === "/latency-gate") {
      const gate = gates.get(url.searchParams.get("trial"));
      if (!gate) { res.writeHead(409, headers); res.end(); return; }
      if (gate.released) { res.writeHead(204, headers); res.end(); return; }
      gate.responses.add(res); pendingResponses.add(res);
      res.on("close", () => { gate.responses.delete(res); pendingResponses.delete(res); }); return;
    }
    const match = /^\/(before|after)\/(.*)$/.exec(decodeURIComponent(url.pathname));
    if (!match || match[2].split("/").includes("..")) throw Error("unknown resource");
    const [, variant, requested] = match, name = requested || "index.html";
    let bytes, expectedHash;
    if (name === "worker.js") { bytes = workerWrapper; expectedHash = sha(workerWrapper); }
    else if (name === "latency-file-probe.mjs") { bytes = fileProbe; expectedHash = sha(fileProbe); }
    else {
      // Baseline is manifest-only: no lookup or fallback into current app/web.
      const artifact = artifacts[variant].get(name === "latency-actual-worker.js" ? "worker.js" : name);
      if (!artifact) throw Error("resource absent from immutable variant manifest");
      bytes = await readFile(artifact.file); expectedHash = artifact.sha256;
    }
    assert.equal(sha(bytes), expectedHash, "artifact changed after measurement freeze");
    const key = url.pathname;
    loaded.set(key, expectedHash);
    res.writeHead(200, { ...headers, "Content-Type": ({ ".html": "text/html", ".css": "text/css", ".mjs": "text/javascript", ".js": "text/javascript", ".wasm": "application/wasm" })[extname(name)] ?? "application/octet-stream",
      "X-Latency-Artifact-Sha256": expectedHash }); res.end(bytes);
  } catch (error) { detail.errors.push({ resource: req.url, message: String(error) }); res.writeHead(404, headers); res.end(); }
});

let browser, xvfb, display, processCounter = 0;
const resources = new Set();
async function stop(child, label) {
  if (!child) return;
  if (child.exitCode === null && child.signalCode === null) {
    child.kill("SIGTERM");
    try { await bounded(new Promise(resolve => child.once("exit", resolve)), 4000, label); }
    catch { child.kill("SIGKILL"); await bounded(new Promise(resolve => child.once("exit", resolve)), 4000, label + " kill"); }
  }
}
function groupFor(variant, scenario, temperature) {
  let group = records.groups.find(group => group.variant === variant && group.scenario === scenario && group.temperature === temperature);
  if (!group) {
    group = { variant, scenario, temperature, fixture_sha256: fixtureHash, config_sha256: configHash,
      source_revision: variant === "before" ? baselineManifest.source_revision : sourceRevision,
      artifact_manifest: artifactManifest(variant), trials: [] }; records.groups.push(group);
  }
  return group;
}
function event(name, at, start, correlation, source) {
  assert(at >= start, name + " must follow its actual request");
  return { name, at_ms: at - start, correlation_id: correlation, source };
}
async function checkpoint() {
  await writeFile(resolve(out, "observations.json"), JSON.stringify(detail, null, 2));
  await writeFile(resolve(out, "trials.json"), JSON.stringify(records, null, 2));
}
async function runProcess(puppeteer, base, variant, index, scenario) {
  const owner = variant + "-" + scenario + "-" + index + "-" + randomUUID();
  const gate = { released: false, responses: new Set() }; gates.set(owner, gate);
  loaded.clear(); // All previous Chromium processes have joined before this one.
  const profile = await mkdtemp(resolve(out, "profile-")), cache = await mkdtemp("/dev/shm/beatkernel-loading-latency-");
  resources.add(profile); resources.add(cache);
  const processId = owner + "-process-" + (++processCounter);
  const trialDetail = { owner, processId, variant, index, scenario, errors: [], consoleErrors: [], httpErrors: [], loaded: {}, operations: [] };
  detail.trials.push(trialDetail);
  try {
    browser = await puppeteer.launch({ env: { ...process.env, DISPLAY: display }, executablePath: config.browser, headless: false, userDataDir: profile,
      args: ["--no-sandbox", "--enable-unsafe-webgpu", "--enable-unsafe-swiftshader", "--use-angle=vulkan", "--use-vulkan=swiftshader",
        "--enable-features=Vulkan", "--disable-vulkan-surface", "--disk-cache-dir=" + cache, "--disk-cache-size=16777216"] });
    trialDetail.browserCommand = browser.process().spawnargs; trialDetail.browserVersion = await browser.version();
    records.environment.browser_version = trialDetail.browserVersion;
    const page = await browser.newPage(); await page.setViewport(config.viewport); page.setDefaultTimeout(30000);
    await page.evaluateOnNewDocument(observeWindow);
    page.on("pageerror", error => trialDetail.errors.push(String(error)));
    page.on("console", message => { if (message.type() === "error") trialDetail.consoleErrors.push(message.text()); });
    page.on("response", response => {
      const url = new URL(response.url());
      if (response.status() >= 400 && !url.pathname.endsWith("favicon.ico")) trialDetail.httpErrors.push({ url: response.url(), status: response.status() });
      const digest = response.headers()["x-latency-artifact-sha256"];
      if (digest) {
        if (artifactManifest(variant)[url.pathname] !== digest) trialDetail.errors.push("response differs from frozen manifest: " + url.pathname);
        trialDetail.loaded[url.pathname] = digest;
      }
    });
    await page.goto(base + "/" + variant + "/?trial=" + encodeURIComponent(owner), { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => !document.querySelector("#files").disabled);
    const snapshot = () => page.evaluate(() => ({ messages: __latency.messages, sent: __latency.sent, inputs: __latency.inputs, marks: __latency.marks,
      status: document.querySelector("#status").textContent, canvasVisible: !document.querySelector("#canvas").hidden,
      canvasBounds: { width: document.querySelector("#canvas").getBoundingClientRect().width, height: document.querySelector("#canvas").getBoundingClientRect().height } }));
    const cursor = () => page.evaluate(() => ({ messages: __latency.messages.length, sent: __latency.sent.length, inputs: __latency.inputs.length, at: performance.now() }));
    const upload = async names => {
      const before = await cursor();
      await (await page.$("#files")).uploadFile(...names.map(name => resolve(out, "fixtures", name)));
      await page.waitForFunction(n => __latency.messages.slice(n).some(message => message.kind === "catalog") && !document.querySelector("#prepare").disabled, {}, before.messages);
      const observed = await snapshot(), request = observed.sent.slice(before.sent).find(message => message.kind === "import"); assert(request);
      const catalog = observed.marks.find(mark => mark.kind === "catalog-selectable" && mark.id === request.id); assert(catalog);
      assert(observed.sent.some(message => message.kind === "accept-library" && message.id === request.id));
      return { before, request, catalog, library: processId + ":library:" + request.id, cache: processId + ":library-cache:" + request.id };
    };
    const firstGeometry = async (before, mode, id, field = "selectedId") => {
      await page.waitForFunction((n, mode, id, field) => __latency.messages.slice(n).some(message => message.kind === "render-geometry" && message.mode === mode
        && (id === null || String(message[field]) === String(id)) && message.width > 0 && message.height > 0)
        && !document.querySelector("#canvas").hidden && document.querySelector("#canvas").getBoundingClientRect().width > 0, {}, before.messages, mode, id, field);
      const observed = await snapshot();
      assert(observed.canvasVisible && observed.canvasBounds.width > 0 && observed.canvasBounds.height > 0);
      return observed.messages.slice(before.messages).find(message => message.kind === "render-geometry" && message.mode === mode
        && (id === null || String(message[field]) === String(id)) && message.width > 0 && message.height > 0);
    };
    const select = async chart => {
      const before = await cursor(); await page.select("#chart", chart); await page.click("#prepare");
      await page.waitForFunction(n => __latency.sent.slice(n).some(message => message.kind === "select"), {}, before.sent);
      const request = (await snapshot()).sent.slice(before.sent).find(message => message.kind === "select"); assert.equal(request.path, chart);
      return { before, request };
    };
    const selected = async operation => {
      await page.waitForFunction(id => __latency.messages.some(message => message.kind === "selected" && message.id === id)
        && document.querySelector("#status").textContent.startsWith("Chart prepared"), {}, operation.request.id);
      return (await snapshot()).messages.find(message => message.kind === "selected" && message.id === operation.request.id);
    };
    const releaseGate = async () => {
      const at = await page.evaluate(() => performance.now()); gate.released = true;
      for (const response of gate.responses) { response.writeHead(204, headers); response.end(); }
      return at;
    };
    const heldResponsiveness = async operation => {
      await until(() => gate.responses.size > 0, 15000, "real selected File read held");
      const held = await snapshot();
      assert(!held.messages.some(message => message.kind === "selected" && message.id === operation.request.id), "actual gated preparation cannot be ready");
      const old = held.messages.findLast(message => message.kind === "render-geometry"); assert(old, "existing actual frame needed for pending redraw");
      await page.evaluate(() => document.activeElement?.blur());
      const beforeInput = await cursor(); await page.keyboard.press(config.heldInput);
      await page.waitForFunction(n => __latency.inputs.slice(n).some(input => input.trusted && input.code === "KeyZ"), {}, beforeInput.inputs);
      const resizeAt = await page.evaluate(() => performance.now());
      await page.setViewport({ width: 1180, height: 1000 });
      await page.waitForFunction((n, version) => __latency.messages.slice(n).some(message => message.kind === "render-geometry"
        && BigInt(message.geometryVersion) > BigInt(version) && message.width > 0 && message.height > 0), {}, held.messages.length, old.geometryVersion);
      const observed = await snapshot(), geometry = observed.messages.slice(held.messages.length).find(message => message.kind === "render-geometry" && BigInt(message.geometryVersion) > BigInt(old.geometryVersion));
      assert(!observed.messages.some(message => message.kind === "selected" && message.id === operation.request.id));
      const input = observed.inputs.slice(beforeInput.inputs).find(input => input.trusted && input.code === "KeyZ");
      assert(input.at >= input.eventAt && geometry.at >= resizeAt);
      return { input, geometry, resizeAt };
    };
    const finishRecord = async (scenarioName, temperature, library, operation, ready, geometry, extras = {}) => {
      const observed = await snapshot(), start = operation.request.at;
      const correlation = owner + ":" + scenarioName + ":" + (operation.request.id ?? operation.request.playId);
      const relevant = observed.messages.slice(operation.before.messages).filter(message => scenarioName !== "catalog" || message.at <= geometry.at);
      const progress = relevant.filter(message => ["selection-progress", "play-preparation-progress"].includes(message.kind)
        && (operation.request.playId !== undefined ? message.playId === operation.request.playId : message.id === (extras.preparationId ?? operation.request.id)));
      const queries = relevant.filter(message => message.kind === "latency-observation" && message.observation === "dependency-query");
      const reads = relevant.filter(message => message.kind === "latency-observation" && message.observation === "read-end");
      const exports = relevant.filter(message => message.kind === "latency-observation" && message.observation === "pcm-export");
      const current = progress.findLast(message => message.stage === "ready") ?? progress.at(-1);
      if (variant === "after" && ["warm_preview", "warm_retry", "section", "replay"].includes(scenarioName)) {
        assert(current, "actual current stage counters required"); assert.equal(current.decodes, 0, "identical warm original must not repeat codec work");
        assert.equal(current.parses, 0); assert.equal(current.compiles, 0);
      }
      if (variant === "after" && scenarioName === "library_content_replacement") {
        assert(current && current.decodes > 0, "changed actual content in a new library must decode its own original");
      }
      const events = [event("request", start, start, correlation, "Window observation of production Worker postMessage")];
      if (extras.catalog) events.push(event("catalog_selectable", extras.catalog.at, start, correlation, "actual enabled catalog DOM MutationObserver"));
      if (ready.kind !== "catalog-selectable") events.push(event("prepared_ready", ready.at, start, correlation, "genuine " + ready.kind + " response"));
      events.push(event("first_presented_usable", geometry.at, start, correlation, "genuine render-geometry submitted draw ACK, visible canvas"));
      if (extras.audioReady) {
        assert.equal(extras.audioReady.kind, "play-reply");
        assert.equal(extras.audioReady.playId, operation.request.playId);
        assert.equal(extras.audioReady.result?.kind, "audio-ready");
        assert(extras.audioReady.at >= ready.at, "actual audio acknowledgement must follow preparation");
        events.push(event("audio_ready", extras.audioReady.at, start, correlation,
          "genuine play-reply result audio-ready for the same playId, observed in Window"));
      }
      if (extras.pending) events.push(event("first_pending_frame", extras.pending.geometry.at, start, correlation, "genuine resized prior frame while File read held"));
      if (extras.releasedAt !== undefined) events.push(event("held_work_released", extras.releasedAt, start, correlation, "external real File acquisition gate release"));
      events.sort((a, b) => a.at_ms - b.at_ms);
      const count = key => current ? current[key] + queries.reduce((sum, query) => sum + (query[key] ?? 0), 0) : null;
      const trial = { trial_id: correlation, process_id: processId, library_id: library.library, cache_owner_id: library.cache,
        reused_owner_id: temperature === "warm" ? library.cache : null, correlation_id: correlation, events,
        loaded_artifacts: { ...trialDetail.loaded }, phases_ms: { dependency_query: queries.length ? queries.reduce((sum, item) => sum + item.durationMs, 0) : null,
          file_acquisition: reads.reduce((sum, item) => sum + item.durationMs, 0), export: exports.reduce((sum, item) => sum + item.durationMs, 0),
          begin: current?.beginMs ?? null, max_indivisible_step: current?.maxStepMs ?? null, parse: null, compile: null, decode: null,
          prepare_to_submitted_frame: geometry.at - ready.at },
        counters: { file_reads: reads.length, acquired_encoded_bytes: reads.reduce((sum, item) => sum + item.bytes, 0), parses: count("parses"), compiles: count("compiles"),
          codec_decodes: current?.decodes ?? null, canonical_asset_reads: current?.reads ?? null, shared_assets: current?.sharedAssets ?? null,
          pcm_exports: exports.length, js_pcm_export_bytes: exports.reduce((sum, item) => sum + item.bytes, 0), rust_pcm_payload_copies: null, rust_retained_bytes: null },
        input_latency_ms: extras.pending ? [extras.pending.input.at - extras.pending.input.eventAt] : [],
        redraw_latency_ms: extras.pending ? [extras.pending.geometry.at - extras.pending.resizeAt] : [] };
      assert(trial.events[0].name === "request" && trial.events[0].at_ms === 0);
      groupFor(variant, scenarioName, temperature).trials.push(trial);
      trialDetail.operations.push({ scenario: scenarioName, operation, ready, geometry, ...extras, progress, queries, reads, exports, observedStatus: observed.status });
      await page.screenshot({ path: resolve(out, owner + "-" + scenarioName + ".png"), fullPage: true });
      await checkpoint();
    };
    const bootstrap = ["bootstrap/short.bms", "bootstrap/short.wav"];
    const original = ["original/large.bms", "original/large.wav", "original/unrelated-hung.bin"];
    if (scenario === "catalog") {
      const library = await upload(original);
      assert.equal((await snapshot()).messages.filter(message => message.kind === "latency-observation" && message.observation === "read-begin").length, 0);
      const beforeMenu = await cursor(); await page.click("#menu-open");
      const geometry = await firstGeometry(beforeMenu, "menu", null);
      // The catalog remains selectable before its unreferenced hung file or the
      // selected large media is acquired. Start a real preparation afterwards.
      const operation = await select("large.bms");
      await until(() => gate.responses.size > 0, 15000, "catalog precedes real selected media acquisition");
      const beforeInput = await cursor(); await page.evaluate(() => document.activeElement?.blur()); await page.keyboard.press(config.heldInput);
      await page.waitForFunction(n => __latency.inputs.slice(n).some(input => input.trusted && input.code === "KeyZ"), {}, beforeInput.inputs);
      const releasedAt = await releaseGate(); await selected(operation);
      await finishRecord("catalog", "cold", library, { before: library.before, request: library.request }, library.catalog, geometry,
        { catalog: library.catalog, releasedAt });
    } else {
      // A previous genuine preview supports redraw probes. Import a NEW library
      // afterwards, so measured cold owners have never prepared an original.
      await upload(bootstrap); const setup = await select("short.bms"); await selected(setup); await firstGeometry(setup.before, "preview", setup.request.id);
      let library = await upload(original), operation = await select("large.bms");
      const pending = await heldResponsiveness(operation), releasedAt = await releaseGate();
      let ready = await selected(operation), geometry = await firstGeometry(operation.before, "preview", operation.request.id);
      if (scenario === "selected_large") {
        await finishRecord("selected_large_audio", "cold", library, operation, ready, geometry, { pending, releasedAt });
        operation = await select("large.bms"); ready = await selected(operation); geometry = await firstGeometry(operation.before, "preview", operation.request.id);
        await finishRecord("warm_preview", "warm", library, operation, ready, geometry);

        const setValue = (selector, value) => page.$eval(selector, (element, value) => { element.value = value; element.dispatchEvent(new Event("input", { bubbles: true })); element.dispatchEvent(new Event("change", { bubbles: true })); }, value);
        await setValue("#output-rate", "48000");
        const play = async (name, mode, record = false) => {
          if (mode === "live") await page.$eval("#record", (element, checked) => { element.checked = checked; element.dispatchEvent(new Event("change", { bubbles: true })); }, record);
          const before = await cursor(); await page.click(mode === "replay" ? "#replay-play" : "#play");
          await page.waitForFunction(n => __latency.sent.slice(n).some(message => message.kind === "play-start"), {}, before.sent);
          const request = (await snapshot()).sent.slice(before.sent).find(message => message.kind === "play-start");
          await page.waitForFunction(id => __latency.messages.some(message => message.kind === "play-reply" && message.playId === id && message.result?.kind === "prepared"), {}, request.playId);
          const ready = (await snapshot()).messages.find(message => message.kind === "play-reply" && message.playId === request.playId && message.result?.kind === "prepared");
          const geometry = await firstGeometry(before, mode, request.playId, "playId");
          // Real sample handoff/AudioWorklet startup is required before Stop.
          await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Playing") && !document.querySelector("#stop").disabled);
          const audioReady = (await snapshot()).messages.slice(before.messages).find(message => message.kind === "play-reply"
            && message.playId === request.playId && message.result?.kind === "audio-ready");
          assert(audioReady, variant + " actual audio-ready acknowledgement unavailable; cannot measure audio readiness from Playing or geometry");
          await finishRecord(name, "warm", library, { before, request }, ready, geometry, { audioReady });
          await page.click("#stop");
          await page.waitForFunction(id => __latency.messages.some(message => message.kind === "play-stopped" && message.playId === id)
            && !document.querySelector("#play").disabled, {}, request.playId);
          assert(!(await snapshot()).messages.some(message => message.kind === "play-error" && message.playId === request.playId));
          return request.playId;
        };
        await play("warm_retry", "live");
        await setValue("#live-start", config.sectionStart); await setValue("#live-end", config.sectionEnd);
        const captureId = await play("section", "live", true);
        const replay = await page.evaluate(id => __latency.replays.find(item => item.playId === id), captureId);
        assert(replay && replay.bytes.length > 0, "actual stopped/fenced game capture required");
        const replayPath = resolve(out, owner + "-genuine-section.bkr"); await writeFile(replayPath, Buffer.from(replay.bytes));
        trialDetail.replay = { sha256: sha(Buffer.from(replay.bytes)), bytes: replay.bytes.length, playId: captureId, complete: false };
        await (await page.$("#replay-file")).uploadFile(replayPath);
        await page.waitForFunction(() => !document.querySelector("#replay-play").disabled);
        await play("replay", "replay");
      } else {
        assert.equal(scenario, "replacement");
        const previous = library;
        library = await upload(["replacement/large.bms", "replacement/large.wav"]);
        assert.notEqual(library.cache, previous.cache);
        operation = await select("large.bms"); ready = await selected(operation); geometry = await firstGeometry(operation.before, "preview", operation.request.id);
        assert.match(await page.$eval("#title", element => element.textContent), /changed content/);
        await finishRecord("library_content_replacement", "cold", library, { before: library.before, request: library.request }, ready, geometry,
          { catalog: library.catalog, preparationId: operation.request.id });
      }
    }
    const final = await snapshot();
    assert(!final.messages.some(message => ["fatal", "selection-error", "play-error", "render-error"].includes(message.kind)), "actual production owner must remain healthy");
    assert(!final.messages.some(message => message.kind === "latency-observation" && message.name === "unrelated-hung.bin"), "unreferenced media must remain unacquired");
    assert.equal(trialDetail.errors.length, 0); assert.equal(trialDetail.httpErrors.length, 0);
    // Worker dependency fetches bypass page response callbacks in Chromium.
    // Record only actually served frozen URLs, with variant separation.
    for (const [url, digest] of loaded) if (url.startsWith("/" + variant + "/")) trialDetail.loaded[url] = digest;
    for (const group of records.groups) for (const trial of group.trials) if (trial.process_id === processId) trial.loaded_artifacts = { ...trialDetail.loaded };
    trialDetail.status = "PASS";
  } finally {
    gate.released = true; for (const response of gate.responses) { response.writeHead(204, headers); response.end(); }
    gates.delete(owner);
    if (browser) {
      const child = browser.process();
      try { await bounded(browser.close(), 10000, "owned Chromium close"); } finally { await stop(child, "owned Chromium"); browser = null; }
    }
    await rm(profile, { recursive: true, force: true }); await rm(cache, { recursive: true, force: true }); resources.delete(profile); resources.delete(cache);
    await checkpoint();
  }
}

try {
  await bounded(new Promise(resolve => server.listen(0, "127.0.0.1", resolve)), 5000, "measurement server");
  const base = "http://127.0.0.1:" + server.address().port;
  const shm = await statfs("/dev/shm"); assert.equal(shm.type, 0x01021994); assert(shm.bavail * shm.bsize >= 24 * 1024 * 1024);
  xvfb = spawn("/usr/bin/Xvfb", ["-displayfd", "3", "-screen", "0", "1400x1200x24", "-nolisten", "tcp"], { stdio: ["ignore", "ignore", "pipe", "pipe"] });
  display = await bounded(new Promise((resolve, reject) => { let output = "";
    xvfb.stdio[3].on("data", bytes => { output += bytes; if (output.includes("\n")) resolve(":" + output.trim()); });
    xvfb.once("error", reject); xvfb.once("exit", code => reject(Error("Xvfb exit " + code))); }), 10000, "owned Xvfb display");
  const require = createRequire(import.meta.url);
  const puppeteer = require(resolve(process.env.PUPPETEER_MODULE ?? "target/wf/qa-browser-parallel-player-01a119a6-1/deps/node_modules/puppeteer-core"));
  // Interleave before/after under unchanged software configuration; each cold
  // group gets its own processes, libraries and caches, not relabelled warm runs.
  for (let index = 0; index < trials; index++) for (const scenario of ["catalog", "selected_large", "replacement"]) {
    for (const variant of index % 2 ? ["after", "before"] : ["before", "after"]) await runProcess(puppeteer, base, variant, index, scenario);
  }
  for (const group of records.groups) assert.equal(group.trials.length, trials, group.scenario + " paired samples");
  detail.status = "PASS"; await checkpoint();
  console.log(JSON.stringify({ status: detail.status, trials: records.groups.map(group => ({ variant: group.variant, scenario: group.scenario, count: group.trials.length })), output: resolve(out, "trials.json") }));
} catch (error) {
  detail.status = "FAIL"; detail.failure = String(error?.stack ?? error); process.exitCode = 1;
} finally {
  for (const response of pendingResponses) response.destroy();
  server.closeAllConnections?.();
  try { await bounded(new Promise(resolve => server.close(resolve)), 5000, "owned server close"); detail.cleanup.server = "closed"; }
  catch (error) { detail.cleanup.server = String(error); process.exitCode = 1; }
  try { await stop(xvfb, "owned Xvfb"); detail.cleanup.xvfb = { exitCode: xvfb?.exitCode, signal: xvfb?.signalCode }; }
  catch (error) { detail.cleanup.xvfb = String(error); process.exitCode = 1; }
  for (const directory of resources) await rm(directory, { recursive: true, force: true });
  await checkpoint();
}
