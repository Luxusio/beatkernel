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
    model = null; frees = 0; retired = 0; appliedPage = 0; identity = null; generationFloor = 0n;
    import_visual_registration(bytes, maxPacketBytes, maxDiagnosticBytes, mode) {
      // Only this mock field represents a roster; it is not a wire codec.
      // The real binding tests below use captured complete native packets.
      const kind = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint16(6, true);
      const rosterCount = bytes[40];
      calls.push(["registration", mode, rosterCount]);
      if (kind !== 1 || ![0, 1, 2, 3].includes(mode)) throw new Error("invalid registration mode or kind");
      if (mode === 0 ? rosterCount !== 0 : mode === 2 ? rosterCount < 1 || rosterCount > 64 : rosterCount !== 1) {
        throw new Error("incompatible visual roster and mode");
      }
      return this.commit(bytes, maxPacketBytes, maxDiagnosticBytes, mode);
    }
    import_visual_packet(bytes, maxPacketBytes, maxDiagnosticBytes) {
      return this.commit(bytes, maxPacketBytes, maxDiagnosticBytes);
    }
    commit(bytes, maxPacketBytes, maxDiagnosticBytes, mode) {
      calls.push(["import", bytes[6], maxPacketBytes, maxDiagnosticBytes]);
      assert.ok(bytes instanceof Uint8Array);
      // The final malformed field is refused by the actual importer boundary.
      // Commit only after complete validation, matching its atomic contract.
      if (bytes.at(-1) === 255 || options.importError) throw new Error("malformed final member/page");
      const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      const generation = v.getBigUint64(8, true), content = v.getBigUint64(16, true);
      const packetKind = v.getUint16(6, true), combined = packetKind === 6 && this.model?.kind === 5
        && this.identity?.generation === generation && this.identity?.content === content;
      this.model = { ...(combined ? this.model : {}), kind: combined ? 5 : packetKind,
        sequence: v.getBigUint64(24, true), final: bytes.at(-1),
        local: mode === undefined ? this.model?.local ?? false : mode === 2 };
      if (packetKind === 5) { this.model.page = 0; this.model.comparisons = false; }
      this.identity = { generation, content, sequence: this.model.sequence };
      this.generationFloor = this.generationFloor > generation ? this.generationFloor : generation;
      if (packetKind === 2) this.appliedPage = bytes[40];
      else if (packetKind !== 6) this.appliedPage = 0;
      return this.model.sequence;
    }
    set_visual_local(local) { calls.push(["local", local]); }
    set_visual_page(page, comparisons) {
      calls.push(["page", page, comparisons]);
      if (options.pageError) throw new Error(options.pageError);
      this.appliedPage = options.appliedPage ?? page;
      this.model = { ...this.model, page: this.appliedPage, comparisons };
    }
    set_visual_room_page(page) { calls.push(["room-page", page]); }
    visual_page() { calls.push(["visual-page", this.appliedPage]); return this.appliedPage; }
    resize(width, height) { calls.push(["resize", width, height]); }
    draw_visual() { calls.push(["draw", this.model?.kind, this.appliedPage, this.model?.comparisons]); if (options.drawError) throw new Error(options.drawError); }
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
  kind: "packet", operationId, generation, content,
  packet: packet(kind, sequence, generation, content, body ?? (kind === 1 ? [mode === "preview" ? 0 : 1] : [0])), mode,
});
const ofKind = (h, kind) => h.messages.filter(message => message.kind === kind);

for (const { mode, roster } of [
  { mode: "live", roster: 2 }, { mode: "replay", roster: 2 },
  { mode: "local", roster: 0 }, { mode: "preview", roster: 1 },
]) test(`${mode} incompatible roster refuses before presentation, identity or floor publication`, async () => {
  const h = await rendererHarness();
  try {
    await h.init(); await h.send(request(1n, 1, 0n, "preview")); await h.draw();
    const view = h.views[0], model = view.model, identity = view.identity, floor = view.generationFloor;
    await h.send(request(2n, 1, 0n, mode, 20n, 21n, [roster]));
    assert.equal(view.model, model); assert.equal(view.identity, identity); assert.equal(view.generationFloor, floor);
    assert.equal(ofKind(h, "state-ack").filter(message => message.operationId === 2n).length, 0);
    assert.ok(ofKind(h, "render-error").some(message => message.operationId === 2n && /roster.*mode/.test(message.message)));
    assert.equal(h.calls.filter(call => call[0] === "local").length, 0);
    assert.equal(h.calls.filter(call => call[0] === "import").length, 1, "refusal occurs before commit");
  } finally { await h.close(); }
});

