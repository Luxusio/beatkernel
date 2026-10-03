// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/*.test.mjs
// Executes the actual processor against binding spies, without audio or WASM execution.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";

const processorSource = await readFile(new URL("./audio-worklet.js", import.meta.url), "utf8");
const encodingSource = await readFile(new URL("./worklet-encoding.mjs", import.meta.url), "utf8");
const emptyModule = new WebAssembly.Module(Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0]));

function options(overrides = {}) {
  return {
    module: emptyModule, generation: 17, channels: 2, ...overrides,
    pcmLimits: { maxAssetBytes: 16, maxTotalBytes: 24, maxSamples: 2, ...overrides.pcmLimits },
    audioLimits: { queueCapacity: 4, maxVoices: 2, pendingCapacity: 4, maxFrames: 5, maxCommandsPerRender: 4, ...overrides.audioLimits },
  };
}
function command(overrides = {}) {
  return { kind: 0, voice: 9007199254740993n, sample: 18446744073709551615n,
    at: -9223372036854775808n, gain: 0.5, value: 9223372036854775807n,
    denominator: 18446744073709551615n, ...overrides };
}
function sample(overrides = {}) {
  return { id: 1n, rate: 44100, channels: 2, pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]), ...overrides };
}

async function harness(faults = {}) {
  const owners = [];
  const ports = [];
  const initializations = [];
  const memory = { buffer: new ArrayBuffer(4096) };
  let Processor;
  let inProcess = false;
  let memoryReads = 0;
  class FakeProcessor {
    constructor() {
      this.port = {
        messages: [], onmessage: null, closed: false,
        postMessage(message) { this.messages.push(structuredClone(message)); },
        close() { assert.equal(inProcess, false, "port cleanup belongs to control messages"); this.closed = true; },
      };
      ports.push(this.port);
    }
  }
  class BrowserAudio {
    constructor(...args) {
      if (faults.constructorError) throw new Error("binding constructor failed");
      this.args = args;
      this.calls = { samples: [], finish: 0, finishAt: [], arm: [], enqueue: [], render: [], reports: [], metadata: 0 };
      this.frees = 0;
      this.pcm = new Float32Array(0);
      this.words = new Uint32Array(56);
      owners.push(this);
    }
    status() { return faults.status ?? 0; }
    insert_sample(...args) { this.calls.samples.push(args); return faults.sampleStatus ?? 0; }
    finish() { this.calls.finish++; return faults.finishStatus ?? 0; }
    finish_at(end) {
      this.calls.finishAt.push(end);
      if (faults.finishAtThrows) throw new Error("finite binding finish failed");
      return faults.finishAtStatus ?? 0;
    }
    channels() { this.calls.metadata++; return faults.channels ?? this.args[1]; }
    max_frames() { this.calls.metadata++; return faults.maxFrames ?? this.args[8]; }
    output_ptr() { this.calls.metadata++; return faults.pointer ?? 64; }
    output_len() { this.calls.metadata++; return faults.length ?? this.args[1] * this.args[8]; }
    arm(...args) { this.calls.arm.push(args); return faults.armStatus ?? 0; }
    enqueue(...args) {
      this.calls.enqueue.push(args);
      if (this.calls.enqueue.length === faults.enqueueThrowAt) throw new Error("binding admission threw");
      return faults.enqueueStatuses?.[this.calls.enqueue.length - 1] ?? 0;
    }
    render(...args) {
      assert.equal(inProcess, true);
      this.calls.render.push(args);
      if (faults.renderThrows) throw new Error("render threw");
      new Float32Array(memory.buffer, 64, this.args[1] * this.args[8]).set(this.pcm);
      if (faults.growOnRender) memory.buffer = new ArrayBuffer(8192);
      return faults.renderStatus ?? 0;
    }
    report_word(index, high) {
      assert.equal(inProcess, false, "polling must stay outside process");
      this.calls.reports.push([index, high]);
      return this.words[index * 2 + Number(high)];
    }
    free() {
      assert.equal(inProcess, false, "Rust destruction must stay outside process");
      this.frees++;
      assert.equal(this.frees, 1);
    }
    static memory() { memoryReads++; return memory; }
  }
  const context = createContext({
    ArrayBuffer, Uint8Array, Uint32Array, Float32Array, WebAssembly,
    AudioWorkletProcessor: FakeProcessor, sampleRate: 48000, currentFrame: 0,
    registerProcessor(name, value) {
      assert.equal(name, "beatkernel-audio");
      assert.equal(Processor, undefined);
      Processor = value;
    },
  });
  const encoding = new SourceTextModule(encodingSource, { context });
  const bindings = new SyntheticModule(["initSync", "BrowserAudio"], function () {
    this.setExport("initSync", args => {
      assert.equal(typeof context.TextEncoder, "function", "bootstrap must precede binding initialization");
      assert.equal(typeof context.TextDecoder, "function");
      initializations.push(args.module);
      if (faults.initError) throw new Error("module initialization failed");
    });
    this.setExport("BrowserAudio", BrowserAudio);
  }, { context });
  const actual = new SourceTextModule(processorSource, { context });
  await actual.link(specifier => {
    if (specifier === "./worklet-encoding.mjs") return encoding;
    if (specifier === "./audio-pkg/beatkernel_bms_runtime.js") return bindings;
    throw new Error(`Unexpected processor import: ${specifier}`);
  });
  await actual.evaluate();
  return {
    owners, ports, memory, context, initializations,
    get memoryReads() { return memoryReads; },
    create(overrides) { return new Processor({ processorOptions: options(overrides) }); },
    send(processor, kind, sequence, fields = {}) {
      assert.equal(typeof processor.port.onmessage, "function");
      processor.port.onmessage({ data: { kind, generation: 17, sequence, ...fields } });
      return processor.port.messages.filter(message => message.kind === "ack").at(-1);
    },
    process(processor, frame, output) {
      context.currentFrame = frame;
      inProcess = true;
      try { return processor.process([], output); } finally { inProcess = false; }
    },
  };
}
function planar(frames, channels = 2) {
  return [Array.from({ length: channels }, () => new Float32Array(frames).fill(9))];
}
function notices(processor, kind) { return processor.port.messages.filter(message => message.kind === kind); }
function assertSilent(output) {
  for (const bus of output) for (const channel of bus) assert.ok(channel.every(value => value === 0));
}

