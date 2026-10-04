// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/room-owner.test.mjs
// Actual owner/transport modules; scripted WASM and channel edges are not a BKMR model.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

const ownerUrl = new URL("./room-owner.mjs", import.meta.url);
const transportUrl = new URL("./multiplayer-transport.mjs", import.meta.url);
const [ownerSource, transportSource] = await Promise.all([
  readFile(ownerUrl, "utf8"), readFile(transportUrl, "utf8"),
]);
const ENDPOINT = "https://example.test:4433/rooms/fixture";
const MAX_U64 = 18446744073709551615n;
const MAX_I64 = 9223372036854775807n;

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function attempt(action) {
  const result = { state: "pending" };
  let promise;
  try { promise = action(); } catch (error) { promise = Promise.reject(error); }
  result.done = Promise.resolve(promise).then(
    value => { result.state = "fulfilled"; result.value = value; },
    error => { result.state = "rejected"; result.error = error; },
  );
  return result;
}
async function flush() { for (let index = 0; index < 48; index++) await Promise.resolve(); }
async function success(result) {
  await result.done;
  assert.equal(result.state, "fulfilled");
  return result.value;
}
async function failure(result, code) {
  await result.done;
  assert.equal(result.state, "rejected");
  if (code !== undefined) assert.equal(result.error.code, code);
  return result.error;
}
function frameBytes(marker = 1) {
  // Opaque, minimum-sized binding output. Protocol bytes are validated by Rust.
  return Uint8Array.of(marker, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0);
}
function frame(id = MAX_U64, bytes = frameBytes()) {
  return { kind: 1, id, bytes };
}
function snapshot(count = 2) {
  return {
    phase: 0, deadlineNs: MAX_I64,
    members: Array.from({ length: count }, (_, index) => ({
      participant: MAX_U64 - BigInt(index),
      players: Uint32Array.from({ length: 64 }, (_, player) => 0xffffffff - player),
      prepared: false,
    })),
  };
}

