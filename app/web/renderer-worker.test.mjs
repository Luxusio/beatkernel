// Executes the actual owner; generated WASM/GPU is mocked at its binding boundary.
// This does not prove real two-Worker WASM/GPU behavior or physical presentation.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { MessageChannel } from "node:worker_threads";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";

const turn = () => new Promise(resolve => setImmediate(resolve));
async function settle() { for (let i = 0; i < 8; i++) await turn(); }
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function packet(kind = 1, sequence = 0n, generation = 7n, content = 9n, body = [0]) {
  const bytes = new Uint8Array(40 + body.length); bytes.set([66, 75, 82, 86]);
  const view = new DataView(bytes.buffer);
  view.setUint16(4, 1, true); view.setUint16(6, kind, true);
  view.setBigUint64(8, generation, true); view.setBigUint64(16, content, true);
  view.setBigUint64(24, sequence, true); view.setBigUint64(32, BigInt(body.length), true);
  bytes.set(body, 40); return bytes;
}

async function rendererHarness(options = {}) {
  const host = [], messages = [], calls = [], views = [], timers = new Map();
  const { port1, port2 } = new MessageChannel();
  port1.on("message", message => messages.push(message));
  let timerId = 0, receive;
  const self = { postMessage: message => host.push(message),
    addEventListener(name, handler) { if (name === "message") receive = handler; }, close() { calls.push(["worker-close"]); } };
  class BrowserView {
    static async create(canvas) {
      calls.push(["create", canvas]);
      if (options.createGate) await options.createGate.promise;
      const view = new BrowserView(); views.push(view); return view;
    }
    model = null; frees = 0; retired = 0;
    import_visual_packet(bytes, maxPacketBytes, maxDiagnosticBytes) {
      calls.push(["import", bytes[6], maxPacketBytes, maxDiagnosticBytes]);
      assert.ok(bytes instanceof Uint8Array);
      // The final malformed field is refused by the actual importer boundary.
      // Commit only after complete validation, matching its atomic contract.
      if (bytes.at(-1) === 255 || options.importError) throw new Error("malformed final member/page");
      const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      this.model = { kind: v.getUint16(6, true), sequence: v.getBigUint64(24, true), final: bytes.at(-1) };
      return this.model.sequence;
    }
    set_visual_local(local) { calls.push(["local", local]); }
    set_visual_page(page, comparisons) { calls.push(["page", page, comparisons]); }
    set_visual_room_page(page) { calls.push(["room-page", page]); }
    resize(width, height) { calls.push(["resize", width, height]); }
    draw_visual() { calls.push(["draw", this.model?.kind]); if (options.drawError) throw new Error(options.drawError); }
    needs_redraw() { return options.needsRedraw ?? false; }
    retire_visual() { this.retired++; this.model = null; calls.push(["retire"]); }
    free() { this.frees++; assert.equal(this.frees, 1); calls.push(["free"]); }
  }
  const context = createContext({ self, console, Uint8Array, ArrayBuffer, DataView, BigInt, TextEncoder, Error,
    performance: { now: () => 100 },
    setTimeout(callback) { const id = ++timerId; timers.set(id, callback); return id; },
    clearTimeout(id) { timers.delete(id); },
    requestAnimationFrame(callback) { const id = ++timerId; timers.set(id, callback); return id; },
    cancelAnimationFrame(id) { timers.delete(id); },
  });
  const bindings = new SyntheticModule(["default", "BrowserView"], function () {
    this.setExport("default", async () => { calls.push(["wasm-init"]); if (options.initGate) await options.initGate.promise; });
    this.setExport("BrowserView", BrowserView);
  }, { context });
  const cache = new Map();
  async function load(url) {
    if (cache.has(url.href)) return cache.get(url.href);
    const module = new SourceTextModule(await readFile(url, "utf8"), { context, identifier: url.href,
      initializeImportMeta(meta) { meta.url = url.href; } });
    cache.set(url.href, module);
    await module.link(async (specifier, parent) => {
      if (/pkg|wasm/.test(specifier)) return bindings;
      return load(new URL(specifier, parent.identifier));
    });
    return module;
  }
  const source = await load(new URL("./renderer-worker.js", import.meta.url));
  await source.evaluate();
  const sendHost = async data => { (receive ?? self.onmessage)({ data }); await settle(); };
  const h = { host, messages, calls, views, options, timers,
    sendHost,
    async init() { await sendHost({ kind: "init", canvas: { width: 640, height: 480 }, port: port2,
      maxPacketBytes: 4096, maxDiagnosticBytes: 1024 }); },
    async send(data) { port1.postMessage(data); await settle(); },
    async draw() { const scheduled = [...timers.values()]; timers.clear(); for (const callback of scheduled) callback(100); await settle(); },
    async close() { await sendHost({ kind: "dispose" }); port1.close(); port2.close(); },
  };
  return h;
}
const request = (operationId, kind = 1, sequence = 0n, mode = "live", generation = 7n, content = 9n, body) => ({
  kind: "packet", operationId, generation, content, packet: packet(kind, sequence, generation, content, body), mode,
});
const ofKind = (h, kind) => h.messages.filter(message => message.kind === kind);

test("delayed WASM initialization cannot create or revive a disposed renderer", async () => {
  const gate = deferred(), h = await rendererHarness({ initGate: gate });
  try {
    await h.init(); assert.equal(h.views.length, 0);
    await h.sendHost({ kind: "dispose" });
    gate.resolve(); await settle();
    assert.equal(h.views.length, 0); assert.equal(h.host.some(m => m.kind === "ready"), false);
  } finally { await h.close(); }
});

