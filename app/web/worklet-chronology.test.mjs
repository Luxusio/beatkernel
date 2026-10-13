// Exercise the actual controller without importing the browser runner's startup effects.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, runInContext, SourceTextModule, SyntheticModule } from "node:vm";

const sourceUrl = new URL("./worklet-chronology.browser.mjs", import.meta.url);
const source = await readFile(sourceUrl, "utf8");
const start = source.indexOf("async function runNativeCase(");
const end = source.indexOf("\nfunction verifyPhase(", start);
assert.ok(start >= 0 && end > start, "the actual native-case controller must be available");
const controllerSource = source.slice(start, end);
const phaseSpec = { name: "phase-controlled", rate: 48000, offset: 17, phaseProbe: true, operation: "stream" };

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function flush() {
  for (let index = 0; index < 20; index++) await Promise.resolve();
}

function observe(promise) {
  const state = { settled: false };
  const result = promise.then(value => {
    Object.assign(state, { settled: true, ok: true, value });
    return state;
  }, error => {
    Object.assign(state, { settled: true, ok: false, error });
    return state;
  });
  return { state, result };
}

function virtualClock() {
  let now = 0, nextId = 0;
  const timers = new Map();
  return {
    timers,
    now: () => now,
    setTimeout(callback, delay) {
      const id = ++nextId;
      timers.set(id, { callback, deadline: now + Math.max(0, Number(delay) || 0) });
      return id;
    },
    clearTimeout(id) { timers.delete(id); },
    async advance(milliseconds) {
      const target = now + milliseconds;
      await flush();
      while (true) {
        const due = [...timers].filter(([, timer]) => timer.deadline <= target)
          .sort((left, right) => left[1].deadline - right[1].deadline || left[0] - right[0])[0];
        if (!due) break;
        const [id, timer] = due;
        now = timer.deadline;
        timers.delete(id);
        timer.callback();
        await flush();
      }
      now = target;
      await flush();
    },
  };
}

function harness({ pendingNavigation = false, pendingEvaluation = false, pendingClose = false } = {}) {
  const clock = virtualClock(), acquisition = deferred(), navigation = deferred(), evaluation = deferred(), closing = deferred();
  const calls = { acquire: [], goto: [], evaluate: [], close: [] };
  const page = {
    on() {},
    goto(url, ...options) {
      calls.goto.push({ time: clock.now(), url, options });
      return pendingNavigation ? navigation.promise : Promise.resolve();
    },
    evaluate(_callback, input) {
      calls.evaluate.push({ time: clock.now(), input });
      return pendingEvaluation ? evaluation.promise : Promise.resolve({ ...input.spec, generation: input.generation, cleanup: {} });
    },
    close() {
      calls.close.push(clock.now());
      return pendingClose ? closing.promise : Promise.resolve();
    },
  };
  const context = createContext({
    browser: { newPage() { calls.acquire.push(clock.now()); return acquisition.promise; } },
    port: 8121,
    performance: { now: clock.now },
    setTimeout: clock.setTimeout,
    clearTimeout: clock.clearTimeout,
  });
  const runNativeCase = runInContext(`${controllerSource}\nrunNativeCase;`, context, { filename: sourceUrl.pathname });
  return { clock, acquisition, navigation, evaluation, closing, page, calls, runNativeCase };
}

function phaseTimeout(outcome) {
  assert.equal(outcome.settled, true, "the phase case must end after bounded deadline cleanup");
  assert.equal(outcome.ok, false, "the phase deadline must reject the case");
  assert.match(String(outcome.error), /phase/i);
  assert.match(String(outcome.error), /deadline|watchdog|timeout/i);
}

test("phase controller bounds pending page acquisition from case entry at 120 seconds", async () => {
  const h = harness();
  const pending = observe(h.runNativeCase(phaseSpec, 111));
  await flush();
  assert.deepEqual(h.calls.acquire, [0]);
  await h.clock.advance(119999);
  assert.equal(pending.state.settled, false, "acquisition remains eligible before the deadline");
  await h.clock.advance(1);
  phaseTimeout(pending.state);
  assert.equal(h.calls.goto.length, 0);
  assert.equal(h.calls.evaluate.length, 0);
  assert.equal(h.clock.timers.size, 0, "expired acquisition must leave no controller timer");
});

test("a page acquired after phase timeout is closed without starting the case", async () => {
  const h = harness();
  const pending = observe(h.runNativeCase(phaseSpec, 112));
  await h.clock.advance(120000);
  phaseTimeout(pending.state);
  const originalError = pending.state.error;
  h.acquisition.resolve(h.page);
  await flush();
  assert.equal(h.calls.close.length, 1, "late acquired pages remain owned and must close");
  assert.equal(h.calls.goto.length, 0);
  assert.equal(h.calls.evaluate.length, 0);
  assert.equal(pending.state.error, originalError);
  assert.equal(h.clock.timers.size, 0, "late page cleanup must clear its own timer");
});

