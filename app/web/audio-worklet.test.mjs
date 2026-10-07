// Deferred: node --experimental-vm-modules --test app/web/*.test.mjs
// Executes the actual processor against binding spies, without audio or WASM execution.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";
import { readAudioFailureDiagnostics, AUDIO_FAILURE_ORIGIN_CONTROL,
  AUDIO_FAILURE_ORIGIN_ARM, AUDIO_FAILURE_ORIGIN_PROCESS } from "./audio-failure.mjs";

const processorSource = await readFile(new URL("./audio-worklet.js", import.meta.url), "utf8");
const encodingSource = await readFile(new URL("./worklet-encoding.mjs", import.meta.url), "utf8");
const failureSource = await readFile(new URL("./audio-failure.mjs", import.meta.url), "utf8");
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
  let activeProcessor;
  let callbackViews = 0;
  let callbackBigInts = 0;
  class CommandPort {
    constructor() { this.messages = []; this.onmessage = null; this.onmessageerror = null; this.closes = 0; this.starts = 0; }
    postMessage(message) { this.messages.push(structuredClone(message)); if (faults.portPostThrows) throw new Error("port post failed"); }
    start() { this.starts++; }
    close() { assert.equal(inProcess, false, "command port disposal belongs to host control"); this.closes++; if (faults.portCloseThrows) throw new Error("port close failed"); }
    send(message) { this.onmessage?.({ data: message }); }
  }
  class FakeProcessor {
    constructor() {
      this.port = {
        messages: [], onmessage: null, closed: false,
        postMessage(message) {
          this.messages.push(structuredClone(message));
          faults.hostPost?.(message);
        },
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
    arm(...args) { this.calls.arm.push(args); if (faults.armThrows) throw new Error("arm failed"); return faults.armStatus ?? 0; }
    enqueue(...args) {
      this.calls.enqueue.push(args);
      if (this.calls.enqueue.length === faults.enqueueThrowAt) throw new Error("binding admission threw");
      return faults.enqueueStatuses?.[this.calls.enqueue.length - 1] ?? 0;
    }
    render(...args) {
      assert.equal(inProcess, true);
      this.calls.render.push(args);
      const status = faults.renderStatuses?.[this.calls.render.length - 1] ?? faults.renderStatus ?? 0;
      if (faults.renderThrows) throw new Error("render threw");
      new Float32Array(memory.buffer, 64, this.args[1] * this.args[8]).set(this.pcm);
      if (faults.growOnRender) memory.buffer = new ArrayBuffer(8192);
      return status;
    }
    report_word(index, high) {
      this.calls.reports.push([index, high]);
      if (inProcess) {
        assert.equal(activeProcessor.failed, true, "snapshot reads require the first failed callback");
        assert.equal(activeProcessor.diagnosed, true, "first cause is latched before getters");
        assert.ok(index >= 23 && index <= 26, "failure callback reads only chronology scalars");
      }
      if (this.calls.reports.length === faults.reportThrowAt) throw new Error("actual report binding failed");
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
    ArrayBuffer,
    Uint8Array: new Proxy(Uint8Array, { construct(target, args) {
      if (inProcess) callbackViews++;
      return Reflect.construct(target, args);
    } }),
    Uint32Array: new Proxy(Uint32Array, { construct(target, args) {
      if (inProcess) callbackViews++;
      return Reflect.construct(target, args);
    } }),
    Float32Array: new Proxy(Float32Array, { construct(target, args) {
      if (inProcess) callbackViews++;
      return Reflect.construct(target, args);
    } }),
    BigInt(value) { if (inProcess) callbackBigInts++; return BigInt(value); }, WebAssembly,
    AudioWorkletProcessor: FakeProcessor, sampleRate: 48000, currentFrame: 0,
    registerProcessor(name, value) {
      assert.equal(name, "beatkernel-audio");
      assert.equal(Processor, undefined);
      Processor = value;
    },
  });
  const encoding = new SourceTextModule(encodingSource, { context });
  const failure = new SourceTextModule(failureSource, { context });
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
    if (specifier === "./audio-failure.mjs") return failure;
    if (specifier === "./audio-pkg/beatkernel_bms_runtime.js") return bindings;
    throw new Error(`Unexpected processor import: ${specifier}`);
  });
  await actual.evaluate();
  return {
    owners, ports, memory, context, initializations,
    commandPort() { return new CommandPort(); },
    get memoryReads() { return memoryReads; },
    get callbackViews() { return callbackViews; },
    get callbackBigInts() { return callbackBigInts; },
    create(overrides) { return new Processor({ processorOptions: options(overrides) }); },
    send(processor, kind, sequence, fields = {}) {
      assert.equal(typeof processor.port.onmessage, "function");
      processor.port.onmessage({ data: { kind, generation: 17, sequence, ...fields } });
      return processor.port.messages.filter(message => message.kind === "ack").at(-1);
    },
    process(processor, frame, output) {
      context.currentFrame = frame;
      activeProcessor = processor;
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

test("dedicated samples use shared insertion with independent sequence and exact EOS totals while host finish and free remain authoritative", async () => {
  const h = await harness(), processor = h.create({ pcmLimits: { maxSamples: 3 } }), owner = h.owners[0];
  const port = h.commandPort(); // Controlled MessagePort edge, shared with the command fixtures.
  assert.equal(h.send(processor, "attach-samples", 1, { port }).status, 0);
  assert.equal(port.starts, 1); assert.equal(port.messages.length, 0);
  const stale = port.onmessage;
  const inputs = [sample({ id: 18446744073709551615n }), sample({ id: 0n, pcm: new Float32Array(0) }),
    sample({ id: 9007199254740993n, rate: 96000, pcm: new Float32Array([1, -1]) })];
  for (const [index, input] of inputs.entries()) {
    const backing = input.pcm.buffer;
    const message = structuredClone({ kind: "sample", generation: 17, sequence: index + 1, ...input }, { transfer: [backing] });
    assert.throws(() => new Float32Array(backing, 0, 0), TypeError);
    port.send(message);
    assert.deepEqual(port.messages.at(-1), { kind: "ack", generation: 17, sequence: index + 1,
      operation: "sample", status: 0, admitted: 0, error: null, report: null });
  }
  assert.equal(owner.calls.samples.length, 3);
  assert.deepEqual(owner.calls.samples.map(args => [args[0], args[1], args[2], Array.from(args[3])]), [
    [18446744073709551615n, 44100, 2, [0.25, -0.25, 0.5, -0.5]],
    [0n, 44100, 2, []], [9007199254740993n, 96000, 2, [1, -1]],
  ]);
  assert.equal(notices(processor, "ack").length, 1, "direct sample ACKs never use the host lane");
  port.send({ kind: "end-samples", generation: 17, sequence: 4, count: 3, bytes: 24 });
  assert.deepEqual(port.messages.at(-1), { kind: "ack", generation: 17, sequence: 4,
    operation: "end-samples", status: 0, admitted: 0, error: null, report: null });
  assert.equal(port.closes, 1); assert.equal(port.onmessage, null); assert.equal(port.onmessageerror, null);
  assert.equal(owner.frees, 0); assert.equal(owner.calls.finish, 0);
  const replies = port.messages.length;
  stale({ data: { kind: "sample", generation: 17, sequence: 5, ...sample() } });
  assert.equal(port.messages.length, replies); assert.equal(owner.calls.samples.length, 3);
  assert.equal(h.send(processor, "finish", 2, { endFrame: 7n }).status, 0);
  assert.deepEqual(owner.calls.finishAt, [7n]);
  const commands = h.commandPort(); assert.equal(h.send(processor, "attach-commands", 3, { port: commands }).status, 0);
  assert.equal(h.send(processor, "arm", 4, { frame: 11n }).status, 0);
  commands.send({ kind: "commands", generation: 17, sequence: 1, commands: [command()] });
  assert.equal(commands.messages.at(-1).admitted, 1);
  assert.equal(h.send(processor, "stop", 5).status, 0);
  assert.equal(owner.frees, 1); assert.equal(port.closes, 1); assert.equal(commands.closes, 1);
  assert.equal(processor.port.closed, true);
  const replacement = h.create();
  stale({ data: { kind: "sample", generation: 17, sequence: 5, ...sample() } });
  assert.equal(h.owners[1].calls.samples.length, 0); h.send(replacement, "stop", 1);
});

test("direct sample validation and end totals refuse before inappropriate binding calls and fence both lanes", async () => {
  const cases = [
    ...[NaN, Infinity, -Infinity].map(value => ({ input: () => sample({ pcm: new Float32Array([0, value]) }) })),
    { input: () => sample({ id: -1n }) }, { input: () => sample({ rate: 0 }) },
    { input: () => sample({ channels: 1 }) }, { input: () => sample({ pcm: new Float32Array(3) }) },
    { input: () => sample({ pcm: new Float32Array(6) }) },
    { input: () => sample({ pcm: new Float32Array(new ArrayBuffer(16), 8, 2) }) },
    { input: () => sample({ pcm: new Float32Array(new SharedArrayBuffer(8)) }) },
    { input: () => sample({ pcm: new Float32Array(new ArrayBuffer(8, { maxByteLength: 16 })) }) },
    { input: () => { const pcm = new Float32Array(0);
      structuredClone(pcm.buffer, { transfer: [pcm.buffer] }); return sample({ pcm }); } },
    { fields: { generation: 16 } }, { fields: { sequence: 2 } },
    { fields: { sequence: Number.MAX_SAFE_INTEGER + 1 } }, { fields: { kind: "finish" } },
    { fields: { kind: "stop" } }, { fields: { kind: "commands", commands: [command()] } },
    { prefix: true, input: () => sample() }, // Duplicate actual ID.
    { prefix: true, input: () => sample({ id: 2n }) }, // Remaining total is only eight bytes.
    { prefix: true, limits: { maxSamples: 1 }, input: () => sample({ id: 2n, pcm: new Float32Array(0) }) },
    ...[{ count: 0, bytes: 16 }, { count: 1, bytes: 0 }, { count: 2, bytes: 16 },
      { count: 1.5, bytes: 16 }, { count: 1, bytes: 17 }].map(totals => ({ prefix: true,
      fields: { kind: "end-samples", ...totals } })),
  ];
  for (const scenario of cases) {
    const h = await harness(), processor = h.create({ pcmLimits: scenario.limits }), owner = h.owners[0], port = h.commandPort();
    h.send(processor, "attach-samples", 1, { port });
    let sequence = 1;
    if (scenario.prefix) { port.send({ kind: "sample", generation: 17, sequence: sequence++, ...sample() });
      assert.equal(port.messages.at(-1).status, 0); }
    const prior = owner.calls.samples.length;
    port.send({ kind: "sample", generation: 17, sequence, ...(scenario.input?.() ?? sample()), ...scenario.fields });
    const ack = port.messages.filter(message => message.kind === "ack").at(-1);
    assert.notEqual(ack.status, 0); assert.equal(ack.admitted, 0); assert.equal(ack.report, null);
    assert.equal(owner.calls.samples.length, prior); assert.equal(owner.calls.finish, 0); assert.equal(owner.frees, 0);
    assert.equal(notices(processor, "terminal").length, 1);
    assert.equal(port.messages.filter(message => message.kind === "terminal").length, 1);
    port.send({ kind: "end-samples", generation: 17, sequence: sequence + 1, count: prior, bytes: prior * 16 });
    assert.equal(owner.calls.samples.length, prior); assert.equal(owner.calls.finish, 0);
    assert.equal(port.messages.filter(message => message.kind === "terminal").length, 1);
    assert.equal(h.send(processor, "stop", 2).status, 0);
    assert.equal(owner.frees, 1); assert.equal(port.closes, 1); assert.equal(port.onmessage, null);
  }
  // A valid PCM shape can still be refused by the actual binding boundary; end cannot credit it.
  const failed = await harness({ sampleStatus: 7 }), processor = failed.create(), port = failed.commandPort();
  failed.send(processor, "attach-samples", 1, { port });
  port.send({ kind: "sample", generation: 17, sequence: 1, ...sample() });
  assert.equal(port.messages.find(message => message.kind === "ack").status, 7);
  assert.equal(failed.owners[0].calls.samples.length, 1);
  assert.equal(failed.owners[0].frees, 0); failed.send(processor, "stop", 2);
  assert.equal(failed.owners[0].frees, 1);
});

test("sample attachment, premature host finish and endpoint failures preserve exclusive ownership until actual stop", async () => {
  for (const phase of ["host-sample", "finished", "duplicate"]) {
    const h = await harness(), processor = h.create(), owner = h.owners[0]; let sequence = 0;
    if (phase === "host-sample") h.send(processor, "sample", ++sequence, sample({ pcm: new Float32Array(0) }));
    if (phase === "finished") h.send(processor, "finish", ++sequence);
    const adopted = phase === "duplicate" ? h.commandPort() : null;
    if (adopted) assert.equal(h.send(processor, "attach-samples", ++sequence, { port: adopted }).status, 0);
    const refused = h.commandPort();
    assert.notEqual(h.send(processor, "attach-samples", ++sequence, { port: refused }).status, 0);
    assert.equal(refused.closes, 1); assert.equal(owner.frees, 0);
    assert.equal(notices(processor, "terminal").length, 1);
    h.send(processor, "stop", ++sequence); assert.equal(owner.frees, 1);
    if (adopted) assert.equal(adopted.closes, 1);
  }
  for (const action of ["finish", "host-sample", "messageerror", "stop"]) {
    const h = await harness(), processor = h.create(), owner = h.owners[0], port = h.commandPort();
    h.send(processor, "attach-samples", 1, { port }); const stale = port.onmessage;
    if (action === "finish") { assert.notEqual(h.send(processor, "finish", 2).status, 0); assert.equal(owner.calls.finish, 0); }
    else if (action === "host-sample") assert.notEqual(h.send(processor, "sample", 2, sample()).status, 0);
    else if (action === "messageerror") port.onmessageerror({});
    assert.equal(owner.calls.samples.length, 0); assert.equal(owner.frees, 0);
    if (action !== "stop") assert.equal(port.messages.filter(message => message.kind === "terminal").length, 1);
    assert.equal(h.send(processor, "stop", action === "stop" ? 2 : 3).status, 0); assert.equal(owner.frees, 1);
    assert.equal(port.closes, 1); assert.deepEqual(port.messages.at(-1), { kind: "closed", generation: 17 });
    const replacement = h.create(), count = port.messages.length;
    stale({ data: { kind: "end-samples", generation: 17, sequence: 1, count: 0, bytes: 0 } });
    assert.equal(port.messages.length, count); assert.equal(h.owners[1].calls.samples.length, 0);
    h.send(replacement, "stop", 1);
  }
  const h = await harness(), processor = h.create(), port = h.commandPort();
  h.send(processor, "attach-samples", 1, { port });
  port.send({ kind: "end-samples", generation: 17, sequence: 1, count: 0, bytes: 0 });
  assert.equal(port.messages.at(-1).status, 0); assert.equal(port.closes, 1);
  assert.equal(h.send(processor, "finish", 2).status, 0); assert.equal(h.owners[0].calls.finish, 1);
  h.send(processor, "stop", 3); assert.equal(h.owners[0].frees, 1); assert.equal(port.closes, 1);
});

test("dedicated command ownership uses the actual shared enqueue path and independent sequence while host poll and stop remain authoritative", async () => {
  const h = await harness(), processor = h.create(), owner = h.owners[0];
  h.send(processor, "finish", 1);
  const first = command(); assert.equal(h.send(processor, "commands", 2, { commands: [first] }).admitted, 1);
  const port = h.commandPort();
  assert.equal(h.send(processor, "attach-commands", 3, { port }).status, 0);
  assert.equal(port.starts, 1); assert.equal(port.messages.length, 0);
  const controlCount = notices(processor, "ack").length;
  const values = [command({ kind: 1, voice: 0n }), command({ kind: 2, value: -1n, denominator: 3n })];
  port.send({ kind: "commands", generation: 17, sequence: 1, commands: values });
  const ack = port.messages.at(-1);
  assert.equal(ack.kind, "ack"); assert.equal(ack.sequence, 1); assert.equal(ack.operation, "commands");
  assert.equal(ack.status, 0); assert.equal(ack.admitted, 2); assert.equal(ack.report, null);
  assert.equal(notices(processor, "ack").length, controlCount, "command ACKs never return through the host lane");
  assert.deepEqual(owner.calls.enqueue, [first, ...values].map(value => [value.kind, value.voice, value.sample,
    value.at, value.gain, value.value, value.denominator]));
  const poll = h.send(processor, "poll", 4); assert.equal(poll.sequence, 4); assert.equal(poll.report.available, false);
  assert.equal(owner.calls.reports.length, 56);
  assert.equal(h.send(processor, "arm", 5, { frame: 9007199254740993n }).status, 0);
  port.send({ kind: "commands", generation: 17, sequence: 2, commands: [command({ kind: 3 })] });
  assert.equal(port.messages.at(-1).admitted, 1);
  const stale = port.onmessage;
  assert.equal(h.send(processor, "stop", 6).status, 0);
  assert.equal(owner.frees, 1); assert.equal(port.closes, 1);
  assert.deepEqual(port.messages.at(-1), { kind: "closed", generation: 17 });
  assert.equal(port.onmessage, null); assert.equal(port.onmessageerror, null);
  const noticesBefore = port.messages.length;
  stale({ data: { kind: "commands", generation: 17, sequence: 3, commands: [command()] } });
  assert.equal(owner.calls.enqueue.length, 4); assert.equal(port.messages.length, noticesBefore);
  const output = planar(2); assert.equal(h.process(processor, 0, output), false); assertSilent(output);
  const next = h.create(); h.send(next, "finish", 1);
  stale({ data: { kind: "commands", generation: 17, sequence: 3, commands: [command()] } });
  assert.equal(h.owners[1].calls.enqueue.length, 0); h.send(next, "stop", 2);
});

test("command endpoint refusal and actual partial admission fence both lanes once and reserve deallocation for host stop", async () => {
  for (const phase of ["setup", "armed", "duplicate"]) {
    const h = await harness(), processor = h.create(); let sequence = 0;
    if (phase !== "setup") h.send(processor, "finish", ++sequence);
    if (phase === "armed") h.send(processor, "arm", ++sequence, { frame: 0n });
    const existing = phase === "duplicate" ? h.commandPort() : null;
    if (existing) assert.equal(h.send(processor, "attach-commands", ++sequence, { port: existing }).status, 0);
    const refused = h.commandPort();
    assert.notEqual(h.send(processor, "attach-commands", ++sequence, { port: refused }).status, 0);
    assert.equal(refused.closes, 1); assert.equal(h.owners[0].frees, 0);
    assert.equal(notices(processor, "terminal").length, 1);
    h.send(processor, "stop", ++sequence); assert.equal(h.owners[0].frees, 1);
    if (existing) assert.equal(existing.closes, 1);
  }
  const cases = [
    { fields: { generation: 16 }, status: 102 }, { fields: { sequence: 2 }, status: 101 },
    { fields: { sequence: Number.MAX_SAFE_INTEGER + 1 }, status: 101 },
    { fields: { kind: "stop" }, status: 100 }, { fields: { kind: "finish" }, status: 100 },
    { fields: { commands: [command(), command({ extra: true })] }, status: 100 },
    { fields: { commands: [command(), command({ gain: NaN })] }, status: 100 },
    { fields: { commands: Array.from({ length: 5 }, () => command()) }, status: 100 },
    { faults: { enqueueStatuses: [0, 3] }, status: 3, admitted: 1 },
    { faults: { enqueueThrowAt: 2 }, status: 106, admitted: 1 },
    { hostCommands: true, status: 103 }, { messageError: true }, { faults: { renderStatus: 9 }, render: true },
  ];
  for (const scenario of cases) {
    const h = await harness(scenario.faults), processor = h.create(), owner = h.owners[0], port = h.commandPort();
    h.send(processor, "finish", 1); h.send(processor, "attach-commands", 2, { port });
    if (scenario.hostCommands) assert.equal(h.send(processor, "commands", 3, { commands: [command()] }).status, scenario.status);
    else if (scenario.messageError) port.onmessageerror({ type: "messageerror" });
    else if (scenario.render) {
      const output = planar(2); assert.equal(h.process(processor, 0, output), false); assertSilent(output);
    } else {
      port.send({ kind: "commands", generation: 17, sequence: 1,
        commands: [command(), command({ kind: 1 }), command({ kind: 2 })], ...scenario.fields });
      const ack = port.messages.find(message => message.kind === "ack");
      assert.equal(ack.status, scenario.status); assert.equal(ack.admitted, scenario.admitted ?? 0);
      assert.equal(ack.report, null);
      assert.equal(owner.calls.enqueue.length, scenario.admitted ? 2 : 0);
    }
    assert.equal(notices(processor, "terminal").length, 1);
    assert.equal(port.messages.filter(message => message.kind === "terminal").length, 1);
    assert.equal(owner.frees, 0); assert.equal(port.closes, 0, "callback failure does not dispose Rust or port ownership");
    const admittedCalls = owner.calls.enqueue.length;
    port.send({ kind: "commands", generation: 17, sequence: 3, commands: [command()] });
    assert.equal(owner.calls.enqueue.length, admittedCalls);
    assert.equal(port.messages.filter(message => message.kind === "terminal").length, 1);
    h.send(processor, "stop", 20);
    assert.equal(owner.frees, 1); assert.equal(port.closes, 1); assert.equal(processor.port.closed, true);
    assert.deepEqual(port.messages.at(-1), { kind: "closed", generation: 17 });
  }
});

test("host and direct polling read the same actual report words outside process with independent lane sequences", async () => {
  const h = await harness(), processor = h.create(), owner = h.owners[0], port = h.commandPort();
  h.send(processor, "finish", 1); h.send(processor, "attach-commands", 2, { port });
  port.send({ kind: "poll", generation: 17, sequence: 1 });
  const empty = port.messages.at(-1);
  assert.equal(empty.operation, "poll"); assert.equal(empty.sequence, 1);
  assert.equal(empty.status, 0); assert.equal(empty.admitted, 0); assert.equal(empty.error, null);
  assert.equal(empty.report.available, false);
  assert.deepEqual(Array.from(empty.report.words), Array(56).fill(0));
  const hostEmpty = h.send(processor, "poll", 3);
  assert.deepEqual(Array.from(hostEmpty.report.words), Array.from(empty.report.words));
  assert.equal(hostEmpty.sequence, 3); assert.equal(owner.calls.render.length, 0);
  assert.equal(owner.calls.reports.length, 112);
  assert.equal(empty.report.words.byteOffset, 0); assert.equal(empty.report.words.buffer.byteLength, 224);
  const values = [command(), command({ kind: 1 })];
  port.send({ kind: "commands", generation: 17, sequence: 2, commands: values });
  assert.equal(port.messages.at(-1).admitted, 2); assert.equal(port.messages.at(-1).report, null);
  assert.equal(h.send(processor, "arm", 4, { frame: 9007199254740993n }).status, 0);

  owner.pcm = new Float32Array([0.25, -0.25, 0.5, -0.5, 1, -1]);
  const readCount = owner.calls.reports.length, messages = port.messages.length;
  assert.equal(h.process(processor, 3 * 4294967296 + 7, planar(3)), true);
  assert.equal(owner.calls.reports.length, readCount, "process never allocates or reads the control report");
  assert.equal(port.messages.length, messages);
  // Opaque binding values exercise exact word transport, not fake Mixer physics.
  owner.words.set([1, 0, 0xffffffff, 0x80000000, 3, 0, 0xffffffff, 0xffffffff]);
  owner.words[48] = 0x89abcdef; owner.words[49] = 0xfedcba98;
  owner.words[54] = 0xffffffff; owner.words[55] = 0xffffffff;
  port.send({ kind: "poll", generation: 17, sequence: 3 });
  const direct = port.messages.at(-1), host = h.send(processor, "poll", 5);
  assert.equal(direct.operation, "poll"); assert.equal(direct.sequence, 3);
  assert.equal(direct.admitted, 0); assert.equal(direct.report.available, true);
  assert.deepEqual(Array.from(direct.report.words), Array.from(owner.words));
  assert.deepEqual(Array.from(host.report.words), Array.from(direct.report.words));
  assert.equal(owner.calls.reports.length, readCount + 112);
  assert.deepEqual(owner.calls.reports.slice(readCount, readCount + 56),
    Array.from({ length: 28 }, (_, index) => [[index, false], [index, true]]).flat());
  const retained = direct.report.words.slice();
  owner.words[2] = 17;
  port.send({ kind: "commands", generation: 17, sequence: 4, commands: [command({ kind: 3 })] });
  assert.equal(port.messages.at(-1).admitted, 1); assert.equal(port.messages.at(-1).report, null);
  port.send({ kind: "poll", generation: 17, sequence: 5 });
  assert.equal(port.messages.at(-1).report.words[2], 17);
  assert.deepEqual(Array.from(direct.report.words), Array.from(retained), "later polls cannot replace an already sent report");
  assert.equal(owner.calls.enqueue.length, 3);
  assert.equal(notices(processor, "ack").length, 5, "direct command and report ACKs stay off the host lane");
  h.send(processor, "stop", 6);
  assert.equal(owner.frees, 1); assert.equal(port.closes, 1);
});

test("direct report faults preserve zero admission, fence both lanes once and cannot read a freed or replacement owner", async () => {
  const scenarios = [
    { fields: { generation: 16 }, status: 102 },
    { fields: { sequence: 2 }, status: 101 },
    { fields: { kind: "arm" }, status: 100 },
    { faults: { reportThrowAt: 17 }, status: 106, reads: 17 },
    { priorCommand: true, fields: { sequence: 1 }, status: 101 },
    { renderFailure: true, faults: { renderStatus: 9 }, status: 103 },
  ];
  for (const scenario of scenarios) {
    const h = await harness(scenario.faults), processor = h.create(), owner = h.owners[0], port = h.commandPort();
    h.send(processor, "finish", 1); h.send(processor, "attach-commands", 2, { port });
    const stale = port.onmessage;
    if (scenario.priorCommand) {
      port.send({ kind: "commands", generation: 17, sequence: 1, commands: [command()] });
      assert.equal(port.messages.at(-1).admitted, 1);
    }
    if (scenario.renderFailure) {
      const output = planar(1); assert.equal(h.process(processor, 0, output), false); assertSilent(output);
    }
    const before = port.messages.length;
    port.send({ kind: "poll", generation: 17, sequence: scenario.priorCommand ? 2 : 1, ...scenario.fields });
    const ack = port.messages.slice(before).find(message => message.kind === "ack");
    assert.ok(ack); assert.equal(ack.status, scenario.status); assert.equal(ack.admitted, 0);
    assert.equal(ack.report, null, "a partial report read never becomes output evidence");
    assert.equal(owner.calls.reports.length, (scenario.reads ?? 0) + 2);
    assert.deepEqual(owner.calls.reports.slice(scenario.reads ?? 0), [[23, false], [25, false]],
      "only the first failure adds the two absent chronology presence reads");
    assert.equal(owner.calls.enqueue.length, scenario.priorCommand ? 1 : 0);
    assert.equal(port.messages.filter(message => message.kind === "terminal").length, 1);
    assert.equal(notices(processor, "terminal").length, 1);
    assert.equal(owner.frees, 0); assert.equal(port.closes, 0);
    const reads = owner.calls.reports.length;
    port.send({ kind: "poll", generation: 17, sequence: 2 });
    assert.equal(owner.calls.reports.length, reads);
    assert.equal(port.messages.filter(message => message.kind === "terminal").length, 1);
    h.send(processor, "stop", 20);
    assert.equal(owner.frees, 1); assert.equal(port.closes, 1);
    assert.deepEqual(port.messages.at(-1), { kind: "closed", generation: 17 });
    assert.equal(port.onmessage, null); assert.equal(port.onmessageerror, null);
    const count = port.messages.length;
    stale({ data: { kind: "poll", generation: 17, sequence: 3 } });
    assert.equal(owner.calls.reports.length, reads); assert.equal(port.messages.length, count);
    const replacement = h.create(); h.send(replacement, "finish", 1);
    stale({ data: { kind: "poll", generation: 17, sequence: 4 } });
    assert.equal(h.owners[1].calls.reports.length, 0);
    h.send(replacement, "stop", 2); assert.equal(h.owners[1].frees, 1);
  }
});

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

test("transferred NaN and both infinities reject before Rust insertion and leave deallocation to actual host stop", async () => {
  for (const value of [NaN, Infinity, -Infinity]) {
    const h = await harness();
    const processor = h.create(), owner = h.owners[0];
    assert.equal(h.send(processor, "sample", 1, sample()).status, 0);
    const pcm = new Float32Array([0.25, value]);
    const transferred = structuredClone(sample({ id: 2n, pcm }), { transfer: [pcm.buffer] });
    assert.equal(pcm.byteLength, 0);
    const ack = h.send(processor, "sample", 2, transferred);
    assert.equal(ack.operation, "sample"); assert.equal(ack.generation, 17); assert.equal(ack.sequence, 2);
    assert.equal(ack.status, 100); assert.equal(ack.admitted, 0); assert.equal(ack.error, "sample-pcm");
    assert.equal(owner.calls.samples.length, 1, "the binding saw only the earlier admitted finite sample");
    assert.deepEqual(Array.from(owner.calls.samples[0][3]), [0.25, -0.25, 0.5, -0.5]);
    assert.equal(owner.frees, 0);
    assert.equal(notices(processor, "terminal").length, 1);
    assert.equal(notices(processor, "terminal")[0].status, 100);
    assert.notEqual(h.send(processor, "finish", 3).status, 0);
    assert.equal(owner.calls.finish, 0);
    assert.notEqual(h.send(processor, "sample", 4, sample({ id: 2n, pcm: new Float32Array(0) })).status, 0);
    assert.equal(owner.calls.samples.length, 1, "fencing prevents retry even with a fresh valid buffer");
    const silent = planar(2);
    assert.equal(h.process(processor, 0, silent), false); assertSilent(silent);
    assert.equal(owner.calls.render.length, 0); assert.equal(owner.frees, 0);
    assert.equal(notices(processor, "terminal").length, 1);
    assert.equal(h.send(processor, "stop", 5).status, 0);
    assert.equal(owner.frees, 1); assert.equal(processor.port.closed, true);
    assert.equal(processor.port.onmessage, null);

    // Cleanup releases the singleton; a new owner can accept empty PCM and
    // genuine finite data under the original exact per-asset/aggregate limits.
    const next = h.create(), replacement = h.owners[1];
    const empty = new Float32Array(0);
    assert.equal(h.send(next, "sample", 1, sample({ pcm: empty })).status, 0);
    assert.equal(replacement.calls.samples[0][3], empty);
    assert.equal(h.send(next, "sample", 2, sample({ id: 2n })).status, 0);
    assert.equal(replacement.calls.samples.length, 2);
    assert.equal(h.send(next, "finish", 3).status, 0);
    assert.equal(h.send(next, "stop", 4).status, 0);
    assert.equal(replacement.frees, 1);
  }
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
    assert.deepEqual(owner.calls.reports, [[23, false], [25, false]]);
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

function chronology(owner, { expected = null, start = null } = {}) {
  for (const [presence, pair, value] of [[23, 24, expected], [25, 26, start]]) {
    owner.words[presence * 2] = value === null ? 0 : 1;
    owner.words[pair * 2] = value?.[0] ?? 0;
    owner.words[pair * 2 + 1] = value?.[1] ?? 0;
  }
}

function facts(processor) { return readAudioFailureDiagnostics(notices(processor, "terminal")[0]); }

test("mocked native chronology gap repeat and backwards preserve exact failing grid and retained high words", async () => {
  const base = 3 * 4294967296 + 7;
  for (const failedFrame of [base + 5, base, base - 1]) {
    const h = await harness({ renderStatuses: [0, 6] }), processor = h.create(), owner = h.owners[0];
    const terminal = processor.terminal;
    h.send(processor, "finish", 1);
    h.context.currentFrame = base - 10;
    h.send(processor, "arm", 2, { frame: BigInt(base) });
    chronology(owner, { expected: [0xffffffff, 0xffffffff], start: [0, 0] });
    assert.equal(h.process(processor, base, planar(3)), true);
    assert.equal(owner.calls.reports.length, 0, "successful callback cannot poll chronology");
    // These are retained native scalar facts, not a JS-generated cursor.
    chronology(owner, { expected: [10, 3], start: [7, 3] });
    const output = planar(2);
    assert.equal(h.process(processor, failedFrame, output), false); assertSilent(output);
    assert.equal(processor.terminal, terminal);
    assert.deepEqual(facts(processor), {
      diagnosticVersion: 1, origin: AUDIO_FAILURE_ORIGIN_PROCESS, ownerPhase: 2,
      currentFramePresent: 1, currentFrame: failedFrame, blockFramesPresent: 1, blockFrames: 2,
      expectedFramePresent: 1, expectedFrameLow: 10, expectedFrameHigh: 3,
      startFramePresent: 1, startFrameLow: 7, startFrameHigh: 3,
      successfulArmFramePresent: 1, successfulArmFrame: base - 10,
    });
    assert.deepEqual(owner.calls.reports, [[23, false], [24, false], [24, true], [25, false], [26, false], [26, true]]);
    assert.equal(h.callbackViews, 0); assert.equal(h.callbackBigInts, 0); assert.equal(owner.frees, 0);
    const original = structuredClone(terminal);
    chronology(owner, { expected: [0xffffffff, 0xffffffff], start: [0, 0] });
    assert.equal(h.process(processor, failedFrame + 99, planar(1)), false);
    assert.equal(h.send(processor, "arm", 3, { frame: 0n }).status, 103);
    h.send(processor, "stop", 4);
    assert.equal(processor.terminal, terminal);
    assert.deepEqual(structuredClone(terminal), original); assert.equal(notices(processor, "terminal").length, 1);
  }
});

test("native diagnostic zero is present and absence never borrows stale low high words", async () => {
  for (const present of [false, true]) {
    const h = await harness({ renderStatus: 6 }), processor = h.create(), owner = h.owners[0];
    h.send(processor, "finish", 1);
    chronology(owner, present ? { expected: [0, 0], start: [0, 0] } : {});
    if (!present) { owner.words[48] = 99; owner.words[49] = 8; owner.words[52] = 77; owner.words[53] = 9; }
    assert.equal(h.process(processor, 0, planar(0)), false);
    const diagnostic = facts(processor);
    assert.equal(diagnostic.currentFramePresent, 1); assert.equal(diagnostic.currentFrame, 0);
    assert.equal(diagnostic.blockFramesPresent, 1); assert.equal(diagnostic.blockFrames, 0);
    assert.equal(diagnostic.expectedFramePresent, Number(present)); assert.equal(diagnostic.startFramePresent, Number(present));
    assert.equal(diagnostic.expectedFrameLow, 0); assert.equal(diagnostic.expectedFrameHigh, 0);
    assert.equal(diagnostic.startFrameLow, 0); assert.equal(diagnostic.startFrameHigh, 0);
    assert.equal(diagnostic.successfulArmFramePresent, 0); assert.equal(diagnostic.successfulArmFrame, 0);
    h.send(processor, "stop", 2);
  }
});

test("each failed chronology getter retains completed prior facts and original native status", async () => {
  for (let reportThrowAt = 1; reportThrowAt <= 6; reportThrowAt++) {
    const h = await harness({ renderStatus: 6, reportThrowAt }), processor = h.create(), owner = h.owners[0];
    h.send(processor, "finish", 1);
    chronology(owner, { expected: [0xabcdef01, 0xffffffff], start: [0x76543210, 0x80000001] });
    assert.equal(h.process(processor, 123, planar(2)), false);
    assert.equal(notices(processor, "terminal")[0].status, 6);
    const diagnostic = facts(processor), expectedPresent = Number(reportThrowAt > 3);
    assert.equal(diagnostic.currentFrame, 123); assert.equal(diagnostic.blockFrames, 2);
    assert.equal(diagnostic.expectedFramePresent, expectedPresent);
    assert.equal(diagnostic.expectedFrameLow, expectedPresent ? 0xabcdef01 : 0);
    assert.equal(diagnostic.expectedFrameHigh, expectedPresent ? 0xffffffff : 0);
    assert.equal(diagnostic.startFramePresent, 0); assert.equal(diagnostic.startFrameLow, 0); assert.equal(diagnostic.startFrameHigh, 0);
    assert.equal(owner.calls.reports.length, reportThrowAt);
    h.send(processor, "stop", 2);
  }
});

test("reentrant failed ACK posting preserves first cause admitted prefix and the same diagnostic payload", async () => {
  const faults = { enqueueStatuses: [0, 3] }, h = await harness(faults), processor = h.create(), owner = h.owners[0];
  h.send(processor, "finish", 1);
  chronology(owner, { expected: [0xabcdef01, 0x80000001], start: [0, 0] });
  const terminal = processor.terminal;
  let reentered = false;
  faults.hostPost = message => {
    if (message.kind === "ack" && message.status === 3 && !reentered) {
      reentered = true;
      assert.equal(processor.failed, true); assert.equal(processor.diagnosed, true);
      assert.equal(message.diagnostics, terminal);
      chronology(owner, { expected: [99, 99], start: [99, 99] });
      processor.fence(106);
    }
  };
  const messagesBefore = processor.port.messages.length;
  const ack = h.send(processor, "commands", 2, { commands: [command(), command(), command()] });
  assert.equal(reentered, true); assert.equal(ack.status, 3); assert.equal(ack.admitted, 1);
  assert.equal(owner.calls.enqueue.length, 2);
  assert.deepEqual(processor.port.messages.slice(messagesBefore).map(message => message.kind), ["ack", "terminal"]);
  assert.deepEqual(ack.diagnostics, notices(processor, "terminal")[0]);
  assert.equal(ack.diagnostics.status, 3);
  assert.equal(ack.diagnostics.expectedFrameLow, 0xabcdef01); assert.equal(ack.diagnostics.expectedFrameHigh, 0x80000001);
  assert.equal(ack.diagnostics.startFramePresent, 1); assert.equal(ack.diagnostics.startFrameLow, 0);
  const original = structuredClone(terminal);
  assert.equal(h.send(processor, "stop", 3).status, 0);
  assert.equal(processor.terminal, terminal);
  assert.deepEqual(structuredClone(terminal), original);
  assert.equal(Object.hasOwn(notices(processor, "ack").at(-1), "diagnostics"), false, "successful ACK carries no failure object");
});

test("control and arm failures preserve operation origin phase and only validated actual arm frames", async () => {
  for (const scenario of [
    { kind: "finish", faults: { finishStatus: 9 }, phase: 0, origin: AUDIO_FAILURE_ORIGIN_CONTROL, status: 9, current: false },
    { kind: "arm", faults: { armStatus: 6 }, phase: 1, origin: AUDIO_FAILURE_ORIGIN_ARM, status: 6, current: true },
    { kind: "arm", faults: { armThrows: true }, phase: 1, origin: AUDIO_FAILURE_ORIGIN_ARM, status: 106, current: true },
    { kind: "arm", frame: -1, faults: {}, phase: 1, origin: AUDIO_FAILURE_ORIGIN_ARM, status: 100, current: false },
  ]) {
    const h = await harness(scenario.faults), processor = h.create();
    let sequence = 1;
    if (scenario.kind === "arm") h.send(processor, "finish", sequence++);
    h.context.currentFrame = scenario.frame ?? 4294967305;
    assert.equal(h.send(processor, scenario.kind, sequence++, { frame: 0n }).status, scenario.status);
    const diagnostic = facts(processor);
    assert.equal(diagnostic.origin, scenario.origin); assert.equal(diagnostic.ownerPhase, scenario.phase);
    assert.equal(diagnostic.currentFramePresent, Number(scenario.current));
    assert.equal(diagnostic.currentFrame, scenario.current ? 4294967305 : 0);
    assert.equal(diagnostic.blockFramesPresent, 0); assert.equal(diagnostic.blockFrames, 0);
    assert.equal(diagnostic.successfulArmFramePresent, 0); assert.equal(diagnostic.successfulArmFrame, 0);
    h.send(processor, "stop", sequence);
  }
});

test("first payload reaches every owned port despite posting cleanup and failed-stop errors", async () => {
  const faults = {}, h = await harness(faults), processor = h.create(), owner = h.owners[0];
  const samples = h.commandPort(), commands = h.commandPort();
  h.send(processor, "attach-samples", 1, { port: samples });
  // Host premature finish fails while sample endpoint remains owned.
  h.send(processor, "finish", 2);
  const sampleTerminal = processor.terminal;
  const original = structuredClone(sampleTerminal);
  assert.deepEqual(samples.messages.find(message => message.kind === "terminal"), original);
  faults.portCloseThrows = true;
  assert.equal(h.send(processor, "stop", 3).status, 106);
  assert.equal(processor.terminal, sampleTerminal);
  assert.deepEqual(structuredClone(processor.terminal), original); assert.equal(notices(processor, "terminal").length, 1);
  assert.equal(owner.frees, 1);
  faults.portCloseThrows = false;
  h.send(processor, "stop", 4);

  const next = h.create(), nextOwner = h.owners[1];
  h.send(next, "finish", 1); h.send(next, "attach-commands", 2, { port: commands });
  faults.renderStatus = 6; faults.portPostThrows = true;
  chronology(nextOwner, { expected: [0, 8], start: [9, 7] });
  assert.equal(h.process(next, 4294967300, planar(2)), false);
  const nextTerminal = next.terminal;
  const captured = structuredClone(nextTerminal);
  assert.deepEqual(commands.messages.find(message => message.kind === "terminal"), captured);
  assert.deepEqual(notices(next, "terminal")[0], captured);
  faults.portCloseThrows = true;
  h.send(next, "stop", 3);
  assert.equal(next.terminal, nextTerminal);
  assert.deepEqual(structuredClone(next.terminal), captured); assert.equal(notices(next, "terminal").length, 1);
  faults.portPostThrows = false; faults.portCloseThrows = false;
  h.send(next, "stop", 4);
});

test("layout and memory failures retain known callback facts without inventing missing block extent", async () => {
  for (const scenario of [
    { output: () => [], block: null, status: 104 },
    { output: () => planar(2, 1), block: 2, status: 104 },
    { output: () => [[new Float32Array(2), new Float32Array(3)]], block: 2, status: 104 },
    { output: () => planar(6), block: 6, status: 104 },
    { output: () => planar(2), before: h => { h.memory.buffer = new ArrayBuffer(8192); }, block: 2, status: 105 },
  ]) {
    const h = await harness(), processor = h.create(); h.send(processor, "finish", 1);
    scenario.before?.(h);
    assert.equal(h.process(processor, 77, scenario.output()), false);
    assert.equal(notices(processor, "terminal")[0].status, scenario.status);
    const diagnostic = facts(processor);
    assert.equal(diagnostic.origin, AUDIO_FAILURE_ORIGIN_PROCESS); assert.equal(diagnostic.ownerPhase, 1);
    assert.equal(diagnostic.currentFramePresent, 1); assert.equal(diagnostic.currentFrame, 77);
    assert.equal(diagnostic.blockFramesPresent, Number(scenario.block !== null)); assert.equal(diagnostic.blockFrames, scenario.block ?? 0);
    assert.equal(h.owners[0].calls.render.length, 0); assert.equal(h.owners[0].frees, 0);
    assert.deepEqual(h.owners[0].calls.reports, [[23, false], [25, false]]);
    h.send(processor, "stop", 2);
  }
});
