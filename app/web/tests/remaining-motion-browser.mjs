// Actual generated WASM and unmodified production RendererWorker acceptance.
// Run only after source writers stop and the coordinator rebuilds current WASM.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { readFile, writeFile, mkdir, rm, mkdtemp, statfs } from "node:fs/promises";
import { resolve, relative, extname, sep } from "node:path";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";

const root = process.cwd();
const out = resolve(process.env.REMAINING_MOTION_OUT ?? "target/wf/remaining-menu-component-motion/actual-browser");
const expectedSha = process.env.EXPECTED_WASM_SHA256;
assert.match(expectedSha ?? "", /^[a-f0-9]{64}$/, "Set EXPECTED_WASM_SHA256 from the coordinator's current build");
const wasmSha = createHash("sha256").update(await readFile("app/web/pkg/beatkernel_bms_runtime_bg.wasm")).digest("hex");
assert.equal(wasmSha, expectedSha);
await mkdir(out, { recursive: true });
const evidence = { status: "RUNNING", wasmSha, steps: [], errors: [], cleanup: {}, ceilings: [
  "Software Chromium/SwiftShader functional acceptance; not physical input/audio latency or hardware performance.",
  "Genuine solo Results exercises a partial page and comparison switching; absent-card pruning across a multi-page roster remains deterministic CLI evidence.",
] };
const delay = ms => new Promise(r => setTimeout(r, ms));
const bounded = (p, ms, label) => { let timer; return Promise.race([p, new Promise((_, reject) => { timer = setTimeout(() => reject(Error(label + " timeout")), ms); })]).finally(() => clearTimeout(timer)); };
const json = value => JSON.parse(JSON.stringify(value, (_, v) => typeof v === "bigint" ? String(v) : v));

const fixture = `<!doctype html><html><body style="margin:0"><canvas id="canvas" width="960" height="720" style="width:960px;height:720px"></canvas><script type="module">
import init,{BrowserMenuOwner,BrowserView} from '/app/web/pkg/beatkernel_bms_runtime.js';
await init();window.owner=new BrowserMenuOwner(211n);window.messages=[];
window.worker=new Worker('/app/web/renderer-worker.js',{type:'module'});
const channel=new MessageChannel();window.port=channel.port1;port.onmessage=e=>messages.push(e.data);port.start();
worker.onmessage=e=>messages.push(e.data);worker.onerror=e=>messages.push({kind:'worker-error',message:e.message});
const canvas=document.querySelector('canvas').transferControlToOffscreen();worker.postMessage({kind:'init',canvas,port:channel.port2,maxPacketBytes:16777216,maxDiagnosticBytes:4096},[canvas,channel.port2]);
let op=0n,geometry=0n,action=0n;window.state=()=>({generation:1n,content:1n,menuGeneration:211n,screen:owner.screen,revision:owner.revision,geometryVersion:geometry});
window.publish=()=>{port.postMessage({kind:'menu',packet:owner.snapshot(),generation:1n,content:1n,operationId:++op,geometryVersion:++geometry});return geometry.toString();};
window.control=(kind,extra={})=>{port.postMessage({...state(),kind,...extra,operationId:++op,geometryVersion:++geometry});return geometry.toString();};
window.motion=(target,dx=-100,durationMs=500)=>control('menu-motion',{control:BigInt(target),transforms:new Float32Array([0,0,1,1,1,dx,0,1,1,1]),durationMs,easing:0});
window.probe=(x,y)=>port.postMessage({...state(),kind:'menu-input',actionId:++action,x,y});
document.querySelector('canvas').addEventListener('pointerup',e=>probe(e.offsetX,e.offsetY));
window.resultsNodes=async bytes=>{const view=await BrowserView.create(new OffscreenCanvas(960,720));try{view.import_visual_packet(Uint8Array.from(bytes),16777216,4096);return Array.from(view.displayed_results_nodes());}finally{view.dispose_results_motion();view.retire_visual();view.free();}};
</script></body></html>`;