test("phase acquisition and navigation consume the same budget and successful cleanup clears timers", async () => {
  const h = harness({ pendingNavigation: true, pendingEvaluation: true });
  const pending = observe(h.runNativeCase(phaseSpec, 113));
  await h.clock.advance(40000);
  h.acquisition.resolve(h.page);
  await flush();
  assert.equal(h.calls.goto.length, 1);
  assert.equal(h.calls.goto[0].options[0].timeout, 80000, "navigation receives the remaining case budget");
  await h.clock.advance(30000);
  h.navigation.resolve();
  await flush();
  assert.equal(h.calls.evaluate.length, 1);
  assert.equal(h.calls.evaluate[0].input.caseBudgetMs, 50000, "evaluation cannot restart the case budget");
  await h.clock.advance(49999);
  assert.equal(pending.state.settled, false);
  h.evaluation.resolve({ cleanup: {} });
  await flush();
  assert.equal(pending.state.settled, true);
  assert.equal(pending.state.ok, true);
  assert.equal(pending.state.value.cleanup.pageClosed, true);
  assert.equal(h.calls.close.length, 1);
  assert.equal(h.clock.timers.size, 0, "successful page cleanup must clear all controller timers");
  await h.clock.advance(100000);
  assert.equal(pending.state.ok, true);
  assert.equal(h.calls.close.length, 1);
});

for (const stage of ["navigation", "evaluation"]) {
  test(`phase controller bounds stalled ${stage} at the original deadline and separately bounds page close`, async () => {
    const h = harness({ pendingNavigation: true, pendingEvaluation: true, pendingClose: true });
    const pending = observe(h.runNativeCase(phaseSpec, stage === "navigation" ? 115 : 116));
    await h.clock.advance(40000);
    h.acquisition.resolve(h.page);
    await flush();
    assert.equal(h.calls.goto.length, 1);
    if (stage === "evaluation") {
      await h.clock.advance(30000);
      h.navigation.resolve();
      await flush();
      // The controlled evaluate endpoint never invokes its callback, so a
      // page-side watchdog cannot rescue the controller from this stalled call.
      assert.equal(h.calls.evaluate.length, 1);
    }
    await h.clock.advance(119999 - h.clock.now());
    assert.equal(pending.state.settled, false);
    assert.equal(h.calls.close.length, 0, "the original deadline has not expired early");
    await h.clock.advance(1);
    assert.deepEqual(h.calls.close, [120000], "stalled work must enter page cleanup at the original deadline");
    assert.equal(pending.state.settled, false, "pending page close receives its independent cleanup budget");
    await h.clock.advance(2999);
    assert.equal(pending.state.settled, false, "page cleanup remains eligible before its own deadline");
    await h.clock.advance(1);
    phaseTimeout(pending.state);
    assert.match(String(pending.state.error), /(?:case|controller).*(?:deadline|watchdog|timeout)/i,
      "page-close failure must retain the original phase case deadline");
    assert.equal(h.calls.evaluate.length, stage === "evaluation" ? 1 : 0);
    assert.equal(h.calls.close.length, 1);
    assert.equal(h.clock.timers.size, 0, "both the case and page-close deadlines must be cleared");
    const originalError = pending.state.error;
    h.navigation.resolve();
    h.evaluation.resolve({ cleanup: {} });
    h.closing.resolve();
    await flush();
    assert.equal(pending.state.error, originalError, "late completions cannot revive the timed-out case");
    assert.equal(h.calls.evaluate.length, stage === "evaluation" ? 1 : 0);
    assert.equal(h.calls.close.length, 1);
    assert.equal(h.clock.timers.size, 0);
  });
}

test("rejected phase page acquisition retains its error and clears the controller deadline", async () => {
  const h = harness();
  const pending = observe(h.runNativeCase(phaseSpec, 114));
  await h.clock.advance(40000);
  const failure = new Error("owned page acquisition failed");
  h.acquisition.reject(failure);
  await flush();
  assert.equal(pending.state.settled, true);
  assert.equal(pending.state.ok, false);
  assert.equal(pending.state.error, failure);
  assert.equal(h.calls.goto.length, 0);
  assert.equal(h.calls.close.length, 0);
  assert.equal(h.clock.timers.size, 0, "failed acquisition must cancel its controller deadline");
  await h.clock.advance(120000);
  assert.equal(pending.state.error, failure);
});

test("marker acquisition retains its historical pending behavior without phase timers", async () => {
  const h = harness();
  const markerSpec = { name: "native-controlled", rate: 48000, offset: 17 };
  const pending = observe(h.runNativeCase(markerSpec, 15));
  await flush();
  assert.equal(h.clock.timers.size, 0);
  await h.clock.advance(120001);
  assert.equal(pending.state.settled, false, "the phase controller must not change marker acquisition");
  h.acquisition.resolve(h.page);
  await flush();
  assert.equal(pending.state.settled, true);
  assert.equal(pending.state.ok, true);
  assert.equal(h.calls.goto[0].options.length, 0);
  assert.equal(h.calls.evaluate[0].input.caseBudgetMs, null);
  assert.equal(h.calls.close.length, 1);
  assert.equal(h.clock.timers.size, 0);
});

