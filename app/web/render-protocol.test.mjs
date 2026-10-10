// Run by the coordinator after both AC005 author lanes stop.
import assert from "node:assert/strict";
import test from "node:test";
import { MessageChannel } from "node:worker_threads";
import { preflightPacket, RenderClient } from "./render-protocol.mjs";

const turn = () => new Promise(resolve => setImmediate(resolve));
async function settle() { for (let i = 0; i < 6; i++) await turn(); }
function packet(kind = 2, sequence = 1n, generation = 7n, content = 9n, size = 1) {
  const bytes = new Uint8Array(40 + size);
  bytes.set([66, 75, 82, 86]);
  const view = new DataView(bytes.buffer);
  view.setUint16(4, 1, true); view.setUint16(6, kind, true);
  view.setBigUint64(8, generation, true); view.setBigUint64(16, content, true);
  view.setBigUint64(24, sequence, true); view.setBigUint64(32, BigInt(size), true);
  return bytes;
}
function harness(options = {}) {
  const { port1, port2 } = new MessageChannel();
  const sent = [], errors = [], geometry = [];
  port2.on("message", message => sent.push(message));
  const client = new RenderClient({ port: port1, generation: 7n, content: 9n,
    maxPacketBytes: 4096, maxDiagnosticBytes: 1024, timeoutMs: 1000,
    onError: error => errors.push(error), onGeometry: message => geometry.push(message), ...options });
  return { client, sent, errors, geometry, reply: message => port2.postMessage(message),
    close() { client.close(); port1.close(); port2.close(); } };
}
const ack = request => {
  const header = preflightPacket(request.packet, 4096);
  return { kind: "state-ack", operationId: request.operationId, generation: header.generation,
    content: header.content, sequence: header.sequence, packetKind: header.kind };
};

function motionEndpoints() { return new Float32Array([0, 0, 1, 1, 1, 40, 10, 1.25, 0.75, 0.5]); }
function motionMenuPacket() {
  const bytes = new Uint8Array(48); bytes.set([66, 75, 77, 78]); const v = new DataView(bytes.buffer);
  v.setBigUint64(4, 77n, true); v.setBigUint64(12, 3n, true); v.setBigUint64(20, 5n, true); v.setUint32(28, 2, true);
  return bytes;
}

test("menu motion client authenticates exact acknowledged token and copies endpoints before transport", async () => {
  const sent = []; const port = { postMessage: m => sent.push(m), start() {}, close() {} };
  const client = new RenderClient({ port, generation: 7n, content: 9n,
    maxPacketBytes: 4096, maxDiagnosticBytes: 1024, timeoutMs: 1000 });
  const reply = data => port.onmessage({ data });
  try {
    const registration = client.menu(motionMenuPacket(), { geometryVersion: 1n });
    reply({ kind: "menu-ack", operationId: sent[0].operationId, generation: 7n, content: 9n, menuGeneration: 77n, screen: 3n, revision: 5n }); await registration;
    const fields = { menuGeneration: 77n, screen: 3n, revision: 5n, control: 1000n,
      transforms: motionEndpoints(), durationMs: 1000, easing: 0, geometryVersion: 2n };
    for (const wrong of [{ menuGeneration: 78n }, { screen: 4n }, { revision: 4n }, { control: 0n },
      { transforms: new Float32Array(9) }, { durationMs: -1 }, { easing: 4 }]) {
      await assert.rejects(client.control("menu-motion", { ...fields, ...wrong }));
      assert.equal(client.pending, false); assert.equal(client.state, "ready"); assert.equal(sent.length, 1);
    }
    const expected = [...fields.transforms]; const pending = client.control("menu-motion", fields);
    fields.transforms.fill(999); const request = sent.at(-1);
    assert.equal(request.operationId, 2n); assert.deepEqual([...request.transforms], expected);
    assert.equal(request.menuGeneration, 77n); assert.equal(request.screen, 3n); assert.equal(request.revision, 5n);
    const good = { kind: "control-ack", operation: "menu-motion", operationId: request.operationId,
      generation: 7n, content: 9n, geometryVersion: 2n };
    for (const wrong of [{ generation: 8n }, { content: 10n }, { operationId: 3n }, { geometryVersion: 3n }]) {
      reply({ ...good, ...wrong }); assert.equal(client.pending, true);
    }
    reply(good); await pending; assert.equal(client.pending, false);
  } finally { client.close(); }
});

