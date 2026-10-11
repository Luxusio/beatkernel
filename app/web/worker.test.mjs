// Deferred: node --experimental-vm-modules --test app/web/*.test.mjs
// Loads actual Worker/helpers and current generated WASM menu owners; no browser.
import assert from "node:assert/strict";
import { File as NodeFile } from "node:buffer";
import { MessageChannel as NodeMessageChannel } from "node:worker_threads";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { ROOM_SESSION_METHODS } from "./room-owner.mjs";
import { preflightMenuPacket } from "./render-protocol.mjs";
import { LocalRoster } from "./local-play-host.mjs";
import { KEY_BINDINGS } from "./play-model.mjs";
import { keyboardBindingWords } from "./physical-input.mjs";
import { createContext, SourceTextModule, SyntheticModule, runInContext } from "node:vm";
const FileType = globalThis.File ?? NodeFile;

// Public RecordsStore lease double. Native request/transaction terminal events
// are separately controlled from callers rejected by close(). AC001 exercises
// the same public lease using actual RecordsStore and an IndexedDB factory.
function controlledRecordStore({ holdOpen = false, holdTransaction = false, cleanupError = null } = {}) {
  const opens = [], calls = [], transactions = [];
  const metadata = { id: 17, name: "Original member", chartPath: "song/chart.bms", complete: true,
    hits: 17n, misses: 3n, combo: 9n, byteLength: 4, archiveByteLength: 8, archivePlayer: 0xffffffff };
  const makeLease = () => {
    const opening = deferred(), openTerminal = deferred(), storeTerminal = deferred();
    let closed = false, opened = false;
    const finishStore = () => { if (closed && opened && transactions.every(tx => tx.ended)) storeTerminal.resolve(); };
    const operation = (action, value) => {
      calls.push({ action, value });
      if (closed) return Promise.reject(new Error("closed record owner"));
      const caller = deferred();
      const tx = { action, value, ended: false,
        complete() { if (this.ended) return; this.ended = true;
          caller.resolve(action === "list" ? [metadata] : action === "save" ? metadata : action === "remove" ? true :
            { metadata, bytes: Uint8Array.of(66,75,82,0), completedArchive: Uint8Array.of(66,75,82,69,83,85,76,84), archivePlayer: 0xffffffff });
          finishStore(); },
        abort() { if (this.ended) return; this.ended = true; caller.reject(new Error("native transaction aborted")); finishStore(); },
        rejectCaller() { caller.reject(new Error("record operation canceled")); },
      };
      transactions.push(tx);
      if (!holdTransaction) tx.complete();
      return caller.promise;
    };
    const store = { terminal: storeTerminal.promise,
      list: () => operation("list"), load: id => operation("load", id),
      save: value => operation("save", value), remove: id => operation("remove", id),
      close() { if (closed) return; closed = true; lease.closes++;
        for (const tx of transactions) if (!tx.ended) tx.rejectCaller(); finishStore(); },
      closeAndJoin() { store.close(); return cleanupError ? Promise.reject(new Error(cleanupError)) : store.terminal; },
    };
    const lease = { ready: opening.promise, terminal: openTerminal.promise, closes: 0,
      close() { store.close(); if (!opened) opening.reject(new Error("opening canceled")); },
      closeAndJoin() { lease.close(); return cleanupError ? Promise.reject(new Error(cleanupError)) :
        Promise.all([lease.terminal, store.terminal]).then(() => undefined); },
      finishOpen() { if (opened) return; opened = true; openTerminal.resolve();
        if (!closed) opening.resolve(store); finishStore(); },
    };
    opens.push(lease);
    if (!holdOpen) lease.finishOpen();
    return lease;
  };
  return { opens, calls, transactions, metadata, openTracked: makeLease };
}

function recordSaveValue(fields = {}) {
  return { metadata: { name: "Original member", chartPath: "song/chart.bms", complete: true,
    hits: 17n, misses: 3n, combo: 9n }, replayFile: new FileType([Uint8Array.of(66,75,82,0)], "member.bkr"),
    archiveBlob: new Blob([Uint8Array.of(66,75,82,69,83,85,76,84)]), archivePlayer: 0xffffffff,
    players: [0xffffffff, 7], player: 0xffffffff, ...fields };
}

async function recordWorker(recordsStore) {
  const h = await realPreparationWorker({ recordsStore });
  assert.equal(h.of("ready").length, 1); assert.equal(h.of("fatal").length, 0); return h;
}

async function settleRecord(predicate) {
  // Native Node File/Blob reads can settle on a later event-loop turn. Keep
  // controlled preparation delivery unchanged and wait only at explicit
  // expected record milestones, never through intentionally held reads/DBs.
  const deadline = performance.now() + 5000;
  while (!predicate() && performance.now() < deadline) {
    await new Promise(resolve => setImmediate(resolve)); await flushJobs();
  }
  assert.ok(predicate(), "bounded native record milestone must settle");
}

test("record requests lazily open and preserve exact action results and original archive player", async () => {
  const store = controlledRecordStore(), h = await recordWorker(store);
  assert.equal(store.opens.length, 0, "ready never automatically opens storage");
  await h.send({ kind: "records-request", id: 1, action: "list" });
  const listed = h.of("records-reply").at(-1);
  assert.deepEqual(structuredClone(listed), { kind: "records-reply", id: 1, action: "list", result: { entries: [store.metadata] }, error: null });
  await h.send({ kind: "records-request", id: 2, action: "load", recordId: 17 });
  const loaded = h.of("records-reply").at(-1);
  assert.equal(loaded.error, null); assert.equal(loaded.result.archivePlayer, 0xffffffff);
  assert.ok(loaded.result.replayFile instanceof FileType); assert.ok(loaded.result.archiveBlob instanceof Blob);
  assert.equal(loaded.result.replayFile.name, store.metadata.name, "loaded immutable File preserves the stored display name");
  assert.deepEqual(Array.from(new Uint8Array(await loaded.result.replayFile.arrayBuffer())), [66,75,82,0]);
  assert.deepEqual(Array.from(new Uint8Array(await loaded.result.archiveBlob.arrayBuffer())), [66,75,82,69,83,85,76,84]);
  const value = recordSaveValue();
  await h.send({ kind: "records-request", id: 3, action: "save", value });
  await settleRecord(() => h.of("records-reply").some(row => row.id === 3));
  assert.deepEqual(structuredClone(h.of("records-reply").at(-1)), { kind: "records-reply", id: 3, action: "save", result: { metadata: store.metadata }, error: null });
  const saved = store.calls.find(row => row.action === "save").value;
  assert.deepEqual(Array.from(saved.bytes), [66,75,82,0]);
  assert.deepEqual(Array.from(saved.completedArchive), [66,75,82,69,83,85,76,84]);
  assert.equal(saved.archivePlayer, 0xffffffff); assert.equal(saved.hits, 17n); assert.equal(saved.chartPath, "song/chart.bms");
  await h.send({ kind: "records-request", id: 4, action: "remove", recordId: 17 });
  assert.deepEqual(structuredClone(h.of("records-reply").at(-1)), { kind: "records-reply", id: 4, action: "remove", result: { removed: true }, error: null });
  assert.equal(store.calls.find(row => row.action === "remove").value, 17);
  assert.equal(store.opens.length, 1, "successful idle owner is reused");
  await h.send({ kind: "records-close", id: 5 });
  assert.deepEqual(structuredClone(h.of("records-closed").at(-1)), { kind: "records-closed", id: 5, error: null });
});

test("record metadata and handle preflight occurs before opening or any body read", async () => {
  const store = controlledRecordStore(), h = await recordWorker(store);
  let reads = 0, id = 1;
  const file = new FileType([Uint8Array.of(1)], "member.bkr");
  file.arrayBuffer = () => { reads++; assert.fail("invalid metadata must never read a body"); };
  const invalid = [
    { metadata: { ...recordSaveValue().metadata, name: "" } },
    { metadata: { ...recordSaveValue().metadata, chartPath: "../escape.bms" } },
    { metadata: { ...recordSaveValue().metadata, hits: 1 } },
    { metadata: { ...recordSaveValue().metadata, hits: -1n } },
    { metadata: { ...recordSaveValue().metadata, combo: 18446744073709551616n } },
    { players: [7,7], player: 7, archivePlayer: 7 }, { players: [], player: 1 },
    { player: 1 }, { archivePlayer: 7 }, { archiveBlob: null, archivePlayer: 0xffffffff },
    { replayFile: { size: 1, arrayBuffer() { assert.fail("fake handle"); } } },
    { replayFile: new FileType([], "empty.bkr") }, { archiveBlob: new Blob() },
    { replayFile: Object.assign(Object.create(FileType.prototype), { arrayBuffer() { assert.fail("spoofed File body"); } }) },
    { archiveBlob: Object.assign(Object.create(Blob.prototype), { arrayBuffer() { assert.fail("spoofed Blob body"); } }) },
  ];
  for (const fields of invalid) {
    await h.send({ kind: "records-request", id: id++, action: "save", value: recordSaveValue({ replayFile: file, ...fields }) });
    const reply = h.of("records-reply").at(-1);
    assert.equal(reply.result, null); assert.equal(typeof reply.error, "string");
    assert.ok(reply.error.length > 0 && reply.error.length <= 4096);
  }
  for (const recordId of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
    await h.send({ kind: "records-request", id: id++, action: "load", recordId });
    assert.equal(h.of("records-reply").at(-1).result, null);
  }
  assert.equal(reads, 0); assert.equal(store.opens.length, 0); assert.equal(store.calls.length, 0);
  assert.equal(h.of("fatal").length, 0);
});

test("record identity ignores invalid and duplicate IDs and preserves the physical busy slot", async () => {
  const store = controlledRecordStore({ holdTransaction: true }), h = await recordWorker(store);
  for (const id of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, NaN]) await h.send({ kind: "records-request", id, action: "list" });
  assert.equal(store.opens.length, 0); assert.equal(h.of("records-reply").length, 0);
  await h.send({ kind: "records-request", id: 10, action: "list" });
  await h.send({ kind: "records-request", id: 10, action: "list" });
  assert.equal(store.calls.length, 1); assert.equal(h.of("records-reply").length, 0);
  await h.send({ kind: "records-request", id: 11, action: "remove", recordId: 17 });
  assert.equal(store.calls.length, 1); assert.equal(h.of("records-reply").at(-1).id, 11);
  assert.equal(h.of("records-reply").at(-1).result, null);
  store.transactions[0].complete(); await flushJobs();
  assert.equal(h.of("records-reply").at(-1).id, 10); assert.equal(h.of("records-reply").at(-1).error, null);
  await h.send({ kind: "records-request", id: 10, action: "list" });
  await h.send({ kind: "records-request", id: 9, action: "list" });
  assert.equal(store.calls.length, 1, "used/lower IDs cannot repeat storage");
  await h.send({ kind: "records-request", id: 11, action: "remove", recordId: 17 });
  assert.equal(store.calls.length, 2, "a busy refusal did not consume the later physical ID");
  store.transactions[1].complete(); await flushJobs();
  await h.send({ kind: "records-close", id: 12 });
});

test("record close during each uncancelable save read fences mutation and retains capacity until settlement", async () => {
  for (const stage of ["replay", "archive"]) {
    const store = controlledRecordStore(), h = await recordWorker(store), held = deferred();
    const value = recordSaveValue(); let replayReads = 0, archiveReads = 0;
    value.replayFile.arrayBuffer = () => { replayReads++; return stage === "replay" ? held.promise : Promise.resolve(Uint8Array.of(66,75,82,0).buffer); };
    value.archiveBlob.arrayBuffer = () => { archiveReads++; return stage === "archive" ? held.promise : Promise.resolve(Uint8Array.of(66,75,82,69,83,85,76,84).buffer); };
    await h.send({ kind: "records-request", id: 1, action: "save", value });
    assert.equal(replayReads, 1); assert.equal(archiveReads, stage === "archive" ? 1 : 0);
    await h.send({ kind: "records-close", id: 2 });
    assert.equal(h.of("records-closed").length, 0, "caller fencing does not terminate a File read");
    await h.send({ kind: "records-request", id: 3, action: "list" });
    assert.equal(store.calls.length, 0, "replacement cannot bypass the outstanding body read");
    held.resolve((stage === "replay" ? Uint8Array.of(66,75,82,0) : Uint8Array.of(66,75,82,69,83,85,76,84)).buffer);
    await flushJobs();
    assert.equal(store.calls.filter(row => row.action === "save").length, 0);
    assert.equal(archiveReads, stage === "archive" ? 1 : 0, "canceled replay cannot trigger archive acquisition");
    assert.equal(h.of("records-reply").some(row => row.id === 1 && row.error === null), false);
    assert.deepEqual(structuredClone(h.of("records-closed").at(-1)), { kind: "records-closed", id: 2, error: null });
    await h.send({ kind: "records-request", id: 3, action: "list" });
    assert.equal(h.of("records-reply").at(-1).error, null, "explicit fresh retry after actual settlement succeeds");
    await h.send({ kind: "records-close", id: 4 });
  }
});

test("record close joins delayed native open and transaction events instead of canceled caller promises", async () => {
  for (const phase of ["open", "transaction"]) {
    const store = controlledRecordStore({ holdOpen: phase === "open", holdTransaction: phase === "transaction" });
    const h = await recordWorker(store);
    await h.send({ kind: "records-request", id: 1, action: "list" });
    await h.send({ kind: "records-close", id: 2 });
    await h.send({ kind: "records-close", id: 2 });
    assert.equal(store.opens[0].closes, 1, "duplicate close starts no extra physical cleanup");
    assert.equal(h.of("records-closed").length, 0);
    await h.send({ kind: "records-close", id: 3 });
    const competing = h.of("records-closed").find(row => row.id === 3);
    assert.equal(typeof competing?.error, "string");
    await h.send({ kind: "records-request", id: 4, action: "list" });
    assert.equal(store.opens.length, 1);
    if (phase === "open") store.opens[0].finishOpen(); else store.transactions[0].abort();
    await flushJobs();
    const closed = h.of("records-closed").filter(row => row.id === 2);
    assert.ok(closed.length >= 1); assert.ok(closed.every(row => row.error === null));
    assert.equal(h.of("records-reply").some(row => row.id === 1 && row.error === null), false);
    const closes = store.opens[0].closes;
    await h.send({ kind: "records-close", id: 1 }); assert.equal(store.opens[0].closes, closes);
  }
});

test("record cleanup timeout reports failure and keeps the physical owner until a late native terminal event", async () => {
  for (const phase of ["open", "transaction"]) {
    const store = controlledRecordStore({ holdOpen: phase === "open", holdTransaction: phase === "transaction" });
    const h = await recordWorker(store);
    await h.send({ kind: "records-request", id: 1, action: "list" });
    await h.send({ kind: "records-close", id: 2 });
    for (let n = 0; n < 8 && h.of("records-closed").length === 0; n++) {
      const timer = [...h.timers].find(([, entry]) => entry.ms === 10000);
      assert.ok(timer, "bounded native cleanup deadline exists");
      h.timers.delete(timer[0]); timer[1].fn(); await flushJobs();
    }
    const outcome = h.of("records-closed").find(row => row.id === 2);
    assert.ok(outcome && typeof outcome.error === "string" && outcome.error.length > 0 && outcome.error.length <= 4096);
    await h.send({ kind: "records-request", id: 3, action: "list" });
    assert.equal(store.opens.length, 1, "timeout is not native terminal evidence");
    assert.equal(store.calls.length, phase === "open" ? 0 : 1);
    if (phase === "open") store.opens[0].finishOpen(); else store.transactions[0].abort();
    await flushJobs();
    assert.equal(h.of("records-closed").some(row => row.id === 2 && row.error === null), false,
      "late cleanup must not rewrite the earlier failed outcome");
    await h.send({ kind: "records-request", id: 3, action: "list" });
    assert.equal(store.opens.length, 2, "fresh explicit recovery follows actual native termination");
    if (phase === "open") store.opens[1].finishOpen(); else store.transactions.at(-1).complete();
    await flushJobs();
    assert.equal(h.of("records-reply").at(-1).error, null);
    await h.send({ kind: "records-close", id: 4 });
  }
});

test("record save snapshots all bounded metadata before an awaited immutable body read", async () => {
  const store = controlledRecordStore(), h = await recordWorker(store), held = deferred();
  const value = recordSaveValue(); value.replayFile.arrayBuffer = () => held.promise;
  await h.send({ kind: "records-request", id: 1, action: "save", value });
  value.metadata.name = "Mutated after admission"; value.metadata.hits = 123n;
  value.metadata.chartPath = "wrong/chart.bms"; value.archivePlayer = 7; value.player = 7; value.players.reverse();
  held.resolve(Uint8Array.of(66,75,82,0).buffer); await flushJobs();
  await settleRecord(() => store.calls.some(row => row.action === "save"));
  const saved = store.calls.find(row => row.action === "save")?.value;
  assert.ok(saved); assert.equal(saved.name, "Original member"); assert.equal(saved.hits, 17n);
  assert.equal(saved.chartPath, "song/chart.bms"); assert.equal(saved.archivePlayer, 0xffffffff);
  await h.send({ kind: "records-close", id: 2 });
});

test("record save acquisition rejects changed or failed bodies without reaching persistent mutation", async () => {
  for (const stage of ["replay", "archive"]) for (const failure of ["wrong-size", "reject"]) {
    const store = controlledRecordStore(), h = await recordWorker(store), value = recordSaveValue();
    const handle = stage === "replay" ? value.replayFile : value.archiveBlob;
    handle.arrayBuffer = () => failure === "reject" ? Promise.reject(new Error("body acquisition refused")) : Promise.resolve(Uint8Array.of(1).buffer);
    await h.send({ kind: "records-request", id: 1, action: "save", value });
    await settleRecord(() => h.of("records-reply").some(row => row.id === 1));
    assert.equal(store.calls.filter(row => row.action === "save").length, 0);
    assert.equal(h.of("records-reply").at(-1).result, null); assert.ok(h.of("records-reply").at(-1).error);
    assert.equal(h.of("fatal").length, 0);
    await h.send({ kind: "records-request", id: 2, action: "list" });
    assert.equal(h.of("records-reply").at(-1).error, null, "optional acquisition errors permit explicit recovery");
    await h.send({ kind: "records-close", id: 3 });
  }
});

test("retired record body read reserves storage capacity while unrelated gameplay preparation remains available", async () => {
  const store = controlledRecordStore(), h = await gameWorker({ recordsStore: store }), held = deferred();
  const value = recordSaveValue(); value.replayFile.arrayBuffer = () => held.promise;
  await h.send({ kind: "records-request", id: 1, action: "save", value });
  await h.send({ kind: "records-close", id: 2 });
  assert.equal(h.of("records-closed").length, 0);
  await startGame(h, [], { recordReplay: true });
  assert.equal(h.of("play-reply").at(-1).result.kind, "prepared");
  assert.equal(h.games.length, 1); assert.equal(h.games[0].frees, 0);
  await h.send({ kind: "records-request", id: 3, action: "list" });
  assert.equal(store.calls.length, 0); assert.equal(h.of("records-reply").at(-1).result, null);
  held.resolve(Uint8Array.of(66,75,82,0).buffer); await flushJobs();
  assert.equal(store.calls.filter(row => row.action === "save").length, 0);
  assert.equal(h.of("records-reply").some(row => row.id === 1 && row.error === null), false);
  assert.equal(h.of("records-closed").at(-1).error, null); assert.equal(h.games[0].frees, 0);
  await h.send({ kind: "play-stop", playId: 1 });
  const capture = h.of("play-stopped").at(-1); assert.ok(capture.replay instanceof FileType);
  assert.deepEqual(Array.from(new Uint8Array(await capture.replay.arrayBuffer())), [66,75,82]);
  assert.equal(h.games[0].frees, 1);
});

test("record disposal and fatal entry wait for actual native transaction terminal and suppress late UI", async () => {
  for (const boundary of ["dispose", "fatal"]) {
    const store = controlledRecordStore({ holdTransaction: true }), h = await recordWorker(store);
    await h.send({ kind: "records-request", id: 1, action: "list" });
    await h.send(boundary === "dispose" ? { kind: "dispose" } : null);
    assert.equal(store.opens[0].closes, 1, "record owner fenced at entry");
    assert.equal(h.of(boundary === "dispose" ? "disposed" : "fatal").length, 0);
    assert.equal(h.of("records-reply").some(row => row.id === 1 && row.error === null), false);
    store.transactions[0].abort(); await flushJobs();
    assert.equal(h.of(boundary === "dispose" ? "disposed" : "fatal").length, 1);
    assert.equal(h.of("records-reply").some(row => row.id === 1 && row.error === null), false);
  }
});

test("missing native storage terminal reports disposal failure and cannot publish a successful disposed receipt", async () => {
  const store = controlledRecordStore({ holdTransaction: true }), h = await recordWorker(store);
  await h.send({ kind: "records-request", id: 1, action: "list" });
  await h.send({ kind: "dispose" });
  assert.equal(h.of("disposed").length, 0); assert.equal(store.opens[0].closes, 1);
  for (let n = 0; n < 8 && h.of("dispose-error").length === 0; n++) {
    const timer = [...h.timers].find(([, entry]) => entry.ms === 10000);
    assert.ok(timer, "disposal storage cleanup has a finite deadline");
    h.timers.delete(timer[0]); timer[1].fn(); await flushJobs();
  }
  const failure = h.of("dispose-error").at(-1);
  assert.ok(failure); assert.equal(typeof failure.message, "string");
  assert.ok(failure.message.length > 0 && failure.message.length <= 4096);
  assert.equal(h.of("disposed").length, 0, "unknown native cleanup never becomes successful release");
  store.transactions[0].abort(); await flushJobs();
  assert.equal(h.of("disposed").length, 0, "late terminal event cannot rewrite the completed failed receipt");
});

test("close at an interrupted save commit preserves unknown outcome without success or automatic write retry", async () => {
  const store = controlledRecordStore({ holdTransaction: true }), h = await recordWorker(store);
  await h.send({ kind: "records-request", id: 1, action: "save", value: recordSaveValue() });
  await settleRecord(() => store.calls.some(row => row.action === "save"));
  assert.equal(store.calls.filter(row => row.action === "save").length, 1);
  await h.send({ kind: "records-close", id: 2 });
  assert.equal(h.of("records-closed").length, 0);
  store.transactions[0].complete(); await flushJobs();
  assert.equal(h.of("records-reply").some(row => row.id === 1 && row.error === null), false);
  assert.equal(store.calls.filter(row => row.action === "save").length, 1, "no inferred rollback or automatic retry");
  assert.equal(h.of("records-closed").at(-1).error, null, "native transaction completion proves cleanup, not the retired UI operation");
});

test("records are optional before CPU readiness and during active gameplay", async () => {
  const store = controlledRecordStore(), early = await realPreparationWorker({ recordsStore: store, skipInit: true });
  await early.send({ kind: "records-request", id: 1, action: "list" });
  assert.equal(store.opens.length, 0); assert.equal(early.of("records-reply").at(-1).result, null);
  assert.equal(early.of("fatal").length, 0); await early.init(); assert.equal(early.of("ready").length, 1);
  const activeStore = controlledRecordStore(), h = await gameWorker({ recordsStore: activeStore });
  await startGame(h, []);
  await h.send({ kind: "records-request", id: 1, action: "list" });
  assert.equal(activeStore.opens.length, 0); assert.equal(h.of("records-reply").at(-1).result, null);
  assert.equal(h.of("fatal").length, 0); assert.equal(h.games[0].frees, 0);
  await h.send({ kind: "records-close", id: 2 }); assert.equal(h.games[0].frees, 0, "hidden storage close preserves gameplay owner");
  await h.send({ kind: "play-stop", playId: 1 }); assert.equal(h.games[0].frees, 1);
});

// Explicit task delivery, separate from Promise continuations. Closed-port
// queued messages remain observable so tests can exercise a late delivery.
function controlledChannels(channels, queue, auto = false) {
  return class {
    constructor() {
      const state = { delivered: 0, port1: null, port2: null }; channels.push(state);
      const port = () => ({ onmessage: null, closes: 0, start() {}, close() { this.closes++; } });
      this.port1 = state.port1 = port(); this.port2 = state.port2 = port();
      this.port2.postMessage = data => {
        assert.equal(this.port2.closes, 0, "no post on closed preparation port");
        const deliver = () => { if (this.port1.onmessage) { state.delivered++; this.port1.onmessage({ data }); } };
        if (auto) setImmediate(deliver); else queue.push(deliver);
      };
    }
  };
}

