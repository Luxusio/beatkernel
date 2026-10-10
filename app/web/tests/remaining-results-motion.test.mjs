// Executes production Worker and RenderClient. Only generated WASM/GPU bindings
// are mocked: these tests do not establish real GPU or physical presentation.
import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { MessageChannel } from "node:worker_threads";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";
import { RenderClient, preflightPacket } from "../render-protocol.mjs";

const settle = async () => { for (let i = 0; i < 8; i++) await new Promise(resolve => setImmediate(resolve)); };
function bytes(kind = 5, generation = 7n, content = 9n) {
  const b = new Uint8Array(41); b.set([66, 75, 82, 86]); const v = new DataView(b.buffer);
  v.setUint16(4, 1, true); v.setUint16(6, kind, true);
  v.setBigUint64(8, generation, true); v.setBigUint64(16, content, true);
  v.setBigUint64(32, 1n, true); return b;
}
const endpoints = () => new Float32Array([0, 0, 1, 1, 1, 40, 10, 1.25, 0.75, 0.5]);
const request = (fields = {}) => ({ kind: "results-motion", generation: 7n, content: 9n,
  operationId: 3n, geometryVersion: 3n, node: 2, transforms: endpoints(), durationMs: 1000, easing: 0, ...fields });
async function worker(options = {}) {
  const calls = [], messages = [], host = [], timers = new Map();
  const { port1, port2 } = new MessageChannel(); port1.on("message", m => messages.push(m));
  let receive, serial = 0, now = 100;
  const self = { addEventListener: (_, f) => { receive = f; }, postMessage: m => host.push(m), close() {},
    requestAnimationFrame: f => { const id = ++serial; timers.set(id, f); return id; },
    cancelAnimationFrame: id => timers.delete(id) };
  class BrowserView {
    static async create() { return new BrowserView(); }
    page = 0; motion = null; pair = null; mode = null;
    import_visual_packet(b) {
      const h = preflightPacket(b, 4096); calls.push(["import", h.kind, h.generation, h.content]);
      const combined = h.kind === 6 && this.mode === "results" && this.pair?.[0] === h.generation && this.pair?.[1] === h.content;
      if (!combined) { this.motion = null; this.page = 0; this.mode = h.kind === 5 ? "results" : "room"; }
      this.pair = [h.generation, h.content]; return h.sequence;
    }
    import_visual_registration(b, _p, _d, mode) { const n = this.import_visual_packet(b); this.mode = mode === 0 ? "preview" : "live"; return n; }
    request_results_motion(g, c, node, values, duration, easing, at) {
      calls.push(["request", g, c, node, [...values], duration, easing, at]);
      if (options.refuse || node === 900 || (this.page === 1 && node === 2)) throw new Error("unavailable Results node");
      for (const at of [0, 5]) {
        if (Math.abs(values[at]) > 16777216 || Math.abs(values[at + 1]) > 16777216
          || values[at + 2] < 1 / 16 || values[at + 2] > 16 || values[at + 3] < 1 / 16 || values[at + 3] > 16
          || values[at + 4] < 0 || values[at + 4] > 1) throw new Error("invalid Results transform bounds");
      }
      this.motion = { node, start: at, duration, paused: null };
    }
    draw_visual_at(at) {
      calls.push(["timed-draw", at]);
      if (this.motion && this.motion.paused === null && at >= this.motion.start + this.motion.duration) this.motion = null;
      return !options.unpresented;
    }
    results_motion_active() { return !!this.motion && this.motion.paused === null; }
    suspend_results_motion(at) { calls.push(["suspend", at]); if (this.motion?.paused === null) this.motion.paused = at; }
    resume_results_motion(at) { calls.push(["resume", at]); if (this.motion && this.motion.paused !== null) { this.motion.start += at - this.motion.paused; this.motion.paused = null; } }
    dispose_results_motion() { calls.push(["dispose-results"]); this.motion = null; }
    dispose_menu_motion() { calls.push(["dispose-menu"]); }
    set_visual_page(page, comparisons) { calls.push(["page", page, comparisons]); this.page = page; if (page === 1 && this.motion?.node === 2) this.motion = null; }
    set_visual_room_page(page) { calls.push(["room-page", page]); }
    visual_page() { return this.page; }
    resize(w, h) { calls.push(["resize", w, h]); }
    needs_redraw() { return !!options.unpresented; }
    draw_visual() { calls.push(["ordinary-draw"]); }
    retire_visual() { calls.push(["retire"]); this.motion = null; }
    free() { calls.push(["free"]); }
  }
  const context = createContext({ self, console, Uint8Array, Float32Array, ArrayBuffer, DataView, BigInt, TextEncoder, Error,
    performance: { now: () => now }, setTimeout: f => { const id = ++serial; timers.set(id, f); return id; }, clearTimeout: id => timers.delete(id) });
  const bindings = new SyntheticModule(["default", "BrowserView"], function () { this.setExport("default", async () => {}); this.setExport("BrowserView", BrowserView); }, { context });
  const cache = new Map();
  async function load(url) {
    if (cache.has(url.href)) return cache.get(url.href);
    const module = new SourceTextModule(await readFile(url, "utf8"), { context, identifier: url.href, initializeImportMeta: meta => { meta.url = url.href; } });
    cache.set(url.href, module); await module.link((name, parent) => /pkg|wasm/.test(name) ? bindings : load(new URL(name, parent.identifier))); return module;
  }
  await (await load(new URL("../renderer-worker.js", import.meta.url))).evaluate();
  const h = { calls, messages, host, timers,
    async send(m) { port1.postMessage(m); await settle(); },
    async draw(at) { now = at; const callbacks = [...timers.values()]; timers.clear(); for (const f of callbacks) f(at); await settle(); },
    async close() { (receive ?? self.onmessage)({ data: { kind: "dispose" } }); await settle(); port1.close(); port2.close(); },
    async register(g = 7n, c = 9n, op = 1n, version = 1n) {
      await this.send({ kind: "packet", operationId: op, generation: g, content: c, geometryVersion: version, packet: bytes(5, g, c) });
      await this.send({ kind: "page", operationId: op + 1n, generation: g, content: c, geometryVersion: version + 1n, page: 0, comparisons: false });
    },
  };
  (receive ?? self.onmessage)({ data: { kind: "init", canvas: { width: 960, height: 720 }, port: port2, maxPacketBytes: 4096, maxDiagnosticBytes: 1024 } }); await settle();
  return h;
}