test("failed initialization releases singleton and a second live owner never reinitializes bindings", async () => {
  for (const failure of ["initError", "constructorError", "status"]) {
    const faults = { [failure]: failure === "status" ? 9 : true };
    const h = await harness(faults);
    assert.throws(() => h.create());
    assert.equal(h.owners.length, failure === "status" ? 1 : 0);
    if (h.owners.length) assert.equal(h.owners[0].frees, 1);
    faults[failure] = failure === "status" ? 0 : false;
    const live = h.create();
    assert.equal(notices(live, "ready").length, 1);
    const count = h.initializations.length;
    assert.throws(() => h.create(), /already owns/);
    assert.equal(h.initializations.length, count);
    h.send(live, "stop", 1);
    assert.equal(live.port.closed, true);
    assert.equal(h.owners.at(-1).frees, 1);
    const next = h.create();
    assert.equal(notices(next, "ready").length, 1);
    h.send(next, "stop", 1);
  }
  const h = await harness();
  assert.throws(() => h.create({ pcmLimits: { maxAssetBytes: 25, maxTotalBytes: 24 } }));
  assert.equal(h.initializations.length, 0);
  assert.equal(h.owners.length, 0);
  const valid = h.create();
  h.send(valid, "stop", 1);
});

test("sample limits and malformed PCM reject before binding copies while exact budgets remain usable", async () => {
  const cases = [
    { invalid: sample({ id: 1 }) },
    { invalid: sample({ id: 18446744073709551616n }) },
    { invalid: sample({ rate: 0 }) },
    { invalid: sample({ channels: 1 }) },
    { invalid: sample({ pcm: new Float32Array([NaN, 0]) }) },
    { invalid: sample({ pcm: new Float32Array([Infinity, 0]) }) },
    { invalid: sample({ pcm: new Uint8Array(4) }) },
    { invalid: sample({ pcm: new Float32Array(3) }) },
    { invalid: sample({ pcm: new Float32Array(6) }) },
    { prefix: [sample()], invalid: sample() },
    { prefix: [sample()], invalid: sample({ id: 2n }) },
    { prefix: [sample(), sample({ id: 2n, pcm: new Float32Array(2) })], invalid: sample({ id: 3n, pcm: new Float32Array(0) }) },
  ];
  for (const { prefix = [], invalid } of cases) {
    const h = await harness();
    const processor = h.create();
    let sequence = 0;
    for (const value of prefix) assert.equal(h.send(processor, "sample", ++sequence, value).status, 0);
    const before = h.owners[0].calls.samples.length;
    const ack = h.send(processor, "sample", ++sequence, invalid);
    assert.equal(ack.status, 100);
    assert.equal(ack.admitted, 0);
    assert.equal(h.owners[0].calls.samples.length, before);
    assert.equal(notices(processor, "terminal").length, 1);
    h.send(processor, "stop", ++sequence);
  }
  const h = await harness();
  const processor = h.create();
  const pcm = sample().pcm;
  assert.equal(h.send(processor, "sample", 1, sample({ pcm })).status, 0);
  assert.equal(h.send(processor, "sample", 2, sample({ id: 2n, pcm: new Float32Array(2) })).status, 0);
  assert.equal(h.owners[0].calls.samples[0][3], pcm);
  assert.deepEqual(h.owners[0].calls.samples[0].slice(0, 3), [1n, 44100, 2]);
  assert.equal(h.send(processor, "finish", 3).status, 0);
  h.send(processor, "stop", 4);
});

