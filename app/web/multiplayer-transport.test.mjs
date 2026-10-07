// Deferred: node --experimental-vm-modules --test app/web/multiplayer-transport.test.mjs
// Real adapter source with controlled platform streams; no sockets or protocol model.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

const moduleUrl = new URL("./multiplayer-transport.mjs", import.meta.url);
const source = await readFile(moduleUrl, "utf8");
const endpoint = "https://example.test:4433/competition?room=exact";
const MAX_FRAME = 65547;
const MAX_CHUNK = 1024 * 1024;

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function attempt(action) {
  const result = { state: "pending" };
  let value;
  try { value = action(); } catch (error) { value = Promise.reject(error); }
  result.done = Promise.resolve(value).then(
    value => { result.state = "fulfilled"; result.value = value; },
    error => { result.state = "rejected"; result.error = error; },
  );
  return result;
}
async function flush() { for (let index = 0; index < 32; index++) await Promise.resolve(); }
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

async function harness(faults = {}) {
  const transports = [];
  const timers = new Map();
  let now = 0;
  let timerId = 0;

  function cleanupResult(kind) {
    if (faults[`${kind}Throws`]) throw new Error(`${kind} failed synchronously`);
    if (faults.cleanupRejects) return Promise.reject(new Error(`${kind} rejected`));
    if (faults.cleanupPending) return new Promise(() => {});
    return Promise.resolve();
  }
  function stream() {
    const io = { reads: [], writes: [], readerLocks: 0, writerLocks: 0,
      cancels: 0, aborts: 0, readerReleases: 0, writerReleases: 0,
      unlockedCancels: 0, unlockedAborts: 0 };
    io.reader = {
      read() {
        const gate = deferred();
        io.reads.push(gate);
        if (faults.readThrows) throw new Error("read failed synchronously");
        return gate.promise;
      },
      cancel() { io.cancels++; return cleanupResult("cancel"); },
      releaseLock() { io.readerReleases++; if (faults.releaseThrows) throw new Error("reader release failed"); },
    };
    io.writer = {
      write(bytes) {
        const gate = deferred();
        io.writes.push({ bytes, gate });
        if (faults.writeThrows) throw new Error("write failed synchronously");
        return gate.promise;
      },
      abort() { io.aborts++; return cleanupResult("abort"); },
      releaseLock() { io.writerReleases++; if (faults.releaseThrows) throw new Error("writer release failed"); },
    };
    io.stream = {
      readable: {
        getReader() { io.readerLocks++; return io.reader; },
        cancel() { io.unlockedCancels++; return cleanupResult("cancel"); },
      },
      writable: {
        getWriter() { io.writerLocks++; return io.writer; },
        abort() { io.unlockedAborts++; return cleanupResult("abort"); },
      },
    };
    return io;
  }
  class WebTransport {
    constructor(url, ...extra) {
      if (faults.constructorThrows) throw new Error("construction failed");
      this.url = url;
      this.extra = extra;
      this.readyGate = deferred();
      this.closedGate = deferred();
      this.streamGate = deferred();
      this.ready = this.readyGate.promise;
      this.closed = this.closedGate.promise;
      this.creates = 0;
      this.closes = 0;
      transports.push(this);
    }
    createBidirectionalStream() {
      this.creates++;
      if (faults.createThrows) throw new Error("stream creation failed");
      return this.streamGate.promise;
    }
    close() { this.closes++; return cleanupResult("close"); }
  }
  const context = createContext({
    WebTransport: faults.unavailable ? undefined : WebTransport,
    AbortController, AbortSignal, URL, DOMException, Uint8Array, Uint16Array, ArrayBuffer,
    SharedArrayBuffer, DataView, structuredClone,
    Date: class extends Date { static now() { return now; } },
    performance: { now: () => now },
    setTimeout(callback, delay, ...args) {
      const id = ++timerId;
      timers.set(id, { at: now + delay, callback: () => callback(...args) });
      return id;
    },
    clearTimeout(id) { timers.delete(id); },
  });
  const actual = new SourceTextModule(source, { context, identifier: moduleUrl.href });
  await actual.link(specifier => { throw new Error(`Unexpected transport import: ${specifier}`); });
  await actual.evaluate();
  const { WebTransportChannel, WebTransportChannelError } = actual.namespace;
  function opening(options = {}, url = endpoint) {
    const result = attempt(() => WebTransportChannel.open(url, {
      setupTimeoutMs: 50, ioTimeoutMs: 30, ...options,
    }));
    return { result, transport: transports.at(-1) };
  }
  async function opened(options = {}) {
    const { result, transport } = opening(options);
    transport.readyGate.resolve();
    await flush();
    const io = stream();
    transport.streamGate.resolve(io.stream);
    return { owner: await success(result), transport, io };
  }
  return { WebTransportChannel, WebTransportChannelError, WebTransport, transports, timers,
    stream, opening, opened,
    get now() { return now; },
    async elapse(milliseconds) {
      const target = now + milliseconds;
      for (;;) {
        const next = [...timers].filter(([, timer]) => timer.at <= target)
          .sort((a, b) => a[1].at - b[1].at)[0];
        if (!next) break;
        const [id, timer] = next;
        timers.delete(id);
        now = timer.at;
        timer.callback();
        await flush();
      }
      now = target;
      await flush();
    },
  };
}

