// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/audio-host.test.mjs
// The real host module runs against controlled WebAudio endpoints, never an audio device.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

const sourceUrl = new URL("./audio-host.mjs", import.meta.url);
const source = await readFile(sourceUrl, "utf8");
const emptyModule = new WebAssembly.Module(Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0]));
const U64_MAX = 18446744073709551615n;
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;

function options(overrides = {}) {
  return {
    module: emptyModule, generation: 17, channels: 2, timeoutMs: 50, ...overrides,
    pcmLimits: { maxAssetBytes: 16, maxTotalBytes: 24, maxSamples: 2, ...overrides.pcmLimits },
    audioLimits: { queueCapacity: 4, maxVoices: 2, pendingCapacity: 4, maxFrames: 5,
      maxCommandsPerRender: 4, ...overrides.audioLimits },
  };
}
function sample(overrides = {}) {
  return { id: 1n, rate: 44100, channels: 2,
    pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]), ...overrides };
}
function command(overrides = {}) {
  return { kind: 0, voice: U64_MAX, sample: 1n, at: I64_MIN,
    gain: -0.5, value: I64_MAX, denominator: 0n, ...overrides };
}
function report(available = false) {
  const words = new Uint32Array(56);
  words[0] = Number(available);
  return { available, words };
}
function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function observe(action) {
  const watched = { settled: false };
  let returned;
  try { returned = action(); } catch (error) { returned = Promise.reject(error); }
  watched.result = Promise.resolve(returned).then(
    value => { watched.settled = true; return { ok: true, value }; },
    error => { watched.settled = true; return { ok: false, error }; },
  );
  return watched;
}
async function flush() {
  for (let index = 0; index < 32; index++) await Promise.resolve();
}
function failure(result, code) {
  assert.equal(result.ok, false, "operation unexpectedly succeeded");
  if (code !== undefined) assert.equal(result.error.code, code);
  return result.error;
}