test("actual Results Worker uses timed RAF until motion completes", async () => {
  const h = await worker(); try {
    await h.register(); await h.send(request());
    assert.deepEqual(h.calls.find(c => c[0] === "request"), ["request", 7n, 9n, 2, [...endpoints()], 1000, 0, 100]);
    for (const at of [116, 132, 148, 164, 180, 196]) { await h.draw(at); assert.equal(h.timers.size, 1); }
    assert.equal(h.calls.filter(c => c[0] === "ordinary-draw").length, 0);
    await h.draw(1100); assert.equal(h.timers.size, 0);
    assert.ok(h.messages.some(m => m.kind === "geometry-ack" && m.geometryVersion === 3n));
  } finally { await h.close(); }
});

test("same-pair Room continuation preserves Results tracks, replacement fences captured RAF", async () => {
  const h = await worker(); try {
    await h.register(); await h.send(request()); await h.draw(150);
    await h.send({ kind: "packet", operationId: 4n, generation: 7n, content: 9n, packet: bytes(6) });
    await h.draw(200); assert.equal(h.timers.size, 1);
    const stale = [...h.timers.values()][0];
    await h.register(8n, 10n, 5n, 4n);
    const before = h.calls.filter(c => c[0] === "timed-draw").length;
    stale(220); await settle(); assert.equal(h.calls.filter(c => c[0] === "timed-draw").length, before);
    await h.draw(250); assert.equal(h.timers.size, 0);
    assert.equal(h.messages.some(m => m.kind === "render-error"), false);
  } finally { await h.close(); }
});