function assertReleased(h, transport, io) {
  assert.equal(transport.closes, 1);
  assert.equal(io.cancels, 1);
  assert.equal(io.aborts, 1);
  assert.equal(io.readerReleases, 1);
  assert.equal(io.writerReleases, 1);
  assert.equal(h.timers.size, 0);
}

test("explicit room prefix bounds preserve defaults, owned writes, and coalesced suffixes", async () => {
  const h = await harness();
  const roomBound = 65808;
  for (const maxPrefixBytes of [0, -1, roomBound + 1, 1.5, NaN, Infinity, null, "65808"]) {
    await failure(h.opening({ maxPrefixBytes }).result, "validation");
    assert.equal(h.transports.length, 0);
  }

  const ordinary = await h.opened();
  await failure(attempt(() => ordinary.owner.readPrefix(roomBound)), "validation");
  await failure(attempt(() => ordinary.owner.write(new Uint8Array(roomBound))), "validation");
  assert.equal(ordinary.io.reads.length, 0);
  assert.equal(ordinary.io.writes.length, 0);
  ordinary.owner.close();
  assertReleased(h, ordinary.transport, ordinary.io);

  const room = await h.opened({ maxPrefixBytes: roomBound });
  await failure(attempt(() => room.owner.readPrefix(roomBound + 1)), "validation");
  await failure(attempt(() => room.owner.write(new Uint8Array(roomBound + 1))), "validation");
  const chunk = new Uint8Array(roomBound + 4);
  chunk[0] = 0x42;
  chunk[roomBound - 1] = 0x7f;
  chunk.set([1, 2, 3, 4], roomBound);
  const reading = attempt(() => room.owner.readPrefix(roomBound));
  room.io.reads[0].resolve({ done: false, value: chunk });
  const prefix = await success(reading);
  assert.equal(prefix.byteLength, roomBound);
  assert.equal(prefix[0], 0x42);
  assert.equal(prefix[roomBound - 1], 0x7f);
  const suffix = await success(attempt(() => room.owner.readPrefix(4)));
  assert.deepEqual([...suffix], [1, 2, 3, 4]);
  assert.equal(room.io.reads.length, 1);

  const writing = attempt(() => room.owner.write(prefix));
  prefix.fill(0);
  assert.equal(room.io.writes[0].bytes.byteLength, roomBound);
  assert.equal(room.io.writes[0].bytes[0], 0x42);
  assert.equal(room.io.writes[0].bytes[roomBound - 1], 0x7f);
  assert.equal(writing.state, "pending");
  room.io.writes[0].gate.resolve();
  await success(writing);
  room.owner.close();
  assertReleased(h, room.transport, room.io);

  const small = await h.opened({ maxPrefixBytes: 8 });
  await failure(attempt(() => small.owner.readPrefix(9)), "validation");
  await failure(attempt(() => small.owner.write(new Uint8Array(9))), "validation");
  assert.equal(small.io.reads.length, 0);
  assert.equal(small.io.writes.length, 0);
  small.owner.close();
  assertReleased(h, small.transport, small.io);
});

test("idle acquisition retains cancellation ownership while ordinary reads retain deadlines", async () => {
  const h = await harness();
  const first = await h.opened();
  await failure(attempt(() => first.owner.readPrefix(11, "idle")), "validation");
  assert.equal(first.io.reads.length, 0);
  const idle = attempt(() => first.owner.readPrefix(11, true));
  await h.elapse(100);
  assert.equal(idle.state, "pending");
  assert.equal(first.transport.closes, 0);
  first.io.reads[0].resolve({ done: false, value: Uint8Array.of(0x42) });
  assert.deepEqual([...await success(idle)], [0x42]);
  const ordinary = attempt(() => first.owner.readPrefix(10));
  await h.elapse(30);
  await failure(ordinary, "timeout");
  assertReleased(h, first.transport, first.io);

  const second = await h.opened();
  const waiting = attempt(() => second.owner.readPrefix(11, true));
  await h.elapse(100);
  assert.equal(waiting.state, "pending");
  second.owner.close();
  await failure(waiting, "closed");
  second.io.reads[0].resolve({ done: false, value: Uint8Array.of(1) });
  await flush();
  assertReleased(h, second.transport, second.io);
});