async function harness(faults = {}) {
  const trace = [], opens = [], wrappers = [], callbacks = [], closures = [];
  const timers = new Map();
  let now = 0, nextTimer = 0;
  function wrapper(descriptor) {
    let freed = false;
    const value = {
      frees: 0, takes: 0,
      get kind() { assert.equal(freed, false); return descriptor.kind; },
      get frame_id() { assert.equal(freed, false); return descriptor.id ?? 0n; },
      take_bytes() {
        assert.equal(freed, false); assert.equal(this.takes++, 0);
        if (descriptor.takeError) throw descriptor.takeError;
        return descriptor.bytes;
      },
      free() {
        assert.equal(freed, false); freed = true; this.frees++;
        trace.push(["wrapper-free", descriptor.id ?? 0n]);
      },
    };
    wrappers.push(value);
    return value;
  }
  const session = {
    freed: false, closes: 0, frees: 0, need: 11, partial: false,
    participant: 0n, revisionValue: 0n, hasSnapshot: false, leaveDone: false, dto: null,
    controls: [frame()], receives: [], credits: [], requests: [], nextCalls: 0,
    alive() { assert.equal(this.freed, false, "WASM called after free"); },
    request_seal() { this.request("seal"); },
    request_ready() { this.request("ready"); },
    request_leave() { this.request("leave"); },
    request(kind) {
      this.alive(); this.requests.push(kind); trace.push(["request", kind]);
      if (this.requestError) throw this.requestError;
      this.onRequest?.(kind);
    },
    needed_bytes() { this.alive(); return this.need; },
    frame_pending() { this.alive(); return this.partial; },
    receive_bytes(bytes) {
      this.alive(); this.receives.push([...bytes]); trace.push(["receive", bytes.length]);
      if (faults.receiveError) throw faults.receiveError;
      this.onReceive?.(bytes);
      return faults.consumed ?? bytes.length;
    },
    next_write() {
      this.alive(); this.nextCalls++; trace.push(["next-write"]);
      if (faults.nextError) throw faults.nextError;
      return wrapper(this.controls.shift() ?? { kind: 0 });
    },
    written(id) {
      this.alive(); this.credits.push(id); trace.push(["written", id]);
      if (faults.writtenError) throw faults.writtenError;
      this.onWritten?.(id);
    },
    participant_id() { this.alive(); return this.participant; },
    revision() { this.alive(); return this.revisionValue; },
    has_snapshot() { this.alive(); return this.hasSnapshot; },
    leave_written() { this.alive(); return this.leaveDone; },
    snapshot() { this.alive(); trace.push(["snapshot"]); return this.dto; },
    close() { this.alive(); this.closes++; trace.push(["session-close"]); if (faults.sessionCloseError) throw faults.sessionCloseError; },
    free() { this.alive(); this.frees++; this.freed = true; trace.push(["session-free"]); },
  };
  function channel(options = {}) {
    const io = {
      reads: [], writes: [], closes: 0, closed: false,
      activeReads: 0, activeWrites: 0,
      readPrefix(max, waitForData = false) {
        assert.equal(this.activeReads++, 0, "only one read may be owned");
        const gate = deferred();
        this.reads.push({ max, waitForData, gate }); trace.push(["read", max, waitForData]);
        return gate.promise.finally(() => { this.activeReads--; });
      },
      write(bytes) {
        assert.equal(this.activeWrites++, 0, "only one write may be owned");
        assert.ok(wrappers.every(value => value.frees === 1), "wrapper released before transport wait");
        const gate = deferred();
        this.writes.push({ bytes: [...bytes], gate }); trace.push(["write", ...bytes]);
        return gate.promise.finally(() => { this.activeWrites--; });
      },
      close() {
        this.closes++; this.closed = true; trace.push(["channel-close"]);
        if (!options.holdAfterClose) {
          for (const entry of [...this.reads, ...this.writes]) entry.gate.reject(new Error("channel closed"));
        }
        if (options.closeError) throw options.closeError;
      },
    };
    return io;
  }
  function channelFactory(url, options) {
    const gate = deferred(); opens.push({ url, options, gate }); return gate.promise;
  }
  const context = createContext({
    AbortController, AbortSignal, URL, DOMException, Uint8Array, Uint32Array,
    ArrayBuffer, SharedArrayBuffer, structuredClone,
    Date: class extends Date { static now() { return now; } },
    performance: { now: () => now },
    setTimeout(callback, delay, ...args) {
      const id = ++nextTimer;
      timers.set(id, { at: now + delay, callback: () => callback(...args) }); return id;
    },
    clearTimeout(id) { timers.delete(id); },
  });
  const transport = new SourceTextModule(transportSource, { context, identifier: transportUrl.href });
  const ownerModule = new SourceTextModule(ownerSource, { context, identifier: ownerUrl.href });
  await ownerModule.link(specifier => { assert.equal(specifier, "./multiplayer-transport.mjs"); return transport; });
  await ownerModule.evaluate();
  const { BrowserRoomOwner, BrowserRoomOwnerError } = ownerModule.namespace;
  function opening(options = {}) {
    return attempt(() => BrowserRoomOwner.open(ENDPOINT, {
      session, channelFactory, setupTimeoutMs: 50, ioTimeoutMs: 10,
      onSnapshot: value => { callbacks.push(value); trace.push(["on-snapshot"]); },
      onClose: error => { closures.push(error); trace.push(["on-close"]); },
      ...options,
    }));
  }
  async function opened(options = {}, channelOptions = {}) {
    const result = opening(options);
    await flush();
    const io = channel(channelOptions);
    opens.at(-1).gate.resolve(io);
    const owner = await success(result);
    await flush();
    return { owner, io };
  }
  async function receive(io, action, bytes = Uint8Array.of(9)) {
    session.onReceive = action;
    io.reads.at(-1).gate.resolve(bytes);
    await flush();
    session.onReceive = undefined;
  }
  async function observe(io, dto = snapshot(), revision = session.revisionValue + 1n) {
    if (session.participant === 0n) {
      await receive(io, () => { session.participant = MAX_U64; session.revisionValue++; });
      revision = session.revisionValue + 1n;
    }
    await receive(io, () => {
      session.participant = MAX_U64; session.revisionValue = revision;
      session.hasSnapshot = true; session.dto = dto; session.partial = false;
    });
  }
  return {
    session, opens, wrappers, callbacks, closures, timers, trace,
    BrowserRoomOwner, BrowserRoomOwnerError, opening, opened, channel, receive, observe,
    async elapse(milliseconds) {
      const target = now + milliseconds;
      for (;;) {
        const due = [...timers].filter(([, timer]) => timer.at <= target)
          .sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        const [id, timer] = due; timers.delete(id); now = timer.at; timer.callback(); await flush();
      }
      now = target; await flush();
    },
  };
}