function wav() {
  const frames = 9600, bytes = Buffer.alloc(44 + frames * 2);
  bytes.write("RIFF"); bytes.writeUInt32LE(bytes.length - 8, 4); bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(48000, 24); bytes.writeUInt32LE(96000, 28); bytes.writeUInt16LE(2, 32); bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36); bytes.writeUInt32LE(frames * 2, 40);
  for (let i = 0; i < frames; i++) bytes.writeInt16LE(Math.round(Math.sin(i * Math.PI * 880 / 48000) * 1200), 44 + i * 2);
  return bytes;
}
await writeFile(resolve(out, "short.bms"), "#TITLE Remaining motion genuine completion\n#BPM 120\n#TOTAL 320\n#WAV01 x.wav\n#00011:00010000\n");
await writeFile(resolve(out, "x.wav"), wav());
const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, "http://localhost");
    let file = resolve(root, "." + decodeURIComponent(url.pathname));
    const from = relative(root, file);
    if (from === ".." || from.startsWith(".." + sep)) throw Error("outside root");
    if (url.pathname.endsWith("/")) file = resolve(file, "index.html");
    const bytes = url.pathname === "/motion-fixture.html" ? Buffer.from(fixture) : await readFile(file);
    res.writeHead(200, { "Content-Type": ({ ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".css": "text/css" })[extname(file)] ?? "application/octet-stream", "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cache-Control": "no-store" });
    res.end(bytes);
  } catch { res.writeHead(404); res.end(); }
});

// Observation preserves genuine Worker messages and completion packets. It does
// not provide fake engine responses, scores, completion proofs or timestamps.
function observeWindow() {
  window.__motion = { messages: [], workers: [] };
  const Original = Worker;
  window.Worker = class extends Original {
    constructor(url, options) {
      super(url, options); this.probeURL = String(url); __motion.workers.push(this);
      this.addEventListener("message", ({ data }) => {
        if (["play-stopped", "play-error", "render-error", "render-geometry", "menu-motion-reply", "fatal", "disposed", "menu-state", "menu-focus", "menu-error"].includes(data?.kind)) {
          __motion.messages.push(JSON.parse(JSON.stringify(data, (_, v) => typeof v === "bigint" ? String(v) : ArrayBuffer.isView(v) ? { byteLength: v.byteLength } : v)));
        }
      });
    }
    postMessage(data, transfer) {
      if (data?.kind === "menu-edit") __motion.messages.push({ ...data, direction: "sent", screen: String(data.screen), revision: String(data.revision), menuGeneration: String(data.menuGeneration) });
      return super.postMessage(data, transfer);
    }
  };
}
function observeRenderer() {
  if (globalThis.__motionPort) return;
  const p = globalThis.__motionPort = { records: [], packets: [], port: null, op: 0n, geometry: 0n };
  const originalStart = MessagePort.prototype.start, originalPost = MessagePort.prototype.postMessage;
  const seen = new WeakSet();
  MessagePort.prototype.start = function () {
    if (!seen.has(this)) {
      seen.add(this);
      this.addEventListener("message", ({ data }) => {
        if (data?.packet instanceof Uint8Array && data.packet.length >= 40) {
          const h = new DataView(data.packet.buffer, data.packet.byteOffset, data.packet.byteLength);
          const kind = h.getUint16(6, true);
          if (kind === 5) { p.port = this; p.result = { generation: h.getBigUint64(8, true), content: h.getBigUint64(16, true) }; p.packets.push(Array.from(data.packet)); }
        }
        if (typeof data?.operationId === "bigint" && data.operationId > p.op) p.op = data.operationId;
        if (typeof data?.geometryVersion === "bigint" && data.geometryVersion > p.geometry) p.geometry = data.geometryVersion;
      });
    }
    return originalStart.call(this);
  };
  MessagePort.prototype.postMessage = function (data, transfer) {
    if (["drawn", "geometry-ack", "control-ack", "render-error", "draw-wait"].includes(data?.kind)) p.records.push(JSON.parse(JSON.stringify(data, (_, v) => typeof v === "bigint" ? String(v) : v)));
    if (typeof data?.geometryVersion === "bigint" && data.geometryVersion > p.geometry) p.geometry = data.geometryVersion;
    return originalPost.call(this, data, transfer);
  };
  p.control = (kind, extra = {}, stale = null) => {
    if (!p.port || !p.result) throw Error("No genuine Results renderer port");
    const before = p.records.length;
    const geometry = ++p.geometry, op = ++p.op;
    p.port.dispatchEvent(new MessageEvent("message", { data: { kind, ...p.result, ...extra,
      ...(stale === "generation" ? { generation: p.result.generation + 1n } : {}),
      ...(stale === "content" ? { content: p.result.content + 1n } : {}),
      geometryVersion: geometry, operationId: op } }));
    return { before, geometry: String(geometry), operation: String(op) };
  };
}

