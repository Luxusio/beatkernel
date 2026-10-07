// Deferred actual Worker completed Results ownership and cleanup barriers.
// Actual Worker and numeric helpers; only generated WASM owners and browser APIs are mocked.
import assert from "node:assert/strict";
import { File as NodeFile } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";
import { encodeKeyboardEvent, encodeTouchEvent, encodeRawHidEvent } from "./physical-input.mjs";

import { millisecondsToNanos } from "./play-model.mjs";

const FileType = globalThis.File ?? NodeFile;
const ORIGIN = 9007199254740993n;
const START = 9007199254741999n;
const SCORE = { song_ns: 123456789012345n, hits: 17n, misses: 3n, combo: 9n, max_combo: 15n };
const pairs = () => new Uint32Array([0x11, 2, 0x12, 3]);
const command = (voice = 7n) => ({ kind: 0, voice, sample: 19n, at: 100000001n, gain: 0.5, value: 0n, denominator: 1n });
const batch = sequence => ({ sequence, commands: [command(sequence), command(sequence + 1n)] });

function startRequest(fields = {}) {
  return { kind: "play-start", playId: 7, rpcId: 1, libraryId: 1, path: "song/chart.bms", rate: 48000,
    seed: "18446744073709551615", keyPairs: pairs(), windowOriginNs: fields.multiplayer?.windowOriginNs ?? 10000000000n, ...fields };
}

function replayFile(acquire = null, size = 6) {
  const bytes = Uint8Array.from([66, 75, 82, 0, 255, 1]);
  const file = new FileType([bytes], "original-recording.bkr");
  let reads = 0;
  Object.defineProperty(file, "size", { value: size });
  file.arrayBuffer = () => { reads++; return acquire ? acquire() : Promise.resolve(bytes.slice().buffer); };
  return { file, bytes, get reads() { return reads; } };
}
function replayRequest(file, fields = {}) {
  return { kind: "play-start", playId: 7, rpcId: 1, libraryId: 1,
    path: "song/chart.bms", rate: 48000, mode: "replay", replayFile: file, ...fields };
}

async function catalogWorker(options = {}) {
  const h = await workerHarness(options);
  await h.send({ kind: "init", canvas: { transferred: true } });
  assert.equal(h.of("ready").length, 1);
  await h.send({ kind: "import", id: 1, files: [selectedFile("song/chart.bms")] });
  await h.send({ kind: "accept-library", id: 1 });
  await h.send({ kind: "select", id: 2, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" });
  await h.send({ kind: "resize", width: 640, height: 480 });
  return h;
}

async function started(options = {}) {
  const h = await catalogWorker(options);
  await h.send(options.startRequest ?? startRequest(options.recordReplay === undefined ? {} : { recordReplay: options.recordReplay }));
  assert.equal(h.of("play-reply").at(-1).result.kind, "prepared");
  return withPlayRpc(h);
}

function withPlayRpc(h) {
  h.rpcId = 1;
  h.rpc = async (kind, fields = {}) => {
    const rpcId = ++h.rpcId;
    await h.send({ kind, playId: 7, rpcId, ...fields });
    const reply = h.of("play-reply").find(value => value.rpcId === rpcId);
    assert.ok(reply, `missing reply ${rpcId}`);
    return reply;
  };
  return h;
}

async function active(options = {}) {
  const h = await started(options);
  const reply = await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
  assert.equal(reply.result, null);
  return h;
}

function step(fields = {}) {
  return { kind: "play-step", playId: 7, tickId: 1, events: [], watermark: ORIGIN, nowNs: fields.watermark ?? ORIGIN, audioNs: 100000000n, ...fields };
}

const multiplayer = () => ({ url: "https://example.test:4433/competition", host: true, windowOriginNs: 9000000000n });
async function preparedNetwork(options = {}) {
  const h = await started({ ...options, allowNetworkClock: true,
    startRequest: options.startRequest ?? startRequest({ multiplayer: multiplayer() }) });
  while ((await h.rpc("play-sample")).result.kind !== "samples-end") {}
  for (;;) {
    const value = (await h.rpc("play-commands")).result;
    if (value === null) break;
    await h.rpc("play-ack", { sequence: value.sequence, admitted: value.commands.length, success: true });
  }
  return h;
}

async function requestNetwork(h) {
  const rpcId = ++h.rpcId;
  await h.send({ kind: "play-network-ready", playId: 7, rpcId });
  return rpcId;
}

async function activeNetwork(options = {}) {
  const h = await preparedNetwork(options);
  const rpcId = await requestNetwork(h);
  const owner = h.networks[0];
  if (options.networkRoster !== undefined) owner.emit({ kind: "roster", players: options.networkRoster });
  owner.emit({ kind: "connected" }); owner.emit({ kind: "ready" });
  owner.emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 4n });
  await flushJobs();
  const reply = h.of("play-reply").find(value => value.rpcId === rpcId);
  assert.equal(reply.result.targetHostNs, 2500000000n);
  h.networkOrigin = 2500000100n;
  const activated = await h.rpc("play-activate", { hostNs: h.networkOrigin,
    startFrame: 123456n, targetHostNs: reply.result.targetHostNs });
  assert.equal(activated.result, null);
  return h;
}

