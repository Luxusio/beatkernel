// Node transport proof only: real production classes, controlled existing port/
// WebAudio fixtures. Scripted peer packets do not prove Rust, PCM or a browser.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import * as vm from "node:vm";

// Reuse the established endpoint harnesses without editing their files or
// registering their old tests a second time. The sample fixture predates u32
// payloads, so give its production-module context the caller's u32 constructor.
async function fixture(name, exports) {
  const url = new URL(`./${name}.test.mjs`, import.meta.url);
  const original = await readFile(url, "utf8");
  const source = original.replace("createContext({", "createContext({ Uint32Array,");
  const modules = {
    "node:assert/strict": { default: assert },
    "node:fs/promises": { readFile },
    "node:test": { default() {} },
    "node:vm": vm,
  };
  const module = new vm.SourceTextModule(`${source}\nexport { ${exports.join(", ")} };`, {
    identifier: url.href,
    initializeImportMeta(meta) { meta.url = url.href; },
  });
  await module.link(specifier => {
    const values = modules[specifier];
    assert.ok(values, `unexpected fixture import ${specifier}`);
    return new vm.SyntheticModule(Object.keys(values), function () {
      for (const [name, value] of Object.entries(values)) this.setExport(name, value);
    });
  });
  await module.evaluate();
  return module.namespace;
}

const [commands, samples, hosts] = await Promise.all([
  fixture("audio-command-client", ["harness"]),
  fixture("audio-sample-client", ["harness"]),
  fixture("audio-host", ["harness", "options", "open", "stop", "acknowledged", "localError"]),
]);
const U64 = 18446744073709551615n;
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;
const PROGRAM = 9007199254740993n;

function watch(action) {
  const result = { settled: false };
  let returned;
  try { returned = action(); } catch (error) { returned = Promise.reject(error); }
  result.promise = Promise.resolve(returned).then(
    value => { result.settled = true; return { value }; },
    error => { result.settled = true; return { error }; },
  );
  return result;
}
async function flush() { for (let index = 0; index < 32; index++) await Promise.resolve(); }
function failed(result, code) {
  assert.ok(result.error, "operation unexpectedly succeeded");
  if (code !== undefined) assert.equal(result.error.code, code);
  return result.error;
}
function words(values) {
  return Uint32Array.from(values.flatMap(value => {
    const bits = BigInt.asUintN(64, BigInt(value));
    return [Number(bits & 0xffffffffn), Number(bits >> 32n)];
  }));
}
function setWord(array, index, value) {
  array.set(words([value]), index * 2);
  return array;
}
function setup(overrides = {}) {
  // Frozen setup32 fields, in ABI order. These literals do not call a codec.
  const values = [1n, PROGRAM, 100n, 1000000000n, 0n, 1n, 2n, 3n,
    0n, 2n, 1n, 2n, 4n, 4n, 4n, 8n];
  for (const [index, value] of Object.entries(overrides)) values[Number(index)] = value;
  return words(values);
}
function cue(at = I64_MIN) {
  return Uint32Array.from([...words([U64, 1n, at]), 0xbf000000, 0, 0]);
}
function preparation(programId = PROGRAM) {
  // Opaque producer data at this JS boundary. The actual Worklet/Rust owns its
  // geometry/checksum semantics; this test pins version2, identity and all bits.
  return words([2n, programId, 48000n, 2n, 2n, 1n, 2n, U64, 1n, 0x89abcdef01234567n]);
}
function legacyCommand() {
  return { kind: 0, voice: U64, sample: 1n, at: I64_MIN,
    gain: -0.5, value: I64_MAX, denominator: U64 };
}
function pcm() {
  return { id: 1n, rate: 44100, channels: 2,
    pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]) };
}
function report() {
  const output = new Uint32Array(58);
  output[0] = 1; output[3] = 0xffffffff; output[21] = 0x80000000;
  output[48] = 0x12345678; output[49] = 0x100000; output[56] = 0xffffffff;
  return { available: true, words: output };
}
function retained(event = true) {
  const metadata = words([17n, U64, 3n, 2n, 1n, 1n]);
  // History: version/program/context/Hz/initial physical/playback/retained/
  // pulled/revision/count, followed by physical/end-present/end/playback/state.
  const history = words([1n, PROGRAM, 9007199254740995n, 48000n,
    0n, 0n, 0n, 8n, U64, 2n,
    0n, 1n, 3n, 0n, 0n, 3n, 0n, 0n, 0n, 1n]);
  // Pause envelope tag4: the unused branch and reserved fields stay zero.
  const eventWords = new Uint32Array(80);
  for (const [index, value] of [[0, 1n], [1, 4n], [2, PROGRAM], [3, U64],
    [4, U64 - 1n], [5, 17n], [7, 8n], [8, 5n],
    [35, 9007199254740995n], [36, 1n]]) setWord(eventWords, index, value);
  return { metadata, history, event: event ? eventWords : null };
}
function ack(request, fields = {}) {
  return { kind: "ack", version: 1, generation: request.generation,
    sequence: request.sequence, operation: request.kind, programId: request.programId,
    status: 0, admitted: 0, error: null, terminal: false,
    report: null, retained: null, ...fields };
}
function reply(h, fields = {}) {
  const request = h.port.messages.at(-1);
  h.port.receive(ack(request, fields));
  return request;
}
async function local(h, action, code = "validation") {
  const count = h.port.messages.length;
  const error = failed(await watch(action).promise, code);
  assert.equal(h.port.messages.length, count, "local refusal must not post or consume sequence");
  return error;
}
async function successful(h, action, fields = {}) {
  const count = h.port.messages.length;
  const pending = watch(action);
  await flush();
  assert.equal(h.port.messages.length, count + 1);
  reply(h, fields);
  const result = await pending.promise;
  assert.equal(result.error, undefined, result.error?.message);
  return result.value;
}
function controlCases() {
  return [
    ["retainedRate", "retained-rate", { id: U64, attempt: U64 - 1n, revision: U64, preparation: preparation() }],
    ["retainedRegion", "retained-region", { id: U64, attempt: U64, revision: 0n,
      start: I64_MIN, end: I64_MAX, repeat: true }],
    ["retainedDisableLoop", "retained-disable-loop", { id: U64, attempt: 1n, revision: U64 }],
    ["retainedRestartOriginal", "retained-restart-original", { id: 1n, attempt: U64, revision: 0n, preparation: preparation() }],
    ["retainedPause", "retained-pause", { id: U64, attempt: U64, paused: false }],
  ];
}

