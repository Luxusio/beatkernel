// Actual direct-channel protocol; portable Rust fixtures prove body/business semantics.
import assert from "node:assert/strict";
import test from "node:test";
import { MessageChannel } from "node:worker_threads";
import { readFile } from "node:fs/promises";
import { createContext, SourceTextModule, runInContext } from "node:vm";
import { preflightMenuPacket, RenderClient } from "./render-protocol.mjs";

const turn = () => new Promise(resolve => setImmediate(resolve));
async function settle() { for (let index = 0; index < 6; index++) await turn(); }
function menuPacket({ generation = 77n, screen = 3n, revision = 5n, route = 2, fields = ["曲🎵"], pending = false } = {}) {
  const encoded = fields.map(field => new TextEncoder().encode(field));
  const bytes = new Uint8Array(48 + encoded.reduce((total, field) => total + 4 + field.length, 0));
  bytes.set([66, 75, 77, 78]); const view = new DataView(bytes.buffer);
  view.setBigUint64(4, generation, true); view.setBigUint64(12, screen, true); view.setBigUint64(20, revision, true);
  view.setUint32(28, route, true); view.setUint32(36, fields.length, true); bytes[40] = Number(pending);
  let cursor = 44;
  for (const field of encoded) { view.setUint32(cursor, field.length, true); cursor += 4; bytes.set(field, cursor); cursor += field.length; }
  return bytes;
}
function harness() {
  const { port1, port2 } = new MessageChannel(); const sent = [], errors = [], geometry = [];
  port2.on("message", message => sent.push(message));
  const client = new RenderClient({ port: port1, generation: 7n, content: 9n,
    maxPacketBytes: 1024 * 1024, maxDiagnosticBytes: 4096, timeoutMs: 1000,
    onError: error => errors.push(error), onGeometry: evidence => geometry.push(evidence) });
  return { client, port: port1, sent, errors, geometry, reply: message => port2.postMessage(message),
    close() { client.close(); port1.close(); port2.close(); } };
}
function ack(request, changes = {}) {
  const menu = preflightMenuPacket(request.packet);
  return { kind: "menu-ack", operationId: request.operationId,
    generation: request.generation, content: request.content,
    menuGeneration: menu.generation, screen: menu.screen, revision: menu.revision, ...changes };
}

test("menu admission preserves separate wide business identity and freezes every supported route", () => {
  for (let route = 1; route <= 10; route++) {
    const generation = (1n << 64n) - 1n, screen = (1n << 63n) + 1n, revision = (1n << 53n) + 1n;
    const identity = preflightMenuPacket(menuPacket({ generation, screen, revision, route, pending: true }));
    assert.deepEqual(identity, { generation, screen, revision, route, selected: 0, pending: true });
    assert.ok(Object.isFrozen(identity));
  }
});

test("menu header limits and nonnative views refuse before transfer", () => {
  for (const value of [null, [], new Uint32Array(44), new Uint8Array(43), new Uint8Array(4 * 1024 * 1024 + 1),
    new Proxy(menuPacket(), {}), new (class extends Uint8Array {})(menuPacket()),
    new Uint8Array(new SharedArrayBuffer(48))]) assert.throws(() => preflightMenuPacket(value));
  for (const mutate of [
    bytes => { bytes[0] = 0; }, bytes => { bytes[40] = 2; }, bytes => { bytes[41] = 1; },
    bytes => { bytes[42] = 1; }, bytes => { bytes[43] = 1; },
    bytes => { new DataView(bytes.buffer).setUint32(28, 0, true); },
    bytes => { new DataView(bytes.buffer).setUint32(28, 11, true); },
    bytes => { new DataView(bytes.buffer).setUint32(36, 8193, true); },
    ...[4, 12, 20].map(offset => bytes => new DataView(bytes.buffer).setBigUint64(offset, 0n, true)),
  ]) { const bytes = menuPacket(); mutate(bytes); assert.throws(() => preflightMenuPacket(bytes)); }
  const detached = menuPacket(); structuredClone(detached.buffer, { transfer: [detached.buffer] });
  assert.throws(() => preflightMenuPacket(detached));
});

test("only exact render context and menu action token ACK commits a transferred menu", async () => {
  const h = harness(), adopted = [];
  try {
    const bytes = menuPacket(), done = h.client.menu(bytes, { onAck: identity => adopted.push(identity) });
    await settle(); assert.equal(h.sent.length, 1); assert.equal(bytes.byteLength, 0);
    const request = h.sent[0], correct = ack(request);
    assert.equal(request.kind, "menu"); assert.equal(request.generation, 7n); assert.equal(request.content, 9n);
    for (const wrong of [
      { generation: 8n }, { content: 10n }, { operationId: request.operationId + 1n },
      { menuGeneration: 78n }, { screen: 4n }, { revision: 6n }, { kind: "state-ack" },
    ]) { h.reply({ ...correct, ...wrong }); await settle(); assert.equal(adopted.length, 0); assert.ok(h.client.pending); }
    h.reply(correct); await done; assert.equal(adopted.length, 1);
    h.reply(correct); await settle(); assert.equal(adopted.length, 1, "duplicate ACK cannot repeat business adoption");
    assert.equal(h.errors.length, 0);
  } finally { h.close(); }
});

