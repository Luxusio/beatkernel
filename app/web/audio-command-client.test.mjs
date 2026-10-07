// Deferred source fixtures: actual client, controlled MessagePort and deadlines only.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

const url = new URL("./audio-command-client.mjs", import.meta.url);
const source = await readFile(url, "utf8");
const failureUrl = new URL("./audio-failure.mjs", import.meta.url);
const failureSource = await readFile(failureUrl, "utf8");

test("reentrant rejected ACK diagnostic getters cannot replace the first terminal cause or revive a closed owner", async () => {
  for (const action of ["close", "terminal"]) {
    const h = await harness(), owner = h.create();
    const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
    const stale = h.port.onmessage;
    const packet = { kind: "ack", generation: 17, sequence: 1, operation: "commands",
      status: 6, admitted: 1, error: "chronology", report: null };
    let reads = 0;
    Object.defineProperty(packet, "diagnostics", { get() {
      reads++;
      if (action === "close") owner.close();
      else stale({ data: { kind: "terminal", generation: 17, status: 9 } });
      return diagnosticFacts();
    } });
    h.port.receive(packet);
    const original = (await pending.promise).error;
    assert.equal(reads, 1); assert.equal(original.code, action === "close" ? "closed" : "processor");
    assert.equal(original.diagnostics, null);
    if (action === "terminal") assert.equal(original.status, 9);
    owner.close(); assert.equal(owner.state, action === "close" ? "closed" : "failed"); assert.equal(h.port.closes, 1); assert.equal((await watch(() => owner.poll()).promise).error, original);
    assert.equal(h.timers.size, 0);
  }
});


test("rejected ACK preserves its actual sequence and admitted prefix with immutable diagnostics before terminal cleanup", async () => {
  for (const hasDiagnostics of [false, true]) {
    const h = await harness(), owner = h.create();
    const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
    const stale = h.port.onmessage;
    const diagnostics = diagnosticFacts();
    const fields = { status: 6, admitted: 1, error: "chronology", ...(hasDiagnostics ? { diagnostics } : {}) };
    h.reply(fields);
    const original = (await pending.promise).error;
    assert.equal(original.code, "remote"); assert.equal(original.status, 6);
    assert.equal(original.generation, 17); assert.equal(original.sequence, 1);
    assert.equal(original.admitted, 1); assert.ok(original.message.includes("chronology"));
    if (hasDiagnostics) { assert.deepEqual({ ...original.diagnostics }, diagnosticFacts()); assert.ok(Object.isFrozen(original.diagnostics)); }
    else assert.equal(original.diagnostics, null);
    const retained = original.diagnostics;
    diagnostics.expectedFrameHigh = 0; diagnostics.currentFrame = 0;
    stale({ data: diagnosticTerminal({ status: 9 }) });
    stale({ data: diagnosticTerminal({ diagnosticVersion: 2 }) });
    assert.equal((await watch(() => owner.poll()).promise).error, original); assert.equal(original.diagnostics, retained);
    if (hasDiagnostics) assert.deepEqual({ ...retained }, diagnosticFacts());
    owner.close(); assert.equal(h.port.closes, 1);
    assert.equal((await watch(() => owner.poll()).promise).error, original); assert.equal(original.diagnostics, retained);
  }
});

test("malformed rejected ACK diagnostics and unsolicited success diagnostics are protocol failures", async () => {
  for (const [status, diagnostics] of [
    [6, {}], [6, null], [6, diagnosticFacts({ diagnosticVersion: 2 })],
    [6, diagnosticFacts({ expectedFrameHigh: "0" })],
    [6, diagnosticFacts({ successfulArmFramePresent: 0, successfulArmFrame: 1 })],
    [0, diagnosticFacts()],
  ]) {
    const h = await harness(), owner = h.create();
    const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
    const fields = { status, admitted: status === 0 ? 2 : 0,
      error: status === 0 ? null : "chronology", diagnostics };
    h.reply(fields);
    const original = (await pending.promise).error;
    assert.equal(original.code, "protocol"); assert.equal(original.diagnostics, null);
    owner.close(); assert.equal(h.port.closes, 1);
    assert.equal((await watch(() => owner.poll()).promise).error, original);
  }
});