test("optional actual program identity preserves legacy clients and refuses retained calls without posting", async () => {
  for (const programId of [0n, -1n, U64 + 1n, 1, "1", null]) {
    const h = await commands.harness();
    assert.throws(() => h.create({ programId }), error => error.code === "validation");
    assert.equal(h.port.starts, 0); assert.equal(h.port.messages.length, 0);
  }
  const h = await commands.harness(), owner = h.create();
  for (const [method, , request] of controlCases()) await local(h, () => owner[method](request), "state");
  for (const action of [() => owner.retainedPoll(), () => owner.retainedEventAck(1n),
    () => owner.retainedHistoryRetire(0n)]) await local(h, action, "state");
  const pending = watch(() => owner.commands([legacyCommand()]));
  assert.deepEqual(h.port.messages.at(-1), { kind: "commands", generation: 17,
    sequence: 1, commands: [legacyCommand()] });
  h.reply(); assert.equal((await pending.promise).value.admitted, 1);
  owner.close();
  const maximal = await commands.harness(), maxOwner = maximal.create({ programId: U64 });
  await successful(maximal, () => maxOwner.retainedPause({ id: 1n, attempt: 1n, paused: true }));
  assert.equal(maximal.port.messages[0].programId, U64); maxOwner.close();
});

test("all retained commands use one exact envelope, full numeric widths and immutable preparation snapshots", async () => {
  const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
  let sequence = 0;
  for (const [method, kind, request] of controlCases()) {
    const expected = structuredClone(request), input = request.preparation;
    const pending = watch(() => owner[method](request));
    assert.deepEqual(h.port.messages.at(-1), { kind, version: 1, generation: 17,
      sequence: ++sequence, programId: PROGRAM, ...expected });
    if (input) {
      assert.notEqual(h.port.raw.at(-1).preparation, input);
      input.fill(0);
      assert.deepEqual(Array.from(h.port.raw.at(-1).preparation), Array.from(expected.preparation));
      assert.equal(input.byteLength, 80, "preparation remains caller-owned");
    }
    reply(h); assert.equal((await pending.promise).error, undefined);
  }
  await successful(h, () => owner.retainedEventAck(U64));
  assert.deepEqual(h.port.messages.at(-1), { kind: "retained-event-ack", version: 1,
    generation: 17, sequence: ++sequence, programId: PROGRAM, serial: U64 });
  await successful(h, () => owner.retainedHistoryRetire(0n));
  assert.deepEqual(h.port.messages.at(-1), { kind: "retained-history-retire", version: 1,
    generation: 17, sequence: ++sequence, programId: PROGRAM, pin: 0n });
  assert.equal(h.port.messages.length, sequence); assert.equal(h.timers.size, 0); owner.close();
});