async function harness(faults = {}) {
  const contexts = [];
  const nodes = [];
  const trace = [];
  const timers = new Map();
  let now = 0;
  let timerId = 0;
  let postAttempts = 0;

  class EventTarget {
    constructor() { this.listeners = new Map(); }
    addEventListener(type, handler) {
      if (!this.listeners.has(type)) this.listeners.set(type, new Set());
      this.listeners.get(type).add(handler);
    }
    removeEventListener(type, handler) { this.listeners.get(type)?.delete(handler); }
    emit(type, data) {
      const event = type === "message" ? { data } : { type, ...data };
      if (typeof this[`on${type}`] === "function") this[`on${type}`](event);
      for (const handler of [...(this.listeners.get(type) ?? [])]) handler.call(this, event);
    }
    handlers() {
      return [...this.listeners.values()].reduce((count, values) => count + values.size, 0)
        + ["onmessage", "onmessageerror", "onprocessorerror", "onstatechange"]
          .filter(key => typeof this[key] === "function").length;
    }
  }
  class Port extends EventTarget {
    constructor() { super(); this.sent = []; this.closes = 0; this.starts = 0; }
    postMessage(message, transfer = []) {
      postAttempts++;
      if (postAttempts === faults.postThrowsAt) throw new Error("injected postMessage failure");
      assert.equal(this.closes, 0, "posting after port cleanup");
      const transfers = Array.isArray(transfer) ? transfer : transfer.transfer ?? [];
      const snapshot = structuredClone(message, { transfer: transfers });
      this.sent.push({ message: snapshot, transfers: [...transfers] });
      trace.push(["post", message.kind, message.sequence]);
    }
    start() { this.starts++; }
    close() { this.closes++; trace.push(["port-close"]); }
  }
  class AudioContext extends EventTarget {
    constructor(configuration) {
      super();
      trace.push(["context", configuration]);
      if (faults.contextThrows) throw new Error("context constructor failed");
      this.sampleRate = faults.sampleRate ?? 48000;
      this.currentTime = 0;
      this.state = "suspended";
      this.destination = { context: this };
      this.closes = 0;
      this.handlersAtClose = [];
      this.resumes = 0;
      this.audioWorklet = {
        addModule: url => {
          trace.push(["add-module", String(url)]);
          if (faults.moduleThrows) throw new Error("addModule failed");
          return faults.moduleGate?.promise ?? Promise.resolve();
        },
      };
      contexts.push(this);
    }
    resume() {
      this.resumes++;
      trace.push(["resume"]);
      if (faults.resumeThrows) throw new Error("resume failed");
      return (faults.resumeGate?.promise ?? Promise.resolve()).then(() => {
        if (this.state !== "closed") this.state = faults.resumeState ?? "running";
        this.emit("statechange", {});
      });
    }
    close() {
      this.closes++;
      this.handlersAtClose.push(this.handlers());
      trace.push(["context-close"]);
      if (faults.closeThrows) throw new Error("close failed");
      return (faults.closeGate?.promise ?? Promise.resolve()).then(() => {
        this.state = "closed";
        this.emit("statechange", {});
      });
    }
  }
  class AudioWorkletNode extends EventTarget {
    constructor(context, name, configuration) {
      super();
      trace.push(["node", name]);
      if (faults.nodeThrows) throw new Error("node constructor failed");
      this.context = context;
      this.name = name;
      this.configuration = configuration;
      this.port = new Port();
      this.connections = [];
      this.disconnects = 0;
      nodes.push(this);
    }
    connect(destination) {
      this.connections.push(destination);
      trace.push(["connect"]);
      if (faults.connectThrows) throw new Error("connect failed");
      return destination;
    }
    disconnect() { this.disconnects++; trace.push(["disconnect"]); }
  }
  const context = createContext({
    AudioContext: faults.unsupported ? undefined : AudioContext,
    AudioWorkletNode: faults.unsupported ? undefined : AudioWorkletNode,
    AbortController, AbortSignal, ArrayBuffer, SharedArrayBuffer, DataView,
    Float32Array, Uint32Array, Uint8Array, WebAssembly, URL, DOMException, structuredClone,
    Date: class extends Date {
      constructor(...args) { super(...(args.length ? args : [now])); }
      static now() { return now; }
    },
    performance: { now: () => now },
    setTimeout(callback, delay, ...args) {
      const id = ++timerId;
      timers.set(id, { at: now + delay, callback: () => callback(...args) });
      return id;
    },
    clearTimeout(id) { timers.delete(id); },
  });
  const actual = new SourceTextModule(source, {
    context, identifier: sourceUrl.href,
    initializeImportMeta(meta) { meta.url = sourceUrl.href; },
  });
  await actual.link(specifier => { throw new Error(`Unexpected host import: ${specifier}`); });
  await actual.evaluate();
  return {
    AudioHost: actual.namespace.AudioHost, AudioHostError: actual.namespace.AudioHostError,
    faults, contexts, nodes, timers, trace,
    get now() { return now; },
    get sent() { return nodes.at(-1)?.port.sent ?? []; },
    last() { return this.sent.at(-1)?.message; },
    ready(fields = {}) {
      const node = nodes.at(-1);
      node.port.emit("message", { kind: "ready", generation: node.configuration.processorOptions.generation,
        sampleRate: node.context.sampleRate, channels: node.configuration.processorOptions.channels, ...fields });
    },
    reply(message, fields = {}) {
      nodes.at(-1).port.emit("message", {
        kind: "ack", generation: message.generation, sequence: message.sequence,
        operation: message.kind, status: 0,
        admitted: message.kind === "commands" ? message.commands.length : 0,
        error: null, report: message.kind === "poll" ? report() : null, ...fields,
      });
    },
    async expire() {
      assert.ok(timers.size, "expected a finite host deadline");
      const [id, timer] = [...timers].sort((left, right) => left[1].at - right[1].at)[0];
      timers.delete(id);
      now = timer.at;
      timer.callback();
      await flush();
    },
    advanceBeforeDeadline(elapsed) {
      const target = now + elapsed;
      assert.ok([...timers.values()].every(timer => timer.at > target));
      now = target;
    },
  };
}