// Independent preparation acceptance uses genuine current generated bindings.
// The queued renderer endpoint acknowledges packets; it never manufactures PCM
// preparation, job progress, replay identity or engine readiness.
async function realPreparationWorker(options = {}) {
  const messages = [], transfers = [], libraries = [], jobs = [], accesses = [], timers = new Map(), channels = [], tasks = [];
  let receive, nextTimer = 0;
  const self = {
    isSecureContext: false, navigator: {}, performance: { timeOrigin: 0, now: () => 1000 },
    addEventListener(kind, fn) { assert.equal(kind, "message"); receive = fn; },
    postMessage(value, transfer = []) { options.beforePost?.(value, transfer); messages.push(value); transfers.push(transfer); },
  };
  const audioWasm = options.compile ? { Module: WebAssembly.Module, Instance: WebAssembly.Instance,
    Memory: WebAssembly.Memory, Table: WebAssembly.Table, Global: WebAssembly.Global,
    instantiate: WebAssembly.instantiate, compile: options.compile } : WebAssembly;
  const context = createContext({ self, performance: self.performance, WebAssembly: audioWasm, TextEncoder, TextDecoder,
    Uint8Array, Uint32Array, Float32Array, Float64Array, ArrayBuffer, DataView, File: FileType, Blob, queueMicrotask,
    URL, Request, Response, structuredClone, console, AbortController, fetch: options.fetch,
    MessageChannel: options.realChannels ? class {
      constructor() {
        const channel = new NodeMessageChannel(); channels.push(channel);
        for (const port of [channel.port1, channel.port2]) {
          const close = port.close.bind(port); port.closes = 0;
          port.close = () => { port.closes++; close(); };
        }
        return channel;
      }
    } : controlledChannels(channels, tasks),
    setTimeout(fn, ms = 0) { const id = ++nextTimer; timers.set(id, { fn, ms }); return id; },
    clearTimeout(id) { timers.delete(id); } });
  const url = new URL("./pkg/beatkernel_bms_runtime.js", import.meta.url);
  const binding = new SourceTextModule(await readFile(url, "utf8"), { context, identifier: url.href,
    initializeImportMeta(meta) { meta.url = url.href; } });
  await binding.link(name => { throw Error("unexpected generated import " + name); }); await binding.evaluate();
  context.actualPreparationWasm = new WebAssembly.Module(await readFile(new URL("./pkg/beatkernel_bms_runtime_bg.wasm", import.meta.url)));
  binding.namespace.initSync(runInContext("({ module: actualPreparationWasm })", context));
  assert.equal(typeof binding.namespace.BrowserLibrary.prototype.begin_prepare_chart, "function", "fresh generated job API is mandatory");
  assert.equal(typeof binding.namespace.BrowserSample.prototype.take_pcm_array, "function", "fresh direct export API is mandatory");
  function tracked(target, type, state) {
    return new Proxy(target, { get(object, key) {
      assert.equal(state.frees, 0, `no ${type}.${String(key)} lookup after free`);
      const value = Reflect.get(object, key, object);
      if (typeof value !== "function") return value;
      return (...args) => {
        assert.equal(state.frees, 0, `no ${type}.${String(key)} call after free`);
        accesses.push({ type, key: String(key), state });
        if (key === "free") state.frees++;
        if (key === "cancel") state.cancels++;
        const result = value.apply(object, args);
        if (String(key).startsWith("begin_prepare_")) {
          const job = { frees: 0, cancels: 0, advances: 0, stages: [], method: String(key) }; jobs.push(job);
          return tracked(result, "job", job);
        }
        if (type === "job" && key === "advance") { state.advances++; state.stages.push(object.stage); state.ready = result; }
        if (type === "job" && key === "pending_asset_path") state.pendingPath = result;
        return result;
      };
    } });
  }
  const runtime = new SyntheticModule(Object.getOwnPropertyNames(binding.namespace), function () {
    for (const name of Object.getOwnPropertyNames(binding.namespace)) this.setExport(name, name === "default" ? async () => {} : binding.namespace[name]);
    this.setExport("BrowserLibrary", class {
      constructor(...args) { const state = { frees: 0, cancels: 0 }; libraries.push(state); return tracked(new binding.namespace.BrowserLibrary(...args), "library", state); }
    });
  }, { context });
  const modules = new Map();
  async function moduleFor(specifier, parent) {
    if (specifier.endsWith("/pkg/beatkernel_bms_runtime.js") || specifier === "./pkg/beatkernel_bms_runtime.js") return runtime;
    const target = new URL(specifier, parent.identifier);
    if (modules.has(target.href)) return modules.get(target.href);
    const module = new SourceTextModule(await readFile(target, "utf8"), { context, identifier: target.href,
      initializeImportMeta(meta) { meta.url = target.href; } }); modules.set(target.href, module);
    return module;
  }
  const workerUrl = new URL("./worker.js", import.meta.url);
  const observation = options.observeReads ? `
const __originalSelectChart = selectChart;
const __observedSelections = [];
selectChart = function(request) {
  const entry = { id: request.id, settled: false }; __observedSelections.push(entry);
  return __originalSelectChart(request).then(value => { entry.settled = true; return value; }, error => { entry.settled = true; throw error; });
};
export function __actualReadDiagnostics() {
  return { selections: __observedSelections.map(entry => ({ ...entry })), owner: preparationOwner !== null,
    pending: Array.from(libraryContext?.pending ?? [], ([path, record]) => ({ path, waiters: record.waiters?.size ?? -1 })) };
}
export function __actualPendingMap() { return libraryContext?.pending; }
` : "";
  const worker = new SourceTextModule(await readFile(workerUrl, "utf8") + observation, { context, identifier: workerUrl.href,
    initializeImportMeta(meta) { meta.url = workerUrl.href; } });
  modules.set(workerUrl.href, worker); await worker.link(moduleFor); await worker.evaluate();
  if (options.recordsStore) modules.get(new URL("./record-store.mjs", workerUrl).href).namespace.RecordsStore.openTracked = options.recordsStore.openTracked;
  const port = { onmessage: null, starts: 0, closes: 0,
    start() { this.starts++; }, close() { this.closes++; },
    postMessage(request) {
      const base = { generation: request.generation, content: request.content, operationId: request.operationId };
      const packet = request.packet;
      const reply = packet ? (() => { const h = new DataView(packet.buffer, packet.byteOffset, packet.byteLength); return { ...base,
        kind: "state-ack", packetKind: h.getUint16(6, true), sequence: h.getBigUint64(24, true) }; })()
        : { ...base, kind: "control-ack", operation: request.kind, geometryVersion: request.geometryVersion };
      Promise.resolve().then(() => this.onmessage?.({ data: reply }));
    } };
  const h = {
    messages, transfers, libraries, jobs, accesses, timers, channels, tasks, runtime: binding.namespace,
    async init() { await h.send({ kind: "init", renderPort: port, maxPacketBytes: 16 * 1024 * 1024, maxDiagnosticBytes: 4096, renderTimeoutMs: 10000 }); },
    of(kind) { return messages.filter(x => x.kind === kind); },
    async send(value) { receive({ data: value }); await flushJobs(); },
    async yieldOne() {
      if (options.realChannels) await new Promise(resolve => setImmediate(resolve));
      else { assert(tasks.length, "expected genuine MessageChannel task yield"); tasks.shift()(); }
      await flushJobs();
    },
    async settle(predicate) { for (let n = 0; n < 256 && !predicate(); n++) await this.yieldOne(); assert(predicate(), "bounded genuine job must settle"); },
    diagnostics() { assert(options.observeReads); return worker.namespace.__actualReadDiagnostics(); },
    pendingMap() { assert(options.observeReads); return worker.namespace.__actualPendingMap(); },
    assertClosedChannels() {
      for (const channel of channels) for (const port of [channel.port1, channel.port2]) assert.equal(port.closes, 1);
    },
    heldRead() {
      // Same-realm promise: Promise.resolve in production returns this exact
      // underlying File promise, so its actual settlement subscriptions count.
      return runInContext(`(() => {
        const state = { subscriptions: 0 }; let resolve, reject;
        const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
        const original = promise.then;
        promise.then = function(...args) { state.subscriptions++; return original.apply(this, args); };
        return { promise, resolve, reject, state };
      })()`, context);
    },
  };
  if (!options.skipInit) { await h.init(); assert.equal(h.of("ready").length, 1); }
  return h;
}
function preparationWave() {
  const b = new Uint8Array(48), v = new DataView(b.buffer); b.set(new TextEncoder().encode("RIFF")); v.setUint32(4, 40, true);
  b.set(new TextEncoder().encode("WAVEfmt "), 8); v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
  v.setUint32(24, 24000, true); v.setUint32(28, 48000, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
  b.set(new TextEncoder().encode("data"), 36); v.setUint32(40, 4, true); v.setInt16(44, 8192, true); v.setInt16(46, -4096, true); return b;
}
const REAL_PREPARATION_CHART = "#TITLE Actual phased fixture\n#BPM 120\n#RANK 2\n#WAV01 key.wav\n#00011:00010000\n";
function byteFile(path, bytes, read) {
  const file = new FileType([bytes], path.split("/").at(-1));
  file.arrayBuffer = read ?? (() => Promise.resolve(bytes.slice().buffer)); return { file, path };
}
async function admitPreparationFiles(h, extra = [], readWave) {
  await h.send({ kind: "import", id: 1, files: [selectedFile("chart.bms", REAL_PREPARATION_CHART), byteFile("key.wav", preparationWave(), readWave), ...extra] });
  assert.equal(h.of("catalog").length, 1); await h.send({ kind: "accept-library", id: 1 });
}
const actualSelect = (id = 2, path = "chart.bms") => ({ kind: "select", id, libraryId: 1, path, rate: 24000, seed: "0" });

const AUDIO_FIXTURE_BYTES = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]);
const AUDIO_SETUP_LIMIT = 64 * 1024 * 1024;
async function audioSetupWorker(options = {}) {
  const fetches = [], compiles = [], compilePromises = [], readers = [];
  const chunks = options.chunks ?? [AUDIO_FIXTURE_BYTES.subarray(0, 3), new Uint8Array(0), AUDIO_FIXTURE_BYTES.subarray(3)];
  const makeReader = () => ({ index: 0, cancels: 0, releases: 0,
    async read() { return this.index < chunks.length ? { done: false, value: chunks[this.index++] } : { done: true }; },
    cancel() { this.cancels++; return options.cancelPromise ?? Promise.resolve(); },
    releaseLock() { this.releases++; } });
  const reader = makeReader();
  const h = await realPreparationWorker({ ...options,
    fetch: (url, init) => {
      fetches.push({ url, init });
      if (options.fetch) return options.fetch(url, init);
      const responseReader = fetches.length === 1 ? reader : makeReader();
      return Promise.resolve({ ok: true, body: { getReader() { readers.push(responseReader); return responseReader; } } });
    },
    compile: bytes => { compiles.push(new Uint8Array(bytes.buffer ?? bytes, bytes.byteOffset ?? 0, bytes.byteLength));
      const promise = options.compile ? options.compile(bytes) : WebAssembly.compile(bytes);
      compilePromises.push(promise); return promise; } });
  const send = h.send.bind(h);
  return Object.assign(h, { fetches, compiles, readers, reader,
    async send(value) { await send(value); for (let turn = 0; turn < 8; turn++) { await new Promise(resolve => setImmediate(resolve)); await flushJobs(); } },
    async audioReady(id) {
      assert(compilePromises.length, "actual compile has started before waiting for its completion");
      let timeout;
      try { await Promise.race([compilePromises.at(-1), new Promise((_, reject) => {
        timeout = setTimeout(() => reject(Error("actual audio compile did not settle within five seconds")), 5000);
      })]); } finally { clearTimeout(timeout); }
      await flushJobs(); assert(h.of("audio-module-ready").some(reply => reply.id === id), "settled actual compile must publish its current id");
    },
    async deadline() { const entry = [...h.timers].find(([, value]) => value.ms === 10000);
      assert(entry, "audio setup has its ten-second deadline"); h.timers.delete(entry[0]); entry[1].fn(); await flushJobs(); } });
}
function assertAudioError(h, id) {
  const reply = h.of("audio-module-error").find(row => row.id === id);
  assert(reply, `optional audio error belongs to ${id}`);
  assert.equal(typeof reply.message, "string"); assert(reply.message.length > 0 && reply.message.length <= 4096);
  assert.equal(h.of("fatal").length, 0, "optional audio failure leaves business CPU usable");
  return reply;
}