async function completeOutput(h, fields = {}) {
  const replay = h.replays.length !== 0;
  const origin = fields.presentedHostNs ?? ORIGIN;
  const report = fields.report ?? renderReport();
  if (!replay) {
    // Join a fully acquired prefix to two genuine increasing presentation pairs.
    await h.send(step({ watermark: origin + 200000000n, nowNs: origin + 200000000n }));
    await h.send({ kind: "play-render", playId: 7, renderId: 1,
      presentedNs: 0n, presentedHostNs: origin, report });
    assert.equal(h.of("play-render-done").at(-1).completed, false);
  }
  await h.send({ kind: "play-render", playId: 7, renderId: replay ? 1 : 2,
    ...fields, presentedNs: 100000000n, presentedHostNs: replay ? origin : origin + 100000000n, report });
  assert.equal(h.of("play-render-done").at(-1).completed, true);
}
async function showResults(h) {
  const reply = await h.rpc("play-results-present");
  assert.equal(reply.result.kind, "completed-results");
  await h.tick();
  return reply;
}
test("actual Worker captures before game disposal and waits for explicit Window cleanup acknowledgement", async () => {
  const h = await active({ observeOutput: () => true });
  await completeOutput(h);
  assert.equal(h.completedOwners.length, 1);
  assert.equal(h.games[0].frees, 0);
  assert.equal(h.views[0].resultDraws.length, 0);
  await h.send({ kind: "play-stop", playId: 7, completed: true });
  assert.equal(h.games[0].frees, 1);
  assert.equal(h.of("play-stopped").at(-1).completedResults.proof, true);
  assert.ok(h.trace.indexOf("capture-results") < h.trace.indexOf("stop-game"));
  assert.ok(h.trace.indexOf("capture-results") < h.trace.indexOf("free-game"));
  assert.equal(h.views[0].resultDraws.length, 0);
  await h.tick();
  assert.equal(h.views[0].resultDraws.length, 0, "Worker stop receipt alone cannot bypass Window audio/input cleanup");
  const beforeResults = h.of("render-geometry").at(-1);
  const resizeCount = h.renderPort.posts.filter(row => row.kind === "resize").length;
  await showResults(h);
  assert.equal(h.views[0].resultDraws.at(-1).results, h.completedOwners[0]);
  assert.equal(h.completedOwners[0].frees, 0);
  const resultsGeometry = h.of("render-geometry").at(-1);
  assert.equal(resultsGeometry.mode,"results");
  assert.notEqual(resultsGeometry.generation,beforeResults.generation);
  assert.ok(resultsGeometry.geometryVersion>beforeResults.geometryVersion);
  assert.deepEqual([resultsGeometry.page,resultsGeometry.width,resultsGeometry.height],[0,640,480]);
  assert.equal(h.renderPort.posts.filter(row=>row.kind==="resize").length,resizeCount);
});
test("Results page/mode refusal is atomic and consumes RPC without rendering stale requests", async () => {
  const h = await active({ observeOutput: () => true });
  await completeOutput(h); await h.send({ kind: "play-stop", playId: 7, completed: true });
  await showResults(h);
  const binding = h.completedOwners[0];
  assert.equal((await h.rpc("play-results-page", { page: 2, comparisons: true })).result.completedResults.page, 2);
  assert.equal(binding.page, 2); assert.equal(binding.comparisons, true);
  const refused = await h.rpc("play-results-page", { page: 1, comparisons: false });
  assert.ok(refused.error); assert.equal(binding.page, 2); assert.equal(binding.comparisons, true);
  const count = h.trace.filter(value => value === "page-results").length;
  await h.send({ kind: "play-results-page", playId: 7, rpcId: h.rpcId, page: 0, comparisons: false });
  await h.send({ kind: "play-results-page", playId: 6, rpcId: h.rpcId + 1, page: 0, comparisons: false });
  assert.equal(h.trace.filter(value => value === "page-results").length, count);
  assert.equal((await h.rpc("play-results-page", { page: 0, comparisons: false })).result.completedResults.page, 0);
});
test("cleanup failure preserves genuine result while selection, preview reset or new play disposes it exactly once", async () => {
  for (const action of ["select", "seek", "play-start"]) {
    const h = await active({ observeOutput: () => true, stopError: "cleanup refused" });
    await completeOutput(h); await h.send({ kind: "play-stop", playId: 7, completed: true });
    const failed = h.of("play-error").at(-1);
    assert.equal(failed.completedResults.proof, true); assert.equal(failed.released, false);
    await showResults(h);
    const binding = h.completedOwners[0];
    await h.send(action === "select" ? { kind: "select", id: 3, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" }
      : action === "seek" ? { kind: "seek", id: 3, selectedId: 2, ns: 0n } : startRequest({ playId: 8 }));
    assert.equal(binding.frees, 1);
    const draws = h.trace.filter(value => value === "draw-results").length;
    await h.send({ kind: "play-results-present", playId: 7, rpcId: 100 });
    assert.equal(binding.frees, 1);
    assert.equal(h.trace.filter(value => value === "draw-results").length, draws);
  }
});
test("cancelled live and recorded replay prefixes never acquire a completed Results owner", async () => {
  const cancelled = await active();
  await cancelled.send({ kind: "play-stop", playId: 7, completed: false });
  assert.equal(cancelled.of("play-stopped").at(-1).completedResults, null);
  assert.equal(cancelled.completedOwners.length, 0);
  const bytes = new Uint8Array([66, 75, 82, 0, 255, 1]);
  const replay = await active({ observeOutput: () => true,
    startRequest: startRequest({ mode: "replay", replayFile: new FileType([bytes], "prefix.bkr") }) });
  await completeOutput(replay);
  await replay.send({ kind: "play-stop", playId: 7, completed: true });
  assert.equal(replay.of("play-stopped").at(-1).completedResults, null);
  assert.equal(replay.completedOwners.length, 0);
});
test("premature Window presentation cannot bypass an active game or manufacture completion", async () => {
  const h = await active();
  const early = await h.rpc("play-results-present");
  assert.ok(early.error);
  assert.equal(h.completedOwners.length, 0);
  assert.equal(h.views[0].resultDraws.length, 0);
  assert.equal(h.of("play-error").at(-1).completedResults, null);
});
test("async multiplayer write and peer acknowledgement precede result release and Window presentation", async () => {
  const h = await activeNetwork({ observeOutput: () => true });
  await completeOutput(h, { presentedHostNs: h.networkOrigin, report: renderReport({ start: 123456n, cursor: 124456n }) });
  await h.send({ kind: "play-stop", playId: 7, completed: true });
  const network = h.networks[0];
  assert.equal(h.games[0].frees, 1); assert.equal(h.completedOwners.length, 1);
  assert.equal(h.of("play-stopped").length, 0); assert.equal(h.views[0].resultDraws.length, 0);
  await h.send({ kind: "play-results-present", playId: 7, rpcId: ++h.rpcId });
  assert.equal(h.views[0].resultDraws.length, 0);
  for (const submission of network.submissions) submission.gate.resolve();
  await flushJobs();
  for (const submission of network.submissions) submission.gate.resolve();
  await flushJobs();
  assert.equal(h.of("play-stopped").length, 0);
  network.emit({ kind: "final-acknowledged" }); network.ack.resolve(); await flushJobs();
  assert.equal(h.of("play-stopped").at(-1).completedResults.proof, true);
  assert.equal(h.views[0].resultDraws.length, 0);
  await showResults(h);
  assert.equal(h.views[0].resultDraws.at(-1).results, h.completedOwners[0]);
});

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function flushJobs() {
  for (let index = 0; index < 32; index++) await Promise.resolve();
}

function selectedFile(path, acquire) {
  const bytes = new TextEncoder().encode("#BPM 120");
  const file = new FileType([bytes], path.split("/").at(-1));
  file.arrayBuffer = acquire ?? (() => Promise.resolve(bytes.buffer));
  return { file, path };
}

function renderReport({ available = true, cursor = 9007199254742999n, frames = 257n, start = START } = {}) {
  const words = new Uint32Array(56);
  const put = (index, value) => {
    words[index * 2] = Number(value & 0xffffffffn);
    words[index * 2 + 1] = Number(value >> 32n);
  };
  put(0, available ? 1n : 0n);
  put(2, frames);
  put(3, cursor - 129n);
  put(4, 129n);
  put(25, 1n);
  put(26, start);
  return { available, words };
}

async function workerHarness(options = {}) {
  const trace = [];
  const completedOwners = [];
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
    const snapshot = {owner,kind,generation,content,sequence,page:kind>=4 ? owner.page : page,songNs};
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
    width:0, height:0, page:0, presentations:new Map(), posts:[], starts:0, closes:0, blocked:options.renderBlocked??false, onmessage:null,onmessageerror:null,
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
        const view=views[0];
        if(message.kind==="packet") {
          const h=new DataView(message.packet.buffer),kind=h.getUint16(6,true),sequence=h.getBigUint64(24,true);
          const s=visualOwners.get(`${message.generation}:${message.content}:${sequence}:${kind}`);
          assert.ok(s,"actual valid BKRV exporter packet crossed the boundary");
          if(kind!==6)this.page=s.page;
          if(kind===6&&options.roomResultsDrawError){this.emit({kind:"render-error",generation:message.generation,content:message.content,message:options.roomResultsDrawError});return;}
          if(kind>=4)this.presentations.set(message.generation,{...s});
          const mode=this.posts.find(p=>p.kind==="packet"&&p.generation===message.generation&&p.mode)?.mode;
          if(kind===1&&mode==="preview") {
            if(view.current&&view.current!==s.owner)view.current.releasedByView=true;
            view.current=s.owner; view.replacements?.push(s.owner);
          }else if(kind===3){view.positions.push(s.songNs);view.draws++;}
          else if(kind===2&&mode==="local")view.localDraws?.push({game:s.owner,page:s.page});
          else if(kind===2&&mode==="replay")view.replayDraws?.push(s.owner);
          else if(kind===2){view.gameDraws?.push(s.owner);view.draws++;}
          else if(kind===4){view.historicalDraws??=[];view.historicalDraws.push(s.owner);trace.push("draw-historical");}
          else if(kind===5||kind===6){view.resultDraws?.push({results:s.owner,page:s.page});trace.push("draw-results");}
        }else if(message.kind==="resize"){this.width=message.width;this.height=message.height;view.extents.push([message.width,message.height]);}
        if(["page","room-page"].includes(message.kind)){
          this.page=message.page;
          const s=this.presentations.get(message.generation);
          assert.ok(s,"paging follows real frozen registration");
          if(s.kind===4){view.historicalDraws??=[];view.historicalDraws.push(s.owner);}
          else view.resultDraws?.push({results:s.owner,page:message.page});
        }
        this.ack(message);
        if(message.geometryVersion&&this.width>0&&this.height>0)this.emit({kind:"geometry-ack",generation:message.generation,content:message.content,geometryVersion:message.geometryVersion,page:this.page,width:this.width,height:this.height});
      });
    },
  };
  function renderRequest(request) {
    if(request?.kind==="init")return {...request,canvas:undefined,renderPort:Object.hasOwn(request,"renderPort")?request.renderPort:renderPort,maxPacketBytes:request.maxPacketBytes??1024*1024,maxDiagnosticBytes:request.maxDiagnosticBytes??4096,renderTimeoutMs:request.renderTimeoutMs??60000};
    return request;
  }

  const transfers = [];
  const libraries = [];
  const views = [];
  const games = [];
  const replays = [];
  const sectionConstructions = [];
  const physicalConstructions = [];
  const contactConstructions = [];
  const localConstructions = [];
  const locals = [];
  const preparedOwners = [];
  const timers = new Map();
  const timerDelays = new Map();
  const networks = [];
  const networkSessions = [];
  const roomSessions = [];
  const roomChannels = [];
  const roomWrappers = [];
  const roomResults = [];
  let networkNow = 1000;
  let windowOrigin = 10000000000n;
  let timerId = 0;
  let receive;
  function makePrepared(path) {
    const prepared = {
      path, title: "Actual prepared metadata", artist: "Fixture", duration_ns: 604800000000000n,
      start_ns: 0n,
      note_count: 23, sample_count: 2, image_count: 1,
      lanes: new Uint8Array(options.lanes ?? [0x11, 0x12]), moved: false, frees: 0,
      free() { assert.equal(this.moved, false); assert.equal(++this.frees, 1); },
    };
    installVisualProducer(prepared);
    preparedOwners.push(prepared);
    return prepared;
  }
  class BrowserLibrary {
    files = [];
    preparations = [];
    replayPreparations = [];
    frees = 0;
    constructor(...limits) { this.limits = limits; libraries.push(this); }
    add_file(path) { this.files.push(path); }
    chart_paths() { return this.files.filter(path => /\.bms$/i.test(path)); }
    prepare_chart(path, ...args) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.includes(path));
      this.preparations.push({ path, args });
      return makePrepared(path);
    }
    prepare_chart_at(path, rate, channels, seed, startNs, ...limits) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.includes(path));
      this.preparations.push({ path, args: [rate, channels, seed, startNs, ...limits] });
      const prepared = makePrepared(path);
      prepared.start_ns = startNs;
      return prepared;
    }
    prepare_replay_chart(path, bytes, ...args) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.includes(path));
      assert.ok(bytes instanceof Uint8Array);
      this.replayPreparations.push({ path, bytes: bytes.slice(), args });
      if (options.prepareReplayError) throw new Error(options.prepareReplayError);
      const prepared = makePrepared(path);
      if (Object.hasOwn(options, "replayStart")) prepared.start_ns = options.replayStart;
      return prepared;
    }
    free() { assert.equal(++this.frees, 1); }
  }
  class BrowserView {
    static async create() {
      if (options.viewGate) await options.viewGate.promise;
      const view = new BrowserView();
      views.push(view);
      return view;
    }
    current = null;
    extents = [];
    positions = [];
    draws = 0;
    gameDraws = [];
    replayDraws = [];
    localDraws = [];
    resultDraws = [];
    resize(...extent) { this.extents.push(extent); }
    set_chart(prepared) {
      assert.equal(prepared.moved, false);
      prepared.moved = true;
      this.current = prepared;
    }
    seek(ns) { this.positions.push(ns); }
    draw() { this.draws++; }
    draw_game(game) { assert.equal(game.frees, 0); this.gameDraws.push(game); }
    draw_replay(replay) { assert.equal(replay.frees, 0); this.replayDraws.push(replay); }
    draw_local_game(game, page) { assert.equal(game.frees, 0); this.localDraws.push({ game, page }); }
    draw_room_results(results) {
      results.live();
      if (options.roomResultsDrawError) throw new Error(options.roomResultsDrawError);
      this.resultDraws.push({ results, page: results.page });
    }
    draw_completed_results(results) { results.live(); trace.push('draw-results'); this.resultDraws.push({ results, page: results.page }); }
    draw_completed_room_results(results, room) { this.draw_completed_results(results); room.live(); }
    needs_redraw() { return false; }
  }
  // Generated binding edge only. The portable Rust fixture owns roster/prefix
  // semantics; this records actual Worker ownership and the bounded page calls.
  class BrowserRoomResults {
    constructor(participant, words) {
      if (options.roomResultsConstructError) throw new Error(options.roomResultsConstructError);
      assert.ok(words instanceof Uint32Array);
      this.participant = participant; this.roster = words.slice();
      this.updates = []; this.selections = []; this.frees = 0; this.frozen = false;
      this.pageValue = 0; this.failedValue = false;
      let remotePlayers = 0;
      for (let offset = 0; offset < words.length;) {
        const host = BigInt(words[offset]) | (BigInt(words[offset + 1]) << 32n);
        const count = words[offset + 2];
        if (host !== participant) remotePlayers += count;
        offset += 3 + count;
      }
      this.pagesValue = Math.ceil(remotePlayers / 4);
      options.onRoomResultsConstruct?.(this);
      roomResults.push(this);
    }
    live() { assert.equal(this.frees, 0, "retained Results binding was already freed"); }
    update(participant, sequence, finalPrefix, words) {
      this.live(); assert.equal(this.frozen, false);
      if (options.roomResultsUpdateError) throw new Error(options.roomResultsUpdateError);
      this.updates.push({ participant, sequence, finalPrefix, words: words.slice() });
    }
    freeze(page, cancelled, error, failed) {
      this.live(); assert.equal(this.frozen, false);
      if (options.roomResultsFreezeError) throw new Error(options.roomResultsFreezeError);
      this.freezeArgs = { page, cancelled, error, failed };
      this.pageValue = page; this.failedValue = failed; this.frozen = true;
    }
    set_page(page) {
      this.live(); assert.equal(this.frozen, true);
      if (this.pageError || options.roomResultsPageError) throw new Error(this.pageError ?? options.roomResultsPageError);
      assert.ok(Number.isInteger(page) && page >= 0 && page < this.pagesValue);
      this.selections.push(page); this.pageValue = page;
    }
    get page() { this.live(); return this.pageValue; }
    get pages() { this.live(); return this.pagesValue; }
    get failed() { this.live(); return this.failedValue; }
    free() { this.live(); assert.equal(++this.frees, 1); }
  }
  class BrowserCompletedResults {
    constructor(players = [1]) { this.playersValue = players; this.pageValue = 0; this.mode = false; this.frees = 0; completedOwners.push(this); }
    live() { assert.equal(this.frees, 0); }
    get players() { this.live(); return new Uint32Array(this.playersValue); }
    get page() { this.live(); return this.pageValue; }
    get pages() { this.live(); return this.mode ? 3 : 1; }
    get detail_pages() { this.live(); return 1; }
    get comparison_pages() { this.live(); return 3; }
    get comparisons() { this.live(); return this.mode; }
    get has_comparisons() { this.live(); return true; }
    get failed() { this.live(); return false; }
    set_presentation(page, comparisons) {
      this.live();
      if (!Number.isInteger(page) || page < 0 || page >= (comparisons ? 3 : 1)) throw new Error("Result page outside retained packets");
      this.pageValue = page; this.mode = comparisons; trace.push("page-results");
    }
    free() { this.live(); assert.equal(++this.frees, 1); trace.push("free-results"); }
  }
  class BrowserGame {
    completed_archive() { return null; }
    static new_physical_contact(prepared, ...args) {
      contactConstructions.push({ prepared, args });
      if (options.contactConstructError) { prepared.moved = true; throw new Error(options.contactConstructError); }
      const owner = new BrowserGame(prepared, ...args.slice(0, 6));
      owner.physical = true; owner.contact = true;
      return owner;
    }
    static new_physical(prepared, ...args) {
      physicalConstructions.push({ prepared, args });
      if (options.physicalConstructError) {
        assert.equal(prepared.moved, false);
        prepared.moved = true;
        throw new Error(options.physicalConstructError);
      }
      const owner = new BrowserGame(prepared, ...args.slice(0, 6));
      owner.physical = true;
      return owner;
    }
    static new_section(prepared, ...args) {
      sectionConstructions.push({ prepared, args });
      if (options.sectionConstructError) {
        assert.equal(prepared.moved, false);
        prepared.moved = true;
        throw new Error(options.sectionConstructError);
      }
      const owner = new BrowserGame(prepared, ...args.slice(0, -1));
      owner.constructedEnd = args.at(-1);
      return owner;
    }
    constructor(prepared, ...args) {
      assert.equal(prepared.moved, false);
      prepared.moved = true; // The generated consuming constructor owns even its Err argument.
      if (options.constructError) throw new Error(options.constructError);
      this.prepared = prepared;
      this.args = args;
      this.score = { ...SCORE };
      this.calls = [];
      this.pendingInput = []; this.processedInput = []; this.inputOrdinal = 0;
      this.closedPrefix = null; this.presentations = [];
      this.endpointReads = { end: 0, frame: 0 };
      this.frees = 0;
      this.stops = 0;
      this.disposals = [];
      this.replayTakes = 0;
      this.replayBytes = null;
      this.saved = [];
      this.savedReads = 0;
      this.hudDisables = 0;
      this.peerUpdates = [];
      this.peerDisables = 0;
      this.samples = (options.samples ?? [
        { id: 19n, rate: 44100, pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]) },
        { id: 18446744073709551615n, rate: 96000, pcm: new Float32Array([1, -1]) },
      ]).map(value => ({
        ...value, channels: 2, takes: 0, frees: 0,
        take_pcm() {
          assert.equal(++this.takes, 1);
          if (options.takeError) throw new Error(options.takeError);
          return this.pcm;
        },
        free() { assert.equal(++this.frees, 1); if (options.sampleFreeError) throw new Error(options.sampleFreeError); },
      }));
      this.sampleIndex = 0;
      this.batches = [...(options.batches ?? [])];
      games.push(this);
    }
    live() { assert.equal(this.frees, 0, "binding must not be read after free"); }
    get end_ns() {
      this.live(); this.endpointReads.end++;
      return options.gameEndGetter ? options.gameEndGetter(this) : options.gameEnd;
    }
    get playback_end_frame() {
      this.live(); this.endpointReads.frame++;
      return options.gameFrameGetter ? options.gameFrameGetter(this) : options.gameEndFrame;
    }
    get song_ns() { this.live(); return this.score.song_ns; }
    get hits() { this.live(); return this.score.hits; }
    get misses() { this.live(); return this.score.misses; }
    get combo() { this.live(); return this.score.combo; }
    get max_combo() { this.live(); return this.score.max_combo; }
    get failed() { this.live(); return false; }
    get touch_bounds() {
      this.live();
      if (options.touchBoundsError) throw new Error(options.touchBoundsError);
      return Object.hasOwn(options, "touchBounds") ? options.touchBounds : new Float32Array([80, 110, 400, 634, 400, 110, 720, 634]);
    }
    get touch_width() { this.live(); return options.touchWidth ?? 960; }
    get touch_height() { this.live(); return options.touchHeight ?? 720; }
    configure_touch_regions(words, bounds, maximum) {
      this.live(); assert.equal(this.contact, true);
      this.calls.push(["touch-setup", words.slice(), bounds.slice(), maximum]);
      if (options.touchSetupError) throw new Error(options.touchSetupError);
    }
    configure_hid_devices(devices, fields, parameters) {
      this.live(); assert.equal(this.physical, true);
      this.calls.push(["hid-setup", devices.slice(), fields.slice(), parameters.slice()]);
      if (options.hidSetupError) throw new Error(options.hidSetupError);
    }
    competition_identity() {
      this.live(); this.calls.push(["identity"]);
      if (options.identityError) throw new Error(options.identityError);
      return options.identityBytes ?? Uint8Array.from([66, 75, 82, 0, 255]);
    }
    add_saved_opponent(bytes, own, label) {
      this.live(); this.calls.push(["add-opponent", bytes.slice(), own, label]);
      this.saved.push({ own, label }); return this.saved.length - 1;
    }
    saved_opponents() {
      this.live(); assert.equal(this.stops, 0, "final comparisons must be captured before disposal");
      this.savedReads++; this.calls.push(["saved-opponents"]); this.disposals.push("opponents");
      if (options.savedError) throw new Error(options.savedError);
      return options.savedSnapshot?.(this) ?? this.saved.map(value => ({ kind: value.own ? "own" : "other", label: value.label,
        songNs: this.score.song_ns, recordedUntilNs: -1n, hits: 1n, misses: 0n, combo: 1n, maxCombo: 1n }));
    }
    disable_saved_opponent_hud() {
      this.live(); this.hudDisables++; this.calls.push(["disable-opponent-hud"]);
      if (options.disableSavedError) throw new Error(options.disableSavedError);
    }
    update_peer_hud(status, words) {
      this.live(); assert.equal(this.stops, 0, "disposed gameplay cannot receive HUD writes");
      assert.ok(words instanceof Uint32Array);
      this.peerUpdates.push({ status, words: words.slice() });
      options.peerUpdate?.(this, status, words);
    }
    disable_peer_hud() {
      this.live(); this.peerDisables++;
      if (options.disablePeerError) throw new Error(options.disablePeerError);
    }
    sample_count() { this.live(); return options.sampleCount ?? this.samples.length; }
    configure_capture(...limits) {
      this.live();
      this.calls.push(["capture", ...limits]);
      if (options.captureError) throw new Error(options.captureError);
    }
    take_replay() {
      this.live();
      assert.equal(this.stops, 1, "capture export requires a stopped owner");
      assert.equal(++this.replayTakes, 1);
      this.disposals.push("take");
      if (options.replayError) throw new Error(options.replayError);
      // Opaque binding output: actual codec/parity is covered by the Rust fixtures.
      this.replayBytes = options.replayBytes ? options.replayBytes() : Uint8Array.from([66, 75, 82, 255, 0, 1]);
      return this.replayBytes;
    }
    next_sample() { this.live(); this.calls.push(["sample"]); return this.samples[this.sampleIndex++] ?? null; }
    activate(host) { this.live(); this.activationHost = host; this.calls.push(["activate", host]); }
    queue_input(host, key, down, sequence, received) {
      this.live();
      assert.ok(host <= received, "acquisition cannot follow current Window receipt");
      this.calls.push(["input", host, key, down, sequence, received]);
      this.pendingInput.push({ host, key, down, sequence, received, ordinal: this.inputOrdinal++ });
    }
    queue_input_blob(bytes, received) {
      this.live(); assert.equal(this.physical, true);
      this.calls.push(["blob", bytes.slice(), received]);
      options.inputBlob?.(this, bytes, received);
      this.retainPacket(bytes, received);
    }
    queue_hid_blob(bytes, received) {
      this.live(); assert.equal(this.physical, true);
      this.calls.push(["hid", bytes.slice(), received]);
      options.inputHidBlob?.(this, bytes, received);
      this.retainPacket(bytes, received);
    }
    queue_input_blob_on_surface(bytes, cssWidth, cssHeight, surfaceWidth, surfaceHeight, received) {
      this.live(); assert.equal(this.contact, true);
      this.calls.push(["touch", bytes.slice(), cssWidth, cssHeight, surfaceWidth, surfaceHeight, received]);
      options.inputBlobOnSurface?.(this, bytes, cssWidth, cssHeight, surfaceWidth, surfaceHeight, received);
      this.retainPacket(bytes, received, { cssWidth, cssHeight, surfaceWidth, surfaceHeight });
    }
    retainPacket(bytes, received, geometry = null) {
      const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      const host = view.getBigInt64(15, true);
      assert.ok(host <= received);
      this.pendingInput.push({ bytes: bytes.slice(), host, sequence: view.getBigUint64(27, true),
        source: view.getBigUint64(7, true), received, geometry, ordinal: this.inputOrdinal++ });
    }
    close_input_prefix(host) {
      this.live(); this.calls.push(["close", host]);
      assert.ok(this.closedPrefix === null || host >= this.closedPrefix);
      this.closedPrefix = host;
    }
    pending_inputs() { this.live(); return this.pendingInput.length; }
    service_audio(now, audio) {
      this.live(); this.calls.push(["service", now, audio]);
      this.lastService = { now, audio };
      let processed = 0;
      if (this.presentations.length >= 2 && this.closedPrefix !== null) {
        const first = this.presentations.at(-2), last = this.presentations.at(-1);
        if (now >= last.host && now - last.host <= 1000000000n) {
          this.pendingInput.sort((a, b) => a.host < b.host ? -1 : a.host > b.host ? 1 :
            (a.source ?? 0n) < (b.source ?? 0n) ? -1 : (a.source ?? 0n) > (b.source ?? 0n) ? 1 :
            a.sequence < b.sequence ? -1 : a.sequence > b.sequence ? 1 : a.ordinal - b.ordinal);
          while (this.pendingInput[0]?.host <= this.closedPrefix && this.pendingInput[0].host <= last.host) {
            const event = this.pendingInput.shift();
            event.output = first.output + (event.host - first.host) * (last.output - first.output) / (last.host - first.host);
            event.audio = audio;
            this.processedInput.push(event);
            this.calls.push(["processed", event.host, event.output, audio]);
            if (event.key !== undefined) options.input?.(this, [event.host, event.key, event.down, event.sequence, audio]);
            processed++;
          }
          if (!this.pendingInput.length && last.host <= this.closedPrefix) {
            this.logicalOutput = last.output;
          }
        }
      }
      options.service?.(this, now, audio);
      return processed;
    }
    admit_output(words, presentedNs) {
      this.live(); this.calls.push(["output", words.slice(), presentedNs]);
      this.outputEvidence = { words: words.slice(), presentedNs };
      options.admitOutput?.(this, words, presentedNs);
    }
    evaluate_completion() {
      this.live(); this.calls.push(["completion"]);
      assert.ok(this.lastService, "live completion must follow joined audio service");
      if (this.pendingInput.length || !this.outputEvidence || this.outputEvidence.presentedNs === null) return false;
      this.completedEvidence = this.presentations.length >= 2 && this.closedPrefix !== null
        && this.presentations.at(-1).host <= this.closedPrefix
        && (options.observeOutput?.(this, this.outputEvidence.words, this.outputEvidence.presentedNs) ?? false);
      return this.completedEvidence;
    }
    input(...args) {
      this.live();
      assert.notEqual(this.physical, true, "physical owners must not fall back to the legacy key method");
      this.calls.push(["input", ...args]);
      options.input?.(this, args);
    }
    input_blob(bytes, audioNs) {
      this.live();
      assert.equal(this.physical, true);
      assert.ok(bytes instanceof Uint8Array);
      this.calls.push(["blob", bytes.slice(), audioNs]);
      options.inputBlob?.(this, bytes, audioNs);
    }
    input_blob_at(bytes, x, y, audioNs) {
      assert.fail("Worker contact routing must use the shared surface projection binding");
    }
    preflight_touch_surface(x, y, cssWidth, cssHeight, surfaceWidth, surfaceHeight) {
      this.live(); assert.equal(this.contact, true);
      (this.touchPreflights ??= []).push([x, y, cssWidth, cssHeight, surfaceWidth, surfaceHeight]);
      options.preflightTouchSurface?.(this, x, y, cssWidth, cssHeight, surfaceWidth, surfaceHeight);
    }
    input_blob_on_surface(bytes, cssWidth, cssHeight, surfaceWidth, surfaceHeight, audioNs) {
      this.live(); assert.equal(this.contact, true);
      this.calls.push(["touch", bytes.slice(), cssWidth, cssHeight, surfaceWidth, surfaceHeight, audioNs]);
      options.inputBlobOnSurface?.(this, bytes, cssWidth, cssHeight, surfaceWidth, surfaceHeight, audioNs);
    }
    input_hid_blob(bytes, audioNs) {
      this.live(); assert.equal(this.physical, true);
      assert.ok(bytes instanceof Uint8Array);
      this.calls.push(["hid", bytes.slice(), audioNs]);
      options.inputHidBlob?.(this, bytes, audioNs);
    }
    advance(...args) { this.live(); this.calls.push(["advance", ...args]); options.advance?.(this, args); }
    completed_results() { this.live(); trace.push('capture-results'); assert.equal(this.stops, 0); return this.completedEvidence ? new BrowserCompletedResults() : null; }
    observe_output(words, presentedNs) {
      this.live();
      this.calls.push(["output", words.slice(), presentedNs]);
      this.completedEvidence = options.observeOutput?.(this, words, presentedNs) ?? false;
      return this.completedEvidence;
    }
    observe_presentation(outputNs, hostNs) {
      this.live();
      this.calls.push(["presentation", outputNs, hostNs]);
      options.observePresentation?.(this, outputNs, hostNs);
      const previous = this.presentations.at(-1);
      assert.ok(!previous || (hostNs >= previous.host && outputNs >= previous.output));
      if (!previous || (hostNs > previous.host && outputNs > previous.output)) this.presentations.push({ output: outputNs, host: hostNs });
    }
    commands(max) { this.live(); this.calls.push(["commands", max]); return this.batches.shift() ?? null; }
    acknowledge(...args) { this.live(); this.calls.push(["ack", ...args]); options.ack?.(this, args); }
    stop() {
      this.live();
      assert.equal(++this.stops, 1);
      this.disposals.push("stop"); trace.push("stop-game");
      if (options.stopError) throw new Error(options.stopError);
    }
    free() {
      options.beforeFree?.(this);
      assert.equal(this.stops, 1);
      assert.equal(++this.frees, 1);
      this.disposals.push("free"); trace.push("free-game");
      if (options.freeError) throw new Error(options.freeError);
    }
  }
  if (options.missingSectionConstructor) BrowserGame.new_section = undefined;
  if (options.missingDisableSavedHud) BrowserGame.prototype.disable_saved_opponent_hud = undefined;
  if (options.missingPeerUpdate) BrowserGame.prototype.update_peer_hud = undefined;
  if (options.missingPeerDisable) BrowserGame.prototype.disable_peer_hud = undefined;
  if (options.missingPhysicalConstructor) BrowserGame.new_physical = undefined;
  if (options.missingInputBlob) BrowserGame.prototype.queue_input_blob = undefined;
  if (options.missingContactConstructor) BrowserGame.new_physical_contact = undefined;
  if (options.missingTouchSetup) BrowserGame.prototype.configure_touch_regions = undefined;
  if (options.missingInputBlobOnSurface) BrowserGame.prototype.queue_input_blob_on_surface = undefined;
  if (options.missingPreflightTouchSurface) BrowserGame.prototype.preflight_touch_surface = undefined;
  if (options.missingHidSetup) BrowserGame.prototype.configure_hid_devices = undefined;
  if (options.missingInputHidBlob) BrowserGame.prototype.queue_hid_blob = undefined;
  class BrowserLocalGame extends BrowserGame {

    queue_input_blob_on_surface_on_page(bytes, cssWidth, cssHeight, surfaceWidth, surfaceHeight, page, received) {
      this.live(); assert.equal(this.contact, true);
      assert.ok(Number.isInteger(page) && page >= 0 && page < Math.ceil(this.memberIds.length / 4));
      this.calls.push(["touch", bytes.slice(), cssWidth, cssHeight, surfaceWidth, surfaceHeight, received]);
      this.calls.push(["touch-page", page, bytes.slice(), cssWidth, cssHeight, surfaceWidth, surfaceHeight, received]);
      options.inputBlobOnSurface?.(this, bytes, cssWidth, cssHeight, surfaceWidth, surfaceHeight, received);
      this.retainPacket(bytes, received, { cssWidth, cssHeight, surfaceWidth, surfaceHeight });
      this.pendingInput.at(-1).acquisitionPage = page;
    }

    static new_physical(prepared, ...args) {
      localConstructions.push({ prepared, args });
      if (options.localConstructError) { prepared.moved = true; throw new Error(options.localConstructError); }
      return new BrowserLocalGame(prepared, ...args);
    }
    constructor(prepared, ...args) {
      super(prepared, ...args);
      assert.equal(games.pop(), this); locals.push(this);
      this.physical = true; this.contact = args[8];
      this.memberIds = Array.from(args[5]).filter((_, index) => index % 4 === 0);
      this.memberScores = new Map(this.memberIds.map((player, index) => [player,
        options.localScore?.(player, index) ?? { ...SCORE, hits: SCORE.hits + BigInt(index), max_combo: SCORE.max_combo + BigInt(index) }]));
      this.memberCaptures = new Set(); this.memberReplayTakes = new Map(); this.memberReplayBytes = new Map();
      this.memberSaved = new Map(this.memberIds.map(player => [player, []]));
      this.memberHudDisables = new Map();
      this.memberPeerConfigurations = [];
      this.memberPeerUpdates = [];
      this.memberPeerDisables = new Map();
      this.groupProgressReads = 0;
      this.roomHudCalls = [];
    }
    get players() { this.live(); return options.localPlayers ?? new Uint32Array(this.memberIds); }
    memberValue(player, field) {
      this.live(); assert.ok(this.memberScores.has(player));
      this.calls.push(["member-score", player, field]);
      return options.localValue ? options.localValue(this, player, field) : this.memberScores.get(player)[field];
    }
    hits(player) { return this.memberValue(player, "hits"); }
    misses(player) { return this.memberValue(player, "misses"); }
    combo(player) { return this.memberValue(player, "combo"); }
    max_combo(player) { return this.memberValue(player, "max_combo"); }
    member_song_ns(player) { return this.memberValue(player, "song_ns"); }
    competition_identity(player) {
      this.live(); assert.ok(this.memberIds.includes(player));
      this.calls.push(["member-identity", player]);
      if (options.localIdentityError?.(player)) throw new Error("member canonical identity refused");
      return options.localIdentity?.(player) ?? Uint8Array.from([66, 75, 82, 0, 255]);
    }
    progress_words() {
      this.live(); assert.equal(this.stops, 0, "final group prefix must precede disposal");
      this.groupProgressReads++; this.calls.push(["group-progress"]); this.disposals.push("group-progress");
      return options.localProgressWords?.(this)
        ?? new Uint32Array(this.memberIds.flatMap(player => [player, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0]));
    }
    configure_peer_hud(player) {
      this.live(); assert.ok(this.memberIds.includes(player));
      this.calls.push(["local-peer-configure", player]); this.memberPeerConfigurations.push(player);
      if (options.localPeerConfigureError?.(player)) throw new Error("member peer reservation refused");
    }
    configure_room_hud(participant, words) {
      this.live(); assert.equal(this.stops, 0); assert.ok(words instanceof Uint32Array);
      const call = ["configure", participant, words.slice()];
      this.roomHudCalls.push(call); this.calls.push(["room-hud", ...call]);
      if (options.roomHudConfigureError) throw new Error(options.roomHudConfigureError);
    }
    update_room_hud(participant, sequence, finalPrefix, words) {
      this.live(); assert.equal(this.stops, 0); assert.ok(words instanceof Uint32Array);
      const call = ["update", participant, sequence, finalPrefix, words.slice()];
      this.roomHudCalls.push(call); this.calls.push(["room-hud", ...call]);
      if (options.roomHudUpdateError) throw new Error(options.roomHudUpdateError);
    }
    set_room_hud_status(status) {
      this.live(); assert.equal(this.stops, 0);
      this.roomHudCalls.push(["status", status]); this.calls.push(["room-hud", "status", status]);
      if (options.roomHudStatusError) throw new Error(options.roomHudStatusError);
    }
    set_room_hud_page(page) {
      this.live(); assert.equal(this.stops, 0);
      this.roomHudCalls.push(["page", page]); this.calls.push(["room-hud", "page", page]);
      if (options.roomHudPageError) throw new Error(options.roomHudPageError);
    }
    room_hud_pages() { this.live(); return options.roomHudPages ?? 1; }
    disable_room_hud() {
      this.live(); assert.equal(this.stops, 0);
      this.roomHudCalls.push(["disable"]); this.calls.push(["room-hud", "disable"]);
      if (options.roomHudDisableError) throw new Error(options.roomHudDisableError);
    }
    update_peer_hud(player, status, words) {
      this.live(); assert.equal(this.stops, 0); assert.ok(this.memberIds.includes(player));
      assert.ok(words instanceof Uint32Array);
      this.calls.push(["local-peer-update", player, status, words.slice()]);
      this.memberPeerUpdates.push({ player, status, words: words.slice() });
      options.localPeerUpdate?.(this, player, status, words);
    }
    disable_peer_hud(player) {
      this.live(); assert.ok(this.memberIds.includes(player));
      this.calls.push(["local-peer-disable", player]);
      this.memberPeerDisables.set(player, (this.memberPeerDisables.get(player) ?? 0) + 1);
      if (options.localDisablePeerError?.(player)) throw new Error("member peer hide refused");
    }
    add_saved_opponent(player, bytes, own, label) {
      this.live(); assert.ok(this.memberSaved.has(player));
      this.calls.push(["local-add-opponent", player, bytes.slice(), own, label]);
      if (options.localAddSavedError?.(player, label)) throw new Error("actual member replay admission refused");
      const rows = this.memberSaved.get(player); rows.push({ own, label });
      return options.localSavedIndex?.(player, rows.length - 1) ?? rows.length - 1;
    }
    saved_opponents() {
      this.live(); assert.equal(this.stops, 0, "member prefixes must be read before disposal");
      this.savedReads++; this.calls.push(["local-saved-opponents"]); this.disposals.push("opponents");
      const groups = this.memberIds.map(player => ({ player, error: null,
        opponents: this.memberSaved.get(player).map(value => ({ kind: value.own ? "own" : "other", label: value.label,
          songNs: this.memberScores.get(player).song_ns, recordedUntilNs: -1n,
          hits: 1n, misses: 0n, combo: 1n, maxCombo: 1n })) }));
      return options.localSavedSnapshot?.(this, groups) ?? groups;
    }
    disable_saved_opponent_hud(player) {
      this.live(); assert.ok(this.memberSaved.has(player));
      this.calls.push(["local-disable-opponent-hud", player]);
      this.memberHudDisables.set(player, (this.memberHudDisables.get(player) ?? 0) + 1);
      if (options.localDisableSavedError?.(player)) throw new Error("member HUD disable failed");
    }
    touch_bounds(player, page) {
      this.live(); this.calls.push(["local-touch-bounds", player, page]);
      return options.localTouchBounds ?? new Float32Array([34, 176, 249, 356, 249, 176, 464, 356]);
    }
    configure_touch_regions(player, words, bounds, maximum) {
      this.live(); assert.equal(this.contact, true); assert.ok(this.memberIds.includes(player));
      this.calls.push(["local-touch", player, words.slice(), bounds.slice(), maximum]);
      if (options.localTouchSetupError) throw new Error(options.localTouchSetupError);
    }
    set_touch_page(player, page) {
      this.live(); assert.ok(this.memberIds.includes(player));
      this.calls.push(["local-touch-page", player, page]);
      if (options.localTouchPageError?.(player, page)) throw new Error("actual page remap refused");
      return options.localTouchVisibility?.(player, page) ?? Math.floor(this.memberIds.indexOf(player) / 4) === page;
    }
    configure_capture(player, ...limits) {
      this.live(); assert.ok(this.memberIds.includes(player));
      this.calls.push(["member-capture", player, ...limits]);
      if (options.localCaptureError?.(player)) throw new Error("selected member capture setup refused");
      this.memberCaptures.add(player);
    }
    take_replay(player) {
      this.live(); assert.equal(this.stops, 1);
      assert.equal(this.memberReplayTakes.has(player), false); this.memberReplayTakes.set(player, 1);
      this.disposals.push(`take:${player}`);
      if (!this.memberCaptures.has(player)) return null;
      const bytes = options.localReplayBytes ? options.localReplayBytes(player) : Uint8Array.from([66, 75, 82, player & 255]);
      this.memberReplayBytes.set(player, bytes); return bytes;
    }
  }
  if (options.missingLocalTouchSurfacePage) BrowserLocalGame.prototype.queue_input_blob_on_surface_on_page = undefined;
  if (options.missingLocalConstructor) BrowserLocalGame.new_physical = undefined;
  if (options.missingLocalInputBlob) BrowserLocalGame.prototype.queue_input_blob = undefined;
  if (options.missingLocalSavedHud) BrowserLocalGame.prototype.disable_saved_opponent_hud = undefined;
  if (options.missingLocalTouchPage) BrowserLocalGame.prototype.set_touch_page = undefined;
  if (options.missingLocalIdentity) BrowserLocalGame.prototype.competition_identity = undefined;
  if (options.missingLocalProgress) BrowserLocalGame.prototype.progress_words = undefined;
  if (options.missingLocalPeerConfigure) BrowserLocalGame.prototype.configure_peer_hud = undefined;
  if (options.missingLocalPeerUpdate) BrowserLocalGame.prototype.update_peer_hud = undefined;
  if (options.missingLocalPeerDisable) BrowserLocalGame.prototype.disable_peer_hud = undefined;
  if (options.missingRoomHudMethod) BrowserLocalGame.prototype[options.missingRoomHudMethod] = undefined;
  class BrowserReplay extends BrowserGame {
    constructor(prepared, ...args) {
      super(prepared, ...args);
      assert.equal(games.pop(), this);
      this.endpointReads = { end: 0, frame: 0 };
      replays.push(this);
    }
    get end_ns() {
      this.live(); this.endpointReads.end++;
      return options.replayEndGetter ? options.replayEndGetter(this) : options.replayEnd;
    }
    get playback_end_frame() {
      this.live(); this.endpointReads.frame++;
      return options.replayFrameGetter ? options.replayFrameGetter(this) : options.replayEndFrame;
    }
    get recorded_until_ns() { this.live(); return options.recordedUntil === undefined ? SCORE.song_ns : options.recordedUntil; }
    activate() { assert.fail("replay must not activate a live transport"); }
    input() { assert.fail("replay must not accept live input"); }
    advance() { assert.fail("replay must not synthesize live advances"); }
    observe_presentation() { assert.fail("replay must not discipline a live input clock"); }
    configure_capture() { assert.fail("replay must not recapture a recording"); }
    take_replay() { assert.fail("replay playback must not re-export its input bytes"); }
    competition_identity() { assert.fail("replay playback must stay local"); }
  }
  class BrowserMultiplayer {
    static new_group(identity, players, host, preroll) {
      assert.ok(players instanceof Uint32Array);
      const session = new BrowserMultiplayer(identity, host, preroll);
      session.group = true; session.players = players.slice();
      return session;
    }
    constructor(identity, host, preroll) {
      this.identity = [...identity]; this.host = host; this.preroll = preroll;
      this.closes = 0; this.frees = 0; networkSessions.push(this);
      if (options.networkConstructError) throw new Error(options.networkConstructError);
    }
    close() { assert.equal(++this.closes, 1); }
    free() { assert.equal(++this.frees, 1); }
  }
  if (options.missingNetworkGroupConstructor) BrowserMultiplayer.new_group = undefined;
  class BrowserRoomClient {
    static new_with_start(identity, players, preroll) {
      const session = new BrowserRoomClient(identity, players);
      session.preroll = preroll;
      return session;
    }
    constructor(identity, players) {
      this.identity = identity.slice(); this.players = players.slice();
      this.closes = 0; this.frees = 0; this.revisionValue = 0n; this.participantValue = 0n;
      this.dto = null; this.left = false; this.partial = false; this.need = 11;
      this.credits = []; this.received = []; this.requests = [];
      this.receiveTimes = []; this.writeTimes = []; this.pollTimes = []; this.schedules = [];
      this.publications = []; this.peerProgress = [];
      this.finalWritten = false; this.finalAcknowledged = false; this.progressComplete = false; this.drainComplete = false;
      this.frames = [{ kind: 1, id: 1n, bytes: new Uint8Array(11) }];
      roomSessions.push(this);
      if (options.roomConstructError) throw new Error(options.roomConstructError);
    }
    live() { assert.equal(this.frees, 0, "room binding called after free"); }
    request(kind) {
      this.live(); this.requests.push(kind);
      if (this.requestError) throw this.requestError;
      this.onRequest?.(kind);
    }
    request_seal() { this.request("seal"); }
    request_ready() { this.request("ready"); }
    request_leave() { this.request("leave"); }
    request_drain() { this.request("drain"); }
    needed_bytes() { this.live(); return this.need; }
    frame_pending() { this.live(); return this.partial; }
    revision() { this.live(); return this.revisionValue; }
    participant_id() { this.live(); return this.participantValue; }
    has_snapshot() { this.live(); return this.dto !== null; }
    leave_written() { this.live(); return this.left; }
    snapshot() { this.live(); return this.dto; }
    receive_bytes(bytes, captured, processing) {
      this.live(); this.received.push([...bytes]);
      this.receiveTimes.push({ captured, processing });
      if (options.roomReceiveError) throw new Error(options.roomReceiveError);
      this.onReceive?.(bytes);
      return bytes.length;
    }
    next_write(processing) {
      this.live();
      this.pollTimes.push(processing);
      const frame = this.frames.shift() ?? { kind: 0, id: 0n };
      let freed = false;
      const wrapper = {
        frees: 0, takes: 0,
        get kind() { assert.equal(freed, false); return frame.kind; },
        get frame_id() { assert.equal(freed, false); return frame.id; },
        take_bytes() { assert.equal(freed, false); assert.equal(this.takes++, 0); return frame.bytes; },
        free() { assert.equal(freed, false); freed = true; this.frees++; },
      };
      roomWrappers.push(wrapper); return wrapper;
    }
    written(id, completed, processing) {
      this.live(); this.credits.push(id); this.writeTimes.push({ id, completed, processing }); this.onWritten?.(id);
    }
    take_start() { this.live(); return this.schedules.shift() ?? null; }
    publish_progress(words, finalPrefix) {
      this.live();
      assert.ok(words instanceof Uint32Array);
      assert.equal(words.length, this.players.length * 11);
      assert.equal(typeof finalPrefix, "boolean");
      assert.deepEqual(Array.from(words).filter((_, index) => index % 11 === 0), [...this.players]);
      if (this.publicationError) throw this.publicationError;
      this.publications.push({ words: words.slice(), finalPrefix });
      this.onPublish?.(words, finalPrefix);
    }
    take_peer_progress() { this.live(); return this.peerProgress.shift() ?? null; }
    local_final_written() { this.live(); return this.finalWritten; }
    local_final_acknowledged() { this.live(); return this.finalAcknowledged; }
    peer_final_ack_written() { this.live(); return false; }
    progress_complete() { this.live(); return this.progressComplete; }
    drain_complete() { this.live(); return this.drainComplete; }
    close() { this.live(); assert.equal(++this.closes, 1); }
    free() { this.live(); assert.equal(++this.frees, 1); }
  }
  if (options.missingRoomMethod) BrowserRoomClient.prototype[options.missingRoomMethod] = undefined;
  if (options.missingRoomStartConstructor) BrowserRoomClient.new_with_start = undefined;
  // The actual room owner is loaded below. Only its byte channel is controlled.
  class RoomChannel {
    static async open(url, config) {
      const channel = new RoomChannel(url, config); roomChannels.push(channel);
      if (options.roomOpenGate) await options.roomOpenGate.promise;
      if (options.roomOpenError) throw new Error(options.roomOpenError);
      return channel;
    }
    constructor(url, config) {
      this.url = url; this.config = config; this.reads = []; this.writes = [];
      this.closes = 0; this.closed = false; this.activeReads = 0; this.activeWrites = 0;
    }
    readPrefix(max, waitForData) {
      assert.equal(this.closed, false); assert.equal(this.activeReads++, 0);
      const gate = deferred(); this.reads.push({ max, waitForData, gate });
      return gate.promise.finally(() => { this.activeReads--; });
    }
    write(bytes) {
      assert.equal(this.closed, false); assert.equal(this.activeWrites++, 0);
      assert.ok(roomWrappers.every(value => value.frees === 1));
      const gate = deferred(); this.writes.push({ bytes: [...bytes], gate });
      return gate.promise.finally(() => { this.activeWrites--; });
    }
    close() {
      this.closed = true; this.closes++;
      if (!options.roomHoldAfterClose) {
        for (const entry of [...this.reads, ...this.writes]) entry.gate.reject(new Error("room channel closed"));
      }
      if (options.roomCloseError) throw new Error(options.roomCloseError);
    }
  }
  class BrowserMultiplayerOwner {
    static async open(url, config) {
      const owner = {
        url, config, origin: config.now(), closed: false, closes: 0, readyCalls: 0,
        submissions: [], ack: deferred(), ackCalls: 0,
        request_ready() { assert.equal(this.closed, false); this.readyCalls++; },
        submit(value, final) {
          assert.equal(this.closed, false);
          assert.notEqual(config.group, true, "local group sessions cannot use scalar submissions");
          const gate = deferred();
          this.submissions.push({ value: structuredClone(value), final, gate });
          return gate.promise;
        },
        submit_group(value, final) {
          assert.equal(this.closed, false); assert.equal(config.group, true);
          assert.ok(value instanceof Uint32Array);
          const gate = deferred();
          this.submissions.push({ value: value.slice(), final, group: true, gate });
          return gate.promise;
        },
        wait_final_ack() { this.ackCalls++; return this.ack.promise; },
        emit(event) {
          try { config.onEvent(event); }
          catch (error) { this.disconnect(error); } // Script the owner's callback-failure edge, not protocol parsing.
        },
        disconnect(error = new Error("peer disconnected")) {
          if (!this.closed) {
            this.closed = true; this.closes++;
            config.session.close(); config.session.free();
          }
          config.onClose(error);
        },
        close() {
          if (this.closed) return;
          this.disconnect(Object.assign(new Error("owner closed"), { code: "closed" }));
        },
      };
      networks.push(owner);
      try {
        if (options.networkOpenGate) await options.networkOpenGate.promise;
        if (options.networkOpenError) throw new Error(options.networkOpenError);
        return owner;
      } catch (error) { owner.disconnect(error); throw error; }
    }
  }
  views.push(new BrowserView());
  installVisualProducer(BrowserGame.prototype, 1);
  installVisualProducer(BrowserReplay.prototype, 1);
  installVisualProducer(BrowserLocalGame.prototype, 1);
  installVisualProducer(BrowserRoomResults.prototype, 6);
  installVisualProducer(BrowserCompletedResults.prototype, 5);
  const self = {
    isSecureContext: false, navigator: {},
    postMessage(value, transfer = []) {
      transfers.push([...transfer]);
      messages.push(structuredClone(value, { transfer }));
    },
    addEventListener(name, callback) { assert.equal(name, "message"); receive = callback; },
  };
  const context = createContext({
    self, structuredClone, DataView, File: FileType, TextEncoder, TextDecoder, Uint8Array, Uint32Array, Float32Array, ArrayBuffer, URL, AbortController, AbortSignal,
    performance: { timeOrigin: 10000, now() {
      return networkNow;
    } },
    setTimeout(callback, delay = 0) {
      const id = ++timerId; timers.set(id, callback); timerDelays.set(id, delay); return id;
    },
    clearTimeout(id) { timers.delete(id); timerDelays.delete(id); },
  });
  self.performance = context.performance;
  const wasm = new SyntheticModule(["default", "BrowserLibrary", "BrowserGame", "BrowserReplay", "BrowserMultiplayer", "BrowserLocalGame", "BrowserRoomClient", "BrowserRoomResults", "BrowserCompletedResults"], function () {
    this.setExport("default", async () => { if (options.initGate) await options.initGate.promise; if (options.viewGate) await options.viewGate.promise; });
    this.setExport("BrowserCompletedResults", BrowserCompletedResults);
    this.setExport("BrowserLibrary", BrowserLibrary);
    this.setExport("BrowserGame", BrowserGame);
    this.setExport("BrowserReplay", BrowserReplay);
    this.setExport("BrowserMultiplayer", BrowserMultiplayer);
    this.setExport("BrowserLocalGame", options.missingLocalExport ? undefined : BrowserLocalGame);
    this.setExport("BrowserRoomClient", options.missingRoomExport ? undefined : BrowserRoomClient);
    this.setExport("BrowserRoomResults", options.missingRoomResultsExport ? undefined : BrowserRoomResults);
  }, { context });
  const network = new SyntheticModule(["BrowserMultiplayerOwner"], function () {
    this.setExport("BrowserMultiplayerOwner", BrowserMultiplayerOwner);
  }, { context });
  const resultsModel = new SourceTextModule(await readFile(new URL("./completed-results-model.mjs", import.meta.url), "utf8"), { context });
  const helper = new SourceTextModule(await readFile(new URL("./host_model.mjs", import.meta.url), "utf8"), { context });
  const playHelper = new SourceTextModule(await readFile(new URL("./play-model.mjs", import.meta.url), "utf8"), { context });
  const settingsHelper = new SourceTextModule(await readFile(new URL("./settings-profile.mjs", import.meta.url), "utf8"), { context });
  const opponentHelper = new SourceTextModule(await readFile(new URL("./saved-opponents.mjs", import.meta.url), "utf8"), { context });
  const physicalHelper = new SourceTextModule(await readFile(new URL("./physical-input.mjs", import.meta.url), "utf8"), { context });
  const hidProfileHelper = new SourceTextModule(await readFile(new URL("./hid-profile.mjs", import.meta.url), "utf8"), { context });
  const gamepadProfileHelper = new SourceTextModule(await readFile(new URL("./gamepad-profile.mjs", import.meta.url), "utf8"), { context });
  const pointerProfileHelper = new SourceTextModule(await readFile(new URL("./pointer-profile.mjs", import.meta.url), "utf8"), { context });
  const commandClient = new SourceTextModule(await readFile(new URL("./audio-command-client.mjs", import.meta.url), "utf8"), { context });
  const sampleClient = new SourceTextModule(await readFile(new URL("./audio-sample-client.mjs", import.meta.url), "utf8"), { context });
  const localHelper = new SourceTextModule(await readFile(new URL("./local-play-model.mjs", import.meta.url), "utf8"), { context });
  const roomOwner = new SourceTextModule(await readFile(new URL("./room-owner.mjs", import.meta.url), "utf8"), { context });
  const roomTransport = new SyntheticModule(["WebTransportChannel"], function () {
    this.setExport("WebTransportChannel", RoomChannel);
  }, { context });
  const renderClient = new SourceTextModule(await readFile(new URL("./render-protocol.mjs", import.meta.url), "utf8"), { context });
  const worker = new SourceTextModule(await readFile(new URL("./worker.js", import.meta.url), "utf8"), { context });
  await worker.link(specifier => {
    if (specifier === "./render-protocol.mjs") return renderClient;
    if (specifier === "./pkg/beatkernel_bms_runtime.js") return wasm;
    if (specifier === "./completed-results-model.mjs") return resultsModel;
    if (specifier === "./host_model.mjs") return helper;
    if (specifier === "./play-model.mjs") return playHelper;
    if (specifier === "./settings-profile.mjs") return settingsHelper;
    if (specifier === "./multiplayer-owner.mjs") return network;
    if (specifier === "./saved-opponents.mjs") return opponentHelper;
    if (specifier === "./physical-input.mjs") return physicalHelper;
    if (specifier === "./hid-profile.mjs") return hidProfileHelper;
    if (specifier === "./gamepad-profile.mjs") return gamepadProfileHelper;
    if (specifier === "./pointer-profile.mjs") return pointerProfileHelper;
    if (specifier === "./audio-command-client.mjs") return commandClient;
    if (specifier === "./audio-sample-client.mjs") return sampleClient;
    if (specifier === "./local-play-model.mjs") return localHelper;
    if (specifier === "./room-owner.mjs") return roomOwner;
    if (specifier === "./multiplayer-transport.mjs") return roomTransport;
    throw new Error(`Unexpected import: ${specifier}`);
  });
  await worker.evaluate();
  return {
    renderPort, visualExports, visualAcks, trace, completedOwners, messages, transfers, libraries, preparedOwners, views, games, replays, sectionConstructions, physicalConstructions, contactConstructions, localConstructions, locals, timers, networks, networkSessions,
    roomSessions, roomChannels, roomWrappers, roomResults,
    setNetworkNow(value) { assert.ok(value >= networkNow); networkNow = value; },
    post(request) { receive({ data: renderRequest(request) }); },
    async send(request) {
      request = renderRequest(request);
      if (request?.kind === "play-start" && typeof request.windowOriginNs === "bigint") windowOrigin = request.windowOriginNs;
      if (request?.kind === "play-step" && !Object.hasOwn(request, "nowNs")) request = { ...request, nowNs: request.watermark };
      const host = request?.nowNs ?? request?.watermark ?? request?.hostNs ?? request?.presentedHostNs;
      if (typeof host === "bigint" && host >= 0n && (request.kind !== "play-activate" || !options.allowNetworkClock)) {
        networkNow = Math.max(networkNow, Math.ceil(Number(host + windowOrigin - 10000000000n) / 1000000));
      }
      receive({ data: request }); await flushJobs();
    },
    async tick() {
      const entry = [...timers].find(([id]) => timerDelays.get(id) < 60000);
      if (entry) { timers.delete(entry[0]); entry[1](); }
      await flushJobs();
    },
    async expireNetwork() {
      const entry = [...timers].find(([id]) => timerDelays.get(id) === 2000);
      assert.ok(entry, "expected finite final drain deadline");
      timers.delete(entry[0]); timerDelays.delete(entry[0]); entry[1](); await flushJobs();
    },
    async runTimer(delay) {
      const entry = [...timers].find(([id]) => timerDelays.get(id) === delay);
      assert.ok(entry, `expected controlled ${delay} ms callback`);
      timers.delete(entry[0]); timerDelays.delete(entry[0]); entry[1](); await flushJobs();
    },
    of(kind) { return messages.filter(value => value.kind === kind); },
  };
}