// These are controlled callback inputs to the actual served bootstrap. They
// establish its arithmetic/protocol behavior, not native AudioParam precision,
// browser scheduling or production playback. The real-browser lane owns those.
const nativeTemplateStart = source.indexOf("const nativeBootstrap = `");
const nativeTemplateEnd = source.indexOf("\nasync function serve()", nativeTemplateStart);
assert.ok(nativeTemplateStart >= 0 && nativeTemplateEnd > nativeTemplateStart,
  "the actual served native bootstrap must remain extractable");
const nativeTemplate = source.slice(nativeTemplateStart, nativeTemplateEnd);
const nativeHelperSource = await readFile(new URL("./worklet-native-frame-probe.mjs", import.meta.url), "utf8");
const jointPeriods = [1048576, 1048575, 1048573];
const jointNames = jointPeriods.map((_, lane) => `__native_joint_phase_${lane}`);
const jointGeneration = 217;

function jointGeometry(startFrame) {
  const horizonFrames = jointPeriods.map(period => startFrame + 3 * period);
  return { periods: [...jointPeriods], startFrame, horizonFrames,
    jointHorizonFrame: Math.min(...horizonFrames), futureEligible: [true, true, true] };
}

function nativeLanes(base, frames, geometry) {
  return jointPeriods.map((period, lane) => Float32Array.from({ length: frames }, (_, index) => {
    const frame = base + index;
    if (!geometry || frame < geometry.startFrame) return 0;
    if (frame >= geometry.horizonFrames[lane]) return 1;
    // Independent ideal input for quarter-early, unit-slope native ramp events.
    return 1.25 + (frame - geometry.startFrame) % period;
  }));
}

async function nativeHarness({ rate = 48000, joint = true, phase = true, jointFault } = {}) {
  const calls = { process: [], control: [], crt: [] };
  const productionDescriptors = [{ name: "production-owned", defaultValue: 0.5, automationRate: "k-rate" }];
  class Production {
    static get parameterDescriptors() { return productionDescriptors; }
    constructor(options) {
      this.generation = options.processorOptions.generation;
      this.phase = 0;
      this.owner = { report_word() { return 0; } };
    }
    control(message) { calls.control.push(message); }
    process(inputs, outputs, parameters) {
      calls.process.push({ inputs, outputs, parameters });
      for (const bus of outputs) for (const channel of bus) channel.fill(0);
      return true;
    }
  }
  class Port {
    messages = [];
    onmessage = null;
    starts = 0;
    closes = 0;
    postMessage(message) { this.messages.push(structuredClone(message)); }
    start() { this.starts++; }
    close() { this.closes++; }
    send(message) {
      assert.equal(typeof this.onmessage, "function", "requests require a live private port");
      this.onmessage({ data: message });
    }
  }
  let Processor;
  const context = createContext({ Float32Array, sampleRate: rate, currentFrame: 0,
    registerProcessor(name, value) { assert.equal(name, "beatkernel-audio"); Processor = value; } });
  const text = runInContext(`const phaseProbe = ${phase}; const jointProbe = ${joint};\n${nativeTemplate}\nnativeBootstrap;`, context);
  const helper = new SourceTextModule(nativeHelperSource, { context });
  await helper.link(specifier => { throw Error(`Unexpected native helper import: ${specifier}`); });
  await helper.evaluate();
  // Forward every actual helper export. CRT observation/fault injection is an
  // imported-helper seam; neither lane evidence nor CRT math is copied here.
  const helpers = new SyntheticModule(Object.keys(helper.namespace), function () {
    for (const name of Object.keys(helper.namespace)) {
      const actual = helper.namespace[name];
      this.setExport(name, name !== "nativeJointOffset" ? actual : (...values) => {
        const offset = actual(...values);
        calls.crt.push({ values, offset });
        return jointFault ? jointFault(offset, calls.crt.length) : offset;
      });
    }
  }, { context });
  const bindings = new SyntheticModule(["Production"], function () { this.setExport("Production", Production); }, { context });
  const actual = new SourceTextModule(`${text}\nimport { Production } from '__chronology-production';\nregisterProcessor('beatkernel-audio', Production);`, { context });
  await actual.link(specifier => {
    if (specifier === "./worklet-native-frame-probe.mjs") return helpers;
    if (specifier === "__chronology-production") return bindings;
    throw Error(`Unexpected native bootstrap import: ${specifier}`);
  });
  await actual.evaluate();
  const processor = new Processor({ processorOptions: { generation: jointGeneration } });
  const port = new Port();
  processor.control({ kind: "__native-attach", generation: jointGeneration, id: 1, port });
  assert.equal(port.messages[0].kind, "attached");
  let id = 1;
  const ledger = [];
  return {
    processor, port, calls, context, productionDescriptors, ledger,
    get descriptors() { return Processor.parameterDescriptors; },
    request(kind, fields = {}) {
      const message = { kind, generation: jointGeneration, id: ++id, ...fields };
      port.send(message);
      return port.messages.at(-1);
    },
    attach(fields = {}) {
      const rejected = new Port();
      processor.control({ kind: "__native-attach", generation: jointGeneration,
        id: processor.nativeId + 1, port: rejected, ...fields });
      return rejected;
    },
    process({ base = processor.nativeCursor ?? 256, frames = 128, marker = new Float32Array([1]),
      lanes = nativeLanes(base, frames), rawFrame = base } = {}) {
      context.currentFrame = rawFrame;
      const parameters = { __native_marker: marker, __native_phase: new Float32Array([0]),
        ...Object.fromEntries(jointNames.map((name, lane) => [name, lanes[lane]])) };
      const outputs = [[new Float32Array(frames).fill(9), new Float32Array(frames).fill(9)]];
      ledger.push({ base, frames });
      const result = processor.process([], outputs, parameters);
      assert.equal(result, true);
      assert.ok(outputs.every(bus => bus.every(channel => channel.every(value => value === 0))));
      return parameters;
    },
    origin() {
      assert.equal(this.request("watch", { targetFrame: 273, initial: true }).kind, "accepted");
      const marker = new Float32Array(128); marker.fill(1, 17);
      this.process({ base: 256, marker, rawFrame: 999999 });
      assert.equal(processor.nativeAnchor.base, 256, "only the actual interior binary onset establishes origin");
      assert.equal(processor.nativeCursor, 384, "origin advances by the original callback extent once");
      return processor.nativeAnchor;
    },
  };
}