test("audio setup actual WASM CPU readiness precedes preparation and invalid identities never perform IO", async () => {
  const h = await audioSetupWorker({ skipInit: true });
  await h.send({ kind: "audio-load", id: 1 }); assert.equal(h.fetches.length, 0); assert.equal(h.compiles.length, 0);
  await h.init(); assert.equal(h.of("ready").length, 1);
  for (const id of [undefined, null, 0, -1, 1.5, "1", NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) await h.send({ kind: "audio-load", id });
  assert.equal(h.fetches.length, 0); assert.equal(h.compiles.length, 0); assert.equal(h.of("audio-module-ready").length, 0);
  await admitPreparationFiles(h); await h.send(actualSelect()); await h.settle(() => h.of("selected").length === 1);
  assert.equal(h.of("fatal").length, 0); await h.send({ kind: "dispose" });
});

test("audio setup compiles exactly admitted bytes at the fixed package URL and clones a genuine cached Module", async () => {
  const h = await audioSetupWorker(); await h.send({ kind: "audio-load", id: 11 }); await h.audioReady(11);
  assert.equal(h.fetches.length, 1);
  assert.equal(String(h.fetches[0].url), new URL("./audio-pkg/beatkernel_bms_runtime_bg.wasm", new URL("./worker.js", import.meta.url)).href);
  assert(h.fetches[0].init.signal instanceof AbortSignal); assert.equal(h.fetches[0].init.signal.aborted, false);
  assert.equal(h.compiles.length, 1); assert.deepEqual(h.compiles[0], AUDIO_FIXTURE_BYTES);
  const ready = h.of("audio-module-ready"); assert.equal(ready.length, 1); assert.equal(ready[0].id, 11);
  assert(ready[0].module instanceof WebAssembly.Module);
  assert.deepEqual(Array.from(h.transfers[h.messages.indexOf(ready[0])]), [], "compiled Module never enters transfer list");
  const cloned = structuredClone(ready[0]); assert(cloned.module instanceof WebAssembly.Module); new WebAssembly.Instance(cloned.module);
  assert.equal(h.reader.cancels, 0, "fully consumed body must not be canceled after completion");
  assert.equal(h.timers.size, 0); await h.send({ kind: "audio-load", id: 12 });
  assert.equal(h.of("audio-module-ready").at(-1).id, 12); assert.equal(h.of("audio-module-ready").at(-1).module, ready[0].module);
  assert.equal(h.fetches.length, 1); assert.equal(h.compiles.length, 1); await h.send({ kind: "dispose" });
});

test("audio setup identical active id is ignored and competing id cannot replace physical single flight", async () => {
  const held = deferred(), h = await audioSetupWorker({ fetch: () => held.promise });
  await h.send({ kind: "audio-load", id: 21 }); await h.send({ kind: "audio-load", id: 21 }); await h.send({ kind: "audio-load", id: 22 });
  assert.equal(h.fetches.length, 1); assert.equal(h.compiles.length, 0); assertAudioError(h, 22);
  assert.equal(h.of("audio-module-error").filter(row => row.id === 21).length, 0);
  held.resolve({ ok: true, body: { getReader: () => h.reader } }); await flushJobs(); await h.audioReady(21);
  assert.deepEqual(h.of("audio-module-ready").map(row => row.id), [21]); await h.send({ kind: "dispose" });
});

test("audio setup rejects HTTP, body, reader, read, malformed chunk and compile failures without fatal", async () => {
  const cases = [
    { fetch: async () => { throw Error("fetch denied"); } },
    { fetch: async () => ({ ok: false, body: {} }) },
    { fetch: async () => ({ ok: true, body: null }) },
    { fetch: async () => ({ ok: true, body: { getReader() { throw Error("reader denied"); } } }) },
    { fetch: async () => ({ ok: true, body: { getReader: () => ({ read: async () => { throw Error("read denied"); }, cancel() {}, releaseLock() {} }) } }) },
    ...[null, [0, 97], new Uint16Array([1]), new ArrayBuffer(8)].map(value => ({ chunks: [value] })),
    { chunks: [new Uint8Array(0)] },
    { compile: async () => { throw Error("x".repeat(10000)); } },
  ];
  for (let index = 0; index < cases.length; index++) {
    const h = await audioSetupWorker(cases[index]), id = 30 + index; await h.send({ kind: "audio-load", id });
    assertAudioError(h, id); assert.equal(h.of("audio-module-ready").length, 0); assert.equal(h.timers.size, 0);
    await admitPreparationFiles(h); assert.equal(h.of("catalog").length, 1); await h.send({ kind: "dispose" });
  }
});

test("audio setup admits exactly 64 MiB and refuses overflow before inspecting or copying the excess chunk", async () => {
  const module = await WebAssembly.compile(AUDIO_FIXTURE_BYTES);
  for (const overflow of [false, true]) {
    const full = new Uint8Array(AUDIO_SETUP_LIMIT); full[0] = 19; full[full.length - 1] = 37;
    const extra = new Proxy(new Uint8Array(1), { get(target, key) {
      if (key === "byteLength") return 1;
      assert.fail(`overflow chunk must not be copied or inspected through ${String(key)}`);
    } });
    const h = await audioSetupWorker({ chunks: overflow ? [full, extra] : [full], compile: async () => module });
    await h.send({ kind: "audio-load", id: 50 });
    if (overflow) { assertAudioError(h, 50); assert.equal(h.compiles.length, 0); }
    else { assert.equal(h.compiles.length, 1); assert.equal(h.compiles[0].length, AUDIO_SETUP_LIMIT);
      assert.equal(h.compiles[0][0], 19); assert.equal(h.compiles[0].at(-1), 37); assert.equal(h.of("audio-module-ready")[0].module, module); }
    await h.send({ kind: "dispose" });
  }
});

test("audio setup failure permits explicit fresh-id retry without an automatic retry", async () => {
  let first = true; const h = await audioSetupWorker({ compile: async bytes => { if (first) { first = false; throw Error("first compile failed"); } return WebAssembly.compile(bytes); } });
  await h.send({ kind: "audio-load", id: 60 }); assertAudioError(h, 60); assert.equal(h.fetches.length, 1);
  await flushJobs(); assert.equal(h.fetches.length, 1);
  await h.send({ kind: "audio-load", id: 60 }); assert.equal(h.fetches.length, 1); assert.equal(h.compiles.length, 1);
  await h.send({ kind: "audio-load", id: 61 }); await h.audioReady(61);
  assert.equal(h.fetches.length, 2); assert.equal(h.compiles.length, 2); assert.equal(h.of("audio-module-ready").at(-1).id, 61);
  await h.send({ kind: "dispose" });
});

test("audio setup publication clone failure retires the deadline and reports bounded optional failure", async () => {
  const h = await audioSetupWorker({ beforePost(value) { if (value.kind === "audio-module-ready") throw new DOMException("cannot clone Module", "DataCloneError"); } });
  await h.send({ kind: "audio-load", id: 70 }); assertAudioError(h, 70);
  assert.equal(h.of("audio-module-ready").length, 0); assert.equal(h.timers.size, 0); await h.send({ kind: "dispose" });
});

test("audio setup fetch and read deadline retires publication without awaiting never-settling cancellation", async () => {
  for (const stage of ["fetch", "read"]) {
    const held = deferred(), never = deferred();
    const h = await audioSetupWorker({ cancelPromise: never.promise,
      ...(stage === "fetch" ? { fetch: () => held.promise } : {}) });
    if (stage === "read") h.reader.read = () => held.promise;
    await h.send({ kind: "audio-load", id: 80 }); await h.deadline();
    assertAudioError(h, 80); assert.equal(h.fetches[0].init.signal.aborted, true); assert.equal(h.timers.size, 0);
    await h.send({ kind: "audio-load", id: 81 }); assertAudioError(h, 81); assert.equal(h.fetches.length, 1);
    if (stage === "fetch") held.resolve({ ok: true, body: { getReader: () => h.reader } }); else held.resolve({ done: true });
    await flushJobs(); assert.equal(h.of("audio-module-ready").length, 0);
    assert.equal(h.of("audio-module-error").filter(row => row.id === 80).length, 1); await h.send({ kind: "dispose" });
    assert.equal(h.of("disposed").length, 1, "never-settling cancel cannot hold shutdown");
  }
});

test("audio setup compile timeout keeps physical capacity until settlement and never caches late success", async () => {
  const held = deferred(); let first = true;
  const h = await audioSetupWorker({ compile: bytes => { if (first) { first = false; return held.promise; } return WebAssembly.compile(bytes); } });
  await h.send({ kind: "audio-load", id: 90 }); assert.equal(h.compiles.length, 1); await h.deadline(); assertAudioError(h, 90);
  await h.send({ kind: "audio-load", id: 91 }); assertAudioError(h, 91); assert.equal(h.compiles.length, 1); assert.equal(h.fetches.length, 1);
  held.resolve(await WebAssembly.compile(AUDIO_FIXTURE_BYTES)); await flushJobs(); assert.equal(h.of("audio-module-ready").length, 0);
  await h.send({ kind: "audio-load", id: 92 }); await h.audioReady(92); assert.equal(h.fetches.length, 2); assert.equal(h.compiles.length, 2);
  assert.deepEqual(h.of("audio-module-ready").map(row => row.id), [92]); await h.send({ kind: "dispose" });
});

test("audio setup dispose and fatal entry abort pending fetch and suppress late success or optional failure", async () => {
  for (const terminal of ["dispose", "fatal"]) for (const reject of [false, true]) {
    const held = deferred(), h = await audioSetupWorker({ fetch: () => held.promise }); await h.send({ kind: "audio-load", id: 100 });
    if (terminal === "dispose") await h.send({ kind: "dispose" }); else await h.init();
    assert.equal(h.fetches[0].init.signal.aborted, true); assert.equal(h.timers.size, 0);
    if (reject) held.reject(Error("late obsolete fetch")); else held.resolve({ ok: true, body: { getReader: () => h.reader } });
    await flushJobs(); assert.equal(h.of("audio-module-ready").length, 0); assert.equal(h.of("audio-module-error").length, 0);
    assert.equal(h.compiles.length, 0); if (terminal === "fatal") assert.equal(h.of("fatal").length, 1);
  }
});

test("audio setup dispose and fatal suppress uncancelable compile completion without awaiting reader cancellation", async () => {
  for (const terminal of ["dispose", "fatal"]) for (const reject of [false, true]) {
    const held = deferred(), never = deferred(), h = await audioSetupWorker({ compile: () => held.promise, cancelPromise: never.promise });
    await h.send({ kind: "audio-load", id: 110 }); assert.equal(h.compiles.length, 1);
    if (terminal === "dispose") await h.send({ kind: "dispose" }); else await h.init();
    assert.equal(h.fetches[0].init.signal.aborted, true); assert.equal(h.timers.size, 0);
    if (reject) held.reject(Error("late obsolete compile")); else held.resolve(await WebAssembly.compile(AUDIO_FIXTURE_BYTES));
    await flushJobs(); assert.equal(h.of("audio-module-ready").length, 0); assert.equal(h.of("audio-module-error").length, 0);
  }
});

test("audio setup retires at fatal or refused disposal entry before actual active gameplay capture is joined", async () => {
  for (const terminal of ["fatal", "dispose"]) for (const stage of ["fetch", "compile"]) {
    const held = deferred(); let h, abortedAtCapture = false;
    h = await audioSetupWorker({ ...(stage === "fetch" ? { fetch: () => held.promise } : { compile: () => held.promise }),
      beforePost(value) { if (value.kind === "play-error" || value.kind === "play-stopped") {
        abortedAtCapture = h.fetches[0].init.signal.aborted;
      } } });
    await admitPreparationFiles(h);
    await h.send({ kind: "play-start", playId: 1, rpcId: 1, libraryId: 1, path: "chart.bms", rate: 24000, seed: "0",
      keyPairs: new Uint32Array([0x11, 4]), earlyNs: 50000000n, lateNs: 50000000n, offsetNs: 0n,
      windowOriginNs: 0n, recordReplay: true });
    await h.settle(() => h.of("play-reply").length > 0 || h.of("play-error").length > 0);
    assert.equal(h.of("play-error").length, 0); assert.equal(h.of("play-reply").at(-1).result.kind, "prepared");
    await h.send({ kind: "audio-load", id: 120 });
    if (stage === "compile") assert.equal(h.compiles.length, 1);
    if (terminal === "fatal") await h.init();
    else {
      await h.send({ kind: "dispose" }); assert.equal(h.of("dispose-error").length, 1);
      assert.equal(h.of("disposed").length, 0, "existing gameplay disposal refusal is preserved");
      await h.send({ kind: "play-stop", playId: 1, rpcId: 2 });
    }
    assert.equal(h.fetches[0].init.signal.aborted, true); assert.equal(abortedAtCapture, true);
    const capture = h.of(terminal === "fatal" ? "play-error" : "play-stopped").at(-1);
    assert(capture.replay instanceof FileType); assert(capture.replay.size > 0, "actual Rust capture is retained");
    if (terminal === "fatal") {
      assert.equal(h.of("fatal").length, 1);
      assert(h.messages.indexOf(capture) < h.messages.indexOf(h.of("fatal")[0]), "existing capture-before-fatal ordering is preserved");
    }
    if (stage === "fetch") held.resolve({ ok: true, body: { getReader: () => h.reader } });
    else held.resolve(await WebAssembly.compile(AUDIO_FIXTURE_BYTES));
    await flushJobs(); assert.equal(h.of("audio-module-ready").length, 0); assert.equal(h.of("audio-module-error").length, 0);
    assert.equal(h.timers.size, 0); await h.send({ kind: "dispose" });
  }
});

test("actual WASM phased preparation yields macrotasks and reports only correlated real progress before Ready", async () => {
  const h = await realPreparationWorker(); await admitPreparationFiles(h);
  await h.send(actualSelect());
  assert.equal(h.of("selected").length, 0);
  await h.settle(() => h.of("selected").length > 0);
  const progress = h.of("selection-progress"); assert(progress.length > 2);
  for (const p of progress) {
    assert.equal(p.id, 2); assert.equal(p.libraryId, 1); assert.equal(p.path, "chart.bms");
    assert.equal(typeof p.stage, "string"); assert(p.completedAssets <= p.totalAssets);
    for (const value of [p.parses, p.compiles, p.reads, p.decodes, p.sharedAssets]) assert(Number.isInteger(value) && value >= 0);
    assert(Number.isFinite(p.maxStepMs) && p.maxStepMs >= 0);
  }
  assert(progress.some(p => p.decodes === 1), "actual codec counter, not a invented percentage");
  assert(h.jobs[0].advances > 2); assert.equal(h.jobs[0].frees, 1);
  assert.equal(h.of("selected")[0].title, "Actual phased fixture");
  await h.send({ kind: "dispose" }); assert.equal(h.libraries[0].frees, 1);
  assert.equal(h.channels.length, 1, "one channel per multi-phase preparation"); h.assertClosedChannels();
});

test("actual queued Ready admission remains cancellable and closes both preparation ports", async () => {
  for (const transition of ["cancel", "retire"]) {
    const h = await realPreparationWorker(); await admitPreparationFiles(h); await h.send(actualSelect());
    await h.settle(() => h.jobs[0]?.ready === true);
    assert.equal(h.of("selected").length, 0, "Ready still requires its queued task boundary");
    assert.equal(h.channels.length, 1); assert.equal(h.tasks.length, 1, "only one outstanding wake");
    if (transition === "cancel") await h.send({ kind: "select-cancel", id: 2, libraryId: 1 });
    else { await h.send({ kind: "import", id: 4, files: [selectedFile("new.bms", "#BPM 120\n")] }); await h.send({ kind: "accept-library", id: 4 }); }
    h.assertClosedChannels(); const calls = h.accesses.length, delivered = h.channels[0].delivered;
    await h.yieldOne(); assert.equal(h.accesses.length, calls); assert.equal(h.channels[0].delivered, delivered);
    assert.equal(h.of("selected").length, 0); assert.equal(h.of("selection-error").length, 0);
    assert.equal(h.jobs[0].cancels, 1); assert.equal(h.jobs[0].frees, 1);
    await h.send({ kind: "dispose" }); h.assertClosedChannels();
  }
});

test("actual preparation executes real Node MessageChannel task API and always closes ports", async () => {
  const h = await realPreparationWorker({ realChannels: true });
  try {
    await admitPreparationFiles(h); await h.send(actualSelect());
    assert.equal(h.of("selected").length, 0);
    await h.settle(() => h.of("selected").length === 1);
    assert.equal(h.channels.length, 1); h.assertClosedChannels();
    const calls = h.accesses.length; await h.yieldOne(); assert.equal(h.accesses.length, calls);
  } finally {
    await h.send({ kind: "dispose" });
    // Even a failed assertion must not leave real Node ports holding the process.
    for (const channel of h.channels) for (const port of [channel.port1, channel.port2]) if (!port.closes) port.close();
  }
});

test("actual preview Cancel during hydration permanently fences late success/error and keeps the old preview", async () => {
  for (const failure of [false, true]) {
    const held = deferred(), h = await realPreparationWorker(); let readStarted = false;
    await admitPreparationFiles(h, [selectedFile("pending.bms", REAL_PREPARATION_CHART.replace("key.wav", "late.wav")), byteFile("late.wav", preparationWave(), () => { readStarted = true; return held.promise; })]);
    await h.send(actualSelect()); await h.settle(() => h.of("selected").length === 1);
    await h.send(actualSelect(3, "pending.bms"));
    await h.settle(() => readStarted);
    await h.send({ kind: "select-cancel", id: 999, libraryId: 1 }); assert.equal(h.of("selection-cancelled").length, 0);
    await h.send({ kind: "select-cancel", id: 3, libraryId: 1 });
    assert.deepEqual(h.of("selection-cancelled").map(x => [x.id, x.libraryId]), [[3, 1]]);
    const count = h.accesses.length;
    if (failure) held.reject(Error("obsolete read error")); else held.resolve(preparationWave().buffer);
    await flushJobs(); assert.equal(h.accesses.length, count, "no WASM admission after permanent cancellation");
    assert.equal(h.of("selected").length, 1); assert.equal(h.of("selection-error").length, 0);
    await h.send({ kind: "select-cancel", id: 3, libraryId: 1 }); assert.equal(h.of("selection-cancelled").length, 1);
    await h.send({ kind: "dispose" }); assert.equal(h.libraries[0].frees, 1);
  }
});

test("actual preview Cancel at phase yield releases its job exactly once and genuine retry recovers", async () => {
  const h = await realPreparationWorker(); await admitPreparationFiles(h); await h.send(actualSelect());
  for (let n = 0; n < 128 && !(h.jobs[0]?.advances > 0); n++) await h.yieldOne();
  const job = h.jobs[0]; assert(job); await h.send({ kind: "select-cancel", id: 2, libraryId: 1 });
  assert.equal(job.cancels, 1); assert.equal(job.frees, 1);
  const calls = h.accesses.length; await flushJobs(); assert.equal(h.accesses.length, calls);
  assert.equal(h.channels.length, 1); h.assertClosedChannels();
  if (h.tasks.length) { const delivered = h.channels[0].delivered; await h.yieldOne(); assert.equal(h.channels[0].delivered, delivered); assert.equal(h.accesses.length, calls); }
  await h.send(actualSelect(3)); await h.settle(() => h.of("selected").length === 1);
  assert.equal(h.of("selected")[0].id, 3); assert.equal(h.of("selection-error").length, 0);
  assert.equal(h.channels.length, 2); h.assertClosedChannels();
  await h.send({ kind: "dispose" });
});

test("actual retired library and completed settings ownership cannot revive a pending selection", async () => {
  for (const transition of ["import", "settings", "newer-select"]) {
    const held = deferred(), h = await realPreparationWorker(); let readStarted = false;
    await admitPreparationFiles(h, [], () => { readStarted = true; return held.promise; }); await h.send(actualSelect());
    await h.settle(() => readStarted);
    if (transition === "import") {
      await h.send({ kind: "import", id: 4, files: [selectedFile("new.bms", "#BPM 120\n")] });
      await h.send({ kind: "accept-library", id: 4 }); assert.equal(h.libraries[0].frees, 1);
    } else if (transition === "settings") {
      await h.send({ kind: "settings-profile-save", id: 4, settings: JSON.parse(validSettingsProfileText()) });
      assert.equal(h.of("settings-profile-saved").length, 1, JSON.stringify(h.of("settings-profile-error")));
    } else {
      await h.send(actualSelect(4));
      await h.settle(() => h.jobs[1]?.pendingPath === "key.wav");
    }
    const calls = h.accesses.length; held.reject(Error("late obsolete hydration")); await flushJobs();
    if (transition !== "newer-select") assert.equal(h.accesses.length, calls);
    assert.equal(h.of("selected").length, 0);
    assert.equal(h.of("selection-error").filter(x => x.id === 2).length, 0);
    if (transition === "newer-select") assert.equal(h.of("selection-error").filter(x => x.id === 4).length, 1);
    await h.send({ kind: "dispose" }); for (const library of h.libraries) assert.equal(library.frees, 1);
  }
});

test("actual live local section and policy routes all use jobs and retain direct sample export", async () => {
  const modes = [
    {}, { startNs: 1n },
    { startNs: 1n, timingPolicy: { presetId: "beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1", rankPrecedence: "rank-first", gauge: "groove" } },
    { inputMode: "physical", localPlanWords: new Uint32Array([7, 0, 0, 0]), localPage: 0 },
  ];
  for (const extra of modes) {
    const h = await realPreparationWorker(); await admitPreparationFiles(h);
    await h.send({ kind: "play-start", playId: 1, rpcId: 1, libraryId: 1, path: "chart.bms", rate: 24000, seed: "0",
      keyPairs: new Uint32Array([0x11, 4]), earlyNs: 50000000n, lateNs: 50000000n, offsetNs: 0n, windowOriginNs: 0n, ...extra });
    await h.settle(() => h.of("play-reply").length > 0 || h.of("play-error").length > 0);
    assert.deepEqual(h.of("play-error"), []); assert.equal(h.of("play-reply").at(-1).result.kind, "prepared");
    assert(h.jobs[0].advances > 2); assert(h.of("play-preparation-progress").every(p => p.playId === 1 && p.libraryId === 1));
    assert.equal(h.jobs[0].frees, 1);
    if (!Object.hasOwn(extra, "startNs")) {
      await h.send({ kind: "play-sample", playId: 1, rpcId: 2 });
      const reply = h.of("play-reply").at(-1), exported = reply.result;
      assert.equal(exported.kind, "sample"); assert(exported.pcm instanceof Float32Array);
      assert.deepEqual(Array.from(exported.pcm), [0.25, 0.25, -0.125, -0.125]);
      assert(h.transfers[h.messages.indexOf(reply)].includes(exported.pcm.buffer), "direct JS PCM buffer is the actual transfer owner");
      const received = structuredClone(exported.pcm, { transfer: [exported.pcm.buffer] });
      assert.equal(exported.pcm.buffer.byteLength, 0); assert.equal(received[0], 0.25);
    }
    await h.send({ kind: "play-stop", playId: 1, rpcId: 3 }); await h.send({ kind: "dispose" });
  }
});

test("actual replay preparation validates a genuine captured SectionSetup and uses the recorded seed", async () => {
  const h = await realPreparationWorker(), runtime = h.runtime;
  const owner = new runtime.BrowserLibrary(8, 1024 * 1024, 2 * 1024 * 1024, 64);
  owner.add_file("chart.bms", new TextEncoder().encode(REAL_PREPARATION_CHART)); owner.add_file("key.wav", preparationWave());
  let bytes;
  try {
    const prepared = owner.prepare_chart("chart.bms", 24000, 2, 3n, 1024, 4096, 8);
    const game = runtime.BrowserGame.new_physical(prepared, 0n, 100000000n, 50000000n, 50000000n, 0n,
      keyboardBindingWords(new Uint32Array([0x11, 4])), undefined, 4096, 1024);
    try { game.configure_capture(1024 * 1024, 1024); game.stop(); bytes = game.take_replay(); assert(bytes.byteLength > 0); }
    finally { game.free(); }
  } finally { owner.free(); }
  await admitPreparationFiles(h);
  await h.send({ kind: "play-start", mode: "replay", playId: 1, rpcId: 1, libraryId: 1, path: "chart.bms", rate: 24000,
    seed: "invalid live seed must not replace recording", replayFile: new FileType([bytes], "actual.bkr"), windowOriginNs: 0n });
  await h.settle(() => h.of("play-reply").length > 0 || h.of("play-error").length > 0);
  assert.deepEqual(h.of("play-error"), []); assert.equal(h.of("play-reply").at(-1).result.kind, "prepared");
  assert.equal(h.jobs[0].method, "begin_prepare_replay_chart"); assert.equal(h.jobs[0].frees, 1);
  await h.send({ kind: "play-stop", playId: 1, rpcId: 2 }); await h.send({ kind: "dispose" });
});

test("actual play start and Stop fence the prior preview and pending play jobs before late hydration", async () => {
  const h = await realPreparationWorker(), held = deferred(); let readStarted = false;
  await admitPreparationFiles(h, [], () => { readStarted = true; return held.promise; });
  await h.send(actualSelect()); await h.settle(() => readStarted);
  await h.send({ kind: "play-start", playId: 1, rpcId: 1, libraryId: 1, path: "chart.bms", rate: 24000,
    seed: "0", keyPairs: new Uint32Array([0x11, 4]), earlyNs: 50000000n, lateNs: 50000000n, offsetNs: 0n, windowOriginNs: 0n });
  await h.send({ kind: "play-stop", playId: 1, rpcId: 2 });
  const calls = h.accesses.length; held.resolve(preparationWave().buffer); await flushJobs();
  assert.equal(h.accesses.length, calls, "late prior-preview/play bytes must not reach a freed or stopped job");
  assert.equal(h.of("selected").length, 0); assert.equal(h.of("selection-error").length, 0);
  assert(!h.of("play-reply").some(x => x.result?.kind === "prepared"));
  for (const job of h.jobs) { assert.equal(job.frees, 1); assert.equal(job.cancels, 1); }
  await h.send({ kind: "dispose" });
});

test("one held File read retains at most one waiter and every cancelled runner settles before acquisition", async () => {
  for (const activeFinal of [false, true]) {
    const h = await realPreparationWorker({ observeReads: true }), held = h.heldRead();
    let fileCalls = 0;
    await admitPreparationFiles(h, [], () => { fileCalls++; return held.promise; });
    for (let id = 2; id < 8; id++) {
      await h.send(actualSelect(id));
      await h.settle(() => h.diagnostics().pending.some(p => p.path === "key.wav" && p.waiters === 1));
      assert.equal(fileCalls, 1, "each repeated request shares the still-pending original File acquisition");
      assert.equal(held.state.subscriptions, 1, "one actual underlying settlement subscription, not one Promise.race per cancelled request");
      assert(h.diagnostics().pending.every(p => p.waiters <= 1));
      await h.send({ kind: "select-cancel", id, libraryId: 1 });
      const state = h.diagnostics();
      assert.equal(state.owner, false);
      assert(state.pending.every(p => p.waiters === 0), "Cancel synchronously removes its only retained callback");
      assert(state.selections.every(entry => entry.settled), "actual selectChart finally settles while File is still permanently held");
      assert.equal(held.state.subscriptions, 1);
    }
    if (activeFinal) {
      await h.send(actualSelect(8));
      await h.settle(() => h.diagnostics().pending.some(p => p.path === "key.wav" && p.waiters === 1));
      assert.equal(fileCalls, 1); assert.equal(held.state.subscriptions, 1);
    }
    const calls = h.accesses.length;
    held.reject(Error("original held File failed after repeated cancellation")); await flushJobs();
    assert.equal(h.accesses.length, calls + (activeFinal ? 2 : 0), "only the active final job may release; retired jobs do not revisit WASM");
    assert.equal(h.diagnostics().pending.length, 0); assert(h.diagnostics().selections.every(x => x.settled));
    assert.equal(h.of("selected").length, 0); assert.equal(h.of("fatal").length, 0);
    assert.deepEqual(h.of("selection-error").map(x => x.id), activeFinal ? [8] : []);
    for (const job of h.jobs) { assert.equal(job.cancels, 1); assert.equal(job.frees, 1); }
    await h.send({ kind: "dispose" }); assert.equal(h.libraries[0].frees, 1);
  }
});

test("actual importer retirement removes held-read waiters and settles the retired runner before late bytes", async () => {
  const h = await realPreparationWorker({ observeReads: true }), held = h.heldRead(); let fileCalls = 0;
  await admitPreparationFiles(h, [], () => { fileCalls++; return held.promise; });
  await h.send(actualSelect()); await h.settle(() => h.diagnostics().pending.some(p => p.waiters === 1));
  const originalMap = h.pendingMap(), originalRecord = originalMap.get("key.wav");
  await h.send({ kind: "import", id: 4, files: [selectedFile("new.bms", "#BPM 120\n")] });
  await h.send({ kind: "accept-library", id: 4 });
  assert.equal(originalRecord.waiters.size, 0); assert.equal(originalMap.size, 0);
  assert(h.diagnostics().selections.every(x => x.settled)); assert.equal(h.libraries[0].frees, 1);
  assert.equal(fileCalls, 1); assert.equal(held.state.subscriptions, 1);
  const calls = h.accesses.length;
  held.resolve(preparationWave().buffer); await flushJobs();
  assert.equal(h.accesses.length, calls, "late original bytes cannot call the freed WASM library or replacement owner");
  assert.equal(h.of("selected").length, 0); assert.equal(h.of("selection-error").length, 0); assert.equal(h.of("fatal").length, 0);
  await h.send({ kind: "dispose" }); for (const library of h.libraries) assert.equal(library.frees, 1);
});

let actualMenuBinding;
function realMenuOwner() {
  return actualMenuBinding ??= (async () => {
    const url = new URL("./pkg/beatkernel_bms_runtime.js", import.meta.url);
    const context = createContext({ WebAssembly, TextEncoder, TextDecoder, Uint8Array,
      ArrayBuffer, DataView, URL, Request, Response, console });
    const module = new SourceTextModule(await readFile(url, "utf8"), { context,
      identifier: url.href, initializeImportMeta(meta) { meta.url = url.href; } });
    await module.link(specifier => { throw new Error(`unexpected menu binding import ${specifier}`); });
    await module.evaluate();
    context.testWasmModule = new WebAssembly.Module(await readFile(new URL("./pkg/beatkernel_bms_runtime_bg.wasm", import.meta.url)));
    module.namespace.initSync(runInContext("({ module: testWasmModule })", context));
    assert.equal(typeof module.namespace.BrowserMenuOwner, "function");
    return module.namespace.BrowserMenuOwner;
  })();
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function flushJobs() {
  // Advance only promise jobs; Worker presentation timers are entirely fake.
  for (let index = 0; index < 32; index++) await Promise.resolve();
}

function selectedFile(path, content = "#BPM 120", acquire) {
  const bytes = new TextEncoder().encode(content);
  const file = new FileType([bytes], path.split("/").at(-1));
  file.arrayBuffer = acquire ?? (() => Promise.resolve(bytes.buffer));
  return { file, path };
}

async function workerHarness(options = {}) {
  async function flushJobs() {
    // Older mock cases auto-deliver genuine task boundaries, while the real
    // binding harness above controls each MessageChannel delivery explicitly.
    for (let turn = 0; turn < 8; turn++) {
      await new Promise(resolve => setImmediate(resolve));
      for (let n = 0; n < 32; n++) await Promise.resolve();
    }
  }
  const messages = [];

  // Genuine queued port and real RenderClient; only the WASM encoder/GPU edge
  // is mocked. The map records immutable exporter snapshots for assertions.
  const visualExports = [], visualAcks = [], visualOwners = new Map();
  function visualPacket(owner, kind, generation, content, sequence = 0n, page = 0, songNs) {
    assert.equal(typeof generation, "bigint"); assert.ok(generation > 0n);
    assert.equal(typeof content, "bigint"); assert.ok(content > 0n);
    assert.equal(typeof sequence, "bigint");
    const packet = new Uint8Array(40); packet.set([66,75,82,86]);
    const h = new DataView(packet.buffer); h.setUint16(4,1,true); h.setUint16(6,kind,true);
    h.setBigUint64(8,generation,true); h.setBigUint64(16,content,true);
    h.setBigUint64(24,sequence,true); h.setBigUint64(32,0n,true);
    const snapshot = {owner,kind,generation,content,sequence,page:kind===5 ? 0 : kind>=4 ? owner.page : page,comparisons:kind===5 ? false : undefined,songNs};
    visualOwners.set(`${generation}:${content}:${sequence}:${kind}`,snapshot);
    visualExports.push(snapshot); return packet;
  }
  function installVisualProducer(target, kind = 1) {
    target.visual_registration = function(g,c,limit,diagnostics) {
      assert.equal(this.frees,0); assert.ok(limit >= 40); assert.ok(diagnostics >= 0);
      this.visualContext={g,c}; this.visualPending=null; this.visualBaseline=0n;
      return visualPacket(this,1,g,c);
    };
    target.visual_frame = function(sequence,page) {
      assert.equal(this.frees,0); assert.equal(this.visualPending,null,"one frozen snapshot until exact ACK");
      assert.ok(sequence > this.visualBaseline); this.visualPending=sequence;
      return visualPacket(this,2,this.visualContext.g,this.visualContext.c,sequence,page);
    };
    target.preview_state = function(sequence,songNs) {
      assert.equal(this.frees,0); assert.equal(this.visualPending,null);
      this.visualPending=sequence;
      return visualPacket(this,3,this.visualContext.g,this.visualContext.c,sequence,0,songNs);
    };
    target.acknowledge_visual = function(g,c,sequence) {
      assert.equal(this.frees,0);
      if(g!==this.visualContext.g||c!==this.visualContext.c||sequence!==this.visualPending) return false;
      this.visualBaseline=sequence; this.visualPending=null;
      visualAcks.push({owner:this,generation:g,content:c,sequence}); return true;
    };
    if(kind!==1) target.visual_snapshot = function(g,c,limit,diagnostics) {
      assert.equal(this.frees,0); assert.ok(limit>=40); assert.ok(diagnostics>=0);
      return visualPacket(this,kind,g,c);
    };
  }
  const renderPort = {
    comparisons:false, width:0, height:0, page:0, presentations:new Map(), posts:[], starts:0, closes:0, blocked:options.renderBlocked??false, onmessage:null,onmessageerror:null,
    start(){this.starts++;}, close(){this.closes++;},
    emit(reply){Promise.resolve().then(()=>this.onmessage?.({data:structuredClone(reply)}));},
    ack(request,fields={}) {
      const base={generation:request.generation,content:request.content,operationId:request.operationId};
      const h=request.kind==="packet"?new DataView(request.packet.buffer):null;
      this.emit({...base,...(h?{kind:"state-ack",packetKind:h.getUint16(6,true),sequence:h.getBigUint64(24,true)}
        :{kind:"control-ack",operation:request.kind,geometryVersion:request.geometryVersion}),...fields});
    },
    postMessage(request,transfer=[]) {
      const message=structuredClone(request,{transfer}); this.posts.push(message);
      Promise.resolve().then(()=>{
        if(this.blocked)return;
        if (message.kind === "menu") {
          const header = preflightMenuPacket(message.packet);
          this.emit({ kind: "menu-ack", operationId: message.operationId,
            generation: message.generation, content: message.content,
            menuGeneration: header.generation, screen: header.screen, revision: header.revision });
          return;
        }
        const view=views[0];
        if(message.kind==="packet") {
          const h=new DataView(message.packet.buffer),kind=h.getUint16(6,true),sequence=h.getBigUint64(24,true);
          const s=visualOwners.get(`${message.generation}:${message.content}:${sequence}:${kind}`);
          assert.ok(s,"actual valid BKRV exporter packet crossed the boundary");
          if(kind!==6)this.page=s.page;
          if(kind===5)this.comparisons=false;
          if(kind===6&&options.roomResultsDrawError){this.emit({kind:"render-error",generation:message.generation,content:message.content,message:options.roomResultsDrawError});return;}
          if(kind>=4){
            const existing=this.presentations.get(message.generation);
            if(kind===6&&existing?.kind===5)existing.room={...s};
            else this.presentations.set(message.generation,{...s,drawable:kind!==5});
          }
          const mode=this.posts.find(p=>p.kind==="packet"&&p.generation===message.generation&&p.mode)?.mode;
          if(kind===1&&mode==="preview") {
            if(view.current&&view.current!==s.owner)view.current.releasedByView=true;
            view.current=s.owner; view.replacements?.push(s.owner);
          }else if(kind===3){view.positions.push(s.songNs);view.draws++;}
          else if(kind===2&&mode==="local")view.localDraws?.push({game:s.owner,page:s.page});
          else if(kind===2&&mode==="replay")view.replayDraws?.push(s.owner);
          else if(kind===2){view.gameDraws?.push(s.owner);view.draws++;}
          else if(kind===4){view.historicalDraws??=[];view.historicalDraws.push(s.owner);}
          else if(kind===6&&this.presentations.get(message.generation)?.drawable){view.resultDraws?.push({results:s.owner,page:s.page});}
        }else if(message.kind==="resize"){this.width=message.width;this.height=message.height;view.extents.push([message.width,message.height]);}
        if(["page","room-page"].includes(message.kind)){
          const s=this.presentations.get(message.generation);
          assert.ok(s,"paging follows real frozen registration");
          if(message.kind==="room-page"){
            assert.ok(s.room,"combined footer has its separate frozen room registration");
            s.room.page=message.page;
          }else{
            this.page=message.page;s.page=message.page;
            if(s.kind===5){this.comparisons=message.comparisons;s.comparisons=message.comparisons;s.drawable=true;}
          }
          if(s.kind===4){view.historicalDraws??=[];view.historicalDraws.push(s.owner);}
          else if(s.drawable){view.resultDraws?.push({results:s.owner,page:s.page});}
        }
        this.ack(message);
        const registered=this.presentations.get(message.generation);
        if(message.geometryVersion&&this.width>0&&this.height>0&&!(registered?.kind===5&&!registered.drawable))this.emit({kind:"geometry-ack",generation:message.generation,content:message.content,geometryVersion:message.geometryVersion,page:this.page,width:this.width,height:this.height});
      });
    },
  };
  function renderRequest(request) {
    if(request?.kind==="init")return {...request,canvas:undefined,renderPort:Object.hasOwn(request,"renderPort")?request.renderPort:renderPort,maxPacketBytes:request.maxPacketBytes??1024*1024,maxDiagnosticBytes:request.maxDiagnosticBytes??4096,renderTimeoutMs:request.renderTimeoutMs??60000};
    return request;
  }

  const libraries = [];
  const views = [];
  const games = [];
  const preparedOwners = [];
  const calls = [];
  const timers = new Map();
  let now = 0;
  let timerId = 0;
  let receive;
  function makePrepared(path, start = 0n) {
    const prepared = {
      title: `Prepared ${path}`, artist: "Fixture", duration_ns: 604800000000000n,
      note_count: 3, sample_count: 2, image_count: 1, lanes: [0x11], path, moved: false, frees: 0,
      video_registration() {
        assert.equal(this.moved, false, "movie registration is exported before consuming Prepared");
        assert.equal(this.frees, 0);
        return { resources: [], images: [] };
      },
      free() { assert.equal(this.moved, false); assert.equal(++this.frees, 1); },
    };
    if (!options.omitPreparedStart) prepared.start_ns = Object.hasOwn(options, "preparedStart") ? options.preparedStart : start;
    installVisualProducer(prepared);
    preparedOwners.push(prepared);
    return prepared;
  }
  class BrowserLibrary {
    constructor(...limits) {
      this.limits = limits;
      this.files = [];
      this.declarations = new Map();
      this.plans = [];
      this.accesses = [];
      this.preparations = [];
      this.frees = 0;
      libraries.push(this);
    }
    add_file(path, bytes) {
      assert.equal(this.frees, 0);
      this.accesses.push(["add_file", path]);
      assert.equal(this.declarations.get(path), bytes.byteLength, "hydrate exactly the admitted declared extent");
      assert.ok(!this.files.some(entry => entry.path === path), "hydrate each file once");
      this.files.push({ path, bytes: Array.from(bytes) });
    }
    declare_file(path, length) {
      assert.equal(this.frees, 0);
      assert.ok(!this.declarations.has(path), "declarations are unique after canonical preflight");
      assert.ok(Number.isInteger(length) && length >= 0);
      this.declarations.set(path, length);
    }
    chart_paths() {
      assert.equal(this.frees, 0);
      return [...this.declarations.keys()].filter(path => /\.(bms|bme|bml|pms)$/i.test(path));
    }
    referenced_asset_paths(path, seed, maxSamples) {
      assert.equal(this.frees, 0);
      this.accesses.push(["referenced_asset_paths", path]);
      assert.ok(this.files.some(entry => entry.path === path), "resource planning needs acquired chart bytes");
      this.plans.push({ method: "live", path, seed, maxSamples });
      calls.push(["plan-live", path, seed, maxSamples]);
      return [...(typeof options.references === "function" ? options.references(path, seed) : options.references?.[path] ?? [])];
    }
    replay_referenced_asset_paths(path, bytes, maxSamples) {
      assert.equal(this.frees, 0);
      this.accesses.push(["replay_referenced_asset_paths", path]);
      assert.ok(this.files.some(entry => entry.path === path));
      this.plans.push({ method: "replay", path, bytes: Array.from(bytes), maxSamples });
      calls.push(["plan-replay", path, Array.from(bytes), maxSamples]);
      return [...(options.replayReferences?.[path] ?? [])];
    }
    assertHydrated(path, replay = false) {
      assert.equal(this.frees, 0);
      this.accesses.push(["prepare", path]);
      assert.ok(this.files.some(entry => entry.path === path), "prepare from acquired owning chart");
      const plan = this.plans.findLast(plan => plan.path === path && plan.method === (replay ? "replay" : "live"));
      assert.ok(plan, "preparation follows its resource planner");
      const refs = replay ? options.replayReferences?.[path] ?? []
        : typeof options.references === "function" ? options.references(path, plan.seed) : options.references?.[path] ?? [];
      for (const resource of refs) assert.ok(this.files.some(entry => entry.path === resource), `prepare requires acquired ${resource}`);
    }
    prepare_chart(path, ...args) {
      this.assertHydrated(path);
      this.preparations.push({ path, args });
      if (path === options.rejectPreparation) throw new Error("sample rate mismatch");
      return makePrepared(path);
    }
    prepare_chart_at(path, ...args) {
      this.assertHydrated(path);
      this.preparations.push({ path, args, method: "prepare_chart_at" });
      calls.push(["prepare-section", ...args]);
      return makePrepared(path, args[3]);
    }
    prepare_chart_with_policy_at(path, ...args) {
      this.assertHydrated(path);
      this.preparations.push({ path, args, method: "prepare_chart_with_policy_at" });
      calls.push(["prepare-policy", ...args]);
      return makePrepared(path, args[3]);
    }
    prepare_replay_chart(path, bytes, rate) {
      this.assertHydrated(path, true);
      calls.push(["prepare-replay", Array.from(bytes)]);
      this.preparations.push({ path, args: [rate], method: "prepare_replay_chart" });
      const prepared = makePrepared(path);
      if (!options.omitPreparedStart) prepared.start_ns = options.replayStart ?? 0n;
      return prepared;
    }
    // Compatibility model for existing controller/GPU-edge fixtures only.
    // Genuine phase/PCM acceptance uses realPreparationWorker above.
    preparationJob(method, args, replay = false) {
      const [path] = args;
      if (replay) this.replay_referenced_asset_paths(path, args[1], args.at(-1));
      else this.referenced_asset_paths(path, args[3], args[method === "prepare_chart" ? 6 : 7]);
      const references = this.plans.at(-1).method === "replay" ? options.replayReferences?.[path] ?? []
        : typeof options.references === "function" ? options.references(path, args[3]) : options.references?.[path] ?? [];
      let prepared, freed = false, cancelled = false;
      return {
        stage: "resolve", completed_assets: 0, total_assets: references.length,
        parse_count: 0, compile_count: 0, read_count: 0, decode_count: 0, shared_asset_count: 0,
        pending_asset_path: () => { assert(!freed); return references.find(ref => !this.files.some(x => x.path === ref)); },
        advance: current => {
          assert.equal(current, this); assert(!cancelled && !freed);
          if (references.some(ref => !this.files.some(x => x.path === ref))) return false;
          prepared = this[method](...args); return true;
        },
        take_prepared: current => { assert.equal(current, this); assert(!cancelled && !freed && prepared); const result = prepared; prepared = null; return result; },
        cancel() { assert(!freed); cancelled = true; if (prepared) { prepared.free(); prepared = null; } },
        free() { assert(!freed); freed = true; },
      };
    }
    begin_prepare_chart(...args) { return this.preparationJob("prepare_chart", args); }
    begin_prepare_chart_at(...args) { return this.preparationJob("prepare_chart_at", args); }
    begin_prepare_chart_with_policy_at(...args) { return this.preparationJob("prepare_chart_with_policy_at", args); }
    begin_prepare_replay_chart(...args) { return this.preparationJob("prepare_replay_chart", args, true); }
    free() { this.frees++; assert.equal(this.frees, 1); }
  }
  class BrowserView {
    static async create(canvas) {
      if (options.createError) throw new Error(options.createError);
      const view = new BrowserView();
      view.canvas = canvas;
      views.push(view);
      return view;
    }
    current = null;
    replacements = [];
    extents = [];
    positions = [];
    draws = 0;
    resize(width, height) { this.extents.push([width, height]); }
    set_chart(prepared) {
      assert.equal(prepared.moved, false);
      assert.equal(prepared.frees, 0);
      prepared.moved = true;
      if (this.current) this.current.releasedByView = true;
      this.current = prepared;
      this.replacements.push(prepared);
    }
    seek(ns) { this.positions.push(ns); }
    draw() { this.draws++; }
    draw_game(game) { assert.equal(game.frees, 0); this.draws++; }
    draw_replay(game) { assert.equal(game.frees, 0); this.draws++; }
    needs_redraw() { return options.needsRedraw ?? false; }
  }
  class BrowserGame {
    completed_archive() { return null; }
    completed_results() { return null; }
    constructor(prepared, ...constructorArgs) {
      if (!options.gameplay) throw new Error("Preview fixtures must not create gameplay owners");
      assert.equal(prepared.moved, false);
      prepared.moved = true;
      this.prepared = prepared;
      this.constructorArgs = constructorArgs;
      this.frees = 0;
      this.stops = 0;
      this.added = [];
      this.snapshots = 0;
      this.song_ns = -100000000n;
      this.hits = 17n;
      this.misses = 3n;
      this.combo = 4n;
      this.max_combo = 9n;
      this.recorded_until_ns = 1000000000n;
      this.pendingInput = [];
      this.presentations = [];
      this.closedPrefix = null;
      this.processedInput = [];
      games.push(this);
      calls.push(["new-game"]);
    }
    add_saved_opponent(bytes, own, label) {
      assert.equal(this.frees, 0);
      calls.push(["add-opponent", Array.from(bytes), own, label]);
      if (options.addError) throw new Error(options.addError);
      this.added.push({ bytes: Array.from(bytes), own, label });
      return this.added.length - 1;
    }
    saved_opponents() {
      assert.equal(this.frees, 0);
      this.snapshots++;
      calls.push(["snapshot", this.song_ns]);
      if (options.snapshotError) throw new Error(options.snapshotError);
      return options.snapshot?.(this) ?? [];
    }
    disable_saved_opponent_hud() {
      assert.equal(this.frees, 0);
      calls.push(["disable-opponent-hud"]);
    }
    update_peer_hud() { assert.fail("Preview and solo fixtures must not publish peer HUD state"); }
    disable_peer_hud() { assert.fail("Preview and solo fixtures must not own peer HUD state"); }
    configure_capture(...limits) { calls.push(["capture", ...limits]); }
    sample_count() { return options.sampleCount ?? 0; }
    next_sample() { return undefined; }
    commands() { return null; }
    activate(at) { calls.push(["activate", at]); }
    input(...values) { calls.push(["input", ...values]); }
    advance(host, audio) { calls.push(["advance", host, audio]); this.song_ns = options.songNs ?? host; }
    observe_output() { return false; }
    queue_input(host, key, down, sequence, received) {
      assert.ok(host <= received);
      calls.push(["queue-input", host, key, down, sequence, received]);
      this.pendingInput.push({ host, key, down, sequence });
    }
    close_input_prefix(host) { calls.push(["close-prefix", host]); this.closedPrefix = host; }
    pending_inputs() { return this.pendingInput.length; }
    admit_output(words, presented) { this.outputEvidence = { words, presented }; calls.push(["admit-output", presented]); }
    observe_presentation(output, host) {
      calls.push(["presentation", output, host]);
      const previous = this.presentations.at(-1);
      if (!previous || (output > previous.output && host > previous.host)) this.presentations.push({ output, host });
    }
    service_audio(now, audio) {
      calls.push(["service", now, audio]);
      let count = 0;
      const latest = this.presentations.at(-1);
      if (this.presentations.length >= 2 && now >= latest.host && now - latest.host <= 1000000000n) {
        while (this.pendingInput[0]?.host <= this.closedPrefix && this.pendingInput[0].host <= latest.host) {
          this.processedInput.push(this.pendingInput.shift()); count++;
        }
        if (!this.pendingInput.length && latest.host <= this.closedPrefix) this.song_ns = options.songNs ?? latest.output;
      }
      return count;
    }
    evaluate_completion() { calls.push(["completion"]); return false; }
    stop() { this.stops++; calls.push(["stop"]); }
    take_replay() { calls.push(["take-replay"]); return Uint8Array.from([66, 75, 82]); }
    free() { this.frees++; assert.equal(this.frees, 1); calls.push(["free"]); }
  }
  views.push(new BrowserView());
  installVisualProducer(BrowserGame.prototype, 1);
  class BrowserLocalGame extends BrowserGame {
    static new_physical(prepared, ...args) {
      const game = new BrowserLocalGame(prepared, ...args);
      const plan = args[5];
      game.players = Uint32Array.from(Array.from(plan).filter((_, index) => index % 4 === 0));
      calls.push(["new-local-game", Array.from(game.players)]);
      return game;
    }
    queue_input_blob() { assert.fail("hydration fixtures do not synthesize acquired input"); }
  }
  const self = {
    isSecureContext: false,
    navigator: {},
    performance: { timeOrigin: 0, now: () => now + (options.gameplay ? 1000 : 0) },
    postMessage(value) { messages.push(value); },
    addEventListener(name, callback) {
      assert.equal(name, "message");
      receive = callback;
    },
  };
  const context = createContext({
    self, structuredClone, DataView, File: FileType, Blob, queueMicrotask, TextEncoder, TextDecoder, Uint8Array, Uint32Array, Float32Array, ArrayBuffer,
    MessageChannel: controlledChannels([], [], true),
    performance: self.performance,
    setTimeout(callback, delay) {
      const id = ++timerId; timers.set(id, callback);
      // Old mock preparation cases auto-drain their simulated cooperative yield;
      // realPreparationWorker keeps actual job yields explicit and observable.
      if (delay === 0) Promise.resolve().then(() => { if (timers.has(id)) { timers.delete(id); callback(); } });
      return id;
    },
    clearTimeout(id) { timers.delete(id); },
  });
  const ActualMenuOwner = options.actualMenu ? await realMenuOwner() : null;
  const MenuOwner = options.actualMenu ? class {
    constructor(generation) {
      const owner = new ActualMenuOwner(generation);
      if (options.menuAdmissionFault) {
        for (const method of ["set_fields", "navigate_with_fields"]) {
          const actual = owner[method].bind(owner);
          owner[method] = (...args) => {
            options.menuAdmissionFault(method, owner, args);
            return actual(...args);
          };
        }
      }
      if (options.observeRecordPreview) owner.preview_record = (...args) => {
        options.observeRecordPreview(...args);
        assert.fail("obsolete record acquisition must never reach its WASM admission");
      };
      return owner;
    }
  } : class {
    constructor() { assert.fail("menu business fixtures must opt into the actual generated WASM owner"); }
  };
  const wasm = new SyntheticModule(["default", "BrowserLibrary", "BrowserGame", "BrowserLocalGame", "BrowserReplay", "BrowserMenuOwner"], function () {
    this.setExport("default", async () => {
      if (options.initError) throw new Error(options.initError);
    });
    this.setExport("BrowserLibrary", BrowserLibrary);
    this.setExport("BrowserGame", BrowserGame);
    this.setExport("BrowserLocalGame", BrowserLocalGame);
    this.setExport("BrowserReplay", BrowserGame);
    this.setExport("BrowserMenuOwner", MenuOwner);
  }, { context });
  const network = new SyntheticModule(["BrowserMultiplayerOwner"], function () {
    this.setExport("BrowserMultiplayerOwner", class {
      static open() { throw new Error("Preview fixtures must not open multiplayer connections"); }
    });
  }, { context });
  const room = new SyntheticModule(["BrowserRoomOwner", "ROOM_SESSION_METHODS"], function () {
    this.setExport("ROOM_SESSION_METHODS", ROOM_SESSION_METHODS);
    this.setExport("BrowserRoomOwner", class {
      static open() { throw new Error("Preview fixtures must not open room connections"); }
    });
  }, { context });
  const completedHelpers = new SourceTextModule(await readFile(new URL("./completed-results-model.mjs", import.meta.url), "utf8"), { context });
  const helpers = new SourceTextModule(await readFile(new URL("./host_model.mjs", import.meta.url), "utf8"), { context });
  const recordStore = new SourceTextModule(await readFile(new URL("./record-store.mjs", import.meta.url), "utf8"), { context });
  const playHelpers = new SourceTextModule(await readFile(new URL("./play-model.mjs", import.meta.url), "utf8"), { context });
  const settingsHelpers = new SourceTextModule(await readFile(new URL("./settings-profile.mjs", import.meta.url), "utf8"), { context });
  const displayHelpers = new SourceTextModule(await readFile(new URL("./display-settings.mjs", import.meta.url), "utf8"), { context });
  const opponentHelpers = new SourceTextModule(await readFile(new URL("./saved-opponents.mjs", import.meta.url), "utf8"), { context });
  const physicalHelpers = new SourceTextModule(await readFile(new URL("./physical-input.mjs", import.meta.url), "utf8"), { context });
  const localHelpers = new SourceTextModule(await readFile(new URL("./local-play-model.mjs", import.meta.url), "utf8"), { context });
  const localHostHelpers = new SourceTextModule(await readFile(new URL("./local-play-host.mjs", import.meta.url), "utf8"), { context });
  const hidProfileHelpers = new SourceTextModule(await readFile(new URL("./hid-profile.mjs", import.meta.url), "utf8"), { context });
  const gamepadProfileHelpers = new SourceTextModule(await readFile(new URL("./gamepad-profile.mjs", import.meta.url), "utf8"), { context });
  const pointerProfileHelpers = new SourceTextModule(await readFile(new URL("./pointer-profile.mjs", import.meta.url), "utf8"), { context });
  const commandClient = new SourceTextModule(await readFile(new URL("./audio-command-client.mjs", import.meta.url), "utf8"), { context });
  const audioFailure = new SourceTextModule(await readFile(new URL("./audio-failure.mjs", import.meta.url), "utf8"), { context });
  const sampleClient = new SourceTextModule(await readFile(new URL("./audio-sample-client.mjs", import.meta.url), "utf8"), { context });
  const renderClient = new SourceTextModule(await readFile(new URL("./render-protocol.mjs", import.meta.url), "utf8"), { context });
  const worker = new SourceTextModule(await readFile(new URL("./worker.js", import.meta.url), "utf8"), { context });
  await worker.link(specifier => {
    if (specifier === "./record-store.mjs") return recordStore;
    if (specifier === "./render-protocol.mjs") return renderClient;
    if (specifier === "./pkg/beatkernel_bms_runtime.js") return wasm;
    if (specifier === "./host_model.mjs") return helpers;
    if (specifier === "./completed-results-model.mjs") return completedHelpers;
    if (specifier === "./play-model.mjs") return playHelpers;
    if (specifier === "./settings-profile.mjs") return settingsHelpers;
    if (specifier === "./display-settings.mjs") return displayHelpers;
    if (specifier === "./multiplayer-owner.mjs") return network;
    if (specifier === "./room-owner.mjs") return room;
    if (specifier === "./saved-opponents.mjs") return opponentHelpers;
    if (specifier === "./physical-input.mjs") return physicalHelpers;
    if (specifier === "./local-play-model.mjs") return localHelpers;
    if (specifier === "./local-play-host.mjs") return localHostHelpers;
    if (specifier === "./hid-profile.mjs") return hidProfileHelpers;
    if (specifier === "./gamepad-profile.mjs") return gamepadProfileHelpers;
    if (specifier === "./pointer-profile.mjs") return pointerProfileHelpers;
    if (specifier === "./audio-command-client.mjs") return commandClient;
    if (specifier === "./audio-failure.mjs") return audioFailure;
    if (specifier === "./audio-sample-client.mjs") return sampleClient;
    throw new Error(`Unexpected Worker import: ${specifier}`);
  });
  await worker.evaluate();
  if (options.recordsStore) recordStore.namespace.RecordsStore.openTracked = options.recordsStore.openTracked;
  return {
    renderPort, visualExports, visualAcks, messages, libraries, views, timers, games, preparedOwners, calls,
    flushTasks: flushJobs,
    setNow(value) { assert.ok(value >= now); now = value; },
    async send(request) {
      request = renderRequest(request);
      if (request.kind === "play-step" && !Object.hasOwn(request, "nowNs")) {
        request = { ...request, nowNs: request.watermark };
      }
      receive({ data: request }); await flushJobs();
    },
    async tick() {
      const next = timers.entries().next().value;
      assert.ok(next, "expected a scheduled presentation callback");
      timers.delete(next[0]);
      next[1]();
      await flushJobs();
    },
    of(kind) { return messages.filter(message => message.kind === kind); },
  };
}

async function readyWorker(options) {
  const worker = await workerHarness(options);
  await worker.send({ kind: "init", canvas: { transferred: true } });
  assert.equal(worker.of("ready").length, 1);
  assert.equal(worker.of("fatal").length, 0);
  return worker;
}

const acquiredMenuToken = state => ({ menuGeneration: state.menuGeneration, screen: state.screen, revision: state.revision });
const settingsMenuDraft = () => ["73", "12.345678", "-0.000001", "balanced", "10.000001", "044100",
  "65536", "4096", "4096", "4096", "65536", "1.000000001", "2.000000002"];
function validSettingsProfileText() {
  return JSON.stringify({ kind: "beatkernel-browser-settings", version: 1,
    timing: { earlyMs: "50", lateMs: "50", offsetMs: "0" },
    output: { latency: "interactive", latencyMs: "20", rate: "48000" },
    capacities: { queueCapacity: "256", maxVoices: "64", pendingCapacity: "256", maxFrames: "2048", maxCommandsPerRender: "256" },
    section: { startSeconds: "0", endSeconds: "" }, bindings: KEY_BINDINGS.map(([lane, code]) => [lane, code]) });
}

// Literal wire oracle: keep this independent of settingsMenuFields so a wrong
// ordered-index mapping cannot make both the implementation and test agree.
const retainedPolicyId = "beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1";
const completeSettingsDraft = () => [...settingsMenuDraft(), retainedPolicyId, "defexrank-first", "groove"];
const bindingCodes = () => ["KeyZ", "KeyS", "KeyX", "KeyD", "KeyC", "ShiftLeft", "Space", "KeyF", "KeyV",
  "KeyN", "KeyJ", "KeyM", "KeyK", "Comma", "ShiftRight", "Slash", "KeyL", "Period"];
const bindingSettingsDraft = () => [...completeSettingsDraft(), ...bindingCodes()];
const importedBindingCodes = () => ["KeyA", "KeyS", "KeyX", "KeyD", "KeyC", "ShiftLeft", "Space", "KeyF", "KeyV",
  "KeyN", "KeyJ", "KeyM", "KeyK", "Comma", "ShiftRight", "Slash", "KeyL", ""];
const importedSettingsFields = () => ["50", "50", "0", "interactive", "20", "48000", "256", "64", "256", "2048", "256", "3", "9", retainedPolicyId, "defexrank-first", "groove", ...importedBindingCodes()];
function retainedProfile() {
  const dto = JSON.parse(validSettingsProfileText());
  dto.bindings.sort((a, b) => a[0] - b[0]);
  dto.bindings[0][1] = "KeyA";
  dto.bindings[17][1] = "";
  Object.assign(dto.timing, { presetId: retainedPolicyId, rankPrecedence: "defexrank-first", gauge: "groove" });
  dto.section = { startSeconds: "3", endSeconds: "9" };
  return dto;
}
async function retainedSettingsWorker(child = false, bindings = true) {
  const h = await readyWorker({ actualMenu: true });
  await h.send({ kind: "menu-open", fields: ["chart.bms"] });
  await h.send({ kind: "menu-navigate", ...acquiredMenuToken(h.of("menu-state").at(-1)), route: 2, fields: bindings ? bindingSettingsDraft() : completeSettingsDraft() });
  const parent = h.of("menu-state").at(-1);
  if (child) await h.send({ kind: "menu-navigate", ...acquiredMenuToken(parent), route: 3, fields: ["1.000000001", "2.000000002"] });
  return { h, parent, initial: h.of("menu-state").at(-1) };
}
function assertProfileTransition(state, previous, id, pending) {
  assert.equal(state.settingsOperationId, id);
  assert.deepEqual(acquiredMenuToken(state.previousToken), acquiredMenuToken(previous));
  assert.equal(state.screen, previous.screen);
  assert.equal(state.menuGeneration, previous.menuGeneration);
  assert.equal(state.revision, previous.revision + 1n);
  assert.equal(state.pending, pending);
  assert.equal(state.hasSettingsParent, true);
}

test("retained policy complete Apply publishes sixteen exact strings and rejects late policy atomically", async () => {
  const { h } = await retainedSettingsWorker(false, false);
  try {
    for (const [index, invalid] of [[13, "unknown-preset"], [14, "rank-last"], [15, "unknown-gauge"]]) {
      await h.send({ kind: "menu-edit", ...acquiredMenuToken(h.of("menu-state").at(-1)), index, value: invalid });
      const before = h.of("menu-state").at(-1), errors = h.of("menu-error").length;
      await h.send({ kind: "menu-action", ...acquiredMenuToken(before), actionId: 1n, control: 10n });
      assert.equal(h.of("menu-error").length, errors + 1);
      assert.equal(h.of("menu-effect").length, 0);
      assert.deepEqual(h.of("menu-state").at(-1), before, "late policy refusal preserves owner, draft and action ID");
      await h.send({ kind: "menu-edit", ...acquiredMenuToken(before), index, value: completeSettingsDraft()[index] });
    }
    await h.send({ kind: "menu-action", ...acquiredMenuToken(h.of("menu-state").at(-1)), actionId: 1n, control: 10n });
    assert.equal(h.of("menu-effect").length, 1);
    assert.deepEqual(Array.from(h.of("menu-effect")[0].fields), completeSettingsDraft());
    assert.deepEqual(acquiredMenuToken(h.of("menu-effect")[0]), acquiredMenuToken(h.of("menu-state").at(-1)));
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained policy native-only actions explicitly refuse without consuming action identity", async () => {
  const { h, initial } = await retainedSettingsWorker(false, false);
  try {
    for (const control of [12n, 15n]) {
      const errors = h.of("menu-error").length;
      await h.send({ kind: "menu-action", ...acquiredMenuToken(initial), actionId: 1n, control });
      assert.equal(h.of("menu-error").length, errors + 1);
      assert.equal(h.of("menu-effect").length, 0);
      assert.deepEqual(h.of("menu-state").at(-1), initial);
    }
    await h.send({ kind: "menu-action", ...acquiredMenuToken(initial), actionId: 1n, control: 10n });
    assert.equal(h.of("menu-effect").length, 1);
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained policy partial and unknown Apply counts refuse while thirteen fields retain omission", async () => {
  const { h } = await retainedSettingsWorker(false, false);
  try {
    for (const count of [14, 15, 17]) {
      const fields = [...completeSettingsDraft(), "extra"].slice(0, count);
      await h.send({ kind: "menu-fields", ...acquiredMenuToken(h.of("menu-state").at(-1)), fields });
      const before = h.of("menu-state").at(-1), errors = h.of("menu-error").length;
      await h.send({ kind: "menu-action", ...acquiredMenuToken(before), control: 10n, actionId: 1n });
      assert.equal(h.of("menu-error").length, errors + 1);
      assert.equal(h.of("menu-effect").length, 0); assert.deepEqual(h.of("menu-state").at(-1), before);
    }
    await h.send({ kind: "menu-fields", ...acquiredMenuToken(h.of("menu-state").at(-1)), fields: settingsMenuDraft() });
    await h.send({ kind: "menu-action", ...acquiredMenuToken(h.of("menu-state").at(-1)), control: 10n, actionId: 1n });
    assert.deepEqual(Array.from(h.of("menu-effect").at(-1).fields), settingsMenuDraft());
    assert.equal(h.of("menu-effect").at(-1).fields.length, 13, "legacy omission never fabricates policy strings");
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained binding Apply publishes literal 34 fields and refuses invalid final row without consuming its action", async () => {
  const { h } = await retainedSettingsWorker();
  try {
    assert.equal(h.of("menu-state").at(-1).fields.length, 34);
    await h.send({ kind: "menu-edit", ...acquiredMenuToken(h.of("menu-state").at(-1)), index: 16, value: "KeyA" });
    assert.equal(h.of("menu-effect").length, 0, "editing a code alone never admits bindings");
    for (const invalid of ["Escape", "UnsupportedPhysicalCode", "KeyA"]) {
      await h.send({ kind: "menu-edit", ...acquiredMenuToken(h.of("menu-state").at(-1)), index: 33, value: invalid });
      const before = h.of("menu-state").at(-1), errors = h.of("menu-error").length;
      const publications = h.renderPort.posts.length;
      await h.send({ kind: "menu-action", ...acquiredMenuToken(before), actionId: 1n, control: 10n });
      assert.equal(h.of("menu-error").length, errors + 1);
      assert.equal(h.of("menu-effect").length, 0, "last binding refusal admits no scalar, policy or key effect");
      assert.deepEqual(h.of("menu-state").at(-1), before, "invalid row33 preserves exact owner, token, selection and draft");
      assert.equal(h.renderPort.posts.length, publications, "refusal cannot publish a partially accepted menu");
      assert.match(h.of("menu-error").at(-1).message, /binding|code|key/i);
    }
    await h.send({ kind: "menu-edit", ...acquiredMenuToken(h.of("menu-state").at(-1)), index: 33, value: "" });
    const repaired = h.of("menu-state").at(-1);
    await h.send({ kind: "menu-action", ...acquiredMenuToken(repaired), actionId: 1n, control: 10n });
    const effect = h.of("menu-effect").at(-1);
    assert.equal(h.of("menu-effect").length, 1, "same semantic action ID succeeds after repairing the final row");
    assert.equal(effect.fields.length, 34);
    assert.deepEqual(Array.from(effect.fields), [...completeSettingsDraft(), ...importedBindingCodes()]);
    assert.equal(effect.fields[16], "KeyA"); assert.equal(effect.fields[33], "");
    assert.equal(effect.control, 10n); assert.equal(effect.effect, 1n);
    assert.deepEqual(acquiredMenuToken(effect), acquiredMenuToken(h.of("menu-state").at(-1)));
    await h.send({ kind: "menu-action", ...acquiredMenuToken(h.of("menu-state").at(-1)), actionId: 1n, control: 10n });
    assert.equal(h.of("menu-effect").length, 1, "successful Apply consumes the action once");
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained binding Apply refuses partial shapes and stale full draft before a legacy retry", async () => {
  const { h, initial } = await retainedSettingsWorker();
  try {
    for (const count of [17, 18, 32, 33, 35]) {
      const fields = [...bindingSettingsDraft(), "extra"].slice(0, count);
      await h.send({ kind: "menu-fields", ...acquiredMenuToken(h.of("menu-state").at(-1)), fields });
      const before = h.of("menu-state").at(-1), errors = h.of("menu-error").length;
      await h.send({ kind: "menu-action", ...acquiredMenuToken(before), actionId: 1n, control: 10n });
      assert.equal(h.of("menu-error").length, errors + 1);
      assert.equal(h.of("menu-effect").length, 0); assert.deepEqual(h.of("menu-state").at(-1), before);
    }
    await h.send({ kind: "menu-fields", ...acquiredMenuToken(h.of("menu-state").at(-1)), fields: bindingSettingsDraft() });
    const current = h.of("menu-state").at(-1);
    await h.send({ kind: "menu-action", ...acquiredMenuToken(initial), actionId: 1n, control: 10n });
    assert.equal(h.of("menu-effect").length, 0); assert.deepEqual(h.of("menu-state").at(-1), current);
    await h.send({ kind: "menu-fields", ...acquiredMenuToken(current), fields: completeSettingsDraft() });
    await h.send({ kind: "menu-action", ...acquiredMenuToken(h.of("menu-state").at(-1)), actionId: 1n, control: 10n });
    assert.deepEqual(Array.from(h.of("menu-effect").at(-1).fields), completeSettingsDraft());
    assert.equal(h.of("menu-effect").at(-1).fields.length, 16, "legacy scalar Apply cannot fabricate or replace keyboard rows");
  } finally { await h.send({ kind: "dispose" }); }
});

for (const [route, returnControl] of [[3, 71n], [5, 30n], [6, 21n]]) {
  test(`retained full profile in child route${route} preserves parent row33 and imported bindings on return`, async () => {
    const { h, parent } = await retainedSettingsWorker();
    try {
      await h.send({ kind: "menu-action", ...acquiredMenuToken(parent), actionId: 1n, control: 1033n });
      const selected = h.of("menu-state").at(-1); assert.equal(selected.selected, 33);
      const childFields = route === 3 ? ["1.000000001", "2.000000002"]
        : acquiredMenuFields([acquiredSource(1, "keyboard"), acquiredSource(41)]);
      await h.send({ kind: "menu-navigate", ...acquiredMenuToken(selected), route, fields: childFields });
      const t0 = h.of("menu-state").at(-1), dto = retainedProfile(), id = 151;
      await h.send({ kind: "settings-profile-load", id, menuToken: acquiredMenuToken(t0), settings: dto,
        file: selectedFile("bindings.json", JSON.stringify(dto)).file });
      const t1 = h.of("menu-state").at(-1); assertProfileTransition(t1, t0, id, true);
      assert.deepEqual(Array.from(t1.fields), Array.from(t0.fields));
      await h.send({ kind: "settings-profile-admit", id, menuToken: acquiredMenuToken(t1) });
      const t2 = h.of("menu-state").at(-1); assertProfileTransition(t2, t1, id, false);
      assert.equal(t2.route, route); assert.equal(t2.screen, t0.screen); assert.equal(t2.selected, t0.selected);
      assert.deepEqual(structuredClone(t2.roster), structuredClone(t0.roster));
      assert.deepEqual(Array.from(t2.fields), route === 3 ? ["3", "9"] : Array.from(t0.fields));
      const expected = importedSettingsFields();
      if (route === 3) {
        await h.send({ kind: "menu-edit", ...acquiredMenuToken(t2), index: 0, value: "4" });
        expected[11] = "4";
      }
      await h.send({ kind: "menu-action", ...acquiredMenuToken(h.of("menu-state").at(-1)), actionId: 2n, control: returnControl });
      const returned = h.of("menu-state").at(-1);
      assert.equal(returned.route, 2); assert.equal(returned.screen, parent.screen); assert.equal(returned.selected, 33);
      assert.deepEqual(Array.from(returned.fields), expected, "child return preserves every imported binding and policy row");
      await h.send({ kind: "menu-action", ...acquiredMenuToken(returned), actionId: 3n, control: 10n });
      assert.deepEqual(Array.from(h.of("menu-effect").at(-1).fields), expected);
      assert.equal(h.of("settings-profile-loaded").length, 1);
      assert.equal(h.of("menu-error").length, 0);
    } finally { await h.send({ kind: "dispose" }); }
  });
}

test("retained profile candidate survives read finally and strict T1 admission commits parent and Practice once", async () => {
  const { h, parent, initial: t0 } = await retainedSettingsWorker(true);
  try {
    const dto = retainedProfile(), id = 71;
    await h.send({ kind: "settings-profile-load", id, menuToken: acquiredMenuToken(t0), settings: JSON.parse(validSettingsProfileText()), file: selectedFile("policy.json", JSON.stringify(dto)).file });
    const t1 = h.of("menu-state").at(-1), candidate = h.of("settings-profile-candidate").at(-1);
    assertProfileTransition(t1, t0, id, true);
    assert.deepEqual(Array.from(t1.fields), Array.from(t0.fields));
    assert(candidate, "fully decoded profile remains a candidate, not an accepted load");
    assert.equal(candidate.id, id); assert.deepEqual(acquiredMenuToken(candidate.menuToken), acquiredMenuToken(t1));
    assert.deepEqual(structuredClone(candidate.settings), dto);
    assert.equal(candidate.settings.bindings.length, 18);
    assert.equal(h.of("settings-profile-loaded").length, 0);
    // An unrelated higher revision and original T0 never authorize admission.
    for (const menuToken of [acquiredMenuToken(t0), { ...acquiredMenuToken(t1), revision: t1.revision + 1n }]) {
      await h.send({ kind: "settings-profile-admit", id, menuToken });
      assert.equal(h.of("settings-profile-loaded").length, 0);
      assert.deepEqual(h.of("menu-state").at(-1), t1);
    }
    // An asynchronous finally must retain the same candidate/operation owner.
    await h.flushTasks();
    let retryReads = 0;
    await h.send({ kind: "settings-profile-load", id: 72, menuToken: acquiredMenuToken(t1), settings: dto,
      file: selectedFile("retry.json", JSON.stringify(dto), () => { retryReads++; throw Error("busy candidate must not acquire"); }).file });
    assert.equal(retryReads, 0);
    const firstReply = h.messages.length;
    await h.send({ kind: "settings-profile-admit", id, menuToken: acquiredMenuToken(t1) });
    const t2 = h.of("menu-state").at(-1), loaded = h.of("settings-profile-loaded").at(-1);
    assertProfileTransition(t2, t1, id, false);
    assert.equal(t2.route, 3); assert.equal(t2.selected, t0.selected);
    assert.deepEqual(Array.from(t2.fields), ["3", "9"]);
    assert.equal(loaded.id, id); assert.deepEqual(acquiredMenuToken(loaded.admittedToken), acquiredMenuToken(t1));
    assert.deepEqual(acquiredMenuToken(loaded.resultToken), acquiredMenuToken(t2));
    assert.deepEqual(structuredClone(loaded.settings), dto);
    const replies = h.messages.slice(firstReply);
    assert.equal(replies.findIndex(x => x === loaded), replies.findIndex(x => x === t2) + 1, "T2 and final success are published consecutively");
    await h.send({ kind: "settings-profile-admit", id, menuToken: acquiredMenuToken(t1) });
    await h.send({ kind: "settings-profile-cancel", id, menuToken: acquiredMenuToken(t1) });
    assert.equal(h.of("settings-profile-loaded").length, 1);
    assert.deepEqual(h.of("menu-state").at(-1), t2);
    await h.send({ kind: "menu-action", ...acquiredMenuToken(t2), actionId: 1n, control: 72n });
    const restored = h.of("menu-state").at(-1);
    assert.equal(restored.screen, parent.screen);
    assert.deepEqual(Array.from(restored.fields), importedSettingsFields(), "stale Practice values cannot overwrite loaded parent section or policy");
    await h.send({ kind: "menu-action", ...acquiredMenuToken(restored), actionId: 2n, control: 10n });
    assert.deepEqual(Array.from(h.of("menu-effect").at(-1).fields), importedSettingsFields());
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained profile held read cancellation accepts original T0 or current T1 but retains underlying read capacity", async () => {
  for (const useOriginal of [true, false]) {
    const { h, initial: t0 } = await retainedSettingsWorker(); const held = deferred();
    try {
      const dto = retainedProfile(), id = 81; let reads = 0;
      await h.send({ kind: "settings-profile-load", id, menuToken: acquiredMenuToken(t0), settings: dto,
        file: selectedFile("held.json", JSON.stringify(dto), () => { reads++; return held.promise; }).file });
      const t1 = h.of("menu-state").at(-1); assertProfileTransition(t1, t0, id, true);
      for (const [wrongId, menuToken] of [[id + 1, acquiredMenuToken(t0)], [id, { ...acquiredMenuToken(t1), revision: t1.revision + 1n }]]) {
        await h.send({ kind: "settings-profile-cancel", id: wrongId, menuToken });
        assert.deepEqual(h.of("menu-state").at(-1), t1);
      }
      await h.send({ kind: "settings-profile-cancel", id, menuToken: acquiredMenuToken(useOriginal ? t0 : t1) });
      const idle = h.of("menu-state").at(-1); assertProfileTransition(idle, t1, id, false);
      assert.deepEqual(Array.from(idle.fields), bindingSettingsDraft());
      let retryReads = 0;
      await h.send({ kind: "settings-profile-load", id: 82, menuToken: acquiredMenuToken(idle), settings: dto,
        file: selectedFile("retry.json", JSON.stringify(dto), () => { retryReads++; return Promise.resolve(new TextEncoder().encode(JSON.stringify(dto)).buffer); }).file });
      assert.equal(reads, 1); assert.equal(retryReads, 0, "retired unabortable read still owns the one bounded slot");
      held.resolve(new TextEncoder().encode(JSON.stringify(dto)).buffer); await h.flushTasks();
      assert.equal(h.of("settings-profile-candidate").length, 0); assert.equal(h.of("settings-profile-loaded").length, 0);
      await h.send({ kind: "settings-profile-load", id: 83, menuToken: acquiredMenuToken(idle), settings: dto, file: selectedFile("valid.json", JSON.stringify(dto)).file });
      assert.equal(h.of("settings-profile-candidate").at(-1).id, 83, "underlying settlement releases capacity for a genuine retry");
    } finally { held.resolve(new TextEncoder().encode(validSettingsProfileText()).buffer); await h.flushTasks(); await h.send({ kind: "dispose" }); }
  }
});

test("retained profile candidate cancellation drops DTO and late admit cannot revive it", async () => {
  const { h, initial: t0 } = await retainedSettingsWorker();
  try {
    const id = 91, dto = retainedProfile();
    await h.send({ kind: "settings-profile-load", id, menuToken: acquiredMenuToken(t0), settings: dto, file: selectedFile("valid.json", JSON.stringify(dto)).file });
    const t1 = h.of("menu-state").at(-1); assert(h.of("settings-profile-candidate").length === 1);
    await h.send({ kind: "settings-profile-cancel", id, menuToken: acquiredMenuToken(t1) });
    const idle = h.of("menu-state").at(-1); assertProfileTransition(idle, t1, id, false);
    await h.send({ kind: "settings-profile-admit", id, menuToken: acquiredMenuToken(t1) });
    assert.equal(h.of("settings-profile-loaded").length, 0); assert.deepEqual(h.of("menu-state").at(-1), idle);
    await h.send({ kind: "settings-profile-save", id: 92, menuToken: acquiredMenuToken(idle), settings: dto });
    assert.equal(h.of("settings-profile-saved").at(-1).id, 92);
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained profile pending gates navigation, keyboard selection, acquisition, input and motion", async () => {
  const { h, initial: t0 } = await retainedSettingsWorker(); const held = deferred();
  try {
    await h.send({ kind: "resize", width: 960, height: 720 });
    const dto = retainedProfile();
    await h.send({ kind: "settings-profile-load", id: 95, menuToken: acquiredMenuToken(t0), settings: dto,
      file: selectedFile("held.json", JSON.stringify(dto), () => held.promise).file });
    const t1 = h.of("menu-state").at(-1); assertProfileTransition(t1, t0, 95, true);
    const posts = h.renderPort.posts.length;
    for (const request of [
      { kind: "menu-open", fields: ["replacement.bms"] },
      { kind: "menu-navigate", route: 3, fields: ["4", "8"] },
      { kind: "menu-edit", index: 13, value: "" },
      { kind: "menu-fields", fields: settingsMenuDraft() },
      { kind: "menu-action", control: 1015n, actionId: 1n },
      { kind: "menu-action", control: 10n, actionId: 1n },
      { kind: "menu-input", event: "keydown", key: "End" },
    ]) {
      const errors = h.of("menu-error").length;
      await h.send({ ...acquiredMenuToken(t1), ...request });
      assert.equal(h.of("menu-error").length, errors + 1);
      assert.deepEqual(h.of("menu-state").at(-1), t1);
    }
    const latestMenu = h.renderPort.posts.findLast(x => x.kind === "menu");
    await h.send({ kind: "menu-motion", requestId: 1n, hostOwner: 1, ...acquiredMenuToken(t1),
      generation: latestMenu.generation, content: latestMenu.content, geometryVersion: 1n, control: 1000n,
      transforms: new Float32Array([0, 0, 1, 1, 1, 40, 10, 1, 1, 1]), durationMs: 1000, easing: 0 });
    assert.equal(h.of("menu-motion-reply").at(-1).admitted, false);
    assert.equal(h.renderPort.posts.length, posts, "pending input/acquisition/motion never reach the renderer");
    assert.equal(h.of("menu-effect").length, 0);
    held.resolve(new TextEncoder().encode(JSON.stringify(dto)).buffer); await h.flushTasks();
    await h.send({ kind: "settings-profile-cancel", id: 95, menuToken: acquiredMenuToken(t1) });
    const idle = h.of("menu-state").at(-1);
    await h.send({ kind: "menu-action", ...acquiredMenuToken(idle), control: 10n, actionId: 1n });
    assert.equal(h.of("menu-effect").length, 1, "pending refusals never consume the last action counter");
  } finally { held.resolve(new TextEncoder().encode(validSettingsProfileText()).buffer); await h.flushTasks(); await h.send({ kind: "dispose" }); }
});

test("retained profile stale request acquires no bytes and malformed late policy never replaces draft", async () => {
  const { h, initial: t0 } = await retainedSettingsWorker();
  try {
    let reads = 0; const dto = retainedProfile();
    for (const menuToken of [undefined, { ...acquiredMenuToken(t0), revision: t0.revision + 1n }, { ...acquiredMenuToken(t0), screen: t0.screen + 1n }]) {
      await h.send({ kind: "settings-profile-load", id: 96, menuToken, settings: dto,
        file: selectedFile("unused.json", JSON.stringify(dto), () => { reads++; throw Error("stale request must not read"); }).file });
      assert.deepEqual(h.of("menu-state").at(-1), t0);
    }
    assert.equal(reads, 0);
    dto.timing.gauge = "invalid-late-policy";
    await h.send({ kind: "settings-profile-load", id: 97, menuToken: acquiredMenuToken(t0), settings: retainedProfile(), file: selectedFile("invalid.json", JSON.stringify(dto)).file });
    assert.equal(h.of("settings-profile-candidate").length, 0); assert.equal(h.of("settings-profile-loaded").length, 0);
    assert.equal(h.of("settings-profile-error").at(-1).id, 97);
    const idle = h.of("menu-state").at(-1);
    assert.equal(idle.pending, false); assert.deepEqual(Array.from(idle.fields), bindingSettingsDraft());
    await h.send({ kind: "menu-action", ...acquiredMenuToken(idle), control: 10n, actionId: 1n });
    assert.equal(h.of("menu-effect").length, 1);
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained full profile malformed final binding and failed read restore all original 34 rows", async () => {
  const { h } = await retainedSettingsWorker();
  try {
    for (const [id, code] of [[161, "Escape"], [162, "UnknownKey"], [163, "KeyA"]]) {
      const t0 = h.of("menu-state").at(-1), dto = retainedProfile();
      dto.bindings[17][1] = code;
      await h.send({ kind: "settings-profile-load", id, menuToken: acquiredMenuToken(t0), settings: retainedProfile(),
        file: selectedFile("invalid-last-binding.json", JSON.stringify(dto)).file });
      const idle = h.of("menu-state").at(-1);
      assert.equal(idle.pending, false); assert.equal(idle.screen, t0.screen);
      assert.deepEqual(Array.from(idle.fields), bindingSettingsDraft());
      assert.equal(h.of("settings-profile-error").at(-1).id, id);
      assert.equal(h.of("settings-profile-candidate").length, 0);
      assert.equal(h.of("settings-profile-loaded").length, 0);
    }
    const t0 = h.of("menu-state").at(-1), read = deferred(), dto = retainedProfile(), id = 164;
    await h.send({ kind: "settings-profile-load", id, menuToken: acquiredMenuToken(t0), settings: dto,
      file: selectedFile("failed-read.json", JSON.stringify(dto), () => read.promise).file });
    const t1 = h.of("menu-state").at(-1); assertProfileTransition(t1, t0, id, true);
    read.reject(Error("original binding profile read failure")); await h.flushTasks();
    const idle = h.of("menu-state").at(-1); assertProfileTransition(idle, t1, id, false);
    assert.deepEqual(Array.from(idle.fields), bindingSettingsDraft());
    assert.equal(h.of("settings-profile-error").at(-1).id, id);
    assert.match(h.of("settings-profile-error").at(-1).message, /original binding profile read failure/);
    await h.send({ kind: "settings-profile-admit", id, menuToken: acquiredMenuToken(t1) });
    assert.equal(h.of("settings-profile-loaded").length, 0); assert.deepEqual(h.of("menu-state").at(-1), idle);
    await h.send({ kind: "menu-action", ...acquiredMenuToken(idle), actionId: 1n, control: 10n });
    assert.deepEqual(Array.from(h.of("menu-effect").at(-1).fields), bindingSettingsDraft());
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained profile save publishes exact pending and Idle receipts before full eighteen-binding bytes", async () => {
  const { h, initial: t0 } = await retainedSettingsWorker();
  try {
    const start = h.messages.length, dto = retainedProfile(), id = 101;
    await h.send({ kind: "settings-profile-save", id, menuToken: acquiredMenuToken(t0), settings: dto });
    const states = h.messages.slice(start).filter(x => x.kind === "menu-state");
    assert.equal(states.length, 2); const [t1, t2] = states;
    assertProfileTransition(t1, t0, id, true); assertProfileTransition(t2, t1, id, false);
    for (const state of states) assert.deepEqual(Array.from(state.fields), bindingSettingsDraft());
    const saved = h.of("settings-profile-saved").at(-1);
    assert.equal(saved.id, id); assert.deepEqual(acquiredMenuToken(saved.resultToken), acquiredMenuToken(t2));
    assert.deepEqual(JSON.parse(new TextDecoder().decode(saved.bytes)), dto);
    assert.equal(h.messages.indexOf(saved), h.messages.indexOf(t2) + 1);
    assert.equal(h.of("settings-profile-candidate").length, 0);
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained profile no Settings ancestor preserves single-phase legacy full profile with eighteen bindings", async () => {
  const h = await readyWorker({ actualMenu: true });
  try {
    await h.send({ kind: "menu-open", fields: ["chart.bms"] });
    const before = h.of("menu-state").at(-1), dto = JSON.parse(validSettingsProfileText());
    dto.bindings.sort((a, b) => a[0] - b[0]);
    await h.send({ kind: "settings-profile-load", id: 111, settings: dto, file: selectedFile("legacy.json", JSON.stringify(dto)).file });
    assert.equal(h.of("settings-profile-candidate").length, 0);
    assert.equal(h.of("settings-profile-loaded").length, 1);
    assert.deepEqual(structuredClone(h.of("settings-profile-loaded")[0].settings), dto);
    assert.equal(h.of("settings-profile-loaded")[0].settings.bindings.length, 18);
    assert.deepEqual(h.of("menu-state").at(-1), before, "no fabricated Settings parent or pending transition");
  } finally { await h.send({ kind: "dispose" }); }
});

test("retained profile pending refuses source inventory replacement and preserves Players and parent policy", async () => {
  const { h, parent } = await retainedSettingsWorker(); const held = deferred();
  try {
    const inventory = acquiredMenuFields([acquiredSource(1, "keyboard"), acquiredSource(41)]);
    await h.send({ kind: "menu-navigate", ...acquiredMenuToken(parent), route: 5, fields: inventory });
    const t0 = h.of("menu-state").at(-1), dto = retainedProfile();
    await h.send({ kind: "settings-profile-load", id: 121, menuToken: acquiredMenuToken(t0), settings: dto,
      file: selectedFile("held.json", JSON.stringify(dto), () => held.promise).file });
    const t1 = h.of("menu-state").at(-1); assertProfileTransition(t1, t0, 121, true);
    const errors = h.of("menu-error").length;
    await h.send({ kind: "menu-fields", ...acquiredMenuToken(t1), fields: acquiredMenuFields([acquiredSource(1, "keyboard")]) });
    assert.equal(h.of("menu-error").length, errors + 1); assert.deepEqual(h.of("menu-state").at(-1), t1);
    held.resolve(new TextEncoder().encode(JSON.stringify(dto)).buffer); await h.flushTasks();
    await h.send({ kind: "settings-profile-admit", id: 121, menuToken: acquiredMenuToken(t1) });
    const loadedChild = h.of("menu-state").at(-1);
    assert.equal(loadedChild.screen, t0.screen); assert.equal(loadedChild.selected, t0.selected);
    assert.deepEqual(Array.from(loadedChild.fields), Array.from(t0.fields));
    assert.deepEqual(structuredClone(loadedChild.roster), structuredClone(t0.roster));
    await h.send({ kind: "menu-action", ...acquiredMenuToken(loadedChild), actionId: 1n, control: 30n });
    const returned = h.of("menu-state").at(-1);
    assert.equal(returned.screen, parent.screen); assert.deepEqual(Array.from(returned.fields), importedSettingsFields());
  } finally { held.resolve(new TextEncoder().encode(validSettingsProfileText()).buffer); await h.flushTasks(); await h.send({ kind: "dispose" }); }
});

test("retained profile legacy full load restores the exact default policy triple", async () => {
  const { h, initial: t0 } = await retainedSettingsWorker();
  try {
    const dto = JSON.parse(validSettingsProfileText()); dto.bindings.sort((a, b) => a[0] - b[0]);
    await h.send({ kind: "settings-profile-load", id: 131, menuToken: acquiredMenuToken(t0), settings: retainedProfile(), file: selectedFile("legacy.json", JSON.stringify(dto)).file });
    const t1 = h.of("menu-state").at(-1);
    assert.deepEqual(structuredClone(h.of("settings-profile-candidate").at(-1).settings), dto);
    await h.send({ kind: "settings-profile-admit", id: 131, menuToken: acquiredMenuToken(t1) });
    const t2 = h.of("menu-state").at(-1);
    assert.deepEqual(Array.from(t2.fields), ["50", "50", "0", "interactive", "20", "48000", "256", "64", "256", "2048", "256", "0", "", "", "rank-first", "beatkernel", ...bindingCodes()]);
    assert.deepEqual(structuredClone(h.of("settings-profile-loaded").at(-1).settings), dto);
    await h.send({ kind: "menu-action", ...acquiredMenuToken(t2), actionId: 1n, control: 10n });
    assert.deepEqual(Array.from(h.of("menu-effect").at(-1).fields), Array.from(t2.fields));
  } finally { await h.send({ kind: "dispose" }); }
});

for (const phase of ["candidate", "held-read"]) {
  for (const retirement of ["fatal", "dispose"]) {
    for (const settlement of phase === "held-read" ? ["resolve", "reject"] : ["already-settled"]) {
      test(`retained profile ${phase} owner ${retirement} suppresses late ${settlement} and admission`, async () => {
        const { h, initial: t0 } = await retainedSettingsWorker();
        const read = deferred(), dto = retainedProfile(), id = 141;
        let readCalls = 0;
        try {
          await h.send({ kind: "settings-profile-load", id, menuToken: acquiredMenuToken(t0), settings: dto,
            file: selectedFile("owned.json", JSON.stringify(dto), () => { readCalls++; return read.promise; }).file });
          const t1 = h.of("menu-state").at(-1);
          assertProfileTransition(t1, t0, id, true);
          assert.equal(readCalls, 1);
          if (phase === "candidate") {
            read.resolve(new TextEncoder().encode(JSON.stringify(dto)).buffer); await h.flushTasks();
            const candidate = h.of("settings-profile-candidate").at(-1);
            assert.equal(candidate.id, id);
            assert.deepEqual(acquiredMenuToken(candidate.menuToken), acquiredMenuToken(t1));
          }
          assert.equal(h.of("settings-profile-candidate").length, phase === "candidate" ? 1 : 0);
          assert.equal(h.of("settings-profile-loaded").length, 0);
          if (retirement === "fatal") {
            // A second initialization is a genuine public Worker fatal path.
            await h.send({ kind: "init" });
            assert.equal(h.of("fatal").length, 1);
          } else {
            await h.send({ kind: "dispose" });
            assert.equal(h.of("disposed").length, 1);
          }
          const afterRetirement = h.messages.length;
          const candidates = h.of("settings-profile-candidate").length;
          const loaded = h.of("settings-profile-loaded").length;
          const errors = h.of("settings-profile-error").length;
          const states = h.of("menu-state").length;
          const publications = h.renderPort.posts.length;
          await h.send({ kind: "settings-profile-admit", id, menuToken: acquiredMenuToken(t1) });
          await h.send({ kind: "settings-profile-cancel", id, menuToken: acquiredMenuToken(t0) });
          if (phase === "held-read") {
            if (settlement === "resolve") read.resolve(new TextEncoder().encode(JSON.stringify(dto)).buffer);
            else read.reject(Error("retired profile File read failed"));
            await h.flushTasks();
          }
          await h.send({ kind: "settings-profile-admit", id, menuToken: acquiredMenuToken(t1) });
          await h.send({ kind: "menu-fields", ...acquiredMenuToken(t1), fields: settingsMenuDraft() });
          assert.equal(h.of("settings-profile-candidate").length, candidates);
          assert.equal(h.of("settings-profile-loaded").length, loaded);
          assert.equal(h.of("settings-profile-error").length, errors);
          assert.equal(h.of("menu-state").length, states);
          assert.deepEqual(h.of("menu-state").at(-1), t1, "retired owner never publishes a model replacement");
          assert.equal(h.renderPort.posts.length, publications, "late operation completion cannot revive rendering");
          assert.deepEqual(h.messages.slice(afterRetirement), [], "retirement suppresses both late success and late read failure");
          assert.equal(readCalls, 1);
        } finally {
          read.resolve(new TextEncoder().encode(JSON.stringify(dto)).buffer); await h.flushTasks();
          await h.send({ kind: "dispose" });
        }
      });
    }
  }
}

async function motionWorker() {
  const h = await readyWorker({ actualMenu: true });
  await h.send({ kind: "resize", width: 960, height: 720 });
  await h.send({ kind: "menu-open", fields: ["chart.bms"] });
  const menu = h.renderPort.posts.filter(m => m.kind === "menu").at(-1);
  const version = menu.geometryVersion ?? h.renderPort.posts.filter(m => m.kind === "resize").at(-1).geometryVersion;
  const token = acquiredMenuToken(h.of("menu-state").at(-1));
  h.renderPort.emit({ kind: "geometry-ack", generation: menu.generation, content: menu.content,
    geometryVersion: version, page: 0, width: 960, height: 720, ...token });
  await flushJobs(); assert.equal(h.of("render-geometry").at(-1).mode, "menu");
  return h;
}
function cpuMotion(h, requestId = 1n, fields = {}) {
  const geometry = h.of("render-geometry").at(-1);
  return { kind: "menu-motion", requestId, hostOwner: 1, ...acquiredMenuToken(h.of("menu-state").at(-1)),
    generation: geometry.generation, content: geometry.content, geometryVersion: geometry.geometryVersion,
    control: 5n, transforms: new Float32Array([0, 0, 1, 1, 1, 40, 10, 1, 1, 1]), durationMs: 1000, easing: 0, ...fields };
}
function emitMenuGeometry(h, request) {
  h.renderPort.emit({ kind: "geometry-ack", generation: request.generation, content: request.content,
    geometryVersion: request.geometryVersion, page: 0, width: 960, height: 720,
    ...acquiredMenuToken(h.of("menu-state").at(-1)) });
}

test("CPU motion ACK admits a copied request before paint and preserves input independence", async () => {
  const h = await motionWorker(); try {
    const geometry = h.of("render-geometry").at(-1); h.renderPort.blocked = true;
    const request = cpuMotion(h); const expected = [...request.transforms]; await h.send(request); request.transforms.fill(999);
    const dispatched = h.renderPort.posts.at(-1); assert.equal(dispatched.kind, "menu-motion");
    assert.deepEqual([...dispatched.transforms], expected); assert.ok(dispatched.geometryVersion > geometry.geometryVersion);
    assert.equal(h.of("menu-motion-reply").length, 0);
    h.renderPort.ack(dispatched); await flushJobs();
    const reply = h.of("menu-motion-reply").at(-1); assert.equal(reply.admitted, true);
    assert.equal(reply.requestId, 1n); assert.equal(reply.geometryVersion, geometry.geometryVersion);
    assert.equal(reply.admittedGeometryVersion, dispatched.geometryVersion);
    assert.equal(h.of("render-geometry").at(-1), geometry, "control admission is not submission evidence");
    assert.equal(h.of("fatal").length, 0); assert.equal(h.of("render-error").length, 0);
  } finally { await h.send({ kind: "dispose" }); }
});

test("CPU FIFO bounds 64 including in-flight, preserves every target and settles each separately", async () => {
  const h = await motionWorker(); try {
    h.renderPort.blocked = true;
    for (let i = 1; i <= 65; i++) await h.send(cpuMotion(h, BigInt(i), { control: BigInt(1000 + i) }));
    assert.equal(h.renderPort.posts.filter(m => m.kind === "menu-motion").length, 1);
    const refused = h.of("menu-motion-reply"); assert.equal(refused.length, 1); assert.equal(refused[0].requestId, 65n); assert.equal(refused[0].admitted, false);
    for (let i = 1; i <= 64; i++) {
      const dispatched = h.renderPort.posts.filter(m => m.kind === "menu-motion").at(-1);
      assert.equal(dispatched.control, BigInt(1000 + i));
      h.renderPort.ack(dispatched); await flushJobs();
    }
    const replies = h.of("menu-motion-reply"); assert.equal(replies.length, 65);
    assert.equal(replies.filter(m => m.admitted).length, 64); assert.equal(new Set(replies.map(m => m.requestId)).size, 65);
    const versions = h.renderPort.posts.filter(m => m.kind === "menu-motion").map(m => m.geometryVersion);
    assert.equal(versions.every((v, i) => i === 0 || v > versions[i - 1]), true);
    assert.equal(h.of("render-error").length, 0);
  } finally { await h.send({ kind: "dispose" }); }
});

test("submitted paint can stale another queued ticket which rejects explicitly", async () => {
  const h = await motionWorker(); try {
    h.renderPort.blocked = true;
    await h.send(cpuMotion(h, 1n, { control: 1000n })); await h.send(cpuMotion(h, 2n, { control: 1001n }));
    const first = h.renderPort.posts.at(-1); emitMenuGeometry(h, first); await flushJobs();
    h.renderPort.ack(first); await flushJobs();
    assert.equal(h.of("menu-motion-reply").length, 2);
    assert.equal(h.of("menu-motion-reply").find(m => m.requestId === 2n).admitted, false);
    assert.equal(h.renderPort.posts.filter(m => m.kind === "menu-motion").length, 1);
    await h.send(cpuMotion(h, 3n, { control: 1001n }));
    const refreshed = h.renderPort.posts.at(-1); assert.equal(refreshed.control, 1001n);
    h.renderPort.ack(refreshed); await flushJobs(); assert.equal(h.of("menu-motion-reply").at(-1).admitted, true);
  } finally { await h.send({ kind: "dispose" }); }
});

test("malformed and foreign CPU motion requests refuse locally without poisoning later requests", async () => {
  const h = await motionWorker(); try {
    for (const [index, fields] of [{ content: 999n }, { revision: 999n }, { geometryVersion: 999n },
      { transforms: new Float32Array(9) }, { control: 0n }, { easing: 4 }].entries()) await h.send(cpuMotion(h, BigInt(index + 1), fields));
    assert.equal(h.of("menu-motion-reply").length, 6); assert.equal(h.of("menu-motion-reply").some(m => m.admitted), false);
    assert.equal(h.of("fatal").length, 0); assert.equal(h.of("render-error").length, 0);
    await h.send(cpuMotion(h, 7n)); assert.equal(h.of("menu-motion-reply").at(-1).admitted, true);
  } finally { await h.send({ kind: "dispose" }); }
});

test("CPU motion deadline rejects once and late renderer ACK cannot revive the RPC", async () => {
  const h = await motionWorker(); try {
    h.renderPort.blocked = true; await h.send(cpuMotion(h));
    const dispatched = h.renderPort.posts.at(-1);
    await h.tick();
    assert.equal(h.of("menu-motion-reply").length, 1);
    assert.equal(h.of("menu-motion-reply")[0].admitted, false);
    assert.match(h.of("menu-motion-reply")[0].message, /timed out/);
    h.renderPort.ack(dispatched); await flushJobs(); assert.equal(h.of("menu-motion-reply").length, 1);
    await h.send(cpuMotion(h, 2n)); const next = h.renderPort.posts.at(-1);
    h.renderPort.ack(next); await flushJobs(); assert.equal(h.of("menu-motion-reply").at(-1).admitted, true);
    assert.equal(h.of("render-error").length, 0);
  } finally { await h.send({ kind: "dispose" }); }
});

test("resize and menu snapshot counters precede dispatch-time motion geometry", async () => {
  const h = await motionWorker(); try {
    h.renderPort.blocked = true;
    await h.send({ kind: "resize", width: 800, height: 600 }); const resize = h.renderPort.posts.at(-1);
    await h.send(cpuMotion(h));
    assert.equal(h.renderPort.posts.filter(m => m.kind === "menu-motion").length, 0);
    h.renderPort.ack(resize); await flushJobs();
    const motion = h.renderPort.posts.at(-1); assert.equal(motion.kind, "menu-motion"); assert.ok(motion.geometryVersion > resize.geometryVersion);
    h.renderPort.ack(motion); await flushJobs();
    h.renderPort.blocked = false;
    await h.send({ kind: "menu-navigate", ...acquiredMenuToken(h.of("menu-state").at(-1)), route: 2, fields: settingsMenuDraft() });
    await h.send({ kind: "menu-edit", ...acquiredMenuToken(h.of("menu-state").at(-1)), index: 0, value: "74" });
    const menu = h.renderPort.posts.filter(m => m.kind === "menu").at(-1);
    emitMenuGeometry(h, menu); await flushJobs(); await h.send(cpuMotion(h, 2n));
    const last = h.renderPort.posts.filter(m => m.kind === "menu-motion").at(-1); assert.ok(last.geometryVersion > menu.geometryVersion);
  } finally { await h.send({ kind: "dispose" }); }
});

for (const terminal of ["dispose", "replace", "transport-failure", "local-reject"]) test(`CPU ${terminal} settles motion exactly once`, async () => {
  const h = await motionWorker(); try {
    h.renderPort.blocked = true; await h.send(cpuMotion(h)); await h.send(cpuMotion(h, 2n, { control: 6n }));
    const first = h.renderPort.posts.at(-1);
    if (terminal === "dispose") await h.send({ kind: "dispose" });
    else if (terminal === "replace") await h.send({ kind: "menu-open", fields: ["new.bms"] });
    else if (terminal === "transport-failure") { h.renderPort.emit({ kind: "render-error", generation: first.generation, content: first.content, message: "transport failed" }); await flushJobs(); }
    else {
      h.renderPort.emit({ kind: "control-reject", operation: "menu-motion", operationId: first.operationId, generation: first.generation, content: first.content, geometryVersion: first.geometryVersion, message: "unavailable target" }); await flushJobs();
      const second = h.renderPort.posts.at(-1); assert.equal(second.control, 6n); h.renderPort.ack(second); await flushJobs();
      assert.equal(h.of("render-error").length, 0);
    }
    const replies = h.of("menu-motion-reply"); assert.equal(replies.length, 2); assert.equal(new Set(replies.map(m => m.requestId)).size, 2);
    h.renderPort.ack(first); await flushJobs(); assert.equal(h.of("menu-motion-reply").length, 2);
  } finally { await h.send({ kind: "dispose" }); }
});

async function retainedDisplayWorker(fields) {
  const worker = await readyWorker({ actualMenu: true });
  await worker.send({ kind: "menu-open", fields: ["song/chart.bms"] });
  await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(worker.of("menu-state").at(-1)), route: 2, fields: settingsMenuDraft() });
  assert.equal(worker.of("menu-state").at(-1).route, 2);
  await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(worker.of("menu-state").at(-1)), route: 7, fields });
  assert.equal(worker.of("menu-state").at(-1).route, 7);
  return worker;
}

for (const dimensions of [["1", "16384"], ["16384", "1"], ["00001", "00720"], ["00042", "00001"]]) {
  test(`actual Worker Display Done accepts exact CSS bounds and leading zeros ${dimensions.join("x")}`, async () => {
    const fields = ["auto", "fifo", ...dimensions];
    const worker = await retainedDisplayWorker(fields);
    try {
      const draft = worker.of("menu-state").at(-1);
      assert.deepEqual(Array.from(draft.fields), fields);
      await worker.send({ kind: "menu-action", ...acquiredMenuToken(draft), actionId: 1n, control: 40n });
      assert.equal(worker.of("menu-error").length, 0);
      assert.equal(worker.of("menu-effect").length, 1);
      const effect = worker.of("menu-effect")[0];
      assert.equal(effect.route, 7); assert.equal(effect.control, 40n); assert.equal(effect.effect, 1n);
      assert.deepEqual(Array.from(effect.fields), fields, "CSS decimal syntax must survive unchanged rather than becoming native FPS/lookahead");
      assert.deepEqual(acquiredMenuToken(effect), acquiredMenuToken(worker.of("menu-state").at(-1)));
      const accepted = worker.of("menu-state").at(-1), errors = worker.of("menu-error").length;
      await worker.send({ kind: "menu-action", ...acquiredMenuToken(accepted), actionId: 1n, control: 40n });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.equal(worker.of("menu-effect").length, 1, "accepted Done consumes its semantic action exactly once");
      assert.deepEqual(worker.of("menu-state").at(-1), accepted);
      assert.equal(worker.games.length, 0); assert.equal(worker.preparedOwners.length, 0);
    } finally { await worker.send({ kind: "dispose" }); }
  });
}

test("actual Worker Display invalid final extent and unsupported modes refuse atomically then repair with the same action ID", async () => {
  const worker = await retainedDisplayWorker(["auto", "fifo", "00042", "00720"]);
  try {
    for (const [index, value] of [[3, "0"], [3, "16385"], [3, "000001"], [3, "1.5"], [3, ""],
      [3, " 720"], [3, "1e3"], [2, "0"], [2, "16385"], [0, "vulkan"], [1, "immediate"]]) {
      await worker.send({ kind: "menu-edit", ...acquiredMenuToken(worker.of("menu-state").at(-1)), index, value });
      const invalid = worker.of("menu-state").at(-1), errors = worker.of("menu-error").length;
      const publications = worker.renderPort.posts.length;
      assert.equal(invalid.fields[index], value, "the editor preserves invalid draft text until Done validation");
      await worker.send({ kind: "menu-action", ...acquiredMenuToken(invalid), actionId: 1n, control: 40n });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.equal(worker.of("menu-effect").length, 0);
      assert.deepEqual(worker.of("menu-state").at(-1), invalid, "refusal leaves selection, draft, screen and revision unchanged");
      assert.equal(worker.renderPort.posts.length, publications, "refused Done publishes no accepted renderer model");
      await worker.send({ kind: "menu-edit", ...acquiredMenuToken(invalid), index, value: ["auto", "fifo", "00042", "00720"][index] });
    }
    const repaired = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(repaired), actionId: 1n, control: 40n });
    assert.equal(worker.of("menu-effect").length, 1, "every refused Done leaves the same action ID available for repair");
    assert.equal(worker.of("menu-effect")[0].effect, 1n, "refused attempts do not advance the effect counter");
    assert.deepEqual(Array.from(worker.of("menu-effect")[0].fields), ["auto", "fifo", "00042", "00720"]);
    const accepted = worker.of("menu-state").at(-1), errors = worker.of("menu-error").length;
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(accepted), actionId: 1n, control: 40n });
    assert.equal(worker.of("menu-error").length, errors + 1);
    assert.equal(worker.of("menu-effect").length, 1);
    assert.deepEqual(worker.of("menu-state").at(-1), accepted);
  } finally { await worker.send({ kind: "dispose" }); }
});

test("actual Worker Display partial counts and stale tokens cannot consume Done admission before a valid retry", async () => {
  const fields = ["auto", "fifo", "960", "720"];
  const worker = await retainedDisplayWorker(fields);
  try {
    const original = worker.of("menu-state").at(-1);
    for (const malformed of [[], fields.slice(0, 3), [...fields, "extra"]]) {
      await worker.send({ kind: "menu-fields", ...acquiredMenuToken(worker.of("menu-state").at(-1)), fields: malformed });
      const draft = worker.of("menu-state").at(-1), errors = worker.of("menu-error").length;
      const publications = worker.renderPort.posts.length;
      assert.deepEqual(Array.from(draft.fields), malformed);
      await worker.send({ kind: "menu-action", ...acquiredMenuToken(draft), actionId: 1n, control: 40n });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.equal(worker.of("menu-effect").length, 0);
      assert.deepEqual(worker.of("menu-state").at(-1), draft);
      assert.equal(worker.renderPort.posts.length, publications);
    }
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(worker.of("menu-state").at(-1)), fields });
    const repaired = worker.of("menu-state").at(-1);
    for (const token of [acquiredMenuToken(original), { ...acquiredMenuToken(repaired), menuGeneration: repaired.menuGeneration + 1n },
      { ...acquiredMenuToken(repaired), screen: repaired.screen + 1n }]) {
      const errors = worker.of("menu-error").length, publications = worker.renderPort.posts.length;
      await worker.send({ kind: "menu-action", ...token, actionId: 1n, control: 40n });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.equal(worker.of("menu-effect").length, 0);
      assert.deepEqual(worker.of("menu-state").at(-1), repaired);
      assert.equal(worker.renderPort.posts.length, publications);
    }
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(repaired), actionId: 1n, control: 40n });
    assert.equal(worker.of("menu-effect").length, 1);
    assert.equal(worker.of("menu-effect")[0].effect, 1n);
    assert.deepEqual(Array.from(worker.of("menu-effect")[0].fields), fields);
    const accepted = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(accepted), actionId: 1n, control: 40n });
    assert.equal(worker.of("menu-effect").length, 1);
    assert.deepEqual(worker.of("menu-state").at(-1), accepted);
  } finally { await worker.send({ kind: "dispose" }); }
});