function assertClean(h) {
  assert.equal(h.timers.size, 0, "cleanup must clear every deadline");
  for (const context of h.contexts) {
    assert.equal(context.closes, 1);
    assert.deepEqual(context.handlersAtClose, [0], "context listeners must be cleared before close");
    assert.equal(context.handlers(), 0);
  }
  for (const node of h.nodes) {
    assert.equal(node.disconnects, 1);
    assert.equal(node.port.closes, 1);
    assert.equal(node.handlers(), 0);
    assert.equal(node.port.handlers(), 0);
  }
}
async function open(h, configuration = options()) {
  const opening = observe(() => h.AudioHost.open(configuration));
  await flush();
  assert.equal(h.nodes.length, 1);
  assert.equal(opening.settled, false, "ready is required before exposing the owner");
  h.ready();
  const result = await opening.result;
  assert.equal(result.ok, true, result.error?.message);
  assert.equal(result.value.state, "setup");
  return result.value;
}
async function acknowledged(h, action, fields = {}) {
  const count = h.sent.length;
  const pending = observe(action);
  await flush();
  assert.equal(h.sent.length, count + 1, "one operation must post exactly one control");
  const message = h.last();
  h.reply(message, fields);
  const result = await pending.result;
  assert.equal(result.ok, true, result.error?.message);
  return result.value;
}
async function localError(h, action, code = "validation") {
  const count = h.sent.length;
  const error = failure(await observe(action).result, code);
  assert.ok(error instanceof h.AudioHostError);
  assert.equal(h.sent.length, count, "local rejection must not post or consume sequence");
  return error;
}
async function stop(h, owner, expectedState = "closed") {
  const first = owner.stop();
  assert.equal(owner.stop(), first, "concurrent stop shares the cleanup promise");
  const pending = observe(() => first);
  await flush();
  assert.equal(h.last().kind, "stop");
  h.reply(h.last());
  const result = await pending.result;
  assert.equal(result.ok, true, result.error?.message);
  assert.equal(result.value, undefined);
  assert.equal(owner.stop(), first, "settled stop also retains its promise");
  assert.equal(owner.state, expectedState);
  assertClean(h);
}

test("open resumes during the gesture, snapshots configuration, and requires exact ready evidence", async () => {
  const moduleGate = deferred();
  const h = await harness({ moduleGate });
  const configuration = options();
  const opening = observe(() => h.AudioHost.open(configuration));
  assert.deepEqual(h.trace.slice(0, 2).map(entry => entry[0]), ["context", "resume"]);
  assert.equal(h.contexts[0].resumes, 1);
  assert.equal(h.nodes.length, 0);
  configuration.generation = 99;
  configuration.channels = 1;
  configuration.pcmLimits.maxSamples = 999;
  configuration.audioLimits.maxFrames = 999;
  await flush();
  assert.equal(h.trace.findIndex(entry => entry[0] === "add-module") > 1, true);
  moduleGate.resolve();
  await flush();
  const node = h.nodes[0];
  assert.equal(node.name, "beatkernel-audio");
  assert.equal(node.configuration.numberOfInputs, 0);
  assert.equal(node.configuration.numberOfOutputs, 1);
  assert.deepEqual(Array.from(node.configuration.outputChannelCount), [2]);
  assert.equal(node.configuration.processorOptions.module, emptyModule);
  assert.equal(node.configuration.processorOptions.generation, 17);
  assert.equal(node.configuration.processorOptions.pcmLimits.maxSamples, 2);
  assert.equal(node.configuration.processorOptions.audioLimits.maxFrames, 5);
  assert.equal(node.connections[0], h.contexts[0].destination);
  assert.equal(h.trace.find(entry => entry[0] === "add-module")[1], new URL("./audio-worklet.js", sourceUrl).href);
  h.ready({ generation: 99 });
  await flush();
  assert.equal(opening.settled, false, "stale generation cannot satisfy readiness");
  h.ready();
  const opened = await opening.result;
  assert.equal(opened.ok, true);
  const owner = opened.value;
  assert.equal(owner.generation, 17);
  assert.equal(owner.channels, 2);
  assert.equal(owner.sampleRate, 48000);
  assert.equal(owner.state, "setup");
  await stop(h, owner);
});