function rawRecords(port) {
  return port.messages.filter(message => message.kind === "callback" || message.kind === "phase-snapshot");
}
function phaseSnapshots(port, reason) {
  return port.messages.filter(message => message.kind === "phase-snapshot" && message.reason === reason);
}
function jointSummary(h) {
  const summary = h.port.messages.findLast(message => message.kind === "phase-summary");
  assert.ok(summary, "retirement or completion must preserve the actual session summary");
  return summary;
}
function assertRetired(h, reason) {
  const retired = h.port.messages.findLast(message => message.kind === "retired");
  assert.ok(retired);
  assert.equal(retired.reason, reason);
  assert.equal(h.processor.nativePort, null);
  assert.equal(h.processor.nativeSession, null);
  assert.equal(h.processor.nativeWatch, null);
  assert.equal(h.port.onmessage, null);
  assert.equal(h.port.closes, 1);
  const replies = h.port.messages.length, ordinal = h.processor.nativeOrdinal;
  const delegated = h.calls.process.length;
  h.process({ lanes: nativeLanes(0, 128), marker: new Float32Array([0]) });
  assert.equal(h.calls.process.length, delegated + 1, "retirement preserves ordinary production delegation");
  assert.equal(h.port.messages.length, replies, "retirement cannot produce later observation evidence");
  assert.equal(h.processor.nativeOrdinal, ordinal);
  return retired;
}

test("joint native descriptors preserve production entries and expose exactly three fixed a-rate defaults", async () => {
  const h = await nativeHarness();
  const descriptors = h.descriptors;
  assert.equal(descriptors[0], h.productionDescriptors[0]);
  assert.deepEqual(h.productionDescriptors, [{ name: "production-owned", defaultValue: 0.5, automationRate: "k-rate" }]);
  const lanes = Array.from(descriptors).filter(value => jointNames.includes(value.name));
  assert.deepEqual(lanes.map(value => value.name), jointNames);
  for (const [lane, descriptor] of lanes.entries()) {
    assert.equal(descriptor.defaultValue, 0);
    assert.equal(descriptor.minValue, 0);
    assert.equal(descriptor.maxValue, jointPeriods[lane] + 1);
    assert.equal(descriptor.automationRate, "a-rate");
  }
  const legacy = await nativeHarness({ joint: false });
  assert.ok(legacy.descriptors.some(value => value.name === "__native_phase"));
  assert.equal(legacy.descriptors.filter(value => jointNames.includes(value.name)).length, 0);
});

test("joint-start requires the fresh binary origin and retains fixed admitted geometry without mutating its request", async () => {
  const h = await nativeHarness(), geometry = jointGeometry(529);
  const original = structuredClone(geometry);
  assert.equal(h.request("joint-start", geometry).reason, "origin");
  assert.equal(h.processor.nativeSession, null);
  assert.equal(h.processor.nativeAnchor, null);
  assert.equal(h.processor.nativeCursor, null);
  h.origin();
  assert.equal(h.request("joint-start", geometry).kind, "accepted");
  const session = h.processor.nativeSession;
  assert.equal(session.joint, true);
  assert.deepEqual(Array.from(session.periods), jointPeriods);
  assert.deepEqual(Array.from(session.horizonFrames), geometry.horizonFrames);
  assert.equal(session.startFrame, 529);
  assert.equal(session.jointHorizonFrame, geometry.jointHorizonFrame);
  assert.equal(session.checkedSamples, 0);
  assert.equal(h.request("joint-start", jointGeometry(657)).reason, "phase-active");
  assert.equal(h.processor.nativeSession, session);
  assert.equal(h.processor.nativeCursor, 384);
  assert.deepEqual(geometry, original);
});

