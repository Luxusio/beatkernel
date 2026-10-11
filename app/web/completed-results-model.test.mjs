// Deferred pure finite metadata admission. No result/gauge is manufactured here.
import assert from "node:assert/strict";
import test from "node:test";
import { validateCompletedResults, resultRequest, validateCompletedArchive, validateCapturedReplay } from "./completed-results-model.mjs";
const metadata = (extra = {}) => ({ proof: true, players: [4294967295, 7], page: 0,
  pages: 1, detailPages: 1, comparisonPages: 3, comparisons: false, hasComparisons: true, failed: false, ...extra });
const owner = (extra = {}) => ({ ...metadata(), id: 9007199254740991, lastRpc: 3,
  ready: false, shown: false, ...extra });
const replayContext = (extra = {}) => ({ playId: 7, recording: true, natural: true, mode: "live", ...extra });
const replayReceipt = (file, extra = {}) => ({ kind: "play-stopped", playId: 7,
  replay: file, replayComplete: true, replayError: null, ...extra });
function unreadableHandle(handle) {
  for (const method of ["arrayBuffer", "text", "stream", "bytes", "slice"]) {
    Object.defineProperty(handle, method, { value() { throw new Error(`Admission must not call ${method}`); } });
  }
  return handle;
}
test("replay admission retains genuine File without reading its body and respects completion context", () => {
  const file = unreadableHandle(new File([Uint8Array.from([1, 2, 3])], "capture.bkr"));
  assert.deepEqual(validateCapturedReplay(replayReceipt(file), replayContext()), { file, complete: true, error: null });
  assert.deepEqual(validateCapturedReplay(replayReceipt(file, { replayComplete: false }), replayContext({ natural: false })),
    { file, complete: false, error: null });
  assert.deepEqual(validateCapturedReplay(replayReceipt(file, { kind: "play-error", replayComplete: false }), replayContext()),
    { file, complete: false, error: null });
  // Solo captures remain downloadable even when a later cleanup diagnostic fails
  // the joined session; Window decides the final prefix label after cleanup.
  for (const error of ["", "cleanup failed", "x".repeat(4096)]) {
    assert.deepEqual(validateCapturedReplay(replayReceipt(file, { replayError: error }), replayContext()),
      { file, complete: true, error });
  }
  for (const extra of [{ natural: false }, { recording: false }, { mode: "replay" }]) {
    assert.throws(() => validateCapturedReplay(replayReceipt(file), replayContext(extra)));
  }
  assert.throws(() => validateCapturedReplay(replayReceipt(file, { kind: "play-error" }), replayContext()));
});
test("replay absence and bounded diagnostics preserve optional nonrecording and failed capture semantics", () => {
  assert.deepEqual(validateCapturedReplay({ playId: 7 }, replayContext({ recording: false })),
    { file: null, complete: false, error: null });
  assert.deepEqual(validateCapturedReplay(replayReceipt(null, { replayComplete: false }), replayContext()),
    { file: null, complete: false, error: null });
  for (const error of ["", "encode refused", "x".repeat(4096)]) {
    assert.deepEqual(validateCapturedReplay(replayReceipt(null, { replayComplete: false, replayError: error }), replayContext()),
      { file: null, complete: false, error });
  }
  assert.throws(() => validateCapturedReplay({ playId: 7 }, replayContext()));
  assert.throws(() => validateCapturedReplay(replayReceipt(null), replayContext()));
  for (const replayError of [undefined, false, {}, "x".repeat(4097)]) {
    assert.throws(() => validateCapturedReplay(replayReceipt(null, { replayComplete: false, replayError }), replayContext()));
  }
});
test("replay admission rejects raw and forged handles and malformed current-owner metadata", () => {
  const file = new File(["x"], "capture.bkr");
  for (const replay of [new Uint8Array([1]), new ArrayBuffer(1), new Blob(["x"]),
    { name: "capture.bkr", size: 1, arrayBuffer() { throw new Error("must not read fake"); } },
    Object.create(File.prototype), new File([], "empty.bkr")]) {
    assert.throws(() => validateCapturedReplay(replayReceipt(replay), replayContext()));
  }
  for (const extra of [{ playId: 8 }, { replayComplete: 1 }, { replayComplete: undefined },
    { replayError: "x".repeat(4097) }]) {
    assert.throws(() => validateCapturedReplay(replayReceipt(file, extra), replayContext()));
  }
  for (const extra of [{ playId: 0 }, { playId: Number.MAX_SAFE_INTEGER + 1 },
    { recording: "true" }, { natural: undefined }, { mode: "unknown" }]) {
    assert.throws(() => validateCapturedReplay(replayReceipt(file), replayContext(extra)));
  }
  for (const value of [null, undefined, [], "receipt"]) {
    assert.throws(() => validateCapturedReplay(value, replayContext()));
  }
});
test("replay File size ceiling admits exactly 64 MiB and rejects one byte over without a body read", () => {
  const part = new Uint8Array(1024 * 1024);
  const maximum = unreadableHandle(new File(Array(64).fill(part), "maximum.bkr"));
  assert.equal(validateCapturedReplay(replayReceipt(maximum), replayContext()).file, maximum);
  const oversized = unreadableHandle(new File([maximum, new Uint8Array(1)], "oversized.bkr"));
  assert.throws(() => validateCapturedReplay(replayReceipt(oversized), replayContext()));
});
test("archive admission preserves exact roster and opaque Blob without trusting replay completion",()=>{
  const context={playId:7,players:[4294967295,7],recording:true,mode:"live"};
  const blob=unreadableHandle(new Blob([Uint8Array.from([1,2,3])]));const message={playId:7,completedArchive:blob,archivePlayers:[4294967295,7],archiveError:null,replayComplete:false};
  assert.equal(validateCompletedArchive(message,context).blob,blob);
  assert.deepEqual(validateCompletedArchive({playId:7},context),{blob:null,error:null});
  assert.deepEqual(validateCompletedArchive({...message,completedArchive:null,archivePlayers:null,archiveError:"encode refused"},context),{blob:null,error:"encode refused"});
  for(const bad of [{playId:8},{archivePlayers:[7,4294967295]},{archivePlayers:[4294967295]},
    {archivePlayers:[7,7]},{archiveError:"not null"},{completedArchive:new Blob([])},
    {completedArchive:new Blob([new Uint8Array(5*1024*1024+1)])},
    {completedArchive:new Uint8Array([1,2,3])},{completedArchive:{size:3}},
    {completedArchive:Object.create(Blob.prototype)}]){
    assert.throws(()=>validateCompletedArchive({...message,...bad},context));
  }
  for(const bad of [{players:[0,7]},{players:[4294967296,7]},{players:[7,7]},{recording:false},{mode:"replay"}]){
    assert.throws(()=>validateCompletedArchive(message,{...context,...bad}));
  }
  // Sharing an immutable handle is not evidence of underlying byte aliasing.
  // Worker validates standalone raw capture buffers before constructing handles.
  assert.equal(validateCompletedArchive({...message,replay:blob,replays:[{player:4294967295,replay:blob}]},context).blob,blob);
});
test("archive size and diagnostic boundaries require exact original whole-roster association", () => {
  const context = { playId: 7, players: [4294967295, 7], recording: true, mode: "live" };
  const blob = unreadableHandle(new Blob([new Uint8Array(5 * 1024 * 1024)]));
  const message = { playId: 7, completedArchive: blob, archivePlayers: [...context.players], archiveError: null };
  assert.equal(validateCompletedArchive(message, context).blob, blob);
  assert.deepEqual(validateCompletedArchive({ ...message, completedArchive: null, archivePlayers: null,
    archiveError: "x".repeat(4096) }, context), { blob: null, error: "x".repeat(4096) });
  for (const archiveError of ["", "x".repeat(4097), undefined, false]) {
    assert.throws(() => validateCompletedArchive({ ...message, completedArchive: null, archivePlayers: null, archiveError }, context));
  }
  assert.throws(() => validateCompletedArchive({ ...message, completedArchive: null }, context));
  assert.throws(() => validateCompletedArchive({ ...message, archivePlayers: undefined }, context));
  assert.deepEqual(validateCompletedArchive({ playId: 7, completedArchive: null, archivePlayers: null, archiveError: null },
    { ...context, recording: false, mode: "replay" }), { blob: null, error: null });
  const maximumRoster = Array.from({ length: 64 }, (_, index) => 4294967295 - index);
  assert.equal(validateCompletedArchive({ ...message, archivePlayers: maximumRoster },
    { ...context, players: maximumRoster }).blob, blob);
  for (const players of [[], [...maximumRoster, 1], [7, 1.5], [7, NaN]]) {
    assert.throws(() => validateCompletedArchive(message, { ...context, players }));
  }
});
test("admission copies exact original IDs and freezes finite metadata", () => {
  const input = metadata(); const first = validateCompletedResults(input);
  input.players[0] = 1;
  assert.deepEqual(first.players, [4294967295, 7]);
  assert.ok(Object.isFrozen(first)); assert.ok(Object.isFrozen(first.players));
});
test("whole bounded roster and every malformed metadata row are validated atomically", () => {
  const ids = Array.from({ length: 64 }, (_, index) => 4294967295 - index * 17);
  assert.deepEqual(validateCompletedResults(metadata({ players: ids, pages: 16, detailPages: 16, page: 15 })).players, ids);
  for (const bad of [ { proof: false }, { players: [] }, { players: [...ids, 1] },
    { players: [7, 7] }, { players: [7, 0] }, { players: [7, 4294967296] },
    { players: [7, 1.5] }, { page: -1 }, { page: 1 }, { pages: 0 },
    { comparisons: true, hasComparisons: false }, { failed: "false" },
    { failed: true, pages: 1 }, { failed: true, pages: 0, page: 1 } ]) {
    assert.throws(() => validateCompletedResults(metadata(bad)));
  }
  assert.equal(validateCompletedResults(metadata({ failed: true, pages: 0, detailPages: 0, comparisonPages: 0, hasComparisons: false })).failed, true);
});
test("release and original play plus monotonic RPC gate immutable control candidates", () => {
  const original = owner();
  const show = { kind: "play-results-present", playId: original.id, rpcId: 4 };
  assert.throws(() => resultRequest(original, show));
  const ready = owner({ ready: true });
  const shown = resultRequest(ready, show);
  assert.equal(ready.shown, false); assert.equal(ready.lastRpc, 3);
  assert.equal(shown.shown, true); assert.equal(shown.lastRpc, 4);
  for (const request of [ { ...show, playId: 7 }, { ...show, rpcId: 3 },
    { ...show, rpcId: Number.MAX_SAFE_INTEGER + 1 },
    { kind: "play-results-page", playId: ready.id, rpcId: 4, page: 0, comparisons: true } ]) {
    assert.throws(() => resultRequest(ready, request));
  }
  const page = resultRequest(shown, { kind: "play-results-page", playId: shown.id, rpcId: 5, page: 2, comparisons: true });
  assert.equal(page.page, 2); assert.equal(page.comparisons, true);
  assert.equal(shown.page, 0); assert.equal(shown.lastRpc, 4);
  assert.throws(() => resultRequest(null, show));
  assert.throws(() => resultRequest(owner({ ready: true, failed: true }), show));
});
