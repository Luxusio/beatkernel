// Deferred source fixtures: actual client, controlled MessagePort and deadlines only.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

const url = new URL("./audio-command-client.mjs", import.meta.url);
const source = await readFile(url, "utf8");
const U64 = 18446744073709551615n;
function command(fields = {}) {
  return { kind: 0, voice: U64, sample: 9007199254740993n, at: -9223372036854775808n,
    gain: -0.5, value: 9223372036854775807n, denominator: U64, ...fields };
}
function watch(action) {
  const result = { settled: false };
  let returned;
  try { returned = action(); } catch (error) { returned = Promise.reject(error); }
  result.promise = Promise.resolve(returned).then(value => { result.settled = true; return { value }; },
    error => { result.settled = true; return { error }; });
  return result;
}
async function flush() { for (let index = 0; index < 24; index++) await Promise.resolve(); }
async function harness(faults = {}) {
  const timers = new Map(); let serial = 0;
  const port = {
    onmessage: null, onmessageerror: null, messages: [], raw: [], starts: 0, closes: 0,
    postMessage(message, transfer = []) {
      if (faults.sendError) throw faults.sendError;
      assert.equal(this.closes, 0); assert.equal(transfer.length, 0);
      this.raw.push(message); this.messages.push(structuredClone(message));
    },
    start() { this.starts++; if (faults.startError) throw faults.startError; },
    close() { this.closes++; if (faults.closeError) throw faults.closeError; },
    receive(data) { this.onmessage?.({ data }); },
  };
  const context = createContext({ Uint32Array, ArrayBuffer, structuredClone,
    setTimeout(callback, delay) { assert.ok(delay > 0 && delay <= 60000); const id = ++serial; timers.set(id, callback); return id; },
    clearTimeout(id) { timers.delete(id); } });
  const actual = new SourceTextModule(source, { context, identifier: url.href });
  await actual.link(specifier => { throw new Error(`Unexpected client import: ${specifier}`); });
  await actual.evaluate();
  const descriptor = { port, generation: 17, queueCapacity: 4, timeoutMs: 50 };
  return { Client: actual.namespace.AudioCommandClient, port, timers, descriptor,
    create(fields = {}) { return new actual.namespace.AudioCommandClient({ ...descriptor, ...fields }); },
    reply(fields = {}) {
      const request = port.messages.at(-1);
      port.receive({ kind: "ack", generation: 17, operation: request.kind, sequence: request.sequence,
        status: 0, admitted: request.commands?.length ?? 0, error: null, report: null, ...fields });
    },
    async expire() { assert.equal(timers.size, 1); const [id, callback] = timers.entries().next().value;
      timers.delete(id); callback(); await flush(); },
  };
}

test("client preserves full command widths, one immutable batch and independent consecutive ACK ownership", async () => {
  const h = await harness(), client = h.create(); assert.equal(client.state, "ready");
  const values = [command(), command({ kind: 3, gain: 0, value: -1n })];
  const expected = structuredClone(values), first = watch(() => client.commands(values));
  assert.equal(first.settled, false); assert.equal(h.port.messages.length, 1);
  assert.equal(h.port.messages[0].generation, 17); assert.equal(h.port.messages[0].sequence, 1);
  assert.deepEqual(h.port.messages[0].commands, expected);
  assert.notEqual(h.port.raw[0].commands, values); assert.notEqual(h.port.raw[0].commands[0], values[0]);
  values[0].at = 0n; values.push(command());
  assert.equal(h.port.raw[0].commands[0].at, -9223372036854775808n);
  assert.equal(h.port.raw[0].commands.length, 2);
  assert.ok((await watch(() => client.commands([command()])).promise).error);
  assert.equal(h.port.messages.length, 1); assert.equal(client.state, "ready");
  h.reply({ generation: 16 }); await flush(); assert.equal(first.settled, false);
  h.reply(); const accepted = await first.promise; assert.equal(accepted.error, undefined);
  assert.equal(accepted.value.admitted, 2); assert.equal(accepted.value.sequence, 1);
  assert.equal(h.timers.size, 0);
  const second = watch(() => client.commands([command({ kind: 1 })]));
  assert.equal(h.port.messages.at(-1).sequence, 2); h.reply();
  assert.equal((await second.promise).value.admitted, 1);
  assert.equal(client.close(), undefined); assert.equal(client.close(), undefined);
  assert.equal(client.state, "closed"); assert.equal(h.port.closes, 1);
  assert.equal(h.port.onmessage, null); assert.equal(h.port.onmessageerror, null);
  assert.ok((await watch(() => client.commands([command()])).promise).error);
  assert.equal(h.port.messages.length, 2, "closing never invents a processor stop or a command retry");
});