test("raw globals and index-zero marker values cannot establish a fresh joint origin", async () => {
  const h = await nativeHarness();
  assert.equal(h.request("watch", { targetFrame: 273, initial: true }).kind, "accepted");
  h.process({ marker: new Float32Array(128).fill(1), rawFrame: 256 });
  assert.equal(h.processor.nativeAnchor, null);
  assert.equal(h.processor.nativeCursor, null);
  assert.equal(h.request("joint-start", jointGeometry(529)).reason, "origin");
  assert.equal(h.processor.nativeSession, null);
  assert.equal(h.calls.crt.length, 0);
});

test("joint-start refuses malformed order, shape, sums and future flags without replacing origin or session", async () => {
  const h = await nativeHarness(); h.origin();
  const geometry = jointGeometry(529), anchor = h.processor.nativeAnchor;
  const malformed = [
    { periods: undefined }, { periods: [1048575, 1048576, 1048573] },
    { periods: [1048576, 1048575] }, { periods: [...jointPeriods, 7] },
    { periods: new Float32Array(jointPeriods) },
    { startFrame: 0 }, { startFrame: 384 }, { startFrame: 529.5 },
    { startFrame: NaN }, { startFrame: Infinity },
    { startFrame: Number.MAX_SAFE_INTEGER - 10 },
    { horizonFrames: undefined }, { horizonFrames: geometry.horizonFrames.slice(0, 2) },
    { horizonFrames: [...geometry.horizonFrames, 1] },
    { horizonFrames: [...geometry.horizonFrames].reverse() },
    { horizonFrames: geometry.horizonFrames.map(value => value + 1) },
    { horizonFrames: [Number.MAX_SAFE_INTEGER + 1, ...geometry.horizonFrames.slice(1)] },
    { jointHorizonFrame: geometry.horizonFrames[0] }, { jointHorizonFrame: undefined },
    { futureEligible: undefined }, { futureEligible: [true, true] },
    { futureEligible: [true, true, true, true] }, { futureEligible: [true, true, 1] },
    { futureEligible: [true, true, "true"] },
  ];
  for (const fields of malformed) {
    const request = { ...geometry, ...fields }, original = structuredClone(request);
    const refused = h.request("joint-start", request);
    assert.equal(refused.kind, "refused", JSON.stringify(fields));
    assert.equal(refused.reason, "joint-range", JSON.stringify(fields));
    assert.equal(h.processor.nativeSession, null);
    assert.equal(h.processor.nativeAnchor, anchor);
    assert.equal(h.processor.nativeCursor, 384);
    assert.equal(h.processor.nativeRetired, false);
    assert.deepEqual(request, original);
  }
  assert.equal(h.request("joint-start", geometry).kind, "accepted", "range refusal must permit a later valid identity");
});

test("joint private identity, operation and closing refusals preserve the admitted session", async () => {
  const h = await nativeHarness(); h.origin();
  assert.equal(h.request("joint-start", jointGeometry(529)).kind, "accepted");
  const session = h.processor.nativeSession, nativeId = h.processor.nativeId;
  assert.equal(h.request("joint-start", { ...jointGeometry(529), generation: jointGeneration - 1 }).reason, "generation");
  assert.equal(h.processor.nativeId, nativeId);
  assert.equal(h.request("joint-start", { ...jointGeometry(529), id: nativeId }).reason, "identity");
  assert.equal(h.processor.nativeId, nativeId);
  assert.equal(h.request("unknown").reason, "operation");
  assert.equal(h.request("watch", { targetFrame: -1 }).reason, "target");
  for (const fields of [
    { resetFrame: 384, futureEligible: true }, { resetFrame: 657, futureEligible: false },
    { resetFrame: 785, futureEligible: true },
  ]) assert.equal(h.request("watch", { targetFrame: 785, closing: true, ...fields }).reason, "closing-prerequisite");
  assert.equal(h.processor.nativeSession, session);
  assert.equal(h.processor.nativeCursor, 384);
  assert.equal(h.processor.nativeWatch.initial, true);
  for (const [fields, reason] of [[{ generation: jointGeneration - 1 }, "generation"],
    [{ id: h.processor.nativeId }, "identity"], [{}, "duplicate"]]) {
    const refused = h.attach(fields);
    assert.equal(refused.messages[0].reason, reason);
    assert.equal(refused.messages[0].rejectedPortClosed, true);
    assert.equal(refused.closes, 1);
    assert.equal(h.processor.nativeSession, session);
  }
});

test("a late third lane refuses admission while the existing watch retains bounded actual callback inputs", async () => {
  const h = await nativeHarness(); h.origin();
  const geometry = jointGeometry(529);
  assert.equal(h.request("watch", { targetFrame: 529, initial: false }).kind, "accepted");
  const late = { ...geometry, futureEligible: [true, true, false] };
  assert.equal(h.request("joint-start", late).reason, "joint-late");
  for (const lane of [0, 1]) {
    const futureEligible = [true, true, true]; futureEligible[lane] = false;
    assert.equal(h.request("joint-start", { ...geometry, futureEligible }).reason, "joint-late");
  }
  assert.equal(h.processor.nativeSession, null);
  const parameters = nativeLanes(384, 128, geometry);
  parameters[2] = new Float32Array([1]); // Actual input shape near a controlled late submission.
  h.process({ lanes: parameters });
  const record = h.port.messages.findLast(message => message.kind === "callback");
  assert.ok(record);
  assert.deepEqual(record.laneLengths, [128, 128, 1]);
  assert.deepEqual(record.laneValues, parameters.map(values => Array.from(values)));
  assert.equal(h.calls.crt.length, 0, "refused admission cannot grant joint offset evidence");
  assert.equal(h.processor.nativeSession, null);
  assert.ok(rawRecords(h.port).length <= 32);
  assert.ok(h.port.messages.length <= 128);
  h.request("retire", { reason: "case-complete" });
  assert.equal(jointSummary(h).session, null);
  assertRetired(h, "case-complete");
});

