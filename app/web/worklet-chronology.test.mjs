// Exercise the actual controller without importing the browser runner's startup effects.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, runInContext } from "node:vm";

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