test("successful setup transfers exact PCM ownership and preserves command widths and genuine reports", async () => {
  const h = await harness();
  const owner = await open(h);
  const first = sample();
  const backing = first.pcm.buffer;
  const pending = observe(() => owner.sample(first));
  assert.equal(first.pcm.byteLength, 0, "sample admission transfers the standalone backing");
  assert.equal(h.sent[0].transfers[0], backing);
  assert.equal(h.last().rate, 44100, "source rate must not be rewritten to context rate");
  assert.deepEqual(Array.from(h.last().pcm), [0.25, -0.25, 0.5, -0.5]);
  h.reply(h.last());
  assert.equal((await pending.result).ok, true);
  await acknowledged(h, () => owner.sample(sample({ id: 2n, rate: 96000, pcm: new Float32Array([1, -1]) })));
  const unavailable = await acknowledged(h, () => owner.poll());
  assert.equal(unavailable.available, false);
  assert.deepEqual(Array.from(unavailable.words), Array(56).fill(0));
  await acknowledged(h, () => owner.finish());
  assert.equal(owner.state, "allocated");
  const frame = 9007199254741999n;
  await acknowledged(h, () => owner.arm(frame));
  assert.equal(h.last().frame, frame);
  assert.equal(owner.state, "armed");
  const commands = [0, 1, 2, 3].map(kind => command({ kind }));
  const ack = await acknowledged(h, () => owner.commands(commands));
  assert.equal(ack.admitted, 4);
  assert.deepEqual(structuredClone(h.last().commands), commands);
  const evidence = report(true);
  evidence.words[3] = 0xffffffff;
  evidence.words[21] = 0x80000000;
  evidence.words[48] = 1234567890;
  evidence.words[49] = 0x100000;
  const actual = await acknowledged(h, () => owner.poll(), { report: evidence });
  assert.equal(actual.available, true);
  assert.deepEqual(Array.from(actual.words), Array.from(evidence.words));
  assert.deepEqual(h.sent.map(entry => entry.message.sequence), [1, 2, 3, 4, 5, 6, 7]);
  assert.ok(h.sent.every(entry => entry.message.generation === 17));
  await stop(h, owner);
});

test("currentFrame is a checked context-time estimate independent of command and render evidence", async () => {
  const h = await harness();
  const owner = await open(h);
  const context = h.contexts[0];
  assert.equal(owner.currentFrame, 0n);
  context.currentTime = 1.000005;
  assert.equal(owner.currentFrame, 48000n);
  context.currentTime = (4294967296 + 128.75) / context.sampleRate;
  assert.equal(owner.currentFrame, 4294967424n);
  assert.equal(h.sent.length, 0, "reading a scheduling estimate cannot manufacture a report");
  const pending = observe(() => owner.poll());
  assert.equal(owner.currentFrame, 4294967424n, "a pending control does not invalidate context time");
  h.reply(h.last());
  assert.equal((await pending.result).ok, true);
  for (const value of [-1, NaN, Infinity, "1", null, 2 * Number.MAX_SAFE_INTEGER / context.sampleRate]) {
    context.currentTime = value;
    await localError(h, () => owner.currentFrame, "state");
    assert.equal(owner.state, "setup");
  }
  context.currentTime = 0;
  assert.equal(owner.currentFrame, 0n);
  await stop(h, owner);
  await localError(h, () => owner.currentFrame, "state");
});

