// Deferred pure finite metadata admission. No result/gauge is manufactured here.
import assert from "node:assert/strict";
import test from "node:test";
import { validateCompletedResults, resultRequest, validateCompletedArchive } from "./completed-results-model.mjs";
const metadata = (extra = {}) => ({ proof: true, players: [4294967295, 7], page: 0,
  pages: 1, detailPages: 1, comparisonPages: 3, comparisons: false, hasComparisons: true, failed: false, ...extra });
const owner = (extra = {}) => ({ ...metadata(), id: 9007199254740991, lastRpc: 3,
  ready: false, shown: false, ...extra });
test("archive admission preserves exact roster and opaque bytes without trusting replay completion",()=>{
  const context={playId:7,players:[4294967295,7],recording:true,mode:"live"};
  const bytes=Uint8Array.from([1,2,3]);const message={playId:7,completedArchive:bytes,archivePlayers:[4294967295,7],archiveError:null,replayComplete:false};
  assert.equal(validateCompletedArchive(message,context).bytes,bytes);
  assert.deepEqual(validateCompletedArchive({playId:7},context),{bytes:null,error:null});
  assert.deepEqual(validateCompletedArchive({...message,completedArchive:null,archivePlayers:null,archiveError:"encode refused"},context),{bytes:null,error:"encode refused"});
  for(const bad of [{playId:8},{archivePlayers:[7,4294967295]},{archivePlayers:[4294967295]},
    {archivePlayers:[7,7]},{archiveError:"not null"},{completedArchive:new Uint8Array(0)},
    {completedArchive:new Uint8Array(5*1024*1024+1)},{completedArchive:new Uint8Array(4).subarray(1)},
    {replay:bytes},{replays:[{player:4294967295,replay:bytes}]}]){
    assert.throws(()=>validateCompletedArchive({...message,...bad},context));
  }
  for(const bad of [{players:[0,7]},{players:[4294967296,7]},{players:[7,7]},{recording:false},{mode:"replay"}]){
    assert.throws(()=>validateCompletedArchive(message,{...context,...bad}));
  }
  if(typeof ArrayBuffer.prototype.resize==="function"){
    const resizable=new Uint8Array(new ArrayBuffer(3,{maxByteLength:8}));
    assert.throws(()=>validateCompletedArchive({...message,completedArchive:resizable},context));
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