test("retirement fences late menu ACK and never closes the reusable owner port", async () => {
  const h = harness(), adopted = [];
  try {
    const done = h.client.menu(menuPacket(), { onAck: identity => adopted.push(identity) });
    const rejected = assert.rejects(done); await settle(); const correct = ack(h.sent[0]);
    const retired = h.client.retire(); await rejected; h.reply(correct); await settle();
    assert.equal(adopted.length, 0); assert.equal(h.client.pending, true, "retirement owns its separate pending ACK");
    assert.equal(h.sent.at(-1).kind, "retire");
    const retirement = h.sent.at(-1);
    h.reply({ kind: "control-ack", operation: "retire", operationId: retirement.operationId,
      generation: retirement.generation, content: retirement.content });
    await retired;
    assert.equal(h.client.pending, false); assert.equal(h.client.state, "closed");
    const replacement = new RenderClient({ port: h.port, generation: 8n, content: 9n,
      maxPacketBytes: 1024 * 1024, maxDiagnosticBytes: 4096, timeoutMs: 1000 });
    const next = replacement.menu(menuPacket({ revision: 6n })); await settle();
    h.reply(ack(h.sent.at(-1))); await next;
    replacement.close();
  } finally { h.close(); }
});

test("menu state ACK never establishes visible geometry and stale revision cannot admit menu input", async () => {
  const h = harness();
  try {
    const done = h.client.menu(menuPacket(), { geometryVersion: 11n }); await settle();
    assert.equal(h.sent[0].geometryVersion, 11n); h.reply(ack(h.sent[0])); await done;
    assert.equal(h.geometry.length, 0);
    const evidence = { kind: "geometry-ack", generation: 7n, content: 9n,
      geometryVersion: 11n, page: 0, width: 640, height: 480, menuGeneration: 77n, screen: 3n, revision: 5n };
    for (const changes of [{ revision: 4n }, { screen: 2n }, { menuGeneration: 76n },
      { width: 0 }, { height: 0 }, { geometryVersion: 10n }, { generation: 8n }]) {
      h.reply({ ...evidence, ...changes }); await settle(); assert.equal(h.geometry.length, 0);
    }
    h.reply(evidence); await settle(); assert.equal(h.geometry.length, 1); assert.ok(Object.isFrozen(h.geometry[0]));
    assert.equal(h.geometry[0].screen, 3n); assert.equal(h.geometry[0].revision, 5n);
    h.reply(evidence); await settle(); assert.equal(h.geometry.length, 1);
  } finally { h.close(); }
});

async function realMenuBindings() {
  const url = new URL("./pkg/beatkernel_bms_runtime.js", import.meta.url);
  const context = createContext({ console, WebAssembly, TextEncoder, TextDecoder, Uint8Array,
    ArrayBuffer, DataView, URL, Request, Response });
  const module = new SourceTextModule(await readFile(url, "utf8"), { context,
    identifier: url.href, initializeImportMeta(meta) { meta.url = url.href; } });
  await module.link(specifier => { throw new Error(`unexpected generated binding import: ${specifier}`); });
  await module.evaluate();
  context.testWasmModule = new WebAssembly.Module(await readFile(new URL("./pkg/beatkernel_bms_runtime_bg.wasm", import.meta.url)));
  module.namespace.initSync(runInContext("({ module: testWasmModule })", context));
  assert.equal(typeof module.namespace.BrowserMenuOwner, "function", "root must build the actual current menu WASM ABI before this suite");
  return module.namespace;
}

