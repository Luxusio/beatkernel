// Deferred source fixtures: actual selection/boundary module, no browser or WASM.
import assert from "node:assert/strict";
import { File } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

async function model() {
  const context = createContext({ File, TextEncoder, Uint8Array, ArrayBuffer, structuredClone });
  const module = new SourceTextModule(await readFile(new URL("./saved-opponents.mjs", import.meta.url), "utf8"), { context });
  await module.link(specifier => { throw new Error(`Unexpected model import ${specifier}`); });
  await module.evaluate();
  return module.namespace;
}
function descriptor(key = "file:1", byteLength = 4) {
  const file = new File([Uint8Array.from([66, 75, 82, 1])], `${key}.bkr`);
  Object.defineProperty(file, "size", { value: byteLength });
  file.arrayBuffer = () => { throw new Error("selection must not acquire bytes"); };
  return { file, sourceKey: key, own: true, label: "Past record" };
}
function summary(fields = {}) {
  return { kind: "own", label: "<untrusted display>", songNs: -1n, recordedUntilNs: null,
    hits: 2n, misses: 1n, combo: 1n, maxCombo: 2n, ...fields };
}

test("selection snapshots retain immutable Files without acquiring or transferring their bytes", async () => {
  const { SavedOpponentSelection, validateSelections } = await model();
  const input = Uint8Array.from([66, 75, 82, 255]);
  const file = new File([input], "source.bkr");
  const acquire = file.arrayBuffer.bind(file);
  let reads = 0;
  file.arrayBuffer = () => { reads++; return acquire(); };
  const choice = { file, sourceKey: "file:1", own: false, label: "  Other  " };
  const selection = new SavedOpponentSelection();
  const admitted = selection.add(choice);
  choice.label = "changed caller container";
  const first = selection.snapshot();
  const second = selection.snapshot();
  assert.notEqual(first, second);
  assert.equal(first[0].file, file);
  assert.equal(second[0].file, file);
  assert.equal(admitted.label, "  Other  ");
  assert.ok(Object.isFrozen(admitted) && Object.isFrozen(first) && Object.isFrozen(first[0]));
  const request = validateSelections(first);
  assert.equal(request[0].file, file);
  assert.equal(reads, 0);
  structuredClone(input.buffer, { transfer: [input.buffer] });
  assert.equal(input.byteLength, 0);
  assert.equal(selection.byteLength, 4);
  assert.deepEqual(new Uint8Array(await file.arrayBuffer()), Uint8Array.from([66, 75, 82, 255]));
  assert.equal(selection.remove("file:1"), true);
  assert.equal(selection.remove("file:1"), false);
  assert.equal(selection.size, 0);
  assert.equal(selection.byteLength, 0);
  assert.equal(first[0].file, file, "a detached request container cannot mutate a retained File");
  selection.add({ ...choice, label: "retry" });
  selection.clear();
  assert.equal(selection.size, 0);
  assert.equal(selection.byteLength, 0);
});

test("count, duplicate identity and aggregate byte limits reject atomically and removal restores quota", async () => {
  const { SavedOpponentSelection, OPPONENT_LIMITS } = await model();
  assert.equal(OPPONENT_LIMITS.count, 8);
  assert.equal(OPPONENT_LIMITS.bytes, 64 * 1024 * 1024);
  const selected = new SavedOpponentSelection();
  for (let index = 0; index < 8; index++) selected.add(descriptor(`file:${index}`, 1));
  const before = selected.snapshot();
  assert.throws(() => selected.add(descriptor("file:8", 1)));
  assert.throws(() => selected.add({ ...descriptor("file:0", 1), own: false }));
  assert.deepEqual(Array.from(selected.snapshot(), row => row.sourceKey), Array.from(before, row => row.sourceKey));
  assert.equal(selected.byteLength, 8);
  assert.equal(selected.remove("file:3"), true);
  selected.add(descriptor("file:8", 1));
  assert.equal(selected.size, 8);
  const quota = new SavedOpponentSelection();
  quota.add(descriptor("record:1", OPPONENT_LIMITS.bytes - 1));
  assert.throws(() => quota.add(descriptor("record:2", 2)));
  assert.equal(quota.size, 1);
  assert.equal(quota.byteLength, OPPONENT_LIMITS.bytes - 1);
  quota.add(descriptor("record:2", 1));
  assert.equal(quota.byteLength, OPPONENT_LIMITS.bytes);
  quota.remove("record:1");
  quota.add(descriptor("record:3", OPPONENT_LIMITS.bytes - 1));
  assert.equal(quota.byteLength, OPPONENT_LIMITS.bytes);
});