test("retained requests validate exact fields and u64/i64/boolean boundaries before enqueue", async () => {
  const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
  for (const [method, , valid] of controlCases()) {
    for (const input of [null, [], {}, { ...valid, extra: true }, { ...valid, [Symbol("extra")]: 1 }]) {
      await local(h, () => owner[method](input));
    }
    for (const field of Object.keys(valid)) {
      const missing = { ...valid }; delete missing[field];
      await local(h, () => owner[method](missing));
    }
    for (const field of ["id", "attempt", ...(Object.hasOwn(valid, "revision") ? ["revision"] : [])]) {
      for (const value of [-1n, U64 + 1n, 1, "1", null]) {
        await local(h, () => owner[method]({ ...valid, [field]: value }));
      }
    }
  }
  for (const field of ["start", "end"]) for (const value of [I64_MIN - 1n, I64_MAX + 1n, 0, "0", null]) {
    await local(h, () => owner.retainedRegion({ id: 1n, attempt: 1n, revision: 0n,
      start: 0n, end: 10n, repeat: false, [field]: value }));
  }
  for (const value of [0, 1, "true", null]) {
    await local(h, () => owner.retainedPause({ id: 1n, attempt: 1n, paused: value }));
    await local(h, () => owner.retainedRegion({ id: 1n, attempt: 1n, revision: 0n,
      start: 0n, end: 10n, repeat: value }));
  }
  for (const value of [-1n, U64 + 1n, 1, "1", null]) {
    await local(h, () => owner.retainedEventAck(value));
    await local(h, () => owner.retainedHistoryRetire(value));
  }
  await successful(h, () => owner.retainedDisableLoop({ id: 1n, attempt: 1n, revision: 0n }));
  assert.equal(h.port.messages[0].sequence, 1); owner.close();
});

function invalidBuffers(length) {
  const detached = new Uint32Array(length);
  structuredClone(detached.buffer, { transfer: [detached.buffer] });
  return [new Uint32Array(length - 1), new Uint32Array(length + 1), new Int32Array(length),
    new Uint32Array(new ArrayBuffer((length + 1) * 4), 0, length),
    new Uint32Array(new ArrayBuffer((length + 1) * 4), 4, length),
    new Uint32Array(new SharedArrayBuffer(length * 4)),
    new Uint32Array(new ArrayBuffer(length * 4, { maxByteLength: length * 8 }), 0, length),
    detached, Array(length).fill(0)];
}

test("retained preparation rejects nonexclusive backing and wrong fixed extents before posting", async () => {
  const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
  for (const value of invalidBuffers(20)) {
    for (const method of ["retainedRate", "retainedRestartOriginal"]) {
      await local(h, () => owner[method]({ id: 1n, attempt: 1n, revision: 0n, preparation: value }));
    }
  }
  assert.equal(owner.state, "ready"); assert.equal(h.timers.size, 0); owner.close();
});

test("legacy and every retained method share the existing single pending command slot", async () => {
  const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
  const pending = watch(() => owner.retainedPoll());
  assert.deepEqual(h.port.messages[0], { kind: "retained-poll", version: 1,
    generation: 17, sequence: 1, programId: PROGRAM });
  for (const [method, , request] of controlCases()) await local(h, () => owner[method](request), "busy");
  for (const action of [() => owner.poll(), () => owner.commands([legacyCommand()]),
    () => owner.retainedPoll(), () => owner.retainedEventAck(1n),
    () => owner.retainedHistoryRetire(0n)]) await local(h, action, "busy");
  assert.equal(h.timers.size, 1); assert.equal(pending.settled, false);
  reply(h, { report: report(), retained: retained(false) });
  assert.equal((await pending.promise).error, undefined);
  const legacy = watch(() => owner.poll());
  assert.deepEqual(h.port.messages.at(-1), { kind: "poll", generation: 17, sequence: 2 });
  h.reply({ report: report() }); assert.equal((await legacy.promise).value.words.length, 58);
  owner.close();
});

test("normal correlated retained refusal preserves the owner and allows the next command to succeed", async () => {
  const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
  for (const [method, , request] of controlCases()) {
    const pending = watch(() => owner[method](request)), sent = h.port.messages.at(-1);
    reply(h, { status: 7, error: "bounded-admission-refusal", terminal: false });
    const error = failed(await pending.promise, "remote");
    assert.equal(error.status, 7); assert.equal(error.sequence, sent.sequence);
    assert.equal(error.generation, 17); assert.equal(error.admitted, 0);
    assert.equal(owner.state, "ready"); assert.equal(owner.failure, null);
    assert.equal(h.timers.size, 0); assert.equal(h.port.closes, 0);
    await successful(h, () => owner.retainedPause({ id: 19n, attempt: 1n, paused: true }));
    assert.equal(h.port.messages.at(-1).sequence, sent.sequence + 1);
  }
  owner.close();
});

