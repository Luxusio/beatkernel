// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/multiplayer-owner.test.mjs
// Actual owner module; scripted WASM/transport edges exercise ownership, not a BKMP implementation.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

const ownerUrl = new URL("./multiplayer-owner.mjs", import.meta.url);
const transportUrl = new URL("./multiplayer-transport.mjs", import.meta.url);
const [ownerSource, transportSource] = await Promise.all([
  readFile(ownerUrl, "utf8"), readFile(transportUrl, "utf8"),
]);
const URL_ENDPOINT = "https://example.test:4433/competition";
const U64_MAX = 18446744073709551615n;
const I64_MIN = -9223372036854775808n;

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function attempt(action) {
  const tracked = { state: "pending" };
  let result;
  try { result = action(); } catch (error) { result = Promise.reject(error); }
  tracked.done = Promise.resolve(result).then(
    value => { tracked.state = "fulfilled"; tracked.value = value; },
    error => { tracked.state = "rejected"; tracked.error = error; },
  );
  return tracked;
}
async function flush() { for (let count = 0; count < 48; count++) await Promise.resolve(); }
async function success(result) {
  await result.done;
  assert.equal(result.state, "fulfilled");
  return result.value;
}
async function failure(result, code) {
  await result.done;
  assert.equal(result.state, "rejected");
  assert.equal(result.error.code, code);
  return result.error;
}
function progress(fields = {}) {
  return { songNs: I64_MIN, hits: U64_MAX, misses: 0n, combo: U64_MAX, maxCombo: U64_MAX, ...fields };
}
function admitted(id, bytes = Uint8Array.of(1, 2, 3)) { return { kind: 1, frame_id: id, bytes }; }