test("stale rejected ACK never reads nested diagnostics while hostile correlated diagnostics fail closed", async () => {
  const h = await harness(), owner = h.create();
  const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
  let reads = 0;
  // Deliver a raw packet so the fixture itself does not observe the hostile property.
  const terminal = { kind: "ack", generation: 16, sequence: 1,
    operation: "commands", status: 6, admitted: 0, error: "chronology", report: null };
  Object.defineProperty(terminal, "diagnostics", { get() { reads++; throw new Error("stale diagnostics"); } });
  h.port.receive(terminal);
  await flush(); assert.equal(reads, 0); assert.equal(pending.settled, false);
  terminal.generation = 17;
  h.port.receive(terminal);
  const original = (await pending.promise).error;
  assert.equal(reads, 1); assert.equal(original.code, "protocol");
  owner.close(); assert.equal(h.port.closes, 1);
  assert.equal((await watch(() => owner.poll()).promise).error, original);
});


test("each diagnostic field is observed once and a missing versioned numeric field fails closed", async () => {
  for (const missing of [false, true]) {
    const h = await harness(), owner = h.create();
    const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
    const facts = diagnosticFacts(), reads = new Map();
    const terminal = { kind: "terminal", generation: 17, status: 6 };
    for (const [field, value] of Object.entries(facts)) {
      if (missing && field === "expectedFrameHigh") continue;
      Object.defineProperty(terminal, field, { enumerable: true, get() {
        reads.set(field, (reads.get(field) ?? 0) + 1); return value;
      } });
    }
    h.port.receive(terminal);
    const original = (await pending.promise).error;
    assert.equal(original.code, missing ? "protocol" : "processor");
    for (const [field, count] of reads) assert.equal(count, 1, field + " must be read once");
    if (!missing) assert.deepEqual({ ...original.diagnostics }, facts);
    owner.close(); assert.equal(h.port.closes, 1);
    assert.equal((await watch(() => owner.poll()).promise).error, original);
  }
});


function diagnosticFacts(fields = {}) {
  return { diagnosticVersion: 1, origin: 2, ownerPhase: 3,
    currentFramePresent: 1, currentFrame: 9007199254740991,
    blockFramesPresent: 1, blockFrames: 128,
    expectedFramePresent: 1, expectedFrameLow: 0xffffffff, expectedFrameHigh: 0x80000000,
    startFramePresent: 1, startFrameLow: 0, startFrameHigh: 0xffffffff,
    successfulArmFramePresent: 1, successfulArmFrame: 4294967297, ...fields };
}
function diagnosticTerminal(fields = {}) {
  return { kind: "terminal", generation: 17, status: 6, ...diagnosticFacts(), ...fields };
}

test("versioned terminal retains exact immutable known-only high-word facts through pending failure and cleanup", async () => {
  const h = await harness(), owner = h.create();
  const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
  const stale = h.port.onmessage;
  const terminal = diagnosticTerminal();
  Object.defineProperty(terminal, "untrustedExtra", { enumerable: true, get() { throw new Error("unknown fields must not be read"); } });
  h.port.receive(terminal);
  const original = (await pending.promise).error;
  assert.equal(original.code, "processor"); assert.equal(original.status, 6);
  assert.equal(original.generation, 17); assert.equal(original.sequence, null); assert.equal(original.admitted, null);
  assert.deepEqual({ ...original.diagnostics }, diagnosticFacts());
  assert.ok(Object.isFrozen(original.diagnostics)); assert.notEqual(original.diagnostics, terminal);
  assert.throws(() => { original.diagnostics.expectedFrameHigh = 0; }, TypeError);
  terminal.expectedFrameHigh = 0; terminal.startFrameHigh = 1; terminal.currentFrame = 0;
  const retained = original.diagnostics;
  stale({ data: diagnosticTerminal({ status: 9, expectedFrameHigh: 7 }) });
  assert.equal((await watch(() => owner.poll()).promise).error, original);
  assert.equal(original.diagnostics, retained);
  assert.deepEqual({ ...retained }, diagnosticFacts());
  owner.close(); assert.equal(h.port.closes, 1); assert.equal(h.port.onmessage, null); assert.equal(h.port.onmessageerror, null);
  assert.equal((await watch(() => owner.poll()).promise).error, original); assert.equal(original.diagnostics, retained);
  assert.equal(h.port.messages.length, 1); assert.equal(h.timers.size, 0);
});

