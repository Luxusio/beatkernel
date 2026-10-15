// Software-only port/binding contract tests. These do not prove browser audio deadlines,
// native presentation, or physical sound. AC010 supplies the actual browser evidence.
// Coordinator verification: node --experimental-vm-modules --test app/web/runtime-rate-worklet.test.mjs
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";

const sources = await Promise.all([
  "audio-worklet.js", "worklet-encoding.mjs", "audio-failure.mjs",
  "native-audio-clock.mjs", "worklet-native-frame-probe.mjs",
].map(name => readFile(new URL(name, import.meta.url), "utf8")));
const emptyModule = new WebAssembly.Module(Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0));
const PROGRAM = 9007199254740993n;
const SAMPLE = 9007199254740995n;
const CLOCK_NAMES = ["__beatkernel_clock_marker", "__beatkernel_clock_0", "__beatkernel_clock_1", "__beatkernel_clock_2"];
const PERIODS = [1048576, 1048575, 1048573];

// Independent literal producer payloads. No product encoder/phase implementation
// computes the expected values in this file.
function setupWords() {
  return Uint32Array.of(
    1, 0, 1, 2097152, 4294967286, 4294967295, 1000000000, 0,
    0, 0, 3, 0, 2, 0, 0, 0, 0, 0, 2, 0, 1, 0,
    2, 0, 4, 0, 4, 0, 4, 0, 8, 0,
  );
}
function cueWords(index = 0) {
  return index === 0
    ? Uint32Array.of(4294967295, 4294967295, 3, 2097152, 4294967286, 4294967295, 0x3f000000, 0, 0)
    : Uint32Array.of(1, 2097152, 3, 2097152, 20, 0, 0xbf800000, 0, 0);
}
function preparationWords() {
  // Opaque, fixed producer preparation. Its checksum/geometry are Rust's admission
  // responsibility; these tests assert scalar transport, including both high words.
  return Uint32Array.of(2, 0, 1, 2097152, 48000, 0, 2, 0, 5, 0,
    4, 0, 2, 0, 0x89abcdef, 0x12345678, 2, 0, 0xfedcba98, 0x76543210);
}
const STAGED = [
  [0, 2, 0], [1, 1, 2097152], [2, 48000, 0], [3, 2, 0], [4, 5, 0],
  [5, 4, 0], [6, 2, 0], [7, 0x89abcdef, 0x12345678], [8, 2, 0], [9, 0xfedcba98, 0x76543210],
];
function pauseEventWords() {
  const words = new Uint32Array(80);
  words.set([1, 0, 4, 0, 1, 2097152, 5, 1, 17, 2097152, 7, 0]);
  words.set([4, 0, 3, 0], 14);
  words.set([11, 0, 1, 0], 70);
  return words;
}
function sample() {
  return { id: SAMPLE, rate: 44100, channels: 2, pcm: Float32Array.of(0.25, -0.25, 0.5, -0.5) };
}
function nativeParameters(frame, frames, origin = 5) {
  const parameters = { [CLOCK_NAMES[0]]: Float32Array.from({ length: frames }, (_, i) => Number(frame + i >= origin)) };
  for (let lane = 0; lane < 3; lane++) {
    const period = PERIODS[lane];
    parameters[CLOCK_NAMES[lane + 1]] = Float32Array.from({ length: frames }, (_, i) => {
      const position = frame + i;
      if (position < origin) return 0;
      const cycle = Math.floor((position - origin) / period);
      const from = (origin + cycle * period - 0.25) / 48000;
      const to = (origin + (cycle + 1) * period - 0.25) / 48000;
      return 1 + period * (position / 48000 - from) / (to - from);
    });
  }
  return parameters;
}