test("joint onset and unequal callback extents use common original indices and advance the native cursor once", async () => {
  const h = await nativeHarness(); h.origin();
  const geometry = jointGeometry(401);
  assert.equal(h.request("joint-start", geometry).kind, "accepted");
  let expectedCursor = 384, checked = 0;
  for (const frames of [64, 37, 91, 1]) {
    const lanes = nativeLanes(expectedCursor, frames, geometry), before = lanes.map(values => values.slice());
    h.process({ frames, lanes, rawFrame: 7000000 });
    checked += Math.max(0, expectedCursor + frames - Math.max(expectedCursor, geometry.startFrame));
    expectedCursor += frames;
    assert.equal(h.processor.nativeCursor, expectedCursor);
    assert.equal(h.processor.nativeSession.checkedSamples, checked);
    for (const [lane, values] of lanes.entries()) assert.deepEqual(values, before[lane]);
  }
  const onset = phaseSnapshots(h.port, "phase-onset")[0];
  assert.equal(onset.activeIndex, 17);
  assert.equal(onset.actualFrames, 64);
  assert.deepEqual(onset.joint, { firstIndex: 17, lastIndex: 63, firstOffset: 0, lastOffset: 46 });
  assert.deepEqual(h.calls.crt.slice(0, 2).map(call => call.values), [[1.25, 1.25, 1.25], [47.25, 47.25, 47.25]]);
  assert.equal(h.processor.nativeSession.firstCRT, 0);
  assert.equal(h.processor.nativeSession.lastCRT, checked - 1);
  assert.equal(h.processor.nativeSession.crtChecks, 8);
  assert.ok(h.processor.nativeStats.rawFrameDiscrepancies > 0, "raw globals remain observations rather than cursor authority");
});

async function streamJoint(h, { heldFault = false } = {}) {
  h.origin();
  const geometry = jointGeometry(529);
  assert.equal(h.request("joint-start", geometry).kind, "accepted");
  const resetFrame = geometry.horizonFrames[0] + 128, targetFrame = geometry.horizonFrames[0] + 384;
  assert.equal(h.request("watch", { targetFrame, closing: true, resetFrame, futureEligible: true }).kind, "accepted");
  while (h.processor.nativePort && !h.processor.nativeSession?.done) {
    const base = h.processor.nativeCursor, frames = 128;
    const marker = Float32Array.from({ length: frames }, (_, index) => {
      const frame = base + index;
      return frame < resetFrame || frame >= targetFrame ? 1 : 0;
    });
    const lanes = nativeLanes(base, frames, geometry);
    if (heldFault && base <= geometry.horizonFrames[0] && geometry.horizonFrames[0] < base + frames)
      lanes[0][geometry.horizonFrames[0] - base] = 2;
    h.process({ marker, lanes, rawFrame: base - 128 });
    assert.ok(h.ledger.length < 32768, "controlled stream must terminate before the diagnostic callback guard");
  }
  return geometry;
}