test("actual Worker and WASM Settings Apply validates thirteen scalars and emits the exact correlated effect", async () => {
  const worker = await readyWorker({ actualMenu: true });
  try {
    await worker.send({ kind: "menu-open", fields: ["song/chart.bms"] });
    await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(worker.of("menu-state").at(-1)),
      route: 2, fields: settingsMenuDraft() });
    const accepted = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(accepted), actionId: 1n, control: 10n });
    const applied = worker.of("menu-state").at(-1), effect = worker.of("menu-effect").at(-1);
    assert.equal(worker.of("menu-effect").length, 1);
    assert.equal(effect.effect, 1n); assert.equal(effect.control, 10n); assert.equal(effect.route, 2);
    assert.deepEqual(acquiredMenuToken(effect), acquiredMenuToken(applied));
    assert.deepEqual(Array.from(effect.fields), settingsMenuDraft());
    assert.equal(effect.fields.length, 13); assert.equal(effect.fields[0], "73");
    assert.equal(worker.of("menu-error").length, 0);
    assert.equal(worker.games.length, 0); assert.equal(worker.preparedOwners.length, 0);
    assert.equal(worker.of("settings-profile-saved").length, 0, "Apply does not become a full-profile file save");
  } finally { await worker.send({ kind: "dispose" }); }
});