test("correlated menu motion rejection settles only its promise and allows the next control", async () => {
  const sent = [], errors = []; const port = { postMessage: m => sent.push(m), start() {}, close() {} };
  const client = new RenderClient({ port, generation: 7n, content: 9n, maxPacketBytes: 4096,
    maxDiagnosticBytes: 1024, timeoutMs: 1000, onError: e => errors.push(e) });
  const reply = data => port.onmessage({ data });
  const fields = { menuGeneration: 77n, screen: 3n, revision: 5n, control: 1000n,
    transforms: motionEndpoints(), durationMs: 1000, easing: 0, geometryVersion: 2n };
  try {
    const menu = client.menu(motionMenuPacket(), { geometryVersion: 1n });
    reply({ kind: "menu-ack", generation: 7n, content: 9n, operationId: 1n, menuGeneration: 77n, screen: 3n, revision: 5n }); await menu;
    const rejected = client.control("menu-motion", fields);
    const rejection = { kind: "control-reject", operation: "menu-motion", operationId: 2n,
      generation: 7n, content: 9n, geometryVersion: 2n, message: "unavailable target" };
    for (const wrong of [{ content: 10n }, { generation: 8n }, { operationId: 3n }, { geometryVersion: 3n }, { operation: "resize" }]) {
      reply({ ...rejection, ...wrong }); assert.equal(client.pending, true);
    }
    reply(rejection); await assert.rejects(rejected, /unavailable target/);
    assert.equal(client.state, "ready"); assert.equal(client.failure, null); assert.deepEqual(errors, []);
    const next = client.control("menu-motion", { ...fields, control: 1001n, geometryVersion: 3n });
    reply(rejection); assert.equal(client.pending, true, "late rejection cannot settle a newer operation");
    reply({ kind: "control-ack", operation: "menu-motion", operationId: 3n, generation: 7n, content: 9n, geometryVersion: 3n }); await next;
    assert.equal(client.pending, false); assert.deepEqual(errors, []);
  } finally { client.close(); }
});
async function register(h) {
  const done = h.client.packet(packet(1, 0n), { mode: "live" });
  await settle(); h.reply(ack(h.sent[0])); await done; h.sent.length = 0;
}

test("40-byte LE admission preserves every u64 without Number coercion", () => {
  const generation = (1n << 63n) + 17n, content = (1n << 64n) - 1n;
  for (const kind of [1, 2, 3, 4, 5, 6]) {
    const sequence = [2, 3].includes(kind) ? generation : 0n;
    const header = preflightPacket(packet(kind, sequence, generation, content), 4096);
    assert.deepEqual({ kind: header.kind, generation: header.generation, content: header.content,
      sequence: header.sequence, payloadLen: header.payloadLen }, { kind, generation, content, sequence, payloadLen: 1n });
    assert.ok(Object.isFrozen(header));
  }
});