for (const rate of [44100, 48000]) {
  test(`actual joint bootstrap conserves three own horizons, tails and CRT endpoints at ${rate}`, async () => {
    const h = await nativeHarness({ rate }), geometry = await streamJoint(h);
    const summary = jointSummary(h), session = summary.session;
    assert.equal(summary.reason, "phase-complete");
    assert.equal(session.failure, null);
    assert.equal(session.done, true);
    assert.equal(session.checkedSamples, 3145719);
    assert.equal(session.checkedCallbacks, Math.ceil((17 + 3145719) / 128));
    assert.equal(session.crtChecks, 2 * session.checkedCallbacks);
    assert.equal(session.firstCRT, 0);
    assert.equal(session.lastCRT, 3145718);
    assert.equal(session.jointHorizonRefused, true);
    assert.deepEqual(session.lanes.map(lane => lane.checkedSamples), [3145728, 3145725, 3145719]);
    assert.deepEqual(session.lanes.map(lane => lane.tailSamples), [9, 6, 0]);
    for (const lane of session.lanes) {
      assert.equal(lane.checkedCallbacks, session.checkedCallbacks);
      assert.equal(lane.resets.length, 2);
      assert.equal(lane.horizonRefused, true);
      assert.ok(lane.holdFrames > 0);
      assert.ok(lane.maxError < 0.25);
    }
    const resets = phaseSnapshots(h.port, "reset");
    assert.equal(resets.length, 2, "one all-lane snapshot represents each actual reset callback cluster");
    for (const [index, record] of resets.entries()) {
      assert.deepEqual(record.laneLengths, [128, 128, 128]);
      assert.equal(record.laneValues.length, 3);
      assert.deepEqual(record.resets.map(reset => reset.lane), [0, 1, 2]);
      assert.deepEqual(record.resets.map(reset => reset.index), index === 0 ? [17, 16, 14] : [17, 15, 11]);
      assert.deepEqual(record.resets.map(reset => reset.frame), jointPeriods.map(period => geometry.startFrame + (index + 1) * period));
      assert.ok(record.resets.every(reset => reset.interior));
    }
    assert.equal(resets.flatMap(record => record.resets).length, 6);
    const horizons = phaseSnapshots(h.port, "horizon");
    assert.equal(horizons.length, 1, "the three own horizons share one retained callback record");
    const horizon = horizons[0];
    assert.deepEqual(horizon.horizons.map(fact => fact.lane), [0, 1, 2]);
    assert.deepEqual(horizon.horizons.map(fact => fact.index), [17, 14, 8]);
    assert.deepEqual(horizon.horizons.map(fact => fact.frame), geometry.horizonFrames);
    assert.ok(horizon.horizons.every(fact => fact.refused));
    assert.deepEqual(horizon.joint, { firstIndex: 0, lastIndex: 7,
      firstOffset: 3145711, lastOffset: 3145718 });
    for (const [lane, index] of [17, 14, 8].entries()) {
      assert.ok(horizon.laneValues[lane].slice(index).every(value => value === 1));
      assert.ok(horizon.laneValues[lane].slice(0, index).every(value => value > 1));
    }
    const onset = phaseSnapshots(h.port, "phase-onset");
    assert.equal(onset.length, 1);
    assert.equal(onset[0].activeIndex, 17);
    assert.deepEqual(onset[0].joint, { firstIndex: 17, lastIndex: 127, firstOffset: 0, lastOffset: 110 });
    assert.ok(onset[0].laneValues.every(values => values.slice(0, 17).every(value => value === 0)));
    const expectedCRT = h.ledger.flatMap(({ base, frames }) => {
      const first = Math.max(base, geometry.startFrame), last = Math.min(base + frames, geometry.jointHorizonFrame);
      return first < last ? [first - geometry.startFrame, last - 1 - geometry.startFrame] : [];
    });
    assert.equal(h.calls.crt.length, expectedCRT.length);
    for (const [index, call] of h.calls.crt.entries()) {
      const offset = expectedCRT[index];
      assert.equal(call.offset, offset, `actual imported CRT endpoint ${index}`);
      assert.deepEqual(call.values, jointPeriods.map(period => Math.fround(1.25 + offset % period)),
        "all lanes must select the same original sample frame");
    }
    assert.equal(phaseSnapshots(h.port, "closing-reset").length, 1);
    assert.equal(phaseSnapshots(h.port, "closing-marker").length, 1);
    assert.equal(session.closing.candidate, session.closing.expectedBase);
    assert.equal(session.closing.futureEligible, true);
    assert.equal(session.closing.resetObserved, true);
    assert.equal(session.closing.intervalFrames, session.closing.expectedBase - summary.anchor.base);
    assert.ok(summary.rawSnapshots <= 32);
    assert.ok(rawRecords(h.port).length <= 32, "include the raw closing callback sent after summary capture");
    assert.ok(h.port.messages.length <= 128);
    assert.ok(summary.stats.phase0 && summary.stats.zeroPCM && summary.stats.reportUnavailable && summary.stats.resultTrue);
    h.request("retire", { reason: "case-complete" });
    assertRetired(h, "case-complete");
  });
}

for (const [name, make, reason] of [
  ["scalar active lane", () => new Float32Array([1]), "joint-shape"],
  ["wrong active length", () => new Float32Array(127), "joint-shape"],
  ["non-Float32 active lane", () => new Uint32Array(128), "joint-shape"],
  ["unscheduled constant lane", () => new Float32Array(128), "joint-evidence"],
]) {
  test(`joint bootstrap refuses ${name} even when the other lanes are plausible`, async () => {
    const h = await nativeHarness(); h.origin();
    const geometry = jointGeometry(401);
    assert.equal(h.request("joint-start", geometry).kind, "accepted");
    const lanes = nativeLanes(384, 128, geometry); lanes[2] = make();
    const original = lanes.map(values => Array.from(values));
    h.process({ lanes });
    const session = jointSummary(h).session;
    assert.equal(session.failure.reason, reason);
    assert.equal(session.failureSample.lane, 2);
    assert.equal(session.checkedSamples, 0);
    assert.equal(h.calls.crt.length, 0, "one invalid lane must refuse the joint candidate before CRT");
    const failure = phaseSnapshots(h.port, "first-failure");
    assert.equal(failure.length, 1);
    assert.deepEqual(failure[0].laneValues, original);
    assert.deepEqual(lanes.map(values => Array.from(values)), original);
    assertRetired(h, "phase-failure");
  });
}