test("opaque rate preparation semantic refusal stays correlated and permits a valid following request", async () => {
  const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
  // The immutable outer Program remains valid. Only the actual Worklet/Rust
  // may interpret these opaque inner fields and reject their stale meaning.
  const token = preparation(PROGRAM + 1n), expected = token.slice();
  const pending = watch(() => owner.retainedRate({ id: 91n, attempt: 17n,
    revision: 4n, preparation: token }));
  assert.deepEqual(h.port.messages[0], { kind: "retained-rate", version: 1,
    generation: 17, sequence: 1, programId: PROGRAM, id: 91n, attempt: 17n,
    revision: 4n, preparation: expected });
  reply(h, { status: 7, error: "invalid-rate-preparation", terminal: false });
  const error = failed(await pending.promise, "remote");
  assert.equal(error.sequence, 1); assert.equal(owner.failure, null);
  await successful(h, () => owner.retainedRate({ id: 92n, attempt: 17n,
    revision: 4n, preparation: preparation() }));
  assert.equal(h.port.messages[1].id, 92n); assert.equal(h.port.messages[1].sequence, 2);
  owner.close();
});

test("retained malformed ACKs fence exact version, program, sequence and terminal custody", async () => {
  const cases = [
    { version: 2 }, { version: undefined }, { programId: PROGRAM + 1n }, { programId: Number(PROGRAM) },
    { programId: undefined }, { sequence: 2 }, { sequence: Number.MAX_SAFE_INTEGER + 1 },
    { operation: "retained-rate" }, { kind: "unknown" }, { admitted: 1 },
    { terminal: true }, { terminal: 0 }, { terminal: undefined }, { status: "0" },
    { error: "success-with-error" }, { report: report() }, { retained: retained() },
    { status: 7, error: null }, { status: 7, error: "refused", terminal: false, extra: 1 },
    { extra: 1 },
  ];
  for (const fields of cases) {
    const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
    const pending = watch(() => owner.retainedPause({ id: 1n, attempt: 1n, paused: true }));
    reply(h, fields);
    const original = failed(await pending.promise, "protocol");
    assert.equal(owner.state, "failed"); assert.equal(h.timers.size, 0);
    assert.equal(await local(h, () => owner.retainedPoll(), "protocol"), original);
    owner.close(); assert.equal(h.port.closes, 1);
  }
});

test("retained terminal refusal, terminal notification and timeout still fence and preserve the first cause", async () => {
  for (const cause of ["ack", "terminal", "timeout"]) {
    const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
    const pending = watch(() => owner.retainedPause({ id: 1n, attempt: 1n, paused: true }));
    if (cause === "ack") reply(h, { status: 6, error: "chronology", terminal: true });
    if (cause === "terminal") h.port.receive({ kind: "terminal", generation: 17, status: 8 });
    if (cause === "timeout") await h.expire();
    const original = failed(await pending.promise, cause === "ack" ? "remote" : cause === "terminal" ? "processor" : "timeout");
    assert.equal(owner.state, "failed"); assert.equal(owner.failure, original);
    assert.equal(await local(h, () => owner.retainedPoll(), original.code), original);
    assert.equal(h.port.messages.length, 1); assert.equal(h.timers.size, 0); owner.close();
    assert.equal(h.port.closes, 1);
  }
});

test("stale retained generations never inspect nested payload or release the current command slot", async () => {
  const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
  const pending = watch(() => owner.retainedPoll());
  const stale = ack(h.port.messages[0], { generation: 16 });
  let reads = 0;
  for (const name of ["version", "programId", "retained", "report"]) {
    Object.defineProperty(stale, name, { get() { reads++; throw new Error("stale payload"); } });
  }
  h.port.receive(stale); await flush(); assert.equal(reads, 0); assert.equal(pending.settled, false);
  reply(h, { report: report(), retained: retained() });
  assert.equal((await pending.promise).error, undefined); owner.close();
});

test("retained caller getters cannot revive a closed producer or replace a reentrant pending command", async () => {
  for (const action of ["throw", "close", "reenter"]) {
    const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
    let reads = 0, inner;
    const input = { id: 1n, attempt: 1n, paused: true };
    Object.defineProperty(input, "attempt", { enumerable: true, get() {
      reads++;
      if (action === "throw") throw new Error("caller getter failed");
      if (action === "close") owner.close();
      if (action === "reenter") inner = watch(() => owner.retainedPoll());
      return 1n;
    } });
    const result = await watch(() => owner.retainedPause(input)).promise;
    failed(result, action === "close" ? "closed" : action === "reenter" ? "busy" : undefined);
    assert.equal(reads, 1);
    assert.equal(h.port.messages.length, action === "reenter" ? 1 : 0);
    if (inner) {
      assert.equal(h.port.messages[0].kind, "retained-poll");
      reply(h, { report: report(), retained: retained(false) });
      assert.equal((await inner.promise).error, undefined);
    }
    owner.close(); assert.equal(h.timers.size, 0); assert.equal(h.port.closes, 1);
  }
});