async function harness(faults = {}) {
  const trace = [];
  const events = [];
  const closures = [];
  const timers = new Map();
  const opens = [];
  const wrappers = [];
  const origin = 9_000_000_000n;
  let wall = 0;
  let clockOverride;
  let timerId = 0;

  function writeObject(descriptor) {
    let freed = false;
    let taken = false;
    const object = {
      descriptor, frees: 0, takes: 0,
      get kind() { assert.equal(freed, false); return descriptor.kind; },
      get frame_id() { assert.equal(freed, false); return descriptor.frame_id ?? 0n; },
      take_bytes() {
        assert.equal(freed, false); assert.equal(taken, false);
        taken = true; this.takes++; trace.push(["take", descriptor.frame_id]);
        if (descriptor.takeError) throw descriptor.takeError;
        return descriptor.bytes;
      },
      free() { this.frees++; assert.equal(freed, false); freed = true; trace.push(["write-free", descriptor.frame_id ?? 0n]); },
    };
    wrappers.push(object);
    return object;
  }

  const session = {
    freed: false, frees: 0, closes: 0, pending: true, committed: false, need: 4,
    controls: [], application: [], queuedEvents: [], received: [], writtenCalls: [], progressCalls: [], nextCalls: [],
    alive() { assert.equal(this.freed, false, "owner accessed a freed WASM session"); },
    request_ready() {
      this.alive(); trace.push(["ready"]);
      if (faults.readyError) throw faults.readyError;
    },
    needed_bytes() { this.alive(); return this.need; },
    receive_bytes(bytes, now) {
      this.alive(); this.received.push({ bytes: [...bytes], now }); trace.push(["receive", bytes.length, now]);
      if (faults.receiveError) throw faults.receiveError;
      this.onReceive?.(bytes, now);
      return faults.consumed ?? bytes.length;
    },
    next_write(now) {
      this.alive(); this.nextCalls.push(now); trace.push(["next", now]);
      if (faults.nextError) throw faults.nextError;
      return writeObject(this.controls.shift() ?? { kind: 0 });
    },
    send_progress(...args) {
      this.alive(); this.progressCalls.push(args); trace.push(["progress", ...args]);
      if (faults.progressError) throw faults.progressError;
      return writeObject(this.application.shift() ?? admitted(90n));
    },
    written(id, now) {
      this.alive(); this.writtenCalls.push({ id, now }); trace.push(["written", id, now]);
      if (faults.writtenError) throw faults.writtenError;
      this.onWritten?.(id, now);
    },
    poll_event() {
      this.alive();
      if (faults.eventError) throw faults.eventError;
      return this.queuedEvents.shift() ?? null;
    },
    setup_complete() { this.alive(); return this.committed; },
    preparation_pending() { this.alive(); return this.pending; },
    start_committed() { this.alive(); return this.committed; },
    close() { this.alive(); this.closes++; trace.push(["session-close"]); if (faults.closeError) throw faults.closeError; },
    free() { this.alive(); this.freed = true; this.frees++; trace.push(["session-free"]); },
  };

  function channel() {
    return {
      reads: [], writes: [], closes: 0, closed: false,
      readPrefix(max) { const gate = deferred(); this.reads.push({ max, gate }); trace.push(["read", max]); return gate.promise; },
      write(bytes) {
        assert.ok(wrappers.every(wrapper => wrapper.frees === 1), "write wrappers must be freed before channel.write");
        const gate = deferred(); this.writes.push({ bytes: [...bytes], gate }); trace.push(["write", [...bytes]]);
        return gate.promise;
      },
      close() { this.closes++; this.closed = true; trace.push(["channel-close"]); if (faults.channelCloseError) throw faults.channelCloseError; },
    };
  }
  function channelFactory(url, options) {
    const gate = deferred(); opens.push({ url, options, gate }); trace.push(["open-channel", url]);
    return gate.promise;
  }
  const context = createContext({
    AbortController, AbortSignal, URL, DOMException, Uint8Array, ArrayBuffer, SharedArrayBuffer, structuredClone,
    Date: class extends Date { static now() { return wall; } }, performance: { now: () => wall },
    setTimeout(callback, delay, ...args) {
      const id = ++timerId;
      timers.set(id, { at: wall + delay, callback: () => callback(...args) });
      return id;
    },
    clearTimeout(id) { timers.delete(id); },
  });
  const transport = new SourceTextModule(transportSource, { context, identifier: transportUrl.href });
  const actual = new SourceTextModule(ownerSource, { context, identifier: ownerUrl.href });
  await actual.link(specifier => { assert.equal(specifier, "./multiplayer-transport.mjs"); return transport; });
  await actual.evaluate();
  const { BrowserMultiplayerOwner, BrowserMultiplayerOwnerError } = actual.namespace;
  function opening(options = {}) {
    return attempt(() => BrowserMultiplayerOwner.open(URL_ENDPOINT, {
      session, now: () => clockOverride === undefined ? origin + BigInt(wall) * 1_000_000n : clockOverride,
      channelFactory, setupTimeoutMs: 50, tickMs: 5,
      onEvent: event => { events.push(event); trace.push(["event", event.kind]); },
      onClose: error => { closures.push(error); trace.push(["owner-close", error.code]); },
      ...options,
    }));
  }
  async function opened(options = {}) {
    const result = opening(options);
    const io = channel();
    opens.at(-1).gate.resolve(io);
    const owner = await success(result);
    await flush();
    return { owner, io };
  }
  return { BrowserMultiplayerOwner, BrowserMultiplayerOwnerError, session, events, closures, opens,
    trace, timers, wrappers, origin, channel, opening, opened,
    setClock(value) { clockOverride = value; },
    async elapse(milliseconds) {
      const target = wall + milliseconds;
      for (;;) {
        const next = [...timers].filter(([, timer]) => timer.at <= target)
          .sort((a, b) => a[1].at - b[1].at)[0];
        if (!next) break;
        const [id, timer] = next;
        timers.delete(id); wall = timer.at; timer.callback(); await flush();
      }
      wall = target; await flush();
    },
  };
}

function cleaned(h, io) {
  assert.equal(h.session.closes, 1);
  assert.equal(h.session.frees, 1);
  assert.equal(io.closes, 1);
  assert.equal(h.timers.size, 0);
  assert.ok(h.wrappers.every(wrapper => wrapper.frees === 1));
}

test("configuration refusal preserves caller ownership but admitted invalid clocks free the session", async () => {
  for (const options of [{ now: undefined }, { tickMs: 0 }, { tickMs: 1001 },
    { setupTimeoutMs: 0 }, { setupTimeoutMs: 60001 }, { channelFactory: 1 }, { onEvent: 1 }]) {
    const h = await harness();
    assert.ok(await failure(h.opening(options), "validation") instanceof h.BrowserMultiplayerOwnerError);
    assert.equal(h.session.closes, 0);
    assert.equal(h.session.frees, 0);
    assert.equal(h.opens.length, 0);
  }
  for (const now of [0, -1n, "0"]) {
    const h = await harness();
    await failure(h.opening({ now: () => now }), "clock");
    assert.equal(h.session.closes, 1);
    assert.equal(h.session.frees, 1);
    assert.equal(h.opens.length, 0);
  }
  const h = await harness();
  const { owner, io } = await h.opened();
  assert.equal(owner.origin, h.origin);
  assert.equal(owner.elapsed(), 0n);
  await h.elapse(3);
  assert.equal(owner.elapsed(), 3_000_000n);
  assert.equal(owner.closed, false);
  owner.close(); owner.close();
  cleaned(h, io);
  const large = await harness();
  const absolute = 1n << 80n;
  large.setClock(absolute);
  const active = await large.opened();
  assert.equal(active.owner.origin, absolute);
  large.setClock(absolute + 1n);
  assert.equal(active.owner.elapsed(), 1n, "only the elapsed interval must fit i64");
  active.owner.close();
  cleaned(large, active.io);
});