test("poll and commands share one pending sequence while actual reports retain all words without becoming command ACKs", async () => {
  const h = await harness(), client = h.create();
  const initial = watch(() => client.poll());
  assert.deepEqual(h.port.messages, [{ kind: "poll", generation: 17, sequence: 1 }]);
  assert.equal(initial.settled, false);
  assert.equal((await watch(() => client.poll()).promise).error.code, "busy");
  assert.equal((await watch(() => client.commands([command()])).promise).error.code, "busy");
  assert.equal(h.port.messages.length, 1); assert.equal(h.timers.size, 1);
  const unavailable = { available: false, words: new Uint32Array(56) };
  // Armed context metadata is not rendered evidence. The transport preserves it
  // without deriving availability, a cursor or a synthetic command admission.
  unavailable.words[50] = 1; unavailable.words[53] = 0x80000000;
  h.reply({ generation: 18, report: unavailable }); await flush();
  assert.equal(initial.settled, false);
  h.reply({ report: unavailable });
  const first = (await initial.promise).value;
  assert.equal(first.available, false); assert.deepEqual(Array.from(first.words), Array.from(unavailable.words));
  assert.equal(Object.hasOwn(first, "admitted"), false); assert.equal(h.timers.size, 0);

  const submitted = watch(() => client.commands([command(), command({ kind: 1 })]));
  assert.equal(h.port.messages.at(-1).sequence, 2);
  assert.equal((await watch(() => client.poll()).promise).error.code, "busy");
  h.reply();
  const admitted = (await submitted.promise).value;
  assert.equal(admitted.operation, "commands"); assert.equal(admitted.admitted, 2);
  assert.equal(admitted.report, null);

  const completed = watch(() => client.poll());
  assert.deepEqual(h.port.messages.at(-1), { kind: "poll", generation: 17, sequence: 3 });
  const words = new Uint32Array(56);
  words.set([1, 0, 0xffffffff, 0x80000000, 0xffffffff, 0xffffffff, 1, 0x00200000]);
  words[54] = 0x89abcdef; words[55] = 0xfedcba98;
  h.reply({ report: { available: true, words } });
  const actual = (await completed.promise).value;
  assert.equal(actual.available, true); assert.deepEqual(Array.from(actual.words), Array.from(words));
  assert.equal(actual.words.byteOffset, 0); assert.equal(actual.words.buffer.byteLength, 224);
  const next = watch(() => client.commands([command({ kind: 3 })]));
  assert.equal(h.port.messages.at(-1).sequence, 4); h.reply();
  assert.equal((await next.promise).value.admitted, 1);
  assert.equal(client.state, "ready"); assert.equal(h.timers.size, 0);
  client.close(); assert.equal(h.port.closes, 1);
  assert.deepEqual(h.port.messages.map(value => value.kind), ["poll", "commands", "poll", "commands"]);
});