test("retained response getters preserve reentrant close or terminal as the first failure", async () => {
  for (const action of ["throw", "close", "terminal"]) {
    const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
    const pending = watch(() => owner.retainedPoll());
    const staleHandler = h.port.onmessage;
    let reads = 0;
    const packet = ack(h.port.messages[0], { report: report(), retained: retained() });
    Object.defineProperty(packet, "retained", { get() {
      reads++;
      if (action === "throw") throw new Error("response getter failed");
      if (action === "close") owner.close();
      if (action === "terminal") staleHandler({ data: { kind: "terminal", generation: 17, status: 9 } });
      return retained();
    } });
    h.port.receive(packet);
    const error = failed(await pending.promise, action === "throw" ? "protocol" : action === "close" ? "closed" : "processor");
    assert.equal(reads, 1); if (action === "terminal") assert.equal(error.status, 9);
    assert.equal(await local(h, () => owner.retainedPoll(), error.code), error);
    owner.close(); assert.equal(h.timers.size, 0); assert.equal(h.port.closes, 1);
  }
});

test("retained poll preserves every report58, metadata12, history span and event80 word without truncation", async () => {
  const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
  for (const hasEvent of [true, false]) {
    const expectedReport = report(), expectedRetained = retained(hasEvent);
    const actual = await successful(h, () => owner.retainedPoll(), { report: expectedReport, retained: expectedRetained });
    assert.deepEqual(Object.keys(actual).sort(), ["report", "retained"]);
    assert.deepEqual(structuredClone(actual.report), expectedReport);
    assert.deepEqual(structuredClone(actual.retained), expectedRetained);
    assert.equal(actual.report.words.length, 58); assert.equal(actual.retained.metadata.length, 12);
    assert.equal(actual.retained.history.length, 40);
    assert.equal(actual.retained.event?.length ?? null, hasEvent ? 80 : null);
  }
  assert.equal(h.port.messages.length, 2, "poll never acknowledges events or retires history"); owner.close();
});

test("retained poll rejects malformed report, metadata, history, event extents and mixed identities", async () => {
  const malformed = [];
  for (const field of ["metadata", "history", "event"]) {
    const extent = field === "metadata" ? 12 : field === "history" ? 40 : 80;
    for (const value of invalidBuffers(extent)) malformed.push(data => { data.retained[field] = value; });
  }
  malformed.push(
    data => { data.report = null; },
    data => { data.report.words = new Uint32Array(56); },
    data => { data.report.words[0] = 0; },
    data => { data.retained = null; },
    data => { data.retained.extra = 1; },
    data => { delete data.retained.event; },
    data => { setWord(data.retained.history, 0, 2n); },
    data => { setWord(data.retained.history, 1, PROGRAM + 1n); },
    data => { setWord(data.retained.history, 9, 1n); },
    data => { setWord(data.retained.history, 9, 65537n); },
    data => { setWord(data.retained.event, 0, 2n); },
    data => { setWord(data.retained.event, 2, PROGRAM + 1n); },
  );
  for (const mutate of malformed) {
    const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
    const pending = watch(() => owner.retainedPoll());
    const packet = ack(h.port.messages[0], { report: report(), retained: retained() }); mutate(packet);
    h.port.receive(packet); const original = failed(await pending.promise, "protocol");
    assert.equal(owner.failure, original); assert.equal(h.timers.size, 0); owner.close();
  }
});

test("event ACK and input history retirement remain distinct explicit serial and pin requests", async () => {
  const h = await commands.harness(), owner = h.create({ programId: PROGRAM });
  await successful(h, () => owner.retainedPoll(), { report: report(), retained: retained() });
  assert.equal(h.port.messages.length, 1);
  await successful(h, () => owner.retainedEventAck(U64));
  assert.equal(h.port.messages.length, 2);
  assert.equal(h.port.messages.at(-1).kind, "retained-event-ack");
  assert.equal(Object.hasOwn(h.port.messages.at(-1), "pin"), false);
  const pending = watch(() => owner.retainedHistoryRetire(3n));
  assert.equal(h.port.messages.at(-1).kind, "retained-history-retire");
  assert.equal(Object.hasOwn(h.port.messages.at(-1), "serial"), false);
  reply(h, { status: 4, error: "held-input-pin", terminal: false });
  failed(await pending.promise, "remote"); assert.equal(owner.state, "ready");
  await successful(h, () => owner.retainedHistoryRetire(0n));
  assert.deepEqual(h.port.messages.map(packet => packet.kind), ["retained-poll", "retained-event-ack",
    "retained-history-retire", "retained-history-retire"]); owner.close();
});