async function cleaned(h, owner, io) {
  await owner.close();
  assert.equal(owner.closed, true);
  assert.equal(h.session.closes, 1);
  assert.equal(h.session.frees, 1);
  assert.equal(io.closes, 1);
  assert.equal(h.timers.size, 0);
  assert.ok(h.wrappers.every(value => value.frees === 1));
  assert.equal(h.closures.length, 1);
}

test("configuration retains caller ownership; actual frames preserve u64 IDs and release wrappers before writes", async () => {
  for (const url of ["", "https://example.test/rooms/" + "a".repeat(4096),
    { toString() { assert.fail("non-string URL must never be coerced"); } }]) {
    const h = await harness();
    await failure(attempt(() => h.BrowserRoomOwner.open(url, { session: h.session })), "validation");
    assert.equal(h.session.closes, 0);
    assert.equal(h.session.frees, 0);
    assert.equal(h.timers.size, 0);
  }
  for (const options of [
    { setupTimeoutMs: 0 }, { setupTimeoutMs: 60001 }, { ioTimeoutMs: 0 },
    { ioTimeoutMs: 60001 }, { channelFactory: 1 }, { onSnapshot: 1 }, { onClose: 1 },
  ]) {
    const h = await harness();
    assert.ok(await failure(h.opening(options), "validation") instanceof h.BrowserRoomOwnerError);
    assert.equal(h.opens.length, 0);
    assert.equal(h.session.closes, 0); assert.equal(h.session.frees, 0);
  }
  for (const method of ["needed_bytes", "frame_pending", "receive_bytes", "next_write", "written", "revision", "snapshot", "free"]) {
    const h = await harness(); delete h.session[method];
    await failure(h.opening(), "validation");
    assert.equal(h.opens.length, 0); assert.equal(h.session.frees, 0);
  }
  const h = await harness();
  const { owner, io } = await h.opened();
  assert.equal(h.opens[0].options.maxPrefixBytes, 65808);
  assert.equal(h.opens[0].options.setupTimeoutMs, 50);
  assert.equal(h.opens[0].options.ioTimeoutMs, 10);
  assert.equal(owner.participant, 0n); assert.equal(owner.snapshot, null);
  assert.deepEqual(io.writes[0].bytes, [...frameBytes()]);
  assert.deepEqual(h.session.credits, []);
  assert.equal(h.wrappers[0].takes, 1); assert.equal(h.wrappers[0].frees, 1);
  io.writes[0].gate.resolve(); await flush();
  assert.deepEqual(h.session.credits, [MAX_U64]);
  await cleaned(h, owner, io);
  for (const descriptor of [
    { kind: 9 }, frame(0n), frame(MAX_U64 + 1n), frame(1),
    frame(1n, new Uint8Array(65809)), frame(1n, new Uint8Array(new SharedArrayBuffer(2))),
    { ...frame(1n), takeError: new Error("take failed") },
  ]) {
    const h = await harness(); h.session.controls = [descriptor];
    const { owner, io } = await h.opened();
    await flush();
    assert.equal(owner.closed, true);
    assert.deepEqual(h.session.credits, []);
    assert.equal(io.writes.length, 0);
    await cleaned(h, owner, io);
  }
});