async function harness(faults = {}) {
  let Processor, inProcess = false, nativeFrame = 0, candidateCalls = 0;
  let callbackViews = 0, callbackBigInts = 0;
  const owners = [], memory = { buffer: new ArrayBuffer(4096) };
  class Port {
    constructor() { this.messages = []; this.starts = 0; this.closes = 0; this.onmessage = null; this.onmessageerror = null; }
    postMessage(message) { this.messages.push(structuredClone(message)); }
    start() { this.starts++; }
    close() { assert.equal(inProcess, false); this.closes++; }
    send(message) {
      const before = this.messages.length;
      this.onmessage?.({ data: message });
      return this.messages.slice(before).find(item => item.kind === "ack");
    }
  }
  class FakeProcessor { constructor() { this.port = new Port(); } }
  class BrowserAudio {
    constructor(...args) {
      this.args = args; this.calls = []; this.frees = 0; this.currentStatus = 0;
      this.report = new Uint32Array(58);
      this.report.set([1, 0]); this.report.set([1, 0, 11, 0], 50);
      this.metadata = Uint32Array.of(7, 0, 3, 0, 3, 0, 2, 0, 1, 0, 1, 0);
      // Numeric history API fields are sourceHz, initial physical/playback,
      // retained start, pulled end, retirement revision, configured capacity.
      this.history = Uint32Array.of(48000, 0, 0, 0, 0, 0, 0, 0, 5, 0, 2, 0, 8, 0);
      this.spans = [Uint32Array.of(0, 0, 1, 0, 2, 0, 0, 0, 0, 0),
        Uint32Array.of(2, 0, 0, 0, 0, 0, 0, 0, 1, 0)];
      this.event = null;
      for (const name of faults.missingExports ?? []) this[name] = undefined;
      owners.push(this);
    }
    get report_fields() { return 29; }
    status() { return this.currentStatus; }
    record(name, args) {
      assert.equal(inProcess, false, `${name} belongs to message control`);
      this.calls.push([name, ...args]);
      if (faults.throwMethod === name) throw new Error("binding fault");
      const values = faults[`${name}Statuses`];
      this.currentStatus = values?.shift() ?? faults[`${name}Status`] ?? 0;
      return this.currentStatus;
    }
    insert_sample(...args) { return this.record("insert_sample", args); }
    retained_setup(...args) { return this.record("retained_setup", args); }
    retained_cue(...args) { return this.record("retained_cue", args); }
    finish_retained(...args) { return this.record("finish_retained", args); }
    finish(...args) { return this.record("finish", args); }
    finish_at(...args) { return this.record("finish_at", args); }
    retained_preparation_word(...args) { return this.record("retained_preparation_word", args); }
    retained_rate(...args) { return this.record("retained_rate", args); }
    retained_region(...args) { return this.record("retained_region", args); }
    retained_disable_loop(...args) { return this.record("retained_disable_loop", args); }
    retained_restart_original(...args) { return this.record("retained_restart_original", args); }
    retained_pause(...args) { return this.record("retained_pause", args); }
    retained_event_ack(...args) {
      const status = this.record("retained_event_ack", args);
      if (status === 0) this.event = null;
      return status;
    }
    retained_history_retire(...args) { return this.record("retained_history_retire", args); }
    retained_metadata_word(index, high) {
      this.record("retained_metadata_word", [index, high]);
      return this.metadata[2 * index + Number(high)];
    }
    retained_history_len() {
      this.record("retained_history_len", []);
      return faults.historyCount ?? this.spans.length;
    }
    retained_history_word(index, high) {
      this.record("retained_history_word", [index, high]);
      return this.history[2 * index + Number(high)];
    }
    retained_history_span_word(span, index, high) {
      this.record("retained_history_span_word", [span, index, high]);
      return this.spans[span]?.[2 * index + Number(high)];
    }
    retained_event_poll() {
      this.record("retained_event_poll", []);
      return faults.eventPoll ?? Number(this.event !== null);
    }
    retained_event_word(index) { this.record("retained_event_word", [index]); return this.event?.[index]; }
    report_word(index, high) {
      assert.equal(inProcess, false, "actual snapshots belong to control, not render");
      this.calls.push(["report_word", index, high]);
      return this.report[2 * index + Number(high)];
    }
    channels() { return this.args[1]; }
    max_frames() { return this.args[8]; }
    output_ptr() { return 64; }
    output_len() { return this.args[1] * this.args[8]; }
    native_clock_start() { candidateCalls = 0; return 0; }
    native_clock_extend() { return 0; }
    native_clock_ready() { return candidateCalls >= 2; }
    native_frame() { assert.equal(inProcess, true); candidateCalls++; return nativeFrame; }
    arm_words(...args) { assert.equal(inProcess, true); this.calls.push(["arm_words", ...args]); return 0; }
    render(...args) {
      assert.equal(inProcess, true); this.calls.push(["render", ...args]);
      new Float32Array(memory.buffer, 64, 10).set([0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 1, -1, 0, 0]);
      return 0;
    }
    enqueue(...args) { return this.record("enqueue", args); }
    free() { assert.equal(inProcess, false); this.frees++; }
    static memory() { return memory; }
  }
  const monitored = type => new Proxy(type, { construct(target, args) {
    if (inProcess) callbackViews++;
    return Reflect.construct(target, args);
  } });
  // Preserve the standard BigInt signed/unsigned conversion API while counting
  // every callback conversion. The VM must model the actual JavaScript built-in.
  const countedBigInt = Object.assign(value => {
    if (inProcess) callbackBigInts++;
    return BigInt(value);
  }, {
    asIntN(bits, value) { if (inProcess) callbackBigInts++; return BigInt.asIntN(bits, value); },
    asUintN(bits, value) { if (inProcess) callbackBigInts++; return BigInt.asUintN(bits, value); },
  });
  const context = createContext({ ArrayBuffer, Uint8Array: monitored(Uint8Array),
    Uint32Array: monitored(Uint32Array), Float32Array: monitored(Float32Array),
    BigInt: countedBigInt,
    WebAssembly, AudioWorkletProcessor: FakeProcessor, sampleRate: 48000, currentFrame: 0,
    registerProcessor(name, value) { assert.equal(name, "beatkernel-audio"); Processor = value; },
  });
  const [processorSource, encodingSource, failureSource, clockSource, probeSource] = sources;
  const encoding = new SourceTextModule(encodingSource, { context });
  const failure = new SourceTextModule(failureSource, { context });
  const clock = new SourceTextModule(clockSource, { context });
  const probe = new SourceTextModule(probeSource, { context });
  await probe.link(() => { throw new Error("unexpected probe import"); });
  await clock.link(name => { assert.equal(name, "./worklet-native-frame-probe.mjs"); return probe; });
  const bindings = new SyntheticModule(["initSync", "BrowserAudio"], function () {
    this.setExport("initSync", () => {}); this.setExport("BrowserAudio", BrowserAudio);
  }, { context });
  const actual = new SourceTextModule(processorSource, { context });
  await actual.link(name => {
    const modules = { "./worklet-encoding.mjs": encoding, "./audio-failure.mjs": failure,
      "./native-audio-clock.mjs": clock, "./worklet-native-frame-probe.mjs": probe,
      "./audio-pkg/beatkernel_bms_runtime.js": bindings };
    assert.ok(modules[name], `unexpected worklet import ${name}`); return modules[name];
  });
  await actual.evaluate();
  let hostSequence = 0, sampleSequence = 0, commandSequence = 0;
  const processor = new Processor({ processorOptions: { module: emptyModule, generation: 17, channels: 2,
    pcmLimits: { maxAssetBytes: 16, maxTotalBytes: 32, maxSamples: 2 },
    audioLimits: { queueCapacity: 4, maxVoices: 2, pendingCapacity: 4, maxFrames: 5, maxCommandsPerRender: 4 } } });
  const api = {
    processor, owner: owners[0], ports: {},
    host(kind, fields = {}) {
      const sequence = ++hostSequence;
      processor.port.onmessage?.({ data: { kind, generation: 17, sequence, ...fields } });
      return processor.port.messages.filter(message => message.kind === "ack" && message.sequence === sequence).at(-1);
    },
    attachSamples() { this.ports.samples = new Port(); assert.equal(this.host("attach-samples", { port: this.ports.samples }).status, 0); },
    attachCommands() { this.ports.commands = new Port(); assert.equal(this.host("attach-commands", { port: this.ports.commands }).status, 0); },
    sample(kind, fields = {}) {
      const message = kind.startsWith("retained-")
        ? { kind, version: 1, generation: 17, sequence: ++sampleSequence, programId: PROGRAM, ...fields }
        : { kind, generation: 17, sequence: ++sampleSequence, ...fields };
      return this.ports.samples.send(message);
    },
    command(kind, fields = {}) {
      return this.ports.commands.send({ kind, version: 1, generation: 17,
        sequence: ++commandSequence, programId: PROGRAM, ...fields });
    },
    upload({ end = true, finish = true } = {}) {
      this.attachSamples();
      assert.equal(this.sample("retained-setup", { words: setupWords() }).status, 0);
      assert.equal(this.sample("sample", sample()).status, 0);
      assert.equal(this.sample("retained-cue", { index: 0, words: cueWords() }).status, 0);
      assert.equal(this.sample("retained-cue", { index: 1, words: cueWords(1) }).status, 0);
      if (end) assert.equal(this.sample("end-samples", { count: 1, bytes: 16 }).status, 0);
      if (finish) assert.equal(this.host("finish", { mode: "retained" }).status, 0);
      return this;
    },
    process(frame, frames) {
      context.currentFrame = frame; nativeFrame = frame;
      const output = [[new Float32Array(frames).fill(9), new Float32Array(frames).fill(9)]];
      const parameters = nativeParameters(frame, frames);
      inProcess = true;
      try { return { alive: processor.process([], output, parameters), output }; }
      finally { inProcess = false; }
    },
    certify() {
      assert.equal(this.process(0, 5).alive, true);
      const geometry = { origin: 5, h0: 3145733, h1: 3145730, h2: 3145724 };
      processor.port.onmessage({ data: { kind: "clock-reserve", generation: 17,
        clockVersion: 1, clockId: 1, revision: 0, ...geometry } });
      processor.port.onmessage({ data: { kind: "clock-commit", generation: 17,
        clockVersion: 1, clockId: 1, revision: 0, ...geometry, futureEligible: true } });
      assert.equal(this.process(3, 5).alive, true);
      assert.equal(this.process(8, 2).alive, true);
      assert.equal(processor.port.messages.filter(message => message.kind === "clock-ready").length, 1);
    },
    get callbackViews() { return callbackViews; },
    get callbackBigInts() { return callbackBigInts; },
  };
  return api;
}
function calls(h, name) { return h.owner.calls.filter(call => call[0] === name).map(call => call.slice(1)); }
function ackBase(operation, sequence, extra = {}) {
  return { kind: "ack", version: 1, generation: 17, sequence, operation, programId: PROGRAM,
    status: 0, admitted: 0, error: null, terminal: false, report: null, retained: null, ...extra };
}
function assertTerminal(h, acknowledgement) {
  assert.ok(acknowledgement, "the failing port request receives a correlated ACK");
  assert.notEqual(acknowledgement.status, 0);
  assert.equal(acknowledgement.terminal, true);
  assert.equal(h.processor.failed, true);
  assert.equal(h.processor.port.messages.filter(message => message.kind === "terminal").length, 1);
}