test("finite finish selects the exact binding once and omitted endpoints preserve unlimited setup", async () => {
  for (const fields of [{}, { endFrame: undefined }, { endFrame: 0n }, { endFrame: 1n },
    { endFrame: 9007199254740993n }, { endFrame: 18446744073709551615n }]) {
    const h = await harness();
    const processor = h.create();
    const owner = h.owners[0];
    const ack = h.send(processor, "finish", 1, fields);
    assert.equal(ack.status, 0);
    assert.equal(ack.admitted, 0);
    assert.equal(ack.operation, "finish");
    assert.equal(ack.sequence, 1);
    assert.equal(owner.calls.finish, fields.endFrame === undefined ? 1 : 0);
    assert.deepEqual(owner.calls.finishAt, fields.endFrame === undefined ? [] : [fields.endFrame]);
    assert.ok(owner.calls.metadata > 0, "fixed storage is acquired only after the selected finish succeeds");
    const polled = h.send(processor, "poll", 2);
    assert.equal(polled.report.available, false, "configuration alone cannot fabricate a finite render marker");
    assert.ok(polled.report.words.every(value => value === 0));
    assert.equal(h.send(processor, "arm", 3, { frame: 0n }).status, 0);
    assert.deepEqual(owner.calls.arm, [[0n, 0n]]);
    const value = command();
    assert.equal(h.send(processor, "commands", 4, { commands: [value] }).admitted, 1);
    assert.deepEqual(owner.calls.enqueue[0], [value.kind, value.voice, value.sample, value.at, value.gain, value.value, value.denominator]);
    assert.equal(notices(processor, "terminal").length, 0);
    h.send(processor, "stop", 5);
    assert.equal(owner.frees, 1);
  }
});