test("sample preflight rejects unsafe buffers and bounds without posting, and overlap never builds a queue", async () => {
  const h = await harness();
  const owner = await open(h);
  const detached = new Float32Array(0);
  structuredClone(detached.buffer, { transfer: [detached.buffer] });
  for (const invalid of [
    sample({ id: 1 }), sample({ id: -1n }), sample({ id: U64_MAX + 1n }),
    sample({ rate: 0 }), sample({ rate: 4294967296 }), sample({ channels: 1 }),
    sample({ pcm: new Uint8Array(4) }), sample({ pcm: new Float32Array(3) }),
    sample({ pcm: new Float32Array(6) }), sample({ pcm: new Float32Array([NaN, 0]) }),
    sample({ pcm: new Float32Array([0, Infinity]) }),
    sample({ pcm: new Float32Array(new ArrayBuffer(24), 4, 4) }),
    sample({ pcm: new Float32Array(new SharedArrayBuffer(16)) }),
    sample({ pcm: detached }),
  ]) {
    const bytes = invalid.pcm.byteLength;
    await localError(h, () => owner.sample(invalid));
    assert.equal(invalid.pcm.byteLength, bytes, "rejection cannot detach caller data");
    assert.equal(owner.state, "setup");
  }
  const pending = observe(() => owner.sample(sample()));
  const first = h.last();
  await localError(h, () => owner.poll(), "busy");
  await localError(h, () => owner.finish(), "busy");
  await localError(h, () => owner.sample(sample({ id: 2n })), "busy");
  assert.equal(h.sent.length, 1);
  h.reply(first);
  assert.equal((await pending.result).ok, true);
  await localError(h, () => owner.sample(sample()), "validation");
  await localError(h, () => owner.sample(sample({ id: 2n })), "validation");
  await acknowledged(h, () => owner.sample(sample({ id: 2n, pcm: new Float32Array(2) })));
  await localError(h, () => owner.sample(sample({ id: 3n, pcm: new Float32Array(0) })), "validation");
  assert.deepEqual(h.sent.map(entry => entry.message.sequence), [1, 2]);
  await stop(h, owner);
});

test("command batches preflight every record atomically and state transitions remain one-shot", async () => {
  const h = await harness();
  const owner = await open(h);
  await localError(h, () => owner.commands([command()]), "state");
  await localError(h, () => owner.arm(1n), "state");
  await acknowledged(h, () => owner.finish());
  await localError(h, () => owner.finish(), "state");
  await localError(h, () => owner.sample(sample()), "state");
  const missing = command();
  delete missing.value;
  const inherited = Object.create({ denominator: 1n });
  Object.assign(inherited, missing, { value: 0n });
  delete inherited.denominator;
  for (const invalid of [
    null, missing, inherited, command({ extra: true }), command({ kind: 4 }),
    command({ voice: 1 }), command({ voice: -1n }), command({ sample: U64_MAX + 1n }),
    command({ at: I64_MIN - 1n }), command({ value: I64_MAX + 1n }),
    command({ gain: Infinity }), command({ gain: NaN }), command({ gain: 1e40 }),
    command({ denominator: -1n }),
  ]) await localError(h, () => owner.commands([command(), invalid]));
  for (const batch of [[], [command(), command(), command(), command(), command()]]) {
    await localError(h, () => owner.commands(batch));
  }
  for (const frame of [-1n, U64_MAX + 1n, 100]) await localError(h, () => owner.arm(frame));
  await acknowledged(h, () => owner.arm(U64_MAX));
  await localError(h, () => owner.arm(U64_MAX), "state");
  assert.equal(owner.state, "armed");
  assert.deepEqual(h.sent.map(entry => entry.message.kind), ["finish", "arm"]);
  await stop(h, owner);
});