test("retained startup owns actual setup, samples, ordered original cues, EOF and retained finish", async () => {
  const h = await harness(); h.attachSamples();
  assert.deepEqual(h.sample("retained-setup", { words: setupWords() }), ackBase("retained-setup", 1));
  assert.equal(h.sample("sample", sample()).status, 0);
  assert.deepEqual(h.sample("retained-cue", { index: 0, words: cueWords() }), ackBase("retained-cue", 3));
  assert.deepEqual(h.sample("retained-cue", { index: 1, words: cueWords(1) }), ackBase("retained-cue", 4));
  assert.equal(calls(h, "finish_retained").length, 0);
  assert.equal(h.sample("end-samples", { count: 1, bytes: 16 }).status, 0);
  assert.equal(h.ports.samples.closes, 1);
  assert.equal(h.host("finish", { mode: "retained" }).status, 0);
  assert.deepEqual(h.owner.calls.filter(call => ["retained_setup", "insert_sample", "retained_cue", "finish_retained"].includes(call[0])).map(call => call[0]),
    ["retained_setup", "insert_sample", "retained_cue", "retained_cue", "finish_retained"]);
  assert.deepEqual(Array.from(calls(h, "retained_setup")[0][0]), [
    1, 0, 1, 2097152, 4294967286, 4294967295, 1000000000, 0,
    0, 0, 3, 0, 2, 0, 0, 0, 0, 0, 2, 0, 1, 0, 2, 0, 4, 0, 4, 0, 4, 0, 8, 0,
  ]);
  assert.deepEqual(calls(h, "retained_cue"), [
    [0, 18446744073709551615n, SAMPLE, -10n, 0.5], [1, PROGRAM, SAMPLE, 20n, -1],
  ]);
  assert.deepEqual(calls(h, "insert_sample").map(args => [...args.slice(0, 3), Array.from(args[3])]),
    [[SAMPLE, 44100, 2, [0.25, -0.25, 0.5, -0.5]]]);
  assert.deepEqual(calls(h, "finish_retained"), [[]]);
  assert.deepEqual(calls(h, "finish"), []); assert.deepEqual(calls(h, "finish_at"), []);
  h.host("stop"); assert.equal(h.owner.frees, 1);
});

