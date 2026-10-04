// Deferred source fixtures: actual sample client with controlled ports and deadlines.
// Scripted ACKs are transport-boundary evidence, not a second PCM validator.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

const url = new URL("./audio-sample-client.mjs", import.meta.url);
const source = await readFile(url, "utf8");
const U64 = 18446744073709551615n;
function sample(fields = {}) {
  return { id: U64, rate: 44100, channels: 2, pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]), ...fields };
}
function watch(action) {
  const observed = { settled: false };
  let returned;
  try { returned = action(); } catch (error) { returned = Promise.reject(error); }
  observed.promise = Promise.resolve(returned).then(value => { observed.settled = true; return { value }; },
    error => { observed.settled = true; return { error }; });
  return observed;
}
async function flush() { for (let index = 0; index < 24; index++) await Promise.resolve(); }
async function harness(faults = {}) {
  const timers = new Map(); let serial = 0;
  const port = {
    onmessage: null, onmessageerror: null, messages: [], transfers: [], starts: 0, closes: 0,
    postMessage(message, transfer = []) {
      if (faults.sendError) throw faults.sendError;
      assert.equal(this.closes, 0);
      this.messages.push(structuredClone(message, { transfer }));
      this.transfers.push([...transfer]);
    },
    start() { this.starts++; if (faults.startError) throw faults.startError; },
    close() { this.closes++; if (faults.closeError) throw faults.closeError; },
    receive(data) { this.onmessage?.({ data }); },
  };
  const context = createContext({ ArrayBuffer, SharedArrayBuffer, Float32Array, Uint8Array, structuredClone,
    setTimeout(callback, delay) { assert.ok(delay >= 1 && delay <= 60000); const id = ++serial; timers.set(id, callback); return id; },
    clearTimeout(id) { timers.delete(id); } });
  const actual = new SourceTextModule(source, { context, identifier: url.href });
  await actual.link(specifier => { throw new Error(`Unexpected sample client import: ${specifier}`); });
  await actual.evaluate();
  const descriptor = { port, generation: 17, channels: 2, timeoutMs: 50,
    pcmLimits: { maxAssetBytes: 16, maxTotalBytes: 24, maxSamples: 3 } };
  return { port, timers, descriptor,
    create(fields = {}) { return new actual.namespace.AudioSampleClient({ ...descriptor, ...fields }); },
    reply(fields = {}) {
      const request = port.messages.at(-1);
      port.receive({ kind: "ack", generation: 17, sequence: request.sequence, operation: request.kind,
        status: 0, admitted: 0, error: null, report: null, ...fields });
    },
    async expire() { assert.equal(timers.size, 1); const [id, callback] = timers.entries().next().value;
      timers.delete(id); callback(); await flush(); },
  };
}