test("request preflight checks actual Files, UTF-8 labels and source keys before any acquisition", async () => {
  const { SavedOpponentSelection, validateSelections, opponentLabel } = await model();
  assert.throws(() => validateSelections(undefined));
  assert.equal(validateSelections([]).length, 0);
  const good = descriptor();
  for (const bad of [
    { ...good, file: { size: 4, arrayBuffer() { throw new Error("not a File"); } } },
    { ...good, file: descriptor("empty", 0).file },
    { ...good, file: descriptor("too-big", 67108865).file },
    { ...good, file: descriptor("fraction", 1.5).file },
    { ...good, own: "own" }, { ...good, label: "" }, { ...good, label: "é".repeat(128) + "x" },
    { ...good, label: "line\nfeed" }, { ...good, label: "\ud800" },
    { ...good, sourceKey: "" }, { ...good, sourceKey: "é".repeat(513) },
    { ...good, sourceKey: "bad\0key" },
  ]) {
    const selection = new SavedOpponentSelection();
    assert.throws(() => selection.add(bad));
    assert.equal(selection.size, 0);
    assert.equal(selection.byteLength, 0);
    assert.throws(() => validateSelections([bad]));
  }
  assert.throws(() => validateSelections([good, good]));
  assert.throws(() => validateSelections(new Array(1)));
  assert.throws(() => validateSelections(Array.from({ length: 9 }, (_, index) => descriptor(`file:${index}`, 1))));
  assert.throws(() => validateSelections([descriptor("a", 67108864), descriptor("b", 1)]));
  const exact = validateSelections([{ ...good, label: "é".repeat(128), sourceKey: "é".repeat(512) }]);
  assert.equal(exact[0].label.length, 128);
  assert.equal(exact[0].file, good.file);
  const longName = "曲🎵".repeat(100) + ".bkr";
  const label = opponentLabel(longName);
  assert.ok(new TextEncoder().encode(label).length >= 1 && new TextEncoder().encode(label).length <= 256);
  assert.equal(label, opponentLabel(longName));
  assert.doesNotThrow(() => validateSelections([{ ...good, label }]));
  assert.equal(good.file.name, "file:1.bkr");
});

test("comparison snapshots preserve signed prefix evidence and reject malformed or inconsistent actual counters", async () => {
  const { validateOpponentSnapshot } = await model();
  const low = -9223372036854775808n;
  const high = 18446744073709551615n;
  const input = [summary({ songNs: low }), summary({ kind: "other", songNs: 9223372036854775807n,
    recordedUntilNs: -100000000n, hits: high, misses: 0n, combo: high, maxCombo: high })];
  const rows = validateOpponentSnapshot(input, 2);
  assert.equal(rows[0].songNs, low);
  assert.equal(rows[0].recordedUntilNs, null);
  assert.equal(rows[1].recordedUntilNs, -100000000n);
  assert.equal(rows[1].hits, high);
  input[0].label = "mutated outer row";
  assert.equal(rows[0].label, "<untrusted display>");
  assert.ok(Object.isFrozen(rows) && Object.isFrozen(rows[0]));
  assert.throws(() => validateOpponentSnapshot([], 0));
  assert.throws(() => validateOpponentSnapshot([summary()], 2));
  assert.throws(() => validateOpponentSnapshot(new Array(1), 1));
  for (const patch of [
    { kind: "ranked" }, { label: "bad\nlabel" }, { songNs: null }, { songNs: 1 },
    { songNs: low - 1n }, { recordedUntilNs: 9223372036854775808n },
    { recordedUntilNs: undefined }, { hits: "2" }, { misses: -1n }, { hits: high + 1n },
    { combo: 3n }, { maxCombo: 3n },
  ]) assert.throws(() => validateOpponentSnapshot([summary(patch)], 1));
});