test("idle reads keep setup ownership while incomplete frame deadlines never renew on fragments", async () => {
  const h = await harness();
  const { owner, io } = await h.opened();
  io.writes[0].gate.resolve(); await flush();
  assert.equal(io.reads[0].waitForData, true);
  await h.elapse(20);
  assert.equal(owner.closed, false, "idle read has no 10 ms frame deadline");
  await h.observe(io);
  const writes = io.writes.length, polls = h.session.nextCalls;
  await h.elapse(100);
  assert.equal(owner.closed, false); assert.equal(io.writes.length, writes);
  assert.equal(h.session.nextCalls, polls, "idle writer has no timer polling");
  await h.receive(io, () => { h.session.partial = true; h.session.need = 5; }, Uint8Array.of(1));
  assert.equal(io.reads.at(-1).max, 5);
  assert.equal(io.reads.at(-1).waitForData, false);
  await h.elapse(8);
  await h.receive(io, () => { h.session.partial = true; h.session.need = 4; }, Uint8Array.of(2));
  await h.elapse(2);
  assert.equal(owner.closed, true);
  assert.equal(h.closures[0].code, "timeout");
  await cleaned(h, owner, io);

  const never = await harness();
  const waiting = await never.opened();
  waiting.io.writes[0].gate.resolve(); await flush();
  await never.elapse(50);
  assert.equal(waiting.owner.closed, true);
  assert.equal(never.closures[0].code, "timeout");
  await cleaned(never, waiting.owner, waiting.io);
});

test("state refusals are recoverable and request wakes preserve one reader and one actual writer", async () => {
  const h = await harness();
  const { owner, io } = await h.opened();
  io.writes[0].gate.resolve(); await flush();
  await h.observe(io);
  h.session.requestError = new Error("Rust InvalidState");
  for (const invoke of [() => owner.requestSeal(), () => owner.requestReady()]) {
    assert.throws(invoke, error => error instanceof h.BrowserRoomOwnerError && error.code === "state");
  }
  assert.equal(owner.closed, false);
  assert.equal(io.writes.length, 1);
  h.session.requestError = null;
  h.session.onRequest = kind => {
    h.session.controls.push(frame(kind === "seal" ? 17n : 18n, frameBytes(kind === "seal" ? 4 : 5)));
    h.session.requestError = new Error("Rust one pending frame");
  };
  owner.requestSeal(); await flush();
  assert.equal(io.writes.length, 2);
  assert.equal(io.activeReads, 1); assert.equal(io.activeWrites, 1);
  assert.throws(() => owner.requestReady(), error => error.code === "state");
  await h.observe(io, snapshot(3));
  assert.equal(h.callbacks.length, 2, "a pending write does not block the reader");
  assert.deepEqual(h.session.credits, [MAX_U64]);
  io.writes[1].gate.resolve(); await flush();
  assert.deepEqual(h.session.credits, [MAX_U64, 17n]);
  h.session.requestError = null;
  owner.requestReady(); await flush();
  io.writes[2].gate.resolve(); await flush();
  assert.deepEqual(h.session.credits, [MAX_U64, 17n, 18n]);
  await cleaned(h, owner, io);
});