test("actual Worker catalog selection uses current business tokens and never prepares preview audio", async () => {
  const worker = await readyWorker({ actualMenu: true });
  try {
    const files = ["a.bms", "b.bms", "c.bms"].map(path => selectedFile(path));
    await worker.send({ kind: "import", id: 1, files });
    await worker.send({ kind: "accept-library", id: 1 });
    await worker.send({ kind: "select", id: 2, libraryId: 1, path: "a.bms", rate: 48000, seed: "0" });
    const preview = worker.views[0].current;
    const preparations = worker.libraries[0].preparations.length;
    const accesses = worker.libraries[0].accesses.length;
    await worker.send({ kind: "menu-open", fields: ["a.bms", "b.bms", "c.bms"] });
    const first = worker.of("menu-state").at(-1);
    assert.equal(first.route, 1);
    assert.equal(first.selected, 0);
    await worker.send({ kind: "menu-select", ...acquiredMenuToken(first), actionId: 1n, index: 1 });
    const second = worker.of("menu-state").at(-1);
    assert.equal(second.selected, 1);
    assert.equal(second.screen, first.screen);
    assert.ok(second.revision > first.revision);
    assert.deepEqual(Array.from(second.fields), ["a.bms", "b.bms", "c.bms"]);
    assert.equal(worker.of("menu-error").length, 0);

    for (const index of [-1, 3, 0.5, undefined, "1", NaN]) {
      const errors = worker.of("menu-error").length;
      await worker.send({ kind: "menu-select", ...acquiredMenuToken(second), actionId: 2n, index });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.deepEqual(worker.of("menu-state").at(-1), second);
    }
    for (const token of [acquiredMenuToken(first),
      { ...acquiredMenuToken(second), screen: second.screen + 1n },
      { ...acquiredMenuToken(second), menuGeneration: second.menuGeneration + 1n }]) {
      const errors = worker.of("menu-error").length;
      await worker.send({ kind: "menu-select", ...token, actionId: 2n, index: 2 });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.deepEqual(worker.of("menu-state").at(-1), second);
    }
    await worker.send({ kind: "menu-select", ...acquiredMenuToken(second), actionId: 2n, index: 2 });
    const third = worker.of("menu-state").at(-1);
    assert.equal(third.selected, 2, "rejected requests must not consume the action identity");
    for (const actionId of [2n, 0n, 3, "3", 18446744073709551616n]) {
      const errors = worker.of("menu-error").length;
      await worker.send({ kind: "menu-select", ...acquiredMenuToken(third), actionId, index: 0 });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.deepEqual(worker.of("menu-state").at(-1), third);
    }
    await worker.send({ kind: "menu-select", ...acquiredMenuToken(third), actionId: 3n, index: 0 });
    const reset = worker.of("menu-state").at(-1);
    assert.equal(reset.selected, 0);
    await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(reset), route: 2, fields: settingsMenuDraft() });
    const settings = worker.of("menu-state").at(-1), errors = worker.of("menu-error").length;
    await worker.send({ kind: "menu-select", ...acquiredMenuToken(settings), actionId: 4n, index: 1 });
    assert.equal(worker.of("menu-error").length, errors + 1);
    assert.deepEqual(worker.of("menu-state").at(-1), settings);
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(settings), actionId: 4n, control: 11n });
    assert.equal(worker.of("menu-state").at(-1).route, 1);
    assert.equal(worker.of("menu-state").at(-1).selected, 0);
    assert.equal(worker.libraries[0].preparations.length, preparations);
    assert.equal(worker.libraries[0].accesses.length, accesses);
    assert.equal(worker.views[0].current, preview);
    assert.equal(worker.preparedOwners.length, 1);
    assert.equal(worker.of("selected").length, 1);
    assert.equal(worker.games.length, 0);
  } finally { await worker.send({ kind: "dispose" }); }
});

