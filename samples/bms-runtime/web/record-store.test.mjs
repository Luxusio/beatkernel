// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/record-store.test.mjs
// Actual store source; manually delivered IDB events, with no persistence algorithm mock.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

async function flush() { for (let index = 0; index < 24; index++) await Promise.resolve(); }
function track(promise) {
  const result = { state: "pending" };
  result.done = Promise.resolve(promise).then(value => { result.state = "fulfilled"; result.value = value; },
    error => { result.state = "rejected"; result.error = error; });
  return result;
}
function attempt(call) { try { return track(call()); } catch (error) { return track(Promise.reject(error)); } }
async function success(result) { await result.done; assert.equal(result.state, "fulfilled"); return result.value; }
async function failure(result, code) {
  await result.done;
  assert.equal(result.state, "rejected");
  assert.equal(result.error.code, code);
  return result.error;
}
function stored(id = 1, fields = {}) {
  return { id, name: `record-${id}.bkr`, chartPath: "Songs/曲/chart.bms", complete: false,
    hits: "18446744073709551615", misses: "0", combo: null, createdAt: 1234567890,
    byteLength: 4, ...fields };
}
function input(fields = {}) {
  return { bytes: Uint8Array.from([66, 75, 82, 255]), name: "recorded-prefix.bkr",
    chartPath: "Songs/曲/chart.bms", complete: false, hits: 18446744073709551615n,
    misses: 0n, combo: null, ...fields };
}

async function harness() {
  const timers = new Map();
  const allocations = [];
  let timerId = 0;
  class Endpoint {
    constructor() { this.listeners = new Map(); }
    addEventListener(type, handler) {
      if (!this.listeners.has(type)) this.listeners.set(type, new Set());
      this.listeners.get(type).add(handler);
    }
    removeEventListener(type, handler) { this.listeners.get(type)?.delete(handler); }
    emit(type, fields = {}) {
      const event = { type, target: this, preventDefault() {}, ...fields };
      this[`on${type}`]?.(event);
      for (const handler of [...(this.listeners.get(type) ?? [])]) handler(event);
    }
  }
  class Request extends Endpoint {
    constructor(store, kind, args = []) { super(); Object.assign(this, { store, kind, args, result: undefined, error: null }); }
    succeed(value) { this.result = value; this.emit("success"); }
  }
  class Transaction extends Endpoint {
    constructor(db, names, mode) {
      super(); Object.assign(this, { db, names: [...names], mode, requests: [], error: null, aborts: 0 });
    }
    objectStore(name) {
      assert.ok(this.mode === "versionchange" ? this.db.objectStoreNames.contains(name) : this.names.includes(name),
        "operation escaped its declared transaction scope");
      const request = (kind, args) => {
        const result = new Request(name, kind, args);
        this.requests.push(result);
        return result;
      };
      return {
        keyPath: this.db?.schemas.find(row => row.name === name)?.options.keyPath,
        autoIncrement: this.db?.schemas.find(row => row.name === name)?.options.autoIncrement ?? false,
        openCursor: (...args) => request("cursor", args),
        get: (...args) => request("get", args),
        add: (...args) => request("add", args),
        put: (...args) => request("put", args),
        delete: (...args) => request("delete", args),
      };
    }
    abort() { this.aborts++; queueMicrotask(() => this.emit("abort")); }
    complete() { this.emit("complete"); }
    fail(error) { this.error = error; this.emit("abort"); }
    request(store, kind) {
      const matches = this.requests.filter(row => row.store === store && row.kind === kind);
      assert.equal(matches.length, 1, `${store}.${kind} must identify one actual IDB request`);
      return matches[0];
    }
  }
  class Database extends Endpoint {
    constructor() {
      super(); this.transactions = []; this.closes = 0; this.schemas = []; this.version = 1;
      this.objectStoreNames = [];
      this.objectStoreNames.contains = name => this.objectStoreNames.includes(name);
    }
    createObjectStore(name, options) { this.schemas.push({ name, options }); this.objectStoreNames.push(name); return {}; }
    transaction(names, mode) {
      assert.equal(this.closes, 0, "no transaction may start on a closed connection");
      const transaction = new Transaction(this, typeof names === "string" ? [names] : names, mode);
      this.transactions.push(transaction);
      return transaction;
    }
    close() { this.closes++; }
  }
  const factory = {
    opens: [],
    open(name, version) {
      const request = new Request(null, "open", [name, version]);
      request.transaction = new Transaction(null, [], "versionchange");
      this.opens.push(request);
      return request;
    },
  };
  const ObservedBytes = new Proxy(Uint8Array, {
    construct(target, args) { allocations.push(args); return Reflect.construct(target, args, target); },
  });
  const context = createContext({
    indexedDB: factory, AbortController, AbortSignal, DOMException, TextEncoder, Uint8Array: ObservedBytes, ArrayBuffer,
    Date: class extends Date { static now() { return 1234567890; } },
    setTimeout(callback, delay) { const id = ++timerId; timers.set(id, { callback, delay }); return id; },
    clearTimeout(id) { timers.delete(id); },
  });
  const model = new SourceTextModule(await readFile(new URL("./host_model.mjs", import.meta.url), "utf8"), { context });
  const module = new SourceTextModule(await readFile(new URL("./record-store.mjs", import.meta.url), "utf8"), { context });
  await module.link(specifier => { assert.equal(specifier, "./host_model.mjs"); return model; });
  await module.evaluate();
  const { RecordsStore, RecordStoreError } = module.namespace;
  function opening(options = {}) {
    const result = track(RecordsStore.open({ factory, ...options }));
    return { result, request: factory.opens.at(-1) };
  }
  async function opened(options = {}) {
    const { result, request } = opening(options);
    const db = new Database();
    request.result = db;
    request.transaction.db = db;
    request.emit("upgradeneeded", { oldVersion: 0, newVersion: 1 });
    request.succeed(db);
    return { store: await success(result), db };
  }
  return { RecordsStore, RecordStoreError, factory, timers, allocations, Database, opening, opened,
    async timeout() {
      assert.ok(timers.size > 0, "a controlled operation deadline is required");
      const [id, timer] = timers.entries().next().value;
      timers.delete(id); timer.callback(); await flush();
    },
  };
}