test("retained sample upload keeps setup/sample/original cue order and genuine unchanged EOF totals", async () => {
  const h = await samples.harness(), owner = h.create();
  const original = setup(), copy = original.slice();
  const installing = watch(() => owner.retainedSetup(original));
  assert.deepEqual(h.port.messages[0], { kind: "retained-setup", version: 1, generation: 17,
    sequence: 1, programId: PROGRAM, words: copy });
  assert.equal(original.byteLength, 128); assert.deepEqual(h.port.transfers[0], []);
  original.fill(0); assert.deepEqual(h.port.messages[0].words, copy);
  await local(h, () => owner.retainedCue(0, cue()), "busy");
  await local(h, () => owner.sample(pcm()), "busy");
  await local(h, () => owner.end(), "busy");
  reply(h); assert.equal((await installing.promise).error, undefined);
  await local(h, () => owner.end());
  await local(h, () => owner.retainedCue(0, cue()));
  const input = pcm(), uploading = watch(() => owner.sample(input));
  assert.equal(input.pcm.byteLength, 0);
  assert.deepEqual(h.port.messages.at(-1), { kind: "sample", generation: 17, sequence: 2,
    id: 1n, rate: 44100, channels: 2, pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]) });
  h.reply(); assert.equal((await uploading.promise).error, undefined);
  for (const index of [0, 1]) {
    const inputCue = cue(index === 0 ? I64_MIN : I64_MAX), expected = inputCue.slice();
    await successful(h, () => owner.retainedCue(index, inputCue));
    assert.deepEqual(h.port.messages.at(-1), { kind: "retained-cue", version: 1,
      generation: 17, sequence: index + 3, programId: PROGRAM, index, words: expected });
    assert.equal(inputCue.byteLength, 36); assert.deepEqual(h.port.transfers.at(-1), []);
  }
  const ending = watch(() => owner.end());
  assert.deepEqual(h.port.messages.at(-1), { kind: "end-samples", generation: 17,
    sequence: 5, count: 1, bytes: 16 });
  assert.equal(ending.settled, false); assert.equal(owner.state, "ready");
  h.reply(); const ended = (await ending.promise).value;
  assert.deepEqual({ ...ended }, { count: 1, bytes: 16 });
  assert.ok(Object.isFrozen(ended)); assert.equal(owner.state, "ended");
  assert.equal(h.port.closes, 1); assert.equal(h.timers.size, 0);
});

test("retained upload refuses mode mixing, repeated setup, missing samples and nonconsecutive cues locally", async () => {
  const h = await samples.harness(), owner = h.create();
  await local(h, () => owner.retainedCue(0, cue()), "state");
  await successful(h, () => owner.retainedSetup(setup()));
  await local(h, () => owner.retainedSetup(setup()), "state");
  await local(h, () => owner.retainedCue(0, cue()));
  await local(h, () => owner.end());
  const uploading = watch(() => owner.sample(pcm())); h.reply();
  assert.equal((await uploading.promise).error, undefined);
  for (const index of [-1, 0.5, 1, 4294967296, 0n, "0"]) {
    await local(h, () => owner.retainedCue(index, cue()));
  }
  await successful(h, () => owner.retainedCue(0, cue()));
  await local(h, () => owner.retainedCue(0, cue()));
  await local(h, () => owner.end());
  await successful(h, () => owner.retainedCue(1, cue()));
  await local(h, () => owner.retainedCue(2, cue()));
  const eof = watch(() => owner.end()); h.reply(); assert.equal((await eof.promise).error, undefined);
  assert.deepEqual(h.port.messages.map(packet => packet.sequence), [1, 2, 3, 4, 5]);

  const legacy = await samples.harness(), plain = legacy.create();
  const uploaded = watch(() => plain.sample(pcm())); legacy.reply(); await uploaded.promise;
  await local(legacy, () => plain.retainedSetup(setup()), "state");
  const finished = watch(() => plain.end());
  assert.deepEqual(legacy.port.messages.at(-1), { kind: "end-samples", generation: 17,
    sequence: 2, count: 1, bytes: 16 });
  legacy.reply(); assert.equal((await finished.promise).error, undefined);
});