test("prefix acquisition, elapsed timestamps, immutable frame IDs and free-before-write use actual owner flow", async () => {
  const h = await harness();
  h.session.controls.push(admitted(U64_MAX, Uint8Array.of(0xaa, 0xbb)));
  h.session.onWritten = () => h.session.queuedEvents.push({ kind: "ready" });
  const { owner, io } = await h.opened();
  assert.equal(io.reads[0].max, 4);
  assert.deepEqual(io.writes[0].bytes, [0xaa, 0xbb]);
  assert.equal(h.session.writtenCalls.length, 0);
  assert.equal(h.wrappers[0].takes, 1);
  assert.equal(h.wrappers[0].frees, 1);
  await h.elapse(3);
  h.session.onReceive = () => {
    h.session.need = 2;
    h.session.queuedEvents.push({ kind: "connected" });
  };
  io.reads[0].gate.resolve(Uint8Array.of(1, 2, 3, 4));
  await flush();
  assert.deepEqual(h.session.received, [{ bytes: [1, 2, 3, 4], now: 3_000_000n }]);
  assert.equal(io.reads[1].max, 2);
  assert.deepEqual(h.events.map(event => event.kind), ["connected"]);
  await h.elapse(1);
  io.writes[0].gate.resolve();
  await flush();
  assert.deepEqual(h.session.writtenCalls, [{ id: U64_MAX, now: 4_000_000n }]);
  assert.deepEqual(h.events.map(event => event.kind), ["connected", "ready"]);
  owner.close();
  cleaned(h, io);
});

test("one retained submission wakes the writer while final completion waits for a real core event", async () => {
  const h = await harness();
  const { owner, io } = await h.opened();
  const waitingCalls = h.session.nextCalls.length;
  await flush();
  assert.equal(h.session.nextCalls.length, waitingCalls, "Waiting must not spin in microtasks");
  const caller = progress();
  const submit = attempt(() => owner.submit(caller, true));
  caller.songNs = 10n; caller.hits = 0n;
  const acknowledged = attempt(() => owner.wait_final_ack());
  await failure(attempt(() => owner.submit(progress(), false)), "busy");
  await failure(attempt(() => owner.wait_final_ack()), "busy");
  await flush();
  assert.equal(h.session.progressCalls.length, 0, "queued application must await a core slot");
  h.session.controls.push({ kind: 2 });
  h.session.application.push(admitted(42n, Uint8Array.of(4, 2)));
  owner.request_ready();
  await flush();
  assert.equal(h.session.progressCalls.length, 1);
  assert.deepEqual(h.session.progressCalls[0], [I64_MIN, U64_MAX, 0n, U64_MAX, U64_MAX, true, 0n]);
  assert.deepEqual(io.writes[0].bytes, [4, 2]);
  await failure(attempt(() => owner.submit(progress(), false)), "busy");
  assert.equal(submit.state, "pending");
  io.writes[0].gate.resolve();
  assert.equal(await success(submit), undefined);
  assert.equal(acknowledged.state, "pending", "a local write is not a peer application ACK");
  assert.deepEqual(h.session.writtenCalls, [{ id: 42n, now: 0n }]);
  h.session.onReceive = () => h.session.queuedEvents.push({ kind: "final-acknowledged" });
  io.reads[0].gate.resolve(Uint8Array.of(9));
  assert.equal(await success(acknowledged), undefined);
  assert.equal(await success(attempt(() => owner.wait_final_ack())), undefined);
  owner.close();
  cleaned(h, io);
});