test("poll shape, operation and lifecycle failures permanently fence both operations without inventing evidence", async () => {
  const valid = () => ({ available: false, words: new Uint32Array(56) });
  const detached = new Uint32Array(56);
  structuredClone(detached.buffer, { transfer: [detached.buffer] });
  const resizable = new ArrayBuffer(224, { maxByteLength: 448 });
  assert.equal(resizable.resizable, true, "this deferred boundary fixture requires resizable ArrayBuffer support");
  const invalidReports = [null, {}, { available: false, words: Array(56).fill(0) },
    { available: false, words: new Uint8Array(224) },
    { available: false, words: new Uint32Array(55) }, { available: false, words: new Uint32Array(57) },
    { available: false, words: new Uint32Array(new ArrayBuffer(228), 4, 56) },
    { available: false, words: new Uint32Array(new ArrayBuffer(228), 0, 56) },
    { available: false, words: new Uint32Array(new SharedArrayBuffer(224)) },
    { available: false, words: new Uint32Array(resizable, 0, 56) },
    { available: false, words: detached }, { ...valid(), available: 0 }, { ...valid(), available: true }];
  for (const [index, value] of [[0, 2], [1, 1]]) {
    const report = valid(); report.words[index] = value; invalidReports.push(report);
  }
  const cases = [
    ...invalidReports.map(report => ({ report })),
    { operation: "commands", report: null }, { sequence: 2, report: valid() },
    { admitted: 1, report: valid() }, { status: 7, admitted: 1, report: null },
    { status: 7, report: valid() }, { error: "unexpected success diagnostic", report: valid() },
    "remote", "timeout", "close", "remote-close", "terminal", "messageerror", "send",
  ];
  for (const scenario of cases) {
    const h = await harness(scenario === "send" ? { sendError: new Error("poll post failed") } : {});
    const client = h.create(), stale = h.port.onmessage, pending = watch(() => client.poll());
    if (typeof scenario === "object") h.reply(scenario);
    else if (scenario === "remote") h.reply({ status: 7, admitted: 0, report: null, error: "actual report unavailable" });
    else if (scenario === "timeout") await h.expire();
    else if (scenario === "close") client.close();
    else if (scenario === "remote-close") h.port.receive({ kind: "closed", generation: 17 });
    else if (scenario === "terminal") h.port.receive({ kind: "terminal", generation: 17, status: 9 });
    else if (scenario === "messageerror") h.port.onmessageerror({});
    const result = await pending.promise, error = result.error;
    assert.ok(error); assert.equal(result.value, undefined);
    if (scenario === "remote") { assert.equal(error.status, 7); assert.equal(error.admitted, 0); }
    assert.notEqual(client.state, "ready"); assert.equal(h.port.closes, 1); assert.equal(h.timers.size, 0);
    assert.equal(h.port.onmessage, null); assert.equal(h.port.onmessageerror, null);
    const sent = h.port.messages.length;
    stale({ data: { kind: "ack", generation: 17, sequence: 1, operation: "poll",
      status: 0, admitted: 0, error: null, report: valid() } });
    assert.equal((await watch(() => client.poll()).promise).error, error);
    assert.equal((await watch(() => client.commands([command()])).promise).error, error);
    client.close(); assert.equal(h.port.closes, 1); assert.equal(h.port.messages.length, sent);
  }
  // Opposite direction: a valid report cannot authorize a pending command.
  const h = await harness(), client = h.create(), commandPending = watch(() => client.commands([command()]));
  h.reply({ operation: "poll", admitted: 0, report: valid() });
  const error = (await commandPending.promise).error;
  assert.equal(error.code, "protocol");
  assert.equal((await watch(() => client.poll()).promise).error, error);
  assert.equal(h.port.messages.length, 1);
});