test("actual generated WASM menu owner preserves draft and rejects stale actions without fake business effects", async () => {
  const bindings = await realMenuBindings();
  const owner = new bindings.BrowserMenuOwner((1n << 64n) - 1n);
  const identity = () => preflightMenuPacket(owner.snapshot());
  try {
    const selection = identity(); assert.equal(selection.route, 1);
    assert.equal(owner.action(selection.screen, selection.revision, 1n, 5n), 0n);
    const settings = identity(); assert.equal(settings.route, 2);
    owner.set_fields(settings.screen, settings.revision, ["曲🎵", "604800.000000001"]);
    const saved = owner.snapshot(), draft = identity();
    assert.equal(owner.action(draft.screen, draft.revision, 2n, 74n), 0n);
    const practice = identity(); assert.equal(practice.route, 3);
    owner.set_fields(practice.screen, practice.revision, ["1.000000001", "2.000000002"]);
    const edited = identity(); assert.equal(owner.action(edited.screen, edited.revision, 3n, 72n), 0n);
    const returned = identity(); assert.equal(returned.screen, draft.screen); assert.equal(returned.route, 2);
    const before = owner.snapshot();
    assert.throws(() => owner.action(edited.screen, edited.revision, 4n, 71n));
    assert.deepEqual(owner.snapshot(), before);
    // Actual Rust owns body restoration; inspect just the returned immutable fields
    // after replacing token header bytes to compare the retained draft payload.
    assert.deepEqual(owner.snapshot().slice(44), saved.slice(44));
    assert.equal(owner.action(returned.screen, returned.revision, 4n, 13n), 2n, "Load is an intent, not a successful native operation");
    assert.deepEqual(owner.snapshot().slice(44), saved.slice(44));
  } finally { owner.dispose(); owner.free(); }
});

test("actual WASM owner limits lifecycle and forbids forged Results navigation", async () => {
  const bindings = await realMenuBindings(); assert.throws(() => new bindings.BrowserMenuOwner(0n));
  const owner = new bindings.BrowserMenuOwner(77n);
  const identity = () => preflightMenuPacket(owner.snapshot());
  try {
    let token = identity(); owner.navigate(token.screen, token.revision, 2);
    token = identity(); const before = owner.snapshot();
    for (const route of [0, 8, 10, 11, 0xffffffff]) {
      assert.throws(() => owner.navigate(token.screen, token.revision, route)); assert.deepEqual(owner.snapshot(), before);
    }
    assert.throws(() => owner.set_fields(token.screen, token.revision, ["x".repeat(4097)]));
    assert.deepEqual(owner.snapshot(), before);
    owner.suspend();
    assert.throws(() => owner.action(token.screen, token.revision, 1n, 74n));
    owner.resume(); token = identity(); owner.navigate(token.screen, token.revision, 3);
    owner.dispose(); owner.resume();
    assert.throws(() => owner.navigate(token.screen, token.revision, 2));
  } finally { owner.free(); }
});

test("actual WASM navigation stages typed fields before token draft action or screen allocation changes", async () => {
  const bindings = await realMenuBindings();
  const owner = new bindings.BrowserMenuOwner(77n), reference = new bindings.BrowserMenuOwner(77n);
  const token = value => preflightMenuPacket(value.snapshot());
  const acceptedFields = ["retained settings draft"];
  try {
    assert.equal(typeof owner.navigate_with_fields, "function", "rebuild the actual atomic navigation ABI");
    for (const value of [owner, reference]) {
      const initial = token(value);
      value.navigate_with_fields(initial.screen, initial.revision, 2, acceptedFields);
    }
    const accepted = token(owner), snapshot = owner.snapshot();
    for (const [route, fields] of [
      [3, ["0"]], [7, ["auto", "fifo", "960"]], [5, []],
      [5, ["1", "1", "2", "7", "", "7", "", "0"]],
      [9, ["1", "1", "1", "7", "", "1", "1", "forged-kind", "source", "detail", "1"]],
      [2, Array(129).fill("")], [4, Array(257).fill("record")],
      [3, ["x".repeat(4097), ""]], [8, []],
    ]) {
      assert.throws(() => owner.navigate_with_fields(accepted.screen, accepted.revision, route, fields));
      assert.deepEqual(owner.snapshot(), snapshot);
      assert.deepEqual(Array.from(owner.fields()), acceptedFields);
      assert.deepEqual(token(owner), accepted);
    }
    // The unchanged accepted token and first semantic action remain usable.
    // Matching an untouched real owner also proves refusal consumed no screen ID.
    for (const value of [owner, reference]) {
      const current = token(value);
      assert.equal(value.action(current.screen, current.revision, 1n, 74n), 0n);
    }
    assert.deepEqual(owner.snapshot(), reference.snapshot());
    for (const value of [owner, reference]) {
      const current = token(value);
      value.action(current.screen, current.revision, 2n, 72n);
    }
    assert.equal(token(owner).screen, accepted.screen);
    assert.deepEqual(Array.from(owner.fields()), acceptedFields);
    const players = ["1", "1", "2", "7", "", "4294967295", "", "0"];
    for (const value of [owner, reference]) {
      const current = token(value);
      value.navigate_with_fields(current.screen, current.revision, 5, players);
    }
    assert.deepEqual(owner.snapshot(), reference.snapshot());
    assert.deepEqual(Array.from(owner.fields()), players);
  } finally {
    for (const value of [owner, reference]) { value.dispose(); value.free(); }
  }
});