test("retained finish requires actual sample EOF, explicit mode and no fixed endpoint", async () => {
  for (const scenario of [
    { end: false, fields: { mode: "retained" } },
    { end: true, fields: {} }, { end: true, fields: { mode: "plain" } },
    { end: true, fields: { mode: "retained", endFrame: 7n } },
    { end: true, fields: { mode: "unknown" } },
  ]) {
    const h = await harness(); h.upload({ end: scenario.end, finish: false });
    const ack = h.host("finish", scenario.fields);
    assert.notEqual(ack.status, 0); assert.equal(h.processor.failed, true);
    assert.deepEqual(calls(h, "finish_retained"), []);
    assert.deepEqual(calls(h, "finish"), []); assert.deepEqual(calls(h, "finish_at"), []);
  }
  const plain = await harness();
  assert.notEqual(plain.host("finish", { mode: "retained" }).status, 0);
  assert.deepEqual(calls(plain, "finish_retained"), []);
});

test("legacy direct sample, finite finish, command admission and poll keep their wire and numeric API", async () => {
  const h = await harness(); assert.equal(h.host("sample", sample()).status, 0);
  assert.equal(h.host("finish", { endFrame: 7n }).status, 0); h.attachCommands();
  const acknowledgement = h.ports.commands.send({ kind: "commands", generation: 17, sequence: 1,
    commands: [{ kind: 0, voice: 1n, sample: SAMPLE, at: -1n, gain: 0.5, value: 0n, denominator: 1n }] });
  assert.deepEqual(acknowledgement, { kind: "ack", generation: 17, sequence: 1, operation: "commands",
    status: 0, admitted: 1, error: null, report: null });
  assert.deepEqual(calls(h, "finish_at"), [[7n]]);
  assert.deepEqual(calls(h, "enqueue"), [[0, 1n, SAMPLE, -1n, 0.5, 0n, 1n]]);
  const poll = h.ports.commands.send({ kind: "poll", generation: 17, sequence: 2 });
  assert.deepEqual(Object.keys(poll).sort(), ["admitted", "error", "generation", "kind", "operation", "report", "sequence", "status"]);
  assert.equal(poll.report.words.length, 58); assert.equal(poll.report.available, true);
  assert.deepEqual(calls(h, "retained_setup"), []); assert.deepEqual(calls(h, "finish_retained"), []);
});

test("new retained setup refuses unknown version, identity, sequence, fields and nonindependent word extents before Rust", async () => {
  const malformed = [
    () => ({ version: 0 }), () => ({ version: 2 }), () => ({ version: "1" }),
    () => ({ generation: 16 }), () => ({ sequence: 2 }), () => ({ sequence: 0 }),
    () => ({ sequence: 9007199254740992 }), () => ({ programId: 1n }),
    () => ({ programId: 0n }), () => ({ programId: -1n }),
    () => ({ programId: 18446744073709551616n }), () => ({ programId: 9007199254740992 }),
    () => ({ extra: 0 }), () => ({ [Symbol("extra")]: 0 }),
    () => ({ words: new Uint32Array(31) }), () => ({ words: new Uint32Array(33) }),
    () => ({ words: Array.from(setupWords()) }),
    () => ({ words: new Uint32Array(new ArrayBuffer(132), 4, 32) }),
    () => ({ words: new Uint32Array(new SharedArrayBuffer(128)) }),
    () => ({ words: new Uint32Array(new ArrayBuffer(128, { maxByteLength: 256 })) }),
    () => { const words = setupWords(); structuredClone(words.buffer, { transfer: [words.buffer] }); return { words }; },
  ];
  for (const mutate of malformed) {
    const h = await harness(); h.attachSamples();
    const ack = h.sample("retained-setup", { words: setupWords(), ...mutate() });
    assertTerminal(h, ack);
    assert.deepEqual(calls(h, "retained_setup"), []);
    assert.deepEqual(calls(h, "insert_sample"), []);
    assert.deepEqual(calls(h, "retained_cue"), []);
  }
});

test("retained upload preserves original ordering and real counts instead of accepting a synthetic EOF", async () => {
  const cases = [
    { before: h => {}, kind: "retained-cue", fields: () => ({ index: 0, words: cueWords() }), method: "retained_cue", prior: 0 },
    { before: h => { h.sample("sample", sample()); }, kind: "retained-setup", fields: () => ({ words: setupWords() }), method: "retained_setup", prior: 0 },
    { before: h => { h.sample("retained-setup", { words: setupWords() }); },
      kind: "retained-setup", fields: () => ({ words: setupWords() }), method: "retained_setup", prior: 1 },
    { before: h => { h.sample("retained-setup", { words: setupWords() }); },
      kind: "retained-cue", fields: () => ({ index: 0, words: cueWords() }), method: "retained_cue", prior: 0 },
    { before: h => { h.sample("retained-setup", { words: setupWords() }); h.sample("sample", sample()); },
      kind: "retained-cue", fields: () => ({ index: 1, words: cueWords(1) }), method: "retained_cue", prior: 0 },
    { before: h => { h.sample("retained-setup", { words: setupWords() }); h.sample("sample", sample());
        h.sample("retained-cue", { index: 0, words: cueWords() }); },
      kind: "sample", fields: () => ({ ...sample(), id: 8n }), method: "insert_sample", prior: 1 },
    { before: h => { h.sample("retained-setup", { words: setupWords() }); h.sample("sample", sample()); },
      kind: "end-samples", fields: () => ({ count: 1, bytes: 16 }), method: "finish_retained", prior: 0 },
    { before: h => { const words = setupWords(); words[20] = 2;
        h.sample("retained-setup", { words }); h.sample("sample", sample());
        h.sample("retained-cue", { index: 0, words: cueWords() }); h.sample("retained-cue", { index: 1, words: cueWords(1) }); },
      kind: "end-samples", fields: () => ({ count: 1, bytes: 16 }), method: "finish_retained", prior: 0 },
  ];
  for (const scenario of cases) {
    const h = await harness(); h.attachSamples(); scenario.before(h);
    const ack = h.sample(scenario.kind, scenario.fields());
    assert.notEqual(ack.status, 0); assert.equal(h.processor.failed, true);
    assert.equal(calls(h, scenario.method).length, scenario.prior);
  }
  for (const totals of [{ count: 0, bytes: 16 }, { count: 1, bytes: 0 }, { count: 1, bytes: 17 }]) {
    const h = await harness(); h.upload({ end: false, finish: false });
    assert.notEqual(h.sample("end-samples", totals).status, 0);
    assert.deepEqual(calls(h, "finish_retained"), []); assert.equal(h.processor.failed, true);
  }
});