test("admission rejects width/type, malformed envelope, reserved kind, exact extent and limits", () => {
  const mutations = [
    b => { b[0] = 0; }, b => { b[4] = 2; }, b => { b[6] = 0; }, b => { b[6] = 7; },
    b => { new DataView(b.buffer).setBigUint64(8, 0n, true); },
    b => { new DataView(b.buffer).setBigUint64(16, 0n, true); },
    b => { new DataView(b.buffer).setBigUint64(24, 0n, true); },
    b => { new DataView(b.buffer).setBigUint64(32, 2n, true); },
    b => { new DataView(b.buffer).setBigUint64(32, (1n << 64n) - 1n, true); },
  ];
  for (const mutate of mutations) { const b = packet(); mutate(b); assert.throws(() => preflightPacket(b, 4096)); }
  for (const value of [null, [], new Uint32Array(40), new Uint8Array(39)]) assert.throws(() => preflightPacket(value, 4096));
  assert.throws(() => preflightPacket(packet(), 40));
  assert.throws(() => preflightPacket(packet(1, 1n), 4096));
  assert.throws(() => preflightPacket(new Uint8Array([...packet(), 0]), 4096));
  assert.throws(() => preflightPacket(new Proxy(packet(), {}), 4096));
  assert.throws(() => preflightPacket(new (class extends Uint8Array {})(packet()), 4096));
  assert.throws(() => preflightPacket(new Uint8Array(new SharedArrayBuffer(41)), 4096));
  const detached = packet(); structuredClone(detached.buffer, { transfer: [detached.buffer] });
  assert.throws(() => preflightPacket(detached, 4096));
  const backing = new Uint8Array(60); backing.set(packet(), 7);
  assert.equal(preflightPacket(new Uint8Array(backing.buffer, 7, 41), 4096).sequence, 1n);
  assert.throws(() => preflightPacket(new Uint8Array(backing.buffer, 7, 40), 4096));
});

test("detached transfer retains exact immutable ACK identity; hostile ACKs cannot adopt", async () => {
  const h = harness();
  try {
    await register(h);
    const bytes = packet(), adopted = [];
    const done = h.client.packet(bytes, { onAck: header => adopted.push(header) });
    await settle(); assert.equal(h.sent.length, 1);
    const request = h.sent[0], good = ack(request);
    // Sender transfer may detach this buffer; ACK matching must use retained header data.
    assert.equal(bytes.buffer.byteLength, 0);
    for (const wrong of [
      { ...good, generation: 8n }, { ...good, content: 10n }, { ...good, sequence: 0n },
      { ...good, sequence: 2n }, { ...good, operationId: request.operationId + 1n },
      { ...good, packetKind: 3 }, { ...good, kind: "control-ack" },
      { kind: "state-ack", operationId: request.operationId },
    ]) { h.reply(wrong); await settle(); assert.equal(adopted.length, 0); assert.ok(h.client.pending); }
    h.reply(good); await done;
    assert.equal(adopted.length, 1); assert.equal(adopted[0].sequence, 1n); assert.ok(Object.isFrozen(adopted[0]));
    h.reply(good); await settle(); assert.equal(adopted.length, 1);
  } finally { h.close(); }
});

test("dirty publication coalesces builders until full ACK and adopts before rebuilding", async () => {
  const h = harness();
  const calls = []; let baseline = 0;
  try {
    await register(h);
    assert.equal(h.client.publish(() => { calls.push("first"); return packet(); }, () => { baseline = 1; }), true);
    await settle();
    assert.equal(h.client.publish(() => { calls.push("discarded"); return packet(2, 2n); }), false);
    assert.equal(h.client.publish(() => { calls.push(`latest:${baseline}`); return packet(2, 3n); }), false);
    assert.deepEqual(calls, ["first"]);
    h.reply(ack(h.sent[0])); await settle();
    assert.deepEqual(calls, ["first", "latest:1"]); assert.equal(h.sent.length, 2);
    h.reply(ack(h.sent[1])); await settle(); assert.equal(h.client.pending, false);
  } finally { h.close(); }
});

test("static seq0 operations use distinct operation IDs and geometry never releases state", async () => {
  const h = harness();
  try {
    for (const kind of [5, 6]) {
      const done = h.client.packet(packet(kind, 0n)); await settle();
      const request = h.sent.at(-1);
      h.reply({ kind: "geometry-ack", generation: 7n, content: 9n, geometryVersion: 1n, page: 0, width: 640, height: 480 });
      await settle(); assert.ok(h.client.pending);
      h.reply(ack(request)); await done;
    }
    assert.equal(new Set(h.sent.map(message => message.operationId)).size, 2);
    assert.deepEqual(h.sent.map(message => preflightPacket(message.packet, 4096).sequence), [0n, 0n]);
  } finally { h.close(); }
});