test("legacy terminals and canonical absent or present-zero diagnostic facts remain distinct", async () => {
  for (const fields of [null,
    diagnosticFacts({ origin: 0, ownerPhase: 0, currentFramePresent: 0, currentFrame: 0,
      blockFramesPresent: 0, blockFrames: 0, expectedFramePresent: 0, expectedFrameLow: 0, expectedFrameHigh: 0,
      startFramePresent: 0, startFrameLow: 0, startFrameHigh: 0, successfulArmFramePresent: 0, successfulArmFrame: 0 }),
    diagnosticFacts({ currentFrame: 0, blockFrames: 0, expectedFrameLow: 0, expectedFrameHigh: 0,
      startFrameLow: 0, startFrameHigh: 0, successfulArmFrame: 0 })]) {
    const h = await harness(), owner = h.create();
    const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
    const terminal = { kind: "terminal", generation: 17, status: 6, ...(fields ?? {}) };
    h.port.receive(terminal);
    const original = (await pending.promise).error;
    assert.equal(original.code, "processor"); assert.equal(original.status, 6);
    if (fields === null) assert.equal(original.diagnostics, null);
    else { assert.deepEqual({ ...original.diagnostics }, fields); assert.ok(Object.isFrozen(original.diagnostics)); }
    owner.close(); assert.equal(h.port.closes, 1); assert.equal(h.port.onmessage, null); assert.equal(h.port.onmessageerror, null);
    assert.equal((await watch(() => owner.poll()).promise).error, original);
  }
});

test("malformed diagnostic version, types, presence and absent values produce sticky protocol failure", async () => {
  for (const fields of [
    { diagnosticVersion: 2 }, { diagnosticVersion: null }, { diagnosticVersion: "1" },
    { origin: 3 }, { ownerPhase: 4 }, { currentFramePresent: true }, { currentFrame: 1n },
    { currentFrame: Number.MAX_SAFE_INTEGER + 1 }, { blockFrames: 4294967296 },
    { expectedFrameHigh: -1 }, { startFrameLow: "0" }, { successfulArmFrame: Infinity },
    { currentFramePresent: 0, currentFrame: 1 }, { blockFramesPresent: 0, blockFrames: 128 },
    { expectedFramePresent: 0, expectedFrameHigh: 1 }, { startFramePresent: 0, startFrameLow: 1 },
    { successfulArmFramePresent: 0, successfulArmFrame: 1 },
  ]) {
    const h = await harness(), owner = h.create();
    const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
    const terminal = diagnosticTerminal(fields);
    h.port.receive(terminal);
    const original = (await pending.promise).error;
    assert.equal(original.code, "protocol");
    assert.equal(original.diagnostics, null);
    owner.close(); assert.equal(h.port.closes, 1); assert.equal(h.port.onmessage, null); assert.equal(h.port.onmessageerror, null);
    assert.equal((await watch(() => owner.poll()).promise).error, original); assert.equal(h.timers.size, 0);
  }
});

test("old generation never reads diagnostics or settles pending work", async () => {
  const h = await harness(), owner = h.create();
  const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
  let reads = 0;
  const terminal = { kind: "terminal", generation: 16, status: 6 };
  Object.defineProperty(terminal, "diagnosticVersion", { get() { reads++; throw new Error("obsolete diagnostics"); } });
  h.port.receive(terminal);
  await flush(); assert.equal(reads, 0); assert.equal(pending.settled, false);
  h.reply();
  assert.equal((await pending.promise).error, undefined);
  owner.close(); assert.equal(h.port.closes, 1);
});