test("retained cues validate exact extent, reserved bits, index, program and known sample custody", async () => {
  const cases = [
    () => ({ words: new Uint32Array(8) }), () => ({ words: new Uint32Array(10) }),
    () => ({ words: new Uint32Array(new ArrayBuffer(40), 4, 9) }),
    () => ({ words: new Uint32Array(new SharedArrayBuffer(36)) }),
    () => { const words = cueWords(); words[7] = 1; return { words }; },
    () => { const words = cueWords(); words[8] = 1; return { words }; },
    () => { const words = cueWords(); words[6] = 0x7fc00000; return { words }; },
    () => { const words = cueWords(); words[2] = 4; return { words }; },
    () => ({ index: -1 }), () => ({ index: 0.5 }), () => ({ index: 4294967296 }),
    () => ({ programId: PROGRAM + 1n }), () => ({ extra: true }),
  ];
  for (const mutate of cases) {
    const h = await harness(); h.attachSamples();
    assert.equal(h.sample("retained-setup", { words: setupWords() }).status, 0);
    assert.equal(h.sample("sample", sample()).status, 0);
    assertTerminal(h, h.sample("retained-cue", { index: 0, words: cueWords(), ...mutate() }));
    assert.deepEqual(calls(h, "retained_cue"), []);
  }
});

test("cold Rust setup, cue and finish errors fence without falling back to the fixed owner", async () => {
  const setup = await harness({ retained_setupStatus: 1 }); setup.attachSamples();
  assertTerminal(setup, setup.sample("retained-setup", { words: setupWords() }));
  assert.deepEqual(calls(setup, "retained_cue"), []);
  const cue = await harness({ retained_cueStatus: 1 }); cue.attachSamples();
  assert.equal(cue.sample("retained-setup", { words: setupWords() }).status, 0);
  assert.equal(cue.sample("sample", sample()).status, 0);
  assertTerminal(cue, cue.sample("retained-cue", { index: 0, words: cueWords() }));
  const finish = await harness({ finish_retainedStatus: 7 }); finish.upload({ finish: false });
  assert.equal(finish.host("finish", { mode: "retained" }).status, 7);
  assert.equal(finish.processor.failed, true);
  assert.deepEqual(calls(finish, "finish_retained"), [[]]);
  for (const h of [setup, cue, finish]) {
    assert.deepEqual(calls(h, "finish"), []); assert.deepEqual(calls(h, "finish_at"), []);
  }
});

test("active rate and original restart stage exactly ten scalar pairs and preserve u64 control identities", async () => {
  const h = await harness(); h.upload(); h.attachCommands();
  const rate = h.command("retained-rate", { id: 18446744073709551615n,
    attempt: 9007199254741001n, revision: 4294967297n, preparation: preparationWords() });
  assert.deepEqual(rate, ackBase("retained-rate", 1));
  assert.deepEqual(calls(h, "retained_preparation_word"), STAGED);
  assert.deepEqual(calls(h, "retained_rate"), [[18446744073709551615n, 9007199254741001n, 4294967297n]]);
  assert.deepEqual(calls(h, "retained_event_poll"), [], "admission does not manufacture an applied event");
  const restart = h.command("retained-restart-original", { id: 6n, attempt: 7n, revision: 3n, preparation: preparationWords() });
  assert.deepEqual(restart, ackBase("retained-restart-original", 2));
  assert.deepEqual(calls(h, "retained_preparation_word"), [...STAGED, ...STAGED]);
  assert.deepEqual(calls(h, "retained_restart_original"), [[6n, 7n, 3n]]);
  assert.deepEqual(calls(h, "retained_cue").length, 2, "active control never scans or reinstalls original cues");
  assert.deepEqual(calls(h, "retained_setup").length, 1);
});

test("region, loop disable and pause use their existing scalar numeric exports", async () => {
  const h = await harness(); h.upload(); h.attachCommands();
  assert.deepEqual(h.command("retained-region", { id: 1n, attempt: 7n, revision: 3n,
    start: -9223372036854775808n, end: 9223372036854775807n, repeat: true }), ackBase("retained-region", 1));
  assert.deepEqual(h.command("retained-disable-loop", { id: 2n, attempt: 7n, revision: 3n }), ackBase("retained-disable-loop", 2));
  assert.deepEqual(h.command("retained-pause", { id: 3n, attempt: 7n, paused: true }), ackBase("retained-pause", 3));
  assert.deepEqual(calls(h, "retained_region"), [[1n, 7n, 3n, -9223372036854775808n, 9223372036854775807n, true]]);
  assert.deepEqual(calls(h, "retained_disable_loop"), [[2n, 7n, 3n]]);
  assert.deepEqual(calls(h, "retained_pause"), [[3n, 7n, true]]);
  assert.deepEqual(calls(h, "retained_preparation_word"), []);
});