test("foreign outgoing identity is rejected before transfer", async () => {
  const h = harness();
  try {
    await assert.rejects(h.client.packet(packet(2, 1n, 8n)));
    await settle(); assert.equal(h.sent.length, 0);
  } finally { h.close(); }
});

test("geometry controls correlate operation, generation and requested version independently", async () => {
  const h = harness();
  try {
    await register(h);
    const done = h.client.control("resize", { width: 800, height: 600, geometryVersion: 42n });
    await settle(); const request = h.sent[0];
    const good = { kind: "control-ack", operation: "resize", operationId: request.operationId,
      generation: 7n, content: 9n, geometryVersion: 42n };
    for (const wrong of [{ ...good, operation: "page" }, { ...good, geometryVersion: 41n },
      { ...good, generation: 8n }, { ...good, operationId: request.operationId + 1n }]) {
      h.reply(wrong); await settle(); assert.ok(h.client.pending);
    }
    h.reply(good); await done;
    assert.equal(h.geometry.length, 0);
    h.reply({ kind: "geometry-ack", generation: 8n, content: 9n, geometryVersion: 42n, page: 0, width: 800, height: 600 });
    await settle(); assert.equal(h.geometry.length, 0);
    h.reply({ kind: "geometry-ack", generation: 7n, content: 9n, geometryVersion: 42n, page: 0, width: 800, height: 600 });
    await settle(); assert.equal(h.geometry.length, 1);
  } finally { h.close(); }
});

test("geometry evidence admits only complete bounded submitted tuples and freezes the snapshot", async () => {
  const h = harness();
  try {
    await register(h);
    const done = h.client.control("page", { page: 3, comparisons: true, geometryVersion: 42n });
    await settle();
    const good = { kind: "geometry-ack", generation: 7n, content: 9n, geometryVersion: 42n,
      page: 2, width: 800, height: 600 };
    const invalid = [
      { ...good, page: undefined }, { ...good, width: undefined }, { ...good, height: undefined },
      { ...good, page: -1 }, { ...good, page: 0x100000000 }, { ...good, page: 1.5 },
      { ...good, page: 2n }, { ...good, page: "2" }, { ...good, page: NaN },
      { ...good, width: 0 }, { ...good, height: 0 }, { ...good, width: -1 },
      { ...good, height: 0x100000000 }, { ...good, width: 1.5 }, { ...good, height: Infinity },
      { ...good, width: 800n }, { ...good, height: "600" },
      { ...good, content: 10n }, { ...good, generation: 8n },
      { ...good, geometryVersion: 41n }, { ...good, geometryVersion: 43n },
      { ...good, geometryVersion: 42 },
    ];
    for (const tuple of invalid) {
      h.reply(tuple); await settle();
      assert.equal(h.geometry.length, 0); assert.ok(h.client.pending);
    }
    h.reply(good); await settle();
    assert.equal(h.geometry.length, 1); assert.ok(h.client.pending, "submission does not replace control ACK");
    assert.deepEqual(h.geometry[0], { generation: 7n, content: 9n, geometryVersion: 42n,
      page: 2, width: 800, height: 600 });
    assert.ok(Object.isFrozen(h.geometry[0]));
    assert.throws(() => { h.geometry[0].page = 3; }, TypeError);
    h.reply({ ...good, page: 3, width: 900 }); await settle();
    assert.equal(h.geometry.length, 1, "duplicate version cannot replace acquired snapshot");
    h.reply({ kind: "control-ack", operation: "page", operationId: h.sent[0].operationId,
      generation: 7n, content: 9n, geometryVersion: 42n }); await done;
    const newer = h.client.control("resize", { width: 900, height: 700, geometryVersion: 43n });
    await settle(); h.reply(good); await settle(); assert.equal(h.geometry.length, 1);
    h.reply({ ...good, geometryVersion: 43n, page: 0xffffffff, width: 0xffffffff, height: 1 });
    await settle(); assert.equal(h.geometry.length, 2);
    assert.equal(h.geometry[1].page, 0xffffffff); assert.equal(h.geometry[1].width, 0xffffffff);
    h.reply({ kind: "control-ack", operation: "resize", operationId: h.sent.at(-1).operationId,
      generation: 7n, content: 9n, geometryVersion: 43n }); await newer;
    assert.equal(h.errors.length, 0);
  } finally { h.close(); }
});