test("zero extent pauses Results motion and recovery excludes hidden elapsed time", async () => {
  const h = await worker(); try {
    await h.register(); await h.send(request({ durationMs: 100 })); await h.draw(120);
    await h.send({ kind: "resize", generation: 7n, content: 9n, operationId: 4n, geometryVersion: 4n, width: 0, height: 0 });
    assert.equal(h.timers.size, 0); assert.ok(h.calls.some(c => c[0] === "suspend" && c[1] === 120));
    await h.draw(5000);
    await h.send({ kind: "resize", generation: 7n, content: 9n, operationId: 5n, geometryVersion: 5n, width: 960, height: 720 });
    await h.draw(5020); assert.equal(h.timers.size, 1);
    await h.draw(5080); assert.equal(h.timers.size, 0);
  } finally { await h.close(); }
});

test("foreign pair and duplicate operation never reach Results motion admission", async () => {
  const h = await worker(); try {
    await h.register();
    for (const fields of [{ generation: 8n, transforms: null }, { content: 10n, transforms: null }, { operationId: 2n, transforms: null }]) await h.send(request(fields));
    assert.equal(h.calls.some(c => c[0] === "request"), false);
    assert.equal(h.messages.some(m => m.kind === "render-error"), false);
    await h.send(request()); await h.send(request({ geometryVersion: 4n }));
    assert.equal(h.calls.filter(c => c[0] === "request").length, 1);
  } finally { await h.close(); }
});

test("Results-only control rejects other presentation modes before payload admission", async () => {
  const h = await worker(); try {
    await h.send({ kind: "packet", operationId: 1n, generation: 7n, content: 9n, packet: bytes(6) });
    await h.send(request({ transforms: null }));
    assert.equal(h.calls.some(c => c[0] === "request"), false);
    assert.equal(h.messages.some(m => m.kind === "render-error"), false);
  } finally { await h.close(); }
});

test("active Results motion cannot bypass failed-presentation retry limits", async () => {
  const h = await worker({ unpresented: true }); try {
    await h.register(); await h.send(request());
    for (const at of [116, 132, 148, 164, 180, 196]) await h.draw(at);
    assert.equal(h.timers.size, 0);
    assert.equal(h.messages.some(m => m.kind === "drawn"), false);
    assert.ok(h.messages.some(m => m.kind === "render-wait"));
  } finally { await h.close(); }
});

for (const [name, fields] of [
  ["negative node", { node: -1 }], ["out-of-range node", { node: 1024 }], ["BigInt node", { node: 2n }],
  ["short endpoints", { transforms: new Float32Array(9) }], ["wrong typed array", { transforms: new Float64Array(10) }],
  ["nonfinite endpoints", { transforms: new Float32Array([NaN, 0, 1, 1, 1, 0, 0, 1, 1, 1]) }],
  ["negative duration", { durationMs: -1 }], ["unknown easing", { easing: 4 }],
  ["nonincreasing geometry version", { geometryVersion: 2n }],
]) test(`Results Worker refuses ${name} before binding and ACK`, async () => {
  const h = await worker(); try {
    await h.register(); await h.send(request(fields));
    assert.equal(h.calls.some(c => c[0] === "request"), false);
    assert.equal(h.messages.some(m => m.kind === "control-ack" && m.operationId === 3n), false);
    assert.equal(h.messages.some(m => m.kind === "control-reject" && m.operationId === 3n && m.operation === "results-motion"), true);
    assert.equal(h.messages.some(m => m.kind === "render-error"), false);
  } finally { await h.close(); }
});

test("binding refusal never acknowledges new geometry; page pruning rejects absent target", async () => {
  const h = await worker(); try {
    await h.register(); await h.send(request()); await h.draw(120);
    await h.send({ kind: "page", generation: 7n, content: 9n, operationId: 4n, geometryVersion: 4n, page: 1, comparisons: false });
    await h.draw(140); assert.equal(h.timers.size, 0);
    await h.send(request({ operationId: 5n, geometryVersion: 5n }));
    assert.equal(h.messages.some(m => m.kind === "control-ack" && m.operationId === 5n), false);
    assert.equal(h.messages.some(m => m.kind === "geometry-ack" && m.geometryVersion === 5n), false);
    assert.equal(h.messages.some(m => m.kind === "control-reject" && m.operationId === 5n && m.operation === "results-motion"), true);
    assert.equal(h.messages.some(m => m.kind === "render-error"), false);
    await h.send(request({ operationId: 6n, geometryVersion: 6n, node: 3 }));
    assert.equal(h.messages.some(m => m.kind === "control-ack" && m.operationId === 6n), true);
    await h.draw(160);
    assert.equal(h.messages.some(m => m.kind === "geometry-ack" && m.geometryVersion === 6n), true);
  } finally { await h.close(); }
});