test("sample transfers preserve full identities and exact ACKed totals before one immutable end receipt", async () => {
  const h = await harness(), client = h.create(); assert.equal(client.state, "ready");
  h.descriptor.pcmLimits.maxSamples = 1; // The admitted descriptor was snapshotted.
  const input = sample(), backing = input.pcm.buffer;
  const first = watch(() => client.sample(input));
  assert.equal(input.pcm.byteLength, 0); assert.equal(h.port.transfers[0][0], backing);
  assert.deepEqual(h.port.messages[0], { kind: "sample", generation: 17, sequence: 1,
    id: U64, rate: 44100, channels: 2, pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]) });
  assert.equal((await watch(() => client.sample(sample({ id: 0n }))).promise).error.code, "busy");
  assert.equal((await watch(() => client.end()).promise).error.code, "busy");
  h.reply({ generation: 16 }); await flush(); assert.equal(first.settled, false);
  h.reply(); assert.equal((await first.promise).value.sequence, 1);
  const duplicate = sample();
  assert.equal((await watch(() => client.sample(duplicate)).promise).error.code, "validation");
  assert.equal(duplicate.pcm.byteLength, 16);
  const empty = sample({ id: 0n, pcm: new Float32Array(0) }), emptyBacking = empty.pcm.buffer;
  const second = watch(() => client.sample(empty));
  assert.throws(() => new Float32Array(emptyBacking, 0, 0), TypeError);
  h.reply(); assert.equal((await second.promise).value.sequence, 2);
  const third = watch(() => client.sample(sample({ id: 9007199254740993n, rate: 96000, pcm: new Float32Array([1, -1]) })));
  h.reply(); assert.equal((await third.promise).value.sequence, 3);
  assert.equal((await watch(() => client.sample(sample({ id: 2n, pcm: new Float32Array(0) }))).promise).error.code, "validation");
  const ended = watch(() => client.end()), stale = h.port.onmessage;
  assert.deepEqual(h.port.messages.at(-1), { kind: "end-samples", generation: 17, sequence: 4, count: 3, bytes: 24 });
  assert.deepEqual(h.port.transfers.at(-1), []); await flush(); assert.equal(ended.settled, false);
  h.reply(); const receipt = (await ended.promise).value;
  assert.deepEqual({ ...receipt }, { count: 3, bytes: 24 }); assert.ok(Object.isFrozen(receipt));
  assert.equal(client.state, "ended"); assert.equal(h.port.closes, 1); assert.equal(h.timers.size, 0);
  assert.equal(h.port.onmessage, null); assert.equal(h.port.onmessageerror, null);
  stale({ data: { kind: "terminal", generation: 17, status: 9 } });
  assert.equal(client.state, "ended"); client.close(); assert.equal(client.state, "ended");
  assert.equal(h.port.closes, 1); assert.ok((await watch(() => client.end()).promise).error);
  assert.equal(h.port.messages.length, 4, "end and close do not fabricate a host stop/free receipt");
});

test("descriptor and sample metadata refuse before ownership transfer while byte and count budgets remain independent", async () => {
  const invalid = [{ port: {} }, { generation: 0 }, { generation: Number.MAX_SAFE_INTEGER + 1 },
    { channels: 0 }, { channels: 33 }, { timeoutMs: 0 }, { timeoutMs: 60001 },
    ...[{ maxAssetBytes: 0 }, { maxAssetBytes: 2147483645 }, { maxTotalBytes: 0 },
      { maxTotalBytes: 2147483645 }, { maxAssetBytes: 25 }, { maxSamples: 0 }, { maxSamples: 65537 }]
      .map(pcmLimits => ({ pcmLimits: { maxAssetBytes: 16, maxTotalBytes: 24, maxSamples: 3, ...pcmLimits } }))];
  for (const fields of invalid) {
    const h = await harness(); assert.throws(() => h.create(fields), error => error.code === "validation");
    assert.equal(h.port.starts, 0); assert.equal(h.port.closes, 0); assert.equal(h.port.messages.length, 0);
  }
  for (const fields of [
    { generation: 1, channels: 1, timeoutMs: 1, pcmLimits: { maxAssetBytes: 1, maxTotalBytes: 1, maxSamples: 1 } },
    { generation: Number.MAX_SAFE_INTEGER, channels: 32, timeoutMs: 60000,
      pcmLimits: { maxAssetBytes: 2147483644, maxTotalBytes: 2147483644, maxSamples: 65536 } },
  ]) {
    const boundary = await harness(), accepted = boundary.create(fields);
    assert.equal(accepted.state, "ready"); assert.equal(boundary.port.starts, 1);
    accepted.close(); assert.equal(boundary.port.closes, 1);
  }
  const h = await harness(), client = h.create();
  const detached = new Float32Array(0); structuredClone(detached.buffer, { transfer: [detached.buffer] });
  const resizable = new ArrayBuffer(8, { maxByteLength: 16 });
  assert.equal(resizable.resizable, true, "this deferred fixture requires resizable buffer support");
  const invalidSamples = [null, sample({ id: -1n }), sample({ id: U64 + 1n }), sample({ id: 1 }),
    sample({ rate: 0 }), sample({ rate: 4294967296 }), sample({ channels: 1 }),
    sample({ pcm: new Uint8Array(8) }), sample({ pcm: new Float32Array(3) }), sample({ pcm: new Float32Array(6) }),
    sample({ pcm: new Float32Array(new ArrayBuffer(16), 0, 2) }),
    sample({ pcm: new Float32Array(new ArrayBuffer(16), 8, 2) }),
    sample({ pcm: new Float32Array(new SharedArrayBuffer(8)) }), sample({ pcm: new Float32Array(resizable) }),
    sample({ pcm: detached })];
  for (const value of invalidSamples) {
    assert.equal((await watch(() => client.sample(value)).promise).error.code, "validation");
    assert.equal(h.port.messages.length, 0); assert.equal(client.state, "ready");
  }
  const one = watch(() => client.sample(sample())); h.reply(); await one.promise;
  const tooLargeForRemaining = sample({ id: 1n });
  assert.equal((await watch(() => client.sample(tooLargeForRemaining)).promise).error.code, "validation");
  assert.equal(tooLargeForRemaining.pcm.byteLength, 16);
  const two = watch(() => client.sample(sample({ id: 1n, pcm: new Float32Array([1, -1]) })));
  h.reply(); await two.promise;
  const zero = watch(() => client.sample(sample({ id: 2n, pcm: new Float32Array(0) })));
  h.reply(); await zero.promise;
  const done = watch(() => client.end()); assert.equal(h.port.messages.at(-1).count, 3);
  assert.equal(h.port.messages.at(-1).bytes, 24); h.reply(); await done.promise;
  const empty = await harness(), emptyClient = empty.create(), emptyEnd = watch(() => emptyClient.end());
  assert.deepEqual(empty.port.messages[0], { kind: "end-samples", generation: 17, sequence: 1, count: 0, bytes: 0 });
  empty.reply(); assert.deepEqual({ ...(await emptyEnd.promise).value }, { count: 0, bytes: 0 });
});