test("one-member canonical P1 LOCAL is published with local rendering enabled", async () => {
  const h = await rendererHarness();
  try {
    await h.init(); await h.send(request(1n, 1, 0n, "local", 7n, 9n, [1]));
    assert.equal(h.views[0].model.local, true);
    assert.deepEqual(h.calls.filter(call => call[0] === "registration"), [["registration", 2, 1]]);
    assert.equal(ofKind(h, "state-ack").length, 1);
    assert.equal(h.calls.filter(call => call[0] === "local").length, 0);
  } finally { await h.close(); }
});

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
    for (const kind of [4, 5, 6]) {
      const generation = 30n + id + 1n;
      await h.send(request(++id, kind, 0n, undefined, generation));
      if (kind === 5) await h.send({ kind: "page", operationId: ++id, generation, content: 9n,
        page: 0, comparisons: false, geometryVersion: 1n });
      await h.draw();
    }
    // A room packet on the Results identity supplies the combined footer model.
    await h.send(request(++id, 5, 0n, undefined, 60n));
    await h.send(request(++id, 6, 0n, undefined, 60n));
    await h.send({ kind: "page", operationId: ++id, generation: 60n, content: 9n,
      page: 1, comparisons: true, geometryVersion: 2n }); await h.draw();
    assert.equal(ofKind(h, "state-ack").length, 13);
    assert.ok(h.calls.some(call => call[0] === "registration" && call[1] === 2 && call[2] === 1), "one-member LOCAL must be staged with explicit mode");
    assert.equal(h.calls.filter(call => call[0] === "local").length, 0, "mode admission must not call a fallible postcommit setter");
    for (const kind of [2, 3, 4, 5, 6]) assert.ok(h.calls.some(call => call[0] === "draw" && call[1] === kind));
    assert.equal(h.calls.filter(call => call[0] === "import").length, 13);
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

for (const comparisons of [false, true]) test(`cold Results waits for primary ${comparisons ? "comparison" : "detail"} selection before first submission`, async () => {
  const h = await rendererHarness();
  try {
    await h.init(); await h.send(request(1n, 5, 0n));
    await h.draw();
    assert.equal(ofKind(h, "state-ack").length, 1);
    assert.equal(h.calls.filter(call => call[0] === "draw").length, 0);
    await h.send({ kind: "resize", operationId: 2n, generation: 7n, content: 9n,
      width: 900, height: 700, geometryVersion: 1n }); await h.draw();
    await h.send(request(3n, 6, 0n)); await h.draw();
    await h.send({ kind: "room-page", operationId: 4n, generation: 7n, content: 9n,
      page: 2, geometryVersion: 2n }); await h.draw();
    assert.equal(ofKind(h, "state-ack").length, 2, "state import remains independent of selection submission");
    assert.equal(h.calls.filter(call => call[0] === "draw").length, 0, "resize or combined footer must not unlock cold Results");
    assert.equal(ofKind(h, "geometry-ack").length, 0);
    // A stale primary selection cannot unlock either.
    await h.send({ kind: "page", operationId: 5n, generation: 6n, content: 9n,
      page: 0, comparisons: false, geometryVersion: 3n }); await h.draw();
    assert.equal(h.calls.filter(call => call[0] === "draw").length, 0);
    const page = comparisons ? 2 : 0;
    await h.send({ kind: "page", operationId: 6n, generation: 7n, content: 9n,
      page, comparisons, geometryVersion: 4n });
    assert.ok(ofKind(h, "control-ack").some(message => message.operationId === 6n));
    assert.equal(ofKind(h, "geometry-ack").length, 0);
    await h.draw();
    assert.deepEqual(h.calls.filter(call => call[0] === "draw"), [["draw", 5, page, comparisons]],
      "first actual draw must use the validated current primary selection");
    assert.deepEqual(ofKind(h, "geometry-ack"), [{ kind: "geometry-ack", generation: 7n, content: 9n,
      geometryVersion: 4n, page, width: 900, height: 700 }]);
  } finally { await h.close(); }
});

test("refused primary Results page cannot unlock first draw or geometry", async () => {
  const h = await rendererHarness({ pageError: "invalid visual Results page/mode" });
  try {
    await h.init(); await h.send(request(1n, 5, 0n));
    await h.send({ kind: "page", operationId: 2n, generation: 7n, content: 9n,
      page: 99, comparisons: true, geometryVersion: 1n }); await h.draw();
    assert.equal(h.views[0].appliedPage, 0); assert.equal(h.views[0].model.comparisons, false);
    assert.equal(h.calls.filter(call => call[0] === "draw").length, 0);
    assert.equal(ofKind(h, "geometry-ack").length, 0);
    assert.equal(ofKind(h, "control-ack").filter(message => message.operationId === 2n).length, 0);
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
    assert.equal(geometry[0].page, 0); assert.equal(geometry[0].width, 800); assert.equal(geometry[0].height, 600);
    assert.ok(h.calls.some(call => call[0] === "draw"));
  } finally { await h.close(); }
});