test("setup validates before construction and owns exactly one stream after actual readiness", async () => {
  const h = await harness();
  for (const url of ["", "http://example.test/", "/relative", "https://user:secret@example.test/",
    "https://example.test/#fragment", "x".repeat(4097), 123]) {
    const { result } = h.opening({}, url);
    assert.ok(await failure(result, "validation") instanceof h.WebTransportChannelError);
  }
  for (const options of [{ setupTimeoutMs: 0 }, { setupTimeoutMs: 60001 },
    { setupTimeoutMs: 1.5 }, { ioTimeoutMs: NaN }, { ioTimeoutMs: 60001 }, { ioTimeoutMs: 0 }]) {
    await failure(h.opening(options).result, "validation");
  }
  assert.equal(h.transports.length, 0);
  const unavailable = await harness({ unavailable: true });
  await failure(unavailable.opening().result, "unavailable");
  const abort = new AbortController();
  abort.abort();
  await failure(h.opening({ signal: abort.signal }).result, "aborted");
  assert.equal(h.transports.length, 0);

  const { result, transport } = h.opening({ factory: h.WebTransport });
  assert.equal(transport.url, endpoint);
  assert.equal(transport.extra.length, 0, "no certificate bypass or fallback options");
  assert.equal(transport.creates, 0);
  await flush();
  assert.equal(result.state, "pending");
  transport.readyGate.resolve();
  await flush();
  assert.equal(transport.creates, 1);
  assert.equal(result.state, "pending");
  const io = h.stream();
  transport.streamGate.resolve(io.stream);
  const owner = await success(result);
  assert.equal(owner.closed, false);
  assert.equal(io.readerLocks, 1);
  assert.equal(io.writerLocks, 1);
  assert.equal(h.timers.size, 0);
  owner.close();
  assertReleased(h, transport, io);
});

test("one setup deadline and cancellation dispose late streams without reviving the owner", async () => {
  const h = await harness();
  const { result, transport } = h.opening();
  await h.elapse(40);
  transport.readyGate.resolve();
  await flush();
  assert.equal(transport.creates, 1);
  await h.elapse(9);
  assert.equal(result.state, "pending");
  await h.elapse(1);
  await failure(result, "timeout");
  assert.equal(transport.closes, 1);
  const late = h.stream();
  transport.streamGate.resolve(late.stream);
  await flush();
  assert.equal(late.cancels + late.unlockedCancels, 1);
  assert.equal(late.aborts + late.unlockedAborts, 1);
  assert.equal(result.state, "rejected");
  assert.equal(h.timers.size, 0);

  for (const stage of ["ready", "stream"]) {
    const h = await harness();
    const abort = new AbortController();
    const { result, transport } = h.opening({ signal: abort.signal });
    if (stage === "stream") { transport.readyGate.resolve(); await flush(); }
    abort.abort();
    await failure(result, "aborted");
    transport.readyGate.resolve();
    const late = h.stream();
    transport.streamGate.resolve(late.stream);
    await flush();
    assert.equal(transport.closes, 1);
    assert.equal(transport.creates, stage === "stream" ? 1 : 0);
    if (stage === "stream") {
      assert.equal(late.cancels + late.unlockedCancels, 1);
      assert.equal(late.aborts + late.unlockedAborts, 1);
    }
    assert.equal(h.timers.size, 0);
  }
  for (const stage of ["ready", "stream"]) {
    const h = await harness();
    const { result, transport } = h.opening();
    if (stage === "ready") transport.readyGate.reject(new Error("handshake rejected"));
    else {
      transport.readyGate.resolve(); await flush();
      transport.streamGate.reject(new Error("stream refused"));
    }
    await failure(result, "transport");
    assert.equal(transport.closes, 1);
    assert.equal(h.timers.size, 0);
  }
});