test("worklet independently rejects malformed finite endpoints and fences binding failures without an unlimited fallback", async () => {
  for (const endFrame of [null, 0, "1", -1n, 18446744073709551616n, 1.5, NaN, Infinity, {}, []]) {
    const h = await harness();
    const processor = h.create();
    const owner = h.owners[0];
    const ack = h.send(processor, "finish", 1, { endFrame });
    assert.equal(ack.status, 100);
    assert.equal(ack.error, "finish-end");
    assert.equal(ack.admitted, 0);
    assert.equal(owner.calls.finish, 0);
    assert.deepEqual(owner.calls.finishAt, []);
    assert.equal(owner.calls.metadata, 0);
    const output = planar(3);
    assert.equal(h.process(processor, 0, output), false);
    assertSilent(output);
    assert.equal(notices(processor, "terminal").length, 1);
    assert.equal(owner.frees, 0);
    h.send(processor, "stop", 2);
    assert.equal(owner.frees, 1);
  }
  for (const [faults, absent, status] of [[{ finishAtStatus: 9 }, false, 9],
    [{ finishAtThrows: true }, false, 106], [{}, true, 106]]) {
    const h = await harness(faults);
    const processor = h.create();
    const owner = h.owners[0];
    if (absent) owner.finish_at = undefined;
    const ack = h.send(processor, "finish", 1, { endFrame: 3n });
    assert.equal(ack.status, status);
    assert.equal(ack.admitted, 0);
    assert.equal(owner.calls.finish, 0);
    assert.deepEqual(owner.calls.finishAt, absent ? [] : [3n]);
    assert.equal(owner.calls.metadata, 0);
    assert.deepEqual(processor.port.messages.slice(-2).map(message => message.kind), ["ack", "terminal"]);
    assert.equal(h.send(processor, "finish", 2).status, 103);
    assert.equal(owner.calls.finish, 0, "even a later unlimited request cannot recover a failed finite owner");
    assert.equal(notices(processor, "terminal").length, 1);
    assert.equal(owner.frees, 0);
    h.send(processor, "stop", 3);
    assert.equal(owner.frees, 1);
  }
});

test("setup stays silent and unavailable; fixed view copies varied planar blocks with exact frame words", async () => {
  const h = await harness();
  const processor = h.create();
  const owner = h.owners[0];
  const setup = planar(3);
  assert.equal(h.process(processor, 0, setup), true);
  assertSilent(setup);
  assert.equal(owner.calls.render.length, 0);
  const initial = h.send(processor, "poll", 1);
  assert.equal(initial.report.available, false);
  assert.equal(initial.report.words.length, 56);
  assert.ok(initial.report.words.every(value => value === 0));
  assert.equal(h.send(processor, "finish", 2).status, 0);
  const metadataReads = owner.calls.metadata;
  const memoryReads = h.memoryReads;
  const base = 3 * 4294967296 - 1;
  h.context.currentFrame = base;
  assert.equal(h.send(processor, "arm", 3, { frame: BigInt(base + 1) }).status, 0);
  assert.deepEqual(owner.calls.arm, [[BigInt(base + 1), BigInt(base)]]);
  const admitted = h.send(processor, "commands", 4, { commands: [0, 1, 2, 3].map(kind => command({ kind })) });
  assert.equal(admitted.status, 0);
  assert.equal(admitted.admitted, 4);
  assert.deepEqual(owner.calls.enqueue.map(args => args[0]), [0, 1, 2, 3]);
  owner.pcm = new Float32Array([0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 1, -1, 0.125, -0.125]);
  const messagesBefore = processor.port.messages.length;
  const reportsBefore = owner.calls.reports.length;
  let current = base;
  for (const words of [[0xffffffff, 2, 1], [0, 3, 3], [3, 3, 5], [8, 3, 0]]) {
    const frames = words[2];
    const output = planar(frames);
    assert.equal(h.process(processor, current, output), true);
    assert.deepEqual(Array.from(output[0][0]), Array.from(owner.pcm).filter((_, index) => index % 2 === 0).slice(0, frames));
    assert.deepEqual(Array.from(output[0][1]), Array.from(owner.pcm).filter((_, index) => index % 2 === 1).slice(0, frames));
    assert.deepEqual(owner.calls.render.at(-1), words);
    current += frames;
  }
  assert.equal(owner.calls.metadata, metadataReads);
  assert.equal(h.memoryReads, memoryReads);
  assert.equal(owner.calls.reports.length, reportsBefore);
  assert.equal(processor.port.messages.length, messagesBefore);
  assert.equal(owner.frees, 0);
  // The binding's report values are copied, including high words, without deriving a report from callback count.
  owner.words[0] = 1;
  owner.words[3] = 0x80000001;
  owner.words[48] = 0xabcdef01;
  owner.words[49] = 3;
  const polled = h.send(processor, "poll", 5);
  assert.equal(polled.report.available, true);
  assert.deepEqual(Array.from(polled.report.words), Array.from(owner.words));
  assert.equal(owner.calls.reports.length - reportsBefore, 56);
  h.send(processor, "stop", 6);
});