test("actual Worker catalog selection rejects empty and replaced retained owners atomically", async () => {
  const worker = await readyWorker({ actualMenu: true });
  try {
    await worker.send({ kind: "menu-open", fields: ["first.bms"] });
    const retired = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-open", fields: [] });
    const empty = worker.of("menu-state").at(-1);
    for (const token of [acquiredMenuToken(empty), acquiredMenuToken(retired)]) {
      const errors = worker.of("menu-error").length;
      await worker.send({ kind: "menu-select", ...token, actionId: 1n, index: 0 });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.deepEqual(worker.of("menu-state").at(-1), empty);
    }
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(empty), fields: ["available.bms", "next.bms"] });
    const populated = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-select", ...acquiredMenuToken(populated), actionId: 1n, index: 1 });
    assert.equal(worker.of("menu-state").at(-1).selected, 1);
    assert.equal(worker.preparedOwners.length, 0);
    assert.equal(worker.games.length, 0);
  } finally { await worker.send({ kind: "dispose" }); }
});

test("actual Worker Settings scalar refusal preserves the accepted draft and action ID for correction", async () => {
  const worker = await readyWorker({ actualMenu: true });
  try {
    await worker.send({ kind: "menu-open", fields: ["song/chart.bms"] });
    await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(worker.of("menu-state").at(-1)),
      route: 2, fields: settingsMenuDraft() });
    for (const [index, value] of [[0, "-1"], [4, "invalid inactive custom draft"], [6, "65537"], [12, "0"]]) {
      await worker.send({ kind: "menu-edit", ...acquiredMenuToken(worker.of("menu-state").at(-1)), index, value });
      const invalid = worker.of("menu-state").at(-1), errors = worker.of("menu-error").length;
      await worker.send({ kind: "menu-action", ...acquiredMenuToken(invalid), actionId: 1n, control: 10n });
      assert.equal(worker.of("menu-effect").length, 0);
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.deepEqual(worker.of("menu-state").at(-1), invalid, "scalar refusal changes no token or draft");
      await worker.send({ kind: "menu-edit", ...acquiredMenuToken(invalid), index, value: settingsMenuDraft()[index] });
    }
    const corrected = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(corrected), actionId: 1n, control: 10n });
    assert.equal(worker.of("menu-effect").length, 1, "refused Apply never consumes the semantic action ID");
    assert.deepEqual(Array.from(worker.of("menu-effect")[0].fields), settingsMenuDraft());
  } finally { await worker.send({ kind: "dispose" }); }
});

test("actual Worker Settings pending and stale tokens cannot produce Apply effects", async () => {
  const worker = await readyWorker({ actualMenu: true });
  try {
    await worker.send({ kind: "menu-open", fields: ["song/chart.bms"] });
    await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(worker.of("menu-state").at(-1)), route: 2, fields: [] });
    const pending = worker.of("menu-state").at(-1);
    assert.deepEqual(Array.from(pending.fields), [], "the Settings draft is still pending its field acquisition");
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(pending), actionId: 1n, control: 10n });
    assert.equal(worker.of("menu-effect").length, 0);
    assert.deepEqual(worker.of("menu-state").at(-1), pending);
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(pending), fields: settingsMenuDraft() });
    const accepted = worker.of("menu-state").at(-1);
    for (const wrong of [{ menuGeneration: accepted.menuGeneration + 1n }, { screen: accepted.screen + 1n },
      { revision: pending.revision }]) {
      await worker.send({ kind: "menu-action", ...acquiredMenuToken(accepted), ...wrong, actionId: 2n, control: 10n });
      assert.equal(worker.of("menu-effect").length, 0);
      assert.deepEqual(worker.of("menu-state").at(-1), accepted);
    }
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(accepted), actionId: 2n, control: 10n });
    assert.equal(worker.of("menu-effect").length, 1);
  } finally { await worker.send({ kind: "dispose" }); }
});
const acquiredSource = (id, kind = "hid") => [String(id), kind, `Acquired ${id}`, `Original source ${id}`, "1"];
function acquiredMenuFields(sources, members = [[999, "42"]]) {
  return ["1", "1", String(members.length), ...members.flatMap(([id, source]) => [String(id), source]),
    String(sources.length), ...sources.flat()];
}
async function acquiredPlayersWorker(options = {}) {
  const worker = await readyWorker({ ...options, actualMenu: true });
  const roster = new LocalRoster();
  roster.importState({ players: [7, 11], nextPlayerId: 12, assignments: [] });
  await worker.send({ kind: "menu-open", fields: ["song/chart.bms"], roster: roster.exportState() });
  await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(worker.of("menu-state").at(-1)), route: 2, fields: ["accepted settings"] });
  await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(worker.of("menu-state").at(-1)), route: 5,
    fields: acquiredMenuFields([acquiredSource(1, "keyboard"), acquiredSource(41), acquiredSource(42)]) });
  return { worker, roster };
}

test("actual Worker acquired inventory retires only absent assignments while preserving canonical player IDs", async () => {
  const { worker, roster } = await acquiredPlayersWorker();
  try {
    for (const [player, source] of [[7, 1n], [11, 41n]]) {
      await worker.send({ kind: "menu-roster-assign", ...acquiredMenuToken(worker.of("menu-state").at(-1)), player, source });
      roster.assign(player, source);
    }
    const assigned = worker.of("menu-state").at(-1);
    assert.deepEqual(structuredClone(assigned.roster), structuredClone(roster.exportState()));
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(assigned), fields: acquiredMenuFields([acquiredSource(41)]) });
    roster.assign(7, null);
    const partial = worker.of("menu-state").at(-1);
    assert.equal(partial.screen, assigned.screen);
    assert.equal(partial.route, 5);
    assert.ok(partial.revision > assigned.revision);
    assert.deepEqual(structuredClone(partial.roster), structuredClone(roster.exportState()));
    assert.deepEqual(Array.from(partial.fields.slice(0, 8)), ["1", "1", "2", "7", "", "11", "41", "1"],
      "Window member IDs and assignments cannot overwrite the canonical roster");
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(partial), actionId: 1n, control: 34n });
    const devices = worker.of("menu-state").at(-1);
    assert.equal(devices.route, 9);
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(devices), fields: acquiredMenuFields([]) });
    roster.assign(11, null);
    const empty = worker.of("menu-state").at(-1);
    assert.equal(empty.screen, devices.screen);
    assert.equal(empty.route, 9);
    assert.deepEqual(structuredClone(empty.roster), structuredClone(roster.exportState()));
    assert.deepEqual(Array.from(empty.fields), ["1", "1", "2", "7", "", "11", "", "0"]);
    await worker.send({ kind: "menu-roster-count", ...acquiredMenuToken(empty), count: 3 });
    roster.setCount(3);
    assert.deepEqual(structuredClone(worker.of("menu-state").at(-1).roster), structuredClone(roster.exportState()));
    assert.deepEqual(Array.from(worker.of("menu-state").at(-1).roster.players), [7, 11, 12]);
    assert.equal(worker.of("menu-error").length, 0);
  } finally { await worker.send({ kind: "dispose" }); }
});

test("actual Worker rejects stale and malformed acquired inventories without publishing a valid prefix", async () => {
  const { worker } = await acquiredPlayersWorker();
  try {
    await worker.send({ kind: "menu-roster-assign", ...acquiredMenuToken(worker.of("menu-state").at(-1)), player: 7, source: 1n });
    const accepted = worker.of("menu-state").at(-1);
    const menuPublications = () => worker.renderPort.posts.filter(post => post.kind === "menu").length;
    const publicationCount = menuPublications();
    const malformed = [
      acquiredMenuFields([acquiredSource(41), acquiredSource(41)]),
      acquiredMenuFields([acquiredSource(41, "native-path")]),
      acquiredMenuFields([["41", "hid", "source", "detail", "2"]]),
      ["2", ...acquiredMenuFields([acquiredSource(41)]).slice(1)],
      ["1", "2", ...acquiredMenuFields([acquiredSource(41)]).slice(2)],
      [...acquiredMenuFields([acquiredSource(41)]), "foreign trailing field"],
      acquiredMenuFields([acquiredSource(41)]).slice(0, -1),
      ["1", "1", "0", "1025"],
    ];
    for (const fields of malformed) {
      const errors = worker.of("menu-error").length;
      await worker.send({ kind: "menu-fields", ...acquiredMenuToken(accepted), fields });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.deepEqual(worker.of("menu-state").at(-1), accepted);
      assert.equal(menuPublications(), publicationCount);
    }
    for (const token of [
      { ...acquiredMenuToken(accepted), menuGeneration: accepted.menuGeneration + 1n },
      { ...acquiredMenuToken(accepted), screen: accepted.screen + 1n },
      { ...acquiredMenuToken(accepted), revision: accepted.revision - 1n },
    ]) {
      const errors = worker.of("menu-error").length;
      await worker.send({ kind: "menu-fields", ...token, fields: acquiredMenuFields([]) });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.deepEqual(worker.of("menu-state").at(-1), accepted);
    }
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(accepted), fields: acquiredMenuFields([acquiredSource(1, "keyboard")]) });
    const refreshed = worker.of("menu-state").at(-1);
    assert.deepEqual(structuredClone(refreshed.roster.assignments), [[7, 1n]]);
    assert.equal(refreshed.screen, accepted.screen);
    assert.ok(refreshed.revision > accepted.revision);
  } finally { await worker.send({ kind: "dispose" }); }
});

test("actual Worker keeps roster and fields when genuine owner admission or pending operation refuses inventory refresh", async () => {
  let fault = null;
  const { worker } = await acquiredPlayersWorker({ menuAdmissionFault(method, owner, args) {
    if (fault === "domain" && method === "navigate_with_fields" && args[2] === owner.route) {
      throw new Error("injected WASM admission refusal");
    }
    if (fault === "pending" && method === "set_fields") throw new Error("injected pending owner refusal");
  } });
  try {
    await worker.send({ kind: "menu-roster-assign", ...acquiredMenuToken(worker.of("menu-state").at(-1)), player: 7, source: 1n });
    const accepted = worker.of("menu-state").at(-1);
    const publications = worker.renderPort.posts.filter(post => post.kind === "menu").length;
    for (const kind of ["domain", "pending"]) {
      fault = kind;
      const errors = worker.of("menu-error").length;
      await worker.send({ kind: "menu-fields", ...acquiredMenuToken(accepted), fields: acquiredMenuFields([]) });
      assert.equal(worker.of("menu-error").length, errors + 1);
      assert.deepEqual(worker.of("menu-state").at(-1), accepted);
      assert.equal(worker.renderPort.posts.filter(post => post.kind === "menu").length, publications);
    }
    fault = null;
    let chartReads = 0;
    await worker.send({ kind: "import", id: 1, files: [selectedFile("pending/chart.bms", "#BPM 120", () => {
      chartReads++; throw new Error("catalog admission must not acquire this chart");
    })] });
    assert.equal(chartReads, 0);
    await worker.send({ kind: "accept-library", id: 1 });
    // Metadata publication has no pending byte acquisition. Use an actual
    // unabortable profile read to retain the original idle-owner refusal case.
    const read = deferred();
    const profile = validSettingsProfileText();
    const profileFile = selectedFile("pending.json", profile, () => read.promise).file;
    await worker.send({ kind: "settings-profile-load", id: 1, menuToken: acquiredMenuToken(accepted), file: profileFile });
    assert.equal(worker.of("settings-profile-loaded").length, 0);
    const pending = worker.of("menu-state").at(-1);
    assertProfileTransition(pending, accepted, 1, true);
    assert.deepEqual(Array.from(pending.fields), Array.from(accepted.fields));
    assert.deepEqual(structuredClone(pending.roster), structuredClone(accepted.roster));
    const pendingPublications = worker.renderPort.posts.filter(post => post.kind === "menu").length;
    assert.equal(pendingPublications, publications + 1, "typed pending state publishes exactly once");
    const errors = worker.of("menu-error").length;
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(pending), fields: acquiredMenuFields([]) });
    assert.equal(worker.of("menu-error").length, errors + 1);
    assert.deepEqual(worker.of("menu-state").at(-1), pending);
    assert.equal(worker.renderPort.posts.filter(post => post.kind === "menu").length, pendingPublications);
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(pending), actionId: 1n, control: 35n });
    assert.equal(worker.of("menu-error").length, errors + 2);
    assert.deepEqual(worker.of("menu-state").at(-1), pending);
    assert.equal(worker.renderPort.posts.filter(post => post.kind === "menu").length, pendingPublications);
    read.resolve(new TextEncoder().encode(profile).buffer);
    await flushJobs();
    const candidate = worker.of("settings-profile-candidate").at(-1);
    assert.equal(candidate.id, 1);
    assert.deepEqual(acquiredMenuToken(candidate.menuToken), acquiredMenuToken(pending));
    assert.equal(worker.of("settings-profile-loaded").length, 0);
    assert.deepEqual(worker.of("menu-state").at(-1), pending);
    assert.equal(worker.renderPort.posts.filter(post => post.kind === "menu").length, pendingPublications);
    await worker.send({ kind: "settings-profile-admit", id: 1, menuToken: acquiredMenuToken(pending) });
    const committedProfile = worker.of("menu-state").at(-1);
    assertProfileTransition(committedProfile, pending, 1, false);
    assert.deepEqual(Array.from(committedProfile.fields), Array.from(accepted.fields));
    assert.deepEqual(structuredClone(committedProfile.roster), structuredClone(accepted.roster));
    assert.equal(worker.renderPort.posts.filter(post => post.kind === "menu").length, pendingPublications + 1);
    assert.equal(worker.of("settings-profile-loaded").length, 1);
    assert.deepEqual(acquiredMenuToken(worker.of("settings-profile-loaded")[0].admittedToken), acquiredMenuToken(pending));
    assert.deepEqual(acquiredMenuToken(worker.of("settings-profile-loaded")[0].resultToken), acquiredMenuToken(committedProfile));
    assert.equal(worker.of("settings-profile-error").length, 0);
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(committedProfile), fields: acquiredMenuFields([]) });
    const released = worker.of("menu-state").at(-1);
    assert.deepEqual(Array.from(released.roster.assignments), []);
    assert.deepEqual(Array.from(released.roster.players), [7, 11]);
    assert.equal(released.roster.nextPlayerId, 12);
    assert.deepEqual(Array.from(released.fields), ["1", "1", "2", "7", "", "11", "", "0"],
      "inventory refresh keeps the full canonical two-player field projection");
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(released), actionId: 1n, control: 35n });
    assert.equal(worker.of("menu-error").length, errors + 2, "pending refusal must not consume an action identity");
    const committed = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(committed), actionId: 1n, control: 35n });
    assert.equal(worker.of("menu-error").length, errors + 3, "committed action identity cannot be reused");
  } finally { await worker.send({ kind: "dispose" }); }
});

test("actual Worker non-assignment Devices field updates cannot retire canonical source ownership", async () => {
  const { worker } = await acquiredPlayersWorker();
  try {
    await worker.send({ kind: "menu-roster-assign", ...acquiredMenuToken(worker.of("menu-state").at(-1)), player: 7, source: 1n });
    const players = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(players), actionId: 1n, control: 31n });
    const settings = worker.of("menu-state").at(-1);
    assert.equal(settings.route, 2);
    await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(settings), route: 6,
      fields: acquiredMenuFields([acquiredSource(1, "keyboard")]) });
    const devices = worker.of("menu-state").at(-1);
    assert.equal(devices.route, 6);
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(devices), fields: acquiredMenuFields([]) });
    const refreshed = worker.of("menu-state").at(-1);
    assert.equal(refreshed.route, 6);
    assert.equal(refreshed.screen, devices.screen);
    assert.deepEqual(structuredClone(refreshed.roster), structuredClone(players.roster),
      "only Players and assignment-picker refreshes retire canonical assignments");
    assert.equal(worker.of("menu-error").length, 0);
  } finally { await worker.send({ kind: "dispose" }); }
});

test("game Worker uses actual menu owner for Settings Practice Back and keeps retained parent draft", async () => {
  const worker = await readyWorker({ actualMenu: true });
  await worker.send({ kind: "menu-open", fields: ["Songs/曲/chart.bms"] });
  const selection = worker.of("menu-state").at(-1); assert.equal(selection.route, 1);
  const token = state => ({ menuGeneration: state.menuGeneration, screen: state.screen, revision: state.revision });
  await worker.send({ kind: "menu-navigate", ...token(selection), route: 2, fields: ["accepted settings draft"] });
  const settings = worker.of("menu-state").at(-1); assert.equal(settings.route, 2);
  await worker.send({ kind: "menu-navigate", ...token(settings), route: 3, fields: ["1.000000001", "2.000000002"] });
  const practice = worker.of("menu-state").at(-1); assert.equal(practice.route, 3);
  await worker.send({ kind: "menu-edit", ...token(practice), index: 0, value: "12.345678901" });
  const edited = worker.of("menu-state").at(-1);
  await worker.send({ kind: "menu-action", ...token(edited), actionId: 1n, control: 72n });
  const returned = worker.of("menu-state").at(-1);
  assert.equal(returned.route, 2); assert.equal(returned.screen, settings.screen);
  assert.deepEqual(returned.fields, settings.fields, "Back must discard child edits without business Apply");
  assert.equal(worker.games.length, 0); assert.equal(worker.preparedOwners.length, 0);
  assert.ok(worker.renderPort.posts.some(message => message.kind === "menu"), "models cross direct renderer channel");
  await worker.send({ kind: "dispose" });
});

test("stale menu actions cannot prepare a game or mutate a newer retained screen", async () => {
  const worker = await readyWorker({ actualMenu: true });
  await worker.send({ kind: "menu-open", fields: ["Songs/曲/chart.bms"] });
  const old = worker.of("menu-state").at(-1);
  await worker.send({ kind: "menu-navigate", menuGeneration: old.menuGeneration,
    screen: old.screen, revision: old.revision, route: 2, fields: [] });
  const current = worker.of("menu-state").at(-1);
  for (const request of [
    { kind: "menu-action", actionId: 1n, control: 1n },
    { kind: "menu-edit", index: 0, value: "forged" },
    { kind: "menu-navigate", route: 8, fields: [] },
  ]) await worker.send({ ...request, menuGeneration: old.menuGeneration, screen: old.screen, revision: old.revision });
  assert.deepEqual(worker.of("menu-state").at(-1), current);
  assert.equal(worker.games.length, 0); assert.equal(worker.preparedOwners.length, 0);
  await worker.send({ kind: "dispose" });
});

test("actual Worker malformed navigation preserves accepted token draft and next valid action before retry", async () => {
  const worker = await readyWorker({ actualMenu: true });
  const reference = await readyWorker({ actualMenu: true });
  const token = state => ({ menuGeneration: state.menuGeneration, screen: state.screen, revision: state.revision });
  try {
    for (const value of [worker, reference]) {
      await value.send({ kind: "menu-open", fields: ["Songs/曲/chart.bms"] });
      await value.send({ kind: "menu-navigate", ...token(value.of("menu-state").at(-1)), route: 2, fields: ["accepted settings draft"] });
    }
    const accepted = worker.of("menu-state").at(-1);
    const publications = worker.renderPort.posts.filter(message => message.kind === "menu").length;
    for (const [route, fields] of [
      [5, []], [6, ["1", "1", "65", "0"]], [3, ["0"]],
      [7, ["auto", "fifo", "960"]], [4, Array(257).fill("record")],
      [9, ["1", "1", "1", "1", "", "1", "1", "forged-kind", "source", "detail", "1"]],
    ]) {
      const failures = worker.of("menu-error").length;
      await worker.send({ kind: "menu-navigate", ...token(accepted), route, fields });
      assert.equal(worker.of("menu-error").length, failures + 1);
      assert.deepEqual(worker.of("menu-state").at(-1), accepted);
      assert.equal(worker.renderPort.posts.filter(message => message.kind === "menu").length, publications);
      assert.equal(worker.games.length, 0); assert.equal(worker.preparedOwners.length, 0);
    }
    for (const value of [worker, reference]) {
      await value.send({ kind: "menu-action", ...token(value.of("menu-state").at(-1)), actionId: 1n, control: 74n });
    }
    const practice = worker.of("menu-state").at(-1);
    assert.equal(practice.route, 3);
    assert.deepEqual(structuredClone(practice), structuredClone(reference.of("menu-state").at(-1)), "refused navigation consumes no next screen identity or revision");
    for (const value of [worker, reference]) {
      await value.send({ kind: "menu-action", ...token(value.of("menu-state").at(-1)), actionId: 2n, control: 72n });
    }
    const restored = worker.of("menu-state").at(-1);
    assert.equal(restored.screen, accepted.screen); assert.deepEqual(restored.fields, accepted.fields);
    await worker.send({ kind: "menu-navigate", ...token(restored), route: 3, fields: ["1.000000001", "2.000000002"] });
    const retried = worker.of("menu-state").at(-1);
    assert.equal(retried.route, 3); assert.deepEqual(Array.from(retried.fields), ["1.000000001", "2.000000002"]);
    await worker.send({ kind: "menu-action", ...token(retried), actionId: 3n, control: 72n });
    const back = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-navigate", ...token(back), route: 3 });
    assert.equal(worker.of("menu-state").at(-1).route, 3, "fields-absent legacy navigation keeps its original intent");
  } finally {
    await worker.send({ kind: "dispose" }); await reference.send({ kind: "dispose" });
  }
});

test("actual Worker routes all supported shared menu capabilities without inventing Results or native ownership", async () => {
  const worker = await readyWorker({ actualMenu: true });
  const roster = { players: [7, 4294967295], nextPlayerId: 4294967296, assignments: [] };
  const rosterFields = ["1", "1", "2", "7", "", "4294967295", "", "1",
    "1", "keyboard", "Browser keyboard", "Original acquired keyboard source", "1"];
  await worker.send({ kind: "menu-open", fields: ["Songs/曲/chart.bms"], roster });
  const token = state => ({ menuGeneration: state.menuGeneration, screen: state.screen, revision: state.revision });
  const selection = worker.of("menu-state").at(-1);
  await worker.send({ kind: "menu-navigate", ...token(selection), route: 2, fields: [] });
  let actionId = 1n;
  for (const [route, back] of [[3, 72n], [4, 55n], [5, 31n], [6, 21n], [7, 41n]]) {
    const parent = worker.of("menu-state").at(-1);
    await worker.send({ kind: "menu-navigate", ...token(parent), route,
      fields: [5, 6].includes(route) ? rosterFields : route === 3 ? ["0", ""] : route === 7 ? ["auto", "fifo", "960", "720"] : [] });
    const child = worker.of("menu-state").at(-1); assert.equal(child.route, route);
    if ([5, 6].includes(route)) assert.deepEqual(structuredClone(child.roster), roster, "capability projections retain actual original player IDs");
    await worker.send({ kind: "menu-action", ...token(child), actionId: actionId++, control: back });
    const returned = worker.of("menu-state").at(-1);
    assert.equal(returned.route, 2); assert.equal(returned.screen, parent.screen);
  }
  const settings = worker.of("menu-state").at(-1);
  await worker.send({ kind: "menu-navigate", ...token(settings), route: 5, fields: rosterFields });
  const players = worker.of("menu-state").at(-1);
  await worker.send({ kind: "menu-action", ...token(players), actionId: actionId++, control: 34n });
  const devices = worker.of("menu-state").at(-1); assert.equal(devices.route, 9);
  await worker.send({ kind: "menu-action", ...token(devices), actionId: actionId++, control: 21n });
  const restored = worker.of("menu-state").at(-1); assert.equal(restored.screen, players.screen);
  await worker.send({ kind: "menu-navigate", ...token(restored), route: 8, fields: [] });
  assert.deepEqual(worker.of("menu-state").at(-1), restored);
  assert.equal(worker.games.length, 0); assert.equal(worker.preparedOwners.length, 0);
  await worker.send({ kind: "dispose" });
});

function opponentFile(name, values = [66, 75, 82, 1], read) {
  const bytes = Uint8Array.from(values);
  const file = new FileType([bytes], name);
  let reads = 0;
  file.arrayBuffer = () => { reads++; return read ? read() : Promise.resolve(bytes.slice().buffer); };
  return { file, bytes, get reads() { return reads; } };
}
function opponentChoice(selected, sourceKey, own = true) {
  return { file: selected.file, sourceKey, own, label: selected.file.name };
}
async function gameWorker(options = {}) {
  const worker = await readyWorker({ ...options, gameplay: true });
  await worker.send({ kind: "import", id: 1, files: [selectedFile("song/chart.bms")] });
  await worker.send({ kind: "accept-library", id: 1 });
  return worker;
}
function startGame(worker, opponents, extra = {}) {
  return worker.send({ kind: "play-start", playId: 1, rpcId: 1, libraryId: 1,
    path: "song/chart.bms", rate: 48000, seed: "0", keyPairs: new Uint32Array([0x11, 4]),
    opponents, windowOriginNs: 0n, ...extra });
}

function trackedSelected(path, content, log, acquire) {
  const bytes = new TextEncoder().encode(content);
  return selectedFile(path, content, () => {
    log.push(path);
    return acquire ? acquire(bytes) : Promise.resolve(bytes.slice().buffer);
  });
}