test("hostile diagnostic getter rejects once while reentrant close or terminal preserves its first cause", async () => {
  for (const action of ["throw", "close", "terminal"]) {
    const h = await harness(), owner = h.create();
    const pending = watch(() => owner.commands([command(), command({ kind: 1 })]));
    const stale = h.port.onmessage;
    const terminal = diagnosticTerminal();
    let reads = 0;
    Object.defineProperty(terminal, "expectedFrameLow", { get() {
      reads++;
      if (action === "throw") throw new Error("hostile diagnostic read");
      if (action === "close") owner.close();
      else stale({ data: { kind: "terminal", generation: 17, status: 9 } });
      return 0xffffffff;
    } });
    h.port.receive(terminal);
    const original = (await pending.promise).error;
    assert.equal(reads, 1);
    assert.equal(original.code, action === "throw" ? "protocol" : action === "close" ? "closed" : "processor");
    assert.equal(original.diagnostics, null);
    if (action === "terminal") assert.equal(original.status, 9);
    stale({ data: diagnosticTerminal({ diagnosticVersion: 2 }) });
    owner.close(); assert.equal(h.port.closes, 1); assert.equal(owner.state, action === "close" ? "closed" : "failed");
    assert.equal((await watch(() => owner.poll()).promise).error, original);
    assert.equal(h.timers.size, 0);
  }
});

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
  let clockReads = 0;
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
    Date: class extends Date { static now() { clockReads++; return 0; } },
    performance: { now() { clockReads++; return 0; } },
    setTimeout(callback, delay) { assert.ok(delay > 0 && delay <= 60000); const id = ++serial; timers.set(id, callback); return id; },
    clearTimeout(id) { timers.delete(id); } });
  const actual = new SourceTextModule(source, { context, identifier: url.href });
  await actual.link(specifier => {
    if (specifier === "./audio-failure.mjs") return new SourceTextModule(failureSource, {
      context, identifier: failureUrl.href,
    });
    throw new Error(`Unexpected client import: ${specifier}`);
  });
  await actual.evaluate();
  const descriptor = { port, generation: 17, queueCapacity: 4, timeoutMs: 50 };
  return { Client: actual.namespace.AudioCommandClient, port, timers, descriptor,
    get clockReads() { return clockReads; },
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

test("failure getter is read-only and healthy reads perform no port or deadline operation", async () => {
  const h = await harness(), client = h.create();
  const originalClockReads = h.clockReads;
  for (let index = 0; index < 32; index++) assert.equal(client.failure, null);
  assert.throws(() => { client.failure = new Error("replacement"); }, TypeError);
  assert.equal(client.state, "ready"); assert.equal(h.port.starts, 1);
  assert.equal(h.port.messages.length, 0); assert.equal(h.port.closes, 0);
  assert.equal(h.timers.size, 0);
  assert.equal(h.clockReads, originalClockReads);
  client.close();
});

test("validated rejected ACK status and bounded descriptor preserve one sticky first cause", async () => {
  for (const descriptor of [null, "", "queue-admission", "x".repeat(4096)]) {
    const h = await harness(), client = h.create(), stale = h.port.onmessage;
    const pending = watch(() => client.commands([command(), command({ kind: 1 })]));
    h.reply({ status: 4294967295, admitted: 1, error: descriptor });
    const error = (await pending.promise).error;
    assert.equal(error.code, "remote"); assert.equal(error.status, 4294967295);
    assert.equal(error.generation, 17); assert.equal(error.sequence, 1); assert.equal(error.admitted, 1);
    assert.ok(error.message.includes("4294967295"));
    if (descriptor !== null && descriptor.length > 0) {
      assert.ok(error.message.includes(descriptor));
      assert.ok(error.message.indexOf("4294967295") < error.message.indexOf(descriptor));
    }
    assert.equal(client.failure, error);
    stale({ data: diagnosticTerminal({ status: 8 }) });
    client.close();
    assert.equal(client.failure, error);
    assert.equal((await watch(() => client.poll()).promise).error, error);
    assert.equal((await watch(() => client.commands([command()])).promise).error, error);
    assert.equal(h.port.messages.length, 1); assert.equal(h.port.closes, 1); assert.equal(h.timers.size, 0);
  }
});

test("terminal first cause exposes numeric status while stale and malformed packets retain their guards", async () => {
  const h = await harness(), client = h.create(), stale = h.port.onmessage;
  const pending = watch(() => client.poll());
  h.port.receive({ kind: "terminal", generation: 16, status: "unvalidated" });
  await flush(); assert.equal(pending.settled, false); assert.equal(client.failure, null);
  h.port.receive({ kind: "terminal", generation: 17, status: 4294967295 });
  const original = (await pending.promise).error;
  assert.equal(original.code, "processor"); assert.equal(original.generation, 17);
  assert.equal(original.status, 4294967295); assert.ok(original.message.includes("4294967295"));
  assert.equal(client.failure, original);
  stale({ data: { kind: "terminal", generation: 17, status: 2 } });
  client.close(); assert.equal(client.failure, original);
  assert.equal((await watch(() => client.poll()).promise).error, original);
  assert.equal(h.port.messages.length, 1); assert.equal(h.port.closes, 1);
  for (const fields of [
    { kind: "terminal", status: 0 }, { kind: "terminal", status: "8" },
    { kind: "terminal", status: 4294967296 },
    { kind: "ack", error: "x".repeat(4097) }, { kind: "ack", error: { message: "unvalidated" } },
  ]) {
    const invalid = await harness(), owner = invalid.create();
    const request = watch(() => owner.commands([command()]));
    if (fields.kind === "terminal") invalid.port.receive({ generation: 17, ...fields });
    else invalid.reply({ status: 8, admitted: 0, ...fields });
    const error = (await request.promise).error;
    assert.equal(error.code, "protocol"); assert.equal(owner.failure, error);
    owner.close(); assert.equal(owner.failure, error);
    assert.equal(invalid.port.messages.length, 1); assert.equal(invalid.port.closes, 1);
  }
});