test("remote command failure exposes the exact admitted prefix, fences, and never retries", async () => {
  for (const [status, admitted] of [[3, 0], [3, 1], [106, 2]]) {
    const h = await harness();
    const owner = await open(h);
    await acknowledged(h, () => owner.finish());
    const pending = observe(() => owner.commands([command(), command({ kind: 1 }), command({ kind: 2 })]));
    const sent = h.last();
    h.reply(sent, { status, admitted, error: "admission" });
    h.nodes[0].port.emit("message", { kind: "terminal", generation: 17, status });
    const error = failure(await pending.result, "remote");
    assert.ok(error instanceof h.AudioHostError);
    assert.equal(error.operation, "commands");
    assert.equal(error.generation, 17);
    assert.equal(error.sequence, sent.sequence);
    assert.equal(error.status, status);
    assert.equal(error.admitted, admitted);
    await flush();
    assert.equal(h.sent.filter(entry => entry.message.kind === "commands").length, 1);
    const later = await localError(h, () => owner.poll(), "remote");
    assert.equal(later.admitted, admitted);
    assert.equal(later.sequence, sent.sequence);
    assert.equal(h.last().kind, "stop");
    assert.equal(h.last().sequence, sent.sequence + 1);
    await stop(h, owner, "failed");
  }
});

test("stale generations are ignored while malformed correlation or report evidence fences", async () => {
  const h = await harness();
  const owner = await open(h);
  const pending = observe(() => owner.poll());
  const sent = h.last();
  h.reply(sent, { generation: 16 });
  h.nodes[0].port.emit("message", { kind: "terminal", generation: 16, status: 8 });
  await flush();
  assert.equal(pending.settled, false);
  assert.equal(h.sent.length, 1);
  h.reply(sent);
  assert.equal((await pending.result).ok, true);
  h.reply(sent);
  await flush();
  await localError(h, () => owner.poll(), "protocol");
  await stop(h, owner, "failed");

  const highFlag = report(true);
  highFlag.words[1] = 1;
  const nonBooleanFlag = report();
  nonBooleanFlag.words[0] = 2;
  for (const fields of [
    { sequence: 2 }, { operation: "finish" }, { status: "0" }, { admitted: 1 },
    { kind: "unknown" }, { report: null },
    { report: { available: false, words: new Uint32Array(55) } },
    { report: { available: false, words: new Int32Array(56) } },
    { report: { available: true, words: new Uint32Array(56) } },
    { report: highFlag }, { report: nonBooleanFlag },
  ]) {
    const invalid = await harness();
    const live = await open(invalid);
    const operation = observe(() => live.poll());
    invalid.reply(invalid.last(), fields);
    failure(await operation.result, "protocol");
    await flush();
    assert.equal(invalid.last().kind, "stop");
    await stop(invalid, live, "failed");
  }
  for (const fields of [{ admitted: 0 }, { status: 3, admitted: 2, error: "admission" }]) {
    const invalid = await harness();
    const live = await open(invalid);
    await acknowledged(invalid, () => live.finish());
    const operation = observe(() => live.commands([command()]));
    invalid.reply(invalid.last(), fields);
    failure(await operation.result, "protocol");
    await stop(invalid, live, "failed");
  }
});

test("processor, message, terminal and posting failures reject pending work and release the owner", async () => {
  for (const [kind, code] of [["processorerror", "processor"], ["messageerror", "message"], ["terminal", "processor"], ["post", "transport"]]) {
    const h = await harness();
    const owner = await open(h);
    if (kind === "post") h.faults.postThrowsAt = 1;
    const pending = observe(() => owner.poll());
    if (kind === "processorerror") h.nodes[0].emit(kind, {});
    if (kind === "messageerror") h.nodes[0].port.emit(kind, {});
    if (kind === "terminal") h.nodes[0].port.emit("message", { kind, generation: 17, status: 8 });
    failure(await pending.result, code);
    await flush();
    assert.equal(h.last().kind, "stop");
    assert.equal(h.sent.filter(entry => entry.message.kind === "stop").length, 1);
    await stop(h, owner, "failed");
  }
});