test("normal retained admission refusals settle only their request and preserve the following command", async () => {
  for (const status of [1, 2, 3, 7]) {
    const faults = { retained_rateStatus: status }, h = await harness(faults);
    h.upload(); h.attachCommands();
    const ack = h.command("retained-rate", { id: 9n, attempt: 7n, revision: 3n, preparation: preparationWords() });
    assert.equal(ack.status, status); assert.equal(ack.terminal, false); assert.equal(typeof ack.error, "string");
    assert.deepEqual({ ...ack, status: 0, error: null }, ackBase("retained-rate", 1));
    assert.equal(h.processor.failed, false); assert.equal(h.owner.frees, 0);
    assert.equal(h.processor.port.messages.some(message => message.kind === "terminal"), false);
    faults.retained_rateStatus = 0;
    assert.deepEqual(h.command("retained-rate", { id: 10n, attempt: 7n, revision: 3n, preparation: preparationWords() }),
      ackBase("retained-rate", 2));
    assert.deepEqual(calls(h, "retained_preparation_word"), [...STAGED, ...STAGED]);
  }
});

test("refused scalar staging never invokes apply and a fresh request restarts the ten-word import", async () => {
  const h = await harness({ retained_preparation_wordStatuses: [0, 0, 1] }); h.upload(); h.attachCommands();
  const refused = h.command("retained-rate", { id: 1n, attempt: 7n, revision: 3n, preparation: preparationWords() });
  assert.equal(refused.status, 1); assert.equal(refused.terminal, false); assert.equal(h.processor.failed, false);
  assert.deepEqual(calls(h, "retained_preparation_word"), [[0, 2, 0], [1, 1, 2097152], [2, 48000, 0]]);
  assert.deepEqual(calls(h, "retained_rate"), []);
  assert.equal(h.command("retained-rate", { id: 2n, attempt: 7n, revision: 3n, preparation: preparationWords() }).status, 0);
  assert.deepEqual(calls(h, "retained_preparation_word"), [[0, 2, 0], [1, 1, 2097152], [2, 48000, 0], ...STAGED]);
  assert.deepEqual(calls(h, "retained_rate"), [[2n, 7n, 3n]]);
});

test("actual retained owner faults fence, preserve first terminal evidence, and release only on stop", async () => {
  for (const faults of [
    ...[4, 5, 6, 8, 9].map(status => ({ retained_rateStatus: status })),
    { throwMethod: "retained_rate" },
  ]) {
    const h = await harness(faults); h.upload(); h.attachCommands();
    assertTerminal(h, h.command("retained-rate", { id: 1n, attempt: 7n, revision: 3n, preparation: preparationWords() }));
    const first = h.processor.port.messages.find(message => message.kind === "terminal");
    assert.equal(h.owner.frees, 0);
    const callCount = calls(h, "retained_rate").length;
    assertTerminal(h, h.command("retained-disable-loop", { id: 2n, attempt: 7n, revision: 3n }));
    assert.equal(calls(h, "retained_rate").length, callCount);
    assert.deepEqual(calls(h, "retained_disable_loop"), []);
    assert.deepEqual(h.processor.port.messages.find(message => message.kind === "terminal"), first);
    assert.equal(h.host("stop").status, 0); assert.equal(h.owner.frees, 1);
    assert.equal(h.ports.commands.closes, 1);
  }
});

test("retained controls reject malformed identity, extent and shape before any staged or applied mutation", async () => {
  const malformed = [
    () => ({ version: 0 }), () => ({ version: 2 }), () => ({ version: undefined }),
    () => ({ generation: 18 }), () => ({ programId: PROGRAM + 1n }),
    () => ({ programId: 0n }), () => ({ programId: "9007199254740993" }),
    () => ({ sequence: 2 }), () => ({ sequence: -1 }), () => ({ sequence: 9007199254740992 }),
    () => ({ id: -1n }), () => ({ id: 18446744073709551616n }), () => ({ id: 1 }),
    () => ({ attempt: -1n }), () => ({ revision: -1n }), () => ({ revision: 1 }),
    () => ({ preparation: new Uint32Array(19) }), () => ({ preparation: new Uint32Array(21) }),
    () => ({ preparation: new Uint32Array(new ArrayBuffer(84), 4, 20) }),
    () => ({ preparation: new Uint32Array(new SharedArrayBuffer(80)) }),
    () => ({ preparation: new Uint32Array(new ArrayBuffer(80, { maxByteLength: 160 })) }),
    () => { const preparation = preparationWords(); structuredClone(preparation.buffer, { transfer: [preparation.buffer] }); return { preparation }; },
    () => ({ extra: 0 }), () => ({ [Symbol("extra")]: 0 }),
  ];
  for (const mutate of malformed) {
    const h = await harness(); h.upload(); h.attachCommands();
    assertTerminal(h, h.command("retained-rate", { id: 1n, attempt: 7n, revision: 3n,
      preparation: preparationWords(), ...mutate() }));
    assert.deepEqual(calls(h, "retained_preparation_word"), []);
    assert.deepEqual(calls(h, "retained_rate"), []);
  }
  const invalidShapes = [
    ["retained-region", { id: 1n, attempt: 7n, revision: 3n, start: -1n, end: 10n, repeat: 1 }],
    ["retained-region", { id: 1n, attempt: 7n, revision: 3n, start: -9223372036854775809n, end: 10n, repeat: false }],
    ["retained-region", { id: 1n, attempt: 7n, revision: 3n, start: 0n, end: 9223372036854775808n, repeat: false }],
    ["retained-disable-loop", { id: 1n, attempt: 7n, revision: 3n, repeat: false }],
    ["retained-restart-original", { id: 1n, attempt: 7n, revision: 3n, preparation: new Uint32Array(19) }],
    ["retained-pause", { id: 1n, attempt: 7n, paused: 1 }],
    ["retained-pause", { id: 1n, attempt: 7n, paused: false, revision: 3n }],
    ["retained-poll", { id: 1n }], ["retained-event-ack", { serial: -1n }],
    ["retained-history-retire", { pin: 18446744073709551616n }],
    ["retained-unknown", {}],
  ];
  for (const [kind, fields] of invalidShapes) {
    const h = await harness(); h.upload(); h.attachCommands();
    const before = h.owner.calls.length;
    assertTerminal(h, h.command(kind, fields));
    assert.equal(h.owner.calls.slice(before).some(call => call[0].startsWith("retained_")), false);
  }
});