test("absolute preparation deadline includes connection and readiness without resetting after open", async () => {
  const h = await harness();
  const opening = h.opening();
  await h.elapse(40);
  const io = h.channel();
  h.opens[0].gate.resolve(io);
  const owner = await success(opening);
  assert.equal(owner.elapsed(), 40_000_000n);
  const pending = attempt(() => owner.submit(progress(), true));
  const final = attempt(() => owner.wait_final_ack());
  await h.elapse(9);
  assert.equal(owner.closed, false);
  await h.elapse(1);
  const error = await failure(pending, "timeout");
  assert.equal(await failure(final, "timeout"), error);
  assert.equal(h.closures[0], error);
  assert.equal(owner.closed, true);
  assert.equal(await failure(attempt(() => owner.submit(progress())), "timeout"), error);
  cleaned(h, io);

  const completed = await harness();
  const active = await completed.opened();
  completed.session.pending = false;
  completed.session.committed = true;
  completed.session.onReceive = () => completed.session.queuedEvents.push({ kind: "start", targetNs: 100n,
    songTargetNs: 200n, uncertaintyNs: 4n });
  active.io.reads[0].gate.resolve(Uint8Array.of(1));
  await flush();
  await completed.elapse(51);
  assert.equal(active.owner.closed, false, "genuine completed preparation cancels its deadline");
  active.owner.close();
  cleaned(completed, active.io);

  const connecting = await harness();
  const pendingOpen = connecting.opening();
  await connecting.elapse(50);
  await failure(pendingOpen, "timeout");
  assert.equal(connecting.session.frees, 1);
  const late = connecting.channel();
  connecting.opens[0].gate.resolve(late);
  await flush();
  assert.equal(late.closes, 1);
  assert.equal(late.reads.length + late.writes.length, 0);
  assert.equal(connecting.timers.size, 0);
});

test("abort closes late connections and pending reads or writes cannot touch the freed session", async () => {
  const opening = await harness();
  const abort = new AbortController();
  const result = opening.opening({ signal: abort.signal });
  abort.abort();
  const openingError = await failure(result, "aborted");
  assert.equal(opening.session.frees, 1);
  assert.equal(opening.opens[0].options.signal.aborted, true);
  const late = opening.channel();
  opening.opens[0].gate.resolve(late);
  await flush();
  assert.equal(late.closes, 1);
  assert.equal(late.reads.length + late.writes.length, 0);
  assert.equal(opening.closures[0], openingError);
  assert.equal(opening.timers.size, 0);

  const h = await harness();
  const signal = new AbortController();
  h.session.controls.push({ kind: 2 });
  const { owner, io } = await h.opened({ signal: signal.signal });
  h.session.controls.push({ kind: 2 });
  const pending = attempt(() => owner.submit(progress(), true));
  const ack = attempt(() => owner.wait_final_ack());
  await flush();
  assert.equal(io.writes.length, 1);
  signal.abort();
  const error = await failure(pending, "aborted");
  assert.equal(await failure(ack, "aborted"), error);
  assert.equal(h.closures[0], error);
  const events = h.events.length;
  io.reads[0].gate.resolve(Uint8Array.of(1));
  io.writes[0].gate.resolve();
  await flush();
  assert.equal(h.session.writtenCalls.length, 0);
  assert.equal(h.events.length, events);
  owner.close();
  cleaned(h, io);
});

test("pending local write receipts drain events before EOF while invalid prefix evidence fences", async () => {
  const h = await harness();
  h.session.controls.push(admitted(10n));
  h.session.onWritten = () => h.session.queuedEvents.push({ kind: "final-acknowledged" });
  const { owner, io } = await h.opened();
  const acknowledged = attempt(() => owner.wait_final_ack());
  io.reads[0].gate.reject(Object.assign(new Error("EOF"), { code: "closed" }));
  await flush();
  assert.equal(owner.closed, false, "read EOF must wait for already pending local completion");
  io.writes[0].gate.resolve();
  await success(acknowledged);
  await flush();
  const receipt = h.trace.findIndex(row => row[0] === "written");
  const event = h.trace.findIndex(row => row[0] === "event" && row[1] === "final-acknowledged");
  const closed = h.trace.findIndex(row => row[0] === "owner-close");
  assert.ok(receipt >= 0 && event > receipt && closed > event);
  assert.equal(owner.closed, true);
  cleaned(h, io);

  for (const faults of [{ consumed: 0 }, { receiveError: new Error("actual core refused input") }]) {
    const h = await harness(faults);
    const { owner, io } = await h.opened();
    const queued = attempt(() => owner.submit(progress()));
    io.reads[0].gate.resolve(Uint8Array.of(1, 2));
    const error = await failure(queued, "core");
    assert.equal(owner.closed, true);
    assert.equal(await failure(attempt(() => owner.wait_final_ack()), "core"), error);
    cleaned(h, io);
  }
  for (const prefix of [null, new Uint8Array(0), new Uint8Array(5),
    new Uint8Array(new SharedArrayBuffer(2))]) {
    const h = await harness();
    const { owner, io } = await h.opened();
    const ack = attempt(() => owner.wait_final_ack());
    io.reads[0].gate.resolve(prefix);
    await failure(ack, "transport");
    assert.equal(h.session.received.length, 0, "bad channel extents never cross WASM glue");
    cleaned(h, io);
  }
});