test("geometry reports the applied static page and admitted backing extent after successful draw", async () => {
  const h = await rendererHarness({ appliedPage: 2 });
  try {
    await h.init(); await h.send(request(1n, 5, 0n));
    await h.send({ kind: "page", operationId: 2n, generation: 7n, content: 9n,
      page: 99, comparisons: true, geometryVersion: 10n });
    await h.send({ kind: "resize", operationId: 3n, generation: 7n, content: 9n,
      width: 913, height: 517, geometryVersion: 11n });
    assert.equal(ofKind(h, "geometry-ack").length, 0);
    await h.draw();
    assert.deepEqual(ofKind(h, "geometry-ack"), [{ kind: "geometry-ack", generation: 7n,
      content: 9n, geometryVersion: 11n, page: 2, width: 913, height: 517 }]);
    const drawIndex = h.calls.findIndex(call => call[0] === "draw");
    const pageIndex = h.calls.findIndex(call => call[0] === "visual-page");
    assert.ok(pageIndex > drawIndex, "evidence samples validated page after draw, never requested page 99");
    await h.draw(); assert.equal(ofKind(h, "geometry-ack").length, 1);
    await h.send(request(4n, 6, 0n, undefined));
    await h.send({ kind: "room-page", operationId: 5n, generation: 7n, content: 9n,
      page: 7, geometryVersion: 12n });
    await h.draw();
    assert.equal(ofKind(h, "geometry-ack").at(-1).page, 2, "combined footer page is not outer selected page");
    assert.equal(ofKind(h, "geometry-ack").at(-1).geometryVersion, 12n);
  } finally { await h.close(); }
});

test("live packet submission derives page from committed frame and retains actual resized extent", async () => {
  const h = await rendererHarness();
  try {
    await h.init(); await h.send(request(1n, 1, 0n, "local"));
    await h.send({ kind: "resize", operationId: 2n, generation: 7n, content: 9n,
      width: 1024, height: 768, geometryVersion: 5n });
    await h.draw(); assert.equal(ofKind(h, "geometry-ack").length, 0, "registration is not a drawable live frame");
    await h.send({ ...request(3n, 2, 1n, "local", 7n, 9n, [3]), geometryVersion: 6n });
    assert.equal(ofKind(h, "geometry-ack").length, 0);
    await h.draw();
    assert.deepEqual(ofKind(h, "geometry-ack"), [{ kind: "geometry-ack", generation: 7n,
      content: 9n, geometryVersion: 6n, page: 3, width: 1024, height: 768 }]);
    await h.send({ ...request(4n, 2, 2n, "local", 7n, 9n, [1]), geometryVersion: 7n });
    await h.draw(); assert.equal(ofKind(h, "geometry-ack").at(-1).page, 1);
    assert.equal(ofKind(h, "geometry-ack").at(-1).width, 1024);
  } finally { await h.close(); }
});

test("replacement before pending draw reports only replacement owner and committed page", async () => {
  const h = await rendererHarness();
  try {
    await h.init(); await h.send(request(1n, 1, 0n, "local"));
    await h.send({ ...request(2n, 2, 1n, "local", 7n, 9n, [3]), geometryVersion: 1n });
    await h.send(request(3n, 1, 0n, "local", 8n, 10n));
    await h.send({ ...request(4n, 2, 1n, "local", 8n, 10n, [1]), geometryVersion: 2n });
    await h.draw();
    assert.deepEqual(ofKind(h, "geometry-ack"), [{ kind: "geometry-ack", generation: 8n,
      content: 10n, geometryVersion: 2n, page: 1, width: 640, height: 480 }]);
    const before = h.calls.length;
    await h.send({ kind: "page", operationId: 5n, generation: 7n, content: 9n,
      page: 9, comparisons: false, geometryVersion: 3n });
    await h.draw(); assert.equal(ofKind(h, "geometry-ack").length, 1);
    assert.equal(h.calls.slice(before).filter(call => call[0] === "page").length, 0);
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
    assert.equal(h.calls.filter(call => call[0] === "visual-page").length, 0);
    if (condition === "retry") {
      await h.draw(); await h.draw(); await h.draw();
      assert.equal(ofKind(h, "geometry-ack").length, 0);
      assert.ok(ofKind(h, "render-wait").length > 0);
      h.options.needsRedraw = false;
      await h.send({ kind: "resize", operationId: 3n, generation: 7n, content: 9n,
        width: 801, height: 601, geometryVersion: 23n });
      await h.draw();
      assert.deepEqual(ofKind(h, "geometry-ack"), [{ kind: "geometry-ack", generation: 7n,
        content: 9n, geometryVersion: 23n, page: 0, width: 801, height: 601 }]);
    }
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