function scan(request, rows) {
  for (const row of rows) {
    let continued = false;
    request.succeed({ key: row.id, primaryKey: row.id, value: row,
      continue() { assert.equal(continued, false); continued = true; } });
    if (!continued) return;
  }
  request.succeed(null);
}
function bothStores(transaction, mode) {
  assert.equal(transaction.mode, mode);
  assert.deepEqual([...transaction.names].sort(), ["metadata", "recordings"]);
}

test("archive save validates IDs and whole standalone buffers before snapshots or transaction effects", async () => {
  const h=await harness();const {store,db}=await h.opened();const before=db.transactions.length;
  for(const fields of [{completedArchive:new Uint8Array(0),archivePlayer:1},
    {completedArchive:new Uint8Array(5*1024*1024+1),archivePlayer:1},
    {completedArchive:new Uint8Array(4).subarray(1),archivePlayer:1},
    {completedArchive:new Uint8Array([1]),archivePlayer:0},
    {completedArchive:new Uint8Array([1]),archivePlayer:4294967296},
    {completedArchive:new Uint8Array([1]),archivePlayer:1.5}, {archivePlayer:7}]) {
    await failure(attempt(()=>store.save(input(fields))),"validation");
    assert.equal(db.transactions.length,before);
  }
  const source=input({completedArchive:Uint8Array.from([8,7,6]),archivePlayer:4294967295});
  const saved=attempt(()=>store.save(source));source.completedArchive[0]=0;source.bytes[0]=0;
  const tx=db.transactions.at(-1);scan(tx.request("metadata","cursor"),[]);
  const metadata=tx.request("metadata","add").args[0];assert.equal(metadata.archiveByteLength,3);assert.equal(metadata.archivePlayer,4294967295);
  tx.request("metadata","add").succeed(7);const payload=tx.request("recordings","add");
  assert.deepEqual(Array.from(payload.args[0].completedArchive),[8,7,6]);
  assert.notEqual(payload.args[0].completedArchive.buffer,source.completedArchive.buffer);
  assert.notEqual(payload.args[0].completedArchive.buffer,payload.args[0].bytes.buffer);
  payload.succeed(7);await flush();assert.equal(saved.state,"pending");tx.complete();
  assert.equal((await success(saved)).archivePlayer,4294967295);store.close();
});
test("archive association reads reject later corruption atomically while legacy payload stays compatible",async()=>{
  const h=await harness();const {store,db}=await h.opened();const bytes=Uint8Array.from([66,75,82,255]);
  const archive=Uint8Array.from([8,7,6]);const metadata=stored(7,{archiveByteLength:3,archivePlayer:4294967295});
  for(const payload of [{id:7,bytes},{id:7,bytes,completedArchive:archive,archivePlayer:7},
    {id:7,bytes,completedArchive:new Uint8Array(2),archivePlayer:4294967295},
    {id:7,bytes,completedArchive:new Uint8Array(4).subarray(1),archivePlayer:4294967295}]){
    const loaded=attempt(()=>store.load(7));const tx=db.transactions.at(-1);
    tx.request("metadata","get").succeed(metadata);tx.request("recordings","get").succeed(payload);
    if(tx.aborts===0)tx.complete();await failure(loaded,"corrupt");
  }
  const loaded=attempt(()=>store.load(7));const tx=db.transactions.at(-1);
  tx.request("metadata","get").succeed(metadata);tx.request("recordings","get").succeed({id:7,bytes,completedArchive:archive,archivePlayer:4294967295});
  await flush();assert.equal(loaded.state,"pending");tx.complete();const result=await success(loaded);
  assert.equal(result.archivePlayer,4294967295);assert.deepEqual(Array.from(result.completedArchive),[8,7,6]);
  const legacy=attempt(()=>store.load(7));const old=db.transactions.at(-1);
  old.request("metadata","get").succeed(stored(7));old.request("recordings","get").succeed({id:7,bytes});old.complete();
  assert.equal((await success(legacy)).completedArchive,undefined);store.close();
});
test("archive capacity charges every private copy and transaction abort is authoritative",async()=>{
  const h=await harness();const {store,db}=await h.opened();const recording=input({completedArchive:new Uint8Array(3),archivePlayer:7});
  const full=attempt(()=>store.save(recording));const tx=db.transactions.at(-1);
  scan(tx.request("metadata","cursor"),Array.from({length:4},(_,index)=>stored(index+1,{byteLength:64*1024*1024-(index===3?6:0)})));
  await failure(full,"validation");assert.equal(tx.requests.some(row=>row.kind==="add"),false);
  const failed=attempt(()=>store.save(recording));const aborted=db.transactions.at(-1);scan(aborted.request("metadata","cursor"),[]);
  aborted.request("metadata","add").succeed(9);aborted.request("recordings","add").succeed(9);
  await flush();assert.equal(failed.state,"pending");aborted.fail(new DOMException("archive quota at commit","QuotaExceededError"));
  await failure(failed,"quota");store.close();
});