test("malformed later commands preflight the whole batch before any admission", async () => {
  const invalid = [
    command({ kind: 4 }), command({ voice: 1 }), command({ sample: -1n }),
    command({ at: 9223372036854775808n }), command({ at: 0 }), command({ gain: NaN }),
    command({ gain: 1e40 }), command({ value: -9223372036854775809n }),
    command({ denominator: 18446744073709551616n }), null,
  ];
  for (const commands of [...invalid.map(value => [command(), value]), [], Array.from({ length: 5 }, () => command())]) {
    const h = await harness();
    const processor = h.create();
    h.send(processor, "finish", 1);
    const ack = h.send(processor, "commands", 2, { commands });
    assert.equal(ack.status, 100);
    assert.equal(ack.admitted, 0);
    assert.equal(h.owners[0].calls.enqueue.length, 0);
    assert.equal(notices(processor, "terminal").length, 1);
    h.send(processor, "stop", 3);
  }
});

test("binding failure and exceptions acknowledge the exact admitted prefix and never retry it", async () => {
  for (const [faults, status] of [[{ enqueueStatuses: [0, 3] }, 3], [{ enqueueThrowAt: 2 }, 106]]) {
    const h = await harness(faults);
    const processor = h.create();
    const owner = h.owners[0];
    h.send(processor, "finish", 1);
    const commands = [command(), command({ kind: 2, value: -1n, denominator: 2n }), command({ kind: 1 })];
    const ack = h.send(processor, "commands", 2, { commands });
    assert.equal(ack.status, status);
    assert.equal(ack.admitted, 1);
    assert.equal(owner.calls.enqueue.length, 2);
    for (let index = 0; index < 2; index++) {
      const c = commands[index];
      assert.deepEqual(owner.calls.enqueue[index], [c.kind, c.voice, c.sample, c.at, c.gain, c.value, c.denominator]);
    }
    assert.deepEqual(processor.port.messages.slice(-2).map(message => message.kind), ["ack", "terminal"]);
    assert.equal(owner.frees, 0);
    assert.equal(h.send(processor, "commands", 3, { commands }).status, 103);
    assert.equal(owner.calls.enqueue.length, 2);
    const output = planar(2);
    assert.equal(h.process(processor, 0, output), false);
    assertSilent(output);
    assert.equal(owner.calls.render.length, 0);
    assert.equal(notices(processor, "terminal").length, 1);
    h.send(processor, "stop", 10);
    assert.equal(owner.frees, 1);
    assert.equal(processor.port.closed, true);
  }
});

test("stale generations and sequence gaps fence once and only fresh matching stop releases ownership", async () => {
  for (const [fields, sequence, expected] of [[{ generation: 16 }, 2, 102], [{}, 3, 101]]) {
    const h = await harness();
    const processor = h.create();
    h.send(processor, "finish", 1);
    assert.equal(h.send(processor, "poll", sequence, fields).status, expected);
    assert.equal(h.send(processor, "stop", 50, { generation: 16 }).status, 102);
    assert.equal(h.owners[0].frees, 0);
    assert.throws(() => h.create(), /already owns/);
    assert.equal(h.initializations.length, 1);
    assert.equal(notices(processor, "terminal").length, 1);
    assert.equal(h.send(processor, "stop", 50).status, 0);
    assert.equal(h.owners[0].frees, 1);
    const output = planar(1);
    assert.equal(h.process(processor, 0, output), false);
    assertSilent(output);
    const next = h.create();
    h.send(next, "stop", 1);
  }
});