test("metadata catalog precedes every file read including unrelated hung and failing media", async () => {
  const reads = [], held = deferred();
  const worker = await readyWorker({ references: { "song/chart.bms": ["song/kick.wav", "song/bg.png", "song/kick.wav"] } });
  await worker.send({ kind: "import", id: 1, files: [
    trackedSelected("song/chart.bms", "#BPM 120", reads),
    trackedSelected("song/kick.wav", "pcm", reads),
    trackedSelected("song/bg.png", "image", reads),
    trackedSelected("other/hung.wav", "hung", reads, () => held.promise),
    trackedSelected("other/broken.wav", "broken", reads, () => { throw new Error("unrelated media must stay untouched"); }),
    trackedSelected("other/chart.bms", "unselected", reads),
  ] });
  assert.deepEqual(reads, []);
  assert.equal(worker.libraries[0].files.length, 0);
  assert.equal(worker.libraries[0].declarations.size, 6);
  assert.deepEqual(Array.from(worker.of("catalog")[0].charts).sort(), ["other/chart.bms", "song/chart.bms"]);
  await worker.send({ kind: "accept-library", id: 1 });
  assert.deepEqual(reads, []);
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "7" });
  assert.deepEqual(reads, ["song/chart.bms", "song/kick.wav", "song/bg.png"]);
  assert.deepEqual(worker.libraries[0].plans, [{ method: "live", path: "song/chart.bms", seed: 7n, maxSamples: 3844 }]);
  assert.equal(worker.of("selected").at(-1).id, 2);
  assert.equal(worker.of("selection-error").length, 0);
  assert.equal(worker.libraries[0].files.length, 3);
  await worker.send({ kind: "dispose" });
  assert.equal(worker.libraries[0].frees, 1);
});

test("overlapping selections share pending acquisition but only the newest selection hydrates and publishes", async () => {
  const reads = [], held = deferred();
  const worker = await readyWorker({ references: { "song/chart.bms": ["song/kick.wav"] } });
  await worker.send({ kind: "import", id: 1, files: [
    trackedSelected("song/chart.bms", "#BPM 120", reads),
    trackedSelected("song/kick.wav", "pcm", reads, () => held.promise),
  ] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "1" });
  await worker.send({ kind: "select", id: 3, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "2" });
  assert.deepEqual(reads, ["song/chart.bms", "song/kick.wav"]);
  assert.equal(worker.libraries[0].preparations.length, 0);
  held.resolve(new TextEncoder().encode("pcm").buffer);
  await worker.flushTasks();
  assert.deepEqual(worker.of("selected").map(row => row.id), [3]);
  assert.equal(worker.of("selection-error").length, 0);
  assert.equal(worker.libraries[0].preparations.length, 1);
  assert.equal(worker.libraries[0].preparations[0].args[2], 2n);
  assert.equal(worker.libraries[0].files.filter(row => row.path === "song/kick.wav").length, 1);
  await worker.send({ kind: "dispose" });
});

test("late obsolete chart acquisition cannot prepare or replace a newer selected chart", async () => {
  const reads = [], held = deferred();
  const worker = await readyWorker();
  await worker.send({ kind: "import", id: 1, files: [
    trackedSelected("old.bms", "old", reads, () => held.promise),
    trackedSelected("new.bms", "new", reads),
  ] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "old.bms", rate: 48000, seed: "0" });
  await worker.send({ kind: "select", id: 3, libraryId: 1, path: "new.bms", rate: 48000, seed: "0" });
  assert.equal(worker.of("selected").at(-1).path, "new.bms");
  held.resolve(new TextEncoder().encode("old").buffer);
  await flushJobs();
  assert.deepEqual(worker.of("selected").map(row => row.id), [3]);
  assert.deepEqual(worker.libraries[0].files.map(row => row.path), ["new.bms"]);
  assert.equal(worker.of("selection-error").length, 0);
  assert.deepEqual(reads, ["old.bms", "new.bms"]);
  await worker.send({ kind: "dispose" });
});

test("a newer import fences old selection before ACK without prematurely freeing the accepted library", async () => {
  const reads = [], held = deferred();
  const worker = await readyWorker();
  await worker.send({ kind: "import", id: 1, files: [trackedSelected("old.bms", "old", reads, () => held.promise)] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "old.bms", rate: 48000, seed: "0" });
  const accepted = worker.libraries[0];
  await worker.send({ kind: "import", id: 3, files: [trackedSelected("new.bms", "new", reads)] });
  held.resolve(new TextEncoder().encode("old").buffer);
  await flushJobs();
  assert.equal(accepted.frees, 0);
  assert.equal(accepted.files.length, 0);
  assert.equal(accepted.preparations.length, 0);
  assert.equal(worker.of("selected").length, 0);
  assert.equal(worker.of("selection-error").length, 0);
  assert.deepEqual(reads, ["old.bms"]);
  await worker.send({ kind: "accept-library", id: 3 });
  assert.equal(accepted.frees, 1);
  await worker.send({ kind: "select", id: 4, libraryId: 3, path: "new.bms", rate: 48000, seed: "0" });
  assert.equal(worker.of("selected").at(-1).id, 4);
  await worker.send({ kind: "dispose" });
});