let browser, xvfb, profile, cacheDir;
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
  await bounded(new Promise(r => server.listen(0, "127.0.0.1", r)), 5000, "server");
  const base = "http://127.0.0.1:" + server.address().port;
  evidence.base = base;
  const shm = await statfs("/dev/shm");
  assert.equal(shm.type, 0x01021994); assert(shm.bavail * shm.bsize >= 24 * 1024 * 1024);
  cacheDir = await mkdtemp("/dev/shm/beatkernel-remaining-motion-");
  profile = await mkdtemp(resolve(out, "profile-"));
  xvfb = spawn("/usr/bin/Xvfb", ["-displayfd", "3", "-screen", "0", "1400x1200x24", "-nolisten", "tcp"], { stdio: ["ignore", "ignore", "pipe", "pipe"] });
  const display = await bounded(new Promise((r, reject) => { let s = ""; xvfb.stdio[3].on("data", b => { s += b; if (s.includes("\n")) r(":" + s.trim()); }); xvfb.once("error", reject); xvfb.once("exit", c => reject(Error("Xvfb exit " + c))); }), 10000, "display");
  const require = createRequire(import.meta.url);
  const puppeteer = require(resolve(process.env.PUPPETEER_MODULE ?? "target/wf/qa-browser-parallel-player-01a119a6-1/deps/node_modules/puppeteer-core"));
  browser = await puppeteer.launch({ env: { ...process.env, DISPLAY: display }, executablePath: "/usr/bin/chromium", headless: false, userDataDir: profile,
    args: ["--no-sandbox", "--enable-unsafe-webgpu", "--enable-unsafe-swiftshader", "--use-angle=vulkan", "--use-vulkan=swiftshader", "--enable-features=Vulkan", "--disable-vulkan-surface", "--disk-cache-dir=" + cacheDir, "--disk-cache-size=16777216"] });
  evidence.browserCommand = browser.process().spawnargs;
  const menu = await browser.newPage(); await menu.setViewport({ width: 1000, height: 800 }); menu.setDefaultTimeout(15000);
  menu.on("pageerror", e => evidence.errors.push(String(e)));
  await menu.goto(base + "/motion-fixture.html", { waitUntil: "domcontentloaded" });
  await menu.waitForFunction(() => window.messages?.some(x => x.kind === "ready"));
  const messages = () => menu.evaluate(() => JSON.parse(JSON.stringify(messages, (_, v) => typeof v === "bigint" ? String(v) : v)));
  const ack = async version => menu.waitForFunction(v => messages.some(x => x.kind === "geometry-ack" && x.geometryVersion === BigInt(v)), {}, version);
  const screenshot = async (page, name) => { const file = resolve(out, name + ".png"); const canvas = await page.$("#canvas"); assert(canvas); const bytes = await canvas.screenshot({ path: file }); evidence.steps.push({ name, screenshot: file, sha256: createHash("sha256").update(bytes).digest("hex") }); return bytes; };
  const metadata = ["1", "1", "1", "7", "keyboard", "1", "keyboard", "keyboard", "KEYBOARD", "SHARED SOURCE", "1"];
  const cases = [
    { name: "settings", route: 2, fields: ["50", "100", "0", "automatic", "0", "48000", "256", "256", "256", "1024", "256", "0", ""], control: 11, x: 370, newX: 180, y: 630 },
    { name: "records", route: 4, fields: ["original.bkr"], control: 55, x: 920, newX: 740, y: 630 },
    { name: "players", route: 5, fields: metadata, control: 31, x: 304, newX: 160, y: 630 },
    { name: "devices", route: 6, fields: metadata, control: 21, x: 370, newX: 180, y: 630 },
  ];
  for (const c of cases) {
    const version = await menu.evaluate(c => { owner.navigate_with_fields(owner.screen, owner.revision, c.route, c.fields); return publish(); }, c);
    await ack(version); const before = await screenshot(menu, c.name + "-before");
    let start = (await messages()).length;
    await menu.mouse.click(c.x, c.y);
    await menu.waitForFunction((n, control) => messages.slice(n).some(x => x.kind === "menu-action" && x.control === BigInt(control)), {}, start, c.control);
    start = (await messages()).length;
    await ack(await menu.evaluate(control => motion(control), c.control));
    await menu.waitForFunction(n => messages.slice(n).filter(x => x.kind === "drawn").length >= 3, {}, start);
    await delay(650); const moved = await screenshot(menu, c.name + "-moved"); assert(!before.equals(moved), "motion screenshot must change");
    const frames = (await messages()).slice(start).filter(x => x.kind === "drawn").length;
    start = (await messages()).length; await menu.mouse.click(c.x, c.y); await delay(100);
    assert(!(await messages()).slice(start).some(x => x.kind === "menu-action" && x.control === String(c.control)), "old position must refuse moved target");
    start = (await messages()).length; await menu.mouse.click(c.newX, c.y);
    await menu.waitForFunction((n, control) => messages.slice(n).some(x => x.kind === "menu-action" && x.control === BigInt(control)), {}, start, c.control);
    if (c.route === 2) await ack(await menu.evaluate(() => { owner.edit(owner.screen, owner.revision, 0, "51"); return publish(); }));
    else await ack(await menu.evaluate(() => { owner.touch(owner.screen, owner.revision); return publish(); }));
    start = (await messages()).length; await menu.mouse.click(c.newX, c.y);
    await menu.waitForFunction((n, control) => messages.slice(n).some(x => x.kind === "menu-action" && x.control === BigInt(control)), {}, start, c.control);
    const old = await menu.evaluate(() => String(owner.screen));
    await ack(await menu.evaluate(() => { owner.back(owner.screen, owner.revision); return publish(); }));
    await ack(await menu.evaluate(c => { owner.navigate_with_fields(owner.screen, owner.revision, c.route, c.fields); return publish(); }, c));
    assert.notEqual(await menu.evaluate(() => String(owner.screen)), old);
    start = (await messages()).length; await menu.mouse.click(c.x, c.y);
    await menu.waitForFunction((n, control) => messages.slice(n).some(x => x.kind === "menu-action" && x.control === BigInt(control)), {}, start, c.control);
    evidence.steps.push({ name: c.name + "-accepted-lifecycle", frames, control: c.control });
  }
  await ack(await menu.evaluate(() => motion(21, -100, 1000)));
  let suspended = await menu.evaluate(() => control("resize", { width: 0, height: 0 }));
  await menu.waitForFunction(v => messages.some(x => x.kind === "control-ack" && x.geometryVersion === BigInt(v)), {}, suspended);
  let count = (await messages()).filter(x => x.kind === "drawn").length; await delay(120);
  assert.equal((await messages()).filter(x => x.kind === "drawn").length, count, "zero extent must stop animation draws");
  await ack(await menu.evaluate(() => control("resize", { width: 960, height: 720 })));

  const live = await browser.newPage(); await live.setViewport({ width: 1200, height: 1000 }); live.setDefaultTimeout(30000);
  live.on("pageerror", e => evidence.errors.push(String(e)));
  await live.evaluateOnNewDocument(observeWindow);
  const pending = [], workers = new Map();
  live.on("workercreated", w => { workers.set(w.url(), w); pending.push(w.evaluate(observeRenderer)); });
  await live.goto(base + "/app/web/", { waitUntil: "domcontentloaded" });
  await live.waitForFunction(() => !document.querySelector("#files").disabled); await Promise.all(pending);
  await (await live.$("#files")).uploadFile(resolve(out, "short.bms"), resolve(out, "x.wav"));
  await live.waitForFunction(() => !document.querySelector("#prepare").disabled); await live.click("#prepare");
  await live.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Chart prepared"));
  // Exercise the production DOM editor bridge independently of the explicit
  // canvas-owner fixtures above. Native IME anchoring is not inferred here.
  await live.click("#menu-open");
  await live.waitForFunction(() => __motion.messages.some(x => x.kind === "menu-state" && x.route === 1));
  const canvasClick = async (x, y) => {
    const canvas = await live.$("#canvas"); await canvas.scrollIntoView();
    const box = await canvas.boundingBox(); assert(box);
    const dimensions = await live.$eval("#canvas", c => ({ width: c.width, height: c.height }));
    await live.mouse.click(box.x + x * box.width / dimensions.width, box.y + y * box.height / dimensions.height);
  };
  await canvasClick(800, 35);
  await live.waitForFunction(() => __motion.messages.some(x => x.kind === "menu-state" && x.route === 2));
  await live.waitForFunction(() => !document.querySelector("#menu-editor").hidden);
  const submittedSettings = () => live.waitForFunction(() => {
    const state = __motion.messages.findLast(x => x.kind === "menu-state");
    const focus = __motion.messages.findLast(x => x.kind === "menu-focus" && x.screen === state?.screen && x.menuGeneration === state?.menuGeneration);
    return state?.route === 2 && (!focus || BigInt(state.revision) >= BigInt(focus.revision))
      && __motion.messages.some(x => x.kind === "render-geometry" && x.mode === "menu"
      && x.menuGeneration === state.menuGeneration && x.screen === state.screen && x.revision === state.revision
      && x.width > 0 && x.height > 0);
  });
  await submittedSettings();
  await canvasClick(350, 130);
  await live.waitForFunction(() => __motion.messages.some(x => x.kind === "menu-focus" && x.index === 0));
  await submittedSettings();
  const domBefore = await screenshot(live, "production-dom-editor-before-motion");
  // Use the actual exported host request. CPU owner counters, RenderClient
  // acknowledgement and Window submitted-geometry correlation stay authoritative.
  const motionAck = await live.evaluate(async () => {
    const { requestMenuMotion } = await import("/app/web/main.js");
    const reply = await requestMenuMotion(1000n, new Float32Array([0, 0, 1, 1, 1, 101, 0, 1, 1, 1]), 500, 0);
    return JSON.parse(JSON.stringify(reply, (_, v) => typeof v === "bigint" ? String(v) : v));
  });
  assert.equal(motionAck.admitted, true);
  assert(BigInt(motionAck.admittedGeometryVersion) > BigInt(motionAck.geometryVersion));
  await live.waitForFunction(reply => __motion.messages.some(x => x.kind === "render-geometry" && x.mode === "menu"
    && x.generation === reply.generation && x.content === reply.content && x.menuGeneration === reply.menuGeneration
    && x.screen === reply.screen && x.revision === reply.revision && x.geometryVersion === reply.admittedGeometryVersion), {}, motionAck);
  await delay(650);
  const domMoved = await screenshot(live, "production-dom-editor-moved");
  assert(!domBefore.equals(domMoved), "actual production editor motion changes canvas pixels");
  const movedFocusStart = await live.evaluate(() => __motion.messages.length);
  await canvasClick(350, 130); await delay(100);
  assert.equal(await live.evaluate(n => __motion.messages.slice(n).filter(x => x.kind === "menu-focus" && x.index === 0).length, movedFocusStart), 0,
    "original editor position must refuse after accepted motion");
  await canvasClick(450, 130);
  await live.waitForFunction(n => __motion.messages.slice(n).some(x => x.kind === "menu-focus" && x.index === 0), {}, movedFocusStart);
  await submittedSettings();
  const focus = await live.evaluate(() => __motion.messages.findLast(x => x.kind === "menu-focus" && x.index === 0));
  const editorStart = await live.evaluate(() => __motion.messages.length);
  await live.click("#menu-editor");
  await live.keyboard.down("Control"); await live.keyboard.press("KeyA"); await live.keyboard.up("Control"); await live.keyboard.type("51");
  await live.waitForFunction(n => __motion.messages.slice(n).some(x => x.kind === "menu-state" && x.route === 2 && x.fields[0] === "51"), {}, editorStart);
  await submittedSettings();
  const edits = await live.evaluate(n => __motion.messages.slice(n).filter(x => x.direction === "sent" && x.kind === "menu-edit"), editorStart);
  assert(edits.length > 0);
  for (const edit of edits) { assert.equal(edit.index, 0); assert.equal(edit.screen, focus.screen); assert.equal(edit.menuGeneration, focus.menuGeneration); }
  const compositionStart = await live.evaluate(() => __motion.messages.length);
  await live.evaluate(() => {
    const editor = document.querySelector("#menu-editor");
    editor.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "52" }));
    editor.value = "52";
    editor.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertCompositionText", data: "52", isComposing: true }));
  });
  await delay(100);
  assert.equal(await live.evaluate(n => __motion.messages.slice(n).filter(x => x.direction === "sent" && x.kind === "menu-edit").length, compositionStart), 0);
  await live.evaluate(() => document.querySelector("#menu-editor").dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "52" })));
  await live.waitForFunction(n => __motion.messages.slice(n).some(x => x.kind === "menu-state" && x.route === 2 && x.fields[0] === "52"), {}, compositionStart);
  await submittedSettings();
  const repaintedFocusStart = await live.evaluate(() => __motion.messages.length);
  await canvasClick(350, 130); await delay(100);
  assert.equal(await live.evaluate(n => __motion.messages.slice(n).filter(x => x.kind === "menu-focus" && x.index === 0).length, repaintedFocusStart), 0,
    "DOM edit/preedit business repaint must retain old-position refusal");
  await canvasClick(450, 130);
  await live.waitForFunction(n => __motion.messages.slice(n).some(x => x.kind === "menu-focus" && x.index === 0 && x.value === "52"), {}, repaintedFocusStart);
  await submittedSettings();
  await screenshot(live, "production-dom-settings-editor");
  const backStart = await live.evaluate(() => __motion.messages.length);
  await live.click("#menu-back");
  await live.waitForFunction(n => __motion.messages.slice(n).some(x => x.kind === "menu-state" && x.route === 1), {}, backStart);
  assert.equal(await live.$eval("#menu-editor", e => e.hidden), true);
  // Replay a captured genuine old focus response, changing no response body.
  await live.evaluate(old => {
    const worker = __motion.workers.find(w => String(w.probeURL ?? "").endsWith("/worker.js"));
    if (!worker) throw Error("Actual business Worker observation missing");
    worker.dispatchEvent(new MessageEvent("message", { data: { ...old, menuGeneration: BigInt(old.menuGeneration), screen: BigInt(old.screen), revision: BigInt(old.revision) } }));
  }, focus);
  assert.equal(await live.$eval("#menu-editor", e => e.hidden), true);
  evidence.steps.push({ name: "production-dom-editor-correlation", motionAck, matchingSubmittedGeometry: true,
    movedTarget: 1000, originalPoint: [350, 130], movedPoint: [450, 130], edits, focus,
    editAndPreeditRepaintRetainsPose: true, composition: "synthetic DOM preedit event; no physical IME claim", staleCapturedFocusIgnored: true });
  await live.click("#record"); await live.click("#play");
  await live.waitForFunction(() => __motion.messages.some(x => x.kind === "play-stopped"));
  const terminal = await live.evaluate(() => __motion.messages.find(x => x.kind === "play-stopped"));
  assert.equal(terminal.replayComplete, true); assert(terminal.completedResults?.proof, "natural completed owner proof required");
  const renderer = [...workers].find(([url]) => url.endsWith("/renderer-worker.js"))?.[1]; assert(renderer);
  await bounded((async () => { for (;;) { if (await renderer.evaluate(() => globalThis.__motionPort?.packets.length > 0)) break; await delay(40); } })(), 10000, "original completed Results packet");
  const packet = await renderer.evaluate(() => __motionPort.packets.at(-1));
  await writeFile(resolve(out, "genuine-results.packet"), Uint8Array.from(packet));
  const nodes = await menu.evaluate(bytes => resultsNodes(bytes), packet);
  assert.equal(nodes.length, 3, "solo Results exposes Scope, one actual Card, Footer only");
  assert.equal(new Set(nodes).size, nodes.length);
  evidence.steps.push({ name: "genuine-results-source", terminal, nodes, packetBytes: packet.length });
  // Make readonly Results drawable using the production owner/page route first.
  const command = async (kind, extra = {}, stale = null) => renderer.evaluate((kind, extra, stale) => {
    if (extra.transforms) extra.transforms = new Float32Array(extra.transforms);
    return __motionPort.control(kind, extra, stale);
  }, kind, extra, stale);
  const resultAck = async c => bounded((async () => { for (;;) { if (await renderer.evaluate(c => __motionPort.records.slice(c.before).some(x => x.kind === "geometry-ack" && x.geometryVersion === c.geometry), c)) return; await delay(25); } })(), 10000, "Results geometry acknowledgement");
  await resultAck(await command("page", { page: 0, comparisons: false }));
  const resultBefore = await screenshot(live, "results-before");
  const movedCommand = await command("results-motion", { node: nodes[1], transforms: [0, 0, 1, 1, 1, 50, 0, 1, 1, 1], durationMs: 600, easing: 0 });
  await resultAck(movedCommand); await delay(750);
  const resultAfter = await screenshot(live, "results-card-moved"); assert(!resultBefore.equals(resultAfter));
  const frames = await renderer.evaluate(n => __motionPort.records.slice(n).filter(x => x.kind === "drawn").length, movedCommand.before); assert(frames >= 3);
  for (const identity of ["generation", "content"]) {
    const stale = await command("results-motion", { node: nodes[1], transforms: [0, 0, 1, 1, 1, -100, 0, 1, 1, 1], durationMs: 0, easing: 0 }, identity);
    await delay(100);
    assert.equal(await renderer.evaluate(c => __motionPort.records.slice(c.before).filter(x => x.kind === "control-ack").length, stale), 0, identity + " mismatch must refuse");
  }
  await resultAck(await command("page", { page: 0, comparisons: true }));
  await resultAck(await command("page", { page: 0, comparisons: false }));
  const frozen = await command("resize", { width: 0, height: 0 });
  await delay(120);
  assert(await renderer.evaluate(c => __motionPort.records.slice(c.before).some(x => x.kind === "control-ack" && x.geometryVersion === c.geometry), frozen));
  await resultAck(await command("resize", { width: 960, height: 720 }));
  await screenshot(live, "results-resumed");
  const retired = await command("retire"); await delay(100);
  assert(await renderer.evaluate(c => __motionPort.records.slice(c.before).some(x => x.kind === "control-ack" && x.operation === "retire"), retired));
  const staleAfterRetire = await command("results-motion", { node: nodes[1], transforms: [0, 0, 1, 1, 1, 0, 0, 1, 1, 1], durationMs: 0, easing: 0 }); await delay(100);
  assert.equal(await renderer.evaluate(c => __motionPort.records.slice(c.before).filter(x => x.kind === "control-ack").length, staleAfterRetire), 0);
  evidence.steps.push({ name: "genuine-results-motion-lifecycle", frames, stalePairRefused: true, retiredRefused: true });
  evidence.resultsRecords = await renderer.evaluate(() => __motionPort.records);
  assert(!evidence.resultsRecords.some(x => x.kind === "render-error"));
  await live.evaluate(() => { for (const w of __motion.workers) w.postMessage({ kind: "dispose" }); });
  await live.waitForFunction(() => __motion.messages.some(x => x.kind === "disposed"));
  await menu.evaluate(() => worker.postMessage({ kind: "dispose" }));
  await menu.waitForFunction(() => messages.some(x => x.kind === "disposed"));
  evidence.menuMessages = await messages();
  assert(!evidence.menuMessages.some(x => /error/.test(x.kind)));
  assert.equal(evidence.errors.length, 0);
  await menu.evaluate(() => { owner.dispose(); owner.free(); worker.terminate(); port.close(); });
  evidence.status = "PASS";
} catch (error) {
  evidence.status = "FAIL"; evidence.failure = String(error.stack ?? error); process.exitCode = 1;
} finally {
  if (browser) await bounded(browser.close(), 6000, "browser close").catch(e => { evidence.cleanup.browserCloseError = String(e); });
  await stop(browser?.process(), "browser"); await stop(xvfb, "xvfb");
  server.closeAllConnections(); await bounded(new Promise(r => server.close(r)), 3000, "server close");
  if (profile) await rm(profile, { recursive: true, force: true });
  if (cacheDir) await rm(cacheDir, { recursive: true, force: true });
  evidence.cleanup.server = "closed"; evidence.cleanup.ownedDirectories = "removed";
  await writeFile(resolve(out, "evidence.json"), JSON.stringify(json(evidence), null, 2));
  console.log("FINAL", evidence.status, evidence.failure ?? "", out);
}