test("memory changes and invalid output layouts fence before copying stale or partial samples", async () => {
  const cases = [
    { before: h => { h.memory.buffer = new ArrayBuffer(8192); }, status: 105, rendered: 0 },
    { faults: { growOnRender: true }, status: 105, rendered: 1 },
    { output: () => [], status: 104, rendered: 0 },
    { output: () => planar(2, 1), status: 104, rendered: 0 },
    { output: () => [[new Float32Array(2).fill(9), new Float32Array(3).fill(9)]], status: 104, rendered: 0 },
    { output: () => planar(6), status: 104, rendered: 0 },
    { frame: Number.MAX_SAFE_INTEGER + 1, status: 107, rendered: 0 },
    { frame: Number.MAX_SAFE_INTEGER, status: 107, rendered: 0 },
  ];
  for (const scenario of cases) {
    const h = await harness(scenario.faults);
    const processor = h.create();
    h.send(processor, "finish", 1);
    h.owners[0].pcm = new Float32Array([0.5, -0.5]);
    scenario.before?.(h);
    const output = scenario.output?.() ?? planar(2);
    assert.equal(h.process(processor, scenario.frame ?? 0, output), false);
    assertSilent(output);
    assert.equal(h.owners[0].calls.render.length, scenario.rendered);
    assert.equal(notices(processor, "terminal")[0].status, scenario.status);
    const repeated = planar(1);
    assert.equal(h.process(processor, 0, repeated), false);
    assertSilent(repeated);
    assert.equal(notices(processor, "terminal").length, 1);
    assert.equal(h.owners[0].frees, 0);
    h.send(processor, "stop", 2);
  }
});

test("numeric render errors and thrown bindings emit one terminal notice without callback cleanup", async () => {
  for (const [faults, status] of [[{ renderStatus: 6 }, 6], [{ renderThrows: true }, 106]]) {
    const h = await harness(faults);
    const processor = h.create();
    h.send(processor, "finish", 1);
    const owner = h.owners[0];
    owner.pcm = new Float32Array([0.75, -0.75]);
    const output = planar(2);
    const acknowledgments = notices(processor, "ack").length;
    assert.equal(h.process(processor, 0, output), false);
    assertSilent(output);
    assert.equal(notices(processor, "ack").length, acknowledgments);
    assert.equal(notices(processor, "terminal")[0].status, status);
    assert.equal(owner.calls.reports.length, 0);
    assert.equal(owner.frees, 0);
    assert.equal(h.process(processor, 2, planar(2)), false);
    assert.equal(owner.calls.render.length, 1);
    assert.equal(notices(processor, "terminal").length, 1);
    h.send(processor, "stop", 2);
    assert.equal(owner.frees, 1);
  }
});

test("finish rejects invalid fixed storage metadata before rendering and preserves cleanup ownership", async () => {
  for (const faults of [{ pointer: 65 }, { pointer: 4092 }, { length: 9 }, { channels: 1 }, { maxFrames: 4 }]) {
    const h = await harness(faults);
    const processor = h.create();
    const ack = h.send(processor, "finish", 1);
    assert.equal(ack.status, 105);
    assert.equal(h.owners[0].calls.finish, 1);
    assert.equal(h.owners[0].calls.render.length, 0);
    assert.equal(h.owners[0].frees, 0);
    const output = planar(2);
    assert.equal(h.process(processor, 0, output), false);
    assertSilent(output);
    h.send(processor, "stop", 2);
    assert.equal(h.owners[0].frees, 1);
  }
});