test("invalid save fields reject before a transaction and valid aliases retain exact Unicode and integer scores", async () => {
  const h = await harness();
  const { store, db } = await h.opened();
  const transactions = db.transactions.length;
  const detached = new Uint8Array([1]);
  structuredClone(detached.buffer, { transfer: [detached.buffer] });
  for (const fields of [
    { name: "" }, { name: "x".repeat(257) }, { name: 1 },
    { chartPath: "../chart.bms" }, { chartPath: "./C:/chart.bms" }, { chartPath: "가".repeat(1366) },
    { complete: 1 }, { hits: -1n }, { misses: 18446744073709551616n }, { combo: "1" },
    { bytes: [] }, { bytes: new Uint8Array(0) }, { bytes: detached },
    { bytes: new Uint8Array([1, 2]).subarray(1) }, { bytes: new Uint8Array(64 * 1024 * 1024 + 1) },
  ]) {
    const error = await failure(attempt(() => store.save(input(fields))), "validation");
    assert.ok(error instanceof h.RecordStoreError);
    assert.equal(db.transactions.length, transactions);
  }
  assert.equal(h.allocations.length, 0, "all invalid fields are checked before allocating a private byte snapshot");
  const result = attempt(() => store.save(input({ name: "💿".repeat(256), chartPath: "./Songs\\曲//chart.bms" })));
  assert.equal(h.allocations.length, 1, "valid save creates its bounded private snapshot");
  const tx = db.transactions.at(-1);
  bothStores(tx, "readwrite");
  scan(tx.request("metadata", "cursor"), []);
  const metadata = tx.request("metadata", "add");
  assert.equal(metadata.args[0].chartPath, "Songs/曲/chart.bms");
  assert.equal(metadata.args[0].hits, "18446744073709551615");
  assert.equal(metadata.args[0].combo, null);
  metadata.succeed(9);
  tx.request("recordings", "add").succeed(9);
  tx.complete();
  const saved = await success(result);
  assert.equal(saved.id, 9);
  assert.equal(saved.name, "💿".repeat(256));
  assert.equal(saved.hits, 18446744073709551615n);
  assert.equal(saved.byteLength, 4);
  store.close();
  assert.equal(h.timers.size, 0);
});