test("core, callback, clock, and output-object faults retain the first error and free all ownership", async () => {
  const cause = new Error("actual write acknowledgement refused");
  const core = await harness({ writtenError: cause });
  core.session.controls.push({ kind: 2 });
  const running = await core.opened();
  core.session.controls.push({ kind: 2 });
  const submitted = attempt(() => running.owner.submit(progress()));
  await flush();
  running.io.writes[0].gate.resolve();
  const error = await failure(submitted, "core");
  assert.equal(error.cause, cause);
  assert.equal(core.closures[0], error);
  cleaned(core, running.io);

  const callback = await harness();
  const callbackCause = new Error("consumer failed");
  const active = await callback.opened({ onEvent() { throw callbackCause; } });
  const ack = attempt(() => active.owner.wait_final_ack());
  const queued = attempt(() => active.owner.submit(progress()));
  callback.session.onReceive = () => callback.session.queuedEvents.push({ kind: "final-acknowledged" });
  active.io.reads[0].gate.resolve(Uint8Array.of(1));
  assert.equal(await success(ack), undefined, "proven peer ACK survives a later callback failure");
  const callbackError = await failure(queued, "callback");
  assert.equal(callbackError.cause, callbackCause);
  assert.equal(callback.closures[0], callbackError);
  cleaned(callback, active.io);

  const closing = await harness();
  let closeFromEvent;
  const local = await closing.opened({ onEvent() { closeFromEvent(); } });
  closeFromEvent = () => local.owner.close();
  closing.session.controls.push({ kind: 2 });
  closing.session.onWritten = () => closing.session.queuedEvents.push({ kind: "ready" });
  const accepted = attempt(() => local.owner.submit(progress()));
  await flush();
  local.io.writes[0].gate.resolve();
  assert.equal(await success(accepted), undefined, "proven local write survives callback-owned close");
  assert.equal(local.owner.closed, true);
  assert.equal(closing.session.writtenCalls.length, 1);
  cleaned(closing, local.io);

  const flooded = await harness();
  const flood = await flooded.opened();
  const waiting = attempt(() => flood.owner.wait_final_ack());
  flooded.session.onReceive = () => flooded.session.queuedEvents.push(
    ...Array.from({ length: 9 }, () => ({ kind: "connected" })),
  );
  flood.io.reads[0].gate.resolve(Uint8Array.of(1));
  await failure(waiting, "core");
  assert.equal(flooded.events.length, 8, "bounded callback delivery retains only admitted prefix");
  cleaned(flooded, flood.io);

  for (const time of [8_999_999_999n, 9_000_000_000n + 9223372036854775808n]) {
    const h = await harness({ closeError: new Error("close failed") });
    const { owner, io } = await h.opened();
    h.setClock(time);
    const clockError = await failure(attempt(() => owner.elapsed()), "clock");
    assert.equal(await failure(attempt(() => owner.request_ready()), "clock"), clockError);
    owner.close();
    cleaned(h, io);
  }
  const malformed = await harness();
  const takeCause = new Error("take failed");
  malformed.session.controls.push({ ...admitted(1n), takeError: takeCause });
  const opening = malformed.opening();
  const io = malformed.channel();
  malformed.opens[0].gate.resolve(io);
  await flush();
  // Opening may finish before its writer reaches this admitted object.
  if (opening.state === "fulfilled") assert.equal(opening.value.closed, true);
  else await failure(opening, "core");
  assert.equal(malformed.closures[0].code, "core");
  assert.equal(malformed.closures[0].cause, takeCause);
  assert.equal(malformed.wrappers[0].frees, 1);
  assert.equal(io.writes.length, 0);
  cleaned(malformed, io);
});