test("operation and shutdown deadlines remain finite even when ACK or close never settles", async () => {
  const h = await harness();
  const owner = await open(h);
  const pending = observe(() => owner.poll());
  await h.expire();
  const original = failure(await pending.result, "timeout");
  assert.equal(original.operation, "poll");
  assert.equal(original.admitted, null);
  assert.equal(h.last().kind, "stop");
  const cleanup = observe(() => owner.stop());
  await h.expire();
  failure(await cleanup.result, "timeout");
  assert.equal(original.operation, "poll", "cleanup cannot replace the original failure");
  assertClean(h);

  const closeGate = deferred();
  const stalled = await harness({ closeGate });
  const live = await open(stalled);
  const stopping = observe(() => live.stop());
  stalled.reply(stalled.last());
  await flush();
  assert.equal(stalled.contexts[0].closes, 1);
  assert.equal(stopping.settled, false);
  await stalled.expire();
  failure(await stopping.result, "timeout");
  assertClean(stalled);

  for (const broken of ["stop-ack", "close"]) {
    const failing = await harness(broken === "close" ? { closeThrows: true } : {});
    const active = await open(failing);
    const first = active.stop();
    const cleanup = observe(() => first);
    failing.reply(failing.last(), broken === "stop-ack" ? { status: 106, error: "exception" } : {});
    failure(await cleanup.result, broken === "stop-ack" ? "remote" : "transport");
    assert.equal(active.stop(), first);
    assertClean(failing);
  }
});

test("stop cancels setup work, tolerates its valid late ACK, and shares one cleanup promise", async () => {
  const h = await harness();
  const owner = await open(h);
  const admission = observe(() => owner.sample(sample()));
  const canceled = h.last();
  const first = owner.stop();
  const stopping = observe(() => first);
  assert.equal(owner.stop(), first);
  h.contexts[0].state = "interrupted";
  h.contexts[0].emit("statechange", {});
  failure(await admission.result, "closed");
  await flush();
  const fresh = h.last();
  assert.equal(fresh.kind, "stop");
  assert.equal(fresh.sequence, canceled.sequence + 1);
  h.reply(canceled);
  await flush();
  assert.equal(stopping.settled, false, "old valid ACK cannot finish or fail fresh stop");
  h.reply(fresh);
  assert.equal((await stopping.result).ok, true);
  assert.equal(owner.stop(), first);
  await localError(h, () => owner.finish(), "closed");
  assertClean(h);
});

test("invalid options and aborted setup reject before resources, and abort ownership ends after ready", async () => {
  for (const configuration of [
    options({ module: {} }), options({ generation: 0 }), options({ channels: 0 }),
    options({ timeoutMs: 0 }), options({ timeoutMs: 60001 }), options({ timeoutMs: NaN }),
    options({ pcmLimits: { maxAssetBytes: 25 } }), options({ pcmLimits: { maxSamples: 0 } }),
    options({ audioLimits: { queueCapacity: 0 } }), options({ audioLimits: { maxFrames: 1048577 } }),
  ]) {
    const h = await harness();
    failure(await observe(() => h.AudioHost.open(configuration)).result, "validation");
    assert.equal(h.contexts.length, 0);
    assert.equal(h.timers.size, 0);
  }
  const early = new AbortController();
  early.abort();
  const h = await harness();
  failure(await observe(() => h.AudioHost.open(options({ signal: early.signal }))).result, "aborted");
  assert.equal(h.contexts.length, 0);
  const moduleGate = deferred();
  const waiting = await harness({ moduleGate });
  const controller = new AbortController();
  const opening = observe(() => waiting.AudioHost.open(options({ signal: controller.signal })));
  controller.abort();
  failure(await opening.result, "aborted");
  assertClean(waiting);
  moduleGate.resolve();
  await flush();
  assert.equal(waiting.nodes.length, 0, "late module completion cannot recreate canceled ownership");

  const accepted = await harness();
  const after = new AbortController();
  const owner = await open(accepted, options({ signal: after.signal }));
  after.abort();
  await acknowledged(accepted, () => owner.poll());
  await stop(accepted, owner);
});