test("save snapshots bytes and requires atomic transaction completion despite successful individual requests", async () => {
  const h = await harness();
  const { store, db } = await h.opened();
  const source = input();
  const result = attempt(() => store.save(source));
  source.bytes[0] = 0;
  const tx = db.transactions.at(-1);
  bothStores(tx, "readwrite");
  scan(tx.request("metadata", "cursor"), []);
  tx.request("metadata", "add").succeed(5);
  const payload = tx.request("recordings", "add");
  assert.equal(payload.args[0].id, 5);
  assert.deepEqual(Array.from(payload.args[0].bytes), [66, 75, 82, 255]);
  assert.notEqual(payload.args[0].bytes.buffer, source.bytes.buffer);
  payload.succeed(5);
  await flush();
  assert.equal(result.state, "pending");
  tx.complete();
  assert.equal((await success(result)).id, 5);

  const failed = attempt(() => store.save(input()));
  const aborted = db.transactions.at(-1);
  scan(aborted.request("metadata", "cursor"), []);
  aborted.request("metadata", "add").succeed(6);
  aborted.request("recordings", "add").succeed(6);
  aborted.fail(new DOMException("storage quota failed at commit", "QuotaExceededError"));
  await failure(failed, "quota");
  assert.equal(store.closed, false, "an ordinary failed write does not claim connection failure");
  const list = attempt(() => store.list());
  const after = db.transactions.at(-1);
  scan(after.request("metadata", "cursor"), [stored(5)]);
  after.complete();
  assert.deepEqual(Array.from(await success(list), row => row.id), [5], "a later actual metadata response remains authoritative");
  store.close();
});

test("capacity checks and writes share the same cross-connection lock scope without scanning payload bytes", async () => {
  const h = await harness();
  const left = await h.opened();
  const right = await h.opened();
  assert.deepEqual(h.factory.opens.map(request => request.args), [["beatkernel-records", 1], ["beatkernel-records", 1]]);
  const a = attempt(() => left.store.save(input()));
  const b = attempt(() => right.store.save(input()));
  for (const tx of [left.db.transactions.at(-1), right.db.transactions.at(-1)]) {
    bothStores(tx, "readwrite");
    assert.deepEqual(tx.requests.map(request => [request.store, request.kind]), [["metadata", "cursor"]]);
  }
  const first = left.db.transactions.at(-1);
  scan(first.request("metadata", "cursor"), Array.from({ length: 128 }, (_, index) => stored(index + 1)));
  await failure(a, "validation");
  assert.equal(first.requests.some(request => request.kind === "add"), false);
  assert.equal(first.aborts, 1);
  const second = right.db.transactions.at(-1);
  scan(second.request("metadata", "cursor"), Array.from({ length: 4 }, (_, index) => stored(index + 1,
    { byteLength: 64 * 1024 * 1024 })));
  await failure(b, "validation");
  assert.equal(second.requests.some(request => request.store === "recordings"), false);
  const exact = attempt(() => left.store.save(input()));
  const tx = left.db.transactions.at(-1);
  scan(tx.request("metadata", "cursor"), Array.from({ length: 4 }, (_, index) => stored(index + 1,
    { byteLength: 64 * 1024 * 1024 - (index === 3 ? 4 : 0) })));
  tx.request("metadata", "add").succeed(5);
  tx.request("recordings", "add").succeed(5);
  tx.complete();
  assert.equal((await success(exact)).byteLength, 4);
  left.store.close(); right.store.close();
});

