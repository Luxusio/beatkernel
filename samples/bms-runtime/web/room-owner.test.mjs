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
const CLOCK_ORIGIN = 9007199254740993n;

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
function preparedSnapshot() {
  const value = snapshot();
  value.phase = 2; value.deadlineNs = null;
  for (const member of value.members) member.prepared = true;
  return value;
}

async function harness(faults = {}) {
  const trace = [], opens = [], wrappers = [], callbacks = [], closures = [], starts = [];
  const progress = [], receipts = [];
  const timers = new Map();
  let now = 0, nextTimer = 0;
  let clockValue = null, clockReads = [], clockCalls = 0;
  const clock = () => { clockCalls++; return clockReads.shift() ?? clockValue ?? CLOCK_ORIGIN + BigInt(now) * 1000000n; };
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
    receiveTimes: [], writeTimes: [], pollTimes: [], schedules: [], startCalls: 0,
    publications: [], peerUpdates: [], finalWritten: false, finalAcknowledged: false,
    progressComplete: false, drainComplete: false, peerAcks: new Set(), peerAckQueries: [],
    frameConfigurations: [], frameSteps: [], frameOutputs: [], frameExpires: null,
    configure_frame_wait(timeout) {
      this.alive(); this.frameConfigurations.push(timeout);
      if (faults.configureFrameError) throw faults.configureFrameError;
      this.frameTimeout = timeout;
    },
    frame_wait_step(elapsed) {
      this.alive(); this.frameSteps.push(elapsed);
      if (this.frameOutputs.length) {
        const output = this.frameOutputs.shift();
        if (output instanceof Error) throw output;
        return output;
      }
      if (this.frameExpires !== null && elapsed >= this.frameExpires) {
        throw Object.assign(new Error("scripted common frame expiry"), { code: "timeout", operation: "frame" });
      }
      if (!this.partial) { this.frameExpires = null; return -1n; }
      this.frameExpires ??= elapsed + this.frameTimeout;
      return this.frameExpires - elapsed;
    },
    setupBegins: [], setupSteps: [], setupOutputs: [], setupPhase: "admission",
    begin_setup(elapsed, timeout) {
      this.alive(); this.setupBegins.push({ elapsed, timeout });
      if (this.beginSetupError) throw this.beginSetupError;
      this.setupTimeout = timeout; this.setupExpires = elapsed + timeout;
    },
    setup_wait_step(elapsed) {
      this.alive(); this.setupSteps.push(elapsed);
      // Compatibility scripts expose the old fake's accepted DTO transitions.
      // Independent cases below provide explicit outputs rather than a model.
      if (this.setupOutputs.length) {
        const output = this.setupOutputs.shift();
        if (output instanceof Error) throw output;
        return output;
      }
      if (this.setupPhase === "complete") return -1n;
      if (this.setupPhase !== "lobby" && elapsed >= this.setupExpires) {
        throw Object.assign(new Error("scripted common setup expiry"), {
          code: "timeout", operation: this.setupPhase === "prepared" ? "prepared" : "setup",
        });
      }
      if (this.setupPhase === "admission" && this.hasSnapshot && this.participant !== 0n) this.setupPhase = "lobby";
      if (this.setupPhase === "lobby" && this.dto?.phase === 2) {
        this.setupPhase = "prepared"; this.setupExpires = elapsed + this.setupTimeout;
      }
      if (this.setupPhase === "prepared" && this.schedules.length) {
        this.setupPhase = "complete"; return -1n;
      }
      return this.setupPhase === "lobby" ? -2n : this.setupExpires - elapsed;
    },
    drainBegins: [], drainSteps: [], drainOutputs: [], drainAdmitted: false,
    begin_drain(elapsed, timeout) {
      this.alive(); this.drainBegins.push({ elapsed, timeout });
      if (this.beginDrainError) throw this.beginDrainError;
      this.drainExpires = elapsed + timeout;
    },
    drain_requested() { this.alive(); return this.drainAdmitted; },
    drain_wait_step(elapsed) {
      this.alive(); this.drainSteps.push(elapsed);
      // Explicit scripts below exercise adapter authority. These default
      // outputs preserve the older tests' scripted WASM receipt transitions.
      if (this.drainOutputs.length) {
        const output = this.drainOutputs.shift();
        if (output instanceof Error) throw output;
        return output;
      }
      if (elapsed >= this.drainExpires) throw Object.assign(new Error("scripted common drain deadline"), { code: "timeout" });
      if (this.progressComplete && !this.drainAdmitted) {
        this.request("drain"); this.drainAdmitted = true;
      }
      return this.drainComplete ? -1n : 1000000n;
    },
    alive() { assert.equal(this.freed, false, "WASM called after free"); },
    request_seal() { this.request("seal"); },
    request_ready() { this.request("ready"); },
    request_leave() { this.request("leave"); },
    request_drain() { this.request("drain"); },
    request(kind) {
      this.alive(); this.requests.push(kind); trace.push(["request", kind]);
      if (this.requestError) throw this.requestError;
      this.onRequest?.(kind);
    },
    needed_bytes() { this.alive(); return this.need; },
    frame_pending() { this.alive(); return this.partial; },
    receive_bytes(bytes, captured, processing) {
      this.alive(); this.receives.push([...bytes]); trace.push(["receive", bytes.length]);
      this.receiveTimes.push({ captured, processing });
      if (faults.receiveError) throw faults.receiveError;
      if (this.frameExpires !== null && processing >= this.frameExpires) {
        throw Object.assign(new Error("original common late fragment refusal"), { code: "timeout", operation: "frame" });
      }
      this.onReceive?.(bytes);
      if (this.partial) this.frameExpires ??= processing + this.frameTimeout;
      else this.frameExpires = null;
      return faults.consumed ?? bytes.length;
    },
    next_write(processing) {
      this.alive(); this.nextCalls++; trace.push(["next-write"]);
      this.pollTimes.push(processing);
      if (faults.nextError) throw faults.nextError;
      return wrapper(this.controls.shift() ?? { kind: 0 });
    },
    written(id, completed, processing) {
      this.alive(); this.credits.push(id); trace.push(["written", id]);
      this.writeTimes.push({ id, completed, processing });
      if (faults.writtenError) throw faults.writtenError;
      this.onWritten?.(id);
    },
    participant_id() { this.alive(); return this.participant; },
    revision() { this.alive(); return this.revisionValue; },
    has_snapshot() { this.alive(); return this.hasSnapshot; },
    leave_written() { this.alive(); return this.leaveDone; },
    snapshot() { this.alive(); trace.push(["snapshot"]); return this.dto; },
    take_start() { this.alive(); this.startCalls++; return this.schedules.shift() ?? null; },
    publish_progress(words, finalPrefix) {
      this.alive();
      if (this.publishError) throw this.publishError;
      this.publications.push({ words, finalPrefix });
      this.onPublish?.(words, finalPrefix);
    },
    take_peer_progress() { this.alive(); return this.peerUpdates.shift() ?? null; },
    local_final_written() { this.alive(); return this.finalWritten; },
    local_final_acknowledged() { this.alive(); return this.finalAcknowledged; },
    peer_final_ack_written(participant) { this.alive(); this.peerAckQueries.push(participant); return this.peerAcks.has(participant); },
    progress_complete() { this.alive(); return this.progressComplete; },
    drain_complete() { this.alive(); return this.drainComplete; },
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
      session, channelFactory, now: clock, setupTimeoutMs: 50, ioTimeoutMs: 10,
      onSnapshot: value => { callbacks.push(value); trace.push(["on-snapshot"]); },
      onStart: (value, origin) => { starts.push({ value, origin }); trace.push(["on-start"]); },
      onProgress: value => { progress.push(value); trace.push(["on-progress"]); },
      onReceipts: value => { receipts.push(value); trace.push(["on-receipts"]); },
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
    session, opens, wrappers, callbacks, closures, starts, progress, receipts, timers, trace,
    BrowserRoomOwner, BrowserRoomOwnerError, opening, opened, channel, receive, observe,
    get clockCalls() { return clockCalls; },
    setClock(value, reads = []) { clockValue = value; clockReads = [...reads]; },
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
    { now: undefined }, { now: 1 }, { onStart: 1 }, { onProgress: 1 }, { onReceipts: 1 },
  ]) {
    const h = await harness();
    assert.ok(await failure(h.opening(options), "validation") instanceof h.BrowserRoomOwnerError);
    assert.equal(h.opens.length, 0);
    assert.equal(h.session.closes, 0); assert.equal(h.session.frees, 0);
  }
  for (const method of ["needed_bytes", "frame_pending", "receive_bytes", "next_write", "written", "revision", "snapshot", "take_start", "publish_progress", "take_peer_progress", "local_final_written", "local_final_acknowledged", "peer_final_ack_written", "progress_complete", "request_drain", "drain_complete", "begin_drain", "drain_wait_step", "drain_requested", "begin_setup", "setup_wait_step", "configure_frame_wait", "frame_wait_step", "free"]) {
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

test("original transport times reach the common client and controls wake one writer without clock polling", async () => {
  const h = await harness();
  const { owner, io } = await h.opened();
  assert.equal(owner.origin, CLOCK_ORIGIN);
  assert.deepEqual(h.session.pollTimes, [0n]);
  h.setClock(CLOCK_ORIGIN + 200n, [CLOCK_ORIGIN + 100n, CLOCK_ORIGIN + 200n]);
  io.writes[0].gate.resolve(); await flush();
  assert.deepEqual(h.session.writeTimes, [{ id: MAX_U64, completed: 100n, processing: 200n }]);
  await h.observe(io, preparedSnapshot());
  const callbacks = h.callbacks.length, polls = h.session.nextCalls;
  await h.elapse(7);
  assert.equal(h.session.nextCalls, polls, "no interval pumps probes while both APIs are idle");
  h.setClock(CLOCK_ORIGIN + 300n, [CLOCK_ORIGIN + 250n, CLOCK_ORIGIN + 300n]);
  await h.receive(io, () => h.session.controls.push(frame(17n, frameBytes(8))));
  assert.deepEqual(h.session.receiveTimes.at(-1), { captured: 250n, processing: 300n });
  assert.equal(h.callbacks.length, callbacks, "control receipts do not invent admission revisions");
  assert.equal(io.writes.length, 2); assert.equal(io.activeWrites, 1); assert.equal(io.activeReads, 1);
  assert.deepEqual(h.starts, [], "queued control is not a committed schedule");
  const actual = { targetNs: 9007199254740993n, songTargetNs: 9007199354740993n, uncertaintyNs: MAX_U64 };
  h.session.onWritten = id => { if (id === 17n) h.session.schedules.push(actual); };
  h.setClock(CLOCK_ORIGIN + 400n, [CLOCK_ORIGIN + 350n, CLOCK_ORIGIN + 400n]);
  io.writes[1].gate.resolve(); await flush();
  assert.deepEqual(h.session.writeTimes.at(-1), { id: 17n, completed: 350n, processing: 400n });
  assert.deepEqual(h.starts.map(row => ({ ...row, value: { ...row.value } })), [{ value: actual, origin: CLOCK_ORIGIN }]);
  assert.ok(Object.isFrozen(h.starts[0].value));
  actual.targetNs = 0n;
  assert.equal(h.starts[0].value.targetNs, 9007199254740993n, "binding DTO mutation cannot retarget the published start");
  await h.receive(io, () => {});
  await h.elapse(100);
  assert.equal(h.starts.length, 1); assert.equal(owner.closed, false);
  assert.equal(owner.origin, CLOCK_ORIGIN);
  await cleaned(h, owner, io);
});

test("invalid clocks and malformed or repeated common schedules fence without publishing substitute starts", async () => {
  for (const clock of [() => -1n, () => MAX_I64 + 1n, () => 0, () => { throw new Error("clock unavailable"); }]) {
    const h = await harness();
    await failure(h.opening({ now: clock }), "clock");
    assert.equal(h.opens.length, 0); assert.equal(h.session.closes, 1); assert.equal(h.session.frees, 1);
    assert.deepEqual(h.starts, []);
  }
  const regressed = await harness();
  const opened = await regressed.opened();
  opened.io.writes[0].gate.resolve(); await flush();
  regressed.setClock(CLOCK_ORIGIN - 1n);
  opened.io.reads[0].gate.resolve(Uint8Array.of(1)); await flush();
  assert.equal(regressed.closures[0].code, "clock");
  assert.deepEqual(regressed.session.receives, []);
  await cleaned(regressed, opened.owner, opened.io);

  const valid = { targetNs: 1000000000n, songTargetNs: 1100000000n, uncertaintyNs: 40n };
  for (const dto of [undefined, {}, [], { ...valid, targetNs: -1n }, { ...valid, targetNs: 1 },
    { ...valid, songTargetNs: 999999999n }, { ...valid, songTargetNs: MAX_I64 + 1n },
    { ...valid, uncertaintyNs: -1n }, { ...valid, uncertaintyNs: MAX_U64 + 1n }]) {
    const h = await harness(); const { owner, io } = await h.opened();
    io.writes[0].gate.resolve(); await flush();
    await h.observe(io, preparedSnapshot());
    // Return the literal malformed value, including undefined, from the WASM edge.
    h.session.take_start = function () { this.alive(); return dto; };
    await h.receive(io, () => {});
    assert.equal(owner.closed, true); assert.equal(h.closures[0].code, "protocol");
    assert.deepEqual(h.starts, []); await cleaned(h, owner, io);
  }
  for (const fault of ["before-prepared", "duplicate", "callback"]) {
    const h = await harness();
    const { owner, io } = await h.opened(fault === "callback" ? { onStart() { throw new Error("start callback failed"); } } : {});
    io.writes[0].gate.resolve(); await flush();
    await h.observe(io, fault === "before-prepared" ? snapshot() : preparedSnapshot());
    await h.receive(io, () => h.session.schedules.push({ ...valid }));
    if (fault === "duplicate") {
      assert.equal(owner.closed, false); assert.equal(h.starts.length, 1);
      await h.receive(io, () => h.session.schedules.push({ ...valid }));
      assert.equal(h.starts.length, 1);
    }
    assert.equal(owner.closed, true);
    assert.equal(h.closures[0].code, fault === "callback" ? "callback" : "protocol");
    await cleaned(h, owner, io);
  }
});

test("Prepared has one fixed handshake deadline and cancellation joins controls without granting a late start", async () => {
  const h = await harness(); const { owner, io } = await h.opened();
  io.writes[0].gate.resolve(); await flush();
  await h.observe(io);
  await h.elapse(70);
  assert.equal(owner.closed, false, "accepted admission has finished its setup timer");
  await h.observe(io, preparedSnapshot());
  await h.elapse(40);
  await h.observe(io, preparedSnapshot());
  await h.receive(io, () => {});
  await h.elapse(10);
  assert.equal(owner.closed, true); assert.equal(h.closures[0].code, "timeout");
  assert.equal(h.closures[0].operation, "prepared");
  assert.deepEqual(h.starts, []); await cleaned(h, owner, io);

  const late = await harness();
  const opened = await late.opened({}, { holdAfterClose: true });
  opened.io.writes[0].gate.resolve(); await flush();
  await late.observe(opened.io, preparedSnapshot());
  await late.receive(opened.io, () => late.session.controls.push(frame(29n)));
  late.session.onWritten = () => late.session.schedules.push({ targetNs: 1000000000n, songTargetNs: 1100000000n, uncertaintyNs: 0n });
  const credits = [...late.session.credits], reads = late.session.receives.length;
  const closing = attempt(() => opened.owner.close()); await flush();
  assert.equal(closing.state, "pending");
  opened.io.writes.at(-1).gate.resolve(); await flush();
  assert.equal(closing.state, "pending");
  opened.io.reads.at(-1).gate.resolve(Uint8Array.of(1));
  await success(closing);
  assert.deepEqual(late.session.credits, credits); assert.equal(late.session.receives.length, reads);
  assert.deepEqual(late.starts, []); await cleaned(late, opened.owner, opened.io);
});

function progressWords(players = snapshot().members[0].players) {
  const words = new Uint32Array(players.length * 11);
  for (let index = 0; index < players.length; index++) {
    // Signed song -2; hits/max-combo u64MAX; combo u64MAX-1; zero misses.
    words.set([players[index], 0xfffffffe, 0xffffffff, 0xffffffff, 0xffffffff,
      0, 0, 0xfffffffe, 0xffffffff, 0xffffffff, 0xffffffff], index * 11);
  }
  return words;
}
function peerUpdate(overrides = {}) {
  return { participant: MAX_U64 - 1n, sequence: MAX_U64, finalPrefix: true,
    words: progressWords(), ...overrides };
}
async function progressOwner(h, options = {}, channelOptions = {}) {
  const opened = await h.opened(options, channelOptions);
  opened.io.writes[0].gate.resolve(); await flush();
  await h.observe(opened.io, preparedSnapshot());
  await h.receive(opened.io, () => h.session.schedules.push({
    targetNs: 1000000000n, songTargetNs: 1100000000n, uncertaintyNs: 40n,
  }));
  assert.equal(h.starts.length, options.onStart ? 0 : 1);
  return opened;
}

test("progress publication snapshots exact full-width member words and wakes existing writes without invented credit", async () => {
  const h = await harness(); const { owner, io } = await h.opened();
  const words = progressWords();
  assert.throws(() => owner.publishProgress(words), error => error.code === "state");
  io.writes[0].gate.resolve(); await flush();
  await h.observe(io, preparedSnapshot());
  h.session.publishError = Object.assign(new Error("actual common start has not committed"), { code: "state" });
  assert.throws(() => owner.publishProgress(words), error => error.code === "state");
  assert.equal(owner.closed, false); assert.equal(h.session.publications.length, 0);
  await h.receive(io, () => h.session.schedules.push({ targetNs: 1000000000n,
    songTargetNs: 1100000000n, uncertaintyNs: 0n }));
  h.session.publishError = null;
  const invalid = [null, [], new Uint8Array(44), new Uint32Array(0), new Uint32Array(10),
    new Uint32Array(705), new Uint32Array(new SharedArrayBuffer(704 * 4)), progressWords(Uint32Array.of(1))];
  const zero = words.slice(); zero[0] = 0; invalid.push(zero);
  const reordered = words.slice(); [reordered[0], reordered[11]] = [reordered[11], reordered[0]]; invalid.push(reordered);
  const detached = words.slice(); structuredClone(detached.buffer, { transfer: [detached.buffer] }); invalid.push(detached);
  for (const value of invalid) {
    assert.throws(() => owner.publishProgress(value), error => error.code === "validation");
    assert.equal(h.session.publications.length, 0);
    assert.equal(owner.closed, false);
  }
  assert.throws(() => owner.publishProgress(words, 1), error => error.code === "validation");
  h.session.onPublish = () => {
    if (h.session.publications.length === 1) h.session.controls.push(frame(20n, frameBytes(13)));
    // These are scripted common-client outputs, not a JavaScript coalescer.
    if (h.session.publications.length === 3) h.session.controls.push(frame(21n, frameBytes(14)));
  };
  owner.publishProgress(words); words.fill(0); await flush();
  assert.deepEqual([...h.session.publications[0].words], [...progressWords()]);
  assert.equal(h.session.publications[0].finalPrefix, false);
  assert.equal(io.writes.length, 2); assert.equal(io.activeWrites, 1);
  owner.publishProgress(progressWords(), false);
  owner.publishProgress(progressWords(), true); await flush();
  assert.equal(h.session.publications.length, 3); assert.equal(io.writes.length, 2);
  assert.deepEqual(h.session.credits, [MAX_U64]);
  assert.equal(owner.receipts.localFinalWritten, false);
  io.writes[1].gate.resolve(); await flush();
  assert.deepEqual(h.session.credits, [MAX_U64, 20n]); assert.equal(io.writes.length, 3);
  assert.equal(owner.receipts.localFinalWritten, false);
  h.session.onWritten = id => { if (id === 21n) h.session.finalWritten = true; };
  io.writes[2].gate.resolve(); await flush();
  assert.equal(owner.receipts.localFinalWritten, true);
  assert.equal(owner.receipts.localFinalAcknowledged, false);
  const polls = h.session.nextCalls;
  await h.elapse(100);
  assert.equal(h.session.nextCalls, polls, "publication adds no timer-based network polling");
  await cleaned(h, owner, io);
});

test("accepted pending-start peer DTOs retain exact participant and member words independently of callback mutation", async () => {
  const h = await harness(); const { owner, io } = await h.opened();
  io.writes[0].gate.resolve(); await flush();
  await h.observe(io, preparedSnapshot());
  assert.deepEqual(h.starts, []);
  // Mutation by a setup callback cannot rewrite the frozen roster used at the
  // progress boundary. The binding remains authority for accepting the prefix.
  h.callbacks[0].members[1].players[0] = 7;
  const update = peerUpdate(); const originalWords = update.words.slice();
  await h.receive(io, () => h.session.peerUpdates.push(update));
  assert.equal(owner.closed, false); assert.equal(h.progress.length, 1);
  const delivered = h.progress[0];
  assert.equal(delivered.participant, MAX_U64 - 1n);
  assert.equal(delivered.sequence, MAX_U64); assert.equal(delivered.finalPrefix, true);
  assert.deepEqual([...delivered.words], [...originalWords]);
  assert.ok(Object.isFrozen(delivered));
  update.words.fill(0);
  assert.deepEqual([...delivered.words], [...originalWords]);
  assert.deepEqual(h.session.peerAckQueries, [], "ordinary receipt observation does not scan every peer through WASM");
  assert.equal(owner.peerFinalAckWritten(MAX_U64 - 1n), false);
  const calls = h.session.nextCalls;
  await flush(); assert.equal(h.session.nextCalls, calls);
  await h.receive(io, () => {});
  assert.equal(h.progress.length, 1, "one accepted pending update is drained once");
  assert.deepEqual(h.starts, [], "a peer prefix cannot fabricate the local start receipt");
  h.session.peerAcks.add(MAX_U64 - 1n);
  assert.equal(owner.peerFinalAckWritten(MAX_U64 - 1n), true);
  for (const id of [0n, MAX_U64, 1n, MAX_U64 + 1n, 1]) {
    assert.throws(() => owner.peerFinalAckWritten(id));
    assert.equal(owner.closed, false);
  }
  assert.deepEqual(h.session.peerAckQueries, [MAX_U64 - 1n, MAX_U64 - 1n]);
  await cleaned(h, owner, io);
});

test("local completion waits for actual changed receipt booleans and never sends Leave or closes the room", async () => {
  const h = await harness(); const { owner, io } = await progressOwner(h);
  assert.deepEqual({ ...owner.receipts }, { localFinalWritten: false, localFinalAcknowledged: false, complete: false, drainComplete: false });
  assert.equal(h.receipts.length, 0, "unchanged initial booleans do not generate callbacks");
  const promise = owner.waitForLocalCompletion();
  assert.equal(owner.waitForLocalCompletion(), promise);
  const waiting = attempt(() => promise);
  h.session.onPublish = () => h.session.controls.push(frame(31n, frameBytes(13)));
  owner.publishProgress(progressWords(), true); await flush();
  await h.receive(io, () => {}); // Early aggregate acceptance has no written credit yet.
  assert.equal(waiting.state, "pending"); assert.equal(h.receipts.length, 0);
  h.session.onWritten = id => {
    if (id === 31n) { h.session.finalWritten = true; h.session.finalAcknowledged = true; }
    if (id === 32n) { h.session.peerAcks.add(MAX_U64 - 1n); h.session.progressComplete = true; }
  };
  io.writes.at(-1).gate.resolve(); await flush();
  assert.deepEqual({ ...owner.receipts }, { localFinalWritten: true, localFinalAcknowledged: true, complete: false, drainComplete: false });
  assert.equal(waiting.state, "pending"); assert.equal(h.receipts.length, 1);
  await h.receive(io, () => {});
  assert.equal(h.receipts.length, 1);
  await h.receive(io, () => h.session.controls.push(frame(32n, frameBytes(15))));
  assert.equal(waiting.state, "pending");
  assert.equal(owner.peerFinalAckWritten(MAX_U64 - 1n), false);
  io.writes.at(-1).gate.resolve();
  const completed = await success(waiting); await flush();
  assert.equal(completed, owner.receipts); assert.ok(Object.isFrozen(completed));
  assert.deepEqual({ ...completed }, { localFinalWritten: true, localFinalAcknowledged: true, complete: true, drainComplete: false });
  assert.equal(owner.peerFinalAckWritten(MAX_U64 - 1n), true);
  assert.equal(h.receipts.length, 2); assert.equal(owner.closed, false);
  assert.deepEqual(h.session.requests, []); assert.equal(h.session.frees, 0); assert.equal(io.closes, 0);
  await h.elapse(100);
  assert.equal(owner.closed, false); assert.equal(h.receipts.length, 2);
  await cleaned(h, owner, io);
});

test("malformed progress and receipt boundaries or callbacks fence before publishing partial metadata", async () => {
  const invalid = [
    undefined, {}, peerUpdate({ participant: 0n }), peerUpdate({ participant: MAX_U64 }),
    peerUpdate({ participant: 1n }), peerUpdate({ sequence: 0n }), peerUpdate({ sequence: 1 }),
    peerUpdate({ sequence: MAX_U64 + 1n }), peerUpdate({ finalPrefix: 1 }),
    peerUpdate({ words: new Uint32Array(10) }), peerUpdate({ words: new Uint32Array(715) }),
    peerUpdate({ words: new Uint32Array(new SharedArrayBuffer(704 * 4)) }),
  ];
  const wrongRoster = progressWords(); wrongRoster[11] = wrongRoster[0];
  invalid.push(peerUpdate({ words: wrongRoster }));
  for (const dto of invalid) {
    const h = await harness(); const { owner, io } = await progressOwner(h);
    h.session.take_peer_progress = function () { this.alive(); return dto; };
    await h.receive(io, () => {});
    assert.equal(owner.closed, true); assert.equal(h.closures[0].code, "protocol");
    assert.deepEqual(h.progress, []); await cleaned(h, owner, io);
  }
  for (const method of ["local_final_written", "local_final_acknowledged", "progress_complete", "drain_complete"]) {
    const h = await harness(); const { owner, io } = await progressOwner(h);
    h.session[method] = function () { this.alive(); return 1; };
    await h.receive(io, () => {});
    assert.equal(owner.closed, true); assert.equal(h.closures[0].code, "protocol");
    assert.deepEqual(h.receipts, []); await cleaned(h, owner, io);
  }
  for (const status of [
    { finalAcknowledged: true },
    { finalWritten: true, progressComplete: true },
    { drainComplete: true },
  ]) {
    const h = await harness(); const { owner, io } = await progressOwner(h);
    await h.receive(io, () => Object.assign(h.session, status));
    assert.equal(owner.closed, true); assert.equal(h.closures[0].code, "protocol");
    assert.deepEqual(h.receipts, []); await cleaned(h, owner, io);
  }
  for (const callback of ["progress", "receipts"]) {
    const h = await harness();
    const options = callback === "progress"
      ? { onProgress() { throw new Error("progress callback failed"); } }
      : { onReceipts() { return Promise.reject(new Error("receipt callback failed")); } };
    const { owner, io } = await progressOwner(h, options);
    await h.receive(io, () => {
      if (callback === "progress") h.session.peerUpdates.push(peerUpdate());
      else h.session.finalWritten = true;
    });
    assert.equal(owner.closed, true); assert.equal(h.closures[0].code, "callback");
    await cleaned(h, owner, io);
  }
  const faults = {}, h = await harness(faults), opened = await progressOwner(h);
  faults.receiveError = new Error("Rust rejected duplicate or regressed peer prefix");
  h.session.peerUpdates.push(peerUpdate());
  await h.receive(opened.io, () => {});
  assert.equal(h.closures[0].code, "core"); assert.deepEqual(h.progress, []);
  await cleaned(h, opened.owner, opened.io);
  const publication = await harness(), active = await progressOwner(publication);
  publication.session.publishError = new Error("fatal common-client failure");
  assert.throws(() => active.owner.publishProgress(progressWords()), error => error.code === "core");
  assert.equal(active.owner.closed, true); assert.equal(publication.session.publications.length, 0);
  await cleaned(publication, active.owner, active.io);
});

test("completion cancellation rejects once while close joins late channel operations without post-free progress", async () => {
  for (const abort of [false, true]) {
    const h = await harness(), signal = new AbortController();
    const { owner, io } = await progressOwner(h, { signal: signal.signal }, { holdAfterClose: true });
    h.session.onPublish = () => h.session.controls.push(frame(41n, frameBytes(13)));
    owner.publishProgress(progressWords(), true); await flush();
    const waiting = attempt(() => owner.waitForLocalCompletion());
    const written = [...h.session.credits], receives = h.session.receives.length;
    h.session.onWritten = () => { h.session.finalWritten = true; h.session.finalAcknowledged = true; h.session.progressComplete = true; };
    if (abort) signal.abort();
    const closing = attempt(() => owner.close()); await flush();
    await failure(waiting, abort ? "aborted" : "closed");
    assert.equal(closing.state, "pending"); assert.equal(h.session.frees, 1);
    assert.throws(() => owner.publishProgress(progressWords()), error => error.code === (abort ? "aborted" : "closed"));
    io.writes.at(-1).gate.resolve(); await flush();
    assert.equal(closing.state, "pending");
    h.session.peerUpdates.push(peerUpdate());
    io.reads.at(-1).gate.resolve(Uint8Array.of(1));
    await success(closing);
    assert.deepEqual(h.session.credits, written); assert.equal(h.session.receives.length, receives);
    assert.deepEqual(h.progress, []); assert.deepEqual(h.receipts, []);
    assert.deepEqual(h.session.requests, []);
    await cleaned(h, owner, io);
  }
});

test("explicit drain shares one promise and waits for actual local completion and Ready write before accepting Complete", async () => {
  const h = await harness(); const { owner, io } = await progressOwner(h, {}, { holdAfterClose: true });
  let pendingNotice = false;
  h.session.onPublish = () => h.session.controls.push(frame(51n, frameBytes(13)));
  h.session.onRequest = kind => {
    assert.equal(kind, "drain");
    assert.equal(h.session.progressComplete, true);
    h.session.controls.push(frame(52n, frameBytes(16)));
  };
  h.session.onWritten = id => {
    if (id === 51n) h.session.finalWritten = true;
    // Script the binding's receipt output only; Rust fixtures cover the actual
    // matching early-notice admission and sequence/participant validation.
    if (id === 52n && pendingNotice) h.session.drainComplete = true;
  };
  owner.publishProgress(progressWords(), true); await flush();
  const promise = owner.drain(), waiting = attempt(() => promise);
  assert.equal(owner.drain(), promise);
  await flush();
  assert.deepEqual(h.session.requests, []);
  assert.equal(owner.receipts.complete, false); assert.equal(waiting.state, "pending");
  io.writes.at(-1).gate.resolve(); await flush();
  assert.equal(owner.receipts.localFinalWritten, true);
  assert.deepEqual(h.session.requests, []);
  await h.receive(io, () => { h.session.finalAcknowledged = true; h.session.progressComplete = true; });
  assert.deepEqual(h.session.requests, ["drain"]);
  assert.equal(owner.drain(), promise); assert.equal(io.activeWrites, 1);
  await h.receive(io, () => { pendingNotice = true; });
  assert.equal(waiting.state, "pending"); assert.equal(owner.receipts.drainComplete, false);
  assert.deepEqual(h.session.credits, [MAX_U64, 51n]);
  io.writes.at(-1).gate.resolve();
  const receipt = await success(waiting); await flush();
  assert.equal(receipt, owner.receipts);
  assert.deepEqual({ ...receipt }, { localFinalWritten: true, localFinalAcknowledged: true, complete: true, drainComplete: true });
  assert.ok(Object.isFrozen(receipt)); assert.equal(owner.drain(), promise);
  assert.deepEqual(h.session.requests, ["drain"]); assert.equal(owner.closed, false);
  assert.equal(io.closes, 0); assert.equal(h.session.frees, 0);
  const before = { reads: io.reads.length, writes: io.writes.length, clocks: h.clockCalls, polls: h.session.nextCalls };
  await h.elapse(100);
  assert.deepEqual({ reads: io.reads.length, writes: io.writes.length, clocks: h.clockCalls, polls: h.session.nextCalls }, before);
  const receives = h.session.receives.length, credits = [...h.session.credits];
  const closing = attempt(() => owner.close()); await flush();
  assert.equal(closing.state, "pending", "successful drain still joins the already owned read");
  assert.equal(h.session.frees, 1);
  io.reads.at(-1).gate.resolve(Uint8Array.of(17));
  await success(closing);
  assert.equal(h.session.receives.length, receives); assert.deepEqual(h.session.credits, credits);
  assert.equal(owner.receipts.drainComplete, true, "the published receipt retains historical evidence");
  await cleaned(h, owner, io);
});

test("one drain deadline spans local receipts Ready and Complete without renewal or automatic retry", async () => {
  for (const stage of ["local", "ready", "complete"]) {
    const h = await harness(); const { owner, io } = await progressOwner(h);
    h.session.onRequest = kind => {
      assert.equal(kind, "drain"); h.session.controls.push(frame(61n, frameBytes(16)));
    };
    const waiting = attempt(() => owner.drain());
    await h.elapse(4);
    if (stage !== "local") {
      await h.receive(io, () => { h.session.finalWritten = true; h.session.finalAcknowledged = true; h.session.progressComplete = true; });
      assert.deepEqual(h.session.requests, ["drain"]);
      if (stage === "complete") { io.writes.at(-1).gate.resolve(); await flush(); }
    } else {
      await h.receive(io, () => {});
      assert.deepEqual(h.session.requests, []);
    }
    await h.elapse(5);
    assert.equal(waiting.state, "pending"); assert.equal(owner.closed, false);
    await h.elapse(1);
    const error = await failure(waiting, "timeout");
    assert.equal(error.operation, "drain"); assert.equal(owner.closed, true);
    assert.equal(owner.receipts.drainComplete, false);
    assert.deepEqual(h.session.requests, stage === "local" ? [] : ["drain"]);
    await cleaned(h, owner, io);
  }
});

test("drain cancellation joins pending operations and local Ready refusal cannot fabricate completion", async () => {
  const refused = await harness(), opened = await progressOwner(refused);
  await refused.receive(opened.io, () => {
    refused.session.finalWritten = true; refused.session.finalAcknowledged = true; refused.session.progressComplete = true;
  });
  refused.session.requestError = Object.assign(new Error("common drain phase refused"), { code: "state" });
  await failure(attempt(() => opened.owner.drain()), "state");
  assert.equal(opened.owner.closed, false); assert.equal(opened.owner.receipts.drainComplete, false);
  assert.deepEqual(refused.session.requests, ["drain"]);
  const refusedSteps = refused.session.drainSteps.length;
  const refusedPromise = opened.owner.drain();
  await refused.receive(opened.io, () => {});
  await refused.elapse(1);
  assert.equal(opened.owner.drain(), refusedPromise);
  assert.equal(refused.session.drainSteps.length, refusedSteps);
  assert.equal(refused.session.drainBegins.length, 1);
  assert.deepEqual(refused.session.requests, ["drain"]);
  await cleaned(refused, opened.owner, opened.io);

  for (const mode of ["close", "abort", "leave"]) {
    const h = await harness(), controller = new AbortController();
    const { owner, io } = await progressOwner(h, { signal: controller.signal }, { holdAfterClose: true });
    await h.receive(io, () => { h.session.finalWritten = true; h.session.finalAcknowledged = true; h.session.progressComplete = true; });
    h.session.onRequest = kind => h.session.controls.push(frame(kind === "drain" ? 71n : 72n, frameBytes(kind === "drain" ? 16 : 6)));
    h.session.onWritten = id => { if (id === 72n) h.session.leaveDone = true; };
    const waiting = attempt(() => owner.drain()); await flush();
    const credits = [...h.session.credits], receives = h.session.receives.length;
    let leaving;
    if (mode === "leave") leaving = attempt(() => owner.leave());
    else if (mode === "abort") controller.abort();
    else void owner.close();
    await flush();
    await failure(waiting, mode === "abort" ? "aborted" : "closed");
    if (mode === "leave") {
      io.writes.at(-1).gate.resolve(); await flush();
      assert.equal(leaving.state, "pending"); assert.equal(h.session.leaveDone, false);
      io.writes.at(-1).gate.resolve(); await success(leaving); await flush();
      assert.deepEqual(h.session.credits, [...credits, 71n, 72n]);
    } else {
      io.writes.at(-1).gate.resolve(); await flush();
      assert.deepEqual(h.session.credits, credits);
    }
    const closing = attempt(() => owner.close()); await flush();
    assert.equal(closing.state, "pending"); assert.equal(h.session.frees, 1);
    io.reads.at(-1).gate.resolve(Uint8Array.of(17));
    await success(closing);
    assert.equal(h.session.receives.length, receives); assert.equal(owner.receipts.drainComplete, false);
    await cleaned(h, owner, io);
  }
});

test("Rust drain steps own completion despite complete-looking old receipt fields", async () => {
  const h = await harness(); const { owner, io } = await progressOwner(h);
  h.session.drainAdmitted = true;
  h.session.drainOutputs.push(1000000n, 1000000n, 1000000n, -1n);
  const promise = owner.drain(), waiting = attempt(() => promise);
  assert.equal(owner.drain(), promise);
  assert.equal(h.session.drainBegins.length, 1);
  assert.equal(h.session.drainBegins[0].timeout, 10000000n);
  await h.receive(io, () => {
    h.session.finalWritten = true; h.session.finalAcknowledged = true;
    h.session.progressComplete = true; h.session.drainComplete = true;
  });
  assert.equal(waiting.state, "pending", "old complete receipts cannot override pending Rust step");
  assert.equal(h.session.requests.length, 0, "adapter must not issue request_drain itself");
  await h.elapse(1);
  assert.equal(waiting.state, "pending");
  await h.elapse(1);
  const receipts = await success(waiting);
  assert.equal(receipts, owner.receipts);
  assert.equal(h.session.drainBegins.length, 1);
  assert.equal(h.session.requests.length, 0);
  assert.equal(owner.drain(), promise);
  await cleaned(h, owner, io);
});

test("invalid Rust drain return shapes and completion without receipts refuse atomically", async () => {
  for (const output of [-2n, 1000001n, 0, null, "-1", -1n]) {
    const h = await harness(); const { owner, io } = await progressOwner(h);
    h.session.drainOutputs.push(output);
    const waiting = attempt(() => owner.drain());
    await failure(waiting, "protocol");
    assert.equal(h.session.drainBegins.length, 1);
    assert.equal(h.session.drainSteps.length, 1);
    assert.deepEqual(h.session.requests, []);
    assert.equal(owner.receipts.drainComplete, false);
    await cleaned(h, owner, io);
  }
});

test("scripted common timeout and cancelled owner stop finite drain timers and never begin twice", async () => {
  for (const cancel of [false, true]) {
    const h = await harness(); const { owner, io } = await progressOwner(h);
    const original = Object.assign(new Error("original common timeout"), { code: "timeout" });
    h.session.drainOutputs.push(0n, original);
    const promise = owner.drain(), waiting = attempt(() => promise);
    assert.equal(owner.drain(), promise);
    assert.equal(h.session.drainBegins.length, 1);
    const before = h.session.drainSteps.length;
    if (cancel) {
      await owner.close();
      await failure(waiting, "closed");
      await h.elapse(5);
      assert.equal(h.session.drainSteps.length, before);
    } else {
      await h.elapse(1);
      const error = await failure(waiting, "timeout");
      assert.equal(error.operation, "drain");
      assert.equal(error.cause, original);
      assert.equal(h.session.drainSteps.length, before + 1);
    }
    assert.equal(h.session.drainBegins.length, 1);
    assert.deepEqual(h.session.requests, []);
    await cleaned(h, owner, io);
  }
});

test("recoverable common refusal seals the drain promise across later valid receipt updates", async () => {
  const h = await harness(); const { owner, io } = await progressOwner(h);
  const original = Object.assign(new Error("original local drain refusal"), { code: "state" });
  h.session.drainOutputs.push(original);
  const promise = owner.drain();
  const error = await failure(attempt(() => promise), "state");
  assert.equal(error.cause, original);
  assert.equal(owner.closed, false);
  assert.equal(h.session.drainSteps.length, 1);
  await h.receive(io, () => {
    h.session.finalWritten = true; h.session.finalAcknowledged = true;
    h.session.progressComplete = true;
  });
  await h.elapse(5);
  assert.equal(owner.drain(), promise);
  assert.equal(h.session.drainBegins.length, 1);
  assert.equal(h.session.drainSteps.length, 1);
  assert.deepEqual(h.session.requests, []);
  assert.equal(owner.receipts.drainComplete, false);
  await cleaned(h, owner, io);
});

test("setup timer consults scripted Rust policy instead of declaring timeout and idle clears it", async () => {
  const h = await harness();
  h.session.setupOutputs.push(50000000n, 50000000n, -2n);
  const { owner, io } = await h.opened();
  assert.equal(h.session.setupBegins.length, 1);
  assert.equal(h.session.setupBegins[0].elapsed, 0n);
  assert.equal(h.session.setupBegins[0].timeout, 50000000n);
  io.writes[0].gate.resolve(); await flush();
  const steps = h.session.setupSteps.length;
  await h.elapse(50);
  assert.equal(owner.closed, false, "timer must ask Rust even at the former JS timeout");
  assert.equal(h.session.setupSteps.length, steps + 1);
  assert.equal(h.timers.size, 0);
  h.session.setupOutputs.push(-2n);
  h.setClock(CLOCK_ORIGIN + 9007199254740993n);
  await h.receive(io, () => {});
  assert.equal(h.session.setupSteps.at(-1), 9007199254740993n);
  assert.equal(h.session.setupBegins.length, 1);
  await cleaned(h, owner, io);
});

test("post-receive and post-write setup refusal preserves IO evidence but suppresses callbacks", async () => {
  for (const boundary of ["receive", "write"]) {
    const h = await harness(); const { owner, io } = await h.opened();
    const original = Object.assign(new Error("original late accepted setup evidence"), {
      code: "timeout", operation: boundary === "receive" ? "prepared" : "setup",
    });
    h.session.setupOutputs.push(original);
    if (boundary === "receive") {
      await h.receive(io, () => {
        h.session.participant = MAX_U64; h.session.revisionValue = 1n;
        h.session.hasSnapshot = true; h.session.dto = preparedSnapshot();
        h.session.schedules.push({ targetNs: 1000000000n, songTargetNs: 1100000000n, uncertaintyNs: 17n });
      });
      assert.equal(h.session.receives.length, 1);
    } else {
      io.writes[0].gate.resolve(); await flush();
      assert.deepEqual(h.session.credits, [MAX_U64]);
    }
    assert.equal(owner.closed, true);
    assert.equal(h.closures[0].cause, original);
    assert.equal(h.closures[0].code, "timeout");
    assert.equal(h.closures[0].operation, original.operation);
    assert.deepEqual(h.callbacks, []);
    assert.deepEqual(h.starts, []);
    assert.equal(h.session.setupBegins.length, 1);
    await cleaned(h, owner, io);
  }
});

test("malformed setup scheduling outputs refuse before channel acquisition", async () => {
  for (const output of [-3n, 0n, 120000000001n, 1, null, "-2"]) {
    const h = await harness(); h.session.setupOutputs.push(output);
    const error = await failure(h.opening(), "protocol");
    assert.equal(error.operation, "setup");
    assert.equal(h.opens.length, 0);
    assert.equal(h.session.setupBegins.length, 1);
    assert.equal(h.session.setupSteps.length, 1);
    assert.equal(h.timers.size, 0);
    assert.deepEqual(h.callbacks, []);
    assert.deepEqual(h.starts, []);
  }
});

test("completed Rust setup adds no later setup calls during actual transport traffic", async () => {
  const h = await harness(); const { owner, io } = await progressOwner(h);
  const steps = h.session.setupSteps.length;
  assert.equal(h.session.setupPhase, "complete");
  await h.receive(io, () => {});
  h.session.onPublish = () => h.session.controls.push(frame(811n));
  owner.publishProgress(progressWords(), false); await flush();
  io.writes.at(-1).gate.resolve(); await flush();
  assert.equal(h.session.setupSteps.length, steps);
  assert.equal(h.session.setupBegins.length, 1);
  assert.equal(h.starts.length, 1);
  assert.ok(h.session.credits.includes(811n));
  await cleaned(h, owner, io);
});

test("close abort and Leave clear pending setup scheduling without a later policy call", async () => {
  for (const mode of ["close", "abort", "leave"]) {
    const h = await harness(); const controller = new AbortController();
    const { owner, io } = await h.opened({ signal: controller.signal });
    io.writes[0].gate.resolve(); await flush();
    const steps = h.session.setupSteps.length;
    if (mode === "leave") {
      h.session.onRequest = kind => {
        assert.equal(kind, "leave"); h.session.controls.push(frame(812n));
      };
      h.session.onWritten = id => { if (id === 812n) h.session.leaveDone = true; };
      const leaving = attempt(() => owner.leave()); await flush();
      io.writes.at(-1).gate.resolve(); await success(leaving);
    } else if (mode === "abort") controller.abort();
    else await owner.close();
    await flush(); await h.elapse(100);
    assert.equal(h.session.setupSteps.length, steps);
    assert.equal(h.session.setupBegins.length, 1);
    assert.equal(h.timers.size, 0);
    assert.deepEqual(h.starts, []);
    await cleaned(h, owner, io);
  }
});

test("frame timers consult Rust outputs and fragments do not refresh the original wake", async () => {
  const h = await harness(); const { owner, io } = await h.opened();
  io.writes[0].gate.resolve(); await flush(); await h.observe(io);
  assert.deepEqual(h.session.frameConfigurations, [10000000n]);
  h.session.frameOutputs.push(10000000n, 5000000n, -1n);
  await h.receive(io, () => { h.session.partial = true; h.session.need = 5; });
  const steps = h.session.frameSteps.length;
  const deadline = [...h.timers.values()][0].at;
  await h.elapse(8);
  await h.receive(io, () => { h.session.partial = true; h.session.need = 4; });
  assert.equal(h.session.frameSteps.length, steps);
  assert.equal([...h.timers.values()][0].at, deadline);
  await h.elapse(2);
  assert.equal(owner.closed, false, "timer must not invent expiration over Rust Wait");
  assert.equal(h.session.frameSteps.length, steps + 1);
  await h.elapse(5);
  assert.equal(owner.closed, false, "Rust Idle clears the scheduled wake");
  assert.equal(h.session.frameSteps.length, steps + 2);
  assert.equal(h.timers.size, 0);
  await cleaned(h, owner, io);
});

test("Rust receive refuses expired final fragment before timer runs or callbacks can observe metadata", async () => {
  const h = await harness(); const { owner, io } = await h.opened();
  io.writes[0].gate.resolve(); await flush(); await h.observe(io);
  await h.receive(io, () => { h.session.partial = true; h.session.need = 5; });
  const revision = h.session.revisionValue, callbacks = h.callbacks.length;
  const steps = h.session.frameSteps.length;
  h.setClock(CLOCK_ORIGIN + 10000000n);
  await h.receive(io, () => {
    assert.fail("late fragment must be refused before admission changes");
  });
  assert.equal(owner.closed, true);
  assert.equal(h.closures[0].code, "timeout");
  assert.equal(h.closures[0].operation, "frame");
  assert.equal(h.session.revisionValue, revision);
  assert.equal(h.callbacks.length, callbacks);
  assert.deepEqual(h.starts, []);
  assert.equal(h.session.frameSteps.length, steps, "late entry guard precedes timer callback");
  await cleaned(h, owner, io);
});

test("frame configuration precedes IO and invalid scheduling output cannot create a timer", async () => {
  const original = new Error("original frame configuration refusal");
  const refused = await harness({ configureFrameError: original });
  const error = await failure(refused.opening(), "transport");
  assert.equal(error.cause, original);
  assert.equal(refused.opens.length, 0);
  assert.deepEqual(refused.session.frameConfigurations, [10000000n]);
  for (const output of [0n, -2n, 120000000001n, 1, null]) {
    const h = await harness(); const { owner, io } = await h.opened();
    io.writes[0].gate.resolve(); await flush(); await h.observe(io);
    h.session.frameOutputs.push(output);
    await h.receive(io, () => { h.session.partial = true; h.session.need = 5; });
    assert.equal(owner.closed, true);
    assert.equal(h.closures[0].code, "protocol");
    assert.equal(h.closures[0].operation, "frame");
    assert.equal(h.timers.size, 0);
    assert.equal(h.session.frameSteps.length, 1);
    await cleaned(h, owner, io);
  }
});

test("close abort and Leave clear incomplete frame wakes without another Rust observation", async () => {
  for (const mode of ["close", "abort", "leave"]) {
    const h = await harness(); const controller = new AbortController();
    const { owner, io } = await h.opened({ signal: controller.signal });
    io.writes[0].gate.resolve(); await flush(); await h.observe(io);
    await h.receive(io, () => { h.session.partial = true; h.session.need = 5; });
    const steps = h.session.frameSteps.length;
    if (mode === "leave") {
      h.session.onRequest = kind => { assert.equal(kind, "leave"); h.session.controls.push(frame(813n)); };
      h.session.onWritten = id => { if (id === 813n) h.session.leaveDone = true; };
      const leaving = attempt(() => owner.leave()); await flush();
      io.writes.at(-1).gate.resolve(); await success(leaving);
    } else if (mode === "abort") controller.abort();
    else await owner.close();
    await flush(); await h.elapse(100);
    assert.equal(h.session.frameSteps.length, steps);
    assert.deepEqual(h.session.frameConfigurations, [10000000n]);
    assert.equal(h.timers.size, 0);
    await cleaned(h, owner, io);
  }
});