test("renderer failure preserves genuine completion and capture after Window cleanup", async () => {
  const h=await active({recordReplay:true,observeOutput:()=>true});
  await completeOutput(h); await h.send({kind:"play-stop",playId:7,completed:true});
  const terminal=h.of("play-stopped").at(-1),replay=terminal.replay.slice();
  assert.equal(h.visualExports.some(row=>row.kind===5),false);
  await showResults(h);
  const packet=h.renderPort.posts.findLast(row=>row.kind==="packet");
  assert.equal(new DataView(packet.packet.buffer).getUint16(6,true),5);
  h.renderPort.emit({kind:"render-error",generation:packet.generation+1n,content:packet.content,message:"stale graphics error"});await flushJobs();
  assert.equal(h.completedOwners[0].frees,0);
  h.renderPort.emit({kind:"render-error",generation:packet.generation,content:packet.content,message:"result device lost"});await flushJobs();
  assert.equal(h.of("play-error").length,0);assert.equal(h.of("fatal").length,0);
  assert.equal(terminal.completedResults.proof,true);assert.deepEqual(terminal.replay,replay);
  assert.equal(h.games[0].frees,1);
  assert.equal(h.completedOwners[0].frees,0,"graphics failure preserves the genuine completed authority");
  const notice=h.of("play-completed-results").at(-1);
  assert.equal(notice.completedResults.failed,true);assert.match(notice.error,/result device lost/);
  await h.send({kind:"seek",id:3,selectedId:2,ns:0n});
  assert.equal(h.completedOwners[0].frees,1,"explicit navigation disposes retained authority exactly once");
});