test("readPrefix preserves coalesced suffixes, exact view extents, and empty-chunk boundaries", async () => {
  const h = await harness();
  const { owner, io, transport } = await h.opened();
  const first = attempt(() => owner.readPrefix(4));
  await flush();
  assert.equal(io.reads.length, 1);
  const backing = Uint8Array.from([99, 1, 2, 3, 4, 5, 6, 7, 88]);
  io.reads[0].resolve({ done: false, value: backing.subarray(1, 8) });
  assert.deepEqual([...await success(first)], [1, 2, 3, 4]);
  assert.deepEqual([...await success(attempt(() => owner.readPrefix(2)))], [5, 6]);
  assert.deepEqual([...await success(attempt(() => owner.readPrefix(MAX_FRAME)))], [7]);
  assert.equal(io.reads.length, 1, "suffix reads must not acquire a second platform chunk");
  const next = attempt(() => owner.readPrefix(2));
  await flush();
  io.reads[1].resolve({ done: false, value: new Uint8Array(0) });
  await flush();
  assert.equal(next.state, "pending");
  io.reads[2].resolve({ done: false, value: Uint8Array.from([8, 9, 10]) });
  assert.deepEqual([...await success(next)], [8, 9]);
  assert.deepEqual([...await success(attempt(() => owner.readPrefix(1)))], [10]);
  const bounded = attempt(() => owner.readPrefix(MAX_FRAME));
  await flush();
  io.reads[3].resolve({ done: false, value: new Uint8Array(MAX_CHUNK).fill(0xa5) });
  const prefix = await success(bounded);
  assert.equal(prefix.length, MAX_FRAME);
  assert.equal(prefix[0], 0xa5);
  assert.equal(prefix.at(-1), 0xa5);
  owner.close();
  assertReleased(h, transport, io);
});

test("read validation is recoverable but malformed or unbounded platform chunks fence", async () => {
  const h = await harness();
  const { owner, io, transport } = await h.opened();
  for (const max of [0, -1, 1.5, NaN, Infinity, MAX_FRAME + 1, 1n]) {
    await failure(attempt(() => owner.readPrefix(max)), "validation");
  }
  assert.equal(io.reads.length, 0);
  assert.equal(owner.closed, false);
  owner.close();
  assertReleased(h, transport, io);

  const detached = new Uint8Array([1]);
  structuredClone(detached.buffer, { transfer: [detached.buffer] });
  for (const value of [new ArrayBuffer(4), new DataView(new ArrayBuffer(4)), new Uint16Array([1]),
    new Uint8Array(new SharedArrayBuffer(4)), new Uint8Array(MAX_CHUNK + 1),
    new Uint8Array(new ArrayBuffer(MAX_CHUNK + 1), 0, 1), detached, null]) {
    const h = await harness();
    const { owner, io, transport } = await h.opened();
    const read = attempt(() => owner.readPrefix(4));
    await flush();
    io.reads[0].resolve({ done: false, value });
    const error = await failure(read, "protocol");
    assert.equal(owner.closed, true);
    assert.equal(await failure(attempt(() => owner.readPrefix(1)), "protocol"), error);
    assertReleased(h, transport, io);
  }
  const empty = await harness();
  const opened = await empty.opened();
  const read = attempt(() => opened.owner.readPrefix(1));
  for (let index = 0; index < 17 && read.state === "pending"; index++) {
    await flush();
    assert.ok(opened.io.reads[index], "empty-chunk retries must remain bounded and sequential");
    opened.io.reads[index].resolve({ done: false, value: new Uint8Array(0) });
    await flush();
  }
  assert.equal(read.state, "rejected", "empty input must not spin indefinitely");
  await failure(read, "protocol");
  assert.ok(opened.io.reads.length <= 17);
  assertReleased(empty, opened.transport, opened.io);
});

test("writes snapshot selected bytes before waiting, preserve completion barriers, and allow duplex only", async () => {
  const h = await harness();
  const { owner, io, transport } = await h.opened();
  for (const bytes of [[], new ArrayBuffer(4), new Uint8Array(0), new Uint8Array(MAX_FRAME + 1),
    new Uint8Array(new SharedArrayBuffer(4))]) {
    await failure(attempt(() => owner.write(bytes)), "validation");
  }
  assert.equal(io.writes.length, 0);
  const source = Uint8Array.from([99, 66, 75, 77, 80, 88]);
  const view = source.subarray(1, 5);
  const write = attempt(() => owner.write(view));
  source.fill(0);
  await flush();
  assert.equal(write.state, "pending");
  assert.equal(io.writes.length, 1);
  const snapshot = io.writes[0].bytes;
  assert.notEqual(snapshot.buffer, source.buffer);
  assert.deepEqual([...snapshot], [66, 75, 77, 80]);
  assert.equal(source.byteLength, 6, "caller ownership is not transferred or detached");
  await failure(attempt(() => owner.write(Uint8Array.of(1))), "busy");
  assert.equal(io.writes.length, 1);
  const read = attempt(() => owner.readPrefix(1));
  await flush();
  await failure(attempt(() => owner.readPrefix(1)), "busy");
  assert.equal(io.reads.length, 1);
  io.reads[0].resolve({ done: false, value: Uint8Array.of(9) });
  assert.deepEqual([...await success(read)], [9]);
  assert.equal(write.state, "pending", "received peer bytes are not a local write receipt");
  io.writes[0].gate.resolve();
  assert.equal(await success(write), undefined);
  assert.equal(owner.closed, false);
  const maximum = attempt(() => owner.write(new Uint8Array(MAX_FRAME)));
  await flush();
  assert.equal(io.writes[1].bytes.length, MAX_FRAME);
  io.writes[1].gate.resolve();
  await success(maximum);
  owner.close();
  assertReleased(h, transport, io);
});