test("delayed canvas creation releases a cancelled view exactly once", async () => {
  const gate = deferred(), h = await rendererHarness({ createGate: gate });
  try {
    await h.init(); await h.sendHost({ kind: "dispose" }); gate.resolve(); await settle();
    assert.equal(h.views.length, 1); assert.equal(h.views[0].frees, 1);
    assert.equal(h.host.some(m => m.kind === "ready"), false);
  } finally { await h.close(); }
});

test("all visual modes import actual bytes and draw through the visual-only binding", async () => {
  const h = await rendererHarness();
  try {
    await h.init();
    let id = 0n;
    for (const mode of ["preview", "live", "local", "replay"]) {
      const generation = 7n + id;
      await h.send(request(++id, 1, 0n, mode, generation));
      await h.send(request(++id, mode === "preview" ? 3 : 2, 1n, mode, generation));
      await h.draw();
    }
    for (const kind of [4, 5, 6]) { await h.send(request(++id, kind, 0n, undefined, 30n + id)); await h.draw(); }
    // A room packet on the Results identity supplies the combined footer model.
    await h.send(request(++id, 5, 0n, undefined, 60n));
    await h.send(request(++id, 6, 0n, undefined, 60n)); await h.draw();
    assert.equal(ofKind(h, "state-ack").length, Number(id));
    assert.ok(h.calls.some(call => call[0] === "local" && call[1] === true), "one-member LOCAL must use explicit mode hint");
    for (const kind of [2, 3, 4, 5, 6]) assert.ok(h.calls.some(call => call[0] === "draw" && call[1] === kind));
    assert.equal(h.calls.filter(call => call[0] === "import").length, Number(id));
    assert.ok(h.calls.filter(call => call[0] === "import").every(call => call[2] === 4096 && call[3] === 1024));
    await h.send({ kind: "room-page", operationId: ++id, generation: 60n, content: 9n, page: 2, geometryVersion: 3n });
    assert.ok(h.calls.some(call => call[0] === "room-page" && call[1] === 2));
  } finally { await h.close(); }
});

test("malformed final body refuses ACK and preserves previously imported presentation", async () => {
  const h = await rendererHarness();
  try {
    await h.init(); await h.send(request(1n)); const model = h.views[0].model;
    await h.send(request(2n, 2, 1n, "live", 7n, 9n, [0, 255]));
    assert.equal(h.views[0].model, model);
    assert.deepEqual(ofKind(h, "state-ack").map(message => message.operationId), [1n]);
    assert.ok(ofKind(h, "render-error").some(message => message.operationId === 2n));
  } finally { await h.close(); }
});

test("state ACK is importer evidence; geometry ACK follows successful requested-version submission", async () => {
  const h = await rendererHarness();
  try {
    await h.init(); await h.send(request(1n, 1, 0n, "preview"));
    assert.equal(ofKind(h, "state-ack").length, 1);
    await h.send({ kind: "resize", operationId: 2n, generation: 7n, content: 9n, width: 800, height: 600, geometryVersion: 12n });
    assert.ok(h.calls.some(call => call[0] === "resize" && call[1] === 800 && call[2] === 600));
    assert.equal(ofKind(h, "geometry-ack").filter(message => message.geometryVersion === 12n).length, 0);
    await h.draw();
    const geometry = ofKind(h, "geometry-ack").filter(message => message.geometryVersion === 12n);
    assert.equal(geometry.length, 1); assert.equal(geometry[0].generation, 7n); assert.equal(geometry[0].content, 9n);
    assert.ok(h.calls.some(call => call[0] === "draw"));
  } finally { await h.close(); }
});

for (const condition of ["zero", "retry", "fault"]) test(`${condition} surface cannot fabricate geometry success`, async () => {
  const h = await rendererHarness(condition === "retry" ? { needsRedraw: true } : condition === "fault" ? { drawError: "GPU terminal fault" } : {});
  try {
    await h.init(); await h.send(request(1n, 1, 0n, "preview"));
    await h.send({ kind: "resize", operationId: 2n, generation: 7n, content: 9n,
      width: condition === "zero" ? 0 : 640, height: 480, geometryVersion: 22n });
    await h.draw();
    assert.equal(ofKind(h, "geometry-ack").filter(message => message.geometryVersion === 22n).length, 0);
    assert.equal(ofKind(h, "state-ack").length, 1);
    if (condition === "fault") assert.ok(ofKind(h, "render-error").length > 0);
    if (condition === "zero") assert.equal(h.calls.filter(call => call[0] === "draw").length, 0);
  } finally { await h.close(); }
});

test("retirement fences old packets, resize/page callbacks and releases resources once", async () => {
  const h = await rendererHarness();
  try {
    await h.init(); await h.send(request(1n, 5, 0n));
    await h.send({ kind: "page", operationId: 2n, generation: 7n, content: 9n, page: 1, comparisons: true, geometryVersion: 2n });
    assert.ok(h.calls.some(call => call[0] === "page" && call[1] === 1 && call[2] === true));
    await h.send({ kind: "retire", operationId: 3n, generation: 7n, content: 9n });
    const count = h.calls.length;
    await h.send(request(4n, 5, 0n));
    await h.send({ kind: "resize", operationId: 5n, generation: 7n, content: 9n, width: 900, height: 700, geometryVersion: 3n });
    await h.send({ kind: "page", operationId: 6n, generation: 7n, content: 9n, page: 0, comparisons: false, geometryVersion: 4n });
    await h.draw();
    assert.equal(h.calls.slice(count).filter(call => ["import", "resize", "page", "draw"].includes(call[0])).length, 0);
    assert.equal(ofKind(h, "state-ack").filter(message => message.operationId === 4n).length, 0);
    await h.sendHost({ kind: "dispose" }); await h.sendHost({ kind: "dispose" });
    assert.equal(h.views[0].frees, 1);
  } finally { await h.close(); }
});