test("opening failures and one shared setup deadline clean every partially created resource", async () => {
  for (const faults of [
    { contextThrows: true }, { resumeThrows: true }, { moduleThrows: true },
    { nodeThrows: true }, { connectThrows: true }, { unsupported: true },
  ]) {
    const h = await harness(faults);
    const opening = observe(() => h.AudioHost.open(options()));
    await flush();
    for (let turn = 0; !opening.settled && turn < 4; turn++) await h.expire();
    failure(await opening.result);
    assertClean(h);
  }
  for (const stage of ["resume", "module"]) {
    const gate = deferred();
    const h = await harness(stage === "resume" ? { resumeGate: gate } : { moduleGate: gate });
    const opening = observe(() => h.AudioHost.open(options()));
    await flush();
    gate.reject(new Error(`asynchronous ${stage} failure`));
    await flush();
    for (let turn = 0; !opening.settled && turn < 4; turn++) await h.expire();
    failure(await opening.result, "transport");
    assertClean(h);
  }
  for (const stalledStage of ["resume", "module", "ready"]) {
    const gate = deferred();
    const h = await harness(stalledStage === "resume" ? { resumeGate: gate }
      : stalledStage === "module" ? { moduleGate: gate } : {});
    const opening = observe(() => h.AudioHost.open(options()));
    await flush();
    await h.expire();
    // If the processor exists, let its cleanup time out too; no fabricated ready/ACK.
    for (let turn = 0; !opening.settled && turn < 4; turn++) await h.expire();
    failure(await opening.result, "timeout");
    assertClean(h);
    gate.resolve();
    await flush();
    assertClean(h);
  }
  const moduleGate = deferred();
  const h = await harness({ moduleGate });
  const opening = observe(() => h.AudioHost.open(options()));
  await flush();
  h.advanceBeforeDeadline(40);
  moduleGate.resolve();
  await flush();
  assert.equal(h.nodes.length, 1);
  await h.expire();
  assert.equal(h.now, 50, "module completion must not renew the original setup deadline");
  for (let turn = 0; !opening.settled && turn < 4; turn++) await h.expire();
  failure(await opening.result, "timeout");
  assertClean(h);
});

test("ready must match actual context rate and channels before an owner becomes usable", async () => {
  for (const fields of [{ sampleRate: 44100 }, { channels: 1 }, { sampleRate: NaN }]) {
    const h = await harness();
    const opening = observe(() => h.AudioHost.open(options()));
    await flush();
    h.ready(fields);
    await flush();
    const stopMessage = h.sent.find(entry => entry.message.kind === "stop")?.message;
    if (stopMessage) h.reply(stopMessage);
    failure(await opening.result, "protocol");
    assertClean(h);
  }
});

test("a fulfilled resume is insufficient when the context is not running, and later interruption fences", async () => {
  const initial = await harness({ resumeState: "suspended" });
  const opening = observe(() => initial.AudioHost.open(options()));
  await flush();
  assert.equal(opening.settled, false, "opening statechanges do not bypass the ready handshake");
  initial.ready();
  await flush();
  assert.equal(initial.last().kind, "stop");
  initial.reply(initial.last());
  const rejected = failure(await opening.result, "state");
  assert.equal(rejected.operation, "open");
  assertClean(initial);

  for (const state of ["suspended", "interrupted", "closed"]) {
    const h = await harness();
    const owner = await open(h);
    const pending = observe(() => owner.poll());
    h.contexts[0].state = state;
    h.contexts[0].emit("statechange", {});
    const error = failure(await pending.result, "state");
    assert.equal(error.operation, "context");
    assert.equal(error.admitted, null);
    await flush();
    assert.equal(h.last().kind, "stop");
    const retained = await localError(h, () => owner.currentFrame, "state");
    assert.equal(retained.operation, "context");
    await stop(h, owner, "failed");
  }
});