test("retained setup and cue enforce independent fixed extent, version and program custody", async () => {
  const h = await samples.harness(), owner = h.create();
  for (const invalid of [...invalidBuffers(32), setWord(setup(), 0, 2n),
    setWord(setup(), 1, 0n)]) await local(h, () => owner.retainedSetup(invalid));
  await successful(h, () => owner.retainedSetup(setup()));
  const uploaded = watch(() => owner.sample(pcm())); h.reply(); await uploaded.promise;
  for (const invalid of invalidBuffers(9)) await local(h, () => owner.retainedCue(0, invalid));
  await successful(h, () => owner.retainedCue(0, cue()));
  assert.equal(h.port.messages.at(-1).sequence, 3); owner.close();
  assert.equal(h.timers.size, 0); assert.equal(h.port.closes, 1);
});

test("setup identity and cue cursor commit only after successful correlated ACK", async () => {
  const h = await samples.harness(), owner = h.create();
  const refused = watch(() => owner.retainedSetup(setup()));
  reply(h, { status: 7, error: "setup-capacity", terminal: false });
  failed(await refused.promise, "remote"); assert.equal(owner.state, "ready");
  const nextProgram = PROGRAM + 9n;
  await successful(h, () => owner.retainedSetup(setup({ 1: nextProgram })));
  assert.equal(h.port.messages.at(-1).programId, nextProgram);
  const uploaded = watch(() => owner.sample(pcm())); h.reply(); await uploaded.promise;
  const pendingCue = watch(() => owner.retainedCue(0, cue()));
  reply(h, { status: 4, error: "cue-capacity", terminal: false });
  failed(await pendingCue.promise, "remote");
  await successful(h, () => owner.retainedCue(0, cue()));
  assert.equal(h.port.messages.at(-1).programId, nextProgram);
  assert.equal(h.port.messages.at(-1).index, 0); owner.close();
});

test("retained setup word getters cannot replace reentrant sample custody or revive a closed owner", async () => {
  for (const action of ["throw", "close", "reenter"]) {
    const h = await samples.harness(), owner = h.create();
    const input = setup(), backing = input.buffer;
    let reads = 0, inner;
    Object.defineProperty(input, "buffer", { get() {
      reads++;
      if (action === "throw") throw new Error("setup backing getter failed");
      if (action === "close") owner.close();
      if (action === "reenter") inner = watch(() => owner.sample(pcm()));
      return backing;
    } });
    failed(await watch(() => owner.retainedSetup(input)).promise,
      action === "throw" ? "validation" : action === "close" ? "closed" : "busy");
    assert.equal(reads, 1); assert.equal(input.byteLength, 128);
    assert.equal(h.port.messages.length, action === "reenter" ? 1 : 0);
    if (inner) {
      assert.equal(h.port.messages[0].kind, "sample"); h.reply();
      assert.equal((await inner.promise).error, undefined);
      await local(h, () => owner.retainedSetup(setup()), "state");
    }
    owner.close(); assert.equal(h.timers.size, 0); assert.equal(h.port.closes, 1);
  }
});

test("retained sample ACK getters keep the first close or terminal failure without pinning setup", async () => {
  for (const action of ["throw", "close", "terminal"]) {
    const h = await samples.harness(), owner = h.create();
    const pending = watch(() => owner.retainedSetup(setup()));
    const staleHandler = h.port.onmessage;
    const packet = ack(h.port.messages[0]); let reads = 0;
    Object.defineProperty(packet, "version", { get() {
      reads++;
      if (action === "throw") throw new Error("setup ACK getter failed");
      if (action === "close") owner.close();
      if (action === "terminal") staleHandler({ data: { kind: "terminal", generation: 17, status: 9 } });
      return 1;
    } });
    h.port.receive(packet);
    const error = failed(await pending.promise, action === "throw" ? "protocol" : action === "close" ? "closed" : "processor");
    assert.equal(reads, 1); if (action === "terminal") assert.equal(error.status, 9);
    assert.equal(await local(h, () => owner.retainedCue(0, cue()), error.code), error);
    owner.close(); assert.equal(h.timers.size, 0); assert.equal(h.port.closes, 1);
  }
});

test("retained sample ACK faults still fence while legacy sample and EOF wire stay unversioned", async () => {
  for (const fields of [{ programId: PROGRAM + 1n }, { version: 2 }, { sequence: 2 },
    { admitted: 1 }, { terminal: undefined }, { report: report() }, { extra: 1 }]) {
    const h = await samples.harness(), owner = h.create();
    const pending = watch(() => owner.retainedSetup(setup())); reply(h, fields);
    const original = failed(await pending.promise, "protocol");
    assert.equal(await local(h, () => owner.end(), "protocol"), original);
    owner.close(); assert.equal(h.port.closes, 1); assert.equal(h.timers.size, 0);
  }
  const h = await samples.harness(), owner = h.create();
  const uploading = watch(() => owner.sample(pcm()));
  assert.deepEqual(Object.keys(h.port.messages[0]).sort(), ["channels", "generation", "id", "kind", "pcm", "rate", "sequence"]);
  h.reply(); assert.equal((await uploading.promise).error, undefined);
  const ending = watch(() => owner.end()); h.reply(); assert.equal((await ending.promise).error, undefined);
  assert.deepEqual(Object.keys(h.port.messages[1]).sort(), ["bytes", "count", "generation", "kind", "sequence"]);
});