for (const [name, fields] of [
  ["unavailable mounted card", { node: 900 }],
  ["finite negative scale", { transforms: new Float32Array([0, 0, 1, 1, 1, 40, 10, -1, 1, 1]) }],
  ["finite excessive opacity", { transforms: new Float32Array([0, 0, 1, 1, 1, 40, 10, 1, 1, 1.5]) }],
  ["finite excessive offset", { transforms: new Float32Array([0, 0, 1, 1, 1, 16777218, 10, 1, 1, 1]) }],
]) test(`Results ${name} rejects locally while prior animation and accepted geometry survive`, async () => {
  const h = await worker(); try {
    await h.register(); await h.send(request()); await h.draw(120);
    const initialFrames = h.messages.filter(m => m.kind === "drawn").length;
    await h.send(request({ ...fields, operationId: 4n, geometryVersion: 4n }));
    const rejection = h.messages.find(m => m.kind === "control-reject" && m.operationId === 4n);
    assert.ok(rejection); assert.equal(rejection.operation, "results-motion");
    assert.equal(rejection.generation, 7n); assert.equal(rejection.content, 9n); assert.equal(rejection.geometryVersion, 4n);
    assert.equal(h.messages.some(m => m.kind === "render-error"), false);
    assert.equal(h.messages.some(m => m.kind === "control-ack" && m.operationId === 4n), false);
    await h.draw(140);
    assert.ok(h.messages.filter(m => m.kind === "drawn").length > initialFrames);
    assert.equal(h.timers.size, 1, "previous accepted animation keeps its RAF successor");
    assert.equal(h.messages.some(m => m.kind === "geometry-ack" && m.geometryVersion === 4n), false);
    await h.send(request({ operationId: 5n, geometryVersion: 5n, node: 3, durationMs: 100 }));
    assert.equal(h.messages.some(m => m.kind === "control-ack" && m.operationId === 5n), true);
    await h.draw(160);
    assert.equal(h.messages.some(m => m.kind === "geometry-ack" && m.geometryVersion === 5n), true);
    await h.draw(240); assert.equal(h.timers.size, 0);
  } finally { await h.close(); }
});

test("RenderClient Results rejection settles only matching operation and remains reusable", async () => {
  const sent = [], errors = []; const port = { postMessage: m => sent.push(m), start() {}, close() {} };
  const reply = data => port.onmessage({ data });
  const client = new RenderClient({ port, generation: 7n, content: 9n, maxPacketBytes: 4096,
    maxDiagnosticBytes: 1024, timeoutMs: 1000, onError: e => errors.push(e) });
  try {
    const registration = client.packet(bytes());
    reply({ kind: "state-ack", generation: 7n, content: 9n, operationId: 1n, sequence: 0n, packetKind: 5 }); await registration;
    const fields = { node: 900, transforms: endpoints(), durationMs: 1000, easing: 0, geometryVersion: 1n };
    const pending = client.control("results-motion", fields);
    const rejection = { kind: "control-reject", operation: "results-motion", operationId: 2n,
      generation: 7n, content: 9n, geometryVersion: 1n, message: "Results node is not displayed" };
    for (const wrong of [{ generation: 8n }, { content: 10n }, { operationId: 3n }, { operation: "menu-motion" }, { geometryVersion: 2n }]) {
      reply({ ...rejection, ...wrong }); assert.equal(client.pending, true);
    }
    reply(rejection); await assert.rejects(pending, /not displayed/);
    assert.equal(client.state, "ready"); assert.equal(client.failure, null); assert.deepEqual(errors, []);
    const next = client.control("results-motion", { ...fields, node: 2, geometryVersion: 2n });
    reply(rejection); assert.equal(client.pending, true);
    reply({ kind: "control-ack", operation: "results-motion", operationId: 3n, generation: 7n, content: 9n, geometryVersion: 2n }); await next;
    assert.equal(client.state, "ready"); assert.deepEqual(errors, []);
  } finally { client.close(); }
});