test("retired owner cannot publish geometry even with its former complete tuple", async () => {
  const h = harness();
  try {
    await register(h);
    const control = h.client.control("resize", { width: 640, height: 480, geometryVersion: 5n });
    await settle(); h.reply({ kind: "control-ack", operation: "resize", operationId: h.sent[0].operationId,
      generation: 7n, content: 9n, geometryVersion: 5n }); await control;
    const retired = h.client.retire(); if (retired?.catch) retired.catch(() => {});
    await settle();
    h.reply({ kind: "geometry-ack", generation: 7n, content: 9n, geometryVersion: 5n,
      page: 0, width: 640, height: 480 });
    await settle(); assert.equal(h.geometry.length, 0);
    const retirement = h.sent.find(message => message.kind === "retire");
    h.reply({ kind: "control-ack", operation: "retire", operationId: retirement.operationId,
      generation: 7n, content: 9n });
    if (retired?.then) await retired;
  } finally { h.close(); }
});

test("retirement cancels queued builders and stale ACK cannot revive publication", async () => {
  const h = harness(); const built = [];
  try {
    await register(h);
    h.client.publish(() => { built.push(1); return packet(); }); await settle();
    h.client.publish(() => { built.push(2); return packet(2, 2n); });
    const retirement = h.client.retire();
    if (retirement?.catch) retirement.catch(() => {});
    h.reply(ack(h.sent[0])); await settle();
    assert.deepEqual(built, [1]);
    const retiring = h.sent.find(message => message.kind === "retire");
    assert.ok(retiring);
    h.reply({ kind: "control-ack", operation: "retire", operationId: retiring.operationId,
      generation: 7n, content: 9n });
    if (retirement?.then) await retirement;
    await settle();
    await assert.rejects(h.client.packet(packet(2, 3n)));
    assert.deepEqual(built, [1]);
  } finally { h.close(); }
});

test("deadline retires the pending operation; later cleanup preserves first cause", async () => {
  const h = harness({ timeoutMs: 5 });
  try {
    const done = h.client.packet(packet(5, 0n));
    await assert.rejects(done, /tim|deadline/i);
    const cause = h.client.failure; assert.ok(cause);
    h.client.close(); assert.equal(h.client.failure, cause);
    await assert.rejects(h.client.packet(packet(2, 2n)));
    assert.equal(h.errors.length, 1);
  } finally { h.close(); }
});

test("renderer failure rejects pending work once and clears the dirty builder", async () => {
  const h = harness(); const adopted = [], built = [];
  try {
    await register(h);
    const done = h.client.packet(packet(), { onAck: value => adopted.push(value) });
    await settle();
    h.client.publish(() => { built.push("forbidden"); return packet(2, 2n); });
    h.reply({ kind: "render-error", generation: 7n, content: 9n,
      operationId: h.sent[0].operationId, message: "GPU terminal fault", mode: "live" });
    await assert.rejects(done, /GPU terminal fault/);
    const first = h.client.failure;
    h.client.close(); assert.equal(h.client.failure, first);
    assert.deepEqual(adopted, []); assert.deepEqual(built, []); assert.equal(h.errors.length, 1);
  } finally { h.close(); }
});