test("nonfinite samples transfer without a value scan and a remote refusal cannot become admitted totals or a retry", async () => {
  for (const value of [NaN, Infinity, -Infinity]) {
    const h = await harness(), client = h.create();
    const accepted = watch(() => client.sample(sample({ id: 1n, pcm: new Float32Array(0) })));
    h.reply(); await accepted.promise;
    const input = sample({ pcm: new Float32Array([value, 0]) }), pending = watch(() => client.sample(input));
    assert.equal(input.pcm.byteLength, 0); assert.ok(Object.is(h.port.messages.at(-1).pcm[0], value));
    await flush(); assert.equal(pending.settled, false);
    // This scripted status represents the remote validator, which is tested in audio-worklet.test.mjs.
    h.reply({ status: 100, error: "sample-pcm" });
    const error = (await pending.promise).error;
    assert.equal(error.code, "remote"); assert.equal(error.generation, 17); assert.equal(error.sequence, 2);
    assert.equal(error.status, 100); assert.equal(error.admitted, 0); assert.equal(client.state, "failed");
    assert.equal((await watch(() => client.end()).promise).error, error);
    assert.equal((await watch(() => client.sample(input)).promise).error, error);
    assert.equal(h.port.messages.length, 2); assert.equal(h.port.closes, 1); assert.equal(h.timers.size, 0);
    client.close(); assert.equal(h.port.closes, 1);
  }
});