test("client validates before port ownership or posting and preserves exact rejected-prefix evidence without fallback", async () => {
  for (const fields of [{ generation: 0 }, { generation: Number.MAX_SAFE_INTEGER + 1 }, { queueCapacity: 0 },
    { queueCapacity: 65537 }, { timeoutMs: 0 }, { timeoutMs: 60001 }, { port: {} }]) {
    const h = await harness(); assert.throws(() => h.create(fields));
    assert.equal(h.port.starts, 0); assert.equal(h.port.closes, 0);
  }
  const h = await harness(), client = h.create();
  const inherited = Object.create({ denominator: 0n }); Object.assign(inherited, command()); delete inherited.denominator;
  for (const commands of [[], Array.from({ length: 5 }, () => command()), [command(), null],
    [command({ extra: 1 })], [inherited], [command({ sample: U64 + 1n })], [command({ voice: 7 })],
    [command({ at: -9223372036854775809n })], [command({ value: 9223372036854775808n })],
    [command({ gain: NaN })], [command({ gain: 1e40 })], [command({ kind: 4 })]]) {
    assert.ok((await watch(() => client.commands(commands)).promise).error);
    assert.equal(client.state, "ready"); assert.equal(h.port.messages.length, 0);
  }
  client.close();
  for (const admitted of [0, 1, 3]) {
    const rejected = await harness(), owner = rejected.create();
    const request = watch(() => owner.commands([command(), command({ kind: 1 }), command({ kind: 2 })]));
    rejected.reply({ status: 3, admitted, error: "queue-admission" });
    const failure = (await request.promise).error; assert.ok(failure);
    assert.equal(failure.status, 3); assert.equal(failure.admitted, admitted);
    assert.equal(failure.generation, 17); assert.equal(failure.sequence, 1);
    assert.equal(owner.state, "failed"); assert.equal(rejected.port.closes, 1);
    assert.equal((await watch(() => owner.commands([command()])).promise).error, failure);
    owner.close(); assert.equal(rejected.port.closes, 1);
    assert.equal(rejected.port.messages.length, 1); assert.equal(rejected.timers.size, 0);
  }
});

test("malformed replies, terminal delivery, deadlines and close fence pending clients permanently and release handlers once", async () => {
  const malformed = [{ sequence: 2 }, { sequence: Number.MAX_SAFE_INTEGER + 1 }, { operation: "poll" },
    { admitted: 0 }, { admitted: 3 }, { status: 3, admitted: -1, error: "bad" },
    { status: 0, error: "unexpected" }, { report: { available: false } }];
  for (const scenario of [...malformed, "terminal", "messageerror", "timeout", "close", "remote-close", "send"]) {
    const h = await harness(scenario === "send" ? { sendError: new Error("actual post failed") } : {});
    const client = h.create(), stale = h.port.onmessage;
    const pending = watch(() => client.commands([command(), command({ kind: 1 })]));
    if (typeof scenario === "object") h.reply(scenario);
    else if (scenario === "terminal") h.port.receive({ kind: "terminal", generation: 17, status: 5 });
    else if (scenario === "messageerror") h.port.onmessageerror({ type: "messageerror" });
    else if (scenario === "timeout") await h.expire();
    else if (scenario === "close") client.close();
    else if (scenario === "remote-close") h.port.receive({ kind: "closed", generation: 17 });
    const error = (await pending.promise).error; assert.ok(error);
    assert.notEqual(client.state, "ready"); assert.equal(h.port.closes, 1);
    assert.equal(h.port.onmessage, null); assert.equal(h.port.onmessageerror, null); assert.equal(h.timers.size, 0);
    const sent = h.port.messages.length;
    stale({ data: { kind: "ack", generation: 17, operation: "commands", sequence: 1, status: 0,
      admitted: 2, error: null, report: null } });
    assert.equal((await watch(() => client.commands([command()])).promise).error, error);
    client.close(); assert.equal(h.port.closes, 1); assert.equal(h.port.messages.length, sent);
  }
  const start = await harness({ startError: new Error("actual port start failed") });
  assert.throws(() => start.create(), /start/i); assert.equal(start.port.closes, 1);
  assert.equal(start.port.onmessage, null); assert.equal(start.port.onmessageerror, null);
});