for (const action of ["retire", "dispose"]) test(`${action} stops Results RAF and fences captured callbacks`, async () => {
  const h = await worker(); try {
    await h.register(); await h.send(request()); const stale = [...h.timers.values()][0];
    if (action === "retire") await h.send({ kind: "retire", operationId: 4n, generation: 7n, content: 9n });
    else await h.close();
    assert.equal(h.timers.size, 0);
    assert.ok(h.calls.some(c => c[0] === "dispose-results"));
    const n = h.calls.length; stale(150); await settle(); assert.equal(h.calls.length, n);
  } finally { if (action !== "dispose") await h.close(); }
});

test("actual RenderClient copies Results endpoint payload and matches exact ACK", async () => {
  // Keep the exact postMessage argument: MessageChannel's own structured clone
  // would otherwise hide a missing client admission copy.
  const sent = []; const port = { postMessage: m => sent.push(m), start() {}, close() {} };
  const reply = m => port.onmessage({ data: m });
  const client = new RenderClient({ port, generation: 7n, content: 9n, maxPacketBytes: 4096, maxDiagnosticBytes: 1024, timeoutMs: 1000 });
  try {
    const registration = client.packet(bytes()); await settle();
    reply({ kind: "state-ack", generation: 7n, content: 9n, operationId: sent[0].operationId, sequence: 0n, packetKind: 5 }); await registration;
    const transform = endpoints(); const expected = [...transform];
    const operation = client.control("results-motion", { node: 2, transforms: transform, durationMs: 1000, easing: 0, geometryVersion: 1n });
    transform.fill(999); await settle();
    const m = sent.at(-1); assert.equal(m.kind, "results-motion"); assert.equal(m.node, 2); assert.deepEqual([...m.transforms], expected);
    const good = { kind: "control-ack", operation: "results-motion", generation: 7n, content: 9n, operationId: m.operationId, geometryVersion: 1n };
    for (const wrong of [{ content: 10n }, { generation: 8n }, { geometryVersion: 2n }, { operation: "menu-motion" }]) { reply({ ...good, ...wrong }); await settle(); assert.equal(client.pending, true); }
    reply(good); await operation; assert.equal(client.pending, false);
  } finally { client.close(); }
});

test("RenderClient invalid Results targets do not consume operation or geometry versions", async () => {
  const sent = []; const port = { postMessage: m => sent.push(m), start() {}, close() {} };
  const client = new RenderClient({ port, generation: 7n, content: 9n, maxPacketBytes: 4096, maxDiagnosticBytes: 1024, timeoutMs: 1000 });
  try {
    const registration = client.packet(bytes()); const m = sent[0];
    port.onmessage({ data: { kind: "state-ack", generation: 7n, content: 9n, operationId: m.operationId, sequence: 0n, packetKind: 5 } }); await registration;
    for (const fields of [{ node: -1 }, { node: 1024 }, { node: 1.5 }, { easing: 4 }, { durationMs: Infinity }, { transforms: new Float32Array(9) }]) {
      await assert.rejects(client.control("results-motion", { node: 2, transforms: endpoints(), durationMs: 1000, easing: 0, geometryVersion: 1n, ...fields }));
      assert.equal(sent.length, 1); assert.equal(client.pending, false);
    }
    const operation = client.control("results-motion", { node: 2, transforms: endpoints(), durationMs: 1000, easing: 0, geometryVersion: 1n });
    const admitted = sent.at(-1); assert.equal(admitted.operationId, 2n);
    port.onmessage({ data: { kind: "control-ack", operation: "results-motion", generation: 7n, content: 9n, operationId: admitted.operationId, geometryVersion: 1n } }); await operation;
  } finally { client.close(); }
});
