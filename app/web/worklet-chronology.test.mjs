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

function harness({ pendingNavigation = false, pendingEvaluation = false, pendingClose = false,
  controller = controllerSource, controllerName = "runNativeCase" } = {}) {
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
  const runNativeCase = runInContext(`${controller}\n${controllerName};`, context, { filename: sourceUrl.pathname });
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

// Product-only controller coverage executes the actual function. Deliberately
// stalled controlled endpoints prove the total budget and owned cleanup; they
// do not establish native AudioParam precision or successful playback.
const productControllerStart = source.indexOf("async function runProductCase(");
const productControllerEnd = source.indexOf("\nfunction verifyProduct(", productControllerStart);
assert.ok(productControllerStart >= 0 && productControllerEnd > productControllerStart,
  "the actual long product controller and verifier must remain extractable");
const productControllerSource = source.slice(productControllerStart, productControllerEnd);
const productSpec = { name: "product-48000", rate: 48000, productProbe: true };

function productHarness(options = {}) {
  return harness({ ...options, controller: productControllerSource, controllerName: "runProductCase" });
}

function productTimeout(outcome) {
  assert.equal(outcome.settled, true, "the product case must finish after bounded cleanup");
  assert.equal(outcome.ok, false);
  assert.match(String(outcome.error), /product/i);
  assert.match(String(outcome.error), /deadline|watchdog|timeout/i);
}

test("product controller bounds page acquisition at 180 seconds without changing diagnostic bounds", async () => {
  const h = productHarness(), pending = observe(h.runNativeCase(productSpec, 311));
  await flush();
  assert.deepEqual(h.calls.acquire, [0]);
  await h.clock.advance(179999);
  assert.equal(pending.state.settled, false);
  await h.clock.advance(1);
  productTimeout(pending.state);
  assert.equal(h.calls.goto.length, 0);
  assert.equal(h.calls.evaluate.length, 0);
  assert.equal(h.clock.timers.size, 0);
});

test("a product page acquired after the total deadline closes without navigation or evaluation", async () => {
  const h = productHarness(), pending = observe(h.runNativeCase(productSpec, 312));
  await h.clock.advance(180000);
  productTimeout(pending.state);
  const original = pending.state.error;
  h.acquisition.resolve(h.page);
  await flush();
  assert.deepEqual(h.calls.close, [180000]);
  assert.equal(h.calls.goto.length, 0);
  assert.equal(h.calls.evaluate.length, 0);
  assert.equal(pending.state.error, original);
  assert.equal(h.clock.timers.size, 0);
});

test("product acquisition and navigation share one 180-second budget and successful cleanup clears timers", async () => {
  const h = productHarness({ pendingNavigation: true, pendingEvaluation: true });
  const pending = observe(h.runNativeCase(productSpec, 313));
  await h.clock.advance(60000);
  h.acquisition.resolve(h.page);
  await flush();
  assert.equal(h.calls.goto[0].options[0].timeout, 120000);
  await h.clock.advance(50000);
  h.navigation.resolve();
  await flush();
  assert.equal(h.calls.evaluate[0].input.caseBudgetMs, 70000,
    "page-side evaluation cannot receive a restarted budget");
  await h.clock.advance(69999);
  assert.equal(pending.state.settled, false);
  h.evaluation.resolve({ cleanup: {} });
  await flush();
  assert.equal(pending.state.ok, true);
  assert.equal(pending.state.value.cleanup.pageClosed, true);
  assert.deepEqual(h.calls.close, [179999]);
  assert.equal(h.clock.timers.size, 0);
  await h.clock.advance(210000);
  assert.equal(pending.state.ok, true);
  assert.equal(h.calls.close.length, 1);
});

for (const stage of ["navigation", "evaluation"]) {
  test(`product stalled ${stage} uses the original deadline and independent three-second page close`, async () => {
    const h = productHarness({ pendingNavigation: true, pendingEvaluation: true, pendingClose: true });
    const pending = observe(h.runNativeCase(productSpec, stage === "navigation" ? 315 : 316));
    await h.clock.advance(60000);
    h.acquisition.resolve(h.page);
    await flush();
    if (stage === "evaluation") {
      await h.clock.advance(50000);
      h.navigation.resolve();
      await flush();
      assert.equal(h.calls.evaluate.length, 1);
    }
    await h.clock.advance(179999 - h.clock.now());
    assert.equal(pending.state.settled, false);
    assert.equal(h.calls.close.length, 0);
    await h.clock.advance(1);
    assert.deepEqual(h.calls.close, [180000]);
    assert.equal(pending.state.settled, false);
    await h.clock.advance(2999);
    assert.equal(pending.state.settled, false);
    await h.clock.advance(1);
    productTimeout(pending.state);
    assert.match(String(pending.state.error), /(?:case|controller).*(?:deadline|watchdog|timeout)/i);
    assert.equal(h.clock.timers.size, 0);
    const original = pending.state.error;
    h.navigation.resolve(); h.evaluation.resolve({ cleanup: {} }); h.closing.resolve();
    await flush();
    assert.equal(pending.state.error, original, "late completions cannot revive an expired product case");
    assert.equal(h.calls.evaluate.length, stage === "evaluation" ? 1 : 0);
    assert.equal(h.calls.close.length, 1);
    assert.equal(h.clock.timers.size, 0);
  });
}

test("rejected product acquisition retains the original failure and clears its total deadline", async () => {
  const h = productHarness(), pending = observe(h.runNativeCase(productSpec, 314));
  await h.clock.advance(60000);
  const failure = new Error("controlled owned product page acquisition failed");
  h.acquisition.reject(failure);
  await flush();
  assert.equal(pending.state.ok, false);
  assert.equal(pending.state.error, failure);
  assert.equal(h.calls.goto.length, 0);
  assert.equal(h.calls.close.length, 0);
  assert.equal(h.clock.timers.size, 0);
});

// Execute actual mode admission and launch options with a controlled launch
// endpoint. No HTTPS server, certificate command or real browser is acquired.
async function modeAdmission(environment) {
  const declarationsStart = source.indexOf("const requireCold =");
  const declarationsEnd = source.indexOf("let browser, server;", declarationsStart);
  const entryStart = source.indexOf("\ntry {\n", source.indexOf("function verify("));
  const launchStart = source.indexOf("browser = await puppeteer.launch(", entryStart);
  const launchEnd = source.indexOf("] });", launchStart) + "] });".length;
  assert.ok(declarationsStart >= 0 && declarationsEnd > declarationsStart);
  assert.ok(entryStart >= 0 && launchStart > entryStart && launchEnd > launchStart);
  const calls = { serve: 0, require: 0, launch: [] }, sentinel = new Error("controlled launch endpoint");
  const context = createContext({ assert, process: { env: environment },
    serve: async () => { calls.serve++; return "controlled-spki"; },
    createRequire: () => { calls.require++; return () => ({ launch(options) {
      calls.launch.push(structuredClone(options)); throw sentinel;
    } }); },
  });
  const declarations = source.slice(declarationsStart, declarationsEnd);
  // import.meta.url is a module location argument only, not mode authority.
  const startup = source.slice(entryStart + "\ntry {\n".length, launchEnd)
    .replace("createRequire(import.meta.url)", "createRequire('controlled://fixture')");
  const promise = runInContext(`(async () => { ${declarations}\nlet browser, server;\n${startup}\n})()`, context);
  let error;
  try { await promise; } catch (failure) { error = failure; }
  return { calls, error, sentinel };
}

for (const other of ["WORKLET_NATIVE_FRAME_PROBE", "WORKLET_NATIVE_PHASE_PROBE",
  "WORKLET_NATIVE_JOINT_PHASE_PROBE", "WORKLET_CHRONOLOGY_REQUIRE_COLD"]) {
  test(`actual product entry rejects ${other} before resource acquisition`, async () => {
    const result = await modeAdmission({ WORKLET_NATIVE_CLOCK_PRODUCT: "1", [other]: "1" });
    assert.notEqual(result.error, result.sentinel);
    assert.match(String(result.error), /WORKLET_NATIVE_CLOCK_PRODUCT/);
    assert.equal(result.calls.serve, 0);
    assert.equal(result.calls.require, 0);
    assert.equal(result.calls.launch.length, 0);
  });
}

for (const [environment, timeout] of [
  [{ WORKLET_NATIVE_CLOCK_PRODUCT: "1" }, 210000],
  [{ WORKLET_NATIVE_PHASE_PROBE: "1" }, 150000],
  [{ WORKLET_NATIVE_JOINT_PHASE_PROBE: "1" }, 150000],
  [{ WORKLET_NATIVE_FRAME_PROBE: "1" }, 30000],
  [{ WORKLET_CHRONOLOGY_REQUIRE_COLD: "1" }, undefined],
]) {
  test(`actual mode launch preserves CDP timeout ${timeout ?? "default"} for ${Object.keys(environment)[0]}`, async () => {
    const result = await modeAdmission(environment);
    assert.equal(result.error, result.sentinel);
    assert.equal(result.calls.serve, 1);
    assert.equal(result.calls.require, 1);
    assert.equal(result.calls.launch.length, 1);
    assert.equal(result.calls.launch[0].protocolTimeout, timeout);
  });
}

const chronologyTemplateStart = source.indexOf("const bootstrap = `");
const chronologyTemplateEnd = source.indexOf("\n`;", chronologyTemplateStart) + "\n`;".length;
assert.ok(chronologyTemplateStart >= 0 && chronologyTemplateEnd > chronologyTemplateStart);
const chronologyTemplate = source.slice(chronologyTemplateStart, chronologyTemplateEnd);

async function chronologyHarness() {
  const calls = { process: [], control: [], native: [], arm: [], messages: [] };
  const state = { frame: 384, armStatus: 0, nativeStatus: 0, available: 0, expected: 0, start: null };
  class Production {
    constructor() {
      this.phase = 1; this.failed = false; this.generation = 311;
      this.port = { postMessage(message) { calls.messages.push(structuredClone(message)); } };
      this.owner = {
        native_clock_start(...args) { calls.native.push({ kind: "start", args }); return state.nativeStatus; },
        native_frame(...args) { calls.native.push({ kind: "frame", args }); return state.frame; },
        arm_words(...args) {
          calls.arm.push(args);
          if (state.armStatus === 0) {
            state.start = args[0] + args[1] * 4294967296;
            state.expected = args[2] + args[3] * 4294967296;
          }
          return state.armStatus;
        },
        report_word(index, high) {
          const value = index === 0 ? state.available : index === 23 ? 1
            : index === 24 ? state.expected : index === 25 ? Number(state.start !== null)
            : index === 26 ? state.start ?? 0 : index === 1 ? Math.max(0, state.frame - (state.start ?? state.frame))
            : index === 2 ? 128 : index === 12 ? Math.max(0, state.expected - (state.start ?? state.expected)) : 0;
          return high ? Math.floor(value / 4294967296) : value % 4294967296;
        },
      };
    }
    control(message) { calls.control.push(message); if (message.kind === "arm") this.pendingStart = message.startFrame; }
    silence(outputs) { for (const bus of outputs) for (const channel of bus) channel.fill(0); }
    process(inputs, outputs, parameters) {
      calls.process.push({ inputs, outputs, parameters });
      this.silence(outputs);
      const frame = this.owner.native_frame(0, 0, 0, 127, 127, 127, 0, outputs[0][0].length);
      if (frame < 0) { this.failed = true; return true; }
      if (this.pendingStart !== undefined) {
        const target = this.pendingStart;
        const status = this.owner.arm_words(Number(target & 0xffffffffn), Number(target >> 32n),
          frame % 4294967296, Math.floor(frame / 4294967296));
        this.pendingStart = undefined;
        if (status === 0) this.phase = 2;
      }
      state.expected = frame + outputs[0][0].length;
      state.available = Number(this.phase === 2);
      return true;
    }
  }
  let Processor;
  const context = createContext({ currentFrame: 999999, Float32Array,
    registerProcessor(name, value) { assert.equal(name, "beatkernel-audio"); Processor = value; } });
  const text = runInContext(`${chronologyTemplate}\nbootstrap;`, context);
  const helpers = new SourceTextModule(nativeHelperSource, { context });
  await helpers.link(specifier => { throw Error(`Unexpected helper import: ${specifier}`); });
  const bindings = new SyntheticModule(["Production"], function () { this.setExport("Production", Production); }, { context });
  const actual = new SourceTextModule(`${text}\nimport { Production } from '__chronology-production';\nregisterProcessor('beatkernel-audio', Production);`, { context });
  await actual.link(specifier => {
    if (specifier === "./worklet-native-frame-probe.mjs") return helpers;
    if (specifier === "__chronology-production") return bindings;
    throw Error(`Unexpected chronology bootstrap import: ${specifier}`);
  });
  await actual.evaluate();
  const processor = new Processor();
  const inputs = [[]], outputs = [[new Float32Array(128), new Float32Array(128)]];
  const parameters = { original: new Float32Array([9]) };
  return { processor, calls, state, context, inputs, outputs, parameters,
    process() { return processor.process(inputs, outputs, parameters); } };
}

test("actual chronology bootstrap forwards original inputs, outputs and parameters and records numeric native result", async () => {
  const h = await chronologyHarness();
  h.process();
  assert.equal(h.calls.process.length, 1);
  assert.equal(h.calls.process[0].inputs, h.inputs);
  assert.equal(h.calls.process[0].outputs, h.outputs);
  assert.equal(h.calls.process[0].parameters, h.parameters, "original parameter dictionary must be delegated unchanged");
  const receipt = h.processor.probe.successful[0];
  assert.equal(receipt.currentFrame, 384);
  assert.equal(receipt.rawGlobalFrame, 999999);
  assert.equal(receipt.blockFrames, 128);
  assert.equal(h.processor.previousUnarmedEnd, 512);
  assert.deepEqual(h.calls.native[0].args, [0, 0, 0, 127, 127, 127, 0, 128]);
});

test("actual chronology arm evidence is absent at control and comes from callback-time numeric arm_words", async () => {
  const h = await chronologyHarness(), message = { kind: "arm", startFrame: 384n };
  h.processor.control(message);
  assert.equal(h.calls.control[0], message);
  assert.equal(h.calls.arm.length, 0);
  assert.equal(h.processor.probe.armedAt, null);
  assert.equal(h.processor.probe.armSnapshot, null);
  h.process();
  assert.deepEqual(h.calls.arm, [[384, 0, 384, 0]]);
  assert.equal(h.processor.probe.armedAt, 384);
  assert.equal(h.processor.probe.armSnapshot.currentFrame, 384);
  assert.equal(h.processor.probe.armSnapshot.rawGlobalFrame, 999999);
  assert.equal(h.processor.probe.armSnapshot.expectedLow, 384);
  assert.equal(h.processor.probe.armSnapshot.startLow, 384);
  assert.equal(h.processor.probe.firstMixer.currentFrame, 384);
  assert.equal(h.processor.probe.firstMixer.rawGlobalFrame, 999999);
  assert.equal(h.processor.probe.successful[0].phase, 2);
});

test("failed numeric native/arm observations cannot be replaced by plausible published globals", async () => {
  const h = await chronologyHarness();
  h.state.armStatus = 6;
  h.processor.control({ kind: "arm", startFrame: 384n });
  h.process();
  assert.equal(h.processor.probe.armedAt, null);
  assert.equal(h.processor.probe.armSnapshot, null);
  assert.equal(h.processor.probe.firstMixer, null);
  const retained = h.processor.probe.successful.length;
  h.state.frame = -1;
  h.context.currentFrame = 512;
  h.process();
  assert.equal(h.processor.probe.successful.length, retained);
  assert.equal(h.processor.probe.successful[0].currentFrame, 384);
});

test("native gap observation and retained chronology receipts use actual callback coordinates and stay bounded32", async () => {
  const h = await chronologyHarness();
  h.process();
  h.state.frame = 768; h.context.currentFrame = 512;
  h.process();
  const gap = h.processor.probe.firstUnarmedGap;
  assert.equal(gap.previousEnd, 512);
  assert.equal(gap.currentFrame, 768);
  assert.equal(gap.rawGlobalFrame, 512);
  assert.equal(gap.accepted, true);
  for (let index = 0; index < 40; index++) {
    h.state.frame = 896 + index * 128;
    h.context.currentFrame = 0;
    h.process();
  }
  assert.equal(h.processor.probe.successful.length, 32);
  assert.equal(h.processor.probe.successful.at(-1).currentFrame, 5888);
  assert.equal(h.processor.probe.firstUnarmedGap, gap);
});

const productTemplateStart = source.indexOf("const productBootstrap = `");
const productTemplateEnd = source.indexOf("\n`;", productTemplateStart) + "\n`;".length;
assert.ok(productTemplateStart >= 0 && productTemplateEnd > productTemplateStart);
const productTemplate = source.slice(productTemplateStart, productTemplateEnd);
const productClockNames = ["__beatkernel_clock_0", "__beatkernel_clock_1", "__beatkernel_clock_2"];

async function productObserverHarness({ rate = 48000 } = {}) {
  const calls = { process: [], control: [], frame: [], arm: [], messages: [] };
  const state = { frame: 384, frames: 128, start: 384, available: 1, commitStatus: 0 };
  class Production {
    constructor(options) {
      this.generation = options.processorOptions.generation;
      this.phase = 2; this.failed = false;
      this.port = { postMessage(message) { calls.messages.push(structuredClone(message)); } };
      this.owner = {
        native_clock_start() { return 0; },
        native_frame(...args) { calls.frame.push(args); return state.frame; },
        arm_words(...args) { calls.arm.push(args); return 0; },
        report_word(index, high) {
          const value = index === 0 ? state.available : index === 24 ? state.frame + state.frames
            : index === 26 ? state.start : index === 1 ? state.frame - state.start
            : index === 2 ? state.frames : 0;
          return high ? Math.floor(value / 4294967296) : value % 4294967296;
        },
      };
    }
    control(message) {
      calls.control.push(message);
      if (message.kind === "clock-commit") this.port.postMessage({ kind: "clock-committed",
        generation: this.generation, revision: message.revision, status: state.commitStatus });
    }
    process(inputs, outputs, parameters) {
      calls.process.push({ inputs, outputs, parameters });
      this.owner.native_frame(0, 0, 0, 0, 0, 0, 0, outputs[0][0].length);
      for (const bus of outputs) for (const channel of bus) channel.fill(0.5);
      return true;
    }
  }
  class Port {
    messages = []; starts = 0; closes = 0; onmessage = null;
    postMessage(message) { this.messages.push(structuredClone(message)); }
    start() { this.starts++; }
    close() { this.closes++; }
    send(message) { this.onmessage({ data: message }); }
  }
  let Processor;
  const context = createContext({ Float32Array, sampleRate: rate, currentFrame: 999999,
    registerProcessor(name, value) { assert.equal(name, "beatkernel-audio"); Processor = value; } });
  const text = runInContext(`${productTemplate}\nproductBootstrap;`, context);
  const helpers = new SourceTextModule(nativeHelperSource, { context });
  await helpers.link(specifier => { throw Error(`Unexpected helper import: ${specifier}`); });
  const bindings = new SyntheticModule(["Production"], function () { this.setExport("Production", Production); }, { context });
  const actual = new SourceTextModule(`${text}\nimport { Production } from '__chronology-production';\nregisterProcessor('beatkernel-audio', Production);`, { context });
  await actual.link(specifier => {
    if (specifier === "./worklet-native-frame-probe.mjs") return helpers;
    if (specifier === "__chronology-production") return bindings;
    throw Error(`Unexpected product bootstrap import: ${specifier}`);
  });
  await actual.evaluate();
  const processor = new Processor({ processorOptions: { generation: 311 } }), port = new Port();
  processor.control({ kind: "__product-attach", generation: 311, port });
  const origin = 273;
  processor.owner.native_clock_start(origin, 0,
    origin + 3 * jointPeriods[0], 0, origin + 3 * jointPeriods[1], 0, origin + 3 * jointPeriods[2], 0);
  return { processor, port, context, calls, state, origin,
    process({ frame = state.frame, frames = 128, corrupt, raw = 999999 } = {}) {
      state.frame = frame; state.frames = frames; context.currentFrame = raw;
      const inputs = [[]], outputs = [[new Float32Array(frames), new Float32Array(frames)]];
      const lanes = jointPeriods.map(period => Float32Array.from({ length: frames }, (_, index) =>
        frame + index < origin ? 0 : Math.fround(1.25 + (frame + index - origin) % period)));
      if (corrupt) corrupt(lanes);
      const marker = new Float32Array([1]);
      const parameters = { __beatkernel_clock_marker: marker,
        ...Object.fromEntries(productClockNames.map((name, lane) => [name, lanes[lane]])) };
      const before = lanes.map(values => Array.from(values));
      const result = processor.process(inputs, outputs, parameters);
      assert.equal(result, true);
      assert.equal(calls.process.at(-1).parameters, parameters);
      assert.deepEqual(lanes.map(values => Array.from(values)), before, "native originals must not be changed by observations");
      return { inputs, outputs, parameters };
    },
    commit(revision) {
      const from = Array.from(processor.productHorizons);
      const to = from.map((value, lane) => value + jointPeriods[lane]);
      processor.control({ kind: "clock-commit", generation: 311, revision, origin,
        h0: to[0], h1: to[1], h2: to[2], futureEligible: true });
      return from;
    },
  };
}

test("actual product observer needs original native crossings after two whole refill ACKs", async () => {
  const h = await productObserverHarness();
  h.process();
  const old1 = h.commit(1);
  assert.equal(h.processor.productDone, false);
  h.process({ frame: 1024 });
  h.process({ frame: Math.floor(Math.min(...old1) / 128) * 128 });
  assert.equal(h.processor.productDone, false, "one whole refill crossed is insufficient");
  const old2 = h.commit(2);
  assert.equal(h.processor.productDone, false, "scheduling and ACKing two transactions is insufficient");
  assert.equal(h.port.messages.some(message => message.reason === "two-refills-crossed"), false);
  h.process({ frame: Math.floor(Math.max(...old1) / 128) * 128 + 128 });
  h.process({ frame: Math.floor(Math.min(...old2) / 128) * 128 });
  const summary = h.port.messages.findLast(message => message.kind === "product-summary");
  assert.ok(summary);
  assert.equal(summary.reason, "two-refills-crossed");
  assert.equal(summary.transactions.length, 2);
  for (const [index, transaction] of summary.transactions.entries()) {
    assert.equal(transaction.accepted, true);
    assert.equal(transaction.ackObserved, true);
    assert.ok(transaction.oldPrefixCheckedSamples > 0);
    assert.ok(transaction.postCommitCallbacks > 0);
    assert.deepEqual(transaction.crossings.map(value => value.lane), [0, 1, 2]);
    assert.deepEqual(transaction.crossings.map(value => value.horizon), index === 0 ? old1 : old2);
    for (const crossing of transaction.crossings) {
      const receipt = summary.receipts[crossing.receiptIndex];
      assert.equal(receipt.frame + crossing.index, crossing.horizon);
      assert.equal(receipt.laneValues[crossing.lane].length, receipt.frames);
      assert.equal(receipt.laneValues[crossing.lane][crossing.index], 1.25);
      assert.equal(receipt.rawGlobalFrame, 999999);
      assert.equal(receipt.expectedLow, receipt.frame + receipt.frames);
    }
  }
  assert.ok(summary.receipts.length <= 32);
  assert.equal(summary.stats.failed, false);
  assert.equal(summary.stats.reportConserved, true);
  assert.ok(summary.stats.nonzeroPCMCallbacks > 0);
});

test("actual product observer refuses a corrupt middle sample and stops producing completion evidence", async () => {
  const h = await productObserverHarness();
  h.process({ corrupt(lanes) { lanes[1][63] += 1; } });
  const summary = h.port.messages.findLast(message => message.kind === "product-summary");
  assert.equal(summary.reason, "native-evidence");
  assert.equal(summary.stats.failed, true);
  assert.equal(h.processor.productDone, true);
  const messages = h.port.messages.length, callbacks = h.processor.productStats.callbacks;
  h.process({ frame: 512 });
  assert.equal(h.port.messages.length, messages);
  assert.equal(h.processor.productStats.callbacks, callbacks);
  assert.equal(h.calls.process.length, 2, "ordinary production delegation remains alive after observer retirement");
});

test("actual product observer callback guard fires at65537 and never resumes observation", async () => {
  const h = await productObserverHarness();
  h.state.frame = -1;
  const inputs = [[]], outputs = [[new Float32Array(1), new Float32Array(1)]], parameters = {};
  for (let index = 0; index < 65536; index++) h.processor.process(inputs, outputs, parameters);
  assert.equal(h.processor.productDone, false);
  assert.equal(h.processor.productStats.callbacks, 65536);
  h.processor.process(inputs, outputs, parameters);
  const summary = h.port.messages.findLast(message => message.kind === "product-summary");
  assert.equal(summary.reason, "callback-limit");
  assert.equal(summary.stats.callbacks, 65537);
  assert.equal(summary.stats.failed, true);
  assert.equal(h.processor.productDone, true);
  assert.equal(summary.receipts.length, 0);
  h.processor.process(inputs, outputs, parameters);
  assert.equal(h.processor.productStats.callbacks, 65537);
});

test("actual product raw receipt guard rejects a33rd record and private retirement is generation owned", async () => {
  const h = await productObserverHarness(), original = h.process();
  for (let index = h.processor.productReceipts.length; index < 32; index++)
    h.processor.productReceipt("controlled-bound", 384, 128, original.parameters, true);
  assert.equal(h.processor.productReceipts.length, 32);
  assert.throws(() => h.processor.productReceipt("controlled-overflow", 384, 128, original.parameters, true), /receipt.*bound/i);
  assert.equal(h.processor.productReceipts.length, 32);
  h.port.send({ kind: "retire", generation: 310 });
  assert.equal(h.processor.productRetired, false);
  assert.equal(h.port.closes, 0);
  h.port.send({ kind: "retire", generation: 311 });
  assert.equal(h.processor.productRetired, true);
  assert.equal(h.port.closes, 1);
  const last = h.port.messages.at(-1);
  assert.equal(last.kind, "product-retired");
  assert.equal(last.generation, 311);
});

const productVerifierStart = source.indexOf("function verifyProduct(");
const productVerifierEnd = source.indexOf("\nasync function runNativeCase(", productVerifierStart);
assert.ok(productVerifierStart >= 0 && productVerifierEnd > productVerifierStart);
const productVerifierSource = source.slice(productVerifierStart, productVerifierEnd);

async function actualProductVerifier() {
  const helperContext = createContext();
  const helpers = new SourceTextModule(nativeHelperSource, { context: helperContext });
  await helpers.link(specifier => { throw Error(`Unexpected helper import: ${specifier}`); });
  await helpers.evaluate();
  const checks = [];
  const context = createContext({ assert, evidence: { checks }, nativeJointOffset: helpers.namespace.nativeJointOffset,
    pair: (low, high) => BigInt(low) | BigInt(high) << 32n });
  runInContext(productVerifierSource, context);
  return { checks, verifyProduct(proof) {
    // The real runner reads JSON-shaped evidence in its own realm. Preserve
    // that boundary so assert.deepStrictEqual compares its original arrays
    // with expected arrays of the same realm, without weakening assertions.
    context.proofJson = JSON.stringify(proof);
    return runInContext("verifyProduct(JSON.parse(proofJson));", context);
  } };
}

// Independently manufactured controlled proof vectors test the actual verifier
// against missing/contradictory evidence. Their ideal values are integer phase
// arithmetic, not a claim that these samples came from a browser or Rust core.
function productProofVector(rate = 48000) {
  const generation = 311, origin = 273, start = 384;
  const old1 = jointPeriods.map(period => origin + 3 * period);
  const old2 = jointPeriods.map(period => origin + 4 * period);
  const next = jointPeriods.map(period => origin + 5 * period);
  const receipt = (reason, frame, phase, ordinal) => ({
    reason, ordinal, frame, frames: 128, phase, rawGlobalFrame: 999999,
    markerValues: Array.from({ length: 128 }, (_, index) => Number(frame + index >= origin)),
    laneValues: jointPeriods.map(period => Array.from({ length: 128 }, (_, index) =>
      frame + index < origin ? 0 : Math.fround(1.25 + (frame + index - origin) % period))),
    expectedLow: phase === 2 ? frame + 128 : 0, expectedHigh: 0,
    startLow: phase === 2 ? start : 0, startHigh: 0,
    reportAvailable: Number(phase === 2), mixerStartLow: phase === 2 ? frame - start : 0,
    mixerStartHigh: 0, mixerFrames: phase === 2 ? 128 : 0, pcmNonzero: phase === 2,
  });
  const boundary1 = Math.floor(Math.min(...old1) / 128) * 128;
  const boundary2 = Math.floor(Math.min(...old2) / 128) * 128;
  const receipts = [receipt("origin", 256, 0, 3), receipt("callback-arm", 384, 2, 4),
    receipt("old-horizon", boundary1, 2, (boundary1 - 256) / 128 + 3),
    receipt("old-horizon", boundary2, 2, (boundary2 - 256) / 128 + 3)];
  const transactions = [old1, old2].map((from, index) => ({ revision: index + 1, origin,
    from, to: index === 0 ? old2 : next, accepted: true, ackObserved: true,
    oldPrefixCheckedSamples: 3 * 1000000, postCommitCallbacks: 10000,
    crossings: from.map((horizon, lane) => ({ lane, horizon, ordinal: receipts[index + 2].ordinal,
      frame: receipts[index + 2].frame, index: horizon - receipts[index + 2].frame, receiptIndex: index + 2 })) }));
  const lastNativeEnd = boundary2 + 128;
  const summary = { kind: "product-summary", reason: "two-refills-crossed", generation, origin,
    stats: { callbacks: (lastNativeEnd - 256) / 128 + 2,
      validatedCallbacks: (lastNativeEnd - 256) / 128, armedCallbacks: (lastNativeEnd - start) / 128,
      checkedSamples: 3 * (lastNativeEnd - origin), nonzeroPCMCallbacks: 30000, maxError: 0.000001,
      minFrames: 128, maxFrames: 128, firstNativeFrame: 256, lastNativeEnd,
      failed: false, resultTrue: true, reportConserved: true }, transactions, receipts };
  const identity = { generation, clockVersion: 1, clockId: 1 };
  const hostMessages = [{ ...identity, kind: "clock-ready", revision: 0, origin }];
  const sentMessages = [{ ...identity, kind: "clock-commit", revision: 0, origin,
    h0: old1[0], h1: old1[1], h2: old1[2], futureEligible: true }];
  for (const transaction of transactions) {
    hostMessages.push({ ...identity, kind: "clock-refill", revision: transaction.revision,
      from0: transaction.from[0], from1: transaction.from[1], from2: transaction.from[2],
      to0: transaction.to[0], to1: transaction.to[1], to2: transaction.to[2],
      frames: 128, endLow: origin + (transaction.revision + 1) * jointPeriods[2], endHigh: 0 });
    sentMessages.push({ ...identity, kind: "clock-commit", revision: transaction.revision, origin,
      h0: transaction.to[0], h1: transaction.to[1], h2: transaction.to[2], futureEligible: true });
    hostMessages.push({ ...identity, kind: "clock-committed", revision: transaction.revision, status: 0, error: null });
  }
  const events = [];
  const event = (parameter, method, value, frame, contextFrame = 0) => events.push({ parameter, method, value,
    time: (frame - 0.25) / rate, beforeContextTime: contextFrame / rate, afterContextTime: contextFrame / rate });
  event("__beatkernel_clock_marker", "setValueAtTime", 1, origin);
  for (const [lane, period] of jointPeriods.entries()) {
    event(productClockNames[lane], "setValueAtTime", 1, origin);
    for (let cycle = 1; cycle <= 3; cycle++) {
      event(productClockNames[lane], "linearRampToValueAtTime", period + 1, origin + cycle * period);
      event(productClockNames[lane], "setValueAtTime", 1, origin + cycle * period);
    }
    for (const transaction of transactions) {
      const scheduleFrame = origin + (transaction.revision + 1) * jointPeriods[2];
      event(productClockNames[lane], "linearRampToValueAtTime", period + 1, transaction.to[lane], scheduleFrame);
      event(productClockNames[lane], "setValueAtTime", 1, transaction.to[lane], scheduleFrame);
    }
  }
  const words = new Array(58).fill(0);
  words[0] = 1; words[2] = lastNativeEnd - start - 128; words[4] = 128;
  words[24] = lastNativeEnd - start; words[46] = 1; words[48] = lastNativeEnd;
  words[50] = 1; words[52] = start;
  return { name: `product-${rate}`, rate, generation, sampleRate: rate, pageErrors: [], hostState: "armed",
    hostMessages, sentMessages, events, beforeReport: { available: false }, report: { available: true, words },
    assetFrames: 5 * jointPeriods[0], summary,
    cleanup: { contextsConstructed: 1, nodesConstructed: 1, contextsClosed: true,
      ownedPortEndpointsClosed: 2, observerRetired: true, pageClosed: true } };
}

for (const rate of [44100, 48000]) {
  test(`actual product verifier accepts a complete controlled original-sample proof vector at${rate}`, async () => {
    const h = await actualProductVerifier(), proof = productProofVector(rate);
    h.verifyProduct(proof);
    assert.equal(h.checks.length, 1);
    assert.equal(h.checks[0].passed, true);
    assert.equal(h.checks[0].refills, 2);
    assert.deepEqual(Array.from(h.checks[0].crossedOldHorizons), proof.summary.transactions.flatMap(value => value.from));
  });
}

const productProofFaults = [
  ["two ACKed schedules without native boundary traversal", proof => {
    for (const transaction of proof.summary.transactions) transaction.crossings = [];
  }],
  ["only one whole refill", proof => { proof.summary.transactions.pop(); }],
  ["missing one old own horizon", proof => { proof.summary.transactions[1].crossings.pop(); }],
  ["duplicate crossing lane", proof => { proof.summary.transactions[1].crossings[2].lane = 1; }],
  ["crossing reindexed from the original sample", proof => { proof.summary.transactions[0].crossings[1].index++; }],
  ["crossing references no retained native input", proof => { proof.summary.transactions[0].crossings[0].receiptIndex = 32; }],
  ["scalar expansion of a native lane", proof => { proof.summary.receipts[2].laneValues[1] = [1.25]; }],
  ["plausible endpoints with corrupt original middle", proof => { proof.summary.receipts[2].laneValues[2][63] += 1; }],
  ["nonzero original prefix", proof => { proof.summary.receipts[0].laneValues[2][1] = 1.25; }],
  ["origin marker has no interior first-event edge", proof => { proof.summary.receipts[0].markerValues.fill(1); }],
  ["origin marker prefix is not virgin zero", proof => { proof.summary.receipts[0].markerValues[1] = 1; }],
  ["origin marker suffix returns to zero", proof => { proof.summary.receipts[0].markerValues[63] = 0; }],
  ["scalar marker cannot certify initial origin", proof => { proof.summary.receipts[0].markerValues = [1]; }],
  ["authenticated marker is held at zero", proof => { proof.summary.receipts[2].markerValues = [0]; }],
  ["authenticated full marker contains a bad middle", proof => { proof.summary.receipts[3].markerValues[63] = 0; }],
  ["native end before the old boundary", proof => { proof.summary.stats.lastNativeEnd = proof.summary.transactions[1].from[0]; }],
  ["no old prefix checked after commit", proof => { proof.summary.transactions[1].oldPrefixCheckedSamples = 0; }],
  ["no native callback after commit", proof => { proof.summary.transactions[0].postCommitCallbacks = 0; }],
  ["refill ACK rejected by production", proof => {
    proof.hostMessages.find(value => value.kind === "clock-committed" && value.revision === 2).status = 6;
  }],
  ["refill metadata claims ACK without actual reply", proof => {
    proof.hostMessages = proof.hostMessages.filter(value => value.kind !== "clock-committed" || value.revision !== 2);
  }],
  ["only one lane committed", proof => {
    proof.sentMessages.find(value => value.kind === "clock-commit" && value.revision === 2).h2--;
  }],
  ["second revision starts from uncommitted horizons", proof => { proof.summary.transactions[1].from[1]++; }],
  ["native API append reached OLD horizon deadline", proof => {
    const transaction = proof.summary.transactions[0];
    proof.events.find(value => value.parameter === productClockNames[0]
      && value.time === (transaction.to[0] - 0.25) / proof.rate).afterContextTime = (Math.min(...transaction.from) - 0.25) / proof.rate;
  }],
  ["duplicate reset rewrites the old native prefix", proof => {
    const old = proof.summary.transactions[0].from[0];
    proof.events.push({ parameter: productClockNames[0], method: "setValueAtTime", value: 1,
      time: (old - 0.25) / proof.rate, beforeContextTime: 0, afterContextTime: 0 });
  }],
  ["cancelled native automation history", proof => { proof.events[0].method = "cancelAndHoldAtTime"; }],
  ["old horizons traversed with no strict armed core report", proof => {
    for (const receipt of proof.summary.receipts.slice(2)) { receipt.phase = 0; receipt.reportAvailable = 0; }
  }],
  ["crossing phase is unarmed despite a plausible core report", proof => { proof.summary.receipts[2].phase = 1; }],
  ["crossing lacks actual mixer PCM despite earlier PCM", proof => { proof.summary.receipts[3].pcmNonzero = false; }],
  ["crossing changes immutable core start while conserving its local equation", proof => {
    proof.summary.receipts[2].startLow++;
    proof.summary.receipts[2].mixerStartLow--;
  }],
  ["crossing core expected frame lost original context", proof => { proof.summary.receipts[2].expectedLow--; }],
  ["crossing report mixer extent disagrees", proof => { proof.summary.receipts[3].mixerFrames--; }],
  ["final report lost absolute start association", proof => { proof.report.words[52]++; }],
  ["final report changes immutable start while preserving its local equation", proof => {
    proof.report.words[52]++; proof.report.words[2]--;
  }],
  ["observer exceeds65536 callbacks", proof => { proof.summary.stats.callbacks = 65537; }],
  ["observer retains33 original receipts", proof => {
    while (proof.summary.receipts.length < 33) proof.summary.receipts.push(structuredClone(proof.summary.receipts[1]));
  }],
  ["asset ends before two old boundary traversals", proof => { proof.assetFrames = proof.summary.stats.lastNativeEnd - proof.summary.origin; }],
  ["no actual callback-time arm observation", proof => { proof.summary.receipts[1].reason = "control-arm"; }],
  ["no actual mixer PCM", proof => {
    for (const receipt of proof.summary.receipts) receipt.pcmNonzero = false;
  }],
  ["terminal output hidden by successful summary", proof => { proof.hostMessages.push({ kind: "terminal", status: 6 }); }],
  ["owned context survives cleanup", proof => { proof.cleanup.contextsClosed = false; }],
];

for (const [name, mutate] of productProofFaults) {
  test(`actual product verifier rejects ${name}`, async () => {
    const h = await actualProductVerifier(), proof = productProofVector();
    mutate(proof);
    assert.throws(() => h.verifyProduct(proof), undefined, name);
    assert.equal(h.checks.length, 0, "contradictory or incomplete proof cannot register PASS");
  });
}

// A separately compiled observer plus a synthetic Production cannot detect
// duplicate lexical imports in the real served concatenation. Execute the
// actual HTTPS response selector and parse its whole current production body.
// This is module syntax evidence only; it does not initialize WASM or audio.
const productionWorkletSource = await readFile(new URL("./audio-worklet.js", import.meta.url), "utf8");
const servedBodyStart = source.indexOf('const body = pathname === "/__chronology.html"');
const servedBodyEnd = source.indexOf("\n      response.writeHead", servedBodyStart);
assert.ok(servedBodyStart >= 0 && servedBodyEnd > servedBodyStart);
const servedBodySelector = source.slice(servedBodyStart, servedBodyEnd);

for (const mode of ["cold", "product", "marker", "single-phase", "joint-phase"]) {
  test(`actual served ${mode} observer plus current complete Production compiles in one module`, async () => {
    const context = createContext({ production: productionWorkletSource,
      productProbe: mode === "product", nativeFixture: ["product", "marker", "single-phase", "joint-phase"].includes(mode),
      readFile() { throw Error("The served Worklet body must be assembled from actual bootstrap and production bytes"); } });
    const templates = runInContext(`${chronologyTemplate}\n${productTemplate}\n`
      + `const phaseProbe = ${mode === "single-phase"}; const jointProbe = ${mode === "joint-phase"};\n`
      + `${nativeTemplate}\n({ bootstrap, productBootstrap, nativeBootstrap });`, context);
    Object.assign(context, templates);
    const body = await runInContext(`(async () => { const pathname = '/app/web/audio-worklet.js';\n${servedBodySelector}\nreturn body; })()`, context);
    assert.ok(body.endsWith(productionWorkletSource), "the complete current Production source must remain the served suffix");
    const actual = new SourceTextModule(body, { identifier: `https://controlled.invalid/app/web/audio-worklet.js?mode=${mode}` });
    assert.equal(actual.status, "unlinked", "whole served module must parse before external imports are linked");
  });
}