test("malformed ACKs, endpoint faults and fixed deadlines fence pending sample and end requests exactly once", async () => {
  const cases = [{ sequence: 2 }, { sequence: Number.MAX_SAFE_INTEGER + 1 }, { operation: "finish" },
    { admitted: 1 }, { status: -1 }, { status: 0, error: "false success" }, { report: {} },
    "remote", "terminal", "messageerror", "timeout", "close", "remote-close", "send"];
  for (const operation of ["sample", "end"]) for (const scenario of cases) {
    const h = await harness(scenario === "send" ? { sendError: new Error("post failed") } : {});
    const client = h.create(), stale = h.port.onmessage;
    const pending = watch(() => operation === "sample" ? client.sample(sample()) : client.end());
    if (typeof scenario === "object") h.reply(scenario);
    else if (scenario === "remote") h.reply({ status: 100, error: "actual totals mismatch" });
    else if (scenario === "terminal") h.port.receive({ kind: "terminal", generation: 17, status: 9 });
    else if (scenario === "messageerror") h.port.onmessageerror({});
    else if (scenario === "timeout") await h.expire();
    else if (scenario === "close") client.close();
    else if (scenario === "remote-close") h.port.receive({ kind: "closed", generation: 17 });
    const error = (await pending.promise).error; assert.ok(error);
    assert.notEqual(client.state, "ready"); assert.equal(h.port.closes, 1); assert.equal(h.timers.size, 0);
    assert.equal(h.port.onmessage, null); assert.equal(h.port.onmessageerror, null);
    const count = h.port.messages.length;
    stale({ data: { kind: "ack", generation: 17, sequence: 1, operation: operation === "sample" ? "sample" : "end-samples",
      status: 0, admitted: 0, error: null, report: null } });
    assert.equal((await watch(() => client.sample(sample())).promise).error, error);
    assert.equal((await watch(() => client.end()).promise).error, error);
    client.close(); assert.equal(h.port.closes, 1); assert.equal(h.port.messages.length, count);
  }
  const broken = await harness({ startError: new Error("start failed") });
  assert.throws(() => broken.create(), error => error.code === "transport");
  assert.equal(broken.port.closes, 1); assert.equal(broken.port.onmessage, null);
  assert.equal(broken.port.onmessageerror, null); assert.equal(broken.timers.size, 0);
});

test("caller getters cannot revive a closed producer or replace a reentrantly admitted sample or end request", async () => {
  for (const action of ["close", "sample", "end"]) {
    const h = await harness(), client = h.create(), input = sample();
    const backing = input.pcm.buffer; let nested;
    Object.defineProperty(input, "id", { get() {
      if (action === "close") client.close();
      else nested = watch(() => action === "sample" ? client.sample(sample({ id: 1n })) : client.end());
      return U64;
    } });
    const outer = await watch(() => client.sample(input)).promise;
    assert.equal(outer.error.code, action === "close" ? "closed" : "busy");
    assert.equal(backing.byteLength, 16, "the outer sample did not transfer after losing admission");
    if (action === "close") { assert.equal(h.port.messages.length, 0); assert.equal(h.port.closes, 1); }
    else {
      assert.equal(h.port.messages.length, 1); assert.equal(h.port.messages[0].sequence, 1);
      assert.equal(h.port.messages[0].kind, action === "sample" ? "sample" : "end-samples");
      h.reply(); assert.equal((await nested.promise).error, undefined);
      client.close(); assert.equal(h.port.closes, 1);
    }
    assert.equal(h.timers.size, 0);
  }
  for (const operation of ["sample", "end"]) {
    const h = await harness(), client = h.create();
    const pending = watch(() => operation === "sample" ? client.sample(sample()) : client.end());
    const response = { kind: "ack", generation: 17, sequence: 1,
      operation: operation === "sample" ? "sample" : "end-samples", admitted: 0, error: null, report: null };
    Object.defineProperty(response, "status", { get() { client.close(); return 0; } });
    h.port.receive(response);
    const error = (await pending.promise).error;
    assert.equal(error.code, "closed"); assert.equal(client.state, "closed");
    assert.equal(h.port.closes, 1); assert.equal(h.timers.size, 0);
    assert.equal((await watch(() => client.end()).promise).error, error);
    assert.equal(h.port.messages.length, 1, "reentrant close cannot commit or initiate an EOS after refusal");
  }
});