test("a corrupt interior sample fails actual every-sample evidence before otherwise valid CRT endpoints", async () => {
  const h = await nativeHarness(); h.origin();
  const geometry = jointGeometry(401);
  assert.equal(h.request("joint-start", geometry).kind, "accepted");
  const lanes = nativeLanes(384, 128, geometry); lanes[1][63] += 1;
  h.process({ lanes });
  const session = jointSummary(h).session;
  assert.equal(session.failure.reason, "joint-evidence");
  assert.equal(session.failureSample.lane, 1);
  assert.equal(session.failureSample.index, 63);
  assert.equal(session.failureSample.frame, 447);
  assert.equal(session.checkedSamples, 0);
  assert.equal(h.calls.crt.length, 0);
  assertRetired(h, "phase-failure");
});

test("joint held-value validation rejects a bad P0 tail after the earliest joint horizon", async () => {
  const h = await nativeHarness(); await streamJoint(h, { heldFault: true });
  const session = jointSummary(h).session;
  assert.equal(session.failure.reason, "joint-hold");
  assert.equal(session.failureSample.lane, 0);
  assert.equal(session.checkedSamples, 3145719, "joint samples end before P0's own horizon");
  assert.equal(session.lanes[0].checkedSamples, 3145728);
  assert.equal(session.lanes[0].tailSamples, 9);
  assert.equal(session.done, false);
  assert.equal(phaseSnapshots(h.port, "first-failure").length, 1);
  assertRetired(h, "phase-failure");
});

test("controlled imported-helper contradiction exercises the defensive joint CRT refusal without claiming native reachability", async () => {
  // Valid finite native lanes reconstruct their expected offsets. This is an
  // explicitly controlled imported-helper fault, not an unsafe native vector.
  const h = await nativeHarness({ jointFault: offset => offset + 1 }); h.origin();
  const geometry = jointGeometry(401);
  assert.equal(h.request("joint-start", geometry).kind, "accepted");
  h.process({ lanes: nativeLanes(384, 128, geometry), rawFrame: 9000000 });
  const session = jointSummary(h).session;
  assert.equal(session.failure.reason, "joint-crt");
  assert.equal(session.failure.frame, 384, "faulty CRT must never rebase from a returned candidate or raw globals");
  assert.ok(session.lanes.every(lane => lane.checkedSamples === 111));
  assert.deepEqual(h.calls.crt.map(call => call.offset), [0, 110], "the actual helper produced correct finite offsets before the controlled fault");
  assert.equal(phaseSnapshots(h.port, "first-failure").length, 1);
  assertRetired(h, "phase-failure");
});

for (const reason of ["timeout", "suspend", "production-stop"]) {
  test(`active joint ${reason} invalidates pending observation and closes its private port once`, async () => {
    const h = await nativeHarness(); h.origin();
    const geometry = jointGeometry(401);
    assert.equal(h.request("joint-start", geometry).kind, "accepted");
    h.process({ lanes: nativeLanes(384, 128, geometry) });
    if (reason === "production-stop") h.processor.control({ kind: "stop", generation: jointGeneration });
    else h.request("retire", { reason });
    const summary = jointSummary(h);
    assert.equal(summary.reason, reason);
    assert.equal(summary.session.checkedSamples, 111);
    assert.equal(summary.session.done, false);
    assert.equal(assertRetired(h, reason).pendingInvalidated, true);
    const rejected = h.attach();
    assert.equal(rejected.messages[0].reason, "retired");
    assert.equal(rejected.closes, 1);
  });
}

test("the actual joint callback guard retires an active session on callback 32769", async () => {
  const h = await nativeHarness(); h.origin();
  const geometry = jointGeometry(401);
  assert.equal(h.request("joint-start", geometry).kind, "accepted");
  while (h.processor.nativePort) {
    const base = h.processor.nativeCursor;
    h.process({ frames: 1, lanes: nativeLanes(base, 1, geometry) });
    assert.ok(h.ledger.length <= 32769);
  }
  const summary = jointSummary(h);
  assert.equal(summary.reason, "callback-limit");
  assert.equal(summary.ordinal, 32769);
  assert.equal(summary.stats.callbacks, 32768);
  assert.equal(summary.session.checkedSamples, 32750);
  assert.equal(summary.session.done, false);
  assert.equal(summary.session.failure, null);
  assert.ok(rawRecords(h.port).length <= 32);
  assert.ok(h.port.messages.length <= 128);
  assert.equal(assertRetired(h, "callback-limit").pendingInvalidated, true);
});

test("repeated legal joint watches reach the actual snapshot cap without a 33rd retained raw record", async () => {
  const h = await nativeHarness();
  for (let watch = 0; h.processor.nativePort; watch++) {
    assert.ok(watch < 32);
    assert.equal(h.request("watch", { targetFrame: 100000 + watch, initial: false }).kind, "accepted");
    for (let callback = 0; callback < 2 && h.processor.nativePort; callback++)
      h.process({ marker: new Float32Array([0]) });
  }
  const summary = jointSummary(h);
  assert.equal(summary.reason, "snapshot-limit");
  assert.equal(summary.rawSnapshots, 32);
  assert.equal(rawRecords(h.port).length, 32);
  assert.equal(summary.session, null);
  assert.ok(h.port.messages.length <= 128);
  assert.equal(assertRetired(h, "snapshot-limit").pendingInvalidated, true);
});