test("saved targets use stable u32 member IDs without acquiring Files or changing count and byte charges", async () => {
  const { SavedOpponentSelection, validateSelections, validateOpponentTargets } = await model();
  const selected = new SavedOpponentSelection(), original = descriptor();
  selected.add(original); selected.add({ ...descriptor("file:2"), player: 0xffffffff });
  const before = selected.snapshot();
  assert.equal(Object.hasOwn(before[0], "player"), false);
  selected.setPlayer(original.sourceKey, 7);
  assert.equal(selected.snapshot()[0].player, 7); assert.equal(Object.hasOwn(before[0], "player"), false);
  assert.equal(selected.snapshot()[0].file, original.file); assert.equal(selected.byteLength, 8); assert.equal(selected.size, 2);
  assert.doesNotThrow(() => validateOpponentTargets(selected.snapshot(), [7, 91, 0xffffffff]));
  assert.throws(() => validateOpponentTargets(selected.snapshot(), [1, 2, 3]));
  assert.throws(() => validateOpponentTargets(selected.snapshot()));
  for (const target of [0, -1, 4294967296, 1.5, "7", 7n, NaN]) {
    assert.throws(() => selected.setPlayer(original.sourceKey, target));
    assert.equal(selected.snapshot()[0].player, 7); assert.equal(selected.byteLength, 8);
    assert.throws(() => validateSelections([{ ...original, player: target }]));
  }
  assert.throws(() => selected.setPlayer("retired-source", 7));
  selected.setPlayer(original.sourceKey, null); selected.setPlayer("file:2", undefined);
  assert.ok(selected.snapshot().every(row => !Object.hasOwn(row, "player")));
  assert.doesNotThrow(() => validateOpponentTargets(selected.snapshot()));
  assert.throws(() => validateOpponentTargets(selected.snapshot(), [7, 91]));
  assert.equal(selected.snapshot()[0].file, original.file); assert.equal(selected.byteLength, 8);
});

test("local comparison snapshots require exact member ownership but isolate malformed rows from healthy and empty member prefixes", async () => {
  const { validateLocalOpponentSnapshot } = await model();
  const players = [7, 0xffffffff, 91], selections = [
    { ...descriptor(), label: "<untrusted display>", player: 7 },
    { ...descriptor("file:2"), own: false, label: "<untrusted display>", player: 0xffffffff },
  ];
  const fresh = () => [
    { player: 7, opponents: [summary({ songNs: -9223372036854775808n })], error: null },
    { player: 0xffffffff, opponents: [summary({ kind: "other", hits: 18446744073709551615n, misses: 0n, combo: 0n, maxCombo: 18446744073709551615n })], error: null },
    { player: 91, opponents: [], error: null },
  ];
  const input = fresh(), saved = validateLocalOpponentSnapshot(input, players, selections);
  assert.equal(saved[0].opponents[0].songNs, -9223372036854775808n);
  assert.equal(saved[1].opponents[0].hits, 18446744073709551615n); assert.equal(saved[2].opponents.length, 0);
  assert.ok(Object.isFrozen(saved) && saved.every(Object.isFrozen));
  input[0].opponents[0].label = "changed caller"; assert.equal(saved[0].opponents[0].label, "<untrusted display>");
  for (const bad of [null, [], new Array(3), fresh().reverse(), [...fresh().slice(0, 2), { player: 7, opponents: [], error: null }]]) {
    assert.throws(() => validateLocalOpponentSnapshot(bad, players, selections));
  }
  for (const bad of [{ player: 0xffffffff, opponents: [], error: null },
    { player: 0xffffffff, opponents: [summary({ hits: 1 })], error: null },
    { player: 0xffffffff, opponents: [summary({ kind: "other", label: "wrong record" })], error: null },
    { player: 0xffffffff, opponents: [summary()], error: null },
    { player: 0xffffffff, opponents: null, error: "" },
    { player: 0xffffffff, opponents: [], error: "actual failure" },
    { player: 0xffffffff, opponents: null, error: "x".repeat(4097) }]) {
    const rows = fresh(); rows[1] = bad;
    const result = validateLocalOpponentSnapshot(rows, players, selections);
    assert.equal(result[0].error, null); assert.equal(result[0].opponents.length, 1);
    assert.equal(result[1].opponents, null); assert.equal(typeof result[1].error, "string");
    assert.equal(result[2].error, null); assert.equal(result[2].opponents.length, 0);
  }
  const failed = fresh(); failed[1] = { player: 0xffffffff, opponents: null, error: "member HUD unavailable" };
  assert.equal(validateLocalOpponentSnapshot(failed, players, selections)[1].error, "member HUD unavailable");
});
