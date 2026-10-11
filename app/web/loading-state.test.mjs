// Actual main.js loading handlers through the existing controlled Window harness.
// This suite makes no engine-readiness claim; real jobs are worker.test evidence.
import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";

let hostHarness;
async function harness(faults = {}) {
  if (!hostHarness) {
    const url = new URL("./play-host.test.mjs", import.meta.url);
    const source = await readFile(url, "utf8");
    assert.match(source, /async function harness\(faults = \{\}\)/);
    assert(source.includes('"prepare-cancel"') && source.includes('"prepare-progress"'), "existing DOM fixture must admit the new real controls");
    const context = createContext({ console, URL, structuredClone, WebAssembly, queueMicrotask, AbortController, AbortSignal,
      TextEncoder, TextDecoder, Uint8Array, Uint32Array, Float32Array, DataView, ArrayBuffer });
    let registrations = 0;
    const modules = new Map();
    const module = new SourceTextModule(source + "\nexport { harness };\n", { context, identifier: url.href,
      initializeImportMeta(meta) { meta.url = url.href; } });
    await module.link(async specifier => {
      if (modules.has(specifier)) return modules.get(specifier);
      const namespace = specifier === "node:test" ? { default: () => { registrations++; } }
        : await import(specifier.startsWith("node:") ? specifier : new URL(specifier, url).href);
      const names = Object.keys(namespace);
      const dependency = new SyntheticModule(names, function () { for (const name of names) this.setExport(name, namespace[name]); }, { context });
      modules.set(specifier, dependency); return dependency;
    });
    await module.evaluate(); assert(registrations > 20, "reuse fixture helpers while suppressing all unrelated test execution");
    assert.equal(typeof module.namespace.harness, "function"); hostHarness = module.namespace.harness;
  }
  return hostHarness(faults);
}
const flush = async () => { for (let i = 0; i < 48; i++) await Promise.resolve(); };
const progress = (request, fields = {}) => ({ kind: "selection-progress", id: request.id, libraryId: request.libraryId, path: request.path,
  stage: "decode", completedAssets: 0, totalAssets: 1, parses: 1, compiles: 1, reads: 1, decodes: 0, sharedAssets: 0,
  maxStepMs: 0.25, beginMs: 1000, ...fields });
async function pendingPreview(h) {
  await h.preview(); h.get("prepare-form").emit("submit"); await flush();
  return h.workers.at(-1).last("select");
}

test("actual Window exposes correlated phase counts and one keyboard-accessible Cancel request", async () => {
  const h = await harness();
  try {
    const request = await pendingPreview(h), worker = h.workers.at(-1);
    await h.receive(progress(request));
    assert.equal(h.get("prepare-cancel").disabled, false);
    assert(h.get("prepare-progress").textContent.includes("decode"));
    assert(!h.get("prepare-progress").textContent.includes("%"), "no invented percentage");
    h.click("prepare-cancel"); await flush();
    const cancellation = worker.last("select-cancel"); assert(cancellation);
    assert.equal(cancellation.id, request.id); assert.equal(cancellation.libraryId, request.libraryId);
    await h.receive({ kind: "selection-cancelled", id: request.id, libraryId: request.libraryId });
    assert.equal(h.get("prepare-cancel").disabled, true);
    assert.equal(h.get("prepare").disabled, false);
    assert.equal(h.get("play").disabled, false, "last accepted preview remains usable");
    h.click("prepare-cancel"); assert.equal(worker.posts.filter(x => x.value.kind === "select-cancel").length, 1);
  } finally { await h.close(); }
});

test("actual Window ignores foreign progress/cancel owners and prevents stale phase text replacement", async () => {
  const h = await harness();
  try {
    const request = await pendingPreview(h);
    await h.receive(progress(request)); const before = h.get("prepare-progress").textContent;
    for (const fields of [{ id: request.id + 1 }, { libraryId: request.libraryId + 1 }]) {
      await h.receive(progress(request, { stage: "ready", ...fields }));
      assert.equal(h.get("prepare-progress").textContent, before);
    }
    await h.receive({ kind: "selection-cancelled", id: request.id + 1, libraryId: request.libraryId });
    assert.equal(h.get("prepare-cancel").disabled, false);
    h.click("prepare-cancel"); await h.receive({ kind: "selection-cancelled", id: request.id, libraryId: request.libraryId });
    const stopped = h.get("prepare-progress").textContent;
    await h.receive(progress(request, { stage: "ready", completedAssets: 1, decodes: 1 }));
    assert.equal(h.get("prepare-progress").textContent, stopped);
    assert.equal(h.get("prepare-cancel").disabled, true);
  } finally { await h.close(); }
});

test("actual Window shows resource counts without claiming readiness and retains prior preview on current-owner error", async () => {
  const h = await harness();
  try {
    const request = await pendingPreview(h);
    await h.receive(progress(request));
    await h.receive(progress(request, { stage: "admit", completedAssets: 1, decodes: 1 }));
    assert(h.get("prepare-progress").textContent.includes("1/1"));
    assert.equal(h.get("play").disabled, true, "progress counters do not admit a new playable chart");
    const title = h.get("title").textContent;
    await h.receive({ kind: "selection-error", id: request.id, libraryId: request.libraryId, message: "actual decoder rejected current resource" });
    assert.equal(h.get("title").textContent, title);
    assert.equal(h.get("prepare-cancel").disabled, true);
    assert.equal(h.get("prepare").disabled, false); assert.equal(h.get("play").disabled, false);
    assert(h.get("status").textContent.includes("decoder rejected"));
  } finally { await h.close(); }
});

test("actual Window allows the existing menu editor input path while current preparation is pending", async () => {
  const h = await harness();
  try {
    await h.preview();
    const worker = h.workers.at(-1);
    h.click("menu-open"); await flush();
    await h.receive({ kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n, route: 2, selected: 0, fields: ["50", "50", "0"] });
    h.get("prepare-form").emit("submit"); await flush(); const request = worker.last("select");
    await h.receive(progress(request));
    h.get("menu-editor").value = "51"; h.get("menu-editor").emit("input", { isComposing: false }); await flush();
    const edit = worker.last("menu-edit"); assert(edit, "legal active editor input reaches its existing business route");
    assert.equal(edit.screen, 3n); assert.equal(edit.index, 0); assert.equal(edit.value, "51");
    h.click("prepare-cancel"); await h.receive({ kind: "selection-cancelled", id: request.id, libraryId: request.libraryId });
  } finally { await h.close(); }
});

test("actual Window keeps menu keyboard editing independent of a pending Worker record listing", async () => {
  const h = await harness({ holdRecordsReply: true });
  try {
    await h.preview(); const worker = h.workers.at(-1);
    h.click("menu-open"); await flush();
    await h.receive({ kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 2, selected: 0, fields: ["50", "50", "0"] });
    h.click("records-refresh"); await flush();
    const request = worker.last("records-request"); assert.equal(request.action, "list");
    assert.equal(h.get("records-refresh").disabled, true);
    h.get("menu-editor").value = "51";
    h.get("menu-editor").emit("input", { isComposing: false }); await flush();
    const edit = worker.last("menu-edit"); assert.ok(edit);
    assert.equal(edit.screen, 3n); assert.equal(edit.index, 0); assert.equal(edit.value, "51");
    assert.equal(h.recordCalls.length, 0, "Window listing intent has no database/body work");
    await h.receive({ kind: "records-reply", id: request.id, action: "list", result: { entries: [] }, error: null });
    assert.equal(h.get("records-refresh").disabled, false);
  } finally { await h.close(); }
});