test("bounded metadata lists never acquire payloads and reject corrupt stored identity, scores and extents", async () => {
  const h = await harness();
  const { store, db } = await h.opened();
  const listed = attempt(() => store.list());
  const tx = db.transactions.at(-1);
  assert.equal(tx.mode, "readonly");
  scan(tx.request("metadata", "cursor"), [stored(2), stored(7, { complete: true })]);
  assert.equal(tx.requests.some(request => request.store === "recordings"), false);
  await flush();
  assert.equal(listed.state, "pending");
  tx.complete();
  const rows = await success(listed);
  assert.deepEqual(Array.from(rows, row => row.id), [7, 2]);
  assert.equal(rows[0].hits, 18446744073709551615n);
  assert.equal(rows[0].combo, null);
  for (const row of [stored(1, { hits: "01" }), stored(1, { misses: "18446744073709551616" }),
    stored(1, { chartPath: "Songs/./曲/chart.bms" }), stored(1, { complete: "false" }),
    stored(1, { byteLength: 0 }), stored(1, { createdAt: -1 })]) {
    const invalid = attempt(() => store.list());
    scan(db.transactions.at(-1).request("metadata", "cursor"), [row]);
    await failure(invalid, "corrupt");
  }
  const mismatched = attempt(() => store.list());
  db.transactions.at(-1).request("metadata", "cursor").succeed({ key: 2, primaryKey: 2, value: stored(1),
    continue() { assert.fail("corrupt cursor identity cannot progress"); } });
  await failure(mismatched, "corrupt");
  for (const rows of [Array.from({ length: 129 }, (_, index) => stored(index + 1)),
    Array.from({ length: 5 }, (_, index) => stored(index + 1, { byteLength: 64 * 1024 * 1024 }))]) {
    const oversized = attempt(() => store.list());
    scan(db.transactions.at(-1).request("metadata", "cursor"), rows);
    await failure(oversized, "corrupt");
  }
  store.close();
});

test("load validates the exact payload on one transaction and delete commits both stores even for a missing record", async () => {
  const h = await harness();
  const { store, db } = await h.opened();
  const bytes = Uint8Array.from([66, 75, 82, 255]);
  const loaded = attempt(() => store.load(7));
  const tx = db.transactions.at(-1);
  bothStores(tx, "readonly");
  tx.request("metadata", "get").succeed(stored(7));
  tx.request("recordings", "get").succeed({ id: 7, bytes });
  await flush();
  assert.equal(loaded.state, "pending");
  tx.complete();
  const result = await success(loaded);
  assert.equal(result.metadata.id, 7);
  assert.equal(result.metadata.hits, 18446744073709551615n);
  assert.deepEqual(Array.from(result.bytes), Array.from(bytes));
  for (const [metadata, payload, code] of [
    [undefined, undefined, "not-found"], [stored(7), undefined, "corrupt"],
    [stored(7), { id: 8, bytes }, "corrupt"], [stored(7), { id: 7, bytes: [1, 2, 3, 4] }, "corrupt"],
    [stored(7), { id: 7, bytes: new Uint8Array(3) }, "corrupt"],
    [stored(7, { byteLength: 3 }), { id: 7, bytes: bytes.subarray(1) }, "corrupt"],
  ]) {
    const invalid = attempt(() => store.load(7));
    const bad = db.transactions.at(-1);
    bad.request("metadata", "get").succeed(metadata);
    const payloadRequest = bad.requests.find(request => request.store === "recordings" && request.kind === "get");
    if (metadata !== undefined) { assert.ok(payloadRequest); payloadRequest.succeed(payload); }
    else assert.equal(payloadRequest, undefined, "missing metadata does not acquire orphan payload bytes");
    // Validation can wait for both requests but must never publish a corrupt result.
    if (bad.aborts === 0) bad.complete();
    await failure(invalid, code);
  }
  for (const exists of [true, false]) {
    const removed = attempt(() => store.remove(7));
    const deletion = db.transactions.at(-1);
    bothStores(deletion, "readwrite");
    deletion.request("metadata", "get").succeed(exists ? stored(7) : undefined);
    for (const name of ["metadata", "recordings"]) {
      const request = deletion.request(name, "delete");
      assert.equal(request.args[0], 7);
      request.succeed(undefined);
    }
    await flush();
    assert.equal(removed.state, "pending");
    deletion.complete();
    assert.equal(await success(removed), exists);
  }
  const count = db.transactions.length;
  for (const id of [0, -1, 1.5, "1", Number.MAX_SAFE_INTEGER + 1]) {
    await failure(attempt(() => store.load(id)), "validation");
    await failure(attempt(() => store.remove(id)), "validation");
  }
  assert.equal(db.transactions.length, count);
  store.close();
});