test("retained requests cannot cross transferred ports, bypass setup, or reenter a closed sample endpoint", async () => {
  const plain = await harness(); assert.equal(plain.host("finish").status, 0); plain.attachCommands();
  assertTerminal(plain, plain.command("retained-rate", { id: 1n, attempt: 7n, revision: 3n, preparation: preparationWords() }));
  assert.deepEqual(calls(plain, "retained_rate"), []);

  const samples = await harness(); samples.attachSamples();
  assertTerminal(samples, samples.sample("retained-rate", { id: 1n, attempt: 7n, revision: 3n, preparation: preparationWords() }));
  assert.deepEqual(calls(samples, "retained_preparation_word"), []);

  const commands = await harness(); commands.upload(); commands.attachCommands();
  assertTerminal(commands, commands.command("retained-cue", { index: 0, words: cueWords() }));
  assert.equal(calls(commands, "retained_cue").length, 2);

  const closed = await harness(); closed.attachSamples();
  const oldEndpoint = closed.ports.samples, stale = oldEndpoint.onmessage;
  closed.sample("retained-setup", { words: setupWords() }); closed.sample("sample", sample());
  closed.sample("retained-cue", { index: 0, words: cueWords() }); closed.sample("retained-cue", { index: 1, words: cueWords(1) });
  assert.equal(closed.sample("end-samples", { count: 1, bytes: 16 }).status, 0);
  const before = oldEndpoint.messages.length;
  stale({ data: { kind: "retained-cue", version: 1, generation: 17, sequence: 6,
    programId: PROGRAM, index: 2, words: cueWords() } });
  assert.equal(oldEndpoint.messages.length, before); assert.equal(calls(closed, "retained_cue").length, 2);
  assert.equal(closed.host("finish", { mode: "retained" }).status, 0);
});

test("retained poll copies the genuine 58-word report, six metadata pairs, bounded spans and exact 80-word event", async () => {
  const h = await harness(); h.upload(); h.attachCommands();
  h.owner.report = Uint32Array.of(
    1, 0, 11, 0, 5, 0, 3, 0, 2, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0,
    4294967286, 4294967295, 0, 0, 5, 0, 2, 0, 2, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 16, 0, 1, 0, 11, 0, 0, 0, 0, 0,
  );
  h.owner.event = pauseEventWords();
  const ack = h.command("retained-poll");
  assert.deepEqual({ ...ack, report: null, retained: null }, ackBase("retained-poll", 1));
  assert.equal(ack.report.available, true); assert.ok(ack.report.words instanceof Uint32Array);
  assert.deepEqual(Array.from(ack.report.words), [
    1, 0, 11, 0, 5, 0, 3, 0, 2, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0,
    4294967286, 4294967295, 0, 0, 5, 0, 2, 0, 2, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 16, 0, 1, 0, 11, 0, 0, 0, 0, 0,
  ]);
  assert.deepEqual(Object.keys(ack.retained).sort(), ["event", "history", "metadata"]);
  assert.deepEqual(Array.from(ack.retained.metadata), [7, 0, 3, 0, 3, 0, 2, 0, 1, 0, 1, 0]);
  assert.deepEqual(Array.from(ack.retained.history), [
    1, 0, 1, 2097152, 11, 0, 48000, 0, 0, 0, 0, 0, 0, 0, 5, 0, 2, 0, 2, 0,
    0, 0, 1, 0, 2, 0, 0, 0, 0, 0,
    2, 0, 0, 0, 0, 0, 0, 0, 1, 0,
  ]);
  assert.equal(ack.retained.history.length, 40); assert.equal(ack.retained.event.length, 80);
  assert.deepEqual(Array.from(ack.retained.event), Array.from(pauseEventWords()));
  assert.notEqual(ack.report.words.buffer, h.owner.report.buffer);
  assert.notEqual(ack.retained.metadata.buffer, h.owner.metadata.buffer);
  assert.notEqual(ack.retained.event.buffer, h.owner.event.buffer);
  assert.deepEqual(calls(h, "report_word"), Array.from({ length: 29 }, (_, index) => [[index, false], [index, true]]).flat());
  assert.deepEqual(calls(h, "retained_metadata_word"), Array.from({ length: 6 }, (_, index) => [[index, false], [index, true]]).flat());
  assert.deepEqual(calls(h, "retained_event_word"), Array.from({ length: 80 }, (_, index) => [index]));
  assert.equal(calls(h, "retained_history_span_word").length, 20);
  h.owner.metadata[4] = 9; h.owner.event[8] = 99;
  assert.equal(ack.retained.metadata[4], 3); assert.equal(ack.retained.event[8], 17);
  assert.deepEqual(calls(h, "retained_event_ack"), []); assert.deepEqual(calls(h, "retained_history_retire"), []);
});