test("AudioHost finish keeps every plain call and adds only explicit retained mode without endFrame", async () => {
  const cases = [
    [[], {}], [[undefined], {}], [[undefined, { mode: "plain" }], {}],
    [[undefined, {}], {}], [[undefined, { mode: undefined }], {}],
    [[0n], { endFrame: 0n }], [[U64, { mode: "plain" }], { endFrame: U64 }],
    [[undefined, { mode: "retained" }], { mode: "retained" }],
  ];
  for (const [args, fields] of cases) {
    const h = await hosts.harness(), owner = await hosts.open(h);
    const pending = watch(() => owner.finish(...args));
    assert.deepEqual(structuredClone(h.last()), { kind: "finish", generation: 17, sequence: 1, ...fields });
    assert.equal(owner.state, "setup"); assert.equal(pending.settled, false);
    assert.deepEqual(h.sent[0].transfers, []);
    h.reply(h.last()); assert.equal((await pending.promise).error, undefined);
    assert.equal(owner.state, "allocated"); await hosts.stop(h, owner);
  }
});

test("AudioHost retained finish rejects invalid modes or finite endpoints before consuming state or sequence", async () => {
  const h = await hosts.harness(), owner = await hosts.open(h);
  for (const args of [[0n, { mode: "retained" }], [U64, { mode: "retained" }],
    [undefined, { mode: "unknown" }], [undefined, { mode: null }],
    [undefined, { mode: true }]]) {
    await hosts.localError(h, () => owner.finish(...args), "validation");
    assert.equal(owner.state, "setup");
  }
  await hosts.acknowledged(h, () => owner.finish(undefined, { mode: "retained" }));
  assert.deepEqual(structuredClone(h.last()), { kind: "finish", generation: 17, sequence: 1, mode: "retained" });
  await hosts.stop(h, owner);
});

test("AudioHost retained finish keeps the single pending slot and finite deadline", async () => {
  const h = await hosts.harness(), owner = await hosts.open(h);
  const pending = watch(() => owner.finish(undefined, { mode: "retained" }));
  await hosts.localError(h, () => owner.finish(), "busy");
  await hosts.localError(h, () => owner.poll(), "busy");
  assert.equal(h.sent.length, 1); await h.expire();
  failed(await pending.promise, "timeout"); await flush();
  assert.deepEqual(h.sent.map(row => row.message.kind), ["finish", "stop"]);
  await hosts.stop(h, owner, "failed");
});

test("AudioHost retained mode getter cannot overwrite a reentrant pending control", async () => {
  const h = await hosts.harness(), owner = await hosts.open(h);
  let reads = 0, inner;
  const options = { get mode() {
    reads++; inner = watch(() => owner.poll()); return "retained";
  } };
  failed(await watch(() => owner.finish(undefined, options)).promise, "busy");
  assert.equal(reads, 1); assert.equal(h.sent.length, 1);
  assert.equal(h.last().kind, "poll"); assert.equal(h.last().sequence, 1);
  h.reply(h.last()); assert.equal((await inner.promise).error, undefined);
  assert.equal(owner.state, "setup");
  await hosts.acknowledged(h, () => owner.finish(undefined, { mode: "retained" }));
  assert.equal(h.last().sequence, 2); assert.equal(h.last().mode, "retained");
  await hosts.stop(h, owner);
});

test("AudioHost retained finish cannot replace actual sample EOF validation or retry as plain output", async () => {
  const h = await hosts.harness(), owner = await hosts.open(h);
  await hosts.acknowledged(h, () => owner.openSamplePort());
  const pending = watch(() => owner.finish(undefined, { mode: "retained" })), sent = h.last();
  h.reply(sent, { status: 3, error: "end-samples-required" });
  const original = failed(await pending.promise, "remote");
  assert.equal(original.operation, "finish"); assert.equal(original.sequence, sent.sequence);
  await flush(); assert.deepEqual(h.sent.map(row => row.message.kind), ["attach-samples", "finish", "stop"]);
  assert.equal(await hosts.localError(h, () => owner.finish(), "remote"), original);
  await hosts.stop(h, owner, "failed");
});