test("only validated changed revisions publish whole full-width room snapshots", async () => {
  for (const count of [2, 3, 4, 64]) {
    const h = await harness();
    const { owner, io } = await h.opened();
    io.writes[0].gate.resolve(); await flush();
    const dto = snapshot(count);
    await h.observe(io, dto);
    assert.equal(owner.participant, MAX_U64);
    assert.equal(owner.snapshot.members.length, count);
    assert.equal(owner.snapshot.members.at(-1).participant, MAX_U64 - BigInt(count - 1));
    assert.equal(owner.snapshot.members[0].players[0], 0xffffffff);
    assert.equal(owner.snapshot.deadlineNs, MAX_I64);
    assert.equal(h.callbacks.length, 1);
    await h.receive(io, () => {});
    assert.equal(h.callbacks.length, 1, "same revision cannot duplicate a snapshot notification");
    const frozen = snapshot(count); frozen.phase = 1; frozen.members[0].prepared = true;
    await h.observe(io, frozen);
    assert.equal(owner.snapshot.phase, 1);
    const prepared = snapshot(count); prepared.phase = 2; prepared.deadlineNs = null;
    for (const member of prepared.members) member.prepared = true;
    await h.observe(io, prepared);
    assert.equal(owner.snapshot.phase, 2);
    assert.equal(owner.snapshot.deadlineNs, null);
    assert.equal(h.callbacks.length, 3);
    await cleaned(h, owner, io);
  }
  const mutations = [
    dto => { dto.phase = 3; }, dto => { dto.deadlineNs = -1n; },
    dto => { dto.phase = 2; dto.deadlineNs = null; },
    dto => { dto.members[0].prepared = true; },
    dto => { dto.members = []; }, dto => { dto.members = Array.from({ length: 65 }, (_, i) => ({ ...snapshot().members[0], participant: BigInt(i + 1) })); },
    dto => { dto.members[1].participant = dto.members[0].participant; },
    dto => { dto.members[1].participant = 0n; }, dto => { dto.members[1].participant = MAX_U64 + 1n; },
    dto => { dto.members[1].players = Uint32Array.of(1, 1); },
    dto => { dto.members[1].players = Uint32Array.of(0); },
    dto => { dto.members[1].players = [1]; }, dto => { dto.members[1].prepared = 1; },
    dto => { dto.members[1].players = new Uint32Array(new SharedArrayBuffer(4)); },
    dto => { delete dto.members[1]; },
  ];
  for (const mutate of mutations) {
    const h = await harness();
    const { owner, io } = await h.opened();
    io.writes[0].gate.resolve(); await flush();
    await h.observe(io);
    const accepted = owner.snapshot;
    const dto = snapshot(); mutate(dto);
    await h.observe(io, dto);
    assert.equal(owner.closed, true);
    assert.equal(h.closures[0].code, "protocol");
    assert.equal(h.callbacks.length, 1);
    assert.equal(owner.snapshot, accepted, "a malformed later member cannot replace the accepted snapshot");
    await cleaned(h, owner, io);
  }
});

test("leave resolves only after actual transport fulfillment and Rust leave-written evidence", async () => {
  const h = await harness();
  const { owner, io } = await h.opened();
  io.writes[0].gate.resolve(); await flush();
  await h.observe(io);
  h.session.onRequest = kind => { assert.equal(kind, "leave"); h.session.controls.push(frame(23n, frameBytes(6))); };
  h.session.onWritten = id => { if (id === 23n) h.session.leaveDone = true; };
  const leaving = attempt(() => owner.leave());
  await flush();
  assert.equal(leaving.state, "pending");
  assert.equal(h.session.leaveDone, false);
  assert.deepEqual(h.session.credits, [MAX_U64]);
  assert.equal(io.writes.length, 2);
  io.writes[1].gate.resolve();
  await success(leaving);
  assert.deepEqual(h.session.credits, [MAX_U64, 23n]);
  assert.equal(h.session.leaveDone, true);
  await cleaned(h, owner, io);

  const rejected = await harness();
  const blocked = await rejected.opened();
  blocked.io.writes[0].gate.resolve(); await flush();
  await rejected.observe(blocked.io);
  rejected.session.onRequest = () => rejected.session.controls.push(frame(24n, frameBytes(6)));
  rejected.session.onWritten = id => { if (id === 24n) throw new Error("Rust refused receipt"); };
  const badLeave = attempt(() => blocked.owner.leave()); await flush();
  blocked.io.writes[1].gate.resolve();
  await failure(badLeave, "core");
  assert.equal(blocked.owner.closed, true);
  assert.equal(rejected.closures[0].code, "core");
  await cleaned(rejected, blocked.owner, blocked.io);
});