test("I/O deadlines fence both directions and ignore late successful platform operations", async () => {
  for (const first of ["read", "write"]) {
    const h = await harness();
    const { owner, io, transport } = await h.opened();
    const read = first === "read" ? attempt(() => owner.readPrefix(2)) : null;
    const write = attempt(() => owner.write(Uint8Array.of(1, 2)));
    const pendingRead = read ?? attempt(() => owner.readPrefix(2));
    await flush();
    await h.elapse(29);
    assert.equal(write.state, "pending");
    assert.equal(pendingRead.state, "pending");
    await h.elapse(1);
    const error = await failure(first === "read" ? pendingRead : write, "timeout");
    assert.equal(await failure(first === "read" ? write : pendingRead, "timeout"), error);
    assert.equal(owner.closed, true);
    io.reads[0].resolve({ done: false, value: Uint8Array.of(3, 4) });
    io.writes[0].gate.resolve();
    await flush();
    assert.equal(await failure(attempt(() => owner.readPrefix(1)), "timeout"), error);
    assert.equal(await failure(attempt(() => owner.write(Uint8Array.of(5))), "timeout"), error);
    assert.equal(io.reads.length, 1);
    assert.equal(io.writes.length, 1);
    assertReleased(h, transport, io);
  }
});

test("EOF, remote closure, and platform errors terminate pending operations without retry", async () => {
  for (const fault of ["eof", "remote-close", "remote-reject", "read-reject", "write-reject"]) {
    const h = await harness();
    const { owner, io, transport } = await h.opened();
    const read = attempt(() => owner.readPrefix(2));
    const write = attempt(() => owner.write(Uint8Array.of(1)));
    await flush();
    if (fault === "eof") io.reads[0].resolve({ done: true, value: undefined });
    if (fault === "remote-close") transport.closedGate.resolve({ closeCode: 0, reason: "peer ended" });
    if (fault === "remote-reject") transport.closedGate.reject(new Error("connection failed"));
    if (fault === "read-reject") io.reads[0].reject(new Error("receive failed"));
    if (fault === "write-reject") io.writes[0].gate.reject(new Error("send failed"));
    const code = fault === "eof" || fault === "remote-close" ? "closed" : "transport";
    const error = await failure(read, code);
    assert.equal(await failure(write, code), error);
    owner.close();
    owner.close();
    assertReleased(h, transport, io);
  }
});

test("explicit close and lifetime abort are nonblocking, idempotent, and release owned locks", async () => {
  for (const faults of [{ cleanupPending: true }, { cleanupRejects: true },
    { cancelThrows: true, abortThrows: true, closeThrows: true, releaseThrows: true }]) {
    const h = await harness(faults);
    const { owner, io, transport } = await h.opened();
    const read = attempt(() => owner.readPrefix(1));
    const write = attempt(() => owner.write(Uint8Array.of(1)));
    await flush();
    assert.equal(owner.close(), undefined);
    assert.equal(owner.close(), undefined);
    assert.equal(owner.closed, true);
    await failure(read, "closed");
    await failure(write, "closed");
    await flush();
    assertReleased(h, transport, io);
    io.reads[0].resolve({ done: false, value: Uint8Array.of(9) });
    io.writes[0].gate.resolve();
    await flush();
    await failure(attempt(() => owner.readPrefix(1)), "closed");
  }
  const h = await harness();
  const abort = new AbortController();
  const { owner, io, transport } = await h.opened({ signal: abort.signal });
  const read = attempt(() => owner.readPrefix(1));
  await flush();
  abort.abort();
  const error = await failure(read, "aborted");
  assert.equal(owner.closed, true);
  assert.equal(await failure(attempt(() => owner.write(Uint8Array.of(1))), "aborted"), error);
  owner.close();
  assertReleased(h, transport, io);
});