test("unavailable, blocked, timed-out and cancelled opens cannot retain a late connection or upgrade", async () => {
  const h = await harness();
  await failure(attempt(() => h.RecordsStore.open({ factory: null })), "unavailable");
  for (const timeoutMs of [0, 60001, 1.5]) {
    await failure(attempt(() => h.RecordsStore.open({ factory: h.factory, timeoutMs })), "validation");
  }
  const already = new AbortController(); already.abort();
  await failure(attempt(() => h.RecordsStore.open({ factory: h.factory, signal: already.signal })), "aborted");
  assert.equal(h.factory.opens.length, 0);
  for (const code of ["blocked", "timeout", "aborted"]) {
    const controller = new AbortController();
    const { result, request } = h.opening({ signal: controller.signal });
    if (code === "blocked") request.emit("blocked");
    else if (code === "aborted") controller.abort();
    else await h.timeout();
    const error = await failure(result, code);
    assert.equal(error.operation, "open");
    const late = new h.Database();
    request.result = late;
    request.transaction.db = late;
    request.emit("upgradeneeded", { oldVersion: 0, newVersion: 1 });
    request.succeed(late);
    assert.ok(request.transaction.aborts >= 1);
    assert.ok(late.closes >= 1);
    assert.equal(late.schemas.length, 0, "settled opening cannot create stores during a late upgrade");
    assert.equal(late.transactions.length, 0);
    assert.equal(h.timers.size, 0);
  }
  const controller = new AbortController();
  const upgrading = h.opening({ signal: controller.signal });
  const db = new h.Database();
  upgrading.request.result = db;
  upgrading.request.transaction.db = db;
  upgrading.request.emit("upgradeneeded", { oldVersion: 0, newVersion: 1 });
  controller.abort();
  await failure(upgrading.result, "aborted");
  assert.ok(db.closes >= 1, "an in-progress upgrade connection is also released");
  assert.ok(upgrading.request.transaction.aborts >= 1);

  const incompatible = h.opening();
  const wrong = new h.Database();
  wrong.createObjectStore("metadata", { keyPath: "different", autoIncrement: true });
  wrong.createObjectStore("recordings", { keyPath: "id", autoIncrement: false });
  incompatible.request.succeed(wrong);
  await failure(incompatible.result, "corrupt");
  assert.ok(wrong.closes >= 1);
  assert.equal(h.timers.size, 0);
});

test("owner closure and version or timeout faults fence every pending operation without late success", async () => {
  for (const code of ["closed", "versionchange", "timeout", "unexpected-close"]) {
    const h = await harness();
    const controller = new AbortController();
    const { store, db } = await h.opened({ signal: controller.signal });
    controller.abort();
    assert.equal(store.closed, false, "the open signal does not become an owner lifetime signal");
    const list = attempt(() => store.list());
    const listTx = db.transactions.at(-1);
    const saved = attempt(() => store.save(input()));
    const saveTx = db.transactions.at(-1);
    scan(saveTx.request("metadata", "cursor"), []);
    saveTx.request("metadata", "add").succeed(1);
    saveTx.request("recordings", "add").succeed(1);
    assert.equal(saved.state, "pending");
    if (code === "closed") store.close();
    else if (code === "versionchange") db.emit("versionchange", { newVersion: 2 });
    else if (code === "unexpected-close") db.emit("close");
    else {
      // A native abort can fail after a commit became uncertain. A timeout must
      // still fence ownership and may never claim a successful durable write.
      saveTx.abort = () => { saveTx.aborts++; throw new DOMException("already committing", "InvalidStateError"); };
      await h.timeout();
    }
    const expected = code === "unexpected-close" ? "closed" : code;
    assert.equal((await failure(list, expected)).operation, "list");
    assert.equal((await failure(saved, expected)).operation, "save");
    assert.equal(store.closed, true);
    assert.ok(listTx.aborts >= 1);
    assert.ok(saveTx.aborts >= 1);
    assert.equal(db.closes, 1);
    const count = db.transactions.length;
    await failure(attempt(() => store.list()), "closed");
    await failure(attempt(() => store.save(input())), "closed");
    assert.equal(db.transactions.length, count);
    listTx.complete(); saveTx.complete();
    await flush();
    assert.equal(list.state, "rejected");
    assert.equal(saved.state, "rejected");
    store.close();
    assert.equal(db.closes, 1);
    assert.equal(h.timers.size, 0);
  }
});