test("empty retained poll reports no applied event, and repeated event polls remain latched until explicit serial ACK", async () => {
  const h = await harness(); h.upload(); h.attachCommands();
  // Report availability is independent of the owner's absolute context start;
  // preserve that common basis for the later event from the same owner.
  h.owner.report[0] = 0;
  const empty = h.command("retained-poll");
  assert.equal(empty.report.available, false); assert.equal(empty.report.words.length, 58);
  assert.equal(empty.retained.event, null); assert.deepEqual(calls(h, "retained_event_word"), []);
  h.owner.event = pauseEventWords();
  const first = h.command("retained-poll"), second = h.command("retained-poll");
  assert.deepEqual(Array.from(first.retained.event), Array.from(pauseEventWords()));
  assert.deepEqual(Array.from(second.retained.event), Array.from(pauseEventWords()));
  assert.deepEqual(calls(h, "retained_event_ack"), []); assert.deepEqual(calls(h, "retained_history_retire"), []);
});

test("retained poll fences oversized history and invalid event-owner results before an unbounded copy", async () => {
  for (const faults of [{ historyCount: 9 }, { historyCount: 65537 }, { eventPoll: 2 },
    { throwMethod: "retained_metadata_word" }, { throwMethod: "retained_event_word" }]) {
    const h = await harness(faults); h.upload(); h.attachCommands();
    h.owner.event = pauseEventWords();
    assertTerminal(h, h.command("retained-poll"));
    if (faults.historyCount) assert.deepEqual(calls(h, "retained_history_span_word"), []);
    assert.deepEqual(calls(h, "retained_event_ack"), []);
    assert.deepEqual(calls(h, "retained_history_retire"), []);
  }
});

test("event ACK and input-history pin retirement stay separate and wrong serial preserves the latched event", async () => {
  const faults = { retained_event_ackStatus: 1 }, h = await harness(faults); h.upload(); h.attachCommands();
  h.owner.event = pauseEventWords();
  const wrong = h.command("retained-event-ack", { serial: 4294967300n });
  assert.equal(wrong.status, 1); assert.equal(wrong.terminal, false);
  assert.deepEqual(calls(h, "retained_event_ack"), [[4, 1]]);
  assert.deepEqual(Array.from(h.command("retained-poll").retained.event), Array.from(pauseEventWords()));
  assert.deepEqual(calls(h, "retained_history_retire"), []);
  faults.retained_event_ackStatus = 0;
  assert.deepEqual(h.command("retained-event-ack", { serial: 4294967301n }), ackBase("retained-event-ack", 3));
  assert.deepEqual(calls(h, "retained_event_ack"), [[4, 1], [5, 1]]);
  assert.deepEqual(calls(h, "retained_history_retire"), [], "ACK grants no input-retirement permission");
  assert.equal(h.command("retained-poll").retained.event, null);
  assert.deepEqual(h.command("retained-history-retire", { pin: 4294967299n }), ackBase("retained-history-retire", 5));
  assert.deepEqual(calls(h, "retained_history_retire"), [[3, 1]]);
  assert.equal(h.processor.failed, false);
});

test("history retirement refusal remains nonterminal while the owner retains an event pin", async () => {
  const h = await harness({ retained_history_retireStatus: 1 }); h.upload(); h.attachCommands();
  h.owner.event = pauseEventWords();
  const ack = h.command("retained-history-retire", { pin: 5n });
  assert.equal(ack.status, 1); assert.equal(ack.terminal, false); assert.equal(h.processor.failed, false);
  assert.deepEqual(calls(h, "retained_history_retire"), [[5, 0]]);
  assert.deepEqual(calls(h, "retained_event_ack"), []);
  assert.deepEqual(Array.from(h.command("retained-poll").retained.event), Array.from(pauseEventWords()));
});

test("paused rate admission remains pending and the next resume request reaches the owner", async () => {
  const h = await harness(); h.upload(); h.attachCommands();
  assert.deepEqual(h.command("retained-pause", { id: 1n, attempt: 7n, paused: true }), ackBase("retained-pause", 1));
  h.owner.metadata[10] = 0; // The binding fixture now reports actual paused state.
  assert.deepEqual(h.command("retained-rate", { id: 2n, attempt: 7n, revision: 3n, preparation: preparationWords() }),
    ackBase("retained-rate", 2));
  const pending = h.command("retained-poll");
  assert.equal(pending.retained.event, null);
  assert.deepEqual(Array.from(pending.retained.metadata), [7, 0, 3, 0, 3, 0, 2, 0, 1, 0, 0, 0]);
  assert.deepEqual(h.command("retained-pause", { id: 3n, attempt: 7n, paused: false }), ackBase("retained-pause", 4));
  assert.deepEqual(calls(h, "retained_pause"), [[1n, 7n, true], [3n, 7n, false]]);
  assert.deepEqual(calls(h, "retained_rate"), [[2n, 7n, 3n]]);
  assert.deepEqual(calls(h, "retained_event_ack"), []);
  assert.equal(h.processor.failed, false);
});

test("retained startup keeps certified native rendering on the existing scalar callback path", async () => {
  const h = await harness(); h.certify(); h.upload(); h.attachCommands();
  assert.equal(h.host("arm", { frame: 11n }), undefined);
  assert.equal(h.process(10, 1).alive, true);
  assert.deepEqual(calls(h, "arm_words"), [[11, 0, 10, 0]]);
  const before = h.owner.calls.length, posted = h.processor.port.messages.length;
  const { alive, output } = h.process(11, 5);
  assert.equal(alive, true);
  assert.deepEqual(Array.from(output[0][0]), [0.25, 0.5, 0.75, 1, 0]);
  assert.deepEqual(Array.from(output[0][1]), [-0.25, -0.5, -0.75, -1, 0]);
  assert.deepEqual(h.owner.calls.slice(before), [["render", 11, 0, 5]]);
  assert.equal(h.callbackViews, 0); assert.equal(h.callbackBigInts, 0);
  assert.equal(h.processor.port.messages.length, posted);
  assert.deepEqual(calls(h, "retained_metadata_word"), []);
  assert.deepEqual(calls(h, "retained_event_poll"), []);
  assert.deepEqual(calls(h, "retained_history_len"), []);
});
