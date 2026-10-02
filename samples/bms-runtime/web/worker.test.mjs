// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/*.test.mjs
// Loads the actual Worker and helper sources; no generated WASM or browser.
import assert from "node:assert/strict";
import { File as NodeFile } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";

const FileType = globalThis.File ?? NodeFile;

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
  const messages = [];
  const libraries = [];
  const views = [];
  const timers = new Map();
  let timerId = 0;
  let receive;
  class BrowserLibrary {
    constructor(...limits) {
      this.limits = limits;
      this.files = [];
      this.preparations = [];
      this.frees = 0;
      libraries.push(this);
    }
    add_file(path, bytes) {
      assert.equal(this.frees, 0);
      this.files.push({ path, bytes: Array.from(bytes) });
    }
    chart_paths() {
      return this.files.map(entry => entry.path).filter(path => /\.(bms|bme|bml|pms)$/i.test(path));
    }
    prepare_chart(path, ...args) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.some(entry => entry.path === path), "prepare from the owning library");
      this.preparations.push({ path, args });
      if (path === options.rejectPreparation) throw new Error("sample rate mismatch");
      const prepared = {
        title: `Prepared ${path}`, artist: "Fixture", duration_ns: 604800000000000n,
        note_count: 3, sample_count: 2, image_count: 1, path, moved: false, frees: 0,
        free() { assert.equal(this.moved, false); this.frees++; },
      };
      return prepared;
    }
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
    needs_redraw() { return options.needsRedraw ?? false; }
  }
  const self = {
    isSecureContext: true,
    navigator: { gpu: {} },
    postMessage(value) { messages.push(value); },
    addEventListener(name, callback) {
      assert.equal(name, "message");
      receive = callback;
    },
  };
  const context = createContext({
    self, File: FileType, TextEncoder, Uint8Array, ArrayBuffer,
    setTimeout(callback) { const id = ++timerId; timers.set(id, callback); return id; },
    clearTimeout(id) { timers.delete(id); },
  });
  const wasm = new SyntheticModule(["default", "BrowserLibrary", "BrowserView"], function () {
    this.setExport("default", async () => {
      if (options.initError) throw new Error(options.initError);
    });
    this.setExport("BrowserLibrary", BrowserLibrary);
    this.setExport("BrowserView", BrowserView);
  }, { context });
  const helpers = new SourceTextModule(await readFile(new URL("./host_model.mjs", import.meta.url), "utf8"), { context });
  const worker = new SourceTextModule(await readFile(new URL("./worker.js", import.meta.url), "utf8"), { context });
  await worker.link(specifier => {
    if (specifier === "./pkg/beatkernel_bms_runtime.js") return wasm;
    if (specifier === "./host_model.mjs") return helpers;
    throw new Error(`Unexpected Worker import: ${specifier}`);
  });
  await worker.evaluate();
  return {
    messages, libraries, views, timers,
    async send(request) { receive({ data: request }); await flushJobs(); },
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

test("overlapping imports retain only the latest pending request and free the stale candidate", async () => {
  const worker = await readyWorker();
  const pending = deferred();
  let reads = 0;
  let skippedReads = 0;
  let newerReads = 0;
  await worker.send({ kind: "import", id: 1, files: [selectedFile("old/a.bms", "old", () => { reads++; return pending.promise; })] });
  assert.equal(reads, 1);
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
  assert.equal(worker.libraries.length, 1);
  assert.equal(worker.of("catalog").length, 0);
  pending.resolve(new TextEncoder().encode("old").buffer);
  await flushJobs();
  assert.equal(newerReads, 1);
  assert.equal(skippedReads, 0);
  assert.equal(worker.libraries.length, 2);
  assert.equal(worker.libraries[0].frees, 1);
  assert.equal(worker.libraries[0].files.length, 0);
  assert.equal(worker.libraries[1].frees, 0);
  assert.deepEqual(worker.libraries[1].files.map(entry => entry.path), ["new/c.bms"]);
  assert.deepEqual(worker.of("catalog").map(message => message.id), [3]);
  assert.equal(worker.of("import-error").length, 0);
  await worker.send({ kind: "accept-library", id: 3 });
  await worker.send({ kind: "select", id: 4, libraryId: 3, path: "new/c.bms", rate: 48000, seed: "0" });
  assert.equal(worker.views[0].current.path, "new/c.bms");
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

  await worker.send({ kind: "import", id: 5, files: [selectedFile("failed/b.bms", "x", () => Promise.reject(new Error("file read failed")))] });
  const failed = worker.libraries[2];
  assert.equal(ignored.frees, 1);
  assert.equal(failed.frees, 1);
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
  const replacement = worker.libraries[3];
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
  await worker.send({ kind: "select", id: 3, libraryId: 1, path: "bad.bms", rate: 48000, seed: "7" });
  assert.equal(worker.views[0].current, original);
  assert.equal(original.frees, 0);
  assert.equal(original.releasedByView, undefined);
  assert.equal(worker.views[0].replacements.length, 1);
  assert.deepEqual(worker.of("selected").map(message => message.id), [2]);
  assert.equal(worker.of("selection-error")[0].id, 3);
  assert.match(worker.of("selection-error")[0].message, /sample rate mismatch/);
  await worker.send({ kind: "seek", id: 4, selectedId: 2, ns: "604800000000001" });
  assert.deepEqual(worker.views[0].positions, [604800000000001n]);
  assert.equal(worker.of("position")[0].selectedId, 2);
});

test("WASM or GPU initialization failure never reports readiness or admits later work", async () => {
  for (const options of [{ initError: "missing WASM" }, { createError: "GPU unavailable" }]) {
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

test("surface retries are bounded and zero extent cancels the pending callback", async () => {
  const worker = await readyWorker({ needsRedraw: true });
  await worker.send({ kind: "resize", width: 960, height: 720 });
  await worker.send({ kind: "import", id: 1, files: [selectedFile("a.bms")] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "a.bms", rate: 48000, seed: "0" });
  assert.equal(worker.timers.size, 1);
  for (let index = 0; index < 4; index++) await worker.tick();
  assert.equal(worker.views[0].draws, 4);
  assert.equal(worker.timers.size, 0);
  assert.equal(worker.of("render-wait").length, 1);
  assert.equal(worker.of("drawn").length, 0);
  await worker.send({ kind: "seek", id: 3, selectedId: 2, ns: "1" });
  assert.equal(worker.timers.size, 1);
  await worker.send({ kind: "resize", width: 0, height: 720 });
  assert.equal(worker.timers.size, 0);
  assert.equal(worker.views[0].draws, 4);
  await worker.send({ kind: "resize", width: 960, height: 720 });
  assert.equal(worker.timers.size, 1);
});