test("malformed prefixes, cancellation and late continuations fence before free and close joins owned work", async () => {
  const detached = new Uint8Array(2); structuredClone(detached.buffer, { transfer: [detached.buffer] });
  for (const bytes of [new Uint8Array(0), new Uint8Array(12), new Uint8Array(new SharedArrayBuffer(1)),
    new Uint8Array(new ArrayBuffer(1024 * 1024 + 1), 0, 1), detached, [1]]) {
    const h = await harness(); const { owner, io } = await h.opened();
    io.writes[0].gate.resolve(); await flush();
    io.reads[0].gate.resolve(bytes); await flush();
    assert.equal(owner.closed, true);
    assert.equal(h.session.receives.length, 0);
    await cleaned(h, owner, io);
  }
  for (const need of [0, 65809, 1.5, 1n]) {
    const h = await harness(); h.session.need = need;
    const { owner, io } = await h.opened(); await flush();
    assert.equal(owner.closed, true); assert.equal(io.reads.length, 0);
    await cleaned(h, owner, io);
  }
  for (const operation of ["read", "write"]) {
    const h = await harness(); const { owner, io } = await h.opened();
    const pending = operation === "read" ? io.reads[0] : io.writes[0];
    pending.gate.reject(new Error(`${operation} transport failed`)); await flush();
    assert.equal(owner.closed, true); assert.equal(h.closures[0].code, "transport");
    assert.deepEqual(h.session.credits, []);
    await cleaned(h, owner, io);
  }
  const h = await harness();
  const { owner, io } = await h.opened({}, { holdAfterClose: true });
  const closing = attempt(() => owner.close());
  await flush();
  assert.equal(owner.closed, true);
  assert.equal(closing.state, "pending", "close waits for the actual read/write continuations");
  io.writes[0].gate.resolve(); await flush();
  assert.equal(closing.state, "pending");
  io.reads[0].gate.resolve(Uint8Array.of(9));
  await success(closing);
  assert.deepEqual(h.session.credits, []);
  assert.deepEqual(h.session.receives, []);
  await cleaned(h, owner, io);

  const abort = new AbortController();
  const late = await harness();
  const opening = late.opening({ signal: abort.signal }); await flush();
  abort.abort(); await flush();
  const lateIo = late.channel(); late.opens[0].gate.resolve(lateIo);
  await failure(opening, "aborted"); await flush();
  assert.equal(lateIo.closes, 1);
  assert.equal(lateIo.reads.length, 0); assert.equal(lateIo.writes.length, 0);
  assert.equal(late.session.closes, 1); assert.equal(late.session.frees, 1);
  assert.equal(late.timers.size, 0);

  const callbacks = await harness();
  const failed = await callbacks.opened({ onSnapshot() { throw new Error("UI callback failed"); } });
  failed.io.writes[0].gate.resolve(); await flush();
  await callbacks.observe(failed.io);
  assert.equal(failed.owner.closed, true);
  assert.equal(callbacks.closures[0].code, "callback");
  await cleaned(callbacks, failed.owner, failed.io);
});

test("failed opening joins an acquired channel operation before rejecting without a public handle", async () => {
  const h = await harness();
  const signal = new AbortController();
  const opening = h.opening({ signal: signal.signal });
  await flush();
  const io = h.channel({ holdAfterClose: true });
  const write = io.write;
  io.write = function (bytes) {
    const pending = write.call(this, bytes);
    signal.abort(); // Cancel reentrantly after this channel operation is acquired.
    return pending;
  };
  h.opens[0].gate.resolve(io);
  await flush();
  assert.equal(opening.state, "pending", "failed open retains the hidden channel continuation");
  assert.equal(io.closes, 1);
  assert.equal(h.session.closes, 1);
  assert.equal(h.session.frees, 1);
  assert.deepEqual(h.session.credits, []);
  assert.equal(io.reads.length, 0);
  io.writes[0].gate.resolve();
  await failure(opening, "aborted");
  assert.deepEqual(h.session.credits, []);
  assert.deepEqual(h.session.receives, []);
  assert.equal(h.closures.length, 1);
  assert.equal(h.timers.size, 0);
});

test("cancelled opening joins a late channel acquisition and closes it before rejection", async () => {
  const h = await harness();
  const controller = new AbortController();
  const opening = h.opening({ signal: controller.signal });
  await flush();
  controller.abort();
  await flush();
  assert.equal(opening.state, "pending", "acquisition remains owned after cancellation fencing");
  assert.equal(h.session.closes, 1);
  assert.equal(h.session.frees, 1);
  const io = h.channel();
  h.opens[0].gate.resolve(io);
  await failure(opening, "aborted");
  assert.equal(io.closes, 1);
  assert.equal(io.reads.length, 0);
  assert.equal(io.writes.length, 0);
  assert.deepEqual(h.session.receives, []);
  assert.deepEqual(h.session.credits, []);
  assert.equal(h.closures.length, 1);
  assert.equal(h.timers.size, 0);
});