test("preview validation refuses invalid rate and seed before acquiring declared chart bytes", async () => {
  const reads = [];
  const worker = await readyWorker();
  await worker.send({ kind: "import", id: 1, files: [trackedSelected("song/chart.bms", "chart", reads)] });
  await worker.send({ kind: "accept-library", id: 1 });
  const invalid = [{ rate: 0 }, { rate: 48000.5 }, { seed: "-1" }, { seed: "18446744073709551616" }];
  for (const [index, fields] of invalid.entries()) {
    await worker.send({ kind: "select", id: index + 2, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0", ...fields });
    assert.equal(worker.of("selection-error").at(-1).id, index + 2);
  }
  assert.deepEqual(reads, []);
  assert.equal(worker.libraries[0].plans.length, 0);
  assert.equal(worker.libraries[0].preparations.length, 0);
  await worker.send({ kind: "dispose" });
});

test("wrong acquisition extent or buffer type rejects selected preparation and can retry without a stuck pending read", async () => {
  for (const bad of [() => new ArrayBuffer(1), () => new Uint8Array(3)]) {
    const reads = []; let attempts = 0;
    const worker = await readyWorker({ references: { "song/chart.bms": ["song/kick.wav"] } });
    await worker.send({ kind: "import", id: 1, files: [
      trackedSelected("good.bms", "good", reads),
      trackedSelected("song/chart.bms", "chart", reads),
      trackedSelected("song/kick.wav", "pcm", reads, bytes => Promise.resolve(++attempts === 1 ? bad() : bytes.slice().buffer)),
    ] });
    await worker.send({ kind: "accept-library", id: 1 });
    await worker.send({ kind: "select", id: 2, libraryId: 1, path: "good.bms", rate: 48000, seed: "0" });
    const original = worker.views[0].current;
    await worker.send({ kind: "select", id: 3, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" });
    assert.equal(worker.views[0].current, original);
    assert.equal(worker.of("selection-error").at(-1).id, 3);
    assert.equal(worker.libraries[0].files.some(row => row.path === "song/kick.wav"), false);
    assert.equal(worker.libraries[0].preparations.length, 1);
    await worker.send({ kind: "select", id: 4, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" });
    assert.equal(worker.of("selected").at(-1).id, 4);
    assert.equal(attempts, 2);
    assert.equal(reads.filter(path => path === "song/chart.bms").length, 1);
    assert.equal(worker.libraries[0].preparations.length, 2);
    await worker.send({ kind: "dispose" });
  }
});

test("ACK retirement and disposal fence every continuation of an outstanding library read", async () => {
  for (const action of ["replace", "dispose"]) {
    for (const settlement of ["resolve", "reject"]) {
      const reads = [], held = deferred();
      const worker = await readyWorker({ references: { "old.bms": ["late.wav", "never.wav"] } });
      await worker.send({ kind: "import", id: 1, files: [
        trackedSelected("old.bms", "old", reads),
        trackedSelected("late.wav", "pcm", reads, () => held.promise),
        trackedSelected("never.wav", "next", reads),
      ] });
      await worker.send({ kind: "accept-library", id: 1 });
      await worker.send({ kind: "select", id: 2, libraryId: 1, path: "old.bms", rate: 48000, seed: "0" });
      const retired = worker.libraries[0];
      if (action === "replace") {
        await worker.send({ kind: "import", id: 3, files: [trackedSelected("new.bms", "new", reads)] });
        assert.equal(retired.frees, 0, "a proposal alone cannot release the admitted library");
        await worker.send({ kind: "accept-library", id: 3 });
        await worker.send({ kind: "select", id: 4, libraryId: 3, path: "new.bms", rate: 48000, seed: "0" });
      } else await worker.send({ kind: "dispose" });
      assert.equal(retired.frees, 1);
      if (settlement === "resolve") held.resolve(new TextEncoder().encode("pcm").buffer);
      else held.reject(new Error("obsolete late read failed"));
      await flushJobs();
      assert.equal(retired.frees, 1);
      assert.deepEqual(retired.files.map(row => row.path), ["old.bms"]);
      assert.equal(reads.includes("never.wav"), false);
      assert.equal(worker.of("selection-error").length, 0);
      assert.equal(worker.of("fatal").length, 0);
      assert.deepEqual(worker.of("selected").map(row => row.id), action === "replace" ? [4] : []);
      if (action === "replace") await worker.send({ kind: "dispose" });
    }
  }
});

test("Stop during selected gameplay hydration cannot admit into a replacement play owner", async () => {
  const reads = [], held = deferred();
  const worker = await readyWorker({ gameplay: true, references: { "song/chart.bms": ["song/held.wav"] } });
  await worker.send({ kind: "import", id: 1, files: [
    trackedSelected("song/chart.bms", "chart", reads),
    trackedSelected("song/held.wav", "pcm", reads, () => held.promise),
    trackedSelected("other.bms", "other", reads),
  ] });
  await worker.send({ kind: "accept-library", id: 1 });
  await startGame(worker, []);
  assert.equal(worker.games.length, 0);
  await worker.send({ kind: "play-stop", playId: 1 });
  await startGame(worker, [], { playId: 2, path: "other.bms" });
  assert.equal(worker.games.length, 1);
  held.resolve(new TextEncoder().encode("pcm").buffer);
  await flushJobs();
  assert.equal(worker.games.length, 1);
  assert.equal(worker.games[0].prepared.path, "other.bms");
  assert.equal(worker.of("play-reply").filter(row => row.result?.kind === "prepared").length, 1);
  assert.equal(worker.of("play-error").length, 0);
  assert.equal(worker.libraries[0].files.some(row => row.path === "song/held.wav"), false);
  await worker.send({ kind: "play-stop", playId: 2 });
  await worker.send({ kind: "dispose" });
});

test("a pending preview cannot revive after an entire play reservation and Stop cycle", async () => {
  for (const settlement of ["resolve", "reject"]) {
    const reads = [], held = deferred();
    const worker = await readyWorker({ gameplay: true });
    await worker.send({ kind: "import", id: 1, files: [
      trackedSelected("pending.bms", "pending", reads, () => held.promise),
      trackedSelected("cached.bms", "cached", reads),
    ] });
    await worker.send({ kind: "accept-library", id: 1 });
    await worker.send({ kind: "select", id: 2, libraryId: 1, path: "cached.bms", rate: 48000, seed: "0" });
    const accepted = worker.views[0].current;
    await worker.send({ kind: "select", id: 3, libraryId: 1, path: "pending.bms", rate: 48000, seed: "0" });
    await startGame(worker, [], { path: "cached.bms" });
    assert.equal(worker.games.length, 1);
    assert.equal(worker.of("play-reply").at(-1).result.kind, "prepared");
    await worker.send({ kind: "play-stop", playId: 1 });
    assert.equal(worker.of("play-stopped").length, 1);
    const accessCount = worker.libraries[0].accesses.length;
    const preparationCount = worker.libraries[0].preparations.length;
    if (settlement === "resolve") held.resolve(new TextEncoder().encode("pending").buffer);
    else held.reject(new Error("obsolete preview read failure"));
    await flushJobs();
    assert.equal(worker.libraries[0].accesses.length, accessCount, "a completed play cycle cannot revive a prior preview's WASM access");
    assert.equal(worker.libraries[0].preparations.length, preparationCount);
    assert.equal(worker.libraries[0].files.some(row => row.path === "pending.bms"), false);
    assert.deepEqual(worker.of("selected").map(row => row.id), [2]);
    assert.equal(worker.of("selection-error").length, 0);
    assert.equal(worker.of("fatal").length, 0);
    assert.equal(worker.views[0].current, accepted);
    await worker.send({ kind: "dispose" });
  }
});

test("a pending preview cannot revive after an entire settings load or save reservation", async () => {
  for (const operation of ["load", "save"]) {
    for (const settlement of ["resolve", "reject"]) {
      const reads = [], held = deferred(), profileRead = deferred();
      const worker = await readyWorker();
      await worker.send({ kind: "import", id: 1, files: [trackedSelected("pending.bms", "pending", reads, () => held.promise)] });
      await worker.send({ kind: "accept-library", id: 1 });
      await worker.send({ kind: "select", id: 2, libraryId: 1, path: "pending.bms", rate: 48000, seed: "0" });
      const profile = validSettingsProfileText();
      if (operation === "load") {
        await worker.send({ kind: "settings-profile-load", id: 1, file: selectedFile("valid.json", profile, () => profileRead.promise).file });
        assert.equal(worker.of("settings-profile-loaded").length, 0);
        profileRead.resolve(new TextEncoder().encode(profile).buffer);
        await flushJobs();
        assert.equal(worker.of("settings-profile-loaded").length, 1);
      } else {
        await worker.send({ kind: "settings-profile-save", id: 1, settings: JSON.parse(profile) });
        assert.equal(worker.of("settings-profile-saved").length, 1);
      }
      assert.equal(worker.of("settings-profile-error").length, 0);
      const accessCount = worker.libraries[0].accesses.length;
      if (settlement === "resolve") held.resolve(new TextEncoder().encode("pending").buffer);
      else held.reject(new Error("obsolete preview read failure"));
      await flushJobs();
      assert.equal(worker.libraries[0].accesses.length, accessCount, "completed settings ownership must still fence a prior preview");
      assert.equal(worker.of("selected").length, 0);
      assert.equal(worker.of("selection-error").length, 0);
      assert.equal(worker.libraries[0].preparations.length, 0);
      assert.equal(worker.of("fatal").length, 0);
      await worker.send({ kind: "dispose" });
    }
  }
});

test("invalid settings and play requests do not cancel an otherwise valid pending preview", async () => {
  const reads = [], held = deferred();
  const worker = await readyWorker({ gameplay: true });
  await worker.send({ kind: "import", id: 1, files: [trackedSelected("pending.bms", "pending", reads, () => held.promise)] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "pending.bms", rate: 48000, seed: "0" });
  await worker.send({ kind: "settings-profile-save", id: 1, settings: {} });
  await worker.send({ kind: "settings-profile-load", id: 2, file: { size: 1, arrayBuffer() { assert.fail("invalid non-File must never be acquired"); } } });
  await worker.send({ kind: "settings-profile-load", id: 0, file: selectedFile("unused.json", validSettingsProfileText(), () => { assert.fail("invalid identity must not acquire"); }).file });
  await startGame(worker, [], { playId: 0 });
  assert.equal(worker.of("settings-profile-error").length, 3);
  assert.equal(worker.games.length, 0);
  held.resolve(new TextEncoder().encode("pending").buffer);
  await worker.flushTasks();
  assert.deepEqual(worker.of("selected").map(row => row.id), [2]);
  assert.equal(worker.of("selection-error").length, 0);
  assert.equal(worker.libraries[0].preparations.length, 1);
  await worker.send({ kind: "dispose" });
});

test("pending record acquisition cannot revive the same menu after a play and Stop cycle", async () => {
  for (const settlement of ["resolve", "reject"]) {
    const reads = [], held = deferred(); let admissions = 0;
    const worker = await readyWorker({ gameplay: true, actualMenu: true, observeRecordPreview() { admissions++; } });
    await worker.send({ kind: "import", id: 1, files: [
      trackedSelected("pending.bms", "pending", reads, () => held.promise),
      trackedSelected("cached.bms", "cached", reads),
    ] });
    await worker.send({ kind: "accept-library", id: 1 });
    await worker.send({ kind: "select", id: 2, libraryId: 1, path: "cached.bms", rate: 48000, seed: "0" });
    await worker.send({ kind: "menu-open", fields: ["cached.bms"] });
    await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(worker.of("menu-state").at(-1)), route: 2, fields: settingsMenuDraft() });
    await worker.send({ kind: "menu-navigate", ...acquiredMenuToken(worker.of("menu-state").at(-1)), route: 4, fields: ["prefix.bkr"] });
    const token = acquiredMenuToken(worker.of("menu-state").at(-1));
    await worker.send({ kind: "menu-record-preview", ...token, chartPath: "pending.bms", key: "prefix", replayFile: new FileType([Uint8Array.from([66,75,82,1])], "prefix.bkr"), archiveBlob: null, player: 1 });
    assert.equal(reads.includes("pending.bms"), true);
    await startGame(worker, [], { path: "cached.bms" });
    await worker.send({ kind: "play-stop", playId: 1 });
    const resumed = worker.of("menu-state").at(-1);
    assert.equal(resumed.route, 4);
    assert.equal(resumed.screen, token.screen);
    const errors = worker.of("menu-error").length;
    const accessCount = worker.libraries[0].accesses.length;
    if (settlement === "resolve") held.resolve(new TextEncoder().encode("pending").buffer);
    else held.reject(new Error("obsolete record read failure"));
    await flushJobs();
    assert.equal(admissions, 0);
    assert.equal(worker.libraries[0].accesses.length, accessCount);
    assert.equal(worker.of("menu-error").length, errors);
    assert.equal(worker.of("fatal").length, 0);
    await worker.send({ kind: "dispose" });
  }
});

test("actual menu preview retains one physical File or Blob read across menu replacement and gameplay until genuine settlement", async () => {
  for (const stage of ["replay", "archive"]) for (const settlement of ["resolve", "reject"]) {
    const h = await realPreparationWorker({ realChannels: true }), runtime = h.runtime;
    const library = new runtime.BrowserLibrary(8, 1024 * 1024, 2 * 1024 * 1024, 64);
    let bytes;
    try {
      library.add_file("chart.bms", new TextEncoder().encode(REAL_PREPARATION_CHART));
      library.add_file("key.wav", preparationWave());
      const prepared = library.prepare_chart("chart.bms", 24000, 2, 3n, 1024, 4096, 8);
      const game = runtime.BrowserGame.new_physical(prepared, 0n, 100000000n, 50000000n, 50000000n, 0n,
        keyboardBindingWords(new Uint32Array([0x11,4])), undefined, 4096, 1024);
      try { game.configure_capture(1024 * 1024, 1024); game.stop(); bytes = game.take_replay(); }
      finally { game.free(); }
    } finally { library.free(); }
    assert.ok(bytes.byteLength > 0, "retry uses actual canonical captured replay bytes");
    await admitPreparationFiles(h); await h.send(actualSelect()); await h.settle(() => h.of("selected").length === 1);
    const openRecordsMenu = async () => {
      await h.send({ kind: "menu-open", fields: ["chart.bms"] });
      await h.send({ kind: "menu-navigate", ...acquiredMenuToken(h.of("menu-state").at(-1)), route: 2, fields: settingsMenuDraft() });
      await h.send({ kind: "menu-navigate", ...acquiredMenuToken(h.of("menu-state").at(-1)), route: 4, fields: ["actual-prefix.bkr"] });
      return acquiredMenuToken(h.of("menu-state").at(-1));
    };
    let token = await openRecordsMenu(); const held = deferred();
    const file = new FileType([bytes], "actual-prefix.bkr"), archive = new Blob([Uint8Array.of(66,75,82,69,83,85,76,84)]);
    let replayReads = 0, archiveReads = 0;
    file.arrayBuffer = () => { replayReads++; return stage === "replay" ? held.promise : Promise.resolve(bytes.slice().buffer); };
    archive.arrayBuffer = () => { archiveReads++; return held.promise; };
    const preview = fields => ({ kind: "menu-record-preview", ...token, chartPath: "chart.bms", key: "actual-prefix.bkr",
      replayFile: file, archiveBlob: stage === "archive" ? archive : null, player: 1, ...fields });
    await h.send(preview());
    assert.equal(replayReads, 1); assert.equal(archiveReads, stage === "archive" ? 1 : 0);
    const pendingState = h.of("menu-state").at(-1), publications = h.of("menu-state").length;
    await h.send(preview());
    assert.equal(replayReads, 1); assert.equal(archiveReads, stage === "archive" ? 1 : 0);
    assert.equal(h.of("menu-state").length, publications); assert.equal(h.of("menu-state").at(-1), pendingState);
    assert.match(h.of("menu-error").at(-1).message, /acquisition|finish|wait/i);
    token = await openRecordsMenu();
    await h.send(preview());
    assert.equal(replayReads, 1, "new menu owner cannot replace the outstanding native binary read");
    assert.equal(archiveReads, stage === "archive" ? 1 : 0);
    await h.send({ kind: "play-start", playId: 1, rpcId: 1, libraryId: 1, path: "chart.bms", rate: 24000, seed: "0",
      keyPairs: new Uint32Array([0x11,4]), windowOriginNs: 0n });
    await h.settle(() => h.of("play-reply").length > 0 || h.of("play-error").length > 0);
    assert.equal(h.of("play-error").length, 0); assert.equal(h.of("play-reply").at(-1).result.kind, "prepared");
    await h.send({ kind: "play-stop", playId: 1 }); assert.equal(h.of("play-stopped").length, 1);
    const stateCount = h.of("menu-state").length, errorCount = h.of("menu-error").length;
    if (settlement === "resolve") held.resolve(stage === "replay" ? bytes.slice().buffer : Uint8Array.of(66,75,82,69,83,85,76,84).buffer);
    else held.reject(new Error("retired immutable acquisition failed"));
    await flushJobs();
    assert.equal(h.of("menu-state").length, stateCount, "obsolete read cannot publish into resumed/current menu");
    assert.equal(h.of("menu-error").length, errorCount, "obsolete acquisition failure cannot replace current feedback");
    assert.equal(h.of("fatal").length, 0);
    token = await openRecordsMenu();
    const retryStateCount = h.of("menu-state").length;
    const retry = new FileType([bytes], "actual-prefix.bkr"); let retryReads = 0;
    retry.arrayBuffer = () => { retryReads++; return Promise.resolve(bytes.slice().buffer); };
    await h.send(preview({ replayFile: retry, archiveBlob: null }));
    assert.equal(retryReads, 1, "explicit retry acquires only after the old physical operation settles");
    assert.equal(h.of("menu-error").length, errorCount, "actual WASM admits the genuine captured retry");
    assert.equal(h.of("menu-state").length, retryStateCount + 1);
    assert.ok(h.of("menu-state").at(-1).revision > token.revision);
    await h.send({ kind: "dispose" }); assert.equal(h.of("disposed").length, 1);
  }
});

test("every live preparation route acquires only its seeded plan before constructing its owner", async () => {
  const cases = [
    { extra: {}, method: undefined },
    { extra: { startNs: 2n }, method: "prepare_chart_at" },
    { extra: { startNs: 2n, timingPolicy: { presetId: "beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1", rankPrecedence: "rank-first", gauge: "groove" } }, method: "prepare_chart_with_policy_at" },
    { extra: { inputMode: "physical", localPlanWords: new Uint32Array([7, 0, 0, 0]) }, method: undefined },
  ];
  for (const { extra, method } of cases) {
    const reads = [];
    const worker = await readyWorker({ gameplay: true, references: (_path, seed) => seed === 19n ? ["needed.wav"] : ["wrong-seed.wav"] });
    await worker.send({ kind: "import", id: 1, files: [
      trackedSelected("song/chart.bms", "chart", reads),
      trackedSelected("needed.wav", "pcm", reads),
      trackedSelected("wrong-seed.wav", "unused", reads, () => { throw new Error("wrong RANDOM branch must not read"); }),
    ] });
    await worker.send({ kind: "accept-library", id: 1 });
    await startGame(worker, [], { seed: "19", ...extra });
    assert.deepEqual(reads, ["song/chart.bms", "needed.wav"]);
    assert.equal(worker.libraries[0].plans[0].seed, 19n);
    assert.equal(worker.libraries[0].preparations[0].method, method);
    assert.equal(worker.games.length, 1);
    assert.equal(worker.of("play-error").length, 0);
    assert.equal(worker.of("play-reply").at(-1).result.kind, "prepared");
    if (extra.localPlanWords) assert.deepEqual(Array.from(worker.games[0].players), [7]);
    await worker.send({ kind: "dispose" });
  }
});

test("replay resource planning receives actual recording bytes instead of the live seed draft", async () => {
  const reads = [];
  const worker = await readyWorker({ gameplay: true,
    references: { "song/chart.bms": ["wrong.wav"] }, replayReferences: { "song/chart.bms": ["recorded.wav"] } });
  await worker.send({ kind: "import", id: 1, files: [
    trackedSelected("song/chart.bms", "chart", reads),
    trackedSelected("recorded.wav", "pcm", reads),
    trackedSelected("wrong.wav", "unused", reads, () => { throw new Error("live seed must not influence recorded resource plan"); }),
  ] });
  await worker.send({ kind: "accept-library", id: 1 });
  const replay = opponentFile("recorded.bkr", [66,75,82,1,19]);
  await startGame(worker, undefined, { mode: "replay", replayFile: replay.file, seed: "invalid live seed" });
  assert.equal(replay.reads, 1);
  assert.deepEqual(reads, ["song/chart.bms", "recorded.wav"]);
  assert.deepEqual(worker.libraries[0].plans, [{ method: "replay", path: "song/chart.bms", bytes: [66,75,82,1,19], maxSamples: 3844 }]);
  assert.equal(worker.games.length, 1);
  assert.equal(worker.of("play-error").length, 0);
  assert.ok(worker.calls.findIndex(row => row[0] === "plan-replay") < worker.calls.findIndex(row => row[0] === "prepare-replay"));
  await worker.send({ kind: "dispose" });
});

test("section start routes fresh preparations and exact source metadata before capture while zero remains compatible", async () => {
  for (const startNs of [undefined, 0n, 1125000001n, 604800000000001n]) {
    const worker = await gameWorker();
    await startGame(worker, [], { startNs, seed: "18446744073709551615", recordReplay: true });
    const entry = worker.libraries[0].preparations[0];
    const game = worker.games[0];
    if (startNs) {
      assert.equal(entry.method, "prepare_chart_at");
      assert.deepEqual(entry.args, [48000, 2, 18446744073709551615n, startNs, 64 * 1024 * 1024, 256 * 1024 * 1024, 3844]);
    } else {
      assert.equal(entry.method, undefined);
      assert.deepEqual(entry.args, [48000, 2, 18446744073709551615n, 64 * 1024 * 1024, 256 * 1024 * 1024, 3844]);
    }
    assert.equal(worker.of("play-reply").at(-1).result.startNs, startNs ?? 0n);
    assert.equal(game.prepared.start_ns, startNs ?? 0n);
    assert.deepEqual(game.constructorArgs.slice(0, 2), [0n, 100000000n], "original-song start belongs to the prepared owner, not the output-origin argument");
    assert.ok(worker.calls.findIndex(call => call[0] === "new-game") < worker.calls.findIndex(call => call[0] === "capture"));
    await worker.send({ kind: "play-stop", playId: 1 });
    await startGame(worker, [], { playId: 2, startNs: 2000000001n });
    assert.equal(worker.libraries[0].preparations.length, 2);
    assert.notEqual(worker.games[1].prepared, game.prepared, "restart constructs from the library again");
    assert.equal(worker.games[1].prepared.start_ns, 2000000001n);
    assert.equal(game.frees, 1);
    await worker.send({ kind: "play-stop", playId: 2 });
  }
  const legacy = await gameWorker({ omitPreparedStart: true });
  await startGame(legacy, []);
  assert.equal(legacy.of("play-reply").at(-1).result.startNs, 0n);
  await legacy.send({ kind: "play-stop", playId: 1 });
  const full = await gameWorker({ sampleCount: 7940 });
  await startGame(full, [], { startNs: 1n });
  assert.equal(full.of("play-reply").at(-1).result.samples, 7940);
  assert.equal(full.libraries[0].preparations[0].args.at(-1), 3844);
  await full.send({ kind: "play-stop", playId: 1 });
});

test("invalid requested starts fail before acquisition and mismatched prepared starts release unconsumed owners", async () => {
  for (const startNs of [null, "1", 1, -1n, 9223372036854775808n]) {
    const worker = await gameWorker();
    const unread = opponentFile("must-not-read.bkr");
    await startGame(worker, [opponentChoice(unread, "file:1")], { startNs });
    assert.equal(worker.libraries[0].preparations.length, 0);
    assert.equal(worker.games.length, 0);
    assert.equal(unread.reads, 0);
    assert.equal(worker.of("play-error").length, 1);
  }
  for (const options of [{ omitPreparedStart: true }, { preparedStart: 0n }, { preparedStart: 1 },
    { preparedStart: -1n }, { preparedStart: 9223372036854775808n }]) {
    const worker = await gameWorker(options);
    await startGame(worker, [], { startNs: 1000000000n, recordReplay: true });
    assert.equal(worker.preparedOwners.length, 1);
    assert.equal(worker.preparedOwners[0].moved, false);
    assert.equal(worker.preparedOwners[0].frees, 1);
    assert.equal(worker.games.length, 0);
    assert.equal(worker.calls.some(call => call[0] === "capture"), false);
    assert.equal(worker.of("play-error").length, 1);
    assert.equal(worker.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
  }
  const oversized = await gameWorker({ sampleCount: 7941 });
  await startGame(oversized, [], { startNs: 1n });
  assert.equal(oversized.of("play-error").length, 1);
  assert.equal(oversized.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
  assert.equal(oversized.games[0].frees, 1);
});

test("cancelled section preparation cannot revive and replay retains its actual recorded start independently", async () => {
  const cancelled = await gameWorker();
  const pending = startGame(cancelled, [], { startNs: 9000000001n });
  const stopping = cancelled.send({ kind: "play-stop", playId: 1 });
  await Promise.all([pending, stopping]);
  assert.equal(cancelled.libraries[0].preparations.length, 0);
  assert.equal(cancelled.games.length, 0);
  assert.equal(cancelled.of("play-stopped").length, 1);
  await startGame(cancelled, [], { playId: 2, startNs: 4000000001n });
  assert.equal(cancelled.games[0].prepared.start_ns, 4000000001n);
  await cancelled.send({ kind: "play-stop", playId: 2 });
  const worker = await gameWorker({ replayStart: 604800000000001n });
  const replay = opponentFile("section-prefix.bkr");
  await startGame(worker, undefined, { mode: "replay", replayFile: replay.file, startNs: "invalid live draft" });
  assert.equal(replay.reads, 1);
  assert.equal(worker.calls.filter(call => call[0] === "prepare-replay").length, 1);
  assert.equal(worker.calls.some(call => call[0] === "prepare-section" || call[0] === "capture"), false);
  assert.equal(worker.of("play-reply").at(-1).result.startNs, 604800000000001n);
  assert.equal(worker.games[0].prepared.start_ns, 604800000000001n);
  assert.deepEqual(worker.games[0].constructorArgs, [100000000n]);
  await worker.send({ kind: "play-stop", playId: 1 });
});

test("live judge timing forwards exact validated constructor values while omitted timing preserves defaults", async () => {
  const defaults = await gameWorker();
  await startGame(defaults, []);
  assert.deepEqual(defaults.games[0].constructorArgs.slice(0, 5), [0n, 100000000n, 50000000n, 50000000n, 0n]);
  await defaults.send({ kind: "play-stop", playId: 1 });
  const configured = await gameWorker();
  const timing = { earlyNs: 12345678n, lateNs: 87654321n, offsetNs: -12500001n };
  const preparing = startGame(configured, [], { timing, recordReplay: true });
  timing.earlyNs = 0n;
  timing.offsetNs = 900n;
  await preparing;
  assert.deepEqual(configured.games[0].constructorArgs.slice(0, 5), [0n, 100000000n, 12345678n, 87654321n, -12500001n]);
  assert.deepEqual(Array.from(configured.games[0].constructorArgs[5]), [0x11, 4]);
  assert.equal(configured.of("play-error").length, 0);
  assert.ok(configured.calls.findIndex(call => call[0] === "new-game") < configured.calls.findIndex(call => call[0] === "capture"));
  await configured.send({ kind: "play-stop", playId: 1 });
});

test("bad live timing fails before chart or opponent acquisition and replay uses only its recorded constructor", async () => {
  const baseline = { earlyNs: 50000000n, lateNs: 50000000n, offsetNs: 0n };
  for (const timing of [null, {}, { ...baseline, earlyNs: -1n }, { ...baseline, lateNs: -1n },
    { ...baseline, earlyNs: 50 }, { ...baseline, offsetNs: "0" },
    { ...baseline, offsetNs: 9223372036854775808n }, { ...baseline, offsetNs: -9223372036854775809n }]) {
    const worker = await gameWorker();
    const unread = opponentFile("must-not-read.bkr", [1], () => { throw new Error("timing preflight must precede acquisition"); });
    await startGame(worker, [opponentChoice(unread, "file:1")], { timing });
    assert.equal(unread.reads, 0);
    assert.equal(worker.libraries[0].preparations.length, 0);
    assert.equal(worker.games.length, 0);
    assert.equal(worker.of("play-error").length, 1);
    assert.equal(worker.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
  }
  const worker = await gameWorker();
  const replay = opponentFile("recorded-profile.bkr");
  await startGame(worker, undefined, { mode: "replay", replayFile: replay.file,
    timing: { earlyNs: "invalid live draft", lateNs: -1n, offsetNs: null } });
  assert.equal(replay.reads, 1);
  assert.equal(worker.of("play-error").length, 0);
  assert.deepEqual(worker.games[0].constructorArgs, [100000000n]);
  assert.equal(worker.of("play-reply").at(-1).result.mode, "replay");
  assert.equal(worker.calls.some(call => call[0] === "capture"), false);
  await worker.send({ kind: "play-stop", playId: 1 });
});

test("live preparation reads selected immutable Files sequentially and admits actual bindings before capture or activation", async () => {
  const firstRead = deferred();
  const secondRead = deferred();
  const first = opponentFile("own.bkr", [66, 75, 82, 1], () => firstRead.promise);
  const second = opponentFile("other.bkr", [66, 75, 82, 2], () => secondRead.promise);
  const worker = await gameWorker();
  await startGame(worker, [opponentChoice(first, "file:1"), opponentChoice(second, "record:2", false)], { recordReplay: true });
  assert.equal(first.reads, 1);
  assert.equal(second.reads, 0);
  assert.equal(worker.games[0].added.length, 0);
  assert.equal(worker.calls.some(call => call[0] === "capture"), false);
  assert.equal(worker.of("play-reply").length, 0);
  firstRead.resolve(first.bytes.slice().buffer); await flushJobs();
  assert.equal(second.reads, 1);
  assert.deepEqual(worker.games[0].added, [{ bytes: [66, 75, 82, 1], own: true, label: "own.bkr" }]);
  secondRead.resolve(second.bytes.slice().buffer); await flushJobs();
  const prepared = worker.of("play-reply").at(-1).result;
  assert.equal(prepared.kind, "prepared");
  assert.equal(prepared.opponentCount, 2);
  assert.deepEqual(worker.games[0].added[1], { bytes: [66, 75, 82, 2], own: false, label: "other.bkr" });
  const operations = worker.calls.map(call => call[0]);
  assert.ok(operations.lastIndexOf("add-opponent") < operations.indexOf("capture"));
  assert.equal(first.bytes.byteLength, 4);
  assert.equal(second.bytes.byteLength, 4);
  await worker.send({ kind: "play-activate", playId: 1, rpcId: 2, hostNs: 1000000000n, startFrame: 48000n });
  assert.ok(worker.calls.findIndex(call => call[0] === "capture") < worker.calls.findIndex(call => call[0] === "activate"));
  await worker.send({ kind: "play-stop", playId: 1 });
  assert.equal(worker.games[0].stops, 1);
  assert.equal(worker.games[0].frees, 1);
  assert.equal(worker.of("play-stopped")[0].replayError, null);
});

test("invalid selection, changed read extent and incompatible bytes fail explicitly while cancelled reads cannot admit into a newer game", async () => {
  const invalid = await gameWorker();
  let forbiddenReads = 0;
  await startGame(invalid, [{ file: { size: 4, arrayBuffer() { forbiddenReads++; } }, sourceKey: "fake", own: true, label: "fake" }]);
  assert.equal(forbiddenReads, 0);
  assert.equal(invalid.games.length, 0);
  assert.equal(invalid.libraries[0].preparations.length, 0);
  assert.equal(invalid.of("play-error").length, 1);

  for (const failure of ["extent", "layout", "incompatible"]) {
    const worker = await gameWorker(failure === "incompatible" ? { addError: "actual binding rejected incompatible chart" } : {});
    const selected = opponentFile("bad.bkr", [1, 2, 3, 4], failure === "extent"
      ? () => Promise.resolve(new ArrayBuffer(3))
      : failure === "layout" ? () => Promise.resolve(new Uint8Array(4)) : undefined);
    await startGame(worker, [opponentChoice(selected, "file:1")], { recordReplay: true });
    assert.equal(selected.reads, 1);
    assert.equal(worker.of("play-error").length, 1);
    assert.equal(worker.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
    assert.equal(worker.calls.some(call => call[0] === "capture" || call[0] === "activate"), false);
    assert.equal(worker.games[0].stops, 1);
    assert.equal(worker.games[0].frees, 1);
  }
  const pendingRead = deferred();
  const pending = await gameWorker();
  const unread = opponentFile("pending.bkr", [1, 2, 3, 4], () => pendingRead.promise);
  await startGame(pending, [opponentChoice(unread, "pending")]);
  await pending.send({ kind: "play-activate", playId: 1, rpcId: 2, hostNs: 1000000000n, startFrame: 48000n });
  assert.equal(pending.calls.some(call => call[0] === "activate"), false);
  assert.equal(pending.of("play-error").length, 1);
  pendingRead.resolve(unread.bytes.slice().buffer); await flushJobs();
  assert.deepEqual(pending.games[0].added, []);
  assert.equal(pending.games[0].frees, 1);
  const gate = deferred();
  const oldFile = opponentFile("old.bkr", [1, 2, 3, 4], () => gate.promise);
  const worker = await gameWorker();
  await startGame(worker, [opponentChoice(oldFile, "old")], { recordReplay: true });
  const previous = worker.games[0];
  await worker.send({ kind: "play-stop", playId: 1 });
  assert.equal(previous.frees, 1);
  await startGame(worker, [], { playId: 2 });
  const current = worker.games[1];
  gate.resolve(oldFile.bytes.slice().buffer); await flushJobs();
  assert.deepEqual(previous.added, []);
  assert.deepEqual(current.added, []);
  assert.equal(current.frees, 0);
  assert.equal(worker.of("play-reply").filter(reply => reply.playId === 1 && reply.result?.kind === "prepared").length, 0);
  assert.equal(worker.of("play-reply").find(reply => reply.playId === 2).result.opponentCount, 0);
  await worker.send({ kind: "play-stop", playId: 2 });
});

test("actual comparison snapshots are throttled independently and comparison faults do not stop local capture or solo and replay paths", async () => {
  const controls = { songNs: 999999990n, snapshot: game => [{ kind: "own", label: "own.bkr",
    songNs: game.song_ns, recordedUntilNs: 500000000n, hits: 1n, misses: 0n, combo: 1n, maxCombo: 1n }] };
  const worker = await gameWorker(controls);
  const selected = opponentFile("own.bkr");
  await startGame(worker, [opponentChoice(selected, "file:1")], { recordReplay: true });
  await worker.send({ kind: "play-activate", playId: 1, rpcId: 2, hostNs: 1000000000n, startFrame: 48000n });
  const step = (tickId, time) => worker.send({ kind: "play-step", playId: 1, tickId,
    events: [], watermark: BigInt(time), audioNs: BigInt(time) });
  await step(1, 1000000000);
  assert.equal(worker.games[0].snapshots, 1);
  assert.equal(worker.of("play-opponents").length, 0, "successful comparison prefixes stay in the Worker HUD");
  assert.ok(worker.calls.some(call => call[0] === "snapshot" && call[1] === -100000000n),
    "without real output, comparisons retain the actual initial song prefix");
  worker.setNow(249);
  await step(2, 1000000001);
  assert.equal(worker.games[0].snapshots, 1);
  worker.setNow(250);
  await step(3, 1000000002);
  assert.equal(worker.games[0].snapshots, 2);
  worker.games[0].saved_opponents = () => { throw new Error("comparison prefix failure"); };
  worker.setNow(500);
  await step(4, 1000000003);
  assert.match(worker.of("play-opponents").at(-1).error, /comparison prefix failure/);
  assert.equal(worker.of("play-opponents").at(-1).opponents, null);
  worker.setNow(750);
  await step(5, 1000000004);
  assert.equal(worker.of("play-error").length, 0);
  assert.equal(worker.games[0].frees, 0);
  assert.equal(worker.of("play-step-done").length, 5);
  assert.equal(worker.of("play-step-done").at(-1).hits, 17n);
  assert.equal(worker.of("play-opponents").filter(value => value.error !== null).length, 1);
  await worker.send({ kind: "play-stop", playId: 1 });
  assert.equal(worker.of("play-stopped").at(-1).replayError, null);
  assert.equal(worker.of("play-stopped").at(-1).savedOpponents.opponents, null);
  assert.match(worker.of("play-stopped").at(-1).savedOpponents.error, /comparison prefix failure/);
  assert.equal(worker.calls.filter(call => call[0] === "disable-opponent-hud").length, 1);
  const publicationCount = worker.of("play-opponents").length;
  await startGame(worker, [], { playId: 2 });
  await worker.send({ kind: "play-activate", playId: 2, rpcId: 2, hostNs: 2000000000n, startFrame: 96000n });
  await worker.send({ kind: "play-step", playId: 1, tickId: 6, events: [], watermark: 2000000000n, audioNs: 2000000000n });
  await worker.send({ kind: "play-step", playId: 2, tickId: 1, events: [], watermark: 2000000000n, audioNs: 2000000000n });
  assert.equal(worker.games[1].snapshots, 0);
  assert.equal(worker.of("play-opponents").length, publicationCount);
  await worker.send({ kind: "play-stop", playId: 2 });
  const replay = opponentFile("replay.bkr");
  await startGame(worker, undefined, { playId: 3, mode: "replay", replayFile: replay.file });
  assert.equal(worker.games[2].added.length, 0);
  assert.equal(worker.games[2].snapshots, 0);
  await worker.send({ kind: "play-stop", playId: 3 });
});

test("successive metadata catalogs acquire no bytes and stale acknowledgements cannot adopt retired proposals", async () => {
  const worker = await readyWorker();
  const pending = deferred();
  let reads = 0;
  let skippedReads = 0;
  let newerReads = 0;
  await worker.send({ kind: "import", id: 1, files: [selectedFile("old/a.bms", "old", () => { reads++; return pending.promise; })] });
  assert.equal(reads, 0);
  assert.equal(worker.libraries.length, 1);
  await worker.send({ kind: "import", id: 2, files: [selectedFile("skipped/b.bms", "skip", () => {
    skippedReads++;
    return Promise.resolve(new TextEncoder().encode("skip").buffer);
  })] });
  await worker.send({ kind: "import", id: 3, files: [selectedFile("new/c.bms", "new", () => {
    newerReads++;
    return Promise.resolve(new TextEncoder().encode("new").buffer);
  })] });
  assert.equal(newerReads, 0);
  assert.equal(skippedReads, 0);
  assert.equal(worker.libraries.length, 3);
  assert.deepEqual(worker.of("catalog").map(message => message.id), [1, 2, 3]);
  pending.resolve(new TextEncoder().encode("old").buffer);
  await flushJobs();
  assert.equal(newerReads, 0);
  assert.equal(skippedReads, 0);
  assert.equal(worker.libraries.length, 3);
  assert.equal(worker.libraries[0].frees, 1);
  assert.equal(worker.libraries[0].files.length, 0);
  assert.equal(worker.libraries[1].frees, 1);
  assert.equal(worker.libraries[2].frees, 0);
  assert.deepEqual([...worker.libraries[2].declarations.keys()], ["new/c.bms"]);
  assert.equal(worker.libraries[2].files.length, 0);
  assert.equal(worker.of("import-error").length, 0);
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "accept-library", id: 2 });
  await worker.send({ kind: "accept-library", id: 3 });
  await worker.send({ kind: "select", id: 4, libraryId: 3, path: "new/c.bms", rate: 48000, seed: "0" });
  assert.equal(worker.views[0].current.path, "new/c.bms");
  assert.equal(newerReads, 1);
  assert.equal(reads, 0);
  assert.equal(skippedReads, 0);
});

test("invalid import metadata acquires no bytes and preserves the admitted library", async () => {
  const worker = await readyWorker();
  await worker.send({ kind: "import", id: 10, files: [selectedFile("song/chart.bms")] });
  await worker.send({ kind: "accept-library", id: 10 });
  let reads = 0;
  const acquire = () => { reads++; throw new Error("must not read invalid import"); };
  await worker.send({ kind: "import", id: 11, files: [
    selectedFile("bad/a.bms", "x", acquire),
    selectedFile("bad/./a.bms", "x", acquire),
  ] });
  assert.equal(reads, 0);
  assert.equal(worker.libraries.length, 1);
  assert.equal(worker.libraries[0].frees, 0);
  assert.deepEqual(worker.of("catalog").map(message => message.id), [10]);
  assert.equal(worker.of("import-error")[0].id, 11);
  await worker.send({ kind: "select", id: 12, libraryId: 10, path: "song/chart.bms", rate: 44100, seed: "18446744073709551615" });
  assert.equal(worker.views[0].current.path, "song/chart.bms");
  assert.equal(worker.libraries[0].preparations[0].args[2], 18446744073709551615n);
  assert.deepEqual(worker.libraries[0].preparations[0].args,
    [44100, 2, 18446744073709551615n, 64 * 1024 * 1024, 256 * 1024 * 1024, 3844],
    "actual preview preparation keeps the same original count and independent byte budgets as live/replay");
});

test("an ignored catalog followed by a failed import preserves the accepted library until matching acknowledgement", async () => {
  const worker = await readyWorker();
  await worker.send({ kind: "import", id: 1, files: [selectedFile("accepted/old.bms")] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "accepted/old.bms", rate: 48000, seed: "0" });
  const accepted = worker.libraries[0];
  const previousView = worker.views[0].current;

  // Main has moved on to import B before it receives proposal A's catalog.
  await worker.send({ kind: "import", id: 3, files: [selectedFile("ignored/a.bms")] });
  const ignored = worker.libraries[1];
  assert.deepEqual(worker.of("catalog").map(message => message.id), [1, 3]);
  await worker.send({ kind: "accept-library", id: 999 });
  await worker.send({ kind: "accept-library", id: 1 });
  assert.equal(accepted.frees, 0);
  assert.equal(ignored.frees, 0);
  await worker.send({ kind: "select", id: 4, libraryId: 3, path: "ignored/a.bms", rate: 48000, seed: "0" });
  assert.equal(worker.of("selection-error").at(-1).id, 4);
  assert.equal(worker.views[0].current, previousView);
  assert.equal(ignored.preparations.length, 0);

  await worker.send({ kind: "import", id: 5, files: [selectedFile("failed/../b.bms", "x", () => { throw new Error("invalid metadata must not read"); })] });
  assert.equal(ignored.frees, 1);
  assert.equal(worker.libraries.length, 2, "invalid metadata must not allocate a candidate");
  assert.equal(accepted.frees, 0);
  assert.equal(worker.of("import-error").at(-1).id, 5);
  assert.equal(worker.views[0].current, previousView);
  assert.equal(previousView.releasedByView, undefined);
  await worker.send({ kind: "accept-library", id: 3 });
  assert.equal(ignored.frees, 1);
  assert.equal(accepted.frees, 0);
  await worker.send({ kind: "select", id: 6, libraryId: 1, path: "accepted/old.bms", rate: 48000, seed: "0" });
  assert.equal(worker.views[0].current.path, "accepted/old.bms");
  assert.equal(accepted.preparations.length, 2);
  assert.equal(worker.of("selected").at(-1).libraryId, 1);

  // Only acknowledgement of the current proposal may release the old owner.
  await worker.send({ kind: "import", id: 7, files: [selectedFile("accepted/new.bms")] });
  const replacement = worker.libraries[2];
  await worker.send({ kind: "accept-library", id: 3 });
  assert.equal(accepted.frees, 0);
  assert.equal(replacement.frees, 0);
  await worker.send({ kind: "accept-library", id: 7 });
  assert.equal(accepted.frees, 1);
  assert.equal(replacement.frees, 0);
  await worker.send({ kind: "accept-library", id: 7 });
  await worker.send({ kind: "accept-library", id: 1 });
  assert.equal(accepted.frees, 1);
  assert.equal(replacement.frees, 0);
  await worker.send({ kind: "select", id: 8, libraryId: 7, path: "accepted/new.bms", rate: 48000, seed: "0" });
  assert.equal(worker.views[0].current.path, "accepted/new.bms");
  assert.equal(replacement.preparations.length, 1);
  assert.equal(worker.of("fatal").length, 0);
});

test("preparation failure retains the old view and its selected identity", async () => {
  const worker = await readyWorker({ rejectPreparation: "bad.bms" });
  await worker.send({ kind: "import", id: 1, files: [selectedFile("good.bms"), selectedFile("bad.bms")] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "good.bms", rate: 48000, seed: "7" });
  const original = worker.views[0].current;
  const exports = worker.visualExports.length;
  await worker.send({ kind: "select", id: 3, libraryId: 1, path: "bad.bms", rate: 48000, seed: "7" });
  assert.equal(worker.views[0].current, original);
  assert.equal(original.frees, 0);
  assert.equal(original.releasedByView, undefined);
  assert.equal(worker.views[0].replacements.length, 1);
  assert.deepEqual(worker.of("selected").map(message => message.id), [2]);
  assert.equal(worker.of("selection-error")[0].id, 3);
  assert.match(worker.of("selection-error")[0].message, /sample rate mismatch/);
  assert.equal(worker.visualExports.length, exports, "failed preparation cannot replace the visual registration");
  await worker.send({ kind: "seek", id: 4, selectedId: 2, ns: "604800000000001" });
  assert.equal(worker.visualExports.at(-1).owner, original);
  assert.equal(worker.visualExports.at(-1).kind, 3);
  assert.equal(worker.visualExports.at(-1).songNs, 604800000000001n);
  assert.equal(worker.visualAcks.at(-1).sequence, worker.visualExports.at(-1).sequence);
  assert.equal(worker.of("position")[0].selectedId, 2);
});

test("CPU initialization failure never reports readiness or admits later work", async () => {
  for (const options of [{ initError: "missing WASM" }]) {
    const worker = await workerHarness(options);
    await worker.send({ kind: "init", canvas: {} });
    assert.equal(worker.of("ready").length, 0);
    assert.equal(worker.of("fatal").length, 1);
    let reads = 0;
    await worker.send({ kind: "import", id: 1, files: [selectedFile("a.bms", "x", () => { reads++; })] });
    assert.equal(reads, 0);
    assert.equal(worker.libraries.length, 0);
    assert.equal(worker.timers.size, 0);
    assert.equal(worker.of("fatal").length, 1);
  }
});

test("submitted geometry forwarding identifies the actual visual mode and play owner", async () => {
  const worker = await gameWorker();
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" });
  await worker.send({ kind: "resize", width: 640, height: 480 });
  const preview = worker.of("render-geometry").at(-1);
  assert.equal(preview.mode, "preview");
  assert.equal(preview.selectedId, 2);
  assert.equal(preview.playId, undefined);
  assert.deepEqual([preview.page, preview.width, preview.height], [0, 640, 480]);
  const resizes = worker.renderPort.posts.filter(row => row.kind === "resize").length;
  await startGame(worker, []);
  const unchangedLive = worker.of("render-geometry").at(-1);
  assert.equal(unchangedLive.mode, "live");
  assert.equal(unchangedLive.playId, 1);
  assert.notEqual(unchangedLive.generation, preview.generation);
  assert.ok(unchangedLive.geometryVersion > preview.geometryVersion);
  assert.deepEqual([unchangedLive.page, unchangedLive.width, unchangedLive.height], [0, 640, 480]);
  const liveRegistration = worker.renderPort.posts.findLast(row => row.kind === "packet" && row.mode === "live");
  assert.equal(liveRegistration.geometryVersion, unchangedLive.geometryVersion);
  assert.equal(worker.renderPort.posts.filter(row => row.kind === "resize").length, resizes,
    "new content acknowledges the retained extent without a duplicate resize");
  await worker.send({ kind: "resize", width: 960, height: 720 });
  const live = worker.of("render-geometry").at(-1);
  assert.equal(live.mode, "live");
  assert.equal(live.playId, 1);
  assert.notEqual(live.generation, preview.generation);
  assert.deepEqual([live.page, live.width, live.height], [0, 960, 720]);
  await worker.send({ kind: "play-stop", playId: 1 });
  const restored = worker.of("render-geometry").at(-1);
  assert.equal(restored.mode, "preview");
  assert.equal(restored.playId, undefined);
  assert.notEqual(restored.generation, live.generation);
  assert.ok(restored.geometryVersion > live.geometryVersion);
  assert.deepEqual([restored.page, restored.width, restored.height], [0,960,720]);
  await worker.send({kind:"select",id:3,libraryId:1,path:"song/chart.bms",rate:48000,seed:"0"});
  const replacement = worker.of("render-geometry").at(-1);
  assert.equal(replacement.mode,"preview");assert.equal(replacement.selectedId,3);
  assert.notEqual(replacement.generation,restored.generation);
  assert.ok(replacement.geometryVersion>restored.geometryVersion);
  assert.deepEqual([replacement.page,replacement.width,replacement.height],[0,960,720]);
  worker.renderPort.emit({kind:"geometry-ack",generation:preview.generation,content:preview.content,
    geometryVersion:preview.geometryVersion,page:0,width:640,height:480});await flushJobs();
  assert.equal(worker.of("render-geometry").at(-1),replacement,"stale owner tuple cannot replace current geometry");
});

test("new content preserves an unsent explicit surface version then stamps later generations freshly", async () => {
  const worker = await gameWorker();
  await worker.send({kind:"resize",width:640,height:480,geometryVersion:41n});
  assert.equal(worker.renderPort.posts.length,0,"surface reservation waits for actual content");
  await worker.send({kind:"select",id:2,libraryId:1,path:"song/chart.bms",rate:48000,seed:"0"});
  const registration=worker.renderPort.posts.find(row=>row.kind==="packet");
  assert.equal(registration.geometryVersion,undefined,"pending explicit resize keeps its original ordered version");
  const resize=worker.renderPort.posts.find(row=>row.kind==="resize");
  assert.equal(resize.geometryVersion,41n);
  const preview=worker.of("render-geometry").at(-1);
  assert.equal(preview.geometryVersion,41n);assert.equal(preview.mode,"preview");
  await startGame(worker,[]);
  const liveRegistration=worker.renderPort.posts.findLast(row=>row.kind==="packet"&&row.mode==="live");
  assert.ok(liveRegistration.geometryVersion>41n);
  const live=worker.of("render-geometry").at(-1);
  assert.equal(live.geometryVersion,liveRegistration.geometryVersion);assert.equal(live.mode,"live");
  assert.deepEqual([live.page,live.width,live.height],[0,640,480]);
  assert.equal(worker.renderPort.posts.filter(row=>row.kind==="resize").length,1);
  await worker.send({kind:"play-stop",playId:1});
});

test("CPU readiness needs neither GPU nor canvas nor a renderer acknowledgement", async () => {
  const worker = await readyWorker({ createError: "must never initialize GPU", renderBlocked: true });
  assert.equal(worker.renderPort.posts.length, 0);
  assert.equal(worker.renderPort.starts, 0);
  assert.equal(worker.views.length, 1, "observation endpoint only; no game-owned BrowserView");
  await worker.send({ kind: "dispose" });
  assert.equal(worker.renderPort.closes, 1);
  assert.equal(worker.of("disposed").length, 1);
});

test("CPU init rejects missing transferred render port and invalid trusted bounds before WASM ownership", async () => {
  for (const fields of [{renderPort:null}, {maxPacketBytes:39}, {maxDiagnosticBytes:-1},
    {renderTimeoutMs:0}, {renderTimeoutMs:60001}]) {
    const worker = await workerHarness();
    await worker.send({kind:"init", ...fields});
    assert.equal(worker.of("ready").length, 0);
    assert.equal(worker.of("fatal").length, 1);
    assert.equal(worker.renderPort.posts.length, 0);
    assert.equal(worker.libraries.length, 0);
  }
});

test("missing renderer ACK deadline fails play with its actual incomplete captured prefix", async () => {
  const worker = await gameWorker();
  await startGame(worker, [], {recordReplay:true});
  await worker.send({kind:"play-activate",playId:1,rpcId:2,hostNs:1000000000n,startFrame:48000n});
  worker.renderPort.blocked = true;
  await worker.send({kind:"play-step",playId:1,tickId:1,events:[],watermark:1000000000n,audioNs:0n});
  const count = worker.visualAcks.length;
  const timeout = worker.timers.entries().next().value;
  assert.ok(timeout, "bounded visual ACK deadline exists");
  worker.timers.delete(timeout[0]); timeout[1](); await flushJobs();
  const final = worker.of("play-error").at(-1);
  assert.ok(final); assert.match(final.message, /acknowledgement timed out/);
  assert.equal(final.hits, 17n); assert.equal(final.misses, 3n);
  assert.equal(final.replayComplete, false);
  assert.ok(final.replay instanceof FileType);
  assert.deepEqual(Array.from(new Uint8Array(await final.replay.arrayBuffer())), [66,75,82]);
  assert.equal(worker.games[0].frees, 1);
  assert.equal(worker.visualAcks.length, count, "timeout does not fabricate producer baseline adoption");
});

test("missing visual ACK coalesces preview changes until exact full acknowledgement", async () => {
  const worker = await readyWorker();
  await worker.send({ kind: "import", id: 1, files: [selectedFile("a.bms")] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "a.bms", rate: 48000, seed: "0" });
  worker.renderPort.blocked = true;
  await worker.send({ kind: "seek", id: 3, selectedId: 2, ns: "1" });
  const pending = worker.renderPort.posts.at(-1);
  assert.equal(pending.kind, "packet");
  const count = worker.visualExports.length, acks = worker.visualAcks.length;
  await worker.send({ kind: "seek", id: 4, selectedId: 2, ns: "2" });
  await worker.send({ kind: "seek", id: 5, selectedId: 2, ns: "3" });
  assert.equal(worker.visualExports.length, count, "busy producer retains the frozen pending snapshot");
  worker.renderPort.ack(pending, { operationId: pending.operationId + 1n }); await flushJobs();
  worker.renderPort.ack(pending, { content: pending.content + 1n }); await flushJobs();
  worker.renderPort.ack(pending, { packetKind: 2 }); await flushJobs();
  assert.equal(worker.visualAcks.length, acks);
  assert.equal(worker.visualExports.length, count);
  worker.renderPort.ack(pending); await flushJobs();
  assert.equal(worker.visualAcks.length, acks + 1);
  assert.equal(worker.visualExports.length, count + 1);
  assert.equal(worker.visualExports.at(-1).songNs, 3n, "coalesced state exports the latest authoritative seek");
  worker.renderPort.ack(pending); await flushJobs();
  assert.equal(worker.visualAcks.length, acks + 1, "duplicate old ACK cannot adopt a newer snapshot");
  await worker.send({ kind: "dispose" });
  const finalCount = worker.visualExports.length;
  worker.renderPort.ack(worker.renderPort.posts.at(-1)); await flushJobs();
  assert.equal(worker.visualExports.length, finalCount);
  assert.equal(worker.renderPort.closes, 1);
});
