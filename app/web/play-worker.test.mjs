// Deferred: node --experimental-vm-modules --test app/web/play-worker.test.mjs
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
    deliveries:new Map(), comparisons:false, width:0, height:0, page:0, presentations:new Map(), posts:[], starts:0, closes:0, blocked:options.renderBlocked??false, onmessage:null,onmessageerror:null,
    start(){this.starts++;}, close(){this.closes++;},
    emit(reply){Promise.resolve().then(()=>this.onmessage?.({data:structuredClone(reply)}));},
    async deliver(request) {
      const dispatch=this.deliveries.get(request);
      assert.ok(dispatch,"queued operation must exist before actual endpoint application");
      this.deliveries.delete(request);
      Promise.resolve().then(dispatch);
      await flushJobs();
    },
    ack(request,fields={}) {
      const base={generation:request.generation,content:request.content,operationId:request.operationId};
      const h=request.kind==="packet"?new DataView(request.packet.buffer):null;
      this.emit({...base,...(h?{kind:"state-ack",packetKind:h.getUint16(6,true),sequence:h.getBigUint64(24,true)}
        :{kind:"control-ack",operation:request.kind,geometryVersion:request.geometryVersion}),...fields});
    },
    postMessage(request,transfer=[]) {
      const message=structuredClone(request,{transfer}); this.posts.push(message);
      const dispatch=()=>{
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
      };
      this.deliveries.set(message,dispatch);
      Promise.resolve().then(()=>{if(!this.blocked)void this.deliver(message);});
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
  let cadenceNow = 1000;
  let cadenceOffset = 0;
  let acquisitionNow = null;
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
      this.pageValue = page; this.mode = comparisons;
    }
    free() { this.live(); assert.equal(++this.frees, 1);  }
  }
  class BrowserGame {
    completed_archive() { return null; }
    completed_results() {
      this.live();assert.equal(this.stops,0);
      if(!options.completedResults || !this.completedEvidence)return null;
      return new BrowserCompletedResults(this.memberIds??[1]);
    }
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
      this.pendingInput = [];
      this.processedInput = [];
      this.presentations = [];
      this.closedPrefix = null;
      this.outputEvidence = null;
      this.lastService = null;
      this.inputOrdinal = 0;
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
          while (this.pendingInput[0]?.host <= this.closedPrefix
            && this.pendingInput[0].host <= now && this.pendingInput[0].host <= last.host) {
            const event = this.pendingInput.shift();
            event.output = first.output + (event.host - first.host) * (last.output - first.output) / (last.host - first.host);
            event.audio = audio;
            this.processedInput.push(event);
            this.calls.push(["processed", event.host, event.output, audio]);
            if (event.key !== undefined) options.input?.(this, [event.host, event.key, event.down, event.sequence, audio]);
            processed++;
          }
          if (!this.pendingInput.length && this.closedPrefix <= now && last.host <= this.closedPrefix) {
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
      this.completedEvidence=options.observeOutput?.(this, this.outputEvidence.words, this.outputEvidence.presentedNs) ?? false;
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
    advance(...args) { this.live(); this.calls.push(["close", ...args]); options.advance?.(this, args); }
    observe_output(words, presentedNs) {
      this.live();
      this.calls.push(["output", words.slice(), presentedNs]);
      options.admitOutput?.(this, words, presentedNs);
      return options.observeOutput?.(this, words, presentedNs) ?? false;
    }
    observe_presentation(outputNs, hostNs) {
      this.live();
      this.calls.push(["presentation", outputNs, hostNs]);
      options.observePresentation?.(this, outputNs, hostNs);
      const previous = this.presentations.at(-1);
      assert.ok(!previous || (hostNs >= previous.host && outputNs >= previous.output));
      if (!previous || (outputNs > previous.output && hostNs > previous.host)) {
        this.presentations.push({ output: outputNs, host: hostNs });
      }
    }
    commands(max) { this.live(); this.calls.push(["commands", max]); return this.batches.shift() ?? null; }
    acknowledge(...args) { this.live(); this.calls.push(["ack", ...args]); options.ack?.(this, args); }
    stop() {
      this.live();
      assert.equal(++this.stops, 1);
      this.disposals.push("stop");
      if (options.stopError) throw new Error(options.stopError);
    }
    free() {
      options.beforeFree?.(this);
      assert.equal(this.stops, 1);
      assert.equal(++this.frees, 1);
      this.disposals.push("free");
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
    queue_input() { assert.fail("replay must not queue live input"); }
    queue_input_blob() { assert.fail("replay must not queue live physical input"); }
    close_input_prefix() { assert.fail("replay must not close live acquisition prefixes"); }
    service_audio() { assert.fail("replay must preserve its recorded-domain output path"); }
    admit_output() { assert.fail("replay must preserve observe_output completion semantics"); }
    evaluate_completion() { assert.fail("replay must not use live split completion"); }
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
      this.publicationDueCalls = []; this.publicationAtCalls = []; this.publicationDueOutputs = []; this.publicationOutputs = [];
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
    configure_frame_wait(timeout) { this.live(); this.frameTimeout = timeout; this.frameExpires = null; }
    frame_wait_step(elapsed) {
      this.live();
      if (!this.partial) { this.frameExpires = null; return -1n; }
      this.frameExpires ??= elapsed + this.frameTimeout;
      if (elapsed >= this.frameExpires) throw Object.assign(new Error("scripted common frame expiry"), { code: "timeout", operation: "frame" });
      return this.frameExpires - elapsed;
    }
    begin_setup(elapsed, timeout) {
      this.live(); this.setupTimeout = timeout; this.setupExpires = elapsed + timeout; this.setupPhase = "admission";
    }
    setup_wait_step(elapsed) {
      this.live();
      if (this.setupPhase === "complete") return -1n;
      if (this.setupPhase !== "lobby" && elapsed >= this.setupExpires) throw Object.assign(new Error("scripted common setup expiry"),
        { code: "timeout", operation: this.setupPhase === "prepared" ? "prepared" : "setup" });
      if (this.setupPhase === "admission" && this.dto !== null && this.participantValue !== 0n) this.setupPhase = "lobby";
      if (this.setupPhase === "lobby" && this.dto?.phase === 2) { this.setupPhase = "prepared"; this.setupExpires = elapsed + this.setupTimeout; }
      if (this.setupPhase === "prepared" && this.schedules.length) { this.setupPhase = "complete"; return -1n; }
      return this.setupPhase === "lobby" ? -2n : this.setupExpires - elapsed;
    }
    begin_drain(elapsed, timeout) {
      this.live(); this.drainStarted = elapsed; this.drainTimeout = timeout;
      this.drainExpires = elapsed + timeout; this.drainAdmitted = false; this.drainSteps = [];
    }
    drain_requested() { this.live(); return this.drainAdmitted === true; }
    drain_wait_step(elapsed) {
      this.live(); this.drainSteps.push(elapsed);
      if (elapsed >= this.drainExpires) throw Object.assign(new Error("scripted common drain deadline"), { code: "timeout" });
      if (this.progressComplete && !this.drainAdmitted) { this.request("drain"); this.drainAdmitted = true; }
      return this.drainComplete ? -1n : 1000000n;
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
    publication_due(elapsed, finalPrefix) {
      this.live(); this.publicationDueCalls.push({ elapsed, finalPrefix });
      const result = this.publicationDueOutputs.length ? this.publicationDueOutputs.shift()
        : this.lastPublication === undefined || (finalPrefix && !this.finalPublished)
          || elapsed - this.lastPublication >= 250000000n;
      if (result instanceof Error) throw result;
      return result;
    }
    publish_progress_at(words, finalPrefix, elapsed) {
      this.live(); this.publicationAtCalls.push({ elapsed, finalPrefix });
      const result = this.publicationOutputs.length ? this.publicationOutputs.shift() : true;
      if (result instanceof Error) throw result;
      if (result !== true) return result;
      this.publish_progress(words, finalPrefix);
      this.lastPublication = elapsed; this.finalPublished = finalPrefix; return true;
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
  installVisualProducer(BrowserCompletedResults.prototype,5);
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
    performance: { timeOrigin: 10000, now() { return networkNow; } },
    setTimeout(callback, delay = 0) {
      const id = ++timerId; timers.set(id, callback); timerDelays.set(id, delay); return id;
    },
    clearTimeout(id) { timers.delete(id); timerDelays.delete(id); },
  });
  self.performance = context.performance;
  const wasm = new SyntheticModule(["default", "BrowserLibrary", "BrowserGame", "BrowserReplay", "BrowserMultiplayer", "BrowserLocalGame", "BrowserRoomClient", "BrowserRoomResults", "BrowserCompletedResults"], function () {
    this.setExport("default", async () => { if (options.initGate) await options.initGate.promise; if (options.viewGate) await options.viewGate.promise; });
    this.setExport("BrowserLibrary", BrowserLibrary);
    this.setExport("BrowserGame", BrowserGame);
    this.setExport("BrowserReplay", BrowserReplay);
    this.setExport("BrowserMultiplayer", BrowserMultiplayer);
    this.setExport("BrowserLocalGame", options.missingLocalExport ? undefined : BrowserLocalGame);
    this.setExport("BrowserRoomClient", options.missingRoomExport ? undefined : BrowserRoomClient);
    this.setExport("BrowserCompletedResults",BrowserCompletedResults);
    this.setExport("BrowserRoomResults", options.missingRoomResultsExport ? undefined : BrowserRoomResults);
  }, { context });
  const network = new SyntheticModule(["BrowserMultiplayerOwner"], function () {
    this.setExport("BrowserMultiplayerOwner", BrowserMultiplayerOwner);
  }, { context });
  const completedHelper = new SourceTextModule(await readFile(new URL("./completed-results-model.mjs", import.meta.url), "utf8"), { context });
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
    if (specifier === "./host_model.mjs") return helper;
    if (specifier === "./completed-results-model.mjs") return completedHelper;
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
    completedOwners, renderPort, visualExports, visualAcks, messages, transfers, libraries, preparedOwners, views, games, replays, sectionConstructions, physicalConstructions, contactConstructions, localConstructions, locals, timers, networks, networkSessions,
    roomSessions, roomChannels, roomWrappers, roomResults,
    // Existing cadence fixtures specify elapsed control time from their initial
    // 1000ms reading; a future scheduled activation first waits for that target.
    setNetworkNow(value) { assert.ok(value >= cadenceNow); cadenceNow = value; networkNow = value + cadenceOffset; },
    setWindowNowNs(value) {
      networkNow = Number(value + windowOrigin - 10000000000n) / 1000000;
      if (!options.manualClock && options.allowNetworkClock) cadenceOffset = networkNow - cadenceNow;
    },
    windowNowNs() { return 10000000000n + millisecondsToNanos(networkNow) - windowOrigin; },
    acquisitionNowNs() { return acquisitionNow; },
    post(request) { receive({ data: renderRequest(request) }); },
    async send(request) {
      request = renderRequest(request);
      if (request?.kind === "play-start" && typeof request.windowOriginNs === "bigint") windowOrigin = request.windowOriginNs;
      if (request?.kind === "play-step" && !Object.hasOwn(request, "nowNs")) request = { ...request, nowNs: request.watermark };
      if (request?.kind === "play-step") acquisitionNow = request.nowNs;
      if (!options.manualClock && options.allowNetworkClock && request?.kind === "play-step"
        && typeof request.nowNs === "bigint" && request.nowNs > 10000000000n + millisecondsToNanos(networkNow) - windowOrigin) {
        networkNow = Math.ceil(Number(request.nowNs + windowOrigin - 10000000000n) / 1000000);
        cadenceOffset = networkNow - cadenceNow;
      }
      if (!options.manualClock && !options.allowNetworkClock) {
        const host = request?.nowNs ?? request?.watermark ?? request?.hostNs ?? request?.presentedHostNs;
        if (typeof host === "bigint" && host >= 0n) networkNow = Math.ceil(Number(host + windowOrigin - 10000000000n) / 1000000);
        else if (Number.isFinite(request?.observedNowMs)) networkNow = request.observedNowMs;
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

function workerSettings() {
  return { kind: "beatkernel-browser-settings", version: 1,
    timing: { earlyMs: "12.345678", lateMs: "87.654321", offsetMs: "-0.000001" },
    output: { latency: "balanced", latencyMs: "10.000001", rate: "44100" },
    capacities: { queueCapacity: "257", maxVoices: "17", pendingCapacity: "31", maxFrames: "257", maxCommandsPerRender: "7" },
    section: { startSeconds: "604800.000000001", endSeconds: "604800.000000002" },
    bindings: [[17, "KeyA"], [18, ""], [19, ""], [20, ""], [21, ""], [22, ""], [23, ""], [24, ""], [25, ""],
      [33, ""], [34, ""], [35, ""], [36, ""], [37, ""], [38, ""], [39, ""], [40, ""], [41, ""]] };
}
function settingsFile(data, acquire) {
  const selected = new FileType([data], "settings.json", { type: "application/json" });
  let reads = 0;
  selected.arrayBuffer = () => { reads++; return acquire ? acquire() : Promise.resolve(data.slice().buffer); };
  return { file: selected, get reads() { return reads; } };
}

test("actual Worker settings codec reads and transfers bounded files without preparing assets or mutating the selected runtime", async () => {
  const h = await catalogWorker();
  const original = { libraries: h.libraries.length, prepared: h.preparedOwners.length,
    previews: h.views[0].current, preparations: h.libraries[0].preparations.length,
    extents: h.views[0].extents.length, draws: h.views[0].draws };
  const supplied = workerSettings(); supplied.bindings.reverse();
  await h.send({ kind: "settings-profile-save", id: 100, settings: supplied });
  const saved = h.of("settings-profile-saved").at(-1);
  assert.equal(saved.id, 100);
  assert.ok(saved.bytes instanceof Uint8Array);
  assert.ok(saved.bytes.byteLength > 0 && saved.bytes.byteLength <= 16384);
  assert.deepEqual(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(saved.bytes)), workerSettings());
  const transferred = h.transfers[h.messages.indexOf(saved)];
  assert.equal(transferred.length, 1);
  assert.equal(transferred[0].byteLength, 0, "actual postMessage transfer relinquishes Worker encoded storage");
  const selected = settingsFile(saved.bytes);
  await h.send({ kind: "settings-profile-load", id: 101, file: selected.file });
  assert.equal(selected.reads, 1);
  assert.deepEqual(h.of("settings-profile-loaded").at(-1), { kind: "settings-profile-loaded", id: 101, settings: workerSettings() });
  let id = 102;
  for (const fault of ["metadata", "actual-size", "utf8", "foreign", "read-error"]) {
    const valid = new TextEncoder().encode(JSON.stringify(workerSettings()));
    const foreign = workerSettings(); foreign.kind = "native-profile";
    const actual = fault === "actual-size" ? new Uint8Array(16385)
      : fault === "utf8" ? new Uint8Array(valid.length).fill(0xff)
      : fault === "foreign" ? new TextEncoder().encode(JSON.stringify(foreign)) : valid;
    const file = settingsFile(fault === "foreign" ? actual : valid, async () => {
      if (fault === "read-error") throw new Error("actual settings File rejected");
      return actual.buffer;
    });
    if (fault === "metadata") Object.defineProperty(file.file, "size", { value: 16385 });
    await h.send({ kind: "settings-profile-load", id, file: file.file });
    assert.equal(h.of("settings-profile-error").at(-1).id, id++);
    assert.equal(file.reads, fault === "metadata" ? 0 : 1);
    assert.equal(h.of("settings-profile-loaded").length, 1);
  }
  assert.equal(h.libraries.length, original.libraries);
  assert.equal(h.preparedOwners.length, original.prepared);
  assert.equal(h.libraries[0].preparations.length, original.preparations);
  assert.equal(h.views[0].current, original.previews);
  assert.equal(h.views[0].extents.length, original.extents);
  assert.equal(h.views[0].draws, original.draws);
  assert.equal(h.games.length + h.locals.length + h.replays.length, 0);
  assert.equal(h.of("fatal").length, 0);
  await h.send({ kind: "settings-profile-save", id, settings: workerSettings() });
  assert.equal(h.of("settings-profile-saved").at(-1).id, id, "local file errors leave settings retry available");
});

test("Worker settings ownership remains exclusive through actual read settlement and late completion cannot revive a failed owner", async () => {
  const initializing = deferred(), fresh = await workerHarness({ viewGate: initializing });
  const bytes = new TextEncoder().encode(JSON.stringify(workerSettings()));
  const unread = settingsFile(bytes);
  await fresh.send({ kind: "settings-profile-load", id: 1, file: unread.file });
  assert.equal(unread.reads, 0); assert.equal(fresh.of("settings-profile-error").length, 1);
  assert.equal(fresh.of("fatal").length, 0);
  await fresh.send({ kind: "init", canvas: {} });
  await fresh.send({ kind: "settings-profile-load", id: 2, file: unread.file });
  assert.equal(unread.reads, 0);
  initializing.resolve(); await flushJobs();
  await fresh.send({ kind: "settings-profile-load", id: 3, file: unread.file });
  assert.equal(unread.reads, 1); assert.equal(fresh.of("settings-profile-loaded").at(-1).id, 3);

  const h = await catalogWorker(), reading = deferred();
  const selected = settingsFile(bytes, () => reading.promise);
  await h.send({ kind: "settings-profile-load", id: 100, file: selected.file });
  assert.equal(selected.reads, 1); assert.equal(h.of("settings-profile-loaded").length, 0);
  const blocked = settingsFile(bytes);
  await h.send({ kind: "settings-profile-load", id: 101, file: blocked.file });
  await h.send({ kind: "settings-profile-save", id: 102, settings: workerSettings() });
  assert.equal(blocked.reads, 0);
  assert.deepEqual(h.of("settings-profile-error").map(reply => reply.id), [101, 102]);
  await h.send(startRequest());
  assert.equal(h.of("play-error").at(-1).playId, 7);
  assert.equal(h.of("play-error").at(-1).released, true);
  assert.equal(h.games.length, 0);
  await h.send({ kind: "import", id: 4, files: [selectedFile("other/chart.bms")] });
  await h.send({ kind: "select", id: 5, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" });
  assert.equal(h.of("import-error").at(-1).id, 4);
  assert.equal(h.of("selection-error").at(-1).id, 5);
  assert.equal(h.libraries.length, 1);
  assert.equal(h.libraries[0].preparations.length, 1);
  await h.send({ kind: "settings-profile-load", id: 100, file: blocked.file });
  assert.equal(blocked.reads, 0, "stale identity cannot replace the outstanding read");
  reading.resolve(bytes.slice().buffer); await flushJobs();
  assert.equal(h.of("settings-profile-loaded").length, 1);
  assert.equal(h.of("settings-profile-loaded")[0].id, 100);
  await h.send(startRequest({ playId: 8 }));
  assert.equal(h.of("play-reply").at(-1).result.kind, "prepared");
  await h.send({ kind: "settings-profile-load", id: 103, file: blocked.file });
  assert.equal(blocked.reads, 0, "a prepared game retains its exclusive owner");
  assert.equal(h.games[0].frees, 0); assert.equal(h.of("fatal").length, 0);
  await h.send({ kind: "play-stop", playId: 8, completed: false });

  const obsolete = deferred(), late = settingsFile(bytes, () => obsolete.promise);
  await fresh.send({ kind: "settings-profile-load", id: 4, file: late.file });
  await fresh.send(null); // Actual fatal owner boundary; never a fabricated cancellation receipt.
  assert.equal(fresh.of("fatal").length, 1);
  obsolete.resolve(bytes.slice().buffer); await flushJobs();
  assert.equal(fresh.of("settings-profile-loaded").length, 1);
  assert.equal(fresh.of("settings-profile-error").filter(reply => reply.id === 4).length, 0);
  assert.equal(fresh.games.length, 0);
});

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
  const reply = await h.rpc("play-activate", { hostNs: options.activationHost ?? ORIGIN, startFrame: options.activationFrame ?? START });
  assert.equal(reply.result, null);
  return h;
}

function step(fields = {}) {
  return { kind: "play-step", playId: 7, tickId: 1, events: [], watermark: ORIGIN, audioNs: 100000000n,
    nowNs: fields.watermark ?? ORIGIN, ...fields };
}

function assertReleased(h, score = SCORE) {
  const game = h.replays[0] ?? h.games[0];
  assert.equal(game.stops, 1);
  assert.equal(game.frees, 1);
  const last = h.of("play-error").at(-1) ?? h.of("play-stopped").at(-1);
  assert.equal(last.songNs, score.song_ns);
  assert.equal(last.hits, score.hits);
  assert.equal(last.misses, score.misses);
  assert.equal(last.combo, score.combo);
  assert.equal(h.libraries[0].frees, 0, "accepted library remains available after gameplay");
}

function touchEvent(fields = {}) {
  return { kind: "touch", hostNs: ORIGIN, sequence: 1n, contact: 18446744073709551615n,
    phase: 0, code: 0xfffffffe, x: 120, y: 90, pressure: 0.5, width: 480, height: 360,
    surfaceWidth: 960, surfaceHeight: 720, page: 0, ...fields };
}

// Platform endpoint only: the actual AudioCommandClient owns validation,
// sequencing, pending promises and timers inside the Worker module.
function commandPort() {
  return {
    posts: [], starts: 0, closes: 0, onmessage: null, onmessageerror: null,
    start() { this.starts++; },
    postMessage(value) { this.posts.push(structuredClone(value)); },
    close() { this.closes++; },
    async acknowledge(fields = {}) {
      const request = this.posts.at(-1);
      assert.ok(request);
      this.onmessage?.({ data: { kind: "ack", generation: request.generation, sequence: request.sequence,
        operation: request.kind, status: 0, admitted: request.commands?.length ?? 0, error: null, report: null, ...fields } });
      await flushJobs();
    },
  };
}

// Actual AudioSampleClient performs all sequencing/accounting; this port only
// transfers bytes and supplies explicitly controlled Worklet acknowledgements.
function samplePort(onPost = null) {
  return {
    posts: [], transfers: [], starts: 0, closes: 0, onmessage: null, onmessageerror: null,
    start() { this.starts++; },
    postMessage(value, transfer = []) {
      assert.equal(this.closes, 0);
      this.posts.push(structuredClone(value, { transfer })); this.transfers.push([...transfer]);
      onPost?.(this.posts.at(-1));
    },
    close() { this.closes++; },
    async acknowledge(fields = {}) {
      const request = this.posts.at(-1); assert.ok(request);
      this.onmessage?.({ data: { kind: "ack", generation: request.generation, sequence: request.sequence,
        operation: request.kind, status: 0, admitted: 0, error: null, report: null, ...fields } });
      await flushJobs();
    },
  };
}

async function uploadSamples(h, port, fields = {}) {
  const rpcId = ++h.rpcId;
  await h.send({ kind: "play-samples-upload", playId: 7, rpcId, port, generation: 7, channels: 2,
    pcmLimits: { maxAssetBytes: 67108864, maxTotalBytes: 268435456, maxSamples: 7940 }, timeoutMs: 50, ...fields });
  return rpcId;
}

test("direct solo local and replay sample upload waits for every real client ACK and EOS without returning PCM to Window", async () => {
  for (const mode of ["live", "local", "replay", "empty"]) {
    const h = await started(mode === "local" ? { startRequest: localRequest() }
      : mode === "replay" ? { startRequest: replayRequest(replayFile().file) }
      : mode === "empty" ? { samples: [] } : {});
    const game = h.locals[0] ?? h.replays[0] ?? h.games[0], postsBeforeAdmission = [];
    const port = samplePort(message => postsBeforeAdmission.push({ kind: message.kind, admitted: h.of("play-samples-admitted").length }));
    const rpcId = await uploadSamples(h, port), count = mode === "empty" ? 0 : 2;
    assert.equal(port.starts, 1); assert.equal(port.posts.length, 1);
    assert.deepEqual(postsBeforeAdmission, [{ kind: count ? "sample" : "end-samples", admitted: 0 }]);
    assert.deepEqual(h.of("play-samples-admitted"), [{ kind: "play-samples-admitted", playId: 7, rpcId, count }]);
    assert.equal(h.of("play-reply").some(value => value.rpcId === rpcId), false);
    if (count !== 0) {
      assert.deepEqual(port.posts[0], { kind: "sample", generation: 7, sequence: 1, id: 19n, rate: 44100, channels: 2,
        pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]) });
      assert.equal(game.samples[0].frees, 1); assert.equal(game.samples[0].takes, 1);
      assert.equal(game.samples[0].pcm.byteLength, 0); assert.equal(game.samples[1].frees, 0);
      assert.equal(game.calls.filter(row => row[0] === "sample").length, 1);
      await port.acknowledge({ generation: 8 }); assert.equal(port.posts.length, 1);
      await port.acknowledge();
      assert.equal(port.posts.length, 2); assert.equal(port.posts[1].id, 18446744073709551615n);
      assert.equal(port.posts[1].rate, 96000); assert.equal(port.posts[1].sequence, 2);
      assert.deepEqual(Array.from(port.posts[1].pcm), [1, -1]);
      assert.equal(game.samples[1].frees, 1); assert.equal(game.samples[1].takes, 1);
      assert.equal(game.samples[1].pcm.byteLength, 0);
      await port.acknowledge();
    }
    assert.deepEqual(port.posts.at(-1), { kind: "end-samples", generation: 7, sequence: count + 1, count, bytes: count ? 24 : 0 });
    assert.equal(game.calls.filter(row => row[0] === "sample").length, count + 1, "actual null terminator is checked");
    assert.equal(h.of("play-reply").some(value => value.rpcId === rpcId), false);
    assert.equal(game.calls.some(row => row[0] === "activate"), false);
    assert.equal(h.of("play-samples-admitted").length, 1, "per-sample progress cannot generate more Window notifications");
    assert.ok(postsBeforeAdmission.slice(1).every(row => row.admitted === 1));
    await port.acknowledge();
    assert.deepEqual(h.of("play-reply").find(value => value.rpcId === rpcId).result,
      { kind: "samples-uploaded", count, bytes: count ? 24 : 0 });
    assert.equal(h.of("play-samples-admitted").length, 1);
    assert.ok(h.messages.findIndex(value => value.kind === "play-samples-admitted")
      < h.messages.findIndex(value => value.kind === "play-reply" && value.rpcId === rpcId));
    assert.equal(port.closes, 1); assert.equal(port.onmessage, null);
    assert.equal(h.of("play-reply").some(value => value.result?.kind === "sample" || value.result?.pcm), false);
    const commands = commandPort();
    const ready = await h.rpc("play-audio", { port: commands, generation: 7, queueCapacity: 4096, timeoutMs: 50 });
    assert.equal(ready.result.kind, "audio-ready");
    await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
    assert.equal(game.calls.filter(row => row[0] === "activate").length, mode === "replay" ? 0 : 1);
    await h.send({ kind: "play-stop", playId: 7 });
    assert.equal(game.frees, 1); assert.equal(game.stops, 1); assert.equal(port.closes, 1); assert.equal(commands.closes, 1);
  }
});

test("direct sample enumeration, wrapper and remote failures preserve exact prefixes and never acknowledge incomplete setup", async () => {
  const scenarios = [
    { options: { sampleCount: 3 }, acks: 2, reads: 3, takes: [1, 1] },
    { options: { sampleCount: 1 }, acks: 1, reads: 2, takes: [1, 0] },
    { options: { takeError: "original take failed", sampleFreeError: "secondary release failed" }, acks: 0, reads: 1, takes: [1, 0], message: /original take failed/ },
    { options: { samples: [{ id: -1n, rate: 44100, pcm: new Float32Array(2) }] }, acks: 0, reads: 1, takes: [1] },
    { options: {}, reject: "sample", acks: 0, reads: 1, takes: [1, 0] },
    { options: {}, reject: "end", acks: 2, reads: 3, takes: [1, 1] },
    { options: {}, timeout: true, acks: 0, reads: 1, takes: [1, 0] },
  ];
  for (const scenario of scenarios) {
    const h = await started(scenario.options), game = h.games[0], port = samplePort();
    const rpcId = await uploadSamples(h, port);
    for (let index = 0; index < scenario.acks; index++) await port.acknowledge();
    if (scenario.reject) await port.acknowledge({ status: 100, error: scenario.reject === "end" ? "actual EOS totals refused" : "actual PCM refused" });
    if (scenario.timeout) await h.runTimer(50);
    const reply = h.of("play-reply").find(value => value.rpcId === rpcId);
    assert.equal(typeof reply.error, "string"); assert.equal(reply.result, undefined);
    assert.equal(h.of("play-samples-admitted").length, port.posts.length ? 1 : 0);
    if (scenario.message) { assert.match(reply.error, scenario.message); assert.doesNotMatch(reply.error, /secondary release/); }
    assert.equal(game.calls.filter(row => row[0] === "sample").length, scenario.reads);
    assert.deepEqual(game.samples.map(value => value.takes), scenario.takes);
    for (const value of game.samples) assert.ok(value.frees <= 1);
    assert.equal(game.samples[0].frees, 1);
    if (scenario.options.sampleCount === 1) assert.equal(game.samples[1].frees, 1, "extra acquired wrapper is released without taking PCM");
    assert.equal(game.frees, 1); assert.equal(game.stops, 1); assert.equal(port.closes, 1);
    assert.equal(h.of("play-reply").filter(value => value.rpcId === rpcId).length, 1);
    assert.equal(h.of("play-reply").some(value => value.result?.kind === "samples-uploaded"), false);
    assert.equal(game.calls.some(row => row[0] === "activate" || row[0] === "commands"), false);
  }
  for (const method of ["take_pcm", "free"]) for (const cause of [null, false]) {
    const h = await started(), game = h.games[0], wrapper = game.samples[0], port = samplePort();
    wrapper[method] = function () {
      if (method === "take_pcm") assert.equal(++this.takes, 1);
      else assert.equal(++this.frees, 1);
      throw cause;
    };
    const rpcId = await uploadSamples(h, port);
    const response = h.of("play-reply").find(value => value.rpcId === rpcId);
    assert.equal(typeof response.error, "string"); assert.equal(response.result, undefined);
    assert.equal(wrapper.takes, 1); assert.equal(wrapper.frees, 1);
    assert.equal(wrapper.pcm.byteLength, 16, "falsy extraction/release failure never transfers the retained PCM");
    assert.equal(game.samples[1].takes, 0); assert.equal(port.posts.length, 0);
    assert.equal(port.closes, 1); assert.equal(game.frees, 1);
    assert.equal(h.of("play-samples-admitted").length, 0);
    assert.equal(h.of("play-reply").filter(value => value.rpcId === rpcId).length, 1);
    assert.equal(h.of("play-reply").some(value => value.result?.kind === "samples-uploaded"), false);
  }
  for (const count of [-1, 1.5, 7941, Number.MAX_SAFE_INTEGER]) {
    const h = await catalogWorker({ sampleCount: count }); await h.send(startRequest());
    assert.equal(h.of("play-reply").some(value => value.result?.kind === "prepared"), false);
    assert.equal(h.games[0].calls.some(row => row[0] === "sample"), false); assert.equal(h.games[0].frees, 1);
    assert.equal(h.of("play-samples-admitted").length, 0);
  }
  for (const boundary of ["start", "postMessage"]) {
    const h = await started(), port = samplePort();
    port[boundary] = () => { throw new Error(`actual sample endpoint ${boundary} failed`); };
    const rpcId = await uploadSamples(h, port);
    assert.equal(h.of("play-samples-admitted").length, 0);
    assert.equal(h.of("play-reply").filter(value => value.rpcId === rpcId).length, 1);
    assert.match(h.of("play-reply").find(value => value.rpcId === rpcId).error, /sample|start|sent/i);
    assert.equal(port.closes, 1); assert.equal(h.games[0].frees, 1);
    assert.equal(h.games[0].samples[0].frees, boundary === "start" ? 0 : 1);
  }
});

test("stop awaiting a direct sample or EOS settles its RPC once before late ACKs can touch a freed or replacement game", async () => {
  for (const phase of ["sample", "end"]) {
    const port = samplePort();
    const h = await started({ beforeFree() { assert.equal(port.closes, 1, "producer closes before game disposal"); } });
    const game = h.games[0], rpcId = await uploadSamples(h, port);
    if (phase === "end") { await port.acknowledge(); await port.acknowledge(); }
    const stale = port.onmessage, last = port.posts.at(-1);
    assert.equal(last.kind, phase === "sample" ? "sample" : "end-samples");
    assert.deepEqual(h.of("play-samples-admitted"), [{ kind: "play-samples-admitted", playId: 7, rpcId, count: 2 }]);
    await h.send({ kind: "play-stop", playId: 7 });
    assert.equal(game.frees, 1); assert.equal(game.stops, 1); assert.equal(port.closes, 1);
    const cancelled = h.of("play-reply").filter(value => value.rpcId === rpcId);
    assert.equal(cancelled.length, 1); assert.equal(typeof cancelled[0].error, "string");
    const calls = game.calls.length, receipts = h.of("play-reply").length;
    await h.send(startRequest({ playId: 8 })); assert.equal(h.games.length, 2);
    stale({ data: { kind: "ack", generation: 7, sequence: last.sequence, operation: last.kind,
      status: 0, admitted: 0, error: null, report: null } });
    await flushJobs();
    assert.equal(game.calls.length, calls); assert.equal(game.frees, 1);
    assert.equal(h.of("play-reply").length, receipts + 1);
    assert.equal(h.games[1].calls.some(row => row[0] === "sample"), false);
    assert.equal(h.games[1].frees, 0);
    assert.equal(h.of("play-samples-admitted").length, 1, "late ACK cannot publish another admission for either owner");
    await h.send({ kind: "play-stop", playId: 8 }); assert.equal(h.games[1].frees, 1);
  }
});

test("direct sample adoption closes refused endpoints and excludes legacy reads or premature command and activation bypasses", async () => {
  const invalid = [{ generation: 8 }, { channels: 1 }, { timeoutMs: 0 }, { timeoutMs: 60001 },
    { pcmLimits: { maxAssetBytes: 67108863, maxTotalBytes: 268435456, maxSamples: 7940 } },
    { pcmLimits: { maxAssetBytes: 67108864, maxTotalBytes: 268435456, maxSamples: 7939 } }];
  for (const fields of invalid) {
    const h = await started(), port = samplePort(); await uploadSamples(h, port, fields);
    assert.equal(port.closes, 1); assert.equal(port.posts.length, 0);
    assert.equal(h.of("play-samples-admitted").length, 0);
    assert.equal(h.games[0].calls.some(row => row[0] === "sample"), false);
    assert.equal(h.of("play-reply").some(value => value.result?.kind === "samples-uploaded"), false);
  }
  for (const kind of ["play-sample", "play-commands", "play-activate", "play-audio"]) {
    const h = await started(), port = samplePort(), pending = await uploadSamples(h, port);
    const commandEndpoint = kind === "play-audio" ? commandPort() : null;
    await h.rpc(kind, kind === "play-activate" ? { hostNs: ORIGIN, startFrame: START }
      : commandEndpoint ? { port: commandEndpoint, generation: 7, queueCapacity: 4096, timeoutMs: 50 } : {});
    const game = h.games[0]; assert.equal(game.calls.filter(row => row[0] === "sample").length, 1);
    assert.equal(game.calls.some(row => row[0] === "commands" || row[0] === "activate"), false);
    assert.equal(h.of("play-reply").some(value => value.result?.kind === "audio-ready"), false);
    assert.equal(h.of("play-reply").find(value => value.rpcId === pending).result, undefined);
    if (commandEndpoint) assert.equal(commandEndpoint.closes, 1);
    await h.send({ kind: "play-stop", playId: 7 }); assert.equal(port.closes, 1); assert.equal(game.frees, 1);
  }
  const legacy = await started(); assert.equal((await legacy.rpc("play-sample")).result.kind, "sample");
  const refused = samplePort(); await uploadSamples(legacy, refused);
  assert.equal(refused.closes, 1); assert.equal(refused.posts.length, 0);
  assert.equal(legacy.games[0].calls.filter(row => row[0] === "sample").length, 1);
  const ended = await started(), completed = samplePort(); await uploadSamples(ended, completed);
  await completed.acknowledge(); await completed.acknowledge(); await completed.acknowledge();
  const reads = ended.games[0].calls.filter(row => row[0] === "sample").length;
  assert.equal(typeof (await ended.rpc("play-sample")).error, "string");
  assert.equal(ended.games[0].calls.filter(row => row[0] === "sample").length, reads);
  assert.equal(completed.closes, 1); assert.equal(ended.games[0].frees, 1);
  const stale = await started(), old = samplePort(); await uploadSamples(stale, old, { playId: 6 });
  assert.equal(old.closes, 1); assert.equal(stale.games[0].frees, 0);
  assert.equal(stale.games[0].calls.some(row => row[0] === "sample"), false);
  await stale.send({ kind: "play-stop", playId: 7 });
});

async function attachCommands(h, port, fields = {}) {
  for (let count = 0; count < 3; count++) {
    if ((await h.rpc("play-sample")).result.kind === "samples-end") break;
    assert.ok(count < 2, "bounded generated sample owner");
  }
  const rpcId = ++h.rpcId;
  await h.send({ kind: "play-audio", playId: 7, rpcId, port, generation: 7,
    queueCapacity: 4096, timeoutMs: 50, ...fields });
  return rpcId;
}

function localPlan(rows) {
  return new Uint32Array(rows.flatMap(([player, source]) => source === null
    ? [player, 0, 0, 0] : [player, 1, Number(source & 0xffffffffn), Number(source >> 32n)]));
}
function localRequest(fields = {}) {
  return startRequest({ inputMode: "physical-contact",
    localPlanWords: localPlan([[99, HID_SOURCE], [7, 1n], [31, 2n]]), hidSetup: hidSetup(), ...fields });
}

const ROOM_URL = "https://example.test:4433/rooms/fixture";
function roomStartRequest(fields = {}) {
  return startRequest({ inputMode: "physical", localPlanWords: localPlan([[0xffffffff, null]]), recordReplay: true, windowOriginNs: 0n, ...fields });
}
async function roomPrepared(options = {}) {
  const request = { ...(options.startRequest ?? roomStartRequest()), windowOriginNs: options.windowOriginNs ?? 0n };
  const h = await started({ allowNetworkClock: true, ...options, startRequest: request });
  h.roomPort = commandPort();
  const rpc = await attachCommands(h, h.roomPort);
  assert.equal(h.of("play-reply").find(value => value.rpcId === rpc).result.kind, "audio-ready");
  return h;
}
async function requestRoom(h, kind = "play-room-open", fields = {}) {
  const rpcId = ++h.rpcId;
  await h.send({ kind, playId: 7, rpcId, ...(kind === "play-room-open" ? { url: ROOM_URL, windowOriginNs: 0n } : {}), ...fields });
  return rpcId;
}
function roomReply(h, rpc) { return h.of("play-reply").find(value => value.rpcId === rpc); }
async function roomReceive(h, change, bytes = Uint8Array.of(1)) {
  const session = h.roomSessions.at(-1), channel = h.roomChannels.at(-1);
  session.onReceive = change;
  channel.reads.at(-1).gate.resolve(bytes); await flushJobs();
  session.onReceive = null;
}
async function roomSnapshot(h, members = null) {
  const session = h.roomSessions.at(-1);
  await roomReceive(h, () => { session.participantValue = 18446744073709551615n; session.revisionValue++; });
  const snapshot = { phase: 0, deadlineNs: 9223372036854775807n, members: members ?? [
    { participant: 18446744073709551615n, players: session.players.slice(), prepared: false },
    { participant: 9007199254740993n, players: new Uint32Array([800, 4, 0xffffffff]), prepared: false },
  ] };
  await roomReceive(h, () => { session.dto = snapshot; session.revisionValue++; });
  return snapshot;
}
async function preparedRoomReceipt(h, members = null) {
  const session = h.roomSessions.at(-1), channel = h.roomChannels.at(-1);
  channel.writes[0].gate.resolve(); await flushJobs();
  const admitted = await roomSnapshot(h, members);
  await roomReceive(h, () => {
    session.dto = { ...admitted, phase: 1 }; session.revisionValue++;
  });
  session.onRequest = kind => {
    assert.equal(kind, "ready");
    session.frames.push({ kind: 1, id: 2n, bytes: new Uint8Array(11) });
  };
  const rpc = await requestRoom(h, "play-room-ready");
  assert.deepEqual(roomReply(h, rpc).result, { kind: "room-requested", operation: "ready" });
  assert.deepEqual(session.credits, [1n]);
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  await roomReceive(h, () => {
    session.dto = { ...admitted, phase: 2, deadlineNs: null,
      members: admitted.members.map(member => ({ ...member, prepared: true })) };
    session.revisionValue++;
  });
  session.onRequest = null;
  assert.equal(h.of("play-room").some(row => row.event.kind === "start"), false);
}
async function queueRoomStart(h, schedule = { targetNs: 2000000000n, songTargetNs: 2100000000n, uncertaintyNs: 40n }) {
  const session = h.roomSessions.at(-1);
  // Opaque common-client output, not an implementation of its clock or start protocol.
  session.onWritten = id => { if (id === 17n) session.schedules.push(schedule); };
  await roomReceive(h, () => session.frames.push({ kind: 1, id: 17n, bytes: new Uint8Array(11) }));
  assert.equal(h.of("play-room").some(row => row.event.kind === "start"), false);
}
async function committedRoom(h, schedule, members = null) {
  await requestRoom(h);
  await preparedRoomReceipt(h, members);
  await queueRoomStart(h, schedule);
  h.roomChannels.at(-1).writes.at(-1).gate.resolve(); await flushJobs();
  return h.of("play-room").find(row => row.event.kind === "start")?.event;
}

test("actual room Worker asks scalar due before progress words and false admission leaves publication unaccepted", async () => {
  const h = await roomPrepared(); const event = await committedRoom(h);
  await h.rpc("play-activate", { hostNs: event.targetHostNs, targetHostNs: event.targetHostNs, startFrame: START });
  const game = h.locals[0], session = h.roomSessions[0]; const before = game.groupProgressReads;
  session.publicationDueOutputs.push(false);
  await h.send(step({ watermark: event.targetHostNs }));
  assert.equal(game.groupProgressReads, before); assert.equal(session.publicationAtCalls.length, 0); assert.equal(session.publications.length, 0);
  session.publicationDueOutputs.push(true); session.publicationOutputs.push(false);
  await h.send(step({ tickId: 2, watermark: event.targetHostNs }));
  assert.equal(game.groupProgressReads, before + 1); assert.equal(session.publicationAtCalls.length, 1); assert.equal(session.publications.length, 0);
  assert.equal(session.publicationDueCalls.at(-1).finalPrefix, false); assert.equal(typeof session.publicationDueCalls.at(-1).elapsed, "bigint");
  assert.equal(game.stops, 0); assert.equal(game.frees, 0);
  await h.send({ kind: "play-stop", playId: 7 });
});
test("finalQueued follows accepted Rust admission so refusal retries before disposal and success is one-shot", async () => {
  for (const firstRefused of [false, true]) {
    const h = await roomPrepared({ observeOutput: () => true }); const event = await committedRoom(h);
    await h.rpc("play-activate", { hostNs: event.targetHostNs, targetHostNs: event.targetHostNs, startFrame: START });
    h.setWindowNowNs(event.targetHostNs);
    const game = h.locals[0], session = h.roomSessions[0]; session.publicationOutputs.push(...(firstRefused ? [false, true] : [true]));
    session.onPublish = (_, finalPrefix) => { assert.equal(finalPrefix, true); assert.equal(game.stops, 0); assert.equal(game.frees, 0); };
    await h.send(directObservation({ presentedNs: 0n, presentedHostNs: (h.locals[0] ?? h.games[0]).activationHost })); await h.roomPort.acknowledge({ report: renderReport() });
    assert.equal(h.of("play-render-done").at(-1).completed, true);
    assert.equal(session.publications.length, firstRefused ? 0 : 1);
    await h.send({ kind: "play-stop", playId: 7 });
    assert.equal(session.publications.length, 1); assert.equal(session.publicationAtCalls.length, firstRefused ? 2 : 1);
    assert.equal(game.groupProgressReads, firstRefused ? 2 : 1); assert.equal(game.stops, 1); assert.equal(game.frees, 1);
    assert.equal(session.finalAcknowledged, false);
  }
});

test("room acquisition uses every actual local identity only after samples and direct command ACK drain", async () => {
  for (const request of [roomStartRequest(), localRequest({ recordReplay: true, windowOriginNs: 0n })]) {
    const h = await started({ allowNetworkClock: true, batches: [batch(771n)], startRequest: request });
    const game = h.locals[0];
    let rpc = await requestRoom(h);
    assert.ok(roomReply(h, rpc).error);
    assert.equal(h.roomSessions.length, 0); assert.equal(h.roomChannels.length, 0);
    assert.equal(game.frees, 0);
    const port = commandPort(), audioRpc = await attachCommands(h, port);
    rpc = await requestRoom(h);
    assert.ok(roomReply(h, rpc).error);
    assert.equal(game.calls.some(call => call[0] === "member-identity"), false);
    await port.acknowledge();
    assert.equal(roomReply(h, audioRpc).result.kind, "audio-ready");
    rpc = await requestRoom(h, "play-room-open", { identity: Uint8Array.of(9), players: Uint32Array.of(5) });
    assert.deepEqual(roomReply(h, rpc).result, { kind: "room-opened" });
    assert.equal(h.roomSessions.length, 1); assert.equal(h.roomChannels.length, 1);
    const session = h.roomSessions[0], channel = h.roomChannels[0];
    assert.deepEqual(session.players, new Uint32Array(game.memberIds));
    assert.deepEqual(session.identity, Uint8Array.of(66, 75, 82, 0, 255));
    assert.equal(session.preroll, 100000000n);
    assert.deepEqual(game.calls.filter(call => call[0] === "member-identity").map(call => call[1]), game.memberIds);
    assert.equal(channel.url, ROOM_URL); assert.equal(channel.config.maxPrefixBytes, 65808);
    assert.equal(channel.writes.length, 1); assert.deepEqual(session.credits, []);
    assert.equal(game.calls.some(call => call[0] === "activate"), false);
    channel.writes[0].gate.resolve(); await flushJobs();
    assert.deepEqual(session.credits, [1n]);
    await h.send({ kind: "play-stop", playId: 7 });
    assert.equal(game.stops, 1); assert.equal(game.frees, 1);
    assert.equal(session.closes, 1); assert.equal(session.frees, 1); assert.equal(channel.closes, 1);
    assert.equal(port.closes, 1); assert.equal(h.of("play-stopped").length, 1);
    assert.ok(h.roomWrappers.every(wrapper => wrapper.frees === 1));
  }
});

test("room mode, capability, URL and complete member identity refusals stay before acquisition and preserve local preparation", async () => {
  for (const options of [
    { missingRoomExport: true }, { missingRoomMethod: "receive_bytes" }, { missingRoomMethod: "take_start" },
    { missingRoomMethod: "publish_progress" }, { missingRoomMethod: "publication_due" }, { missingRoomMethod: "publish_progress_at" }, { missingRoomMethod: "request_drain" },
    { missingRoomMethod: "drain_complete" },
    ...["begin_drain", "drain_wait_step", "drain_requested", "begin_setup", "setup_wait_step", "configure_frame_wait", "frame_wait_step"]
      .map(missingRoomMethod => ({ missingRoomMethod })),
    { missingLocalProgress: true },
    { missingRoomStartConstructor: true }, { missingLocalIdentity: true },
    { roomConstructError: "actual room constructor refused" },
    { localIdentityError: player => player === 7 },
    { localIdentity: player => Uint8Array.of(player === 7 ? 2 : 1) },
    { localIdentity: player => player === 7 ? new Uint8Array() : Uint8Array.of(1) },
    { localIdentity: player => player === 7 ? new Uint8Array(new SharedArrayBuffer(1)) : Uint8Array.of(1) },
  ]) {
    const h = await roomPrepared({ ...options, startRequest: localRequest() });
    const game = h.locals[0], rpc = await requestRoom(h);
    assert.ok(roomReply(h, rpc).error); assert.equal(h.roomChannels.length, 0);
    assert.equal(game.frees, 0); assert.equal(h.of("play-error").length, 0);
    await h.send({ kind: "play-stop", playId: 7 }); assert.equal(game.frees, 1);
  }
  for (const url of ["http://example.test/rooms/a", "https://example.test/not-a-room", "https://example.test/rooms/a?extra=1"]) {
    const h = await roomPrepared(); const rpc = await requestRoom(h, "play-room-open", { url });
    assert.ok(roomReply(h, rpc).error); assert.equal(h.roomChannels.length, 0);
    assert.equal(h.locals[0].frees, 0);
    assert.deepEqual(roomReply(h, await requestRoom(h)).result, { kind: "room-opened" }, "preflight refusal consumes no room attempt");
    await h.send({ kind: "play-stop", playId: 7 });
  }
  for (const request of [startRequest(), replayRequest(replayFile().file), localNetworkRequest()]) {
    const h = await started({ allowNetworkClock: true, startRequest: request });
    const port = commandPort(); await attachCommands(h, port);
    const rpc = await requestRoom(h);
    assert.ok(roomReply(h, rpc).error); assert.equal(h.roomSessions.length, 0); assert.equal(h.roomChannels.length, 0);
    assert.equal((h.locals[0] ?? h.replays[0] ?? h.games[0]).frees, 0);
    await h.send({ kind: "play-stop", playId: 7 });
  }
  const activated = await roomPrepared();
  assert.equal((await activated.rpc("play-activate", { hostNs: ORIGIN, startFrame: START })).result, null);
  assert.ok(roomReply(activated, await requestRoom(activated)).error);
  assert.equal(activated.roomSessions.length, 0); assert.equal(activated.locals[0].frees, 0);
  await activated.send({ kind: "play-stop", playId: 7 });

  const legacyDrain = await started({ startRequest: roomStartRequest() });
  for (let index = 0; index < 3; index++) await legacyDrain.rpc("play-sample");
  assert.equal((await legacyDrain.rpc("play-commands")).result, null);
  assert.ok(roomReply(legacyDrain, await requestRoom(legacyDrain)).error,
    "room setup requires the retained direct command owner even after legacy pulls drain");
  assert.equal(legacyDrain.roomSessions.length, 0); assert.equal(legacyDrain.locals[0].frees, 0);
  await legacyDrain.send({ kind: "play-stop", playId: 7 });
});

test("room requests preserve recoverable state errors and Leave waits for actual write credit plus joined close", async () => {
  const h = await roomPrepared({ roomHoldAfterClose: true });
  const game = h.locals[0];
  assert.deepEqual(roomReply(h, await requestRoom(h)).result, { kind: "room-opened" });
  const session = h.roomSessions[0], channel = h.roomChannels[0];
  channel.writes[0].gate.resolve(); await flushJobs();
  await roomSnapshot(h);
  session.requestError = new Error("common InvalidState");
  for (const kind of ["play-room-seal", "play-room-ready"]) {
    assert.ok(roomReply(h, await requestRoom(h, kind)).error);
    assert.equal(game.frees, 0); assert.equal(channel.closes, 0);
  }
  session.requestError = null;
  session.onRequest = kind => session.frames.push({ kind: 1,
    id: kind === "seal" ? 9007199254740993n : kind === "ready" ? 9007199254740994n : 18446744073709551615n,
    bytes: new Uint8Array(11) });
  for (const operation of ["seal", "ready"]) {
    const rpc = await requestRoom(h, `play-room-${operation}`);
    assert.deepEqual(roomReply(h, rpc).result, { kind: "room-requested", operation });
    const written = session.credits.length;
    assert.equal(channel.activeWrites, 1); assert.equal(channel.activeReads, 1);
    channel.writes.at(-1).gate.resolve(); await flushJobs();
    assert.equal(session.credits.length, written + 1);
  }
  session.onWritten = id => { if (id === 18446744073709551615n) session.left = true; };
  const leaving = await requestRoom(h, "play-room-leave");
  assert.equal(roomReply(h, leaving), undefined);
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  assert.equal(session.left, true); assert.equal(channel.closes, 1);
  assert.equal(roomReply(h, leaving), undefined, "Leave RPC must also join the held read continuation");
  channel.reads.at(-1).gate.reject(new Error("read cancelled")); await flushJobs();
  assert.deepEqual(roomReply(h, leaving).result, { kind: "room-left", leaveWritten: true });
  assert.equal(h.of("play-room").filter(row => row.event.kind === "closed").length, 1);
  assert.equal(h.of("play-error").length, 0); assert.equal(game.frees, 0);
  assert.ok(roomReply(h, await requestRoom(h)).error, "leaving cannot erase the lifetime attempt");
  assert.equal(h.roomSessions.length, 1); assert.equal(h.roomChannels.length, 1);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.frees, 1); assert.equal(session.frees, 1); assert.equal(channel.closes, 1);
});

test("room snapshots carry actual participant and ordered full-width DTOs without granting gameplay activation", async () => {
  const h = await roomPrepared({ startRequest: localRequest({ recordReplay: true }) });
  const game = h.locals[0]; await requestRoom(h);
  h.roomChannels[0].writes[0].gate.resolve(); await flushJobs();
  const snapshot = await roomSnapshot(h);
  const event = h.of("play-room").find(row => row.event.kind === "snapshot");
  assert.equal(event.playId, 7); assert.equal(event.event.participant, 18446744073709551615n);
  assert.deepEqual(event.event.snapshot, snapshot);
  assert.deepEqual([...event.event.snapshot.members[0].players], game.memberIds);
  assert.equal(game.memberPeerUpdates.length, 0); assert.equal(h.networks.length, 0);
  assert.equal(game.calls.some(call => call[0] === "close" || call[0] === "activate"), false);
  const activation = await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
  assert.ok(activation.error);
  assert.equal(game.calls.some(call => call[0] === "activate"), false);
  assert.equal(game.stops, 1); assert.equal(game.frees, 1);
  const terminal = h.of("play-error").at(-1);
  assert.deepEqual(terminal.replays.map(row => row.player), game.memberIds);
  assert.ok(terminal.replays.every(row => row.replay instanceof Uint8Array && !row.replayComplete));
  const count = h.messages.length;
  h.roomChannels[0].reads.at(-1).gate.resolve(Uint8Array.of(1)); await flushJobs();
  assert.equal(h.messages.length, count);
});

test("stop during room opening disposes gameplay now but joins late acquisition and settles the pending RPC once", async () => {
  const gate = deferred(); const h = await roomPrepared({ roomOpenGate: gate });
  const game = h.locals[0], opening = await requestRoom(h);
  assert.equal(roomReply(h, opening), undefined); assert.equal(h.roomChannels.length, 1);
  assert.ok(roomReply(h, await requestRoom(h)).error, "attempt reserved before the first await");
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.stops, 1); assert.equal(game.frees, 1);
  assert.equal(h.of("play-stopped").length, 0, "terminal ownership cannot precede late channel cleanup");
  await h.send(startRequest({ playId: 8, rpcId: 1 }));
  assert.equal(h.games.length, 0, "replacement waits for joined room cleanup");
  const channel = h.roomChannels[0];
  gate.resolve(); await flushJobs();
  assert.equal(channel.closes, 1); assert.equal(channel.reads.length, 0); assert.equal(channel.writes.length, 0);
  assert.equal(h.roomSessions[0].closes, 1); assert.equal(h.roomSessions[0].frees, 1);
  assert.equal(h.of("play-reply").filter(row => row.playId === 7 && row.rpcId === opening).length, 1);
  assert.ok(roomReply(h, opening).error); assert.equal(h.of("play-stopped").length, 1);
  assert.equal(h.games[0].frees, 0);
  const before = h.messages.length;
  await h.send({ kind: "play-room-ready", playId: 7, rpcId: h.rpcId + 1 });
  assert.equal(h.messages.length, before); assert.equal(h.games[0].frees, 0);
  await h.send({ kind: "play-stop", playId: 8 });
});

test("fatal room I/O and malformed complete snapshots retain capture prefixes while terminal cleanup joins pending work", async () => {
  for (const failure of ["read", "snapshot"]) {
    const h = await roomPrepared({ roomHoldAfterClose: true }); await requestRoom(h);
    const game = h.locals[0], session = h.roomSessions[0], channel = h.roomChannels[0];
    if (failure === "read") channel.reads[0].gate.reject(new Error("actual channel read failed"));
    else {
      channel.writes[0].gate.resolve(); await flushJobs();
      await roomSnapshot(h);
      session.frames.push({ kind: 1, id: 17n, bytes: new Uint8Array(11) });
      const seal = await requestRoom(h, "play-room-seal"); assert.ok(roomReply(h, seal).result);
      await roomReceive(h, () => {
        session.revisionValue++; session.dto = { ...session.dto,
          members: [{ ...session.dto.members[0], players: Uint32Array.of(0) }, session.dto.members[1]] };
      });
    }
    await flushJobs();
    assert.equal(game.stops, 1); assert.equal(game.frees, 1); assert.equal(channel.closes, 1);
    assert.equal(h.of("play-error").length, 0, "pending write continuation still belongs to cleanup");
    const credits = [...session.credits];
    channel.writes.at(-1).gate.resolve(); await flushJobs();
    assert.deepEqual(session.credits, credits, "late transport fulfillment cannot credit a freed session");
    assert.equal(h.of("play-error").length, 1); assert.equal(session.frees, 1);
    assert.ok(h.of("play-error")[0].replays.every(row => row.replay instanceof Uint8Array && row.replayComplete === false));
    assert.equal(h.of("play-room").filter(row => row.event.kind === "closed").length, 1);
    await h.send({ kind: "play-stop", playId: 7 });
    assert.equal(game.stops, 1); assert.equal(session.frees, 1);
  }
});

test("actual room owner delivers only the committed full-width start and Worker translates the original Window origin", async () => {
  const windowOriginNs = 10000000003n;
  const h = await roomPrepared({ windowOriginNs });
  const game = h.locals[0];
  const rpc = await requestRoom(h, "play-room-open", { windowOriginNs });
  assert.deepEqual(roomReply(h, rpc).result, { kind: "room-opened" });
  const session = h.roomSessions[0], channel = h.roomChannels[0];
  assert.equal(session.preroll, 100000000n);
  await preparedRoomReceipt(h);
  const snapshots = h.of("play-room").filter(row => row.event.kind === "snapshot").length;
  const schedule = { targetNs: 9007199254740993n, songTargetNs: 9007199354740993n, uncertaintyNs: 37n };
  h.setNetworkNow(1200);
  await queueRoomStart(h, schedule);
  assert.equal(channel.writes.length, 3); assert.equal(channel.activeReads, 1); assert.equal(channel.activeWrites, 1);
  assert.equal(game.calls.some(row => row[0] === "activate"), false);
  assert.deepEqual(session.credits, [1n, 2n]);
  h.setNetworkNow(1500);
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  assert.deepEqual(session.writeTimes.at(-1), { id: 17n, completed: 500000000n, processing: 500000000n });
  assert.deepEqual(session.receiveTimes.at(-1), { captured: 200000000n, processing: 200000000n });
  const event = h.of("play-room").find(row => row.event.kind === "start");
  assert.deepEqual(event, { kind: "play-room", playId: 7, event: {
    kind: "start", targetHostNs: 9007200254740990n, songTargetHostNs: 9007200354740990n, uncertaintyNs: 37n,
  } });
  assert.equal(h.of("play-room").filter(row => row.event.kind === "snapshot").length, snapshots);
  const target = event.event.targetHostNs;
  schedule.targetNs = 0n;
  const hostNs = target + 20835n; // ceil(1e9 / 48000) + 1, the existing one-frame bound.
  const activated = await h.rpc("play-activate", { targetHostNs: target, hostNs, startFrame: START });
  assert.equal(activated.result, null);
  assert.deepEqual(game.calls.filter(row => row[0] === "activate"), [["activate", hostNs]]);
  await h.send(step({ events: [{ hostNs: hostNs + 11n, key: 2, down: true, sequence: 18446744073709551615n }],
    watermark: hostNs + 11n, audioNs: 100000000n }));
  const blob = game.calls.find(row => row[0] === "blob");
  assert.deepEqual(blob[1], encodeKeyboardEvent({ hostNs: hostNs + 11n, key: 2, down: true, sequence: 18446744073709551615n }));
  assert.equal(blob[2], h.acquisitionNowNs(), "queue admission carries the same-Window acquisition receipt");
  assert.equal(game.lastService.audio, 100000000n, "scheduling remains a separate raw-audio coordinate");
  assert.equal(h.of("play-step-done").at(-1).tickId, 1);
  await roomReceive(h, () => {});
  assert.equal(h.of("play-room").filter(row => row.event.kind === "start").length, 1);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.frees, 1); assert.equal(session.frees, 1); assert.equal(channel.closes, 1);
  assert.deepEqual(h.of("play-stopped")[0].replays.map(row => row.player), [0xffffffff]);
});

test("room Window origin, committed preroll and exact live activation boundaries refuse without alternate starts", async () => {
  for (const windowOriginNs of [undefined, -1n, 1, 9223372036854775808n]) {
    const h = await roomPrepared();
    const rpc = await requestRoom(h, "play-room-open", { windowOriginNs });
    assert.ok(roomReply(h, rpc).error); assert.equal(h.roomSessions.length, 0);
    assert.equal(h.locals[0].frees, 0);
    assert.deepEqual(roomReply(h, await requestRoom(h)).result, { kind: "room-opened" });
    await h.send({ kind: "play-stop", playId: 7 });
  }
  for (const schedule of [
    { targetNs: 2000000000n, songTargetNs: 2099999999n, uncertaintyNs: 40n },
    { targetNs: 9223372036854775807n, songTargetNs: 9223372036854775807n, uncertaintyNs: 0n },
  ]) {
    const h = await roomPrepared();
    assert.equal(await committedRoom(h, schedule), undefined);
    assert.equal(h.of("play-room").filter(row => row.event.kind === "start").length, 0);
    assert.equal(h.locals[0].calls.some(row => row[0] === "activate"), false);
    assert.equal(h.locals[0].frees, 1); assert.equal(h.roomSessions[0].frees, 1);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const fault of ["missing-target", "wrong-target", "before-target", "rounding", "past", "repeated", "leaving"]) {
    const h = await roomPrepared(); const event = await committedRoom(h);
    assert.ok(event);
    const request = { targetHostNs: event.targetHostNs, hostNs: event.targetHostNs, startFrame: START };
    if (fault === "missing-target") delete request.targetHostNs;
    if (fault === "wrong-target") request.targetHostNs++;
    if (fault === "before-target") request.hostNs--;
    if (fault === "rounding") request.hostNs += 20836n;
    if (fault === "past") h.setNetworkNow(3000); // Now maps to the exact 13 s target.
    if (fault === "repeated") assert.equal((await h.rpc("play-activate", request)).result, null);
    if (fault === "leaving") {
      const session = h.roomSessions[0];
      session.onRequest = kind => { assert.equal(kind, "leave"); session.frames.push({ kind: 1, id: 18n, bytes: new Uint8Array(11) }); };
      const leaving = await requestRoom(h, "play-room-leave");
      assert.equal(roomReply(h, leaving), undefined);
    }
    assert.ok((await h.rpc("play-activate", request)).error);
    assert.equal(h.locals[0].calls.filter(row => row[0] === "activate").length, fault === "repeated" ? 1 : 0);
    assert.equal(h.locals[0].frees, 1); assert.equal(h.roomSessions[0].frees, 1);
    assert.equal(h.of("play-error").length, 1);
  }
});

test("Prepared timeout and stop during a pending room control join cleanup and never authorize a later owner", async () => {
  const timed = await roomPrepared(); await requestRoom(timed); await preparedRoomReceipt(timed);
  await roomReceive(timed, () => {});
  timed.setNetworkNow(11000);
  await timed.runTimer(10000);
  assert.equal(timed.of("play-room").some(row => row.event.kind === "start"), false);
  assert.equal(timed.locals[0].frees, 1); assert.equal(timed.roomSessions[0].frees, 1);
  assert.equal(timed.of("play-error").length, 1);

  const h = await roomPrepared({ roomHoldAfterClose: true }); await requestRoom(h); await preparedRoomReceipt(h);
  await queueRoomStart(h);
  const session = h.roomSessions[0], channel = h.roomChannels[0], game = h.locals[0];
  const credits = [...session.credits], received = session.received.length;
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.frees, 1); assert.equal(session.frees, 1);
  assert.equal(h.of("play-stopped").length, 0);
  await h.send(startRequest({ playId: 8, rpcId: 1 }));
  assert.equal(h.games.length, 0, "replacement waits for joined room cleanup");
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  assert.equal(h.of("play-stopped").length, 0, "the outstanding read is still owned by room cleanup");
  channel.reads.at(-1).gate.resolve(Uint8Array.of(1)); await flushJobs();
  assert.deepEqual(session.credits, credits); assert.equal(session.received.length, received);
  assert.equal(h.of("play-room").filter(row => row.event.kind === "start").length, 0);
  assert.equal(h.of("play-stopped").length, 1); assert.equal(h.games[0].frees, 0);
  const count = h.messages.length;
  await h.send({ kind: "play-activate", playId: 7, rpcId: h.rpcId + 1,
    targetHostNs: 13000000000n, hostNs: 13000000000n, startFrame: START });
  assert.equal(h.messages.length, count); assert.equal(h.games[0].frees, 0);
  await h.send({ kind: "play-stop", playId: 8 });
});

test("room progress uses the admitted member order and 250 ms acquisition cadence while pending-start peer prefixes stay inside Worker", async () => {
  const words = new Uint32Array([
    99, 0xffffffff, 0x7fffffff, 0xffffffff, 0xffffffff, 0, 0, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
    7, 0xffffffff, 0xffffffff, 1, 0, 0, 0, 1, 0, 1, 0,
    31, 0, 0x80000000, 0, 0, 0, 0, 0, 0, 0, 0,
  ]);
  const original = words.slice();
  const h = await roomPrepared({ startRequest: localRequest({ recordReplay: true }), localProgressWords: () => words });
  const game = h.locals[0];
  await requestRoom(h); await preparedRoomReceipt(h); await queueRoomStart(h);
  const session = h.roomSessions[0], channel = h.roomChannels[0];
  const peerWords = new Uint32Array([
    800, 0xffffffff, 0x7fffffff, 0xffffffff, 0xffffffff, 0, 0, 1, 0, 0xffffffff, 0xffffffff,
    4, 0, 0, 2, 0, 0, 0, 2, 0, 2, 0,
    0xffffffff, 0xffffffff, 0xffffffff, 0, 0, 0, 0, 0, 0, 0, 0,
  ]);
  const acceptedPeer = peerWords.slice(), before = h.messages.length;
  await roomReceive(h, () => session.peerProgress.push({ participant: 9007199254740993n,
    sequence: 18446744073709551615n, finalPrefix: true, words: peerWords }));
  peerWords.fill(0);
  assert.equal(h.messages.length, before, "accepted pending-Commit progress creates no Window event");
  assert.equal(game.groupProgressReads, 0); assert.deepEqual(session.publications, []);
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  const target = h.of("play-room").find(row => row.event.kind === "start").event.targetHostNs;
  assert.equal((await h.rpc("play-activate", { hostNs: target, targetHostNs: target, startFrame: START })).result, null);
  assert.equal(game.groupProgressReads, 0, "activation alone has no new gameplay prefix");
  await h.send(step({ watermark: target }));
  assert.equal(game.groupProgressReads, 1);
  assert.deepEqual(session.publications, [{ words: original, finalPrefix: false }]);
  words[3] = 0xfffffffe;
  assert.deepEqual(session.publications[0].words, original, "published binding words are independently owned");
  words.set(original);
  words[12] = 0; words[13] = 0; // The second member's actual song frontier advances from -1 to 0.
  h.setNetworkNow(1249);
  await h.send(directObservation());
  await h.roomPort.acknowledge({ report: renderReport() });
  assert.equal(game.groupProgressReads, 1, "a genuine output observation before the cadence acquires no words");
  h.setNetworkNow(1250);
  await h.send(step({ tickId: 2, watermark: target + 1n }));
  assert.equal(game.groupProgressReads, 2); assert.equal(session.publications.length, 2);
  assert.deepEqual(session.publications[1].words, words);
  assert.equal(h.of("play-room").some(row => ["progress", "peer-progress", "receipts"].includes(row.event.kind)), false);
  assert.deepEqual(game.memberPeerUpdates, [], "room participants cannot alias the bilateral per-member peer slot");
  assert.deepEqual(game.roomHudCalls.filter(call => call[0] === "update"), [
    ["update", 9007199254740993n, 18446744073709551615n, true, acceptedPeer],
  ]);
  await h.send({ kind: "play-stop", playId: 7 });
  const final = h.of("play-stopped").at(-1);
  assert.deepEqual(final.room.peers, [{ participant: 9007199254740993n,
    sequence: 18446744073709551615n, finalPrefix: true, words: acceptedPeer }]);
  assert.deepEqual(final.replays.map(row => row.player), [99, 7, 31]);
});

test("room HUD bridges every prepared participant and player with exact words and bounded correlated paging", async () => {
  for (const count of [2, 3, 4, 64]) {
    const pages = (count - 1) * 16;
    const h = await roomPrepared({ roomHudPages: pages,
      ...(count === 3 ? { startRequest: localRequest({ recordReplay: true }) } : {}) });
    const game = h.locals[0];
    const contactSetup = game.calls.filter(call => call[0].startsWith("local-touch"));
    await requestRoom(h);
    const session = h.roomSessions[0], channel = h.roomChannels[0];
    const members = Array.from({ length: count }, (_, index) => ({
      participant: 18446744073709551615n - BigInt(index), prepared: false,
      players: index === 0 ? session.players.slice() : Uint32Array.from({ length: 64 }, (_, player) => 0xffffffff - player),
    }));
    [members[0], members[Math.floor(count / 2)]] = [members[Math.floor(count / 2)], members[0]];
    await preparedRoomReceipt(h, members);
    const expected = new Uint32Array(members.flatMap(member => [
      Number(member.participant & 0xffffffffn), 0xffffffff, member.players.length, ...member.players,
    ]));
    assert.deepEqual(game.roomHudCalls, [["configure", 18446744073709551615n, expected]]);
    assert.deepEqual(h.of("play-room").filter(row => row.event.kind === "score-pages").map(row => row.event),
      [{ kind: "score-pages", page: 0, pages }]);
    assert.equal(game.groupProgressReads, 0);
    await roomReceive(h, () => { session.dto = { ...session.dto }; session.revisionValue++; });
    assert.equal(game.roomHudCalls.filter(call => call[0] === "configure").length, 1);
    assert.equal(h.of("play-room").filter(row => row.event.kind === "score-pages").length, 1);
    await queueRoomStart(h);
    const remote = members.filter(member => member.participant !== 18446744073709551615n);
    const original = [];
    const messages = h.messages.length;
    for (const member of [...remote].reverse()) {
      const words = new Uint32Array(Array.from(member.players).flatMap(player => [
        player, 0xffffffff, 0x7fffffff, 0xffffffff, 0xffffffff, 0, 0, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
      ]));
      original.push(["update", member.participant, 18446744073709551615n, true, words.slice()]);
      await roomReceive(h, () => session.peerProgress.push({ participant: member.participant,
        sequence: 18446744073709551615n, finalPrefix: true, words }));
      words.fill(0);
    }
    assert.deepEqual(game.roomHudCalls.filter(call => call[0] === "update"), original);
    assert.equal(h.messages.length, messages, "all accepted remote rows remain off the Window message path");
    assert.deepEqual(game.memberPeerUpdates, [], "same PlayerIds on different hosts never enter bilateral slots");
    channel.writes.at(-1).gate.resolve(); await flushJobs();
    assert.deepEqual(game.roomHudCalls.at(-1), ["status", 1]);
    const start = h.of("play-room").find(row => row.event.kind === "start").event;
    await h.rpc("play-activate", { hostNs: start.targetHostNs, targetHostNs: start.targetHostNs, startFrame: START });
    await h.send(step({ watermark: start.targetHostNs }));
    assert.equal(session.publications.length, 1);
    for (const page of [pages - 1, 0]) {
      const rpc = await requestRoom(h, "play-room-page", { page });
      assert.deepEqual(roomReply(h, rpc).result, { kind: "room-page", page, pages });
      assert.deepEqual(game.roomHudCalls.at(-1), ["page", page]);
    }
    const calls = game.roomHudCalls.length;
    for (const page of [-1, pages, 0.5, "1", undefined]) {
      const rpc = await requestRoom(h, "play-room-page", { page });
      assert.ok(roomReply(h, rpc).error);
      assert.equal(game.roomHudCalls.length, calls, "bad page cannot mutate the Rust presentation");
      assert.equal(game.frees, 0);
    }
    channel.reads.at(-1).gate.reject(new Error("room disconnected after accepted prefixes")); await flushJobs();
    assert.deepEqual(game.roomHudCalls.at(-1), ["status", 2]);
    const retainedPage = await requestRoom(h, "play-room-page", { page: pages - 1 });
    assert.deepEqual(roomReply(h, retainedPage).result, { kind: "room-page", page: pages - 1, pages });
    assert.equal(game.frees, 0);
    await h.send({ kind: "play-stop", playId: 7 });
    const final = h.of("play-stopped").at(-1);
    assert.deepEqual(final.room.peers.map(row => row.participant), remote.map(member => member.participant));
    assert.equal(final.room.peers.reduce((length, row) => length + row.words.length / 11, 0), (count - 1) * 64);
    assert.deepEqual(game.calls.filter(call => call[0].startsWith("local-touch")), contactSetup,
      "room score membership, page changes and disconnect never remap local contact geometry");
    assert.equal(game.frees, 1);
    const hudCalls = game.roomHudCalls.length;
    const resultPage = await requestRoom(h, "play-room-page", { page: 0 });
    assert.deepEqual(roomReply(h, resultPage).result, { kind: "room-page", page: 0, pages });
    assert.equal(h.roomResults.length, 1); assert.equal(h.roomResults[0].page, 0);
    assert.equal(game.roomHudCalls.length, hudCalls, "joined pages use the archive, never the freed live HUD");
  }
});

test("room HUD capability and binding failures fence only presentation and preserve gameplay, publication and capture", async () => {
  const cases = [
    ...["configure_room_hud", "update_room_hud", "set_room_hud_status", "set_room_hud_page", "room_hud_pages", "disable_room_hud"]
      .map(missingRoomHudMethod => ({ missingRoomHudMethod })),
    { roomHudConfigureError: "display setup refused" }, { roomHudPages: 0 },
    { roomHudUpdateError: "display prefix refused", roomHudDisableError: "display disable refused" },
    { roomHudStatusError: "display lifecycle refused" }, { roomHudPageError: "display page refused" },
  ];
  for (const options of cases) {
    const h = await roomPrepared(options), game = h.locals[0], start = await committedRoom(h);
    const session = h.roomSessions[0], channel = h.roomChannels[0];
    await h.rpc("play-activate", { hostNs: start.targetHostNs, targetHostNs: start.targetHostNs, startFrame: START });
    const words = new Uint32Array([800, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0,
      4, 0, 0, 2, 0, 0, 0, 2, 0, 2, 0, 0xffffffff, 0, 0, 3, 0, 0, 0, 3, 0, 3, 0]);
    for (const sequence of [1n, 2n]) await roomReceive(h, () => session.peerProgress.push({
      participant: 9007199254740993n, sequence, finalPrefix: false, words,
    }));
    const page = await requestRoom(h, "play-room-page", { page: 0 });
    assert.ok(roomReply(h, page).error);
    assert.equal(h.of("play-room").filter(row => row.event.kind === "display-unavailable").length, 1);
    assert.ok(game.roomHudCalls.filter(call => call[0] === "disable").length <= 1);
    assert.equal(h.of("play-error").length, 0); assert.equal(channel.closes, 0); assert.equal(game.stops, 0);
    await h.send(step({ watermark: start.targetHostNs }));
    assert.equal(h.of("play-step-done").length, 1); assert.equal(session.publications.length, 1);
    assert.equal(game.calls.some(call => call[0] === "close"), true);
    await h.send({ kind: "play-stop", playId: 7 });
    const final = h.of("play-stopped").at(-1);
    assert.equal(game.frees, 1); assert.equal(h.roomSessions[0].frees, 1); assert.equal(final.room.error, null);
    assert.equal(final.room.peers[0].sequence, 2n);
    assert.ok(final.replays[0].replay instanceof Uint8Array);
    assert.equal(game.frees, 1); assert.equal(session.frees, 1);
  }
});

test("a completed output publishes one final room prefix outside cadence while actual write and read receipts remain distinct", async () => {
  let complete = false;
  const h = await roomPrepared({ observeOutput: () => complete });
  const game = h.locals[0], event = await committedRoom(h);
  assert.equal((await h.rpc("play-activate", { hostNs: event.targetHostNs,
    targetHostNs: event.targetHostNs, startFrame: START })).result, null);
  const session = h.roomSessions[0], channel = h.roomChannels[0];
  session.onPublish = (_, finalPrefix) => {
    assert.equal(game.stops, 0); assert.equal(game.frees, 0);
    if (finalPrefix) session.frames.push({ kind: 1, id: 18446744073709551614n, bytes: new Uint8Array(11) });
  };
  session.onWritten = id => { if (id === 18446744073709551614n) session.finalWritten = true; };
  await h.send(step({ watermark: event.targetHostNs }));
  assert.deepEqual(session.publications.map(row => row.finalPrefix), [false]);
  h.setNetworkNow(1001); complete = true;
  await h.send(directObservation({ presentedNs: 1000000n, presentedHostNs: h.windowNowNs() }));
  assert.equal(session.publications.length, 1, "a requested poll is not output evidence");
  await h.roomPort.acknowledge({ report: renderReport() });
  assert.equal(h.of("play-render-done").at(-1).completed, true);
  assert.deepEqual(session.publications.map(row => row.finalPrefix), [false, true]);
  assert.equal(game.groupProgressReads, 2); assert.equal(channel.activeWrites, 1);
  assert.equal(session.finalWritten, false); assert.equal(session.finalAcknowledged, false);
  const posted = h.messages.length;
  await roomReceive(h, () => {}); // An admitted read alone supplies no write or aggregate-ACK receipt.
  assert.equal(session.finalWritten, false); assert.equal(session.finalAcknowledged, false);
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  assert.equal(session.finalWritten, true); assert.equal(session.finalAcknowledged, false);
  await roomReceive(h, () => { session.finalAcknowledged = true; session.progressComplete = true; });
  assert.equal(h.messages.length, posted, "receipt changes are retained without periodic Window messages");
  assert.equal(channel.closes, 0); assert.equal(game.stops, 0);
  assert.equal(session.requests.includes("leave"), false, "local completion is not whole-room closure authority");
  session.onRequest = kind => {
    assert.equal(kind, "drain"); session.frames.push({ kind: 1, id: 18446744073709551615n, bytes: new Uint8Array(11) });
  };
  await h.send({ kind: "play-stop", playId: 7, completed: true });
  assert.equal(game.frees, 1); assert.equal(h.of("play-stopped").length, 0);
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  await roomReceive(h, () => { session.drainComplete = true; });
  const final = h.of("play-stopped").at(-1);
  assert.deepEqual(final.room, { participant: 18446744073709551615n, finalQueued: true,
    finalWritten: true, finalAcknowledged: true, localComplete: true, finalDrain: "complete", error: null, peers: [] });
  assert.equal(session.publications.length, 2, "Stop cannot publish the already accepted final twice");
  assert.ok(game.disposals.indexOf("group-progress") < game.disposals.indexOf("stop"));
  assert.equal(final.replays[0].replayComplete, true); assert.equal(session.frees, 1);
});

test("explicit Stop captures the actual room prefix before freeing gameplay and reports queued-only cancellation after joined cleanup", async () => {
  const h = await roomPrepared({ roomHoldAfterClose: true });
  const game = h.locals[0], event = await committedRoom(h);
  await h.rpc("play-activate", { hostNs: event.targetHostNs, targetHostNs: event.targetHostNs, startFrame: START });
  const session = h.roomSessions[0], channel = h.roomChannels[0];
  session.onPublish = (_, finalPrefix) => {
    assert.equal(finalPrefix, true); assert.equal(game.stops, 0); assert.equal(game.frees, 0);
    session.frames.push({ kind: 1, id: 33n, bytes: new Uint8Array(11) });
  };
  const credits = [...session.credits], received = session.received.length;
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.groupProgressReads, 1); assert.equal(session.publications.length, 1);
  assert.equal(game.stops, 1); assert.equal(game.frees, 1); assert.equal(h.roomPort.closes, 1);
  assert.equal(session.frees, 1); assert.equal(channel.closes, 1);
  assert.equal(h.of("play-stopped").length, 0, "terminal receipt still joins the owned read continuation");
  channel.reads.at(-1).gate.resolve(Uint8Array.of(1)); await flushJobs();
  assert.deepEqual(session.credits, credits); assert.equal(session.received.length, received);
  const final = h.of("play-stopped").at(-1);
  assert.deepEqual(final.room, { participant: 18446744073709551615n, finalQueued: true,
    finalWritten: false, finalAcknowledged: false, localComplete: false, finalDrain: "cancelled", error: null, peers: [] });
  assert.equal(final.replays[0].replayComplete, false);
  assert.ok(final.replays[0].replay instanceof Uint8Array);
  assert.deepEqual(game.disposals, ["group-progress", "stop", "take:4294967295", "free"]);
  assert.equal(session.requests.includes("leave"), false);
});

test("an activated room fault fences only publication and joins stale continuations while local input, output and captures survive", async () => {
  const h = await roomPrepared({ roomHoldAfterClose: true });
  const game = h.locals[0], event = await committedRoom(h);
  await h.rpc("play-activate", { hostNs: event.targetHostNs, targetHostNs: event.targetHostNs, startFrame: START });
  const session = h.roomSessions[0], channel = h.roomChannels[0];
  session.onPublish = () => session.frames.push({ kind: 1, id: 33n, bytes: new Uint8Array(11) });
  await h.send(step({ watermark: event.targetHostNs }));
  assert.equal(channel.activeWrites, 1);
  channel.reads.at(-1).gate.reject(new Error("actual room read failed after activation")); await flushJobs();
  assert.equal(h.of("play-room").filter(row => row.event.kind === "closed").length, 1);
  assert.equal(h.of("play-error").length, 0); assert.equal(game.stops, 0); assert.equal(game.frees, 0);
  assert.equal(h.roomPort.closes, 0); assert.equal(session.frees, 1);
  h.setNetworkNow(2000);
  const key = { hostNs: event.targetHostNs + 9n, sequence: 18446744073709551615n, key: 2, down: true };
  await h.send(step({ tickId: 2, watermark: key.hostNs, events: [key] }));
  assert.deepEqual(game.calls.find(row => row[0] === "blob")[1], encodeKeyboardEvent(key));
  const presentedHost = h.windowNowNs();
  await h.send(directObservation({ presentedNs: presentedHost - event.targetHostNs, presentedHostNs: presentedHost }));
  await h.roomPort.acknowledge({ report: renderReport() });
  assert.equal(game.processedInput.length, 0, "one actual anchor retains acquired input");
  h.setWindowNowNs(presentedHost + 1000000n);
  await h.send(directObservation({ renderId: 2, presentedNs: presentedHost - event.targetHostNs + 1000000n,
    presentedHostNs: presentedHost + 1000000n }));
  await h.roomPort.acknowledge({ report: renderReport() });
  assert.equal(game.processedInput.length, 1, "local processing survives the isolated room transport fault");
  assert.equal(h.of("play-render-done").at(-1).observedTick, 2);
  assert.equal(game.groupProgressReads, 1, "fenced transport cannot reacquire or publish further gameplay words");
  const credits = [...session.credits], oldEvents = h.of("play-room").length;
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.frees, 1); assert.equal(h.of("play-stopped").length, 0);
  await h.send(startRequest({ playId: 8, rpcId: 1 }));
  assert.equal(h.games.length, 0, "replacement waits for joined room cleanup");
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  assert.deepEqual(session.credits, credits); assert.equal(h.of("play-room").length, oldEvents);
  const final = h.of("play-stopped").at(-1);
  assert.match(final.room.error, /Room read failed/);
  assert.equal(final.room.finalQueued, false); assert.equal(final.room.finalWritten, false);
  assert.equal(final.room.finalAcknowledged, false); assert.equal(final.room.finalDrain, "cancelled");
  assert.ok(final.replays[0].replay instanceof Uint8Array); assert.equal(h.games[0].frees, 0);
  await h.send({ kind: "play-stop", playId: 8 });

  for (const badWords of [new Uint32Array(), new Uint32Array(12),
    Uint32Array.of(1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0)]) {
    const refused = await roomPrepared({ localProgressWords: () => badWords });
    const start = await committedRoom(refused);
    await refused.rpc("play-activate", { hostNs: start.targetHostNs, targetHostNs: start.targetHostNs, startFrame: START });
    await refused.send(step({ watermark: start.targetHostNs }));
    assert.deepEqual(refused.roomSessions[0].publications, [], "whole malformed local rows refuse before core publication");
    assert.equal(refused.of("play-room").filter(row => row.event.kind === "closed").length, 1);
    assert.equal(refused.locals[0].frees, 0); assert.equal(refused.of("play-error").length, 0);
    await refused.send({ kind: "play-stop", playId: 7 });
    assert.ok(refused.of("play-stopped")[0].replays[0].replay instanceof Uint8Array);
  }
});

async function naturalRoomDrain(options = {}) {
  const h = await roomPrepared({ ...options, observeOutput: () => true });
  const game = h.locals[0], event = await committedRoom(h, undefined, options.roomMembers ?? null);
  if (options.roomInitialPage !== undefined) {
    const rpc = await requestRoom(h, "play-room-page", { page: options.roomInitialPage });
    assert.equal(roomReply(h, rpc).result.page, options.roomInitialPage);
  }
  await h.rpc("play-activate", { hostNs: event.targetHostNs, targetHostNs: event.targetHostNs, startFrame: START });
  h.setWindowNowNs(event.targetHostNs);
  const session = h.roomSessions[0], channel = h.roomChannels[0];
  session.onPublish = (_, finalPrefix) => {
    assert.equal(finalPrefix, true); assert.equal(game.stops, 0); assert.equal(game.frees, 0);
    session.frames.push({ kind: 1, id: 51n, bytes: new Uint8Array(11) });
  };
  session.onWritten = id => { if (id === 51n) session.finalWritten = true; };
  session.onRequest = kind => {
    assert.equal(kind, "drain"); session.frames.push({ kind: 1, id: 52n, bytes: new Uint8Array(11) });
  };
  await h.send(directObservation({ presentedNs: 0n, presentedHostNs: (h.locals[0] ?? h.games[0]).activationHost }));
  assert.deepEqual(session.publications, []);
  await h.roomPort.acknowledge({ report: renderReport() });
  assert.equal(h.of("play-render-done").at(-1).completed, true);
  assert.equal(session.publications.length, 1);
  await h.send({ kind: "play-stop", playId: 7, completed: true });
  assert.equal(game.stops, 1); assert.equal(game.frees, 1); assert.equal(h.roomPort.closes, 1);
  assert.equal(h.of("play-stopped").length, 0);
  assert.equal(h.of("play-error").length, 0);
  return { h, game, session, channel };
}

test("natural room drain retains peer and receipt callbacks after gameplay disposal and joins before publishing complete replay", async () => {
  const { h, game, session, channel } = await naturalRoomDrain({ roomHoldAfterClose: true });
  assert.equal(game.groupProgressReads, 1); assert.deepEqual(session.requests, ["ready"]);
  const calls = game.calls.length, messages = h.messages.length;
  const hudCalls = game.roomHudCalls.length;
  const words = new Uint32Array([
    800, 0xffffffff, 0x7fffffff, 0xffffffff, 0xffffffff, 0, 0, 1, 0, 0xffffffff, 0xffffffff,
    4, 0, 0, 2, 0, 0, 0, 2, 0, 2, 0,
    0xffffffff, 0xffffffff, 0xffffffff, 0, 0, 0, 0, 0, 0, 0, 0,
  ]);
  const original = words.slice();
  await roomReceive(h, () => session.peerProgress.push({ participant: 9007199254740993n,
    sequence: 18446744073709551615n, finalPrefix: true, words }));
  words.fill(0);
  assert.equal(h.messages.length, messages, "retained drain observations do not publish periodic Window rows");
  assert.equal(session.finalWritten, false); assert.equal(channel.closes, 0);
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  await roomReceive(h, () => { session.finalAcknowledged = true; session.progressComplete = true; });
  assert.deepEqual(session.requests, ["ready", "drain"]);
  let pendingComplete = false;
  session.onWritten = id => { if (id === 52n && pendingComplete) session.drainComplete = true; };
  await roomReceive(h, () => { pendingComplete = true; });
  assert.equal(session.drainComplete, false); assert.equal(h.of("play-stopped").length, 0);
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  assert.equal(session.drainComplete, true); assert.equal(channel.closes, 1); assert.equal(session.frees, 1);
  assert.equal(h.of("play-stopped").length, 0, "the already acquired read still belongs to joined cleanup");
  const credited = [...session.credits], received = session.received.length;
  channel.reads.at(-1).gate.resolve(Uint8Array.of(17)); await flushJobs();
  const final = h.of("play-stopped").at(-1);
  assert.equal(final.room.finalDrain, "complete"); assert.equal(final.room.error, null);
  assert.equal(final.room.finalQueued, true); assert.equal(final.room.finalWritten, true);
  assert.equal(final.room.finalAcknowledged, true); assert.equal(final.room.localComplete, true);
  assert.deepEqual(final.room.peers, [{ participant: 9007199254740993n,
    sequence: 18446744073709551615n, finalPrefix: true, words: original }]);
  assert.equal(final.replays[0].replayComplete, true); assert.ok(final.replays[0].replay instanceof Uint8Array);
  assert.deepEqual(game.disposals, ["group-progress", "stop", "take:4294967295", "free"]);
  assert.equal(game.calls.length, calls); assert.equal(session.publications.length, 1);
  assert.equal(game.roomHudCalls.length, hudCalls, "retained final-drain prefixes never revisit the freed HUD binding");
  assert.deepEqual(session.credits, credited); assert.equal(session.received.length, received);
  assert.equal(session.requests.includes("leave"), false);
  assert.equal(h.of("play-stopped").length, 1); assert.equal(h.of("play-error").length, 0);
});

async function completeRoomDrain({ h, session, channel }) {
  channel.writes.at(-1).gate.resolve(); await flushJobs();
  await roomReceive(h, () => { session.finalAcknowledged = true; session.progressComplete = true; });
  let receivedComplete = false;
  session.onWritten = id => { if (id === 52n && receivedComplete) session.drainComplete = true; };
  await roomReceive(h, () => { receivedComplete = true; });
  assert.equal(session.drainComplete, false);
  channel.writes.at(-1).gate.resolve(); await flushJobs();
}

test("cold combined Results retain comparison selection and a separate frozen room footer", async () => {
  const members=[
    {participant:18446744073709551615n,players:Uint32Array.of(0xffffffff),prepared:false},
    {participant:9007199254740993n,players:Uint32Array.of(800,4,0xffffffff),prepared:false},
    {participant:18446744073709551614n,players:Uint32Array.of(0xffffffff,7,9),prepared:false},
  ];
  const fixture=await naturalRoomDrain({completedResults:true,roomMembers:members,roomHudPages:2}),{h}=fixture;
  assert.equal(h.completedOwners.length,1,"actual audio completion captures Results before local ownership is freed");
  h.renderPort.blocked=true;
  await completeRoomDrain(fixture);
  const capture=h.of("play-stopped").at(-1);
  assert.equal(capture.completedResults.proof,true);assert.equal(capture.room.finalDrain,"complete");
  let precedingRoom;
  for(let index=0;index<32;index++){
    const queued=h.renderPort.deliveries.keys().next().value;
    if(queued)await h.renderPort.deliver(queued);
    else await h.tick();
    const candidate=h.renderPort.posts.findLast(row=>row.kind==="packet"&&row.mode==="room");
    if(candidate&&!h.renderPort.deliveries.size&&h.of("render-geometry").at(-1)?.mode==="room"){
      precedingRoom=candidate;break;
    }
  }
  assert.ok(precedingRoom,JSON.stringify({
    posts:h.renderPort.posts.map(row=>({kind:row.kind,mode:row.mode,generation:row.generation?.toString(),
      packetKind:row.kind==="packet"?new DataView(row.packet.buffer).getUint16(6,true):undefined})),
    geometry:h.of("render-geometry"),errors:h.messages.filter(row=>/error|fatal/.test(row.kind)),
    roomOwners:h.roomResults.length,deliveryCount:h.renderPort.deliveries.size,
  },(_,value)=>typeof value==="bigint"?value.toString():value));
  assert.equal(new DataView(precedingRoom.packet.buffer).getUint16(6,true),6);
  assert.equal(h.renderPort.presentations.get(precedingRoom.generation).owner,h.roomResults[0]);
  assert.equal((await h.rpc("play-results-present")).result.kind,"completed-results");
  const retire=h.renderPort.posts.at(-1);
  assert.equal(retire.kind,"retire");assert.equal(retire.generation,precedingRoom.generation);
  assert.equal((await h.rpc("play-results-page",{page:2,comparisons:true})).result.completedResults.page,2);
  assert.equal((await h.rpc("play-room-page",{page:1})).result.page,1);
  assert.equal(h.renderPort.posts.at(-1),retire,"latest accepted choices remain held behind the actual preceding retire ACK");
  assert.equal(h.completedOwners[0].page,2);assert.equal(h.completedOwners[0].comparisons,true);
  await h.renderPort.deliver(retire);
  const results=h.renderPort.posts.at(-1);
  assert.equal(new DataView(results.packet.buffer).getUint16(6,true),5);
  assert.equal(h.visualExports.at(-1).page,0);assert.equal(h.visualExports.at(-1).comparisons,false);
  await h.renderPort.deliver(results);
  const room=h.renderPort.posts.at(-1);
  assert.equal(room.kind,"packet");assert.equal(new DataView(room.packet.buffer).getUint16(6,true),6);
  assert.equal(room.generation,results.generation);assert.equal(room.content,results.content);
  assert.equal(h.visualExports.at(-1).page,1,"room wire already preserves its frozen initial footer choice");
  await h.renderPort.deliver(room);
  const model=h.renderPort.presentations.get(results.generation);
  assert.equal(model.page,0);assert.equal(model.comparisons,false);assert.equal(model.room.page,1);
  assert.equal(model.drawable,false);
  const completedDraws=()=>h.views[0].resultDraws.filter(row=>row.results===h.completedOwners[0]);
  assert.deepEqual(completedDraws(),[],"neither preceding standalone ROOM nor combined ROOM imports can unlock unselected completed Results");
  const restore=h.renderPort.posts.at(-1);
  assert.equal(restore.kind,"page");assert.equal(restore.page,2);assert.equal(restore.comparisons,true);
  await h.renderPort.deliver(restore);
  assert.equal(model.page,2);assert.equal(model.comparisons,true);assert.equal(model.room.page,1);
  assert.equal(model.drawable,true);
  assert.deepEqual(completedDraws(),[{results:h.completedOwners[0],page:2}],"the first completed Results draw is the latest successful comparison page");
  const geometry=h.of("render-geometry").at(-1);
  assert.equal(geometry.generation,results.generation);assert.equal(geometry.geometryVersion,restore.geometryVersion);
  assert.deepEqual([geometry.mode,geometry.page,geometry.width,geometry.height],["results",2,640,480]);
  assert.equal((await h.rpc("play-room-page",{page:0})).result.page,0);
  const footer=h.renderPort.posts.at(-1);assert.equal(footer.kind,"room-page");
  await h.renderPort.deliver(footer);
  assert.equal(model.room.page,0);assert.equal(model.page,2);assert.equal(model.comparisons,true);
  assert.equal(h.completedOwners[0].page,2);assert.equal(h.completedOwners[0].comparisons,true);
  assert.equal(h.of("play-error").length,0);assert.equal(h.of("fatal").length,0);
  assert.equal(capture.replays[0].replayComplete,true);
  await h.send({kind:"dispose"});assert.equal(h.completedOwners[0].frees,1);assert.equal(h.roomResults[0].frees,1);
});

test("joined room Results freeze the latest accepted peer prefix after gameplay disposal and retain one bounded paged archive", async () => {
  const members = [
    { participant: 9007199254740993n, players: Uint32Array.of(800, 4, 0xffffffff), prepared: false },
    { participant: 18446744073709551615n, players: Uint32Array.of(0xffffffff), prepared: false },
    { participant: 18446744073709551614n, players: Uint32Array.of(0xffffffff, 7, 9), prepared: false },
  ];
  const fixture = await naturalRoomDrain({ roomHoldAfterClose: true, roomMembers: members,
    roomHudPages: 2, roomInitialPage: 1 });
  const { h, game, session, channel } = fixture;
  const gameCalls = game.calls.length, hudCalls = game.roomHudCalls.length;
  const first = new Uint32Array([800, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0,
    4, 0, 0, 2, 0, 0, 0, 2, 0, 2, 0, 0xffffffff, 0, 0, 3, 0, 0, 0, 3, 0, 3, 0]);
  await roomReceive(h, () => session.peerProgress.push({ participant: members[0].participant,
    sequence: 1n, finalPrefix: false, words: first }));
  const latest = new Uint32Array(Array.from(members[0].players).flatMap(player => [
    player, 0xffffffff, 0x7fffffff, 0xffffffff, 0xffffffff, 0, 0, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  ]));
  const held = latest.slice();
  await roomReceive(h, () => session.peerProgress.push({ participant: members[0].participant,
    sequence: 18446744073709551615n, finalPrefix: true, words: latest }));
  latest.fill(0);
  assert.equal(h.roomResults.length, 0); assert.equal(h.of("play-stopped").length, 0);
  await completeRoomDrain(fixture);
  assert.equal(channel.closes, 1); assert.equal(session.frees, 1);
  assert.equal(h.roomResults.length, 0, "archive construction waits for the retained read continuation to join");
  channel.reads.at(-1).gate.resolve(Uint8Array.of(17)); await flushJobs();
  const terminal = h.of("play-stopped").at(-1), archive = h.roomResults[0];
  assert.deepEqual(terminal.roomResults, { page: 1, pages: 2, failed: false });
  assert.equal(terminal.room.finalDrain, "complete"); assert.equal(terminal.replays[0].replayComplete, true);
  assert.equal(archive.participant, 18446744073709551615n);
  assert.deepEqual(archive.roster, new Uint32Array(members.flatMap(member => [
    Number(member.participant & 0xffffffffn), Number(member.participant >> 32n), member.players.length, ...member.players,
  ])));
  assert.deepEqual(archive.updates, [{ participant: members[0].participant,
    sequence: 18446744073709551615n, finalPrefix: true, words: held }]);
  assert.deepEqual(archive.freezeArgs, { page: 1, cancelled: false, error: null, failed: false });
  assert.equal(game.calls.length, gameCalls); assert.equal(game.roomHudCalls.length, hudCalls);
  await h.tick();
  assert.deepEqual(h.views[0].resultDraws.at(-1), { results: archive, page: 1 });
  for (const page of [0, 1, 0]) {
    const rpc = await requestRoom(h, "play-room-page", { page });
    assert.deepEqual(roomReply(h, rpc).result, { kind: "room-page", page, pages: 2 });
    await h.tick();
    assert.deepEqual(h.views[0].resultDraws.at(-1), { results: archive, page });
    assert.equal(h.roomResults.length, 1); assert.equal(archive.updates.length, 1);
  }
  const selections = [...archive.selections], draws = h.views[0].resultDraws.length;
  for (const page of [-1, 2, 0.5, "1"]) {
    const rpc = await requestRoom(h, "play-room-page", { page }); assert.ok(roomReply(h, rpc).error);
  }
  const lastRpc = h.rpcId;
  await h.send({ kind: "play-room-page", playId: 7, rpcId: lastRpc, page: 1 });
  assert.ok(h.of("play-reply").at(-1).error, "reusing an old Results RPC does not adopt another page");
  await h.send({ kind: "play-room-page", playId: 6, rpcId: lastRpc + 1, page: 1 });
  assert.deepEqual(archive.selections, selections); assert.equal(archive.page, 0);
  assert.equal(h.views[0].resultDraws.length, draws); assert.equal(archive.frees, 0);
  assert.equal(h.of("play-room-results").length, 0); assert.equal(h.of("play-error").length, 0);
  await h.send(startRequest({ playId: 8, rpcId: 1 }));
  assert.equal(archive.frees, 1); assert.equal(h.games.length, 1);
  const messages = h.messages.length;
  await h.send({ kind: "play-room-page", playId: 7, rpcId: lastRpc + 2, page: 1 });
  assert.equal(h.messages.length, messages); assert.equal(h.games[0].frees, 0);
  await h.send({ kind: "play-stop", playId: 8 });
  assert.equal(archive.frees, 1);
});

test("retained room archive and rendering faults remain presentation-only and every discard fences one joined binding", async () => {
  for (const options of [
    { missingRoomResultsExport: true }, { roomResultsConstructError: "archive allocation refused" },
    { roomResultsFreezeError: "freeze refused" }, { roomResultsUpdateError: "retained prefix refused" },
    { roomResultsDrawError: "renderer unavailable" }, { roomResultsPageError: "page projection refused" },
  ]) {
    const fixture = await naturalRoomDrain(options), { h, session } = fixture;
    await roomReceive(h, () => session.peerProgress.push({ participant: 9007199254740993n,
      sequence: 1n, finalPrefix: false, words: new Uint32Array([
        800, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0xffffffff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      ]) }));
    await completeRoomDrain(fixture);
    const terminal = h.of("play-stopped").at(-1);
    assert.equal(h.locals[0].frees, 1); assert.equal(session.frees, 1); assert.equal(terminal.replays[0].replayComplete, true);
    assert.equal(terminal.room.finalDrain, "complete"); assert.equal(terminal.room.error, null);
    const replay = terminal.replays[0].replay.slice();
    if (options.roomResultsDrawError) await h.tick();
    if (options.roomResultsPageError) {
      const rpc = await requestRoom(h, "play-room-page", { page: 0 }); assert.ok(roomReply(h, rpc).error);
    }
    if (options.roomResultsDrawError || options.roomResultsPageError) {
      const notice = h.of("play-room-results").at(-1);
      assert.deepEqual({ playId: notice.playId, page: notice.page, pages: notice.pages, failed: notice.failed },
        { playId: 7, page: 0, pages: 1, failed: true });
    } else assert.equal(terminal.roomResults.failed, true);
    assert.ok(h.roomResults.every(archive => archive.frees === 1));
    assert.equal(h.of("fatal").length, 0); assert.equal(h.of("play-error").length, 0);
    assert.deepEqual(terminal.replays[0].replay, replay); assert.equal(h.locals[0].frees, 1);
    await h.send({ kind: "seek", id: 9, selectedId: 2, ns: "0" });
    assert.ok(h.roomResults.every(archive => archive.frees === 1));
  }
  {
    const h = await roomPrepared(), event = await committedRoom(h);
    await h.rpc("play-activate", { hostNs: event.targetHostNs, targetHostNs: event.targetHostNs, startFrame: START });
    const session = h.roomSessions[0];
    await roomReceive(h, () => session.peerProgress.push({ participant: 9007199254740993n,
      sequence: 1n, finalPrefix: false, words: new Uint32Array([
        800, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0xffffffff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      ]) }));
    await h.send(step({ watermark: "invalid original time" }));
    const failure = h.of("play-error").at(-1);
    assert.equal(failure.released, true); assert.equal(failure.replays[0].replayComplete, false);
    assert.deepEqual(failure.roomResults, { page: 0, pages: 1, failed: false });
    assert.equal(h.roomResults[0].freezeArgs.cancelled, true);
    assert.equal(h.roomResults[0].updates[0].finalPrefix, false);
    assert.equal(h.locals[0].frees, 1); assert.equal(session.frees, 1);
    const rpc = await requestRoom(h, "play-room-page", { page: 0 }); assert.ok(roomReply(h, rpc).result);
    await h.send({ kind: "seek", id: 10, selectedId: 2, ns: "0" });
    assert.equal(h.roomResults[0].frees, 1);
  }
  for (const discard of ["select", "seek", "import", "fatal", "replacement-during-join"]) {
    const fixture = await naturalRoomDrain({ roomHoldAfterClose: true });
    const { h, channel } = fixture;
    if (discard === "replacement-during-join") {
      await h.send(startRequest({ playId: 8, rpcId: 1 }));
      assert.equal(h.games.length, 0); assert.equal(h.roomResults.length, 0);
      for (const write of channel.writes) write.gate.resolve();
      channel.reads.at(-1).gate.resolve(Uint8Array.of(1)); await flushJobs();
      assert.equal(h.roomResults.length, 0, "obsolete cleanup cannot install an archive over the replacement");
      assert.equal(h.games.length, 1); await h.send({ kind: "play-stop", playId: 8 });
      continue;
    }
    await completeRoomDrain(fixture);
    channel.reads.at(-1).gate.resolve(Uint8Array.of(1)); await flushJobs();
    const archive = h.roomResults[0]; assert.equal(archive.frees, 0);
    if (discard === "select") await h.send({ kind: "select", id: 8, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" });
    if (discard === "seek") await h.send({ kind: "seek", id: 8, selectedId: 2, ns: "0" });
    if (discard === "import") await h.send({ kind: "import", id: 8, files: [selectedFile("other/chart.bms")] });
    if (discard === "fatal") await h.send({ kind: "resize", width: -1, height: 720 });
    assert.equal(archive.frees, 1);
    const selections = archive.selections.length;
    await h.send({ kind: "play-room-page", playId: 7, rpcId: ++h.rpcId, page: 0 });
    assert.equal(archive.selections.length, selections); assert.equal(archive.frees, 1);
  }
});

test("room drain failure preserves genuine local completion while actual gameplay cleanup failure remains separate", async () => {
  for (const mode of ["timeout", "transport", "cleanup"]) {
    const { h, game, session, channel } = await naturalRoomDrain(mode === "cleanup" ? { roomCloseError: "actual channel cleanup failed" } : {});
    if (mode !== "transport") { h.setNetworkNow(11000); await h.runTimer(1); }
    else { channel.writes.at(-1).gate.reject(new Error("final room write failed")); await flushJobs(); }
    const final = h.of(mode === "cleanup" ? "play-error" : "play-stopped").at(-1);
    assert.ok(final); assert.equal(final.room.finalDrain, "failed");
    assert.match(final.room.error, mode === "transport" ? /Room write failed/ : /Room drain failed/);
    if (mode !== "transport") {
      assert.equal(session.drainTimeout, 10000000000n);
      assert.equal(session.drainSteps.at(-1) - session.drainStarted, 10000000000n,
        "the ten-second deadline starts when actual drain begins, after waiting for the armed target");
    }
    assert.equal(final.room.finalQueued, true); assert.equal(final.room.finalWritten, false);
    assert.equal(final.room.finalAcknowledged, false); assert.equal(final.room.localComplete, false);
    assert.equal(final.replays[0].replayComplete, mode !== "cleanup"); assert.ok(final.replays[0].replay instanceof Uint8Array);
    assert.equal(h.of("play-error").length, mode === "cleanup" ? 1 : 0); assert.equal(game.frees, 1); assert.equal(session.frees, 1);
    if (mode === "cleanup") { assert.equal(final.released, false); assert.match(final.message, /actual channel cleanup failed/); }
    assert.equal(channel.closes, 1); assert.equal(h.roomPort.closes, 1);
  }
  const h = await roomPrepared({ observeOutput: () => true, freeError: "actual gameplay free failed" });
  const event = await committedRoom(h);
  await h.rpc("play-activate", { hostNs: event.targetHostNs, targetHostNs: event.targetHostNs, startFrame: START });
  h.setWindowNowNs(event.targetHostNs);
  const session = h.roomSessions[0], channel = h.roomChannels[0];
  session.onPublish = () => session.frames.push({ kind: 1, id: 51n, bytes: new Uint8Array(11) });
  await h.send(directObservation({ presentedNs: 0n, presentedHostNs: (h.locals[0] ?? h.games[0]).activationHost })); await h.roomPort.acknowledge({ report: renderReport() });
  await h.send({ kind: "play-stop", playId: 7, completed: true });
  if (h.of("play-error").length === 0) { h.setNetworkNow(11000); await h.runTimer(1); }
  const failed = h.of("play-error").at(-1);
  assert.match(failed.message, /actual gameplay free failed/); assert.equal(failed.released, false);
  assert.equal(failed.replays[0].replayComplete, false); assert.ok(failed.replays[0].replay instanceof Uint8Array);
  assert.equal(h.of("play-stopped").length, 0); assert.equal(channel.closes, 1); assert.equal(session.frees, 1);
});

test("explicit cancellation and replacement fence retained room drains without late writes into newer gameplay", async () => {
  for (const mode of ["stop", "replacement", "replacement-cleanup", "fatal", "leave"]) {
    const replacing = mode.startsWith("replacement"), cleanupFailure = mode === "replacement-cleanup";
    const { h, game, session, channel } = await naturalRoomDrain({ roomHoldAfterClose: true,
      ...(cleanupFailure ? { roomCloseError: "old room cleanup failed" } : {}) });
    const received = session.received.length, credits = [...session.credits], roomEvents = h.of("play-room").length;
    for (const completed of [null, "true", 1, true]) {
      await h.send({ kind: "play-stop", playId: 7, completed });
      assert.equal(channel.closes, 0); assert.equal(session.frees, 0);
      assert.equal(h.of("play-stopped").length, 0);
      assert.deepEqual(session.credits, credits); assert.equal(session.received.length, received);
    }
    let leaveRpc;
    if (replacing) h.post(startRequest({ playId: 8, rpcId: 1 }));
    else if (mode === "fatal") h.post(null);
    else if (mode === "leave") { leaveRpc = ++h.rpcId; h.post({ kind: "play-room-leave", playId: 7, rpcId: leaveRpc }); }
    else h.post({ kind: "play-stop", playId: 7 });
    await flushJobs();
    assert.equal(game.frees, 1); assert.equal(channel.closes, 1); assert.equal(session.frees, 1);
    if (mode === "leave") {
      assert.match(roomReply(h, leaveRpc).error, /drain cancelled.*gameplay already stopped/i);
      assert.equal(roomReply(h, leaveRpc).result, undefined);
      assert.equal(session.requests.includes("leave"), false);
    }
    assert.equal(h.of("play-stopped").length, 0);
    if (replacing) assert.equal(h.games.length, 0, "replacement joins the old retained transport first");
    session.peerProgress.push({ participant: 9007199254740993n, sequence: 1n, finalPrefix: true, words: new Uint32Array(33) });
    channel.writes.at(-1).gate.resolve(); await flushJobs();
    assert.equal(h.of("play-stopped").length, 0, "the retained read has not joined yet");
    channel.reads.at(-1).gate.resolve(Uint8Array.of(17)); await flushJobs();
    const old = h.of(cleanupFailure ? "play-error" : "play-stopped").find(row => row.playId === 7);
    assert.ok(old); assert.equal(old.room.finalDrain, "cancelled");
    assert.equal(old.room.finalWritten, false); assert.equal(old.room.finalAcknowledged, false);
    assert.deepEqual(old.room.peers, []); assert.equal(old.replays[0].replayComplete, !cleanupFailure);
    if (cleanupFailure) { assert.equal(old.released, false); assert.match(old.message, /old room cleanup failed/); }
    assert.deepEqual(session.credits, credits); assert.equal(session.received.length, received);
    assert.equal(h.of("play-room").length, roomEvents); assert.equal(session.publications.length, 1);
    if (cleanupFailure) {
      assert.equal(h.games.length, 0, "failed old cleanup cannot construct a replacement game");
      assert.equal(h.of("play-reply").some(row => row.playId === 8 && row.result?.kind === "prepared"), false);
      assert.match(h.of("fatal").at(-1).message, /Previous room cleanup failed/);
    } else if (replacing) {
      assert.equal(h.games.length, 1); assert.equal(h.games[0].frees, 0);
      assert.equal(h.of("play-reply").filter(row => row.playId === 8 && row.result?.kind === "prepared").length, 1);
      await h.send({ kind: "play-stop", playId: 8 });
    }
  }
});

test("local saved records admit per member before touch setup and isolate one failed HUD through final capture", async () => {
  const files = [replayFile(), replayFile(), replayFile()];
  const opponents = files.map((selected, index) => ({ file: selected.file, sourceKey: `file:local-${index}`,
    player: [99, 31, 99][index], own: index !== 1, label: ["first own", "touch other", "second own"][index] }));
  const maximum = 18446744073709551615n;
  const h = await active({ allowNetworkClock: true, startRequest: localRequest({ opponents, recordReplay: true }),
    localSavedSnapshot(game, rows) {
      rows[0].opponents[0] = { ...rows[0].opponents[0], songNs: -9223372036854775808n,
        hits: maximum, misses: maximum, combo: maximum, maxCombo: maximum };
      if (game.savedReads === 2) rows[2] = { player: 31, opponents: null, error: "actual member comparison failed" };
      return rows;
    } });
  const game = h.locals[0], admitted = game.calls.filter(call => call[0] === "local-add-opponent");
  assert.equal(h.of("play-reply")[0].result.opponentCount, 3);
  assert.deepEqual(admitted.map(call => [call[1], call[3], call[4]]), [[99, true, "first own"], [31, false, "touch other"], [99, true, "second own"]]);
  assert.deepEqual(admitted.map(call => Array.from(call[2])), files.map(file => Array.from(file.bytes)));
  assert.deepEqual(files.map(file => file.reads), [1, 1, 1]);
  assert.equal(game.memberSaved.get(99).length, 2); assert.equal(game.memberSaved.get(7).length, 0);
  assert.ok(game.calls.findLastIndex(call => call[0] === "local-add-opponent") < game.calls.findIndex(call => call[0] === "local-touch-bounds"),
    "actual admitted comparisons reserve touch geometry before the binding is queried");
  assert.ok(game.calls.findIndex(call => call[0] === "local-touch") < game.calls.findIndex(call => call[0] === "member-capture"));
  await h.send(step());
  h.setNetworkNow(1249); await h.send(step({ tickId: 2, watermark: ORIGIN + 1n }));
  assert.equal(game.savedReads, 1); assert.equal(h.of("play-opponents").length, 0);
  h.setNetworkNow(1250); await h.send(step({ tickId: 3, watermark: ORIGIN + 2n }));
  assert.deepEqual(h.of("play-opponents").map(row => [row.player, row.opponents, row.error]), [[31, null, "actual member comparison failed"]]);
  assert.deepEqual([...game.memberHudDisables], [[31, 1]]);
  assert.equal(h.of("play-error").length, 0); assert.equal(game.stops, 0);
  await h.send({ kind: "play-stop", playId: 7 });
  const stopped = h.of("play-stopped").at(-1), groups = stopped.savedOpponents.localOpponents;
  assert.equal(stopped.savedOpponents.opponents, null); assert.equal(stopped.savedOpponents.error, null);
  assert.deepEqual(groups.map(row => row.player), [99, 7, 31]);
  assert.equal(groups[0].opponents[0].songNs, -9223372036854775808n);
  assert.equal(groups[0].opponents[0].hits, maximum); assert.equal(groups[0].opponents[1].label, "second own");
  assert.equal(groups[1].opponents.length, 0); assert.equal(groups[1].error, null);
  assert.equal(groups[2].opponents, null); assert.equal(groups[2].error, "actual member comparison failed");
  assert.equal(game.savedReads, 3); assert.deepEqual([...game.memberHudDisables], [[31, 1]]);
  assert.deepEqual(game.disposals.slice(-6), ["opponents", "stop", "take:99", "take:7", "take:31", "free"]);
  assert.ok(stopped.replays.every(row => row.replay instanceof Uint8Array && row.replayError === null && row.replayComplete === false));
  assert.equal(stopped.localScores.length, 3); assert.equal(game.stops, 1); assert.equal(game.frees, 1);
  const reads = game.savedReads;
  await h.send(step({ tickId: 4 })); await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.savedReads, reads); assert.equal(h.of("play-opponents").length, 1);
});

test("local saved setup refuses wrong targets and binding failures without fallback or late file-read resurrection", async () => {
  const entry = (file, player) => ({ file, sourceKey: "file:target", own: true, label: "chosen", player });
  for (const request of [localRequest({ opponents: [entry(replayFile().file, undefined)] }),
    localRequest({ opponents: [entry(replayFile().file, 123)] }),
    startRequest({ opponents: [entry(replayFile().file, 99)] })]) {
    const h = await catalogWorker(); const previews = h.preparedOwners.slice(); await h.send(request);
    assert.ok(h.of("play-reply").at(-1).error); assert.equal(h.locals.length, 0); assert.equal(h.games.length, 0);
    assert.deepEqual(h.preparedOwners, previews, "invalid replay target never prepares gameplay beyond the existing preview");
  }
  const missing = await catalogWorker({ missingLocalSavedHud: true });
  await missing.send(localRequest({ opponents: [entry(replayFile().file, 99)] }));
  assert.match(missing.of("play-reply").at(-1).error, /saved comparison/);
  assert.equal(missing.localConstructions.length, 0); assert.equal(missing.preparedOwners.at(-1).frees, 1);
  for (const options of [{ localAddSavedError: player => player === 31 },
    { localSavedIndex: (player, index) => player === 31 ? index + 1 : index }]) {
    const h = await catalogWorker(options), first = replayFile(), refused = replayFile();
    await h.send(localRequest({ recordReplay: true, opponents: [entry(first.file, 99),
      { ...entry(refused.file, 31), sourceKey: "file:second" }] }));
    const game = h.locals[0]; assert.equal(game.stops, 1); assert.equal(game.frees, 1);
    assert.equal(game.calls.some(call => call[0] === "member-capture" || call[0] === "local-touch" || call[0] === "sample"), false);
    assert.equal(game.memberSaved.get(99).length, 1);
    assert.equal(h.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
    assert.match(h.of("play-error").at(-1).message, /member replay admission|admission count/);
  }
  const waiting = deferred(), delayed = replayFile(() => waiting.promise);
  const h = await catalogWorker(); h.post(localRequest({ opponents: [entry(delayed.file, 31)] })); await flushJobs();
  const game = h.locals[0]; assert.equal(delayed.reads, 1);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.stops, 1); assert.equal(game.frees, 1);
  const previous = h.messages.length;
  waiting.resolve(delayed.bytes.slice().buffer); await flushJobs();
  assert.equal(game.calls.some(call => call[0] === "local-add-opponent"), false);
  assert.equal(h.messages.length, previous);
  await h.send(startRequest({ playId: 8 }));
  assert.equal(h.of("play-reply").at(-1).result.opponentCount, 0);
  await h.send({ kind: "play-stop", playId: 8 });
});

test("local players snapshot exact sources and share one PCM, command and report owner while retaining every member score", async () => {
  const high = 9007199254740993n;
  const roster = [[99, HID_SOURCE], [7, 1n], [31, 2n], [0xffffffff, high]];
  const words = localPlan(roster), savedWords = Array.from(words);
  const keys = pairs(), hid = hidSetup([HID_SOURCE, high]);
  const timing = { earlyNs: 1n, lateNs: 2n, offsetNs: -3n };
  const port = commandPort(), initial = batch(18446744073709551614n);
  const following = { sequence: 18446744073709551615n, commands: [command(21n), command(22n)] };
  let complete = false;
  const h = await catalogWorker({ batches: [initial], gameEnd: 1n, gameEndFrame: 4801n,
    observeOutput: () => complete,
    localScore(player, index) { return index === 0
      ? { song_ns: -9223372036854775808n, hits: 18446744073709551615n, misses: 0n, combo: 0n, max_combo: 18446744073709551615n }
      : { ...SCORE, hits: BigInt(player) }; },
    beforeFree() { assert.equal(port.closes, 1, "one direct client closes before the shared local owner"); },
  });
  h.post(localRequest({ localPlanWords: words, keyPairs: keys, hidSetup: hid, timing, endNs: 1n, recordReplay: true }));
  words.fill(0); keys.fill(0); hid.bindingWords.fill(0); hid.deviceWords.fill(0); timing.offsetNs = 999n;
  await flushJobs();
  const prepared = h.of("play-reply").at(-1).result, game = h.locals[0];
  assert.deepEqual(prepared.localPlayers, roster.map(([player]) => player));
  assert.equal(prepared.localPage, 0); assert.equal(prepared.samples, 2);
  assert.equal(prepared.endNs, 1n); assert.equal(prepared.endFrame, 4801n);
  assert.deepEqual(prepared.recordLimits, { bytes: 16777216, records: 250000 });
  assert.equal(h.games.length, 0); assert.equal(h.physicalConstructions.length, 0);
  assert.equal(h.localConstructions.length, 1); assert.equal(game.prepared.moved, true);
  const args = h.localConstructions[0].args;
  assert.deepEqual(args.slice(0, 5), [0n, 100000000n, 1n, 2n, -3n]);
  assert.deepEqual(Array.from(args[5]), savedWords);
  assert.deepEqual(args.slice(7), [1n, true, 4096, 1024]);
  const bindings = Array.from({ length: args[6].length / 8 }, (_, index) => Array.from(args[6].slice(index * 8, index * 8 + 8)));
  assert.equal(bindings.length, 8);
  for (const [player, source] of roster) {
    const rows = bindings.filter(row => row[0] === player);
    assert.deepEqual(rows.map(row => row[1]), [0x11, 0x12]);
    assert.ok(rows.every(row => row[2] === 1 && (BigInt(row[3]) | BigInt(row[4]) << 32n) === source));
  }
  assert.deepEqual(game.calls.filter(row => row[0] === "local-touch-bounds"), [["local-touch-bounds", 31, 0]]);
  assert.deepEqual(game.calls.filter(row => row[0] === "member-capture"), roster.map(([player]) => ["member-capture", player, 16777216, 250000]));
  assert.ok(game.calls.findIndex(row => row[0] === "local-touch") < game.calls.findIndex(row => row[0] === "member-capture"));
  h.rpcId = 1;
  h.rpc = async (kind, fields = {}) => {
    const rpcId = ++h.rpcId; await h.send({ kind, playId: 7, rpcId, ...fields });
    return h.of("play-reply").find(value => value.rpcId === rpcId);
  };
  const audioRpc = await attachCommands(h, port);
  assert.equal(game.samples.every(sample => sample.takes === 1 && sample.frees === 1), true);
  assert.equal(h.of("play-reply").some(value => value.rpcId === audioRpc), false);
  assert.deepEqual(port.posts.map(value => [value.kind, value.sequence]), [["commands", 1]]);
  await port.acknowledge();
  assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [["ack", initial.sequence, 2, true]]);
  assert.equal(h.of("play-reply").find(value => value.rpcId === audioRpc).result.kind, "audio-ready");
  assert.equal((await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START })).result, null);
  game.batches.push(following);
  await h.send(step({ events: [
    { hostNs: ORIGIN, key: 2, down: true, sequence: 1n },
    touchEvent({ sequence: 2n }), hidEvent({ sequence: 3n }),
  ] }));
  const calls = game.calls.filter(row => ["blob", "touch", "hid"].includes(row[0]));
  assert.deepEqual(calls.map(row => row[0]), ["blob", "touch", "hid"]);
  assert.deepEqual(calls.map(row => new DataView(row[1].buffer).getBigUint64(7, true)), [1n, 2n, HID_SOURCE]);
  assert.deepEqual(calls.map(row => new DataView(row[1].buffer).getBigInt64(15, true)), [ORIGIN, ORIGIN, ORIGIN]);
  assert.equal(game.calls.some(row => row[0] === "input"), false);
  const stepped = h.of("play-step-done").at(-1);
  assert.equal(stepped.primaryPlayer, 99); assert.equal(stepped.hits, 18446744073709551615n);
  assert.equal(stepped.songNs, -9223372036854775808n);
  assert.deepEqual(stepped.localScores.map(row => [row.player, row.hits]), [[99, 18446744073709551615n], [7, 7n], [31, 31n], [0xffffffff, 4294967295n]]);
  assert.equal(stepped.commandsPending, true);
  await h.send({ kind: "play-render", playId: 7, renderId: 1, presentedNs: 0n, presentedHostNs: ORIGIN });
  assert.equal(port.posts.at(-1).kind, "commands"); assert.equal(h.of("play-render-done").length, 0);
  await port.acknowledge();
  assert.equal(port.posts.at(-1).kind, "poll"); assert.equal(port.posts.at(-1).sequence, 3);
  complete = true;
  await port.acknowledge({ report: renderReport() });
  assert.equal(h.of("play-render-done").at(-1).completed, false);
  assert.equal(h.of("play-render-done").at(-1).pendingInputs, 3);
  await h.send({ kind: "play-render", playId: 7, renderId: 2, presentedNs: 1n, presentedHostNs: ORIGIN + 1n });
  await port.acknowledge({ report: renderReport() });
  const rendered = h.of("play-render-done").at(-1);
  assert.equal(rendered.completed, true); assert.equal(rendered.commandsPending, false); assert.equal(rendered.observedTick, 1);
  assert.deepEqual(rendered.localScores, stepped.localScores);
  await h.tick();
  assert.equal(h.views[0].localDraws.at(-1).game, game); assert.equal(h.views[0].localDraws.at(-1).page, 0);
  assert.equal(h.views[0].gameDraws.length, 0); assert.equal(h.views[0].replayDraws.length, 0);
  await h.send({ kind: "play-stop", playId: 7, completed: true });
  const stopped = h.of("play-stopped").at(-1);
  assert.equal(stopped.replay, null); assert.equal(stopped.replayComplete, false);
  assert.deepEqual(stopped.replays.map(row => [row.player, row.replayComplete]), roster.map(([player]) => [player, true]));
  assert.equal(game.stops, 1); assert.equal(game.frees, 1); assert.equal(port.closes, 1);
  assert.equal(h.of("play-commands").length, 0);
});

test("local capability and coverage refusals preserve prepared ownership while ordinary and touch page changes remain recoverable", async () => {
  for (const request of [
    localRequest({ inputMode: undefined }), replayRequest(replayFile().file, { localPlanWords: localPlan([[7, null]]) }),
    localRequest({ multiplayer: { ...multiplayer(), peerTargets: new Uint32Array([7, 0]) } }),
    localRequest({ opponents: [{ file: replayFile().file, sourceKey: "file:local", own: false, label: "prior" }] }),
    localRequest({ localPage: 1 }), localRequest({ localPlanWords: localPlan([[7, 3n], [8, 3n]]) }),
  ]) {
    const h = await catalogWorker(); const preparations = h.preparedOwners.length;
    await h.send(request);
    assert.equal(h.preparedOwners.length, preparations); assert.equal(h.locals.length, 0);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const options of [{ missingLocalExport: true }, { missingLocalConstructor: true }, { missingLocalInputBlob: true }, { missingLocalTouchSurfacePage: true }]) {
    const h = await catalogWorker(options); await h.send(localRequest());
    assert.equal(h.preparedOwners.at(-1).moved, false); assert.equal(h.preparedOwners.at(-1).frees, 1);
    assert.equal(h.locals.length, 0); assert.equal(h.games.length, 0); assert.equal(h.of("play-error").length, 1);
  }
  const uncovered = await catalogWorker();
  await uncovered.send(localRequest({ inputMode: "physical", localPlanWords: localPlan([[7, 1n], [8, 4n]]), hidSetup: undefined }));
  assert.equal(uncovered.preparedOwners.at(-1).frees, 1); assert.equal(uncovered.localConstructions.length, 0);
  for (const options of [{ localConstructError: "actual local consuming refusal" },
    { localPlayers: new Uint32Array([7, 99, 31]) }, { localTouchSetupError: "actual local touch refusal" }]) {
    const h = await catalogWorker(options); await h.send(localRequest());
    assert.equal(h.preparedOwners.at(-1).moved, true); assert.equal(h.preparedOwners.at(-1).frees, 0);
    assert.equal(h.of("play-error").length, 1); assert.equal(h.of("play-reply").some(value => value.result?.kind === "prepared"), false);
    assert.ok(h.locals.every(game => game.stops === 1 && game.frees === 1)); assert.equal(h.games.length, 0);
  }
  const sources = [3n, 4n, 5n, 6n, HID_SOURCE], players = [91, 2, 88, 7, 0xffffffff];
  for (const contact of [false, true]) {
    const roster = players.map((player, index) => [player, contact && index === 0 ? 2n : sources[index]]);
    const h = await started({ startRequest: localRequest({ inputMode: contact ? "physical-contact" : "physical",
      localPlanWords: localPlan(roster), keyPairs: new Uint32Array(), hidSetup: hidSetup(contact ? sources.slice(1) : sources) }) });
    const game = h.locals[0];
    assert.equal((await h.rpc("play-page", { page: 2 })).error.includes("page"), true);
    assert.equal(h.of("play-error").length, 0); assert.equal(game.frees, 0);
    const changed = await h.rpc("play-page", { page: 1 });
    assert.deepEqual(changed.result, { kind: "local-page", page: 1, ...(contact ? { touchVisible: false } : {}) });
    await h.tick(); assert.equal(h.views[0].localDraws.at(-1).page, 1);
    assert.deepEqual((await h.rpc("play-page", { page: 0 })).result, { kind: "local-page", page: 0, ...(contact ? { touchVisible: true } : {}) });
    assert.equal((await h.rpc("play-sample")).result.kind, "sample");
    assert.equal(game.frees, 0); assert.equal(h.of("play-error").length, 0);
    await h.send({ kind: "play-stop", playId: 7 });
    assert.ok(h.of("play-stopped").at(-1).replays.every(row => row.replay === null && row.replayComplete === false));
  }
});

test("local touch page RPC retains the input owner and rejects failed or contradictory remap receipts without resetting contacts", async () => {
  const request = () => localRequest({ keyPairs: new Uint32Array(),
    localPlanWords: localPlan([[91, 2n], [2, 3n], [88, 4n], [7, 5n], [0xffffffff, 6n]]),
    hidSetup: hidSetup([3n, 4n, 5n, 6n]), recordReplay: true });
  let refused = false;
  const h = await active({ startRequest: request(), localTouchPageError: () => refused });
  const game = h.locals[0], first = touchEvent({ contact: 0xffffffffffffffffn });
  await h.send(step({ events: [first] }));
  await legacyWitness(h, 1, 100n, ORIGIN);
  await legacyWitness(h, 2, 101n, ORIGIN + 1n);
  assert.deepEqual((await h.rpc("play-page", { page: 1 })).result, { kind: "local-page", page: 1, touchVisible: false });
  await h.tick(); assert.equal(h.views[0].localDraws.at(-1).page, 1);
  const hidden = touchEvent({ sequence: 2n, contact: 77n, hostNs: ORIGIN + 1n, page: 1 });
  const release = touchEvent({ sequence: 3n, phase: 2, contact: first.contact, hostNs: ORIGIN + 2n, x: -50, page: 1 });
  await h.send(step({ tickId: 2, watermark: ORIGIN + 2n, events: [hidden, release] }));
  await legacyWitness(h, 3, 102n, ORIGIN + 2n);
  const inputs = game.calls.filter(call => call[0] === "touch");
  assert.deepEqual(inputs.map(call => call[1]), [first, hidden, release].map(encodeTouchEvent));
  assert.deepEqual(game.calls.filter(call => call[0] === "touch-page").map(call => call[1]), [0, 1, 1]);
  assert.equal(game.calls.filter(call => call[0] === "local-touch").length, 1, "paging never installs a fresh contact owner");
  assert.equal(game.calls.filter(call => call[0] === "member-capture").length, 5);
  assert.ok(game.calls.findIndex(call => call[0] === "touch") < game.calls.findIndex(call => call[0] === "local-touch-page"));
  assert.deepEqual((await h.rpc("play-page", { page: 0 })).result, { kind: "local-page", page: 0, touchVisible: true });
  refused = true;
  assert.match((await h.rpc("play-page", { page: 1 })).error, /actual page remap refused/);
  await h.tick(); assert.equal(h.views[0].localDraws.at(-1).page, 0);
  assert.equal(h.of("play-error").length, 0); assert.equal(game.stops, 0); assert.equal(game.frees, 0);
  const calls = game.calls.filter(call => call[0] === "local-touch-page").length;
  assert.ok((await h.rpc("play-page", { page: 2 })).error);
  assert.equal(game.calls.filter(call => call[0] === "local-touch-page").length, calls);
  await h.send({ kind: "play-stop", playId: 7 }); assert.equal(game.stops, 1); assert.equal(game.frees, 1);
  assert.ok(h.of("play-stopped").at(-1).replays.every(row => row.replay !== null && !row.replayComplete));

  const missing = await catalogWorker({ missingLocalTouchPage: true }); await missing.send(request());
  assert.equal(missing.localConstructions.length, 0); assert.equal(missing.preparedOwners.at(-1).frees, 1);
  assert.match(missing.of("play-reply").at(-1).error, /contact routing/);
  for (const value of [true, "false"]) {
    const mismatch = await active({ startRequest: request(), localTouchVisibility: () => value });
    await mismatch.rpc("play-page", { page: 1 });
    assert.equal(mismatch.of("play-error").length, 1);
    assert.equal(mismatch.locals[0].stops, 1); assert.equal(mismatch.locals[0].frees, 1);
  }
});

test("local captures preserve independent member prefixes through setup, serialization, runtime and rejected-audio failures", async () => {
  const setup = await catalogWorker({ localCaptureError: player => player === 7 });
  await setup.send(localRequest({ recordReplay: true }));
  const failedSetup = setup.of("play-error").at(-1), configured = setup.locals[0];
  assert.match(failedSetup.message, /member capture setup/);
  assert.deepEqual(failedSetup.replays.map(row => [row.player, row.replay !== null, row.replayComplete]), [[99, true, false], [7, false, false], [31, false, false]]);
  assert.deepEqual(configured.disposals, ["stop", "take:99", "take:7", "take:31", "free"]);
  assert.deepEqual(configured.calls.filter(row => row[0] === "member-capture").map(row => row[1]), [99, 7]);
  assert.equal(failedSetup.replay, null); assert.equal(failedSetup.released, true);
  const shared = Uint8Array.from([66, 75, 82, 9]);
  for (const fault of ["serialize", "duplicate", "oversized"]) {
    const h = await active({ startRequest: localRequest({ recordReplay: true }),
      localReplayBytes(player) {
        if (player === 7) {
          if (fault === "serialize") throw new Error("member codec failure");
          if (fault === "duplicate") return shared;
          return new Uint8Array(Math.floor(64 * 1024 * 1024 / 3) + 1);
        }
        return player === 99 && fault === "duplicate" ? shared : Uint8Array.from([66, 75, 82, player]);
      },
      localValue(game, player, field) { if (player === 7 && field === "hits") throw new Error("member score unavailable"); return game.memberScores.get(player)[field]; },
    });
    await h.send({ kind: "play-stop", playId: 7 });
    const receipt = h.of("play-stopped").at(-1), game = h.locals[0];
    assert.equal(receipt.localScores[1].hits, null); assert.equal(receipt.localScores[1].misses, SCORE.misses);
    assert.equal(receipt.hits, SCORE.hits); assert.equal(receipt.replay, null); assert.equal(receipt.replayError, null);
    assert.deepEqual(receipt.replays.map(row => [row.player, row.replay !== null, row.replayComplete]), [[99, true, false], [7, false, false], [31, true, false]]);
    assert.match(receipt.replays[1].replayError, /codec failure|transferable layout/);
    assert.equal(receipt.replays[0].replayError, null); assert.equal(receipt.replays[2].replayError, null);
    assert.equal(game.memberReplayTakes.size, 3); assert.equal(game.stops, 1); assert.equal(game.frees, 1);
    const transfer = h.transfers[h.messages.indexOf(receipt)];
    assert.equal(transfer.length, 2); assert.equal(new Set(transfer).size, 2);
    assert.equal(transfer[0], game.memberReplayBytes.get(99).buffer); assert.equal(transfer[1], game.memberReplayBytes.get(31).buffer);
    assert.equal(transfer.every(buffer => buffer.byteLength === 0), true);
  }
  const partial = await active({ startRequest: localRequest({ recordReplay: true }),
    inputHidBlob(game) { game.memberScores.get(99).hits = 18n; throw new Error("actual committed member report failure"); },
  });
  await partial.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }, hidEvent({ sequence: 2n }), touchEvent({ sequence: 3n })] }));
  const failed = partial.of("play-error").at(-1), game = partial.locals[0];
  assert.match(failed.message, /actual committed member report failure/); assert.equal(failed.hits, 18n);
  assert.deepEqual(game.calls.filter(row => ["blob", "hid", "touch", "close"].includes(row[0])).map(row => row[0]), ["blob", "hid"]);
  assert.ok(failed.replays.every(row => row.replay !== null && row.replayComplete === false));
  const trace = game.calls.length;
  await partial.send(step({ tickId: 2 })); assert.equal(game.calls.length, trace); assert.equal(game.frees, 1);
  const port = commandPort(), original = batch(9007199254740997n);
  const rejected = await started({ batches: [original], startRequest: localRequest({ recordReplay: true }) });
  await attachCommands(rejected, port);
  await port.acknowledge({ status: 9, admitted: 1, error: "actual shared queue refused second command" });
  assert.deepEqual(rejected.locals[0].calls.filter(row => row[0] === "ack"), [["ack", original.sequence, 1, false]]);
  assert.equal(port.posts.length, 1); assert.equal(port.closes, 1);
  assert.ok(rejected.of("play-error").at(-1).replays.every(row => row.replay !== null && row.replayComplete === false));
});

test("direct live and replay commands wait for actual client ACKs using core identities independent of port sequence", async () => {
  for (const mode of ["live", "replay"]) {
    const port = commandPort();
    const coreSequence = 18446744073709551614n;
    const original = { sequence: coreSequence, commands: [
      { ...command(), voice: 18446744073709551615n, sample: 18446744073709551615n, at: -9223372036854775808n,
        value: 9223372036854775807n, denominator: 18446744073709551615n }, command(23n),
    ] };
    let complete = false;
    const h = await started({ batches: [original], observeOutput: () => complete,
      beforeFree() { assert.equal(port.closes, 1, "client closes before generated owner disposal"); },
      ...(mode === "replay" ? { startRequest: replayRequest(replayFile().file) } : {}) });
    const game = h.replays[0] ?? h.games[0];
    const rpcId = await attachCommands(h, port);
    assert.equal(port.starts, 1); assert.equal(port.posts.length, 1);
    assert.deepEqual(port.posts[0], { kind: "commands", generation: 7, sequence: 1, commands: original.commands });
    assert.equal(h.of("play-reply").some(value => value.rpcId === rpcId), false);
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), []);
    await port.acknowledge({ generation: 8 });
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [], "foreign generation is never execution evidence");
    await port.acknowledge();
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [["ack", coreSequence, 2, true]]);
    assert.deepEqual(h.of("play-reply").find(value => value.rpcId === rpcId).result,
      { kind: "audio-ready", commandsPending: false });
    await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
    game.batches.push({ sequence: coreSequence + 1n, commands: [command(31n)] });
    if (mode === "live") await h.send(step({ events: [
      { hostNs: ORIGIN, key: 2, down: true, sequence: 1n },
    ] }));
    else {
      await h.send({ kind: "play-render", playId: 7, renderId: 1, presentedNs: null, presentedHostNs: null });
      assert.equal(port.posts.at(-1).kind, "poll");
      await port.acknowledge({ report: renderReport() });
    }
    const receipt = h.of(mode === "live" ? "play-step-done" : "play-render-done").at(-1);
    assert.equal(receipt.commandsPending, true);
    if (mode === "replay") assert.equal(receipt.observedTick, 0);
    assert.equal(port.posts.length, mode === "live" ? 2 : 3);
    assert.equal(port.posts.at(-1).kind, "commands");
    assert.equal(port.posts.at(-1).sequence, mode === "live" ? 2 : 3);
    assert.equal(h.of("play-commands").length, 0, "no batch returns through Window");
    await port.acknowledge();
    assert.deepEqual(game.calls.filter(row => row[0] === "ack").at(-1), ["ack", coreSequence + 1n, 1, true]);
    if (mode === "live") {
      await h.send({ kind: "play-render", playId: 7, renderId: 1, presentedNs: 0n, presentedHostNs: ORIGIN });
      await port.acknowledge({ report: renderReport() });
      assert.equal(h.of("play-render-done").at(-1).pendingInputs, 1);
    }
    complete = true;
    await h.send({ kind: "play-render", playId: 7, renderId: 2,
      presentedNs: mode === "live" ? 1n : null, presentedHostNs: mode === "live" ? ORIGIN + 1n : null });
    assert.equal(port.posts.at(-1).kind, "poll");
    await port.acknowledge({ report: renderReport() });
    const final = h.of("play-render-done").at(-1);
    assert.equal(final.completed, true); assert.equal(final.commandsPending, false);
    assert.equal(final.observedTick, mode === "live" ? 1 : 0);
    await h.send({ kind: "play-stop", playId: 7, completed: true });
    assertReleased(h); assert.equal(port.closes, 1);
    assert.equal(port.onmessage, null); assert.equal(port.onmessageerror, null);
  }
});

test("direct command rejection retains the real core prefix while timeout, malformed ACK and cancelled late ACK never invent admissions", async () => {
  for (const failure of ["prefix", "timeout", "malformed", "cancel"]) {
    const port = commandPort();
    const h = await started({ batches: [batch(9007199254740995n)],
      ack(_game, args) { if (args[2] === false) throw new Error("secondary core refusal"); },
      beforeFree() { assert.equal(port.closes, 1); } });
    const game = h.games[0], rpcId = await attachCommands(h, port);
    const lateListener = port.onmessage;
    if (failure === "prefix") await port.acknowledge({ status: 3, admitted: 1, error: "actual queue prefix" });
    else if (failure === "timeout") await h.runTimer(50);
    else if (failure === "malformed") await port.acknowledge({ admitted: 1 });
    else await h.send({ kind: "play-stop", playId: 7 });
    const reply = h.of("play-reply").find(value => value.rpcId === rpcId);
    assert.ok(reply.error); assert.equal(reply.result, undefined);
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), failure === "prefix"
      ? [["ack", 9007199254740995n, 1, false]] : []);
    if (failure === "prefix") {
      assert.match(h.of("play-error").at(-1).message, /actual queue prefix/);
      assert.doesNotMatch(h.of("play-error").at(-1).message, /secondary core refusal/);
    }
    assertReleased(h); assert.equal(port.posts.length, 1);
    await h.send(startRequest({ playId: 8 }));
    const count = h.messages.length, oldCalls = game.calls.length;
    lateListener({ data: { kind: "ack", generation: 7, sequence: 1, operation: "commands",
      status: 0, admitted: 2, error: null, report: null } });
    await flushJobs();
    assert.equal(game.calls.length, oldCalls); assert.equal(h.messages.length, count);
    assert.equal(h.games[1].stops, 0); assert.equal(port.posts.length, 1);
    // This new owner never owned the old port.
    await h.send({ kind: "play-stop", playId: 8 });
  }
});

test("direct audio admission closes refused endpoints and excludes legacy pulls or ACKs after handoff", async () => {
  for (const scenario of ["before-samples", "generation", "capacity", "already-drained", "active", "repeat", "pull", "ack"]) {
    const h = await started(); const port = commandPort();
    let owned = null;
    if (scenario === "before-samples") {
      await h.send({ kind: "play-audio", playId: 7, rpcId: ++h.rpcId, port,
        generation: 7, queueCapacity: 4096, timeoutMs: 50 });
    } else if (["repeat", "pull", "ack"].includes(scenario)) {
      owned = commandPort(); await attachCommands(h, owned);
      assert.equal(owned.starts, 1);
      if (scenario === "repeat") await h.send({ kind: "play-audio", playId: 7, rpcId: ++h.rpcId,
        port, generation: 7, queueCapacity: 4096, timeoutMs: 50 });
      else if (scenario === "pull") await h.rpc("play-commands");
      else await h.send({ kind: "play-ack", playId: 7, sequence: 1n, admitted: 0, success: true });
    } else if (scenario === "active") {
      for (let count = 0; count < 3; count++) await h.rpc("play-sample");
      await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
      await h.send({ kind: "play-audio", playId: 7, rpcId: ++h.rpcId, port,
        generation: 7, queueCapacity: 4096, timeoutMs: 50 });
    } else {
      if (scenario === "already-drained") await h.rpc("play-commands");
      await attachCommands(h, port, scenario === "generation" ? { generation: 8 }
        : scenario === "capacity" ? { queueCapacity: 2 } : {});
    }
    assert.equal(h.of("play-error").length, 1); assertReleased(h);
    assert.deepEqual(h.games[0].calls.filter(row => row[0] === "ack"), []);
    assert.equal(port.starts, 0); assert.equal(port.posts.length, 0);
    assert.equal(port.closes, ["pull", "ack"].includes(scenario) ? 0 : 1);
    if (owned) assert.equal(owned.closes, 1);
  }
  const h = await started(), stale = commandPort();
  await h.send({ kind: "play-audio", playId: 6, rpcId: 2, port: stale,
    generation: 6, queueCapacity: 4096, timeoutMs: 50 });
  assert.equal(stale.closes, 1); assert.equal(stale.starts, 0);
  assert.equal(h.games[0].stops, 0); assert.equal(h.of("play-error").length, 0);
  await h.send({ kind: "play-stop", playId: 7 });
});

async function directActive(options = {}) {
  const port = commandPort();
  const h = await started({ ...options,
    beforeFree(game) { assert.equal(port.closes, 1); options.beforeFree?.(game); } });
  const rpcId = await attachCommands(h, port);
  assert.deepEqual(h.of("play-reply").find(value => value.rpcId === rpcId).result,
    { kind: "audio-ready", commandsPending: false });
  await h.rpc("play-activate", { hostNs: options.activationHost ?? ORIGIN, startFrame: options.activationFrame ?? START });
  return { h, port, game: h.replays[0] ?? h.games[0] };
}

function directObservation(fields = {}) {
  return { kind: "play-render", playId: 7, renderId: 1, presentedNs: null, presentedHostNs: null, ...fields };
}

test("a blocked renderer preserves original pending input, direct audio ACK and capture", async () => {
  const { h, port, game } = await directActive({ recordReplay: true });
  h.renderPort.blocked = true;
  const key = { hostNs: ORIGIN, sequence: 9007199254740993n, key: 2, down: true };
  await h.send(step({ events: [key], watermark: ORIGIN + 200000000n }));
  const pending = h.renderPort.posts.at(-1);
  assert.equal(pending.kind, "packet");
  const exports = h.visualExports.length, baseline = game.visualBaseline;
  await h.send(directObservation({ presentedNs: 0n, presentedHostNs: ORIGIN }));
  await port.acknowledge({ report: renderReport() });
  assert.equal(game.pendingInput.length, 1, "one genuine audio anchor retains independently acquired input");
  game.batches.push(batch(71n));
  await h.send(directObservation({ renderId: 2, presentedNs: 100000000n, presentedHostNs: ORIGIN + 100000000n }));
  await port.acknowledge({ report: renderReport({ cursor: 9007199254744000n }) });
  if (port.posts.at(-1).kind === "commands") await port.acknowledge();
  assert.equal(game.processedInput.length, 1);
  assert.equal(game.processedInput[0].hostNs ?? game.processedInput[0].host, ORIGIN);
  assert.ok(game.calls.some(call => call[0] === "ack" && call[1] === 71n), "core audio acknowledgement never waits for visual ACK");
  assert.equal(game.visualBaseline, baseline);
  assert.equal(h.visualExports.length, exports, "latest dirty state cannot replace the frozen unacknowledged snapshot");
  assert.equal(h.of("play-step-done").at(-1).tickId, 1);
  assert.equal(h.of("play-render-done").at(-1).renderId, 2);
  assert.equal(game.frees, 0);
  h.renderPort.ack(pending, { sequence: new DataView(pending.packet.buffer).getBigUint64(24, true) + 1n }); await flushJobs();
  assert.equal(game.visualBaseline, baseline);
  h.renderPort.ack(pending); await flushJobs();
  assert.ok(game.visualBaseline > baseline);
  assert.equal(h.visualExports.length, exports + 1);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.replayTakes, 1);
  assert.ok(h.of("play-stopped").at(-1).replay instanceof Uint8Array);
  const acks = h.visualAcks.length;
  h.renderPort.ack(pending); await flushJobs();
  assert.equal(h.visualAcks.length, acks, "retired ACK never reads the freed gameplay owner");
});

test("terminal graphics failure delivers actual room capture before delayed cleanup permits global fatal", async () => {
  const h = await roomPrepared({ roomHoldAfterClose: true });
  const game = h.locals[0], event = await committedRoom(h);
  await h.rpc("play-activate", { hostNs: event.targetHostNs, targetHostNs: event.targetHostNs, startFrame: START });
  const session = h.roomSessions[0], channel = h.roomChannels[0];
  session.onPublish = (_, finalPrefix) => {
    assert.equal(finalPrefix, true); assert.equal(game.stops, 0); assert.equal(game.frees, 0);
  };
  const visual = h.renderPort.posts.findLast(row => row.kind === "packet");
  h.renderPort.emit({ kind: "render-error", generation: visual.generation, content: visual.content,
    message: "GPU device lost" }); await flushJobs();
  assert.equal(game.groupProgressReads, 1);
  assert.equal(game.frees, 1);
  assert.equal(h.of("play-error").length, 0, "correlated receipt joins the retained room read");
  await h.send(null);
  assert.equal(h.of("fatal").length, 0, "global termination cannot erase a pending capture");
  await h.send({ kind: "dispose" });
  assert.equal(h.of("disposed").length, 0);
  channel.reads.at(-1).gate.resolve(Uint8Array.of(1)); await flushJobs();
  const final = h.of("play-error").find(row => row.playId === 7);
  assert.ok(final); assert.match(final.message, /GPU device lost/);
  assert.equal(final.replays[0].replayComplete, false);
  assert.ok(final.replays[0].replay instanceof Uint8Array);
  assert.equal(final.room.localComplete, false);
  assert.equal(final.room.finalQueued, true);
  assert.deepEqual(game.disposals, ["group-progress", "stop", "take:4294967295", "free"]);
  const fatalIndex = h.messages.findIndex(row => row.kind === "fatal");
  assert.ok(fatalIndex < 0 || h.messages.indexOf(final) < fatalIndex);
});

test("actual direct polls fairly retire reports between retained command ACK and new live or replay batches", async () => {
  for (const mode of ["live", "replay"]) {
    let outputs = 0;
    const first = batch(9007199254740993n), waiting = batch(9007199254740994n), fed = batch(9007199254740995n);
    const { h, port, game } = await directActive({
      ...(mode === "replay" ? { startRequest: replayRequest(replayFile().file) } : {}),
      admitOutput(owner) { owner.batches.push(++outputs === 1 ? first : fed); },
    });
    await h.send(directObservation());
    assert.deepEqual(port.posts, [{ kind: "poll", generation: 7, sequence: 1 }]);
    assert.equal(h.of("play-render-done").length, 0);
    await port.acknowledge({ report: renderReport() });
    assert.equal(port.posts.at(-1).kind, "commands"); assert.equal(port.posts.at(-1).sequence, 2);
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [], "a successful poll never acknowledges a core batch");
    assert.equal(h.of("play-render-done").at(-1).commandsPending, true);
    game.batches.push(waiting);
    const probes = game.calls.filter(row => row[0] === "commands").length;
    await h.send(directObservation({ renderId: 2 }));
    assert.equal(port.posts.length, 2, "the in-flight command owns the one client slot");
    assert.equal(game.calls.filter(row => row[0] === "commands").length, probes);
    await port.acknowledge();
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [["ack", first.sequence, 2, true]]);
    assert.deepEqual(port.posts.at(-1), { kind: "poll", generation: 7, sequence: 3 });
    assert.equal(game.calls.filter(row => row[0] === "commands").length, probes,
      "the waiting report must retire BGM credits before extracting another core batch");
    await port.acknowledge({ report: renderReport({ cursor: 9007199254744000n }) });
    assert.equal(outputs, 2);
    assert.equal(port.posts.at(-1).kind, "commands"); assert.equal(port.posts.at(-1).sequence, 4);
    assert.deepEqual(port.posts.at(-1).commands, waiting.commands);
    const receipt = h.of("play-render-done").at(-1);
    assert.equal(receipt.renderId, 2); assert.equal(receipt.commandsPending, true); assert.equal(receipt.observedTick, 0);
    assert.equal(game.calls.filter(row => row[0] === "ack").length, 1);
    await port.acknowledge();
    assert.equal(port.posts.at(-1).sequence, 5); assert.deepEqual(port.posts.at(-1).commands, fed.commands);
    await port.acknowledge();
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [
      ["ack", first.sequence, 2, true], ["ack", waiting.sequence, 2, true], ["ack", fed.sequence, 2, true],
    ]);
    assert.deepEqual(port.posts.map(value => value.kind), ["poll", "commands", "poll", "commands", "commands"]);
    assert.equal(h.of("play-commands").length, 0);
    await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
  }
});

test("awaited reports preserve original presentation pairs and apply to the current input frontier before completion", async () => {
  let complete = false;
  const next = batch(9007199254741001n);
  const { h, port, game } = await directActive({
    input(owner) { owner.score.hits = 18n; owner.batches.push(next); },
    observeOutput(owner) { assert.equal(owner.score.hits, 18n); return complete; },
  });
  const point = { output: 604800000000001n, host: ORIGIN + 3n };
  const request = directObservation({ presentedNs: point.output, presentedHostNs: point.host });
  await h.send(request);
  request.presentedNs = 0n; request.presentedHostNs = 0n;
  const probes = game.calls.filter(row => row[0] === "commands").length;
  await h.send(step({ events: [{ hostNs: ORIGIN + 7n, key: 2, down: true, sequence: 18446744073709551615n }],
    watermark: ORIGIN + 9n, audioNs: 604800000000003n }));
  assert.equal(h.of("play-step-done").at(-1).commandsPending, true);
  assert.equal(port.posts.length, 1); assert.equal(port.posts[0].kind, "poll");
  assert.equal(game.calls.filter(row => row[0] === "commands").length, probes);
  assert.equal(game.calls.filter(row => row[0] === "output").length, 0);
  const actual = renderReport({ cursor: 9007199254746000n });
  actual.words[24] = 0xffffffff; actual.words[25] = 0x80000000; // Transport preserves counters at full width.
  await port.acknowledge({ report: actual });
  const output = game.calls.find(row => row[0] === "output");
  assert.deepEqual(Array.from(output[1]), Array.from(actual.words)); assert.equal(output[2], point.output);
  assert.deepEqual(game.calls.find(row => row[0] === "presentation"), ["presentation", point.output, point.host]);
  assert.ok(game.calls.findIndex(row => row[0] === "close") < game.calls.findIndex(row => row[0] === "output"));
  const reply = h.of("play-render-done").at(-1);
  assert.equal(reply.observedTick, 1); assert.equal(reply.commandsPending, false); assert.equal(reply.completed, false);
  assert.equal(reply.pendingInputs, 1, "first anchor holds queued input without manufacturing commands");
  assert.equal(game.processedInput.length, 0);
  await h.send(directObservation({ renderId: 2, presentedNs: point.output + 10n, presentedHostNs: point.host + 10n }));
  await port.acknowledge({ report: renderReport({ cursor: 9007199254746257n }) });
  assert.equal(game.processedInput.length, 1);
  assert.equal(h.of("play-render-done").at(-1).commandsPending, true);
  assert.equal(port.posts.at(-1).kind, "commands"); assert.deepEqual(port.posts.at(-1).commands, next.commands);
  assert.equal(game.calls.filter(row => row[0] === "ack").length, 0);
  await port.acknowledge();
  complete = true;
  await h.send(directObservation({ renderId: 3, presentedNs: point.output + 20n, presentedHostNs: point.host + 20n }));
  assert.equal(h.of("play-render-done").length, 2);
  await port.acknowledge({ report: renderReport({ cursor: 9007199254746514n }) });
  const final = h.of("play-render-done").at(-1);
  assert.equal(final.completed, true); assert.equal(final.commandsPending, false); assert.equal(final.observedTick, 1);
  await h.send({ kind: "play-stop", playId: 7, completed: true });
  assertReleased(h, { ...SCORE, hits: 18n });
});

test("direct report overlap, external payloads, bad evidence and cancelled polling fence without late writes or core ACKs", async () => {
  for (const scenario of ["overlap", "duplicate", "stale", "external", "pair", "shape", "semantic", "rust",
    "timeout", "terminal", "cancel", "pending-natural"]) {
    const { h, port, game } = await directActive({
      admitOutput() { if (scenario === "rust") throw new Error("actual Rust output evidence refusal"); },
      observeOutput() { return scenario === "pending-natural"; },
    });
    const stale = port.onmessage;
    if (scenario === "external") await h.send(directObservation({ report: undefined }));
    else if (scenario === "pair") await h.send(directObservation({ presentedNs: 1n }));
    else {
      await h.send(directObservation(scenario === "pending-natural" ? { presentedNs: 0n, presentedHostNs: ORIGIN } : {}));
      assert.equal(port.posts.at(-1).kind, "poll");
      if (scenario === "overlap") await h.send(directObservation({ renderId: 2 }));
      else if (scenario === "duplicate") await h.send(directObservation());
      else if (scenario === "stale" || scenario === "pending-natural") {
        await port.acknowledge({ report: renderReport() });
        await h.send(directObservation({ renderId: scenario === "stale" ? 1 : 2 }));
        if (scenario === "pending-natural") await h.send({ kind: "play-stop", playId: 7, completed: true });
      } else if (scenario === "shape") await port.acknowledge({ report: { available: false, words: new Uint32Array(55) } });
      else if (scenario === "semantic") await port.acknowledge({ report: renderReport({ start: START + 1n }) });
      else if (scenario === "rust") await port.acknowledge({ report: renderReport() });
      else if (scenario === "timeout") await h.runTimer(50);
      else if (scenario === "terminal") {
        port.onmessage({ data: { kind: "terminal", generation: 7, status: 9 } });
        await flushJobs();
      } else await h.send({ kind: "play-stop", playId: 7 });
    }
    if (scenario !== "cancel") assert.equal(h.of("play-error").length, 1);
    if (scenario === "rust") assert.match(h.of("play-error").at(-1).message, /actual Rust output evidence refusal/);
    assertReleased(h); assert.equal(port.closes, 1);
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [], "neither poll success nor rejection can consume a core batch");
    if (scenario === "external" || scenario === "pair") assert.equal(port.posts.length, 0);
    await h.send(startRequest({ playId: 8 }));
    const calls = game.calls.length, messages = h.messages.length;
    stale({ data: { kind: "ack", generation: 7, sequence: port.posts.at(-1)?.sequence ?? 1,
      operation: "poll", status: 0, admitted: 0, error: null, report: renderReport() } });
    await flushJobs();
    assert.equal(game.calls.length, calls); assert.equal(h.messages.length, messages);
    assert.equal(h.games[1].stops, 0);
    await h.send({ kind: "play-stop", playId: 8 });
  }
});

// Live authority fixtures use small exact clocks, separate from long-width wire tests.
const AUDIO_ARM = 1000000000n;
const AUDIO_START = 48000n;
async function authorityActive(extra = {}) {
  return active({ manualClock: true, activationHost: AUDIO_ARM, activationFrame: AUDIO_START, ...extra });
}
function authorityStep(fields = {}) {
  return step({ watermark: 1200000000n, nowNs: 1200000000n, audioNs: 220000000n, ...fields });
}
async function authorityRender(h, id, output, host) {
  await h.send({ kind: "play-render", playId: 7, renderId: id,
    report: renderReport({ start: AUDIO_START, cursor: AUDIO_START + BigInt(id) * 257n + 129n }),
    presentedNs: output, presentedHostNs: host });
}
async function legacyWitness(h, id, output, host) {
  await h.send({ kind: "play-render", playId: 7, renderId: id,
    report: renderReport(), presentedNs: output, presentedHostNs: host });
}

test("live acquisition and render reordering preserves exact occurrence and accepted prefix", async () => {
  const histories = [];
  for (const renderFirst of [false, true]) {
    const h = await authorityActive();
    const game = h.games[0];
    h.setWindowNowNs(1200000000n);
    const request = authorityStep({ events: [{ hostNs: 1150000000n, key: 2, down: true, sequence: 7n }] });
    if (!renderFirst) {
      await h.send(request);
      assert.equal(game.pending_inputs(), 1);
      assert.equal(game.processedInput.length, 0, "submission and moving HOST do not judge");
      assert.equal(h.of("play-step-done").at(-1).pendingInputs, 1);
    }
    await authorityRender(h, 1, 100000000n, 1100000000n);
    assert.equal(game.processedInput.length, 0, "one real anchor is insufficient");
    await authorityRender(h, 2, 200000000n, 1200000000n);
    if (renderFirst) await h.send(request);
    assert.equal(game.pending_inputs(), 0);
    assert.equal(game.processedInput.length, 1);
    assert.equal(game.processedInput[0].host, 1150000000n);
    assert.equal(game.processedInput[0].output, 150000000n);
    assert.equal(game.logicalOutput, 200000000n);
    histories.push(game.processedInput.map(event => [event.host, event.output, event.key, event.sequence]));
    await h.send({ kind: "play-stop", playId: 7 });
  }
  assert.deepEqual(histories[0], histories[1]);
});

test("partial prefixes retain newer original input and permit older cross-source input within the fixed lag", async () => {
  const h = await authorityActive({ startRequest: startRequest({ inputMode: "physical-contact" }) });
  const game = h.games[0]; h.setWindowNowNs(1200000000n);
  const keyboard = { hostNs: 1195000000n, key: 2, down: true, sequence: 1n };
  const contact = touchEvent({ hostNs: 1190000000n, sequence: 2n });
  await h.send(authorityStep({ events: [keyboard], watermark: 1188000000n }));
  assert.equal(h.of("play-error").length, 0); assert.equal(game.closedPrefix, 1188000000n);
  assert.equal(game.pending_inputs(), 1); assert.equal(game.pendingInput[0].host, keyboard.hostNs);
  await h.send(authorityStep({ tickId: 2, events: [contact], watermark: 1188000000n }));
  assert.equal(h.of("play-error").length, 0, "batch maximum is not a closed-prefix proof for another source");
  await h.send(authorityStep({ tickId: 3, watermark: null }));
  assert.equal(game.closedPrefix, 1188000000n); assert.equal(game.calls.filter(row => row[0] === "close").length, 2);
  await authorityRender(h, 1, 100000000n, 1100000000n);
  await authorityRender(h, 2, 200000000n, 1200000000n);
  assert.equal(game.pending_inputs(), 2); assert.equal(game.processedInput.length, 0, "known audio does not cover input newer than the declared prefix");
  await h.send(authorityStep({ tickId: 4, watermark: 1190000000n }));
  assert.equal(game.pending_inputs(), 1); assert.deepEqual(game.processedInput[0].bytes, encodeTouchEvent(contact));
  assert.equal(game.processedInput[0].host, 1190000000n); assert.equal(game.processedInput[0].output, 190000000n);
  const laterContact = touchEvent({ hostNs: 1193000000n, sequence: 3n, phase: 1 });
  await h.send(authorityStep({ tickId: 5, events: [laterContact], watermark: 1190000000n }));
  assert.equal(h.of("play-error").length, 0, "a lower partial prefix does not discard the separately admitted keyboard maximum");
  assert.equal(game.pending_inputs(), 2);
  await h.send(authorityStep({ tickId: 6, watermark: 1195000000n }));
  assert.equal(game.pending_inputs(), 0);
  assert.deepEqual(game.processedInput.map(event => event.host), [1190000000n, 1193000000n, 1195000000n]);
  assert.deepEqual(game.processedInput.map(event => event.output), [190000000n, 193000000n, 195000000n]);
  assert.equal(game.processedInput.at(-1).sequence, keyboard.sequence);
  await h.send(authorityStep({ tickId: 7, watermark: 1200000000n })); assert.equal(game.logicalOutput, 200000000n);
  const mutations = game.calls.filter(row => ["blob", "touch", "input", "close", "service"].includes(row[0])).length;
  await h.send(authorityStep({ tickId: 8, events: [touchEvent({ hostNs: 1199000000n, sequence: 4n, phase: 1 })] }));
  assert.equal(h.of("play-error").length, 1, "an event truly behind the closed prefix still refuses");
  assert.equal(game.calls.filter(row => ["blob", "touch", "input", "close", "service"].includes(row[0])).length, mutations);
  assert.equal(game.processedInput.length, 3);
});

test("regressive or future partial-prefix envelopes refuse before admitting their otherwise valid tail", async () => {
  for (const invalid of [1187999999n, 1200000001n, "not a prefix", 1188000000]) {
    const h = await authorityActive({ startRequest: startRequest({ inputMode: "physical-contact" }) });
    const game = h.games[0]; h.setWindowNowNs(1200000000n);
    await h.send(authorityStep({ events: [{ hostNs: 1195000000n, key: 2, down: true, sequence: 1n }], watermark: 1188000000n }));
    assert.equal(h.of("play-error").length, 0); const original = game.pendingInput[0];
    const before = game.calls.filter(row => ["input", "blob", "touch", "close", "service", "commands"].includes(row[0])).length;
    await h.send(authorityStep({ tickId: 2, events: [touchEvent({ hostNs: 1190000000n, sequence: 2n })], watermark: invalid }));
    assert.equal(h.of("play-error").length, 1); assert.equal(game.pendingInput.length, 1); assert.equal(game.pendingInput[0], original);
    assert.equal(game.closedPrefix, 1188000000n); assert.equal(game.processedInput.length, 0);
    assert.equal(game.calls.filter(row => ["input", "blob", "touch", "close", "service", "commands"].includes(row[0])).length, before);
  }
});

test("input beyond a partial prefix blocks local page and completion after two audio anchors until coverage advances", async () => {
  const request = localRequest({ keyPairs: new Uint32Array(), localPlanWords: localPlan([[91, 2n], [2, 3n], [88, 4n], [7, 5n], [0xffffffff, 6n]]),
    hidSetup: hidSetup([3n, 4n, 5n, 6n]) });
  const h = await authorityActive({ startRequest: request });
  const game = h.locals[0]; h.setWindowNowNs(1200000000n); const original = touchEvent({ hostNs: 1195000000n });
  await h.send(authorityStep({ events: [original], watermark: 1188000000n }));
  await authorityRender(h, 1, 100000000n, 1100000000n); await authorityRender(h, 2, 200000000n, 1200000000n);
  assert.equal(game.presentations.length, 2); assert.equal(game.pending_inputs(), 1); assert.equal(game.processedInput.length, 0);
  assert.equal(h.of("play-render-done").at(-1).completed, false);
  const pageCalls = game.calls.filter(row => row[0] === "local-touch-page").length;
  assert.match((await h.rpc("play-page", { page: 1 })).error, /pending|input|drain/i);
  assert.equal(game.calls.filter(row => row[0] === "local-touch-page").length, pageCalls);
  await h.send(authorityStep({ tickId: 2, watermark: 1200000000n }));
  assert.equal(game.pending_inputs(), 0); assert.deepEqual(game.processedInput[0].bytes, encodeTouchEvent(original));
  assert.equal(game.processedInput[0].host, original.hostNs); assert.equal(game.processedInput[0].output, 195000000n);
  assert.equal((await h.rpc("play-page", { page: 1 })).result.page, 1);
  await authorityRender(h, 3, 200000000n, 1200000000n);
  assert.equal(h.of("play-render-done").at(-1).completed, false, "draining the input prefix does not supply actual end and audio-drain evidence");
  await h.send({ kind: "play-stop", playId: 7 });
});

test("null-prefix chunks accept earlier events from another genuine source and closed prefixes refuse late input", async () => {
  const histories = [];
  for (const split of [false, true]) {
    const h = await authorityActive({ startRequest: startRequest({ inputMode: "physical-contact" }) });
    const game = h.games[0]; h.setWindowNowNs(1200000000n);
    const key = { hostNs: 1180000000n, key: 2, down: true, sequence: 1n };
    const contact = touchEvent({ hostNs: 1150000000n, sequence: 2n });
    if (split) {
      await h.send(authorityStep({ tickId: 1, watermark: null, events: [key] }));
      await h.send(authorityStep({ tickId: 2, watermark: null, events: [contact] }));
      assert.equal(h.of("play-error").length, 0, "another source can arrive earlier before a prefix closes");
      assert.equal(game.closedPrefix, null);
      await h.send(authorityStep({ tickId: 3 }));
    } else await h.send(authorityStep({ events: [key, contact] }));
    assert.equal(game.pending_inputs(), 2);
    await authorityRender(h, 1, 100000000n, 1100000000n);
    await authorityRender(h, 2, 200000000n, 1200000000n);
    assert.deepEqual(game.processedInput.map(event => event.host), [1150000000n, 1180000000n]);
    histories.push(game.processedInput.map(event => [event.host, event.source, event.output]));
    const before = game.processedInput.length;
    await h.send(authorityStep({ tickId: split ? 4 : 2,
      events: [touchEvent({ hostNs: 1170000000n, sequence: 3n })] }));
    assert.equal(h.of("play-error").length, 1, "an already closed actual prefix still rejects late input");
    assert.equal(game.processedInput.length, before);
  }
  assert.deepEqual(histories[0], histories[1]);
});

test("generated cursor and lookahead provide scheduling while missing and stale output hold input", async () => {
  const h = await authorityActive();
  const game = h.games[0];
  h.setWindowNowNs(1200000000n);
  const raw = authorityStep({ events: [{ hostNs: 1150000000n, key: 2, down: true, sequence: 1n }],
    contextFrame: 96000n });
  delete raw.audioNs;
  await h.send(raw);
  assert.equal(h.of("play-error").length, 0);
  assert.equal(game.calls.filter(row => row[0] === "service").at(-1)[2], 1020000000n);
  assert.equal(game.processedInput.length, 0);
  await h.send({ kind: "play-stop", playId: 7 });
  const second = await authorityActive();
  const owner = second.games[0];
  second.setWindowNowNs(1200000000n);
  await second.send(authorityStep({ events: [{ hostNs: 1150000000n, key: 2, down: true, sequence: 1n }] }));
  await authorityRender(second, 1, null, null);
  assert.equal(owner.processedInput.length, 0);
  assert.equal(owner.logicalOutput, undefined);
  assert.equal(owner.pending_inputs(), 1);
  assert.equal(second.of("play-render-done").at(-1).completed, false);
  assert.ok(owner.calls.some(call => call[0] === "service" && call[2] > 0n), "generated credit still services scheduling");
  await authorityRender(second, 2, 100000000n, 1100000000n);
  await authorityRender(second, 3, 100000000n, 1150000000n);
  assert.equal(owner.presentations.length, 1, "repetition is not a new anchor");
  assert.equal(owner.processedInput.length, 0);
  second.setWindowNowNs(2200000000n);
  await second.send(authorityStep({ tickId: 2, watermark: 2200000000n, nowNs: 2200000000n }));
  assert.equal(owner.processedInput.length, 0);
  assert.equal(owner.pendingInput.length, 1);
  assert.match(second.of("play-error").at(-1).message, /clock.*unavailable|unavailable.*clock/i);
  await second.send({ kind: "play-stop", playId: 7 });
});

test("fresh Window receipt after awaited Worklet poll retains original pair and input timestamp", async () => {
  const { h, port, game } = await directActive({ manualClock: true, activationHost: AUDIO_ARM, activationFrame: AUDIO_START });
  h.setWindowNowNs(1200000000n);
  await h.send(authorityStep({ events: [{ hostNs: 1150000000n, key: 2, down: true, sequence: 9n }] }));
  await h.send(rawObservation({ timestamp: { contextTime: 1.1, performanceTime: 1100 }, observedNowMs: 1100 }));
  h.setWindowNowNs(1500000000n);
  await port.acknowledge({ report: renderReport({ start: AUDIO_START, cursor: 48386n }) });
  assert.deepEqual(game.calls.find(row => row[0] === "presentation"), ["presentation", 100000000n, 1100000000n]);
  assert.equal(game.calls.filter(row => row[0] === "service").at(-1)[1], 1500000000n,
    "neither request observedNowMs nor Worker-relative origin is the current Window time");
  assert.equal(game.pendingInput[0].host, 1150000000n);
  assert.equal(game.pendingInput[0].received, 1200000000n);
  await h.send({ kind: "play-stop", playId: 7 });
});

test("unavailable regressing estimates keep history, recover explicitly and do not refresh watchdog", async () => {
  const h = await authorityActive();
  const game = h.games[0];
  h.setWindowNowNs(1200000000n);
  await authorityRender(h, 1, 100000000n, 1100000000n);
  await authorityRender(h, 2, 200000000n, 1200000000n);
  const accepted = game.presentations.map(pair => ({ ...pair }));
  h.setWindowNowNs(1300000000n);
  await authorityRender(h, 3, 190000000n, 1300000000n);
  assert.deepEqual(game.presentations, accepted);
  assert.deepEqual(h.of("play-presentation-unavailable").at(-1),
    { kind: "play-presentation-unavailable", playId: 7, reason: "regressing-estimate" });
  h.setWindowNowNs(1400000000n);
  await authorityRender(h, 4, 300000000n, 1400000000n);
  assert.equal(h.of("play-presentation-unavailable").at(-1).reason, null);
  assert.equal(game.presentations.at(-1).output, 300000000n);
  h.setWindowNowNs(2399000000n);
  await authorityRender(h, 5, 300000000n, 2399000000n);
  assert.equal(h.of("play-error").length, 0);
  h.setWindowNowNs(2400000000n);
  await authorityRender(h, 6, 299000000n, 2400000000n);
  assert.match(h.of("play-error").at(-1).message, /clock.*unavailable|unavailable.*clock/i);
  assert.equal(game.presentations.at(-1).host, 1400000000n);
});

test("completion follows held input service and cannot complete an admission-only prefix", async () => {
  const h = await authorityActive({ observeOutput() { return true; } });
  const game = h.games[0];
  h.setWindowNowNs(1200000000n);
  await h.send(authorityStep({ events: [{ hostNs: 1150000000n, key: 2, down: true, sequence: 1n }] }));
  await authorityRender(h, 1, 100000000n, 1100000000n);
  assert.equal(h.of("play-render-done").at(-1).completed, false);
  await authorityRender(h, 2, 200000000n, 1200000000n);
  assert.equal(h.of("play-render-done").at(-1).completed, true);
  const operations = game.calls.map(row => row[0]);
  const processed = operations.lastIndexOf("processed"), completion = operations.lastIndexOf("completion");
  assert.ok(operations.lastIndexOf("output") < processed);
  assert.ok(processed < completion);
  assert.equal(game.pending_inputs(), 0);
  await h.send({ kind: "play-stop", playId: 7, completed: true });
});

test("pending Rust touch prevents page remap after input ACK and keeps acquisition geometry", async () => {
  const request = localRequest({ keyPairs: new Uint32Array(),
    localPlanWords: localPlan([[91, 2n], [2, 3n], [88, 4n], [7, 5n], [0xffffffff, 6n]]),
    hidSetup: hidSetup([3n, 4n, 5n, 6n]) });
  const h = await authorityActive({ startRequest: request });
  const game = h.locals[0];
  h.setWindowNowNs(1200000000n);
  const original = touchEvent({ hostNs: 1150000000n });
  await h.send(authorityStep({ events: [original] }));
  assert.equal(h.of("play-step-done").at(-1).pendingInputs, 1);
  const before = game.calls.filter(row => row[0] === "local-touch-page").length;
  assert.match((await h.rpc("play-page", { page: 1 })).error, /pending|input|drain/i);
  assert.equal(game.calls.filter(row => row[0] === "local-touch-page").length, before);
  await h.send({ kind: "resize", width: 1920, height: 1080 });
  await authorityRender(h, 1, 100000000n, 1100000000n);
  await authorityRender(h, 2, 200000000n, 1200000000n);
  assert.equal(game.pending_inputs(), 0);
  assert.deepEqual(game.processedInput[0].bytes, encodeTouchEvent(original));
  assert.deepEqual(game.processedInput[0].geometry,
    { cssWidth: original.width, cssHeight: original.height, surfaceWidth: original.surfaceWidth, surfaceHeight: original.surfaceHeight });
  assert.equal((await h.rpc("play-page", { page: 1 })).result.page, 1);
  await h.send({ kind: "play-stop", playId: 7 });
});

test("wrong Window origin and future or malformed complete batches fail before any queue effect", async () => {
  for (const origin of [undefined, -1n, 1, 9223372036854775808n]) {
    const h = await catalogWorker();
    await h.send(startRequest({ windowOriginNs: origin }));
    assert.equal(h.games.length, 0);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const extra of [
    { events: [{ hostNs: 1100000000n, key: 2, down: true, sequence: 1n },
      { hostNs: 1300000000n, key: 3, down: true, sequence: 2n }] },
    { events: [{ hostNs: 1100000000n, key: 2, down: true, sequence: 1n },
      { hostNs: 1150000000n, key: 3, down: "yes", sequence: 2n }] },
  ]) {
    const h = await authorityActive();
    const game = h.games[0]; h.setWindowNowNs(1200000000n);
    await h.send(authorityStep(extra));
    assert.equal(h.of("play-error").length, 1);
    assert.equal(game.pendingInput.length, 0);
    assert.equal(game.processedInput.length, 0);
    assert.equal(game.closedPrefix, null);
    assert.equal(game.calls.filter(row => ["input", "close", "service"].includes(row[0])).length, 0);
  }
});

test("same-Window acquisition envelope ahead of reconstructed service time retains original input until actual coverage", async () => {
  const h = await authorityActive({ startRequest: startRequest({ inputMode: "physical-contact" }) });
  const game = h.games[0];
  const serviceNow = 1200000000n;
  const envelope = 1200125000n;
  h.setWindowNowNs(serviceNow);
  await authorityRender(h, 1, 100000000n, 1100000000n);
  await authorityRender(h, 2, 200000000n, serviceNow);
  const original = touchEvent({ hostNs: envelope, sequence: 18446744073709551615n });
  const expectedPacket = encodeTouchEvent(original);
  await h.send(authorityStep({ nowNs: envelope, watermark: envelope, events: [original] }));
  assert.equal(h.of("play-error").length, 0);
  assert.equal(game.pendingInput.length, 1);
  assert.equal(game.pendingInput[0].host, envelope);
  assert.equal(game.pendingInput[0].received, envelope, "merger admission uses the same-Window acquisition receipt");
  assert.deepEqual(game.pendingInput[0].bytes, expectedPacket);
  assert.equal(game.lastService.now, serviceNow, "the acquisition receipt cannot clamp or replace fresh service time");
  assert.equal(game.processedInput.length, 0);
  assert.equal(game.logicalOutput, undefined);
  assert.equal(h.of("play-step-done").at(-1).pendingInputs, 1);
  h.setWindowNowNs(envelope);
  await h.send(authorityStep({ tickId: 2, nowNs: envelope, watermark: envelope }));
  assert.equal(game.processedInput.length, 0, "timer catch-up alone cannot grant an unobserved audio position");
  h.setWindowNowNs(1300000000n);
  await authorityRender(h, 3, 300000000n, 1300000000n);
  assert.equal(h.of("play-error").length, 0);
  assert.equal(game.pending_inputs(), 0);
  assert.equal(game.processedInput.length, 1);
  assert.equal(game.processedInput[0].host, envelope);
  assert.equal(game.processedInput[0].received, envelope);
  assert.equal(game.processedInput[0].output, 200125000n);
  assert.deepEqual(game.processedInput[0].bytes, expectedPacket, "canonical original/native provenance is unchanged after holding");
  await h.send({ kind: "play-stop", playId: 7 });
});

test("future acquired prefix alone cannot advance presentation until fresh service time covers it", async () => {
  const h = await authorityActive();
  const game = h.games[0];
  const envelope = 1200125000n;
  h.setWindowNowNs(1200000000n);
  await authorityRender(h, 1, 100000000n, 1100000000n);
  await authorityRender(h, 2, 200000000n, 1200000000n);
  await h.send(authorityStep({ nowNs: envelope, watermark: envelope }));
  assert.equal(h.of("play-error").length, 0);
  assert.equal(game.closedPrefix, envelope);
  assert.equal(game.pending_inputs(), 0);
  assert.equal(game.logicalOutput, undefined, "a complete acquired prefix still cannot substitute for current service evidence");
  h.setWindowNowNs(envelope);
  await h.send(authorityStep({ tickId: 2, nowNs: envelope, watermark: envelope }));
  assert.equal(game.logicalOutput, 200000000n, "only the previously accepted actual presentation becomes eligible");
  await h.send({ kind: "play-stop", playId: 7 });
});

test("one Any-source group retains its original member and accepts genuine automatic device identities", async () => {
  const h = await authorityActive({ startRequest: roomStartRequest({ windowOriginNs: 10000000000n }) });
  const game = h.locals[0];
  h.setWindowNowNs(1200000000n);
  await h.send(authorityStep({ events: [{ hostNs: 1150000000n, key: 2, down: true, sequence: 1n }] }));
  assert.deepEqual(game.memberIds, [0xffffffff]);
  assert.equal(game.pending_inputs(), 1);
  assert.equal(game.pendingInput[0].source, 1n);
  assert.equal(h.of("play-error").length, 0);
  await h.send({ kind: "play-stop", playId: 7 });
});

test("live startup carries selected long numeric latency and does not time out before future arm", async () => {
  const armed = 20000000000n;
  const h = await authorityActive({ activationHost: armed,
    startRequest: startRequest({ latencyHint: 60 }) });
  const game = h.games[0];
  h.setWindowNowNs(1000000000n);
  await h.send(authorityStep({ tickId: 1, nowNs: 1000000000n, watermark: null }));
  assert.equal(h.of("play-error").length, 0);
  assert.equal(game.processedInput.length, 0);
  h.setWindowNowNs(89999000000n);
  await h.send(authorityStep({ tickId: 2, nowNs: 89999000000n, watermark: 89999000000n }));
  assert.equal(h.of("play-error").length, 0, "selected 60s latency grants 70s startup after arm");
  h.setWindowNowNs(90000000000n);
  await h.send(authorityStep({ tickId: 3, nowNs: 90000000000n, watermark: 90000000000n }));
  assert.match(h.of("play-error").at(-1).message, /clock.*unavailable|unavailable.*clock/i);
  assert.equal(game.processedInput.length, 0);
});

function rawStep(fields = {}) {
  return { kind: "play-step", playId: 7, tickId: 1, events: [], watermark: ORIGIN, contextFrame: 48000n,
    nowNs: fields.watermark ?? ORIGIN, ...fields };
}
function rawObservation(fields = {}) {
  return { kind: "play-render", playId: 7, renderId: 1, timestamp: null, observedNowMs: 1000, ...fields };
}

test("Worker projects raw frame and presentation observations without replacing Window provenance across awaits or repeated outputs", async () => {
  const armed = 29030406000n; // One week plus 125 ms at the actual 48 kHz grid.
  const { h, port, game } = await directActive({ activationFrame: armed });
  const raw = rawObservation({ timestamp: { contextTime: 604800.25, performanceTime: 9007199254.75 },
    observedNowMs: 9007199255 });
  await h.send(raw);
  assert.equal(port.posts.at(-1).kind, "poll");
  raw.timestamp.contextTime = 0; raw.timestamp.performanceTime = 0; raw.observedNowMs = 0;
  await h.send(rawStep({ contextFrame: armed + 4800n,
    events: [{ hostNs: ORIGIN + 7n, key: 2, down: true, sequence: 9007199254740993n }], watermark: ORIGIN + 9n }));
  assert.deepEqual(game.calls.find(row => row[0] === "input"),
    ["input", ORIGIN + 7n, 2, true, 9007199254740993n, h.acquisitionNowNs()]);
  assert.deepEqual(game.calls.find(row => row[0] === "close"), ["close", ORIGIN + 9n]);
  assert.equal(h.of("play-step-done").at(-1).commandsPending, true, "the in-flight report has not probed new core work yet");
  assert.equal(game.calls.filter(row => row[0] === "output").length, 0);
  await port.acknowledge({ report: renderReport({ start: armed }) });
  assert.equal(game.calls.find(row => row[0] === "output")[2], 125000000n);
  assert.deepEqual(game.calls.find(row => row[0] === "presentation"), ["presentation", 125000000n, 9007199254750000n]);
  assert.equal(h.of("play-render-done").at(-1).observedTick, 1);
  assert.equal(h.of("play-render-done").at(-1).commandsPending, false);
  let renderId = 1;
  for (const [timestamp, now, output, host] of [
    [{ contextTime: 604800.25, performanceTime: 9007199254.875 }, 9007199255, null, null],
    [{ contextTime: 604800.2509765625, performanceTime: 9007199254.75 }, 9007199255, null, null],
    // Equal output above must not retain its newer host coordinate as fresh progress.
    [{ contextTime: 604800.2509765625, performanceTime: 9007199254.8125 }, 9007199255, 125976562n, 9007199254812500n],
    [{ contextTime: 604800.25, performanceTime: 9007199255 }, 9007199255, null, null],
    [{ contextTime: 604800.251953125, performanceTime: 9007199254.75 }, 9007199255, null, null],
    [{ contextTime: 604800.251953125, performanceTime: 9007199254.8125 }, 9007199255, null, null],
    [{ contextTime: 604800.251953125, performanceTime: 9007199255 }, 9007199255, 126953125n, 9007199255000000n],
    [{ contextTime: 604800.2529296875, performanceTime: 9007199256 }, 9007199255, null, null],
    [{ contextTime: 604800, performanceTime: 9007199255 }, 9007199255, null, null],
    [{ contextTime: 0, performanceTime: 0 }, 9007199255, null, null],
    [null, 9007199255, null, null],
  ]) {
    const before = game.calls.filter(row => row[0] === "presentation").length;
    await h.send(rawObservation({ renderId: ++renderId, timestamp, observedNowMs: now }));
    await port.acknowledge({ report: renderReport({ start: armed }) });
    assert.equal(game.calls.filter(row => row[0] === "output").at(-1)[2], output);
    const presented = game.calls.filter(row => row[0] === "presentation");
    assert.equal(presented.length, before + (output === null ? 0 : 1));
    if (output !== null) assert.deepEqual(presented.at(-1), ["presentation", output, host]);
    assert.equal(h.of("play-render-done").at(-1).observedTick, 1);
  }
  assert.equal(h.of("play-error").length, 0, "unavailable estimates retain accepted history within its finite lifetime");
  await h.send(rawObservation({ renderId: ++renderId,
    timestamp: { contextTime: 604800.2529296875, performanceTime: 9007199255 }, observedNowMs: 9007200255.125 }));
  await port.acknowledge({ report: renderReport({ start: armed }) });
  assert.match(h.of("play-error").at(-1).message, /clock.*unavailable|unavailable.*clock/i);
  assertReleased(h);
});

test("malformed or mixed raw observations refuse before input mutation or report polling and cancelled raw reads cannot revive owners", async () => {
  const missingFrame = rawStep(); delete missingFrame.contextFrame;
  const invalid = [missingFrame,
    ...[undefined, null, 1, -1n, 18446744073709551615n].map(contextFrame => rawStep({ contextFrame })),
    rawStep({ audioNs: undefined }), rawStep({ audioNs: 0n }),
    rawObservation({ timestamp: undefined }), rawObservation({ observedNowMs: undefined }),
    rawObservation({ observedNowMs: -1 }), rawObservation({ observedNowMs: Infinity }),
    rawObservation({ timestamp: { contextTime: NaN, performanceTime: 1 } }),
    rawObservation({ timestamp: { contextTime: 1, performanceTime: "1" } }),
    rawObservation({ timestamp: { contextTime: -1, performanceTime: 1 } }),
    rawObservation({ presentedNs: undefined }), rawObservation({ presentedHostNs: undefined }),
    rawObservation({ presentedNs: null, presentedHostNs: null }),
    directObservation({ observedNowMs: 1000 }),
  ];
  for (const request of invalid) {
    const { h, port, game } = await directActive({ activationFrame: 48000n });
    await h.send(request);
    assert.equal(h.of("play-error").length, 1); assert.equal(port.posts.length, 0);
    assert.deepEqual(game.calls.filter(row => ["input", "close", "output", "presentation", "ack"].includes(row[0])), []);
    assertReleased(h);
  }
  for (const mode of ["live", "replay"]) {
    const { h, port, game } = await directActive({ activationFrame: 48000n,
      ...(mode === "replay" ? { startRequest: replayRequest(replayFile().file) } : {}) });
    const stale = port.onmessage;
    await h.send(rawObservation({ timestamp: { contextTime: 1.5, performanceTime: 1499.875 }, observedNowMs: 1500 }));
    await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
    await h.send(startRequest({ playId: 8 }));
    const calls = game.calls.length, messages = h.messages.length;
    stale({ data: { kind: "ack", generation: 7, sequence: 1, operation: "poll", status: 0,
      admitted: 0, error: null, report: renderReport({ start: 48000n }) } });
    await flushJobs();
    assert.equal(game.calls.length, calls); assert.equal(h.messages.length, messages);
    assert.equal(h.games.at(-1).stops, 0);
    await h.send({ kind: "play-stop", playId: 8 });
  }
});

test("saved prefixes refresh the Worker HUD at most four times per second and export one final full-width snapshot before disposal", async () => {
  const first = replayFile(), second = replayFile();
  const opponents = [{ file: first.file, sourceKey: "file:own", own: true, label: "Own 曲" },
    { file: second.file, sourceKey: "file:other", own: false, label: "<other>" }];
  const maximum = 18446744073709551615n;
  const h = await active({ allowNetworkClock: true, startRequest: startRequest({ opponents, recordReplay: true }),
    savedSnapshot: game => game.saved.map((value, index) => ({ kind: value.own ? "own" : "other", label: value.label,
      songNs: game.score.song_ns, recordedUntilNs: index === 0 ? -1n : null,
      hits: maximum, misses: maximum, combo: maximum, maxCombo: maximum })) });
  const game = h.games[0];
  await h.send(step()); assert.equal(game.savedReads, 1);
  h.setNetworkNow(1249); await h.send(step({ tickId: 2, watermark: ORIGIN + 1n }));
  assert.equal(game.savedReads, 1);
  h.setNetworkNow(1250); await h.send(step({ tickId: 3, watermark: ORIGIN + 2n }));
  assert.equal(game.savedReads, 2);
  assert.equal(h.of("play-opponents").length, 0);
  assert.equal(h.of("play-step-done").length, 3);
  game.score.song_ns = SCORE.song_ns + 123n;
  await h.send({ kind: "play-stop", playId: 7 });
  const stopped = h.of("play-stopped").at(-1);
  assert.equal(game.savedReads, 3, "final prefix is freshly captured once, even inside the cadence interval");
  assert.deepEqual(game.disposals.slice(-4), ["opponents", "stop", "take", "free"]);
  assert.equal(stopped.savedOpponents.error, null);
  assert.deepEqual(Array.from(stopped.savedOpponents.opponents, row => [row.kind, row.label, row.songNs,
    row.recordedUntilNs, row.hits, row.misses, row.combo, row.maxCombo]), [
    ["own", "Own 曲", SCORE.song_ns + 123n, -1n, maximum, maximum, maximum, maximum],
    ["other", "<other>", SCORE.song_ns + 123n, null, maximum, maximum, maximum, maximum],
  ]);
  assert.equal(stopped.replayError, null); assert.equal(stopped.replayComplete, false);
  assert.ok(stopped.replay instanceof Uint8Array);
  assertReleased(h, { ...SCORE, song_ns: SCORE.song_ns + 123n });
  const messageCount = h.messages.length;
  await h.send({ kind: "play-stop", playId: 7 }); await h.send(step({ tickId: 4 }));
  assert.equal(h.messages.length, messageCount); assert.equal(game.savedReads, 3);

  const unavailable = await catalogWorker({ missingDisableSavedHud: true });
  await unavailable.send(startRequest({ opponents }));
  assert.ok(unavailable.of("play-reply").at(-1).error);
  assert.equal(unavailable.games.length, 0);
  assert.equal(unavailable.preparedOwners.at(-1).frees, 1, "capability refusal leaves prepared ownership unconsumed");
  await unavailable.send(startRequest({ playId: 8 }));
  assert.equal(unavailable.of("play-reply").at(-1).result.opponentCount, 0);
  await unavailable.send({ kind: "play-stop", playId: 8 });
  assert.equal(unavailable.games[0].savedReads, 0);
  assert.equal(Object.hasOwn(unavailable.of("play-stopped").at(-1), "savedOpponents"), false);
  const replay = await started({ startRequest: replayRequest(replayFile().file) });
  await replay.send({ kind: "play-stop", playId: 7 });
  assert.equal(replay.replays[0].savedReads, 0);
  assert.equal(Object.hasOwn(replay.of("play-stopped").at(-1), "savedOpponents"), false);
});

test("comparison errors hide the HUD once and remain separate from actual local failure, capture and final cleanup", async () => {
  for (const failure of ["getter", "validation", "final"]) {
    const selected = replayFile();
    const options = { allowNetworkClock: true, startRequest: startRequest({ recordReplay: true,
      opponents: [{ file: selected.file, sourceKey: "file:1", own: true, label: "own" }] }) };
    if (failure !== "validation") options.savedError = "actual comparison prefix failure";
    else options.savedSnapshot = () => [{ kind: "own", label: "own", songNs: 0n, recordedUntilNs: null,
      hits: 0n, misses: 0n, combo: 1n, maxCombo: 1n }];
    if (failure === "getter") options.disableSavedError = "HUD disable failed";
    const h = await active(options), game = h.games[0];
    if (failure !== "final") {
      await h.send(step());
      assert.equal(h.of("play-opponents").length, 1);
      assert.equal(h.of("play-opponents")[0].opponents, null);
      assert.equal(game.hudDisables, 1); assert.equal(game.savedReads, 1);
      // A later successful binding cannot resurrect an already failed comparison owner.
      delete options.savedError; options.savedSnapshot = undefined;
      h.setNetworkNow(1500); await h.send(step({ tickId: 2, watermark: ORIGIN + 1n }));
      assert.equal(game.savedReads, 1); assert.equal(h.of("play-opponents").length, 1);
      assert.equal(h.of("play-error").length, 0); assert.equal(game.stops, 0);
    }
    await h.send({ kind: "play-stop", playId: 7 });
    const stopped = h.of("play-stopped").at(-1);
    assert.equal(stopped.savedOpponents.opponents, null);
    assert.ok(stopped.savedOpponents.error.length > 0);
    if (failure === "getter") assert.match(stopped.savedOpponents.error, /actual comparison prefix failure.*HUD disable failed/);
    assert.equal(game.savedReads, 1); assert.equal(game.hudDisables, 1);
    assert.equal(stopped.replayError, null); assert.ok(stopped.replay instanceof Uint8Array);
    assert.equal(stopped.replayComplete, false); assertReleased(h);
  }
  const selected = replayFile();
  const h = await active({ allowNetworkClock: true, freeError: "actual free failure",
    service(game) { if (game.processedInput.length === 1) throw new Error("actual committed local input failure"); },
    startRequest: startRequest({ recordReplay: true,
      opponents: [{ file: selected.file, sourceKey: "file:1", own: false, label: "prefix" }] }) });
  await h.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n, audioNs: 100000000n }] }));
  assert.equal(h.games[0].processedInput.length, 0);
  await legacyWitness(h, 1, 100n, ORIGIN);
  await legacyWitness(h, 2, 101n, ORIGIN + 1n);
  const failed = h.of("play-error").at(-1), game = h.games[0];
  assert.equal(game.processedInput.length, 1, "the error follows actual processing rather than admission");
  assert.match(failed.message, /actual committed local input failure/);
  assert.equal(failed.released, false); assert.equal(failed.savedOpponents.error, null);
  assert.equal(failed.savedOpponents.opponents[0].label, "prefix");
  assert.deepEqual(game.disposals, ["opponents", "opponents", "stop", "take", "free"]);
  assert.equal(game.savedReads, 2, "initial HUD read and fresh final accepted-prefix capture remain separate");
  assert.equal(failed.replayComplete, false);
  assert.equal(failed.replayError, null); assertReleased(h);
  await h.send(step({ tickId: 2 }));
  assert.equal(game.savedReads, 2); assert.equal(h.of("play-error").length, 1);
});

const POINTER_MOUSE = 0xfedcba9876543210n;
const POINTER_PEN = 18446744073709551615n;
function pointerSetup(rows = [[0x11, POINTER_MOUSE, 0], [0x11, POINTER_MOUSE, 1],
  [0x12, POINTER_PEN, 0], [0x12, POINTER_PEN, 32]], devices = [
  { source: POINTER_MOUSE, pointerType: "mouse" }, { source: POINTER_PEN, pointerType: "pen" },
]) {
  return { devices, bindingWords: new Uint32Array(rows.flatMap(([lane, source, control]) =>
    [lane, Number(source & 0xffffffffn), Number(source >> 32n), control])) };
}
function pointerEvent(fields = {}) {
  return { kind: "pointer", pointerType: "mouse", source: POINTER_MOUSE, hostNs: ORIGIN,
    sequence: 1n, code: 0x12345678, control: 0, mode: 1, x: -0, y: -12.5, ...fields };
}
function pointerButton(fields = {}) {
  return { kind: "pointer-button", pointerType: "mouse", source: POINTER_MOUSE, hostNs: ORIGIN,
    sequence: 2n, code: 0xffffffff, control: 1, state: 0, ...fields };
}
async function pointerActive(fields = {}, options = {}) {
  const h = await started({ ...options, startRequest: startRequest({ inputMode: "physical", pointerSetup: pointerSetup(), ...fields }) });
  assert.equal((await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START })).result, null);
  return h;
}

test("Worker snapshots pointer namespaces and sends original mixed acquisitions through the actual solo canonical routes", async () => {
  for (const inputMode of ["physical", "physical-contact"]) {
    const raw = pointerSetup(), h = await catalogWorker();
    h.post(startRequest({ inputMode, pointerSetup: raw, hidSetup: hidSetup([3n]), recordReplay: true }));
    raw.devices[0].source = 4n; raw.devices[1].pointerType = "mouse"; raw.bindingWords.fill(0);
    await flushJobs();
    const prepared = h.of("play-reply").at(-1).result;
    assert.equal(prepared.kind, "prepared");
    assert.deepEqual(prepared.pointerDevices, [{ source: POINTER_MOUSE, pointerType: "mouse" }, { source: POINTER_PEN, pointerType: "pen" }]);
    assert.deepEqual(prepared.hidSources, [3n]);
    const constructor = (inputMode === "physical" ? h.physicalConstructions : h.contactConstructions)[0];
    assert.deepEqual(Array.from(constructor.args[5].slice(-28)), [
      0x11, 1, 0x76543210, 0xfedcba98, 1, 0x574d4f55, 0,
      0x11, 1, 0x76543210, 0xfedcba98, 1, 0x574d4f55, 1,
      0x12, 1, 0xffffffff, 0xffffffff, 1, 0x5750454e, 0,
      0x12, 1, 0xffffffff, 0xffffffff, 1, 0x5750454e, 32,
    ]);
    await h.send({ kind: "play-activate", playId: 7, rpcId: 2, hostNs: ORIGIN, startFrame: START });
    const mouse = pointerEvent({ sequence: 18446744073709551614n });
    const repeat = pointerButton({ sequence: 18446744073709551615n, state: 2 });
    const pen = pointerButton({ source: POINTER_PEN, pointerType: "pen", control: 32, code: 7,
      hostNs: ORIGIN + 1n, sequence: 18446744073709551614n });
    const absolute = pointerEvent({ source: POINTER_PEN, pointerType: "pen", mode: 0, x: 1.5, y: -2.25,
      hostNs: ORIGIN + 1n, sequence: 18446744073709551615n });
    const key = { hostNs: ORIGIN + 3n, key: 2, down: true, sequence: 100n };
    const events = [key, mouse, pen, hidEvent({ source: 3n, hostNs: ORIGIN + 2n, sequence: 0n }), repeat, absolute];
    if (inputMode === "physical-contact") events.push(touchEvent({ hostNs: ORIGIN + 2n, sequence: 9n }));
    await h.send(step({ events, watermark: ORIGIN + 3n, audioNs: 100000003n }));
    const game = h.games[0], calls = inputCalls(game);
    assert.deepEqual(calls.map(call => call[0]), inputMode === "physical-contact"
      ? ["blob", "blob", "blob", "blob", "hid", "touch", "blob", "close"]
      : ["blob", "blob", "blob", "blob", "hid", "blob", "close"]);
    const expected = [
      [77, 3, POINTER_MOUSE, ORIGIN, 18446744073709551614n, 0x574d4f55, 0x12345678, 0],
      [69, 0, POINTER_MOUSE, ORIGIN, 18446744073709551615n, 0x574d4f55, 0xffffffff, 1],
      [69, 0, POINTER_PEN, ORIGIN + 1n, 18446744073709551614n, 0x5750454e, 7, 32],
      [77, 3, POINTER_PEN, ORIGIN + 1n, 18446744073709551615n, 0x5750454e, 0x12345678, 0],
    ];
    for (let index = 0; index < 4; index++) {
      const bytes = calls[index][1], packet = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      assert.deepEqual([bytes.length, bytes[6], packet.getBigUint64(7, true), packet.getBigInt64(15, true),
        packet.getBigUint64(27, true), packet.getUint32(36, true), packet.getUint32(41, true), packet.getUint32(64, true)], expected[index]);
      assert.equal(packet.getUint32(23, true), 0x57494e); assert.equal(packet.getUint32(46, true), 0x57494e);
      assert.equal(packet.getBigInt64(50, true), expected[index][3]);
      assert.equal(bytes[58], 0); assert.equal(bytes[59], 1); assert.equal(packet.getUint32(60, true), expected[index][5]);
    }
    assert.equal(new DataView(calls[0][1].buffer).getUint32(68, true), 0x80000000);
    assert.equal(new DataView(calls[0][1].buffer).getFloat32(72, true), -12.5); assert.equal(calls[0][1][76], 1);
    assert.equal(calls[1][1][68], 2); assert.equal(calls[2][1][68], 0);
    assert.equal(new DataView(calls[3][1].buffer).getFloat32(68, true), 1.5);
    assert.equal(new DataView(calls[3][1].buffer).getFloat32(72, true), -2.25); assert.equal(calls[3][1][76], 0);
    assert.ok(calls.filter(call => call[0] !== "close").every(call => call.at(-1) === h.acquisitionNowNs()));
    assert.equal(game.lastService.audio, 100000003n);
    assert.equal(h.of("play-step-done").at(-1).hits, SCORE.hits, "scripted binding scores do not claim position-based judgment");
    await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
  }
});

test("exact local pointer rosters retain player-qualified rows and exclude unassigned sources without position-only press coverage", async () => {
  const rows = [[0x11, POINTER_MOUSE, 0], [0x11, POINTER_MOUSE, 1], [0x12, POINTER_MOUSE, 2],
    [0x11, POINTER_PEN, 31], [0x12, POINTER_PEN, 32], [0x11, 3n, 1], [0x12, 3n, 2]];
  const devices = [{ source: POINTER_MOUSE, pointerType: "mouse" }, { source: POINTER_PEN, pointerType: "pen" },
    { source: 3n, pointerType: "mouse" }];
  const h = await started({ startRequest: localRequest({ inputMode: "physical", hidSetup: undefined,
    keyPairs: new Uint32Array(), localPlanWords: localPlan([[99, POINTER_MOUSE], [0xffffffff, POINTER_PEN]]),
    pointerSetup: pointerSetup(rows, devices) }) });
  const prepared = h.of("play-reply")[0].result;
  assert.deepEqual(prepared.localPlayers, [99, 0xffffffff]);
  assert.deepEqual(prepared.pointerDevices, devices.slice(0, 2));
  assert.equal(h.games.length, 0); assert.equal(h.locals.length, 1);
  assert.deepEqual(Array.from(h.localConstructions[0].args[6]), [
    99, 0x11, 1, 0x76543210, 0xfedcba98, 1, 0x574d4f55, 0,
    99, 0x11, 1, 0x76543210, 0xfedcba98, 1, 0x574d4f55, 1,
    99, 0x12, 1, 0x76543210, 0xfedcba98, 1, 0x574d4f55, 2,
    0xffffffff, 0x11, 1, 0xffffffff, 0xffffffff, 1, 0x5750454e, 31,
    0xffffffff, 0x12, 1, 0xffffffff, 0xffffffff, 1, 0x5750454e, 32,
  ]);
  assert.equal((await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START })).result, null);
  await h.send(step({ events: [pointerEvent(), pointerButton({ source: POINTER_PEN, pointerType: "pen", control: 32, sequence: 0n })] }));
  const game = h.locals[0];
  assert.deepEqual(inputCalls(game).map(call => call[0]), ["blob", "blob", "close"]);
  assert.deepEqual(inputCalls(game).slice(0, 2).map(call => new DataView(call[1].buffer).getBigUint64(7, true)), [POINTER_MOUSE, POINTER_PEN]);
  const before = inputCalls(game).length;
  await h.send(step({ tickId: 2, events: [pointerButton(), pointerButton({ source: 3n, sequence: 0n })] }));
  assert.equal(inputCalls(game).length, before, "an unassigned configured source refuses the whole following batch");
  assert.equal(game.stops, 1); assert.equal(game.frees, 1); assert.equal(h.of("play-error").length, 1);

  const absent = await started({ startRequest: localRequest({ hidSetup: undefined,
    localPlanWords: localPlan([[9, 1n], [10, 2n]]), pointerSetup: pointerSetup() }) });
  assert.deepEqual(absent.of("play-reply")[0].result.pointerDevices, []);
  await absent.send({ kind: "play-stop", playId: 7 }); assert.equal(absent.locals[0].frees, 1);
  const uncovered = await catalogWorker();
  await uncovered.send(localRequest({ inputMode: "physical", hidSetup: undefined, keyPairs: new Uint32Array(),
    localPlanWords: localPlan([[99, POINTER_MOUSE], [7, POINTER_PEN]]),
    pointerSetup: pointerSetup([[0x11, POINTER_MOUSE, 1], [0x12, POINTER_MOUSE, 0], [0x11, POINTER_PEN, 1], [0x12, POINTER_PEN, 2]]) }));
  assert.equal(uncovered.locals.length, 0); assert.equal(uncovered.of("play-error").length, 1);
  assert.equal(uncovered.preparedOwners.at(-1).moved, false); assert.equal(uncovered.preparedOwners.at(-1).frees, 1);
});

test("pointer setup rejects replay, source collisions and combined overflow before consuming a chart while the exact capacity remains usable", async () => {
  const wideDevices = Array.from({ length: 8 }, (_, index) => ({ source: BigInt(index + 10), pointerType: index % 2 ? "pen" : "mouse" }));
  const wideRows = wideDevices.flatMap(({ source }) => Array.from({ length: 32 }, (_, index) => [index % 2 ? 0x12 : 0x11, source, index + 1]));
  const requests = [startRequest({ pointerSetup: pointerSetup() }), replayRequest(replayFile().file, { pointerSetup: pointerSetup() }),
    startRequest({ inputMode: "physical", pointerSetup: pointerSetup(), hidSetup: hidSetup([POINTER_MOUSE]) }),
    startRequest({ inputMode: "physical", gamepadSetup: gamepadSetup(), pointerSetup: pointerSetup([[0x11, GAMEPAD_SOURCE, 1]],
      [{ source: GAMEPAD_SOURCE, pointerType: "mouse" }]) }),
    startRequest({ inputMode: "physical", pointerSetup: pointerSetup(wideRows, wideDevices) }),
    startRequest({ inputMode: "physical", keyPairs: new Uint32Array(), pointerSetup: pointerSetup([[0x11, POINTER_MOUSE, 1], [0x12, POINTER_PEN, 0]]) }),
  ];
  for (const request of requests) {
    const h = await catalogWorker();
    const previews = h.preparedOwners.map(owner => ({ owner, moved: owner.moved, frees: owner.frees }));
    await h.send(request);
    assert.equal(h.games.length + h.locals.length + h.replays.length, 0);
    assert.equal(h.physicalConstructions.length + h.contactConstructions.length + h.localConstructions.length, 0);
    assert.equal(h.of("play-error").length, 1);
    assert.equal(h.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
    for (const { owner, moved, frees } of previews) {
      assert.equal(owner.moved, moved); assert.equal(owner.frees, frees);
    }
    assert.ok(h.preparedOwners.slice(previews.length).every(owner => !owner.moved && owner.frees === 1),
      "refused gameplay preparation is never consumed and is released exactly once");
  }
  const exact = await started({ startRequest: startRequest({ inputMode: "physical", keyPairs: new Uint32Array(),
    pointerSetup: pointerSetup(wideRows, wideDevices) }) });
  assert.equal(exact.physicalConstructions[0].args[5].length, 1792);
  assert.deepEqual(exact.of("play-reply")[0].result.pointerDevices, wideDevices);
  await exact.send({ kind: "play-stop", playId: 7 }); assertReleased(exact);
  for (const options of [{ missingInputBlob: true }, { missingPhysicalConstructor: true }]) {
    const h = await catalogWorker(options);
    await h.send(startRequest({ inputMode: "physical", pointerSetup: pointerSetup() }));
    assert.equal(h.games.length, 0); assert.equal(h.of("play-error").length, 1);
  }
  const ordinary = await started({ startRequest: startRequest({ inputMode: "physical" }) });
  assert.equal(Object.hasOwn(ordinary.of("play-reply")[0].result, "pointerDevices"), false);
  await ordinary.send({ kind: "play-stop", playId: 7 }); assertReleased(ordinary);
});

test("pointer batches keep atomic validation, original source order, fanout and stopped-owner fences", async () => {
  const invalid = [pointerEvent({ source: 3n }), pointerEvent({ pointerType: "pen" }), pointerEvent({ control: 1 }),
    pointerEvent({ x: Infinity }), pointerEvent({ y: 3.5e38 }), pointerEvent({ mode: 2 }), pointerEvent({ sequence: -1n }),
    pointerEvent({ code: 4294967296 }), pointerButton({ control: 0 }), pointerButton({ control: 2 }),
    pointerButton({ state: 3 }), pointerButton({ source: Number(POINTER_MOUSE) })];
  for (const bad of invalid) {
    const h = await pointerActive();
    await h.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 0n }, bad] }));
    assert.equal(inputCalls(h.games[0]).length, 0); assert.equal(h.of("play-step-done").length, 0); assertReleased(h);
  }
  for (const events of [[pointerEvent({ sequence: 1n }), pointerButton({ sequence: 0n })],
    [pointerEvent({ hostNs: ORIGIN + 1n }), pointerButton({ hostNs: ORIGIN })],
    [pointerEvent({ hostNs: ORIGIN + 2n })]]) {
    const h = await pointerActive();
    await h.send(step({ events, watermark: ORIGIN + 1n }));
    assert.equal(inputCalls(h.games[0]).length, 0); assertReleased(h);
  }
  const prior = await pointerActive();
  await prior.send(step({ events: [pointerEvent({ hostNs: ORIGIN - 1n })] }));
  assert.equal(prior.of("play-step-done").at(-1).preOriginInputs, 1);
  assert.deepEqual(inputCalls(prior.games[0]).map(call => call[0]), ["close"]);
  await prior.send(step({ tickId: 2, events: [pointerButton({ sequence: 2n })], watermark: ORIGIN + 10n }));
  const committedCalls = inputCalls(prior.games[0]).length;
  await prior.send(step({ tickId: 3, events: [pointerEvent({ sequence: 3n, hostNs: ORIGIN + 5n })], watermark: ORIGIN + 10n }));
  assert.equal(inputCalls(prior.games[0]).length, committedCalls, "new pointer bytes cannot rewind the committed watermark");
  assertReleased(prior);

  const many = Array.from({ length: 256 }, (_, index) => pointerEvent({ sequence: BigInt(index), x: index }));
  const exact = await pointerActive(); await exact.send(step({ events: many }));
  assert.equal(inputCalls(exact.games[0]).filter(call => call[0] === "blob").length, 256);
  assert.equal(new DataView(inputCalls(exact.games[0])[255][1].buffer).getBigUint64(27, true), 255n);
  await exact.send({ kind: "play-stop", playId: 7 }); assertReleased(exact);
  const padRows = Array.from({ length: 127 }, (_, index) => [0x11, 0, index]);
  for (const overflow of [false, true]) {
    const mixed = await gamepadActive({}, { pointerSetup: pointerSetup(), gamepadSetup: gamepadSetup(padRows, 128, 0) });
    const buttons = pressed => Array.from({ length: 128 }, () => ({ value: pressed ? 1 : 0, pressed, touched: false }));
    const events = [gamepadEvent({ axes: [], buttons: buttons(true) }), pointerEvent({ hostNs: GAMEPAD_HOST }),
      gamepadEvent({ axes: [], buttons: buttons(false), sequence: 2n }), pointerButton({ hostNs: GAMEPAD_HOST })];
    if (overflow) events.push(pointerButton({ hostNs: GAMEPAD_HOST, source: POINTER_PEN, pointerType: "pen", control: 32, sequence: 0n }));
    await mixed.send(step({ events, watermark: GAMEPAD_HOST }));
    assert.equal(inputCalls(mixed.games[0]).filter(call => call[0] === "blob").length, overflow ? 0 : 256,
      "pointer packets share the post-Gamepad-expansion cap, even when the acquired event array is small");
    if (overflow) assert.equal(inputCalls(mixed.games[0]).length, 0);
    else await mixed.send({ kind: "play-stop", playId: 7 });
    assertReleased(mixed);
  }
  const excess = await pointerActive();
  const rejected = step({ events: [...many, pointerButton({ sequence: 256n })] });
  await excess.send(rejected); assert.equal(inputCalls(excess.games[0]).length, 0); assertReleased(excess);
  const count = excess.of("play-error").length;
  await excess.send(rejected); assert.equal(excess.of("play-error").length, count);
  await excess.send(startRequest({ playId: 8, inputMode: "physical" }));
  await excess.send({ kind: "play-activate", playId: 8, rpcId: 2, hostNs: ORIGIN, startFrame: START });
  const replacement = excess.games[1];
  await excess.send(step({ events: [pointerEvent()] }));
  assert.equal(inputCalls(replacement).length, 0, "stale batches never touch the replacement binding");
  await excess.send(step({ playId: 8, events: [pointerEvent()] }));
  assert.equal(inputCalls(replacement).length, 0, "disposed pointer admission is not inherited by a plain new session");
  assert.equal(replacement.stops, 1); assert.equal(replacement.frees, 1);
});

const GAMEPAD_SOURCE = 0x8877665544332211n;
const GAMEPAD_HOST = 1000000000n;
function gamepadSetup(rows = [[0x11, 0, 0], [0x12, 0, 1]], buttons = 2, axes = 1) {
  return { devices: [{ source: GAMEPAD_SOURCE, buttons, axes }],
    bindingWords: new Uint32Array(rows.flatMap(([lane, type, index]) =>
      [lane, 0x44332211, 0x88776655, type, index])) };
}
function gamepadEvent(fields = {}) {
  return { kind: "gamepad", source: GAMEPAD_SOURCE, index: 7, id: "genuine browser descriptor", mapping: "standard",
    timestampMs: 1000, hostNs: GAMEPAD_HOST, sequence: 1n, axes: [0.25],
    buttons: [{ value: 0.5, pressed: true, touched: true }, { value: 0, pressed: false, touched: false }], ...fields };
}
async function gamepadActive(options = {}, fields = {}) {
  const h = await started({ ...options, startRequest: startRequest({ inputMode: "physical", gamepadSetup: gamepadSetup(), ...fields }) });
  assert.equal((await h.rpc("play-activate", { hostNs: GAMEPAD_HOST, startFrame: START })).result, null);
  return h;
}

test("Worker snapshots exact Gamepad native bindings and sends mixed physical fanout through the genuine canonical blob route", async () => {
  const original = gamepadSetup([[0x11, 0, 0], [0x12, 0, 1], [0x11, 1, 0], [0x11, 2, 0], [0x11, 3, 0]]);
  const h = await catalogWorker();
  h.post(startRequest({ inputMode: "physical-contact", gamepadSetup: original, hidSetup: hidSetup([3n]), recordReplay: true }));
  original.devices[0].source = 3n; original.devices[0].buttons = 0; original.bindingWords.fill(0);
  await flushJobs();
  const prepared = h.of("play-reply").at(-1).result;
  assert.equal(prepared.kind, "prepared");
  assert.equal(prepared.inputMode, "physical-contact");
  assert.deepEqual(prepared.gamepadSources, [GAMEPAD_SOURCE]);
  assert.deepEqual(prepared.hidSources, [3n]);
  const actualWords = h.contactConstructions[0].args[5];
  assert.deepEqual(Array.from(actualWords.slice(-35)), [
    0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0,
    0x12, 1, 0x44332211, 0x88776655, 1, 0x57475044, 1,
    0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x10000,
    0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x20000,
    0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x30000,
  ]);
  const game = h.games[0];
  assert.ok(game.calls.findIndex(call => call[0] === "hid-setup") < game.calls.findIndex(call => call[0] === "capture"));
  assert.equal(game.calls.filter(call => call[0] === "sample").length, 0);
  await h.send({ kind: "play-activate", playId: 7, rpcId: 2, hostNs: GAMEPAD_HOST, startFrame: START });
  const sequence = 0x0102030405060708n;
  const key = { hostNs: GAMEPAD_HOST, key: 2, down: true, sequence };
  const acquired = gamepadEvent({ sequence: sequence + 1n });
  const touch = touchEvent({ hostNs: GAMEPAD_HOST + 1n, sequence: sequence + 2n });
  const hid = hidEvent({ source: 3n, hostNs: GAMEPAD_HOST + 2n, sequence: sequence + 3n });
  const audioNs = 9007199254741222n;
  await h.send(step({ events: [key, acquired, touch, hid], watermark: GAMEPAD_HOST + 3n, audioNs }));
  const calls = inputCalls(game);
  assert.deepEqual(calls.map(call => call[0]), ["blob", "blob", "blob", "blob", "blob", "touch", "hid", "close"]);
  assert.deepEqual(Array.from(calls[0][1]), Array.from(encodeKeyboardEvent(key)));
  const gamepadPackets = calls.slice(1, 5).map(call => call[1]);
  assert.deepEqual(gamepadPackets.map(bytes => new DataView(bytes.buffer).getUint32(64, true)), [0, 0x10000, 0x20000, 0x30000]);
  assert.deepEqual(gamepadPackets.map(bytes => bytes[6]), [0, 1, 1, 0]);
  assert.deepEqual(gamepadPackets.map(bytes => bytes.length), [69, 73, 73, 69]);
  for (const bytes of gamepadPackets) {
    const packet = new DataView(bytes.buffer);
    assert.deepEqual(Array.from(bytes.slice(0, 6)), [0x42, 0x4b, 0x50, 0x49, 1, 0]);
    assert.equal(packet.getBigUint64(7, true), GAMEPAD_SOURCE);
    assert.equal(packet.getBigInt64(15, true), GAMEPAD_HOST);
    assert.equal(packet.getUint32(23, true), 0x57494e);
    assert.equal(packet.getBigUint64(27, true), sequence + 1n);
    assert.equal(packet.getUint32(36, true), 0x57475044);
    assert.equal(packet.getUint32(41, true), packet.getUint32(64, true));
    assert.equal(packet.getBigInt64(50, true), GAMEPAD_HOST);
    assert.equal(bytes[58], 0); assert.equal(bytes[59], 1);
    assert.equal(packet.getUint32(60, true), 0x57475044);
  }
  assert.equal(new DataView(gamepadPackets[1].buffer).getFloat32(68, true), 0.25);
  assert.equal(new DataView(gamepadPackets[2].buffer).getFloat32(68, true), 0.5);
  assert.deepEqual(Array.from(calls[5][1]), Array.from(encodeTouchEvent(touch)));
  assert.deepEqual(Array.from(calls[6][1]), Array.from(encodeRawHidEvent(hid)));
  assert.ok(calls.filter(call => call[0] !== "close").every(call => call.at(-1) === h.acquisitionNowNs()));
  assert.equal(game.lastService.audio, audioNs);
  assert.equal(h.of("play-step-done").at(-1).hits, SCORE.hits, "the routing fixture does not manufacture judge results");
  await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);

  const only = await gamepadActive({}, { keyPairs: new Uint32Array() });
  assert.deepEqual(only.of("play-reply")[0].result.gamepadSources, [GAMEPAD_SOURCE]);
  assert.equal(only.physicalConstructions[0].args[5].length, 14, "pressed Gamepad bindings alone cover the actual prepared lanes");
  await only.send(step({ events: [gamepadEvent()], watermark: GAMEPAD_HOST }));
  assert.deepEqual(inputCalls(only.games[0]).map(call => call[0]), ["blob", "close"]);
  await only.send({ kind: "play-stop", playId: 7 }); assertReleased(only);
  const ordinary = await started({ startRequest: startRequest({ inputMode: "physical" }) });
  assert.equal(Object.hasOwn(ordinary.of("play-reply")[0].result, "gamepadSources"), false);
  await ordinary.send({ kind: "play-stop", playId: 7 });
});

test("touched-only numeric and file profiles cover prepared lanes and route original Button transitions without keyboard fallback", async () => {
  for (const fromFile of [false, true]) {
    const selected = selectedGamepadProfile(null, new TextEncoder().encode(JSON.stringify({ version: 1,
      profiles: [{ id: "custom pad", mapping: "", bindingWords: [0x11, 3, 0, 0x12, 3, 1] }] })));
    const h = await catalogWorker();
    await h.send(startRequest({ inputMode: "physical", keyPairs: new Uint32Array(),
      ...(fromFile ? { gamepadProfileFile: selected.file, gamepadDevices: customGamepadDevices() }
        : { gamepadSetup: gamepadSetup([[0x11, 3, 0], [0x12, 3, 1]]) }) }));
    const prepared = h.of("play-reply").at(-1).result;
    assert.equal(prepared.kind, "prepared"); assert.deepEqual(prepared.lanes, [0x11, 0x12]);
    assert.deepEqual(prepared.gamepadSources, [GAMEPAD_SOURCE]);
    assert.deepEqual(Array.from(h.physicalConstructions[0].args[5]), [
      0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x30000,
      0x12, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x30001,
    ]);
    await h.send({ kind: "play-activate", playId: 7, rpcId: 2, hostNs: GAMEPAD_HOST, startFrame: START });
    const idle = gamepadEvent({ id: "custom pad", mapping: "",
      buttons: [{ value: 1, pressed: true, touched: false }, { value: 1, pressed: true, touched: false }] });
    await h.send(step({ events: [idle], watermark: GAMEPAD_HOST }));
    assert.deepEqual(inputCalls(h.games[0]).map(call => call[0]), ["close"]);
    const sequence = 0x0102030405060708n;
    const down = { ...idle, sequence,
      buttons: [{ value: 0, pressed: false, touched: false }, { value: 0, pressed: false, touched: true }] };
    await h.send(step({ tickId: 2, events: [down], watermark: GAMEPAD_HOST }));
    await h.send(step({ tickId: 3, events: [{ ...down, sequence: sequence + 1n }], watermark: GAMEPAD_HOST }));
    await h.send(step({ tickId: 4, events: [{ ...idle, sequence: sequence + 2n, timestampMs: 1001, hostNs: 1001000000n }],
      watermark: 1001000000n }));
    const calls = inputCalls(h.games[0]), packets = calls.filter(call => call[0] === "blob").map(call => call[1]);
    assert.deepEqual(calls.map(call => call[0]), ["close", "blob", "close", "close", "blob", "close"]);
    assert.deepEqual(packets.map(bytes => [bytes.length, bytes[6], new DataView(bytes.buffer).getUint32(64, true), bytes[68]]),
      [[69, 0, 0x30001, 0], [69, 0, 0x30001, 1]]);
    for (const [index, bytes] of packets.entries()) {
      const decoded = new DataView(bytes.buffer), hostNs = index === 0 ? GAMEPAD_HOST : 1001000000n;
      assert.equal(decoded.getBigUint64(7, true), GAMEPAD_SOURCE);
      assert.equal(decoded.getBigInt64(15, true), hostNs); assert.equal(decoded.getBigInt64(50, true), hostNs);
      assert.equal(decoded.getBigUint64(27, true), sequence + BigInt(index * 2));
      assert.equal(decoded.getUint32(36, true), 0x57475044); assert.equal(decoded.getUint32(41, true), 0x30001);
    }
    assert.equal(h.of("play-step-done").length, 4); assert.equal(selected.reads, fromFile ? 1 : 0);
    await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
  }
});

test("Worker refuses invalid Gamepad ownership or batches atomically and keeps pre-origin, stale-noop and physical fanout barriers", async () => {
  for (const request of [startRequest({ gamepadSetup: gamepadSetup() }),
    replayRequest(replayFile().file, { gamepadSetup: gamepadSetup() }),
    startRequest({ inputMode: "physical", gamepadSetup: gamepadSetup(), hidSetup: hidSetup([GAMEPAD_SOURCE]) })]) {
    const h = await catalogWorker();
    await h.send(request);
    assert.equal(h.games.length, 0); assert.equal(h.replays.length, 0);
    assert.equal(h.of("play-error").length, 1);
    assert.equal(h.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
  }
  const uncovered = await catalogWorker();
  await uncovered.send(startRequest({ inputMode: "physical", keyPairs: new Uint32Array(),
    gamepadSetup: gamepadSetup([[0x11, 0, 0], [0x12, 1, 0]]) }));
  assert.equal(uncovered.games.length, 0);
  assert.equal(uncovered.preparedOwners.at(-1).frees, 1, "axis binding cannot masquerade as ordinary press-lane coverage");
  const fullRows = Array.from({ length: 128 }, (_, index) => [0x11, 0, index])
    .concat(Array.from({ length: 128 }, (_, index) => [0x12, 3, index]));
  const combined = await catalogWorker();
  await combined.send(startRequest({ inputMode: "physical", gamepadSetup: gamepadSetup(fullRows, 128, 0) }));
  assert.equal(combined.games.length, 0, "256 valid profile rows plus keyboard rows exceed the common constructor cap");
  assert.equal(combined.of("play-error").length, 1);
  for (const options of [{ missingInputBlob: true }, { missingPhysicalConstructor: true }]) {
    const h = await catalogWorker(options);
    await h.send(startRequest({ inputMode: "physical", gamepadSetup: gamepadSetup() }));
    assert.equal(h.games.length, 0); assert.equal(h.preparedOwners.at(-1).frees, 1);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const bad of [gamepadEvent({ source: 4n }), gamepadEvent({ axes: [NaN] }),
    gamepadEvent({ hostNs: GAMEPAD_HOST + 1n }), gamepadEvent({ sequence: -1n })]) {
    const h = await gamepadActive();
    await h.send(step({ events: [{ hostNs: GAMEPAD_HOST, key: 2, down: true, sequence: 1n }, bad], watermark: GAMEPAD_HOST + 1n }));
    assert.equal(inputCalls(h.games[0]).length, 0);
    assert.equal(h.of("play-step-done").length, 0); assertReleased(h);
  }
  const atomic = await gamepadActive();
  const rejected = step({ events: [gamepadEvent(), { hostNs: GAMEPAD_HOST, key: 65535, down: true, sequence: 2n }], watermark: GAMEPAD_HOST });
  await atomic.send(rejected);
  assert.equal(inputCalls(atomic.games[0]).length, 0);
  const failureCount = atomic.of("play-error").length;
  await atomic.send(rejected); assert.equal(atomic.of("play-error").length, failureCount);
  await atomic.send(startRequest({ playId: 8, inputMode: "physical", gamepadSetup: gamepadSetup() }));
  await atomic.send({ kind: "play-activate", playId: 8, rpcId: 2, hostNs: GAMEPAD_HOST, startFrame: START });
  await atomic.send(rejected);
  assert.equal(inputCalls(atomic.games[1]).length, 0, "old owner batches cannot reach a newly prepared adapter");
  await atomic.send(step({ playId: 8, events: [gamepadEvent()], watermark: GAMEPAD_HOST }));
  assert.equal(inputCalls(atomic.games[1]).filter(call => call[0] === "blob").length, 1);
  await atomic.send({ kind: "play-stop", playId: 8 });

  const before = await gamepadActive();
  const held = gamepadEvent({ timestampMs: 999, hostNs: 999000000n });
  await before.send(step({ events: [held, { ...held, sequence: 2n }], watermark: GAMEPAD_HOST }));
  assert.deepEqual(inputCalls(before.games[0]).map(call => call[0]), ["close"]);
  assert.equal(before.of("play-step-done").at(-1).preOriginInputs, 2, "a pre-origin sample counts once even when it emits no transition");
  await before.send(step({ tickId: 2, events: [gamepadEvent({ sequence: 3n })], watermark: GAMEPAD_HOST }));
  assert.equal(inputCalls(before.games[0]).filter(call => call[0] === "blob").length, 0,
    "a held pre-origin level does not fabricate a new Down at activation");
  await before.send(step({ tickId: 3, events: [gamepadEvent({ sequence: 4n, timestampMs: 1001, hostNs: 1001000000n,
    buttons: [{ value: 0, pressed: false, touched: false }, { value: 0, pressed: false, touched: false }] })], watermark: 1001000000n }));
  const released = inputCalls(before.games[0]).filter(call => call[0] === "blob");
  assert.equal(released.length, 1); assert.equal(released[0][1][68], 1);
  assert.equal(new DataView(released[0][1].buffer).getBigInt64(15, true), 1001000000n);
  await before.send({ kind: "play-stop", playId: 7 });

  const unchanged = await gamepadActive();
  const idle = gamepadEvent({ buttons: [{ value: 0, pressed: false, touched: false }, { value: 0, pressed: false, touched: false }] });
  await unchanged.send(step({ events: [idle], watermark: 1100000000n }));
  await unchanged.send(step({ tickId: 2, events: [{ ...idle, sequence: 2n }], watermark: 1200000000n }));
  assert.equal(unchanged.of("play-step-done").length, 2);
  assert.deepEqual(inputCalls(unchanged.games[0]).map(call => call[0]), ["close", "close"]);
  await unchanged.send(step({ tickId: 3, events: [gamepadEvent({ sequence: 3n })], watermark: 1200000000n }));
  assert.equal(inputCalls(unchanged.games[0]).length, 2, "a newly changed old sample is refused rather than moved to the latest watermark");
  assertReleased(unchanged);

  const rows = Array.from({ length: 128 }, (_, index) => [0x11, 0, index]);
  const wide = () => gamepadSetup(rows, 128, 0);
  const all = pressed => Array.from({ length: 128 }, () => ({ value: pressed ? 1 : 0, pressed, touched: false }));
  const down = gamepadEvent({ axes: [], buttons: all(true), sequence: 2n });
  const up = { ...down, buttons: all(false), sequence: 3n };
  const exact = await gamepadActive({}, { gamepadSetup: wide() });
  await exact.send(step({ events: [down, up], watermark: GAMEPAD_HOST }));
  assert.equal(inputCalls(exact.games[0]).filter(call => call[0] === "blob").length, 256);
  assert.equal(exact.of("play-step-done").length, 1);
  await exact.send({ kind: "play-stop", playId: 7 });
  const excess = await gamepadActive({}, { gamepadSetup: wide() });
  await excess.send(step({ events: [{ hostNs: GAMEPAD_HOST, key: 2, down: true, sequence: 1n }, down, up], watermark: GAMEPAD_HOST }));
  assert.equal(inputCalls(excess.games[0]).length, 0);
  assert.equal(excess.of("play-step-done").length, 0); assertReleased(excess);

  let accepted = 0;
  const partial = await gamepadActive({ inputBlob() {
    if (++accepted === 2) throw new Error("actual canonical prefix failure");
  } }, { gamepadSetup: gamepadSetup([[0x11, 0, 0], [0x12, 0, 1], [0x11, 1, 0]]) });
  const pending = step({ events: [gamepadEvent()], watermark: GAMEPAD_HOST });
  await partial.send(pending);
  assert.equal(accepted, 2); assert.equal(partial.of("play-step-done").length, 0);
  assert.match(partial.of("play-error").at(-1).message, /actual canonical prefix failure/);
  assertReleased(partial);
  await partial.send(pending); assert.equal(accepted, 2, "committed binding failure cannot retry the physical fanout");
});

test("automatic Gamepad device setup retains real identities and Worker orders different sources by original timestamps without sequence rewriting", async () => {
  const devices = [{ source: GAMEPAD_SOURCE, index: 7, id: "same model", mapping: "standard", buttons: 9, axes: 1 },
    { source: 3n, index: 0, id: "same model", mapping: "", buttons: 9, axes: 0 }];
  const auto = await catalogWorker({ lanes: [0x11, 0x19] });
  auto.post(startRequest({ inputMode: "physical", keyPairs: new Uint32Array(), gamepadDevices: devices }));
  devices[0].source = 4n; devices[0].buttons = 0; devices.length = 0;
  await flushJobs();
  assert.deepEqual(auto.of("play-reply").at(-1).result.gamepadSources, [GAMEPAD_SOURCE]);
  const words = auto.physicalConstructions[0].args[5];
  assert.equal(words.length, 63);
  assert.deepEqual(Array.from(words.slice(0, 7)), [0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0]);
  assert.deepEqual(Array.from(words.slice(-7)), [0x19, 1, 0x44332211, 0x88776655, 1, 0x57475044, 8]);
  await auto.send({ kind: "play-activate", playId: 7, rpcId: 2, hostNs: GAMEPAD_HOST, startFrame: START });
  const buttons = Array.from({ length: 9 }, (_, index) => ({ value: index === 8 ? 1 : 0, pressed: index === 8, touched: false }));
  await auto.send(step({ events: [gamepadEvent({ buttons, id: "same model" })], watermark: GAMEPAD_HOST }));
  const packet = inputCalls(auto.games[0]).find(call => call[0] === "blob")[1];
  assert.equal(new DataView(packet.buffer).getUint32(64, true), 8);
  assert.equal(new DataView(packet.buffer).getBigUint64(7, true), GAMEPAD_SOURCE);
  await auto.send({ kind: "play-stop", playId: 7 }); assertReleased(auto);

  const h = await gamepadActive({}, { inputMode: "physical-contact", hidSetup: hidSetup([3n]) });
  const acquired = gamepadEvent({ timestampMs: 1001, hostNs: 1001000000n, sequence: 2n });
  const key = { hostNs: 1003000000n, key: 2, down: true, sequence: 100n };
  const touch = touchEvent({ hostNs: 1002000000n, sequence: 9n });
  const hid = hidEvent({ source: 3n, hostNs: 1001000000n, sequence: 0n });
  await h.send(step({ events: [key, acquired, touch, hid], watermark: 1004000000n }));
  const calls = inputCalls(h.games[0]);
  assert.deepEqual(calls.map(call => call[0]), ["blob", "hid", "touch", "blob", "close"]);
  assert.deepEqual(calls.slice(0, 4).map(call => new DataView(call[1].buffer).getBigUint64(7, true)), [GAMEPAD_SOURCE, 3n, 2n, 1n]);
  assert.deepEqual(calls.slice(0, 4).map(call => new DataView(call[1].buffer).getBigUint64(27, true)), [2n, 0n, 9n, 100n]);
  assert.deepEqual(calls.slice(0, 4).map(call => new DataView(call[1].buffer).getBigInt64(15, true)),
    [1001000000n, 1001000000n, 1002000000n, 1003000000n]);
  assert.deepEqual(Array.from(calls[1][1]), Array.from(encodeRawHidEvent(hid)));
  assert.deepEqual(Array.from(calls[2][1]), Array.from(encodeTouchEvent(touch)));
  assert.deepEqual(Array.from(calls[3][1]), Array.from(encodeKeyboardEvent(key)));
  await h.send(step({ tickId: 2, events: [{ ...acquired, sequence: 3n }], watermark: 1005000000n }));
  assert.equal(h.of("play-step-done").length, 2);
  assert.equal(inputCalls(h.games[0]).length, 6, "unchanged old Gamepad samples advance source order without rewinding the Runtime frontier");
  await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
});

test("automatic setup and per-source draft ordering refuse conflicting modes or invalid tails without weakening the committed host boundary", async () => {
  const descriptors = () => [{ source: GAMEPAD_SOURCE, index: 7, id: "standard", mapping: "standard", buttons: 9, axes: 1 }];
  for (const request of [startRequest({ gamepadDevices: descriptors() }),
    replayRequest(replayFile().file, { gamepadDevices: descriptors() }),
    startRequest({ inputMode: "physical", gamepadDevices: descriptors(), gamepadSetup: gamepadSetup() }),
    startRequest({ inputMode: "physical", gamepadDevices: descriptors(), hidSetup: hidSetup([GAMEPAD_SOURCE]) })]) {
    const h = await catalogWorker(); await h.send(request);
    assert.equal(h.games.length, 0); assert.equal(h.replays.length, 0); assert.equal(h.of("play-error").length, 1);
  }
  const empty = await started({ startRequest: startRequest({ inputMode: "physical", gamepadDevices: [] }) });
  assert.deepEqual(empty.of("play-reply")[0].result.gamepadSources, []);
  await empty.send({ kind: "play-stop", playId: 7 });
  const invalidBatches = [
    [{ hostNs: GAMEPAD_HOST + 1n, key: 2, down: true, sequence: 1n }, { hostNs: GAMEPAD_HOST, key: 2, down: false, sequence: 2n }],
    [hidEvent({ source: 3n, hostNs: GAMEPAD_HOST, sequence: 9n }), hidEvent({ source: 3n, hostNs: GAMEPAD_HOST, sequence: 8n })],
    [touchEvent({ hostNs: GAMEPAD_HOST + 1n, sequence: 1n }), touchEvent({ hostNs: GAMEPAD_HOST, sequence: 2n, phase: 2 })],
    [gamepadEvent({ sequence: 3n }), gamepadEvent({ sequence: 2n })],
    [gamepadEvent(), { hostNs: GAMEPAD_HOST + 1n, key: 2, down: true, sequence: 9n },
      hidEvent({ source: 3n, hostNs: GAMEPAD_HOST, sequence: 0n, data: new Uint8Array(1025) })],
  ];
  for (const events of invalidBatches) {
    const h = await gamepadActive({}, { inputMode: "physical-contact", hidSetup: hidSetup([3n]) });
    await h.send(step({ events, watermark: GAMEPAD_HOST + 2n }));
    assert.equal(inputCalls(h.games[0]).length, 0); assert.equal(h.of("play-step-done").length, 0); assertReleased(h);
  }
  const prefix = await gamepadActive({}, { hidSetup: hidSetup([3n]) });
  await prefix.send(step({ events: [hidEvent({ source: 3n, hostNs: GAMEPAD_HOST, sequence: 9n })], watermark: 1100000000n }));
  assert.equal(inputCalls(prefix.games[0]).length, 2);
  await prefix.send(step({ tickId: 2, events: [
    { hostNs: 1200000000n, key: 2, down: true, sequence: 1n },
    hidEvent({ source: 3n, hostNs: 1050000000n, sequence: 10n }),
  ], watermark: 1200000000n }));
  assert.equal(inputCalls(prefix.games[0]).length, 2, "sorting cannot admit any newly changed event behind a previously committed watermark");
  assert.equal(prefix.of("play-step-done").length, 1); assertReleased(prefix);
});

function selectedGamepadProfile(acquire, suppliedBytes) {
  const bytes = suppliedBytes ?? new TextEncoder().encode(JSON.stringify({ version: 1, profiles: [{ id: "custom pad", mapping: "", buttons: 2, axes: 1,
    bindingWords: [0x11, 0, 1, 0x12, 0, 0, 0x11, 1, 0] }] }));
  const file = new FileType([bytes], "gamepad.json"); let reads = 0;
  file.arrayBuffer = () => { reads++; return acquire ? acquire() : Promise.resolve(bytes.slice().buffer); };
  return { file, bytes, get reads() { return reads; } };
}
const customGamepadDevices = () => [{ source: GAMEPAD_SOURCE, index: 7, id: "custom pad", mapping: "", buttons: 2, axes: 1 },
  { source: 3n, index: 0, id: "unmatched pad", mapping: "standard", buttons: 9, axes: 0 }];

test("Worker reads one custom Gamepad profile against pre-await descriptors and forwards explicit nonstandard bindings through physical ownership", async () => {
  for (const inputMode of ["physical", "physical-contact"]) {
    const gate = deferred(), selected = selectedGamepadProfile(() => gate.promise), devices = customGamepadDevices();
    const h = await catalogWorker();
    h.post(startRequest({ inputMode, keyPairs: new Uint32Array(), gamepadProfileFile: selected.file, gamepadDevices: devices, recordReplay: true }));
    devices[0].source = 4n; devices[0].id = "changed after submission"; devices[0].buttons = 0;
    await flushJobs();
    assert.equal(selected.reads, 1); assert.equal(h.games.length, 0);
    assert.equal(h.preparedOwners.length, 1, "profile acquisition precedes any gameplay preparation");
    gate.resolve(selected.bytes.slice().buffer); await flushJobs();
    const prepared = h.of("play-reply").at(-1).result;
    assert.equal(prepared.kind, "prepared"); assert.deepEqual(prepared.gamepadSources, [GAMEPAD_SOURCE]);
    assert.equal(prepared.inputMode, inputMode);
    const construction = (inputMode === "physical" ? h.physicalConstructions : h.contactConstructions)[0];
    assert.deepEqual(Array.from(construction.args[5]), [
      0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 1,
      0x12, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0,
      0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x10000,
    ]);
    const game = h.games[0];
    assert.equal(game.calls.filter(call => call[0] === "capture").length, 1);
    assert.equal(game.calls.filter(call => call[0] === "sample").length, 0);
    await h.send({ kind: "play-activate", playId: 7, rpcId: 2, hostNs: GAMEPAD_HOST, startFrame: START });
    await h.send(step({ events: [gamepadEvent({ id: "custom pad", mapping: "" })], watermark: GAMEPAD_HOST }));
    const blobs = inputCalls(game).filter(call => call[0] === "blob");
    assert.deepEqual(blobs.map(call => new DataView(call[1].buffer).getUint32(64, true)), [0, 0x10000]);
    assert.ok(blobs.every(call => new DataView(call[1].buffer).getBigUint64(7, true) === GAMEPAD_SOURCE));
    assert.equal(selected.reads, 1);
    await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
    assert.deepEqual(game.disposals, ["stop", "take", "free"]);
  }
});

test("custom Gamepad profile refusal and cancelled reads cannot construct fallback or stale owners", async () => {
  for (const alter of [request => { request.inputMode = undefined; },
    request => { request.mode = "replay"; request.replayFile = replayFile().file; },
    request => { request.gamepadSetup = gamepadSetup(); }, request => { delete request.gamepadDevices; },
    request => { request.gamepadDevices[0].source = 3; },
    request => { request.gamepadProfileFile = { size: 1, arrayBuffer() { assert.fail("not a genuine File"); } }; }]) {
    const selected = selectedGamepadProfile(), h = await catalogWorker();
    const request = startRequest({ inputMode: "physical", gamepadProfileFile: selected.file, gamepadDevices: customGamepadDevices() }); alter(request);
    await h.send(request);
    assert.equal(selected.reads, 0); assert.equal(h.games.length + h.replays.length, 0); assert.equal(h.of("play-error").length, 1);
  }
  for (const size of [0, 1048577, 1.5]) {
    const selected = selectedGamepadProfile(); Object.defineProperty(selected.file, "size", { value: size });
    const h = await catalogWorker();
    await h.send(startRequest({ inputMode: "physical", gamepadProfileFile: selected.file, gamepadDevices: customGamepadDevices() }));
    assert.equal(selected.reads, 0); assert.equal(h.games.length, 0);
  }
  for (const acquire of [() => Promise.resolve(new Uint8Array(1)), () => Promise.resolve(new ArrayBuffer(1)),
    () => Promise.reject(new Error("Gamepad profile read denied"))]) {
    const selected = selectedGamepadProfile(acquire), h = await catalogWorker();
    await h.send(startRequest({ inputMode: "physical", gamepadProfileFile: selected.file, gamepadDevices: customGamepadDevices() }));
    assert.equal(selected.reads, 1); assert.equal(h.games.length, 0); assert.equal(h.of("play-error").length, 1);
  }
  for (const bytes of [Uint8Array.from([0xc3, 0x28]), new TextEncoder().encode('{"version":1,"profiles":[{"id":"absent","bindingWords":[17,0,0]}]}'),
    new TextEncoder().encode('{"version":1,"profiles":[{"bindingWords":[17,0,0]},{"bindingWords":[18,0,1]}]}')]) {
    const selected = selectedGamepadProfile(null, bytes), h = await catalogWorker();
    await h.send(startRequest({ inputMode: "physical", gamepadProfileFile: selected.file, gamepadDevices: customGamepadDevices() }));
    assert.equal(selected.reads, 1); assert.equal(h.games.length, 0); assert.equal(h.of("play-error").length, 1);
  }
  const gate = deferred(), selected = selectedGamepadProfile(() => gate.promise), h = await catalogWorker();
  await h.send(startRequest({ inputMode: "physical", gamepadProfileFile: selected.file, gamepadDevices: customGamepadDevices() }));
  assert.equal(selected.reads, 1); assert.equal(h.games.length, 0);
  await h.send({ kind: "play-stop", playId: 7 });
  await h.send(startRequest({ playId: 8 }));
  const messages = h.messages.length, next = h.games[0];
  gate.resolve(selected.bytes.slice().buffer); await flushJobs();
  assert.equal(h.messages.length, messages); assert.equal(h.games.length, 1); assert.equal(next.stops, 0);
  assert.equal(h.preparedOwners.length, 2, "late profile bytes do not cause another chart preparation");
  await h.send({ kind: "play-stop", playId: 8 });
  const consumed = await catalogWorker({ physicalConstructError: "genuine consuming constructor refused custom bindings" });
  const refused = selectedGamepadProfile();
  await consumed.send(startRequest({ inputMode: "physical", gamepadProfileFile: refused.file, gamepadDevices: customGamepadDevices() }));
  assert.equal(consumed.physicalConstructions.length, 1); assert.equal(consumed.games.length, 0);
  assert.equal(consumed.preparedOwners.at(-1).moved, true); assert.equal(consumed.preparedOwners.at(-1).frees, 0);
  assert.equal(consumed.of("play-error").length, 1);
});

const HID_SOURCE = 18446744073709551615n;
function hidSetup(sources = [HID_SOURCE]) {
  const bindingWords = [], deviceWords = [], fieldWords = [], axisParams = [];
  for (const [device, source] of sources.entries()) {
    const low = Number(source & 0xffffffffn), high = Number(source >> 32n);
    deviceWords.push(low, high, 1, 65535, 1, 4660);
    for (let index = 0; index < 2; index++) {
      bindingWords.push(0x11 + index, 1, low, high, 1, 0xffffffff, 0xffffffff - index);
      fieldWords.push(device, 1, 9, 1, 1, 0xffffffff, 0xffffffff - index, index, 1, 0, 0, 0, 0);
      axisParams.push(0, 0);
    }
  }
  return { bindingWords: new Uint32Array(bindingWords), deviceWords: new Uint32Array(deviceWords),
    fieldWords: new Uint32Array(fieldWords), axisParams: new Float32Array(axisParams) };
}
function hidEvent(fields = {}) {
  return { kind: "hid", hostNs: ORIGIN, source: HID_SOURCE, sequence: 1n,
    reportId: 9, data: Uint8Array.from([9, 255, 0]), ...fields };
}
function inputCalls(game) {
  return game.calls.filter(call => ["input", "blob", "touch", "hid", "close"].includes(call[0]));
}

function controllerProfile(acquire, suppliedBytes) {
  const bytes = suppliedBytes ?? new TextEncoder().encode(JSON.stringify({ version: 1, profiles: [{ vendorId: 1, productId: 2,
    bindingWords: [0x11, 0, 9, 1, 0x12, 0, 9, 2],
    fieldWords: [1, 9, 1, 0, 9, 1, 0, 1, 0, 0, 0, 0, 1, 9, 1, 0, 9, 2, 1, 1, 0, 0, 0, 0], axisParams: [0, 0, 0, 0] }] }));
  const file = new FileType([bytes], "controller.json"); let reads = 0;
  file.arrayBuffer = () => { reads++; return acquire ? acquire() : Promise.resolve(bytes.slice().buffer); };
  return { file, bytes, get reads() { return reads; } };
}
const profileDevices = () => [{ source: HID_SOURCE, vendorId: 1, productId: 2 }, { source: 3n, vendorId: 9, productId: 9 }];

test("Worker acquires a profile once and actual matching supplies only admitted full-width sources and HID-only constructor coverage", async () => {
  for (const inputMode of ["physical", "physical-contact"]) {
    const selected = controllerProfile(), devices = profileDevices(), h = await catalogWorker();
    h.post(startRequest({ inputMode, keyPairs: new Uint32Array(), hidProfileFile: selected.file, hidDevices: devices, recordReplay: true }));
    devices[0].source = 4n; devices[0].vendorId = 9;
    await flushJobs();
    assert.equal(selected.reads, 1);
    const prepared = h.of("play-reply").at(-1).result;
    assert.equal(prepared.kind, "prepared"); assert.equal(prepared.hidSourceCount, 1);
    assert.deepEqual(prepared.hidSources, [HID_SOURCE]);
    const construction = (inputMode === "physical" ? h.physicalConstructions : h.contactConstructions)[0];
    assert.deepEqual(Array.from(construction.args[5]), [0x11, 1, 0xffffffff, 0xffffffff, 0, 9, 1, 0x12, 1, 0xffffffff, 0xffffffff, 0, 9, 2]);
    const game = h.games[0], configured = game.calls.find(call => call[0] === "hid-setup");
    assert.deepEqual(Array.from(configured[1]), [0xffffffff, 0xffffffff, 1, 1, 1, 2]);
    assert.deepEqual(Array.from(configured[2]), [0, 1, 9, 1, 0, 9, 1, 0, 1, 0, 0, 0, 0, 0, 1, 9, 1, 0, 9, 2, 1, 1, 0, 0, 0, 0]);
    assert.ok(game.calls.indexOf(configured) < game.calls.findIndex(call => call[0] === "capture"));
    await h.send({ kind: "play-activate", playId: 7, rpcId: 2, hostNs: ORIGIN, startFrame: START });
    await h.send(step({ events: [hidEvent({ data: Uint8Array.from([3]) })] }));
    assert.equal(inputCalls(game)[0][0], "hid");
    await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
  }
});

test("profile-file metadata, mutually exclusive setup and malformed actual read extents refuse without a constructed gameplay owner", async () => {
  for (const alter of [request => { request.hidSetup = hidSetup(); }, request => { request.inputMode = undefined; },
    request => { request.mode = "replay"; request.replayFile = replayFile().file; }, request => { request.hidDevices = []; },
    request => { request.hidDevices[0].source = 3; }, request => { request.hidDevices[0].vendorId = 65536; },
    request => { request.hidDevices[1].source = HID_SOURCE; }, request => { request.hidProfileFile = { size: 1, arrayBuffer() { assert.fail("not a File"); } }; }]) {
    const selected = controllerProfile(), h = await catalogWorker();
    const request = startRequest({ inputMode: "physical", hidProfileFile: selected.file, hidDevices: profileDevices() }); alter(request);
    await h.send(request);
    assert.equal(selected.reads, 0); assert.equal(h.games.length + h.replays.length, 0);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const size of [0, 1048577, 1.5]) {
    const selected = controllerProfile(); Object.defineProperty(selected.file, "size", { value: size });
    const h = await catalogWorker();
    await h.send(startRequest({ inputMode: "physical", hidProfileFile: selected.file, hidDevices: profileDevices() }));
    assert.equal(selected.reads, 0); assert.equal(h.games.length, 0);
  }
  for (const acquire of [() => Promise.resolve(new Uint8Array(1)), () => Promise.resolve(new ArrayBuffer(1)),
    () => Promise.reject(new Error("profile acquisition denied"))]) {
    const selected = controllerProfile(acquire), h = await catalogWorker();
    await h.send(startRequest({ inputMode: "physical", hidProfileFile: selected.file, hidDevices: profileDevices() }));
    assert.equal(selected.reads, 1); assert.equal(h.games.length, 0); assert.equal(h.of("play-error").length, 1);
  }
  for (const bytes of [Uint8Array.from([0xc3, 0x28]), new TextEncoder().encode('{"version":1,"profiles":[]}')]) {
    const selected = controllerProfile(null, bytes), h = await catalogWorker();
    await h.send(startRequest({ inputMode: "physical", hidProfileFile: selected.file, hidDevices: profileDevices() }));
    assert.equal(selected.reads, 1); assert.equal(h.games.length, 0); assert.equal(h.of("play-error").length, 1);
  }
});

test("cancelled profile reads cannot admit a late owner and actual profile or consuming constructor failures never retry numeric fallback", async () => {
  const gate = deferred(), selected = controllerProfile(() => gate.promise), h = await catalogWorker();
  await h.send(startRequest({ inputMode: "physical", hidProfileFile: selected.file, hidDevices: profileDevices() }));
  assert.equal(selected.reads, 1); assert.equal(h.games.length, 0);
  await h.send({ kind: "play-stop", playId: 7 });
  await h.send(startRequest({ playId: 8 })); const next = h.games[0], before = h.messages.length;
  gate.resolve(selected.bytes.slice().buffer); await flushJobs();
  assert.equal(h.messages.length, before); assert.equal(h.games.length, 1); assert.equal(next.stops, 0);
  assert.equal(h.physicalConstructions.length, 0);
  await h.send({ kind: "play-stop", playId: 8 });
  for (const options of [{ hidSetupError: "actual common profile refused" }, { physicalConstructError: "actual physical constructor refused" }]) {
    const selected = controllerProfile(), failed = await catalogWorker(options);
    await failed.send(startRequest({ inputMode: "physical", hidProfileFile: selected.file, hidDevices: profileDevices(), recordReplay: true }));
    assert.equal(selected.reads, 1); assert.equal(failed.physicalConstructions.length, 1);
    assert.equal(failed.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
    assert.equal(failed.preparedOwners[1].moved, true); assert.equal(failed.preparedOwners[1].frees, 0);
    if (options.hidSetupError) {
      assertReleased(failed); assert.equal(failed.games[0].calls.some(call => ["capture", "sample", "activate"].includes(call[0])), false);
    } else assert.equal(failed.games.length, 0);
  }
});

test("HID setup snapshots full-width constructor words before readiness and supports HID-only lanes beside keyboard and contact", async () => {
  for (const inputMode of ["physical", "physical-contact"]) {
    const h = await catalogWorker();
    const setup = hidSetup([HID_SOURCE, 3n]);
    const expected = Object.fromEntries(Object.entries(setup).map(([key, value]) => [key, Array.from(value)]));
    const keys = inputMode === "physical" ? new Uint32Array() : pairs();
    h.post(startRequest({ inputMode, keyPairs: keys, hidSetup: setup, recordReplay: true }));
    for (const value of Object.values(setup)) value.fill(0);
    keys.fill(0);
    await flushJobs();
    const result = h.of("play-reply").at(-1).result;
    assert.equal(result.kind, "prepared"); assert.equal(result.hidSourceCount, 2);
    assert.equal(result.inputMode, inputMode);
    const game = h.games[0];
    const construction = (inputMode === "physical" ? h.physicalConstructions : h.contactConstructions)[0];
    const keyboard = inputMode === "physical" ? [] : [0x11, 0, 0, 0, 1, 0x574b4559, 2, 0x12, 0, 0, 0, 1, 0x574b4559, 3];
    assert.deepEqual(Array.from(construction.args[5]), [...keyboard, ...expected.bindingWords]);
    const configured = game.calls.find(call => call[0] === "hid-setup");
    assert.deepEqual(configured.slice(1).map(value => Array.from(value)), [expected.deviceWords, expected.fieldWords, expected.axisParams]);
    assert.equal(game.calls.filter(call => call[0] === "hid-setup").length, 1);
    assert.ok(game.calls.indexOf(configured) < game.calls.findIndex(call => call[0] === "capture"));
    if (inputMode === "physical-contact") {
      const touch = game.calls.findIndex(call => call[0] === "touch-setup");
      assert.ok(touch >= 0 && touch < game.calls.findIndex(call => call[0] === "capture"));
    }
    await h.send({ kind: "play-sample", playId: 7, rpcId: 2 });
    assert.ok(game.calls.indexOf(configured) < game.calls.findIndex(call => call[0] === "sample"));
    await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
  }
  const ordinary = await started({ startRequest: startRequest({ inputMode: "physical" }) });
  assert.equal(Object.hasOwn(ordinary.of("play-reply")[0].result, "hidSourceCount"), false);
  assert.equal(ordinary.games[0].calls.some(call => call[0] === "hid-setup"), false);
  await ordinary.send({ kind: "play-stop", playId: 7 });
});

test("HID mode, bounded typed setup and source admission fail before readiness while missing bindings free unconsumed preparation", async () => {
  const badSetups = [null, [], {},
    { ...hidSetup(), bindingWords: [] }, { ...hidSetup(), deviceWords: new Uint8Array(6) },
    { ...hidSetup(), fieldWords: new Uint32Array(12) }, { ...hidSetup(), axisParams: new Float32Array(3) },
    { ...hidSetup(), axisParams: new Float64Array(4) }, { ...hidSetup(), deviceWords: new Uint32Array() },
    { ...hidSetup(), deviceWords: new Uint32Array(5) }, hidSetup(Array.from({ length: 17 }, (_, index) => BigInt(index + 3))),
    hidSetup([3n, 3n]), hidSetup([2n]),
    { ...hidSetup(), bindingWords: new Uint32Array(6) }, { ...hidSetup(), bindingWords: new Uint32Array(257 * 7) },
    { ...hidSetup(), fieldWords: new Uint32Array((16 * 512 + 1) * 13), axisParams: new Float32Array((16 * 512 + 1) * 2) },
  ];
  const invalidLane = hidSetup(); invalidLane.bindingWords[0] = 0x20; badSetups.push(invalidLane);
  const combined = hidSetup();
  combined.bindingWords = new Uint32Array(Array.from({ length: 255 }, () => [0x11, 1, 0xffffffff, 0xffffffff, 1, 1, 1]).flat());
  badSetups.push(combined); // Two actual keyboard rows exceed the combined 256-row limit.
  const detached = hidSetup(); structuredClone(detached.fieldWords.buffer, { transfer: [detached.fieldWords.buffer] }); badSetups.push(detached);
  for (const setup of badSetups) {
    const gate = deferred(), h = await workerHarness({ initGate: gate });
    await h.send({ kind: "init", canvas: {} });
    await h.send(startRequest({ inputMode: "physical", hidSetup: setup }));
    assert.equal(h.of("play-error").length, 1);
    assert.ok(h.of("play-reply")[0].error);
    assert.equal(h.preparedOwners.length, 0); assert.equal(h.games.length, 0);
    gate.resolve(); await flushJobs(); assert.equal(h.games.length, 0);
  }
  for (const request of [startRequest({ hidSetup: hidSetup() }), replayRequest(replayFile().file, { hidSetup: hidSetup() })]) {
    const h = await catalogWorker(); await h.send(request);
    assert.equal(h.games.length + h.replays.length, 0);
    assert.equal(h.libraries[0].preparations.length, 1); assert.equal(h.libraries[0].replayPreparations.length, 0);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const options of [{ missingHidSetup: true }, { missingInputHidBlob: true }]) {
    const h = await catalogWorker(options);
    await h.send(startRequest({ inputMode: "physical", hidSetup: hidSetup() }));
    assert.equal(h.physicalConstructions.length, 0); assert.equal(h.games.length, 0);
    assert.equal(h.preparedOwners[1].moved, false); assert.equal(h.preparedOwners[1].frees, 1);
    assert.equal(h.of("play-error").length, 1);
  }
  const incomplete = hidSetup(); incomplete.bindingWords = incomplete.bindingWords.slice(0, 7);
  const missingLane = await catalogWorker();
  await missingLane.send(startRequest({ inputMode: "physical", keyPairs: new Uint32Array(), hidSetup: incomplete }));
  assert.equal(missingLane.physicalConstructions.length, 0);
  assert.equal(missingLane.preparedOwners[1].frees, 1);
  assert.equal(missingLane.of("play-error").length, 1, "HID coverage cannot silently omit a prepared lane");
});

test("consuming HID construction and actual profile refusal release exactly one owner and never publish partial setup", async () => {
  for (const [option, message] of [["physicalConstructError", "consuming HID constructor refused"], ["hidSetupError", "actual profile membership refused"]]) {
    const h = await catalogWorker({ [option]: message });
    await h.send(startRequest({ inputMode: "physical", hidSetup: hidSetup(), recordReplay: true }));
    assert.equal(h.physicalConstructions.length, 1);
    assert.equal(h.preparedOwners[1].moved, true); assert.equal(h.preparedOwners[1].frees, 0);
    assert.equal(h.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
    assert.match(h.of("play-error")[0].message, new RegExp(message));
    if (option === "hidSetupError") {
      assertReleased(h);
      assert.deepEqual(h.games[0].calls.map(call => call[0]), ["hid-setup"]);
      assert.equal(h.games[0].replayTakes, 0);
    } else assert.equal(h.games.length, 0);
  }
  const gate = deferred(), cancelled = await workerHarness({ initGate: gate });
  await cancelled.send({ kind: "init", canvas: {} });
  cancelled.post(startRequest({ inputMode: "physical", hidSetup: hidSetup() }));
  cancelled.post({ kind: "play-stop", playId: 7 }); await flushJobs();
  gate.resolve(); await flushJobs();
  assert.equal(cancelled.games.length, 0); assert.equal(cancelled.preparedOwners.length, 0);
  assert.equal(cancelled.of("play-stopped").length, 1); assert.equal(cancelled.of("play-error").length, 0);
});

test("mixed keyboard contact and numbered or zero-ID HID inputs use the actual canonical encoders in original order", async () => {
  const h = await active({ startRequest: startRequest({ inputMode: "physical-contact", hidSetup: hidSetup([HID_SOURCE, 3n]) }) });
  const sequence = 9007199254740993n;
  const backing = Uint8Array.from([99, 9, 255, 0, 88]);
  const key = { hostNs: ORIGIN, key: 2, down: true, sequence };
  const down = touchEvent({ hostNs: ORIGIN + 1n, sequence: sequence + 1n });
  const numbered = hidEvent({ hostNs: ORIGIN + 2n, sequence: sequence + 2n, data: backing.subarray(1, 4) });
  const zero = hidEvent({ hostNs: ORIGIN + 3n, sequence: sequence + 3n, source: 3n, reportId: 0, data: new Uint8Array() });
  const up = touchEvent({ hostNs: ORIGIN + 4n, sequence: sequence + 4n, phase: 2, pressure: null });
  await h.send(step({ events: [key, down, numbered, zero, up], watermark: ORIGIN + 5n, audioNs: 9007199254741222n }));
  const calls = inputCalls(h.games[0]);
  assert.deepEqual(calls.map(call => call[0]), ["blob", "touch", "hid", "hid", "touch", "close"]);
  for (const [index, encoded] of [[0, encodeKeyboardEvent(key)], [1, encodeTouchEvent(down)], [2, encodeRawHidEvent(numbered)], [3, encodeRawHidEvent(zero)], [4, encodeTouchEvent(up)]]) {
    assert.deepEqual(Array.from(calls[index][1]), Array.from(encoded));
    assert.equal(calls[index].at(-1), h.acquisitionNowNs());
  }
  assert.deepEqual(calls[1].slice(2), [480, 360, 960, 720, h.acquisitionNowNs()]);
  assert.equal(new DataView(calls[2][1].buffer).getBigUint64(7, true), HID_SOURCE);
  assert.deepEqual(Array.from(calls[2][1].slice(69)), [9, 255, 0], "a payload byte equal to report ID remains payload");
  assert.equal(calls[3][1].length, 68); assert.equal(calls[3][1][59], 0);
  backing.fill(0); assert.deepEqual(Array.from(calls[2][1].slice(69)), [9, 255, 0]);
  assert.deepEqual(calls[5], ["close", ORIGIN + 5n]);
  assert.equal(h.of("play-step-done")[0].tickId, 1);
  await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
});

test("a late malformed HID event or unknown source refuses the entire mixed batch before any gameplay mutation", async () => {
  const invalid = [hidEvent({ source: 4n }), hidEvent({ source: Number(HID_SOURCE) }), hidEvent({ reportId: 256 }),
    hidEvent({ data: [1] }), hidEvent({ data: new Uint8Array(1025) }), hidEvent({ hostNs: -1n }),
    hidEvent({ sequence: 18446744073709551616n }), hidEvent({ sequence: -1n }), hidEvent({ hostNs: 9223372036854775808n })];
  for (const bad of invalid) {
    const h = await active({ startRequest: startRequest({ inputMode: "physical-contact", hidSetup: hidSetup() }) });
    const key = { hostNs: ORIGIN, key: 2, down: true, sequence: 1n };
    await h.send(step({ events: [key, bad], watermark: ORIGIN + 1n }));
    assert.equal(inputCalls(h.games[0]).length, 0);
    assert.equal(h.of("play-step-done").length, 0); assert.equal(h.of("play-error").length, 1);
    assertReleased(h);
  }
  const unconfigured = await active({ startRequest: startRequest({ inputMode: "physical" }) });
  await unconfigured.send(step({ events: [hidEvent()] }));
  assert.equal(inputCalls(unconfigured.games[0]).length, 0); assertReleased(unconfigured);
  const preroll = await active({ startRequest: startRequest({ inputMode: "physical", hidSetup: hidSetup() }) });
  await preroll.send(step({ events: [hidEvent({ hostNs: ORIGIN - 1n, data: new Uint8Array(1025) })] }));
  assert.equal(inputCalls(preroll.games[0]).length, 0);
  assert.equal(preroll.of("play-step-done").length, 0, "pre-origin suppression never bypasses raw packet validation");
  assertReleased(preroll);
  const history = await active({ startRequest: startRequest({ inputMode: "physical", hidSetup: hidSetup() }) });
  await history.send(step({ events: [hidEvent({ sequence: 9n })], watermark: ORIGIN + 10n }));
  const accepted = inputCalls(history.games[0]).length;
  await history.send(step({ tickId: 2, events: [hidEvent({ sequence: 10n, hostNs: ORIGIN + 9n })], watermark: ORIGIN + 10n }));
  assert.equal(inputCalls(history.games[0]).length, accepted, "a prior watermark remains authoritative across raw-report batches");
  assert.equal(history.of("play-step-done").length, 1); assertReleased(history);
});

test("pre-origin HID remains validated and zero-transition acquisitions advance while a failed typed prefix cannot retry or affect a newer owner", async () => {
  const h = await active({ startRequest: startRequest({ inputMode: "physical", hidSetup: hidSetup() }) });
  const before = hidEvent({ hostNs: ORIGIN - 1n, sequence: 0n });
  const zeroTransition = hidEvent({ sequence: 1n, data: Uint8Array.from([0]) });
  await h.send(step({ events: [before, zeroTransition], watermark: ORIGIN + 2n }));
  const calls = inputCalls(h.games[0]);
  assert.deepEqual(calls.map(call => call[0]), ["hid", "close"]);
  assert.deepEqual(Array.from(calls[0][1]), Array.from(encodeRawHidEvent(zeroTransition)));
  assert.equal(h.of("play-step-done")[0].preOriginInputs, 1);
  assert.equal(h.of("play-step-done")[0].hits, SCORE.hits, "the Worker never fabricates a key or a hit for a raw acquisition");
  await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);

  let acquired = 0;
  const partial = await active({ startRequest: startRequest({ inputMode: "physical-contact", hidSetup: hidSetup(), recordReplay: true }),
    inputHidBlob(game) {
      acquired++; game.score.hits = SCORE.hits + BigInt(acquired);
      if (acquired === 2) throw new Error("actual binding retained a partial HID report");
    } });
  const request = step({ events: [hidEvent(), hidEvent({ hostNs: ORIGIN + 1n, sequence: 2n }),
    touchEvent({ hostNs: ORIGIN + 2n, sequence: 3n })], watermark: ORIGIN + 2n });
  await partial.send(request);
  assert.deepEqual(inputCalls(partial.games[0]).map(call => call[0]), ["hid", "hid"]);
  assert.equal(partial.of("play-step-done").length, 0);
  assert.match(partial.of("play-error")[0].message, /actual binding retained a partial HID report/);
  assert.equal(partial.of("play-error")[0].replayComplete, false);
  assertReleased(partial, { ...SCORE, hits: SCORE.hits + 2n });
  const messages = partial.messages.length;
  await partial.send(request); assert.equal(partial.messages.length, messages); assert.equal(acquired, 2);
  await partial.send(startRequest({ playId: 8, inputMode: "physical", hidSetup: hidSetup() }));
  const next = partial.games[1]; await partial.send(request);
  assert.equal(inputCalls(next).length, 0); assert.equal(next.stops, 0); assert.equal(acquired, 2);
  await partial.send({ kind: "play-stop", playId: 8 });
});

test("contact mode configures actual geometry before capture and forwards mixed canonical inputs with separate projection", async () => {
  for (const finite of [false, true]) {
    const h = await active({ startRequest: startRequest({ inputMode: "physical-contact", recordReplay: true, ...(finite ? { endNs: 1n } : {}) }),
      gameEnd: finite ? 1n : undefined, gameEndFrame: finite ? 4801n : undefined });
    const game = h.games[0], constructed = h.contactConstructions[0];
    assert.equal(h.physicalConstructions.length, 0);
    assert.equal(h.contactConstructions.length, 1);
    assert.deepEqual(constructed.args.slice(6), [finite ? 1n : undefined, 4096, 1024]);
    assert.equal(h.of("play-reply")[0].result.inputMode, "physical-contact");
    const setup = game.calls.find(call => call[0] === "touch-setup");
    assert.deepEqual(Array.from(setup[1]), [0x11, 0, 0, 0, 1, 0x57544f55, 0, 0x12, 0, 0, 0, 1, 0x57544f55, 0]);
    assert.deepEqual(Array.from(setup[2]), [80, 110, 400, 634, 400, 110, 720, 634]);
    assert.equal(setup[3], 256);
    assert.ok(game.calls.indexOf(setup) < game.calls.findIndex(call => call[0] === "capture"));
    const key = { hostNs: ORIGIN, key: 2, down: true, sequence: 1n };
    const down = touchEvent({ hostNs: ORIGIN + 1n, sequence: 2n });
    const up = touchEvent({ hostNs: ORIGIN + 2n, sequence: 3n, phase: 2, x: 500, pressure: null });
    await h.send(step({ events: [key, down, up], watermark: ORIGIN + 2n }));
    assert.deepEqual(Array.from(game.calls.find(call => call[0] === "blob")[1]), Array.from(encodeKeyboardEvent(key)));
    const contacts = game.calls.filter(call => call[0] === "touch");
    assert.equal(contacts.length, 2);
    assert.deepEqual(contacts.map(call => Array.from(call[1])), [down, up].map(event => Array.from(encodeTouchEvent(event))));
    assert.deepEqual(contacts.map(call => call.slice(2)), [[480, 360, 960, 720, h.acquisitionNowNs()], [480, 360, 960, 720, h.acquisitionNowNs()]]);
    assert.equal(game.calls.filter(call => call[0] === "input").length, 0);
    assert.equal(h.of("play-step-done").at(-1).tickId, 1);
    await h.send({ kind: "play-stop", playId: 7 }); assertReleased(h);
  }
});

test("touch surface snapshots reach the shared binding with original bytes after complete scalar geometry preflight", async () => {
  for (const local of [false, true]) {
    const preflights = [];
    const h = await active({ startRequest: local ? localRequest() : startRequest({ inputMode: "physical-contact" }),
      preflightTouchSurface(game, ...geometry) {
        assert.equal(inputCalls(game).length, 0, "every scalar geometry check precedes any input or advance mutation");
        preflights.push(geometry);
      } });
    const game = local ? h.locals[0] : h.games[0];
    const inputs = [
      touchEvent({ x: 200.123456789, y: 300.375, width: 1280, height: 720, surfaceWidth: 2560, surfaceHeight: 1440 }),
      touchEvent({ sequence: 2n, hostNs: ORIGIN + 1n, phase: 1, x: 800.25, y: 500.5,
        width: 1000, height: 1000, surfaceWidth: 1001, surfaceHeight: 1001 }),
      touchEvent({ sequence: 3n, hostNs: ORIGIN + 2n, phase: 2, x: -15.5, y: 800.25,
        width: 1280, height: 720, surfaceWidth: 1280, surfaceHeight: 720 }),
    ];
    await h.send({ kind: "resize", width: 1920, height: 1080 });
    await h.send(step({ events: inputs, watermark: ORIGIN + 2n, audioNs: 9007199254741222n }));
    assert.deepEqual(preflights, inputs.map(input => [Math.fround(input.x), Math.fround(input.y),
      input.width, input.height, input.surfaceWidth, input.surfaceHeight]));
    const contacts = inputCalls(game).filter(call => call[0] === "touch");
    assert.deepEqual(contacts.map(call => call[1]), inputs.map(encodeTouchEvent));
    assert.deepEqual(contacts.map(call => call.slice(2)), [
      [1280, 720, 2560, 1440, h.acquisitionNowNs()],
      [1000, 1000, 1001, 1001, h.acquisitionNowNs()],
      [1280, 720, 1280, 720, h.acquisitionNowNs()],
    ]);
    assert.equal(h.of("play-step-done").length, 1); assert.equal(h.of("play-error").length, 0);
    assert.equal(h.views[0].extents.at(-1)[0], 1920);
    assert.equal(contacts[2][1][76], 2, "the captured off-surface release keeps its actual phase");
    await h.send({ kind: "play-stop", playId: 7 }); assert.equal(game.frees, 1);
  }
});

test("invalid backing extents and binding projection refusal reject a whole mixed input batch before any committed prefix", async () => {
  for (const local of [false, true]) for (const malformed of [
    { surfaceWidth: undefined }, { surfaceHeight: 0 }, { surfaceWidth: -1 }, { surfaceWidth: 1.5 },
    { surfaceHeight: 4294967296 }, { surfaceWidth: NaN }, { width: Number.MIN_VALUE },
  ]) {
    const h = await active({ startRequest: local ? localRequest({ recordReplay: true })
      : startRequest({ inputMode: "physical-contact", recordReplay: true }),
      preflightTouchSurface(game, x, y, cssWidth) {
        assert.equal(inputCalls(game).length, 0);
        if (cssWidth === Number.MIN_VALUE) throw new Error("actual portable projection cannot represent this point");
      } });
    const game = local ? h.locals[0] : h.games[0];
    const first = touchEvent(), bad = touchEvent({ sequence: 2n, hostNs: ORIGIN + 1n, ...malformed });
    await h.send(step({ events: [first, { hostNs: ORIGIN, key: 2, down: true, sequence: 1n }, bad], watermark: ORIGIN + 1n }));
    assert.equal(inputCalls(game).length, 0);
    assert.equal(h.of("play-step-done").length, 0); assert.equal(h.of("play-error").length, 1);
    assert.equal(game.frees, 1);
    const result = h.of("play-error")[0];
    if (local) assert.ok(result.replays.every(row => row.replayComplete === false));
    else assert.equal(result.replayComplete, false);
    if (malformed.width === Number.MIN_VALUE) {
      assert.equal(game.touchPreflights.length, 2);
      assert.match(result.message, /actual portable projection/);
    }
  }
});

test("contact capability geometry and consuming constructor failures preserve explicit ownership without fallback", async () => {
  for (const options of [{ missingContactConstructor: true }, { missingTouchSetup: true }, { missingInputBlobOnSurface: true },
    { missingPreflightTouchSurface: true }, { missingInputBlob: true }]) {
    const h = await catalogWorker(options);
    await h.send(startRequest({ inputMode: "physical-contact" }));
    assert.equal(h.contactConstructions.length, 0);
    assert.equal(h.games.length, 0);
    assert.equal(h.of("play-error").length, 1);
    assert.ok(h.preparedOwners.slice(1).every(owner => !owner.moved && owner.frees === 1));
  }
  for (const options of [{ touchBounds: [] }, { touchBounds: new Float32Array(4) }, { touchWidth: 0 },
    { touchHeight: 1.5 }, { touchBoundsError: "geometry getter refused" }, { touchSetupError: "actual router refused" }]) {
    const h = await catalogWorker(options);
    await h.send(startRequest({ inputMode: "physical-contact", recordReplay: true }));
    assert.equal(h.contactConstructions.length, 1);
    assert.equal(h.games[0].calls.filter(call => ["capture", "sample", "activate"].includes(call[0])).length, 0);
    assertReleased(h);
    assert.equal(h.preparedOwners[1].frees, 0);
  }
  const failed = await catalogWorker({ contactConstructError: "consuming contact construction refused" });
  await failed.send(startRequest({ inputMode: "physical-contact" }));
  assert.equal(failed.preparedOwners[1].moved, true);
  assert.equal(failed.preparedOwners[1].frees, 0);
  assert.equal(failed.physicalConstructions.length, 0);
  const selected = replayFile(), replay = await catalogWorker();
  await replay.send(replayRequest(selected.file, { inputMode: "physical-contact" }));
  assert.equal(selected.reads, 0);
  assert.equal(replay.replays.length, 0);
});

test("mixed touch batches preflight all packets and preserve a committed binding prefix on later failure", async () => {
  for (const bad of [touchEvent({ x: Infinity }), touchEvent({ hostNs: ORIGIN - 1n, pressure: NaN }), touchEvent({ contact: 18446744073709551616n })]) {
    const h = await active({ startRequest: startRequest({ inputMode: "physical-contact" }) });
    const key = { hostNs: ORIGIN - 1n, key: 2, down: true, sequence: 0n };
    await h.send(step({ events: [key, bad] }));
    assert.equal(h.games[0].calls.filter(call => ["blob", "touch", "input", "close"].includes(call[0])).length, 0);
    assert.equal(h.of("play-step-done").length, 0);
    assertReleased(h);
  }
  const plain = await active({ startRequest: startRequest({ inputMode: "physical" }) });
  await plain.send(step({ events: [touchEvent()] }));
  assert.equal(plain.games[0].calls.filter(call => ["blob", "touch", "close"].includes(call[0])).length, 0);
  assertReleased(plain);
  const h = await active({ startRequest: startRequest({ inputMode: "physical-contact" }),
    inputBlobOnSurface(game, bytes) { if (bytes[76] === 1) throw new Error("actual contact binding rejected later input"); } });
  await h.send(step({ events: [touchEvent(), touchEvent({ hostNs: ORIGIN + 1n, sequence: 2n, phase: 1 })], watermark: ORIGIN + 1n }));
  assert.equal(h.games[0].calls.filter(call => call[0] === "touch").length, 2);
  assert.equal(h.games[0].calls.filter(call => call[0] === "close").length, 0);
  assert.equal(h.of("play-step-done").length, 0);
  assert.match(h.of("play-error")[0].message, /actual contact binding rejected/);
  assertReleased(h);
});

test("explicit physical keyboard ownership uses native bindings and canonical blobs with the original acquisition clock and finite setup", async () => {
  for (const finite of [false, true]) {
    const keyPairs = pairs();
    const h = await active({ missingSectionConstructor: true,
      startRequest: startRequest({ inputMode: "physical", keyPairs, recordReplay: true,
        timing: { earlyNs: 7n, lateNs: 9n, offsetNs: -3n }, ...(finite ? { endNs: 1n } : {}) }),
      gameEnd: finite ? 1n : undefined, gameEndFrame: finite ? 4801n : undefined,
    });
    const game = h.games[0], metadata = h.of("play-reply")[0].result;
    assert.equal(metadata.inputMode, "physical");
    assert.equal(h.physicalConstructions.length, 1);
    assert.equal(h.sectionConstructions.length, 0);
    const construction = h.physicalConstructions[0];
    assert.equal(construction.prepared, h.preparedOwners[1]);
    assert.deepEqual(construction.args.slice(0, 5), [0n, 100000000n, 7n, 9n, -3n]);
    assert.deepEqual(Array.from(construction.args[5]), [
      0x11, 0, 0, 0, 1, 0x574b4559, 2, 0x12, 0, 0, 0, 1, 0x574b4559, 3,
    ]);
    assert.deepEqual(construction.args.slice(6), [finite ? 1n : undefined, 4096, 1024]);
    assert.equal(Object.hasOwn(metadata, "endFrame"), finite);
    if (finite) assert.equal(metadata.endFrame, 4801n);
    keyPairs[1] = 99;
    assert.equal(construction.args[5][6], 2);
    const down = { hostNs: ORIGIN, key: 2, down: true, sequence: 1n };
    const up = { hostNs: ORIGIN + 1n, key: 2, down: false, sequence: 2n };
    await h.send(step({ events: [{ hostNs: ORIGIN - 1n, key: 2, down: true, sequence: 0n }, down, up], watermark: ORIGIN + 1n }));
    const blobs = game.calls.filter(row => row[0] === "blob");
    assert.equal(blobs.length, 2, "fully validated pre-origin input remains ignored rather than retimestamped");
    assert.deepEqual(blobs.map(row => Array.from(row[1])), [Array.from(encodeKeyboardEvent(down)), Array.from(encodeKeyboardEvent(up))]);
    assert.deepEqual(blobs.map(row => row[2]), [h.acquisitionNowNs(), h.acquisitionNowNs()]);
    assert.equal(game.calls.filter(row => row[0] === "input").length, 0);
    assert.deepEqual(game.calls.find(row => row[0] === "close"), ["close", ORIGIN + 1n]);
    assert.equal(h.of("play-step-done")[0].preOriginInputs, 1);
    await h.send({ kind: "play-stop", playId: 7 });
    assertReleased(h);
    assert.equal(h.of("play-stopped")[0].replayComplete, false);
    assert.deepEqual(game.disposals, ["stop", "take", "free"]);
    assert.equal(h.preparedOwners[1].frees, 0);
  }
  const empty = await active({ lanes: [], startRequest: startRequest({ inputMode: "physical", keyPairs: new Uint32Array() }) });
  assert.equal(empty.physicalConstructions[0].args[5].length, 0, "an empty chart does not acquire invented keyboard bindings");
  await empty.send(step());
  assert.equal(empty.games[0].calls.filter(row => row[0] === "blob").length, 0);
  await empty.send({ kind: "play-stop", playId: 7 });
  assertReleased(empty);
});

test("physical capability and whole-batch admission fail before consumption while encoded batches and committed failures never retry", async () => {
  for (const inputMode of [null, "", "legacy", "hid", 0]) {
    const gate = deferred(), h = await workerHarness({ initGate: gate });
    await h.send({ kind: "init", canvas: {} });
    await h.send(startRequest({ inputMode }));
    assert.equal(h.of("play-error").length, 1);
    assert.equal(h.preparedOwners.length, 0);
    assert.equal(h.games.length, 0);
    gate.resolve(); await flushJobs();
    assert.equal(h.games.length, 0);
  }
  for (const options of [{ missingPhysicalConstructor: true }, { missingInputBlob: true }]) {
    const h = await catalogWorker(options);
    await h.send(startRequest({ inputMode: "physical" }));
    assert.equal(h.physicalConstructions.length, 0);
    assert.equal(h.games.length, 0);
    assert.equal(h.of("play-error").length, 1);
    assert.ok(h.preparedOwners.slice(1).every(owner => !owner.moved && owner.frees === 1));
  }
  const selected = replayFile(), replay = await catalogWorker();
  await replay.send(replayRequest(selected.file, { inputMode: "physical" }));
  assert.equal(selected.reads, 0, "recorded playback refuses a live acquisition route before reading its recording");
  assert.equal(replay.games.length + replay.replays.length, 0);
  assert.equal(replay.of("play-error").length, 1);
  const refused = await catalogWorker({ physicalConstructError: "consuming physical setup refused" });
  await refused.send(startRequest({ inputMode: "physical" }));
  assert.equal(refused.physicalConstructions.length, 1);
  assert.equal(refused.games.length, 0);
  assert.equal(refused.preparedOwners[1].moved, true);
  assert.equal(refused.preparedOwners[1].frees, 0);
  assert.match(refused.of("play-error")[0].message, /consuming physical setup refused/);
  const malformed = await active({ startRequest: startRequest({ inputMode: "physical" }) });
  await malformed.send(step({ events: [
    { hostNs: ORIGIN, key: 2, down: true, sequence: 1n },
    { hostNs: ORIGIN + 1n, key: 3, down: false, sequence: 18446744073709551616n },
  ], watermark: ORIGIN + 1n }));
  assert.equal(malformed.games[0].calls.filter(row => ["input", "blob", "close"].includes(row[0])).length, 0);
  assert.equal(malformed.of("play-step-done").length, 0);
  assertReleased(malformed);
  const staged = await active({ startRequest: startRequest({ inputMode: "physical" }) });
  const owner = staged.games[0];
  const second = { hostNs: ORIGIN + 1n, down: true, sequence: 2n,
    get key() { return owner.calls.some(row => row[0] === "blob") ? 65536 : 3; } };
  await staged.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }, second], watermark: ORIGIN + 1n }));
  assert.equal(staged.of("play-error").length, 0);
  assert.equal(owner.calls.filter(row => row[0] === "blob").length, 2, "every packet is encoded before the first runtime call");
  assert.deepEqual(Array.from(owner.calls.filter(row => row[0] === "blob")[1][1]),
    Array.from(encodeKeyboardEvent({ hostNs: ORIGIN + 1n, key: 3, down: true, sequence: 2n })));
  await staged.send({ kind: "play-stop", playId: 7 });
  assertReleased(staged);
  const partial = await active({ startRequest: startRequest({ inputMode: "physical" }), inputBlob(game) {
    game.score.hits = 18n;
    throw new Error("actual common input rejected after committed score");
  } });
  await partial.send(step({ events: [
    { hostNs: ORIGIN, key: 2, down: true, sequence: 1n },
    { hostNs: ORIGIN + 1n, key: 3, down: true, sequence: 2n },
  ], watermark: ORIGIN + 1n }));
  assert.equal(partial.games[0].calls.filter(row => row[0] === "blob").length, 1);
  assert.equal(partial.games[0].calls.filter(row => row[0] === "close").length, 0);
  assertReleased(partial, { ...SCORE, hits: 18n });
  await partial.send(step({ tickId: 2 }));
  assert.equal(partial.games[0].calls.filter(row => row[0] === "blob").length, 1);
});

test("finite live ownership uses the consuming static constructor and snapshots actual endpoint metadata before capture and activation", async () => {
  const startNs = 604800000000001n, endNs = startNs + 1n;
  const timing = { earlyNs: 7000001n, lateNs: 9000002n, offsetNs: -3n };
  for (const finite of [false, true]) {
    const h = await started({
      startRequest: startRequest({ startNs, rate: 44100, timing, recordReplay: true, ...(finite ? { endNs } : {}) }),
      gameEndGetter(owner) { assert.equal(owner.endpointReads.end, 1); return finite ? endNs : undefined; },
      gameFrameGetter(owner) { assert.equal(owner.endpointReads.frame, 1); return finite ? 4411n : undefined; },
      observeOutput(owner) { owner.score.song_ns = endNs; return true; },
    });
    const game = h.games[0], metadata = h.of("play-reply")[0].result;
    assert.equal(h.sectionConstructions.length, finite ? 1 : 0);
    assert.deepEqual(h.libraries[0].preparations[1].args,
      [44100, 2, 18446744073709551615n, startNs, 64 * 1024 * 1024, 256 * 1024 * 1024, 3844]);
    assert.deepEqual(game.args.slice(0, 5), [0n, 100000000n, 7000001n, 9000002n, -3n]);
    assert.deepEqual(Array.from(game.args[5]), Array.from(pairs()));
    if (finite) {
      const construction = h.sectionConstructions[0];
      assert.equal(construction.prepared, h.preparedOwners[1]);
      assert.equal(construction.args.length, 7);
      assert.equal(construction.args[6], endNs, "end is the final static binding argument, not an input offset");
      assert.equal(game.constructedEnd, endNs);
      assert.equal(metadata.endNs, endNs);
      assert.equal(metadata.endFrame, 4411n);
    } else {
      assert.equal(Object.hasOwn(metadata, "endNs"), false);
      assert.equal(Object.hasOwn(metadata, "endFrame"), false);
    }
    assert.equal(metadata.startNs, startNs);
    assert.deepEqual(game.calls, [["capture", 64 * 1024 * 1024, 1000000]]);
    await h.rpc("play-sample"); await h.rpc("play-sample");
    assert.equal((await h.rpc("play-sample")).result.kind, "samples-end");
    assert.equal((await h.rpc("play-commands")).result, null);
    await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
    await h.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }], watermark: ORIGIN + 1n }));
    assert.deepEqual(game.calls.find(row => row[0] === "input"), ["input", ORIGIN, 2, true, 1n, h.acquisitionNowNs()]);
    await legacyWitness(h, 1, 0n, ORIGIN);
    assert.equal(h.of("play-render-done").at(-1).completed, false);
    await h.send({ kind: "play-render", playId: 7, renderId: 2, report: renderReport(), presentedNs: 100022676n, presentedHostNs: ORIGIN + 1n });
    assert.equal(h.of("play-render-done").at(-1).completed, true);
    assert.equal(game.stops, 0, "the returned completion proof still waits for the explicit owner stop");
    await h.send({ kind: "play-stop", playId: 7, completed: true });
    assertReleased(h, { ...SCORE, song_ns: endNs });
    assert.equal(h.of("play-stopped")[0].replayComplete, true);
    assert.deepEqual(game.disposals, ["stop", "take", "free"]);
    assert.deepEqual(game.endpointReads, { end: 1, frame: 1 });
    assert.equal(h.preparedOwners[1].frees, 0);
  }
  const replay = await started({ startRequest: replayRequest(replayFile().file, { endNs: "invalid live end", startNs: null }) });
  assert.equal(replay.sectionConstructions.length, 0);
  assert.equal(replay.games.length, 0);
  assert.equal(Object.hasOwn(replay.of("play-reply")[0].result, "endNs"), false);
  await replay.send({ kind: "play-stop", playId: 7 });
  assertReleased(replay);
});

test("live end preflight, consuming-constructor failures and contradictory actual getters cannot retry an unlimited owner", async () => {
  for (const endNs of [null, 10n, 9n, -1n, 11, "11", 9223372036854775808n]) {
    const initGate = deferred();
    const h = await workerHarness({ initGate });
    await h.send({ kind: "init", canvas: {} });
    await h.send(startRequest({ startNs: 10n, endNs }));
    assert.match(h.of("play-reply")[0].error, /section end/i);
    assert.equal(h.of("ready").length, 0);
    assert.equal(h.preparedOwners.length, 0);
    initGate.resolve(); await flushJobs();
    assert.equal(h.games.length, 0);
    assert.equal(h.sectionConstructions.length, 0);
  }
  for (const options of [
    {}, { gameEnd: 1n }, { gameEnd: 2n, gameEndFrame: 4801n },
    { gameEnd: 1n, gameEndFrame: 4800n }, { gameEnd: 1n, gameEndFrame: 4801 },
    { gameEndGetter() { throw new Error("actual live end getter failed"); } },
    { gameEnd: 1n, gameFrameGetter() { throw new Error("actual live frame getter failed"); } },
  ]) {
    const h = await catalogWorker(options);
    await h.send(startRequest({ endNs: 1n, recordReplay: true }));
    assert.equal(h.sectionConstructions.length, 1);
    assert.equal(h.games.length, 1);
    const game = h.games[0];
    assert.equal(game.calls.length, 0, "metadata admission precedes capture, samples and runtime operations");
    assert.equal(h.of("play-reply").filter(row => row.result?.kind === "prepared").length, 0);
    assert.equal(h.of("play-error").length, 1);
    assertReleased(h);
    assert.deepEqual(game.disposals, ["stop", "free"]);
    assert.equal(h.preparedOwners[1].frees, 0);
    assert.ok(game.endpointReads.end <= 1 && game.endpointReads.frame <= 1);
  }
  for (const missing of [false, true]) {
    const h = await catalogWorker(missing ? { missingSectionConstructor: true } : { sectionConstructError: "section constructor refused" });
    await h.send(startRequest({ endNs: 1n }));
    assert.equal(h.games.length, 0);
    assert.equal(h.sectionConstructions.length, missing ? 0 : 1);
    assert.equal(h.preparedOwners[1].moved, !missing);
    assert.equal(h.preparedOwners[1].frees, missing ? 1 : 0,
      "only preparation not passed to a consuming constructor remains a JS-owned resource");
    assert.equal(h.of("play-error").length, 1);
    assert.match(h.of("play-error")[0].message, missing ? /finite section ownership/ : /section constructor refused/);
    assert.equal(h.libraries[0].preparations.length, 2, "no second gameplay preparation or unlimited fallback");
  }
});

test("prepared ownership, original-rate PCM transfers and setup batches retain their exact identities", async () => {
  const first = batch(9007199254740993n);
  const second = batch(9007199254740994n);
  const h = await started({ batches: [first, second] });
  const game = h.games[0];
  const metadata = h.of("play-reply")[0].result;
  assert.deepEqual(structuredClone(metadata), { kind: "prepared", samples: 2, opponentCount: 0, startNs: 0n,
    title: "Actual prepared metadata", artist: "Fixture", notes: 23, lanes: [0x11, 0x12] });
  assert.deepEqual(h.libraries[0].preparations[1].args, [48000, 2, 18446744073709551615n, 64 * 1024 * 1024, 256 * 1024 * 1024, 3844]);
  assert.deepEqual(game.args.slice(0, 5), [0n, 100000000n, 50000000n, 50000000n, 0n]);
  assert.deepEqual(Array.from(game.args[5]), Array.from(pairs()));
  for (const [index, expected] of [[0, [19n, 44100, [0.25, -0.25, 0.5, -0.5]]], [1, [18446744073709551615n, 96000, [1, -1]]]]) {
    const reply = await h.rpc("play-sample");
    assert.equal(reply.result.id, expected[0]);
    assert.equal(reply.result.rate, expected[1]);
    assert.equal(reply.result.channels, 2);
    assert.deepEqual(Array.from(reply.result.pcm), expected[2]);
    assert.equal(game.samples[index].pcm.buffer.byteLength, 0, "the transferable buffer left its Worker owner");
    assert.equal(game.samples[index].takes, 1);
    assert.equal(game.samples[index].frees, 1);
  }
  assert.equal((await h.rpc("play-sample")).result.kind, "samples-end");
  assert.deepEqual((await h.rpc("play-commands")).result, first);
  await h.rpc("play-ack", { sequence: first.sequence, admitted: 2, success: true });
  assert.equal(game.calls.filter(value => value[0] === "commands").length, 1, "setup ACK does not consume the next batch");
  assert.equal(h.of("play-commands").length, 0);
  assert.deepEqual((await h.rpc("play-commands")).result, second);
  await h.rpc("play-ack", { sequence: second.sequence, admitted: 2, success: true });
  assert.equal((await h.rpc("play-commands")).result, null);
  await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
  assert.deepEqual(game.calls.find(value => value[0] === "activate"), ["activate", ORIGIN]);
  await h.tick();
  assert.equal(h.views[0].gameDraws[0], game);
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
  assert.equal(h.preparedOwners[1].frees, 0, "consumed preparation is not explicitly freed twice");
  await h.tick();
  const restored = h.visualExports.findLast(snapshot => snapshot.kind === 3);
  assert.equal(restored.owner, h.preparedOwners[0]);
  assert.equal(restored.songNs, 0n);
  assert.equal(h.visualAcks.at(-1).owner, restored.owner, "restored preview is fully acknowledged");
  assert.equal(h.views[0].current, h.preparedOwners[0], "accepted preview survives gameplay");
});

test("each playback owner retains its command batch limit across setup pulls, active pushes and exact acknowledgements", async () => {
  for (const [mode, requested, limit] of [["live", undefined, 256], ["live", 1, 1], ["live", 256, 256], ["replay", 2, 2]]) {
    const request = mode === "replay" ? replayRequest(replayFile().file, { commandBatchLimit: requested })
      : startRequest(requested === undefined ? {} : { commandBatchLimit: requested });
    const first = { sequence: 9007199254740993n, commands: Array.from({ length: Math.min(limit, 3) }, (_, index) => command(BigInt(index + 1))) };
    const next = { sequence: first.sequence + 1n, commands: [command(4n)] };
    const last = { sequence: first.sequence + 2n, commands: [command(5n)] };
    const h = await started({ startRequest: request, batches: [first, null, next, last, null] });
    const game = h.replays[0] ?? h.games[0];
    request.commandBatchLimit = 17;
    assert.deepEqual((await h.rpc("play-commands")).result, first);
    assert.deepEqual(game.calls.filter(row => row[0] === "commands"), [["commands", limit]]);
    await h.rpc("play-ack", { sequence: first.sequence, admitted: first.commands.length, success: true });
    assert.equal(game.calls.filter(row => row[0] === "commands").length, 1);
    assert.equal((await h.rpc("play-commands")).result, null);
    await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
    for (const id of [1, 2]) {
      await h.send(mode === "replay"
        ? { kind: "play-render", playId: 7, renderId: id, report: renderReport({ available: false }), presentedNs: null }
        : step({ tickId: id }));
    }
    assert.deepEqual(h.of("play-commands").map(value => value.batch), [next]);
    assert.equal(game.calls.filter(row => row[0] === "commands").length, 3, "the held batch blocks another pull while real steps continue");
    await h.send({ kind: "play-ack", playId: 7, sequence: next.sequence, admitted: 1, success: true });
    assert.deepEqual(h.of("play-commands").map(value => value.batch), [next, last]);
    await h.send({ kind: "play-ack", playId: 7, sequence: last.sequence, admitted: 1, success: true });
    assert.deepEqual(game.calls.filter(row => row[0] === "commands"), Array.from({ length: 5 }, () => ["commands", limit]));
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [
      ["ack", first.sequence, first.commands.length, true], ["ack", next.sequence, 1, true], ["ack", last.sequence, 1, true],
    ]);
    await h.send({ kind: "play-stop", playId: 7 });
    assertReleased(h);
  }
});

test("batch limits reject before readiness and oversized binding results or rejected prefixes never split or retry", async () => {
  for (const value of [null, 0, -1, 257, 1.5, "1", 1n, NaN, Infinity]) {
    const gate = deferred();
    const h = await workerHarness({ initGate: gate });
    await h.send({ kind: "init", canvas: {} });
    const file = replayFile();
    await h.send(replayRequest(file.file, { commandBatchLimit: value }));
    assert.equal(h.of("ready").length, 0);
    assert.match(h.of("play-reply")[0].error, /batch limit/i, "invalid transport limits cannot wait on WASM readiness");
    assert.equal(h.of("play-error").length, 1);
    assert.equal(file.reads, 0);
    assert.equal(h.preparedOwners.length, 0);
    gate.resolve(); await flushJobs();
    assert.equal(h.games.length + h.replays.length, 0);
    assert.equal(h.preparedOwners.length, 0);
  }
  for (const setup of [true, false]) {
    const tooLarge = { sequence: 91n, commands: [command(1n), command(2n), command(3n)] };
    const h = await started({ startRequest: startRequest({ commandBatchLimit: 2 }), batches: [tooLarge] });
    const game = h.games[0];
    if (setup) assert.match((await h.rpc("play-commands")).error, /command batch/i);
    else {
      await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
      await h.send(step());
    }
    assert.match(h.of("play-error")[0].message, /command batch/i);
    assert.equal(h.of("play-commands").length, 0, "no accepted-looking truncated prefix is published");
    assert.deepEqual(game.calls.filter(row => row[0] === "commands"), [["commands", 2]]);
    assert.equal(game.calls.filter(row => row[0] === "ack").length, 0);
    assert.equal(tooLarge.commands.length, 3);
    await h.send(step({ tickId: 2 }));
    assert.equal(game.calls.filter(row => row[0] === "commands").length, 1);
    assertReleased(h);
  }
  const rejectedBatch = batch(9007199254741993n);
  const rejected = await active({ startRequest: startRequest({ commandBatchLimit: 2 }), batches: [rejectedBatch, batch(92n)],
    ack() { throw new Error("actual owner retained rejected prefix"); } });
  await rejected.send(step());
  await rejected.send({ kind: "play-ack", playId: 7, sequence: rejectedBatch.sequence, admitted: 1, success: false });
  const game = rejected.games[0];
  assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [["ack", rejectedBatch.sequence, 1, false]]);
  assert.deepEqual(game.calls.filter(row => row[0] === "commands"), [["commands", 2]]);
  assert.deepEqual(rejected.of("play-commands")[0].batch, rejectedBatch);
  assert.equal(game.batches.length, 1, "no later batch or rejected remainder is consumed");
  assert.match(rejected.of("play-error")[0].message, /retained rejected prefix/);
  assertReleased(rejected);
});

test("remapped physical IDs reach the existing constructor and input path while invalid or incomplete bindings fail before consumption", async () => {
  const remapped = new Uint32Array([0x11, 100, 0x12, 101]);
  const h = await active({ startRequest: startRequest({ keyPairs: remapped }) });
  const game = h.games[0];
  remapped[1] = 2;
  assert.deepEqual(Array.from(game.args[5]), [0x11, 100, 0x12, 101]);
  await h.send(step({ events: [
    { hostNs: ORIGIN, key: 100, down: true, sequence: 1n },
    { hostNs: ORIGIN + 1n, key: 100, down: false, sequence: 2n },
    { hostNs: ORIGIN + 2n, key: 101, down: true, sequence: 3n },
  ], watermark: ORIGIN + 2n }));
  assert.deepEqual(game.calls.filter(call => call[0] === "input"), [
    ["input", ORIGIN, 100, true, 1n, h.acquisitionNowNs()],
    ["input", ORIGIN + 1n, 100, false, 2n, h.acquisitionNowNs()],
    ["input", ORIGIN + 2n, 101, true, 3n, h.acquisitionNowNs()],
  ]);
  await h.send(step({ tickId: 2, events: [
    { hostNs: ORIGIN + 3n, key: 2, down: true, sequence: 4n },
  ], watermark: ORIGIN + 3n }));
  assert.equal(game.calls.filter(call => call[0] === "input").length, 3, "old default key cannot enter the remapped owner");
  assertReleased(h);
  for (const keyPairs of [new Uint32Array([0x11, 100, 0x12, 100]), new Uint32Array([0x11, 0]),
    new Uint32Array([0x11, 65536]), new Uint32Array([0x11]), new Uint32Array([0x10, 100]), new Uint32Array(38)]) {
    const invalid = await catalogWorker();
    await invalid.send(startRequest({ keyPairs }));
    assert.equal(invalid.games.length, 0);
    assert.equal(invalid.libraries[0].preparations.length, 1, "invalid pairs fail before a second chart preparation");
    assert.equal(invalid.of("play-error").length, 1);
  }
  const uncovered = await catalogWorker();
  await uncovered.send(startRequest({ keyPairs: new Uint32Array([0x11, 100]) }));
  assert.equal(uncovered.games.length, 0);
  assert.equal(uncovered.preparedOwners[1].moved, false);
  assert.equal(uncovered.preparedOwners[1].frees, 1);
  const empty = await active({ lanes: [], startRequest: startRequest({ keyPairs: new Uint32Array() }) });
  assert.deepEqual(Array.from(empty.games[0].args[5]), []);
  await empty.send({ kind: "play-stop", playId: 7 });
  assertReleased(empty);
});

test("Window input provenance, pre-origin count and actual rendered cursor survive an outstanding audio batch", async () => {
  const h = await active({ batches: [batch(11n), batch(12n)] });
  const game = h.games[0];
  const events = [
    { hostNs: ORIGIN - 1n, key: 2, down: true, sequence: 9007199254740993n },
    { hostNs: ORIGIN, key: 2, down: false, sequence: 9007199254740994n },
    { hostNs: ORIGIN + 9n, key: 3, down: true, sequence: 9007199254740994n },
  ];
  await h.send(step({ events, watermark: ORIGIN + 10n, audioNs: 987654321n }));
  assert.deepEqual(game.calls.filter(value => ["input", "close"].includes(value[0])), [
    ["input", ORIGIN, 2, false, 9007199254740994n, h.acquisitionNowNs()],
    ["input", ORIGIN + 9n, 3, true, 9007199254740994n, h.acquisitionNowNs()],
    ["close", ORIGIN + 10n],
  ]);
  assert.equal(h.of("play-step-done")[0].preOriginInputs, 1);
  assert.equal(h.of("play-step-done")[0].hits, SCORE.hits);
  assert.deepEqual(h.of("play-commands")[0].batch, batch(11n));
  const pulls = game.calls.filter(value => value[0] === "commands").length;
  const unavailable = renderReport({ available: false });
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report: unavailable, presentedNs: null, presentedHostNs: null });
  assert.deepEqual(game.calls.find(value => value[0] === "output"), ["output", unavailable.words, null]);
  assert.deepEqual(h.of("play-render-done")[0], { kind: "play-render-done", playId: 7, renderId: 1,
    completed: false, commandsPending: true, observedTick: 1, pendingInputs: 2,
    songNs: SCORE.song_ns, hits: SCORE.hits, misses: SCORE.misses, combo: SCORE.combo, maxCombo: SCORE.max_combo, preOriginInputs: 1 });
  const actual = renderReport();
  await h.send({ kind: "play-render", playId: 7, renderId: 2, report: actual,
    presentedNs: 9007199254742999n, presentedHostNs: ORIGIN });
  assert.deepEqual(game.calls.filter(value => value[0] === "output")[1], ["output", actual.words, 9007199254742999n]);
  await h.send(step({ tickId: 2, watermark: ORIGIN + 20n }));
  assert.equal(game.calls.filter(value => value[0] === "commands").length, pulls);
  assert.equal(h.of("play-render-done").length, 2);
  await h.send({ kind: "play-ack", playId: 7, sequence: 11n, admitted: 2, success: true });
  assert.deepEqual(game.calls.find(value => value[0] === "ack"), ["ack", 11n, 2, true]);
  assert.deepEqual(h.of("play-commands")[1].batch, batch(12n));
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
});

test("a malformed later input rejects the whole bounded step before any binding mutation", async () => {
  const valid = { hostNs: ORIGIN, key: 2, down: true, sequence: 10n };
  const invalid = [
    { key: 99 }, { key: 2.5 }, { down: 1 }, { hostNs: -1n }, { hostNs: ORIGIN - 1n },
    { hostNs: 9223372036854775808n }, { hostNs: Number(ORIGIN) }, { sequence: 9n }, { sequence: 18446744073709551616n },
  ];
  const cases = invalid.map(fields => step({ events: [valid, { ...valid, hostNs: ORIGIN + 1n, ...fields }], watermark: ORIGIN + 2n }));
  cases.push(step({ events: [valid], watermark: ORIGIN - 1n }), step({ events: Array(257).fill(valid) }),
    step({ audioNs: -1n }), step({ tickId: 0 }), step({ watermark: 0 }));
  for (const request of cases) {
    const h = await active();
    await h.send(request);
    assert.equal(h.games[0].calls.filter(value => ["input", "close", "output", "commands"].includes(value[0])).length, 0);
    assert.equal(h.of("play-step-done").length, 0);
    assert.equal(h.of("play-error").length, 1);
    assertReleased(h);
  }
});

test("partial binding failures and rejected admitted prefixes retain real score and never retry", async () => {
  const partial = { song_ns: 9007199254742999n, hits: 18n, misses: 4n, combo: 0n };
  const h = await active({ input(game) {
    game.score = { ...partial };
    if (game.processedInput.length === 2) throw new Error("actual processing rejected committed prefix");
  } });
  await h.send(step({ events: [0n, 1n, 2n].map(offset => ({ hostNs: ORIGIN + offset, key: 2, down: true, sequence: offset })), watermark: ORIGIN + 3n }));
  assert.equal(h.games[0].calls.filter(value => value[0] === "input").length, 3);
  assert.equal(h.games[0].processedInput.length, 0, "admission alone cannot mutate score");
  await legacyWitness(h, 1, 100n, ORIGIN);
  await legacyWitness(h, 2, 103n, ORIGIN + 3n);
  assert.equal(h.games[0].processedInput.length, 2);
  assert.equal(h.games[0].pendingInput.length, 1);
  assert.equal(h.games[0].calls.filter(value => value[0] === "close").length, 1);
  assert.match(h.of("play-error")[0].message, /committed prefix/);
  assertReleased(h, partial);
  await h.send(step({ tickId: 2 }));
  assert.equal(h.of("play-error").length, 1);

  const rejected = await active({ batches: [batch(55n)], ack(game) {
    game.score = { ...partial };
    throw new Error("remote rejected after one admitted command");
  } });
  await rejected.send(step());
  await rejected.send({ kind: "play-ack", playId: 7, sequence: 55n, admitted: 1, success: false });
  assert.deepEqual(rejected.games[0].calls.filter(value => value[0] === "ack"), [["ack", 55n, 1, false]]);
  assert.equal(rejected.games[0].calls.filter(value => value[0] === "commands").length, 1);
  assertReleased(rejected, partial);
});

test("sample and constructor failures release their consumed owners and preserve the original error", async () => {
  const h = await started({ takeError: "original sample transfer failure", sampleFreeError: "secondary wrapper cleanup failure" });
  const reply = await h.rpc("play-sample");
  assert.match(reply.error, /original sample transfer failure/);
  assert.doesNotMatch(reply.error, /secondary wrapper/);
  assert.equal(h.games[0].samples[0].takes, 1);
  assert.equal(h.games[0].samples[0].frees, 1);
  assertReleased(h);

  const constructor = await catalogWorker({ constructError: "consuming constructor failed" });
  await constructor.send(startRequest());
  assert.equal(constructor.games.length, 0);
  assert.equal(constructor.preparedOwners[1].moved, true);
  assert.equal(constructor.preparedOwners[1].frees, 0);
  assert.equal(constructor.of("play-error")[0].hits, null);

  const missingLane = await catalogWorker({ lanes: [0x11, 0x13] });
  await missingLane.send(startRequest());
  assert.equal(missingLane.games.length, 0);
  assert.equal(missingLane.preparedOwners[1].moved, false);
  assert.equal(missingLane.preparedOwners[1].frees, 1);
});

test("stop cancels reserved asynchronous setup before any late preparation or owner creation", async () => {
  for (const gateName of ["initGate", "viewGate"]) {
    const gate = deferred();
    const h = await workerHarness({ [gateName]: gate });
    await h.send({ kind: "init", canvas: {} });
    h.post(startRequest());
    h.post({ kind: "play-stop", playId: 7 });
    await flushJobs();
    const stopped = h.of("play-stopped")[0];
    assert.equal(stopped.playId, 7);
    for (const field of ["songNs", "hits", "misses", "combo"]) assert.equal(stopped[field], null);
    assert.equal(h.of("play-reply")[0].rpcId, 1);
    assert.match(h.of("play-reply")[0].error, /stopped/i);
    gate.resolve();
    await flushJobs();
    assert.equal(h.games.length, 0);
    assert.equal(h.preparedOwners.length, 0);
    assert.equal(h.of("play-error").length, 0);
  }
  const ready = await catalogWorker();
  ready.post(startRequest());
  ready.post({ kind: "play-stop", playId: 7 });
  await flushJobs();
  assert.equal(ready.games.length, 0);
  assert.equal(ready.libraries[0].preparations.length, 1, "only the earlier preview was prepared");
  await ready.send(startRequest());
  assert.equal(ready.games.length, 0, "a stopped identity cannot be resurrected");
  await ready.send(startRequest({ playId: 8 }));
  assert.equal(ready.games.length, 1);
  await ready.send({ kind: "play-stop", playId: 8 });
  assertReleased(ready);
});

test("live owners reject library and preview mutations while stale play identities do nothing", async () => {
  const h = await active();
  const game = h.games[0];
  const positions = [...h.views[0].positions], exports = h.visualExports.length;
  let reads = 0;
  await h.send({ kind: "import", id: 8, files: [selectedFile("new.bms", () => { reads++; throw new Error("must not acquire while playing"); })] });
  await h.send({ kind: "accept-library", id: 8 });
  await h.send({ kind: "select", id: 9, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" });
  await h.send({ kind: "seek", id: 10, selectedId: 2, ns: "100" });
  assert.equal(reads, 0);
  assert.equal(h.libraries[0].preparations.length, 2);
  assert.deepEqual(h.views[0].positions, positions, "rejected seek cannot alter the renderer preview snapshot");
  assert.equal(h.visualExports.length, exports, "rejected library/preview work publishes no visual state");
  assert.equal(h.of("import-error").length, 2);
  assert.equal(h.of("selection-error").length, 1);
  assert.equal(h.of("seek-error").length, 1);
  const count = h.messages.length;
  await h.send(startRequest({ playId: 99 }));
  await h.send({ kind: "play-stop", playId: 6 });
  await h.send(step({ playId: 99 }));
  assert.equal(h.messages.length, count);
  assert.equal(game.frees, 0);
  await h.send({ kind: "resize", width: 800, height: 600 });
  await h.tick();
  assert.equal(h.views[0].gameDraws[0], game);
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
  await h.tick();
  const restored = h.visualExports.findLast(snapshot => snapshot.kind === 3);
  assert.equal(restored.owner, h.preparedOwners[0]);
  assert.equal(restored.songNs, 0n, "rejected live seek never changes the restored preview position");
  assert.equal(h.visualAcks.at(-1).owner, restored.owner);
});

test("RPC, tick and report fences prevent repeated consumption and reject faulty Mixer evidence", async () => {
  const repeated = await started();
  await repeated.rpc("play-sample");
  await repeated.send({ kind: "play-sample", playId: 7, rpcId: 2 });
  assert.equal(repeated.games[0].sampleIndex, 1);
  assertReleased(repeated);
  for (const next of [step(), step({ tickId: 2, watermark: ORIGIN - 1n })]) {
    const h = await active();
    await h.send(step());
    await h.send(next);
    assert.equal(h.games[0].calls.filter(value => value[0] === "close").length, 1);
    assert.equal(h.of("play-step-done").length, 1);
    assertReleased(h);
  }
  const render = await active();
  await render.send({ kind: "play-render", playId: 7, renderId: 2, report: renderReport(), presentedNs: null, presentedHostNs: null });
  await render.send({ kind: "play-render", playId: 7, renderId: 2, report: renderReport(), presentedNs: null, presentedHostNs: null });
  assert.equal(render.games[0].calls.filter(value => value[0] === "output").length, 1);
  assert.equal(render.of("play-render-done").length, 1);
  assertReleased(render);
  const unknownSample = renderReport();
  unknownSample.words[36] = 1;
  const terminal = renderReport();
  terminal.words[54] = 1;
  for (const faulty of [renderReport({ start: START + 1n }), unknownSample, terminal]) {
    const h = await active();
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: faulty, presentedNs: null, presentedHostNs: null });
    assert.equal(h.games[0].calls.filter(value => value[0] === "output").length, 0);
    assert.equal(h.of("play-render-done").length, 0);
    assertReleased(h);
  }
  const bounded = await active({ input() { throw new Error("x".repeat(5000)); } });
  await bounded.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }] }));
  await legacyWitness(bounded, 1, 100n, ORIGIN);
  await legacyWitness(bounded, 2, 101n, ORIGIN + 1n);
  assert.equal(bounded.of("play-error")[0].message.length, 4096);
  assertReleased(bounded);
});

test("actual completion result is correlated without disposing gameplay before its explicit stop", async () => {
  const h = await active({ observeOutput() { return true; } });
  const report = renderReport();
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report,
    presentedNs: 9223372036854775807n, presentedHostNs: ORIGIN });
  assert.deepEqual(h.of("play-render-done")[0], { kind: "play-render-done", playId: 7, renderId: 1,
    completed: true, commandsPending: false, observedTick: 0, pendingInputs: 0,
    songNs: SCORE.song_ns, hits: SCORE.hits, misses: SCORE.misses, combo: SCORE.combo, maxCombo: SCORE.max_combo, preOriginInputs: 0 });
  assert.equal(h.games[0].frees, 0);
  assert.equal(h.of("play-stopped").length, 0);
  await h.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: false, sequence: 1n }] }));
  assert.equal(h.games[0].calls.filter(value => value[0] === "input").length, 1,
    "captured input may join before the host releases this owner");
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
  for (const fault of ["stopError", "freeError"]) {
    const broken = await active({ [fault]: "actual gameplay disposal failed", observeOutput() { return true; } });
    await broken.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n, presentedHostNs: ORIGIN });
    assert.equal(broken.of("play-render-done")[0].completed, true);
    await broken.send({ kind: "play-stop", playId: 7 });
    assert.equal(broken.of("play-stopped").length, 0);
    assert.equal(broken.of("play-error")[0].released, false);
    assert.match(broken.of("play-error")[0].message, /disposal failed/);
    assertReleased(broken);
  }
});

test("malformed presentation and contradictory or failed completion cannot publish a successful receipt", async () => {
  for (const presentedNs of [undefined, -1n, 9223372036854775808n, 1, "1"]) {
    const h = await active();
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs, presentedHostNs: ORIGIN });
    assert.equal(h.games[0].calls.filter(value => value[0] === "output").length, 0);
    assert.equal(h.of("play-render-done").length, 0);
    assertReleased(h);
  }
  for (const result of ["true", 1]) {
    const h = await active({ observeOutput() { return result; } });
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n, presentedHostNs: ORIGIN });
    assert.equal(h.of("play-render-done").length, 0);
    assertReleased(h);
  }
  const outstanding = await active({ batches: [batch(81n)], observeOutput() { return true; } });
  await outstanding.send(step());
  await outstanding.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n, presentedHostNs: ORIGIN });
  assert.equal(outstanding.of("play-render-done").length, 0);
  assertReleased(outstanding);

  const retained = { song_ns: 888n, hits: 21n, misses: 5n, combo: 2n };
  const failed = await active({ observeOutput(game) {
    game.score = { ...retained };
    throw new Error("actual completion rejected the output domain");
  } });
  await failed.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n, presentedHostNs: ORIGIN });
  assert.equal(failed.of("play-render-done").length, 0);
  assert.match(failed.of("play-error")[0].message, /output domain/);
  assert.equal(failed.of("play-error")[0].released, true);
  assert.equal(failed.games[0].calls.filter(value => value[0] === "presentation").length, 1,
    "admitted evidence precedes the independently fallible completion evaluation");
  assertReleased(failed, retained);
});

test("paired observations are preflighted together and preserve order before a correlated receipt", async () => {
  const h = await active();
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport({ available: false }),
    presentedNs: null, presentedHostNs: null });
  assert.equal(h.games[0].calls.filter(value => value[0] === "presentation").length, 0);
  const actual = renderReport();
  await h.send({ kind: "play-render", playId: 7, renderId: 2, report: actual,
    presentedNs: 0n, presentedHostNs: ORIGIN - 1n });
  assert.deepEqual(h.games[0].calls.filter(value => ["output", "presentation"].includes(value[0])).slice(-2), [
    ["output", actual.words, 0n], ["presentation", 0n, ORIGIN - 1n],
  ]);
  assert.equal(h.of("play-render-done").at(-1).renderId, 2);
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
  for (const [presentedNs, presentedHostNs] of [[null, 1n], [1n, null], [null, undefined],
    [1n, -1n], [1n, 9223372036854775808n], [1n, 1], [1n, "1"]]) {
    const invalid = await active();
    await invalid.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(),
      presentedNs, presentedHostNs });
    assert.equal(invalid.games[0].calls.filter(value => ["output", "presentation"].includes(value[0])).length, 0);
    assert.equal(invalid.of("play-render-done").length, 0);
    assertReleased(invalid);
  }
  const fault = await active({ observePresentation() { throw new Error("actual clock phase bound exceeded"); } });
  await fault.send({ kind: "play-render", playId: 7, renderId: 1, report: actual,
    presentedNs: 1n, presentedHostNs: ORIGIN });
  assert.deepEqual(fault.games[0].calls.filter(value => ["output", "presentation"].includes(value[0])).map(value => value[0]),
    ["output", "presentation"]);
  assert.equal(fault.of("play-render-done").length, 0);
  assert.match(fault.of("play-error")[0].message, /phase bound/);
  assertReleased(fault);
});

test("recording is opt-in before preparation and transfers one stopped-owner prefix without copying its identity", async () => {
  for (const recordReplay of [undefined, false, true]) {
    const h = await active({ recordReplay });
    const game = h.games[0];
    assert.deepEqual(game.calls.filter(row => row[0] === "capture"),
      recordReplay ? [["capture", 64 * 1024 * 1024, 1000000]] : []);
    assert.equal(game.prepared.path, "song/chart.bms");
    assert.equal(h.libraries[0].preparations[1].args[2], 18446744073709551615n);
    await h.send({ kind: "play-stop", playId: 7 });
    const receipt = h.of("play-stopped")[0];
    assert.equal(receipt.replayComplete, false);
    assert.equal(receipt.replayError, null);
    assert.equal(game.replayTakes, recordReplay ? 1 : 0);
    assert.deepEqual(game.disposals, recordReplay ? ["stop", "take", "free"] : ["stop", "free"]);
    const transfer = h.transfers[h.messages.indexOf(receipt)];
    if (recordReplay) {
      assert.deepEqual(Array.from(receipt.replay), [66, 75, 82, 255, 0, 1]);
      assert.equal(receipt.replay.byteOffset, 0);
      assert.equal(receipt.replay.byteLength, receipt.replay.buffer.byteLength);
      assert.equal(transfer.length, 1);
      assert.equal(transfer[0], game.replayBytes.buffer);
      assert.equal(game.replayBytes.buffer.byteLength, 0, "owned bytes leave the Worker exactly once");
    } else {
      assert.equal(receipt.replay, null);
      assert.equal(transfer.length, 0);
    }
    await h.send({ kind: "play-stop", playId: 7 });
    assert.equal(h.of("play-stopped").length, 1);
    assertReleased(h);
  }
  for (const recordReplay of [null, 1, "true"]) {
    const h = await catalogWorker();
    await h.send(startRequest({ recordReplay }));
    assert.equal(h.libraries[0].preparations.length, 1, "invalid choice never prepares gameplay");
    assert.equal(h.games.length, 0);
    assert.match(h.of("play-error")[0].message, /recording choice/);
    assert.equal(h.of("play-error")[0].replay, null);
  }
});

test("complete capture requires current actual completion, while stale proof and operation failures retain only prefixes", async () => {
  const natural = await active({ recordReplay: true, observeOutput: () => true });
  await natural.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(),
    presentedNs: 123n, presentedHostNs: ORIGIN });
  await natural.send({ kind: "play-stop", playId: 7, completed: true });
  assert.equal(natural.of("play-stopped")[0].replayComplete, true);
  assert.equal(natural.of("play-stopped")[0].replayError, null);
  assertReleased(natural);

  for (const invalidation of ["no-proof", "input", "batch", "malformed-choice"]) {
    const h = await active({ recordReplay: true, observeOutput: () => true });
    if (invalidation !== "no-proof") {
      await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(),
        presentedNs: 1n, presentedHostNs: ORIGIN });
    }
    if (invalidation === "batch") h.games[0].batches.push(batch(10n));
    if (invalidation === "input" || invalidation === "batch") {
      await h.send(step({ events: invalidation === "input"
        ? [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }] : [] }));
    }
    await h.send({ kind: "play-stop", playId: 7, completed: invalidation === "malformed-choice" ? 1 : true });
    assert.equal(h.of("play-stopped").length, 0);
    const receipt = h.of("play-error")[0];
    assert.equal(receipt.replayComplete, false);
    assert.equal(receipt.released, true);
    assert.ok(receipt.replay instanceof Uint8Array);
    assertReleased(h);
  }
  const partial = await active({ recordReplay: true, input(game) {
    game.score.hits = 18n;
    throw new Error("committed input capture failed");
  } });
  await partial.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }] }));
  await legacyWitness(partial, 1, 100n, ORIGIN);
  await legacyWitness(partial, 2, 101n, ORIGIN + 1n);
  const receipt = partial.of("play-error")[0];
  assert.match(receipt.message, /committed input capture failed/);
  assert.equal(receipt.replayComplete, false);
  assert.equal(receipt.replayError, null);
  assertReleased(partial, { ...SCORE, hits: 18n });
});

test("serialization and transferable-layout failures stay separate from stop/free ownership failures", async () => {
  for (const options of [
    { replayError: "actual codec byte limit" },
    { replayBytes: () => null },
    { replayBytes: () => new Uint8Array(0) },
    { replayBytes: () => new Uint8Array([1, 2, 3]).subarray(1) },
    { replayBytes: () => new Uint8Array(64 * 1024 * 1024 + 1) },
  ]) {
    const h = await active({ ...options, recordReplay: true });
    await h.send({ kind: "play-stop", playId: 7 });
    const receipt = h.of("play-stopped")[0];
    assert.equal(receipt.replay, null);
    assert.equal(receipt.replayComplete, false);
    assert.match(receipt.replayError, options.replayError ? /actual codec byte limit/ : /transferable layout/);
    assert.equal(h.of("play-error").length, 0, "export failure does not invent a cleanup leak");
    assert.equal(h.transfers[h.messages.indexOf(receipt)].length, 0);
    assert.deepEqual(h.games[0].disposals, ["stop", "take", "free"]);
    assertReleased(h);
  }
  for (const fault of ["stopError", "freeError"]) {
    const h = await active({ recordReplay: true, [fault]: "actual owner cleanup failure" });
    await h.send({ kind: "play-stop", playId: 7 });
    const receipt = h.of("play-error")[0];
    assert.equal(receipt.released, false);
    assert.equal(receipt.replayComplete, false);
    assert.equal(receipt.replayError, null);
    assert.equal(h.games[0].replayTakes, fault === "stopError" ? 0 : 1);
    assert.equal(receipt.replay === null, fault === "stopError");
    assertReleased(h);
  }
  const setup = await catalogWorker({ captureError: "actual capture setup limit" });
  await setup.send(startRequest({ recordReplay: true }));
  assert.match(setup.of("play-error")[0].message, /actual capture setup limit/);
  assert.equal(setup.of("play-error")[0].replayComplete, false);
  assert.equal(setup.of("play-error")[0].replayError, null);
  assert.equal(setup.games[0].replayTakes, 0, "a refused capture was never admitted for export");
  assertReleased(setup);
});

test("replay reads once through canonical preparation and shares original PCM, ACK and output owners without live calls", async () => {
  const file = replayFile();
  const first = batch(101n);
  const next = batch(102n);
  const h = await started({ startRequest: replayRequest(file.file, { seed: "not a live seed", keyPairs: null }),
    batches: [first, null, next], observeOutput(game, words, presented) {
      if (presented !== null) game.score = { song_ns: 604800000000001n, hits: 23n, misses: 4n, combo: 11n };
      return false;
    } });
  assert.equal(file.reads, 1);
  assert.equal(h.games.length, 0);
  const replay = h.replays[0];
  const metadata = h.of("play-reply")[0].result;
  assert.equal(metadata.mode, "replay");
  assert.equal(metadata.recordedUntilNs, SCORE.song_ns);
  assert.deepEqual(replay.args, [100000000n]);
  assert.equal(h.libraries[0].preparations.length, 1, "only the accepted preview uses live-seed preparation");
  const preparation = h.libraries[0].replayPreparations[0];
  assert.deepEqual(preparation.bytes, file.bytes);
  assert.deepEqual(preparation.args, [48000, 2, 64 * 1024 * 1024, 256 * 1024 * 1024, 3844]);
  for (const rate of [44100, 96000]) {
    const sample = (await h.rpc("play-sample")).result;
    assert.equal(sample.rate, rate);
    assert.ok(sample.pcm instanceof Float32Array);
  }
  assert.equal((await h.rpc("play-sample")).result.kind, "samples-end");
  assert.deepEqual((await h.rpc("play-commands")).result, first);
  await h.rpc("play-ack", { sequence: first.sequence, admitted: 2, success: true });
  assert.equal((await h.rpc("play-commands")).result, null);
  await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport({ available: false }), presentedNs: null });
  assert.deepEqual(h.of("play-commands")[0].batch, next);
  const report = renderReport();
  await h.send({ kind: "play-render", playId: 7, renderId: 2, report, presentedNs: 9007199254742999n,
    presentedHostNs: "not used by replay" });
  assert.deepEqual(replay.calls.filter(row => row[0] === "output").at(-1), ["output", report.words, 9007199254742999n]);
  const progress = h.of("play-render-done").at(-1);
  assert.equal(progress.songNs, 604800000000001n);
  assert.equal(progress.hits, 23n);
  assert.equal(progress.preOriginInputs, 0);
  await h.tick();
  assert.equal(h.views[0].replayDraws.at(-1), replay);
  assert.equal(h.views[0].gameDraws.length, 0);
  await h.send({ kind: "play-ack", playId: 7, sequence: next.sequence, admitted: 2, success: true });
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h, replay.score);
  assert.deepEqual(replay.disposals, ["stop", "free"]);
  const stopped = h.of("play-stopped")[0];
  assert.equal(stopped.replay, null);
  assert.equal(stopped.replayComplete, false);
  assert.equal(stopped.replayError, null);
  assert.equal(file.reads, 1);
});

test("actual replay endpoint getters are read once and finite metadata crosses setup without changing the PCM or replay owner", async () => {
  for (const finite of [false, true]) {
    const selected = replayFile();
    const startNs = 604800000000001n, endNs = startNs + 1n;
    const h = await started({ replayStart: startNs,
      startRequest: replayRequest(selected.file, { rate: 44100 }),
      replayEndGetter(owner) { assert.equal(owner.endpointReads.end, 1); return finite ? endNs : undefined; },
      replayFrameGetter(owner) { assert.equal(owner.endpointReads.frame, 1); return finite ? 4411n : undefined; },
    });
    const owner = h.replays[0], result = h.of("play-reply")[0].result;
    assert.equal(result.startNs, startNs);
    assert.equal(Object.hasOwn(result, "endNs"), finite);
    assert.equal(Object.hasOwn(result, "endFrame"), finite);
    if (finite) {
      assert.equal(result.endNs, endNs);
      assert.equal(result.endFrame, 4411n);
    }
    assert.equal(h.libraries[0].replayPreparations[0].args[0], 44100);
    assert.equal((await h.rpc("play-sample")).result.rate, 44100);
    assert.equal((await h.rpc("play-sample")).result.rate, 96000);
    assert.equal((await h.rpc("play-sample")).result.kind, "samples-end");
    assert.equal((await h.rpc("play-commands")).result, null);
    await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
    await h.send({ kind: "play-stop", playId: 7 });
    assertReleased(h);
    assert.deepEqual(owner.endpointReads, { end: 1, frame: 1 });
    assert.deepEqual(owner.disposals, ["stop", "free"]);
    assert.equal(selected.reads, 1);
  }
});

test("malformed or throwing replay endpoint getters fail the consumed owner before samples and never fall back to unlimited setup", async () => {
  for (const options of [
    { replayEnd: 1n }, { replayEndFrame: 4801n }, { replayEnd: null, replayEndFrame: null },
    { replayEnd: 0n, replayEndFrame: 4800n }, { replayEnd: 1n, replayEndFrame: 4800n },
    { replayEnd: 1n, replayEndFrame: 4801 },
    { replayEndGetter() { throw new Error("actual end getter failed"); } },
    { replayEnd: 1n, replayFrameGetter() { throw new Error("actual frame getter failed"); } },
  ]) {
    const h = await catalogWorker(options);
    await h.send(replayRequest(replayFile().file));
    assert.equal(h.replays.length, 1);
    const owner = h.replays[0];
    assert.equal(owner.calls.filter(row => ["sample", "commands", "output"].includes(row[0])).length, 0);
    assert.equal(h.of("play-reply").filter(reply => reply.result?.kind === "prepared").length, 0);
    assert.equal(h.of("play-error").length, 1);
    assert.equal(h.of("play-error")[0].released, true);
    assertReleased(h);
    assert.equal(h.preparedOwners[1].moved, true);
    assert.equal(h.preparedOwners[1].frees, 0, "the consuming replay constructor already owns preparation");
    assert.ok(owner.endpointReads.end <= 1 && owner.endpointReads.frame <= 1);
    await h.send({ kind: "play-stop", playId: 7 });
    assert.deepEqual(owner.disposals, ["stop", "free"]);
  }
});

test("replay metadata is bounded before acquisition and invalid or changed reads never reach WASM preparation", async () => {
  for (const size of [0, -1, 1.5, 64 * 1024 * 1024 + 1, Number.MAX_SAFE_INTEGER + 1]) {
    const file = replayFile(null, size);
    const h = await catalogWorker();
    await h.send(replayRequest(file.file));
    assert.equal(file.reads, 0);
    assert.equal(h.libraries[0].replayPreparations.length, 0);
    assert.equal(h.replays.length, 0);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const fields of [{ replayFile: { size: 6, arrayBuffer() { assert.fail("unbranded file read"); } } },
    { recordReplay: true }, { mode: "unknown" }]) {
    const file = replayFile();
    const h = await catalogWorker();
    await h.send(replayRequest(file.file, fields));
    assert.equal(file.reads, 0);
    assert.equal(h.replays.length, 0);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const acquire of [() => Promise.resolve(new ArrayBuffer(5)),
    () => Promise.resolve(new Uint8Array(6)), () => Promise.reject(new Error("actual file acquisition failed"))]) {
    const file = replayFile(acquire);
    const h = await catalogWorker();
    await h.send(replayRequest(file.file));
    assert.equal(file.reads, 1);
    assert.equal(h.libraries[0].replayPreparations.length, 0);
    assert.equal(h.replays.length, 0);
    assert.equal(h.of("play-error")[0].released, true);
  }
  const file = replayFile();
  const incompatible = await catalogWorker({ prepareReplayError: "canonical chart identity mismatch" });
  await incompatible.send(replayRequest(file.file));
  assert.equal(file.reads, 1);
  assert.equal(incompatible.libraries[0].replayPreparations.length, 1);
  assert.equal(incompatible.replays.length, 0);
  assert.match(incompatible.of("play-error")[0].message, /canonical chart identity mismatch/);
});

test("a cancelled replay read cannot construct a late owner or replace a newer live session", async () => {
  const gate = deferred();
  const file = replayFile(() => gate.promise);
  const h = await catalogWorker();
  await h.send(replayRequest(file.file));
  assert.equal(file.reads, 1);
  assert.equal(h.replays.length, 0);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(h.of("play-stopped")[0].songNs, null);
  assert.equal(h.of("play-stopped")[0].replay, null);
  await h.send(startRequest({ playId: 8 }));
  assert.equal(h.games.length, 1);
  gate.resolve(file.bytes.slice().buffer);
  await flushJobs();
  assert.equal(h.replays.length, 0);
  assert.equal(h.libraries[0].replayPreparations.length, 0);
  assert.equal(h.games[0].frees, 0);
  assert.equal(h.of("play-error").length, 0);
  await h.send({ kind: "play-stop", playId: 8 });
  assertReleased(h);
  assert.equal(file.reads, 1);
});

test("replay rejects live steps and preserves output/ACK failures without inventing a completed capture", async () => {
  const start = () => replayRequest(replayFile().file);
  const natural = await active({ startRequest: start(), observeOutput: () => true, recordedUntil: null });
  assert.equal(natural.of("play-reply")[0].result.recordedUntilNs, null);
  await natural.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n });
  assert.equal(natural.of("play-render-done")[0].completed, true);
  await natural.send({ kind: "play-stop", playId: 7, completed: true });
  assert.equal(natural.of("play-stopped")[0].replayComplete, false);
  assertReleased(natural);
  for (const request of [step(), { kind: "play-render", playId: 7, renderId: 1,
    report: renderReport(), presentedNs: -1n }]) {
    const h = await active({ startRequest: start() });
    await h.send(request);
    assert.equal(h.replays[0].calls.filter(row => ["input", "close", "output"].includes(row[0])).length, 0);
    assert.equal(h.of("play-error")[0].replay, null);
    assertReleased(h);
  }
  const malformed = await active({ startRequest: start(), observeOutput: () => "true" });
  await malformed.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n });
  assert.equal(malformed.of("play-render-done").length, 0);
  assertReleased(malformed);
  const rejected = await active({ startRequest: start(), batches: [batch(83n)], ack() {
    throw new Error("actual replay batch rejected after one command");
  } });
  await rejected.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: null });
  await rejected.send({ kind: "play-ack", playId: 7, sequence: 83n, admitted: 1, success: false });
  assert.deepEqual(rejected.replays[0].calls.filter(row => row[0] === "ack"), [["ack", 83n, 1, false]]);
  assert.equal(rejected.replays[0].calls.filter(row => row[0] === "commands").length, 1);
  assert.match(rejected.of("play-error")[0].message, /after one command/);
  assert.equal(rejected.of("play-error")[0].replay, null);
  assertReleased(rejected);
});

const multiplayer = () => ({ url: "https://example.test:4433/competition", host: true, windowOriginNs: 9000000000n });

// Opaque group binding output, not a replacement for the shared Rust codec.
const LOCAL_NETWORK_PLAYERS = [99, 7, 31];
function groupNetworkWords(players = LOCAL_NETWORK_PLAYERS) {
  const rows = [
    [0, 0x80000000, 0xffffffff, 0xffffffff, 0, 0, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff],
    [0xffffffff, 0x7fffffff, 0, 0, 0xffffffff, 0xffffffff, 0, 0, 0, 0],
    [0xffffffff, 0xffffffff, 1, 0, 0, 0, 1, 0, 1, 0],
  ];
  return new Uint32Array(players.flatMap((player, index) => [player, ...rows[index % rows.length]]));
}
function localNetworkRequest(fields = {}) { return localRequest({ multiplayer: multiplayer(), ...fields }); }
async function activeLocalNetwork(options = {}) {
  return activeNetwork({ ...options, networkRoster: options.networkRoster ?? new Uint32Array([800, 4, 0xffffffff]),
    startRequest: options.startRequest ?? localNetworkRequest() });
}
async function finishGroupNetwork(h) {
  const owner = h.networks[0];
  await h.send({ kind: "play-stop", playId: 7 });
  if (!owner.closed) {
    owner.submissions.at(-1)?.gate.resolve(); await flushJobs();
    owner.ack.resolve(); await flushJobs();
  }
  return h.of("play-stopped").at(-1);
}

test("local network capabilities precede chart consumption and each peer reservation follows saved admission before touch", async () => {
  for (const capability of ["missingLocalIdentity", "missingLocalProgress", "missingLocalPeerConfigure",
    "missingLocalPeerUpdate", "missingLocalPeerDisable", "missingNetworkGroupConstructor"]) {
    const h = await catalogWorker({ allowNetworkClock: true, [capability]: true });
    await h.send(localNetworkRequest());
    assert.equal(h.localConstructions.length, 0); assert.equal(h.locals.length, 0);
    assert.equal(h.preparedOwners.at(-1).moved, false); assert.equal(h.preparedOwners.at(-1).frees, 1);
    assert.equal(h.networkSessions.length, 0); assert.equal(h.networks.length, 0);
    assert.ok(h.of("play-reply").at(-1).error);
  }
  const selected = replayFile();
  const h = await started({ allowNetworkClock: true, startRequest: localNetworkRequest({ recordReplay: true,
    opponents: [{ file: selected.file, sourceKey: "file:local-network", player: 99, own: true, label: "own prefix" }] }) });
  const game = h.locals[0];
  assert.deepEqual(game.memberPeerConfigurations, LOCAL_NETWORK_PLAYERS);
  const saved = game.calls.findIndex(call => call[0] === "local-add-opponent");
  const reservations = game.calls.flatMap((call, index) => call[0] === "local-peer-configure" ? [index] : []);
  const touch = game.calls.findIndex(call => call[0] === "local-touch");
  assert.ok(saved >= 0 && reservations.every(index => index > saved && index < touch));
  assert.equal(game.calls.filter(call => call[0] === "member-capture").length, 3);
  assert.equal(game.calls.filter(call => call[0] === "sample").length, 0);
  assert.equal(game.calls.filter(call => call[0] === "member-identity").length, 0);
  assert.equal(h.networks.length, 0);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.stops, 1); assert.equal(game.frees, 1);
  assert.deepEqual(h.of("play-stopped").at(-1).savedOpponents.localOpponents.map(row => row.player), LOCAL_NETWORK_PLAYERS);

  const refused = await catalogWorker({ allowNetworkClock: true, localPeerConfigureError: player => player === 7 });
  await refused.send(localNetworkRequest());
  assert.equal(refused.preparedOwners.at(-1).moved, true);
  assert.equal(refused.preparedOwners.at(-1).frees, 0);
  assert.equal(refused.locals[0].stops, 1); assert.equal(refused.locals[0].frees, 1);
  assert.equal(refused.locals[0].calls.some(call => call[0] === "local-touch"), false);
  assert.equal(refused.networks.length, 0);
});

test("peer target mappings are copied before awaits with local ownership checks and explicit repeated-remote or empty choices", async () => {
  const detached = new Uint32Array([99, 8]);
  structuredClone(detached.buffer, { transfer: [detached.buffer] });
  const invalid = [null, [], new Uint8Array(8), new Uint32Array([99]), new Uint32Array(130),
    new Uint32Array([99, 0]), new Uint32Array([1, 8]), new Uint32Array([99, 8, 99, 9]),
    new Uint32Array(new SharedArrayBuffer(8)), detached];
  const resizable = new ArrayBuffer(8, { maxByteLength: 16 });
  if (resizable.resizable === true) invalid.push(new Uint32Array(resizable));
  for (const peerTargets of invalid) {
    const h = await catalogWorker({ allowNetworkClock: true }); const before = h.preparedOwners.length;
    await h.send(localNetworkRequest({ multiplayer: { ...multiplayer(), peerTargets } }));
    assert.equal(h.preparedOwners.length, before); assert.equal(h.locals.length, 0);
    assert.equal(h.networks.length, 0); assert.ok(h.of("play-reply").at(-1).error);
  }
  const scalar = await catalogWorker({ allowNetworkClock: true });
  await scalar.send(startRequest({ multiplayer: { ...multiplayer(), peerTargets: new Uint32Array() } }));
  assert.equal(scalar.games.length, 0); assert.equal(scalar.networkSessions.length, 0);

  const h = await catalogWorker({ allowNetworkClock: true });
  const storage = new Uint32Array([999, 99, 8, 7, 8, 999]);
  const peerTargets = storage.subarray(1, 5);
  const preparing = h.send(localNetworkRequest({ multiplayer: { ...multiplayer(), peerTargets } }));
  peerTargets.fill(0);
  await preparing;
  assert.equal(h.of("play-reply").at(-1).result.kind, "prepared");
  withPlayRpc(h);
  while ((await h.rpc("play-sample")).result.kind !== "samples-end") {}
  assert.equal((await h.rpc("play-commands")).result, null);
  await requestNetwork(h);
  const owner = h.networks[0]; owner.emit({ kind: "roster", players: new Uint32Array([8, 9]) });
  owner.emit({ kind: "connected" }); owner.emit({ kind: "ready" });
  owner.emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 0n });
  await flushJobs();
  assert.equal((await h.rpc("play-activate", { hostNs: 2500000100n, startFrame: 123456n,
    targetHostNs: 2500000000n })).result, null);
  owner.emit({ kind: "group-progress", sequence: 0n, words: groupNetworkWords([8, 9]) });
  const rows = h.locals[0].memberPeerUpdates.filter(value => value.words.length === 10);
  assert.deepEqual(rows.map(row => row.player), [99, 7]);
  assert.deepEqual(rows[0].words, rows[1].words, "an explicit repeated remote mapping uses that actual member twice");
  const mapped = await finishGroupNetwork(h);
  assert.deepEqual(mapped.multiplayer.peers.map(row => row.remotePlayer), [8, 8, null]);
  assert.equal(h.locals[0].frees, 1);

  const empty = await activeLocalNetwork({ startRequest: localNetworkRequest({ multiplayer: {
    ...multiplayer(), peerTargets: new Uint32Array(),
  } }) });
  empty.networks[0].emit({ kind: "group-final-progress", sequence: 0n, words: groupNetworkWords([800, 4, 0xffffffff]) });
  assert.equal(empty.locals[0].memberPeerUpdates.some(row => row.words.length > 0), false);
  const receipt = await finishGroupNetwork(empty);
  assert.ok(receipt.multiplayer.peers.every(row => row.remotePlayer === null && row.progress === null && row.final === false));
});

test("all local canonical identities agree before one group session and the shared command-start activation handshake", async () => {
  const h = await started({ allowNetworkClock: true, batches: [batch(73n)], startRequest: localNetworkRequest() });
  const game = h.locals[0], port = commandPort();
  const audioRpc = await attachCommands(h, port);
  assert.equal(h.networkSessions.length, 0);
  assert.equal(game.calls.filter(call => call[0] === "member-identity").length, 0);
  await port.acknowledge();
  assert.equal(h.of("play-reply").find(row => row.rpcId === audioRpc).result.kind, "audio-ready");
  const networkRpc = await requestNetwork(h);
  assert.equal(h.networkSessions.length, 1); assert.equal(h.networks.length, 1);
  const session = h.networkSessions[0], owner = h.networks[0];
  assert.equal(session.group, true); assert.deepEqual(session.players, new Uint32Array(LOCAL_NETWORK_PLAYERS));
  assert.deepEqual(session.identity, [66, 75, 82, 0, 255]);
  assert.equal(session.preroll, 100000000n); assert.equal(owner.config.group, true);
  assert.deepEqual(game.calls.filter(call => call[0] === "member-identity").map(call => call[1]), LOCAL_NETWORK_PLAYERS);
  assert.ok(game.calls.findIndex(call => call[0] === "member-identity") > game.calls.findIndex(call => call[0] === "ack"));
  owner.emit({ kind: "roster", players: new Uint32Array([800]) });
  owner.emit({ kind: "connected" }); owner.emit({ kind: "ready" });
  assert.equal(h.of("play-reply").some(row => row.rpcId === networkRpc), false);
  owner.emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 7n });
  await flushJobs();
  const receipt = h.of("play-reply").find(row => row.rpcId === networkRpc).result;
  assert.deepEqual(receipt, { kind: "multiplayer-start", targetHostNs: 2500000000n,
    songTargetHostNs: 2600000000n, uncertaintyNs: 7n });
  assert.equal((await h.rpc("play-activate", { hostNs: 2500000100n, startFrame: 123456n,
    targetHostNs: receipt.targetHostNs })).result, null);
  assert.deepEqual(game.calls.filter(call => call[0] === "activate"), [["activate", 2500000100n]]);
  await finishGroupNetwork(h);
  assert.equal(game.frees, 1); assert.equal(port.closes, 1); assert.equal(session.frees, 1);

  const premature = await preparedNetwork({ startRequest: localNetworkRequest() });
  const earlyRpc = await requestNetwork(premature);
  premature.networks[0].emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 0n });
  await flushJobs();
  assert.ok(premature.of("play-reply").find(row => row.rpcId === earlyRpc).error);
  assert.equal(premature.locals[0].calls.some(call => call[0] === "activate"), false);
  assert.equal(premature.locals[0].frees, 1);

  for (const localIdentity of [player => Uint8Array.of(player === 7 ? 2 : 1),
    player => player === 7 ? new Uint8Array() : Uint8Array.of(1),
    player => player === 7 ? new Uint8Array(new SharedArrayBuffer(1)) : Uint8Array.of(1)]) {
    const refused = await preparedNetwork({ allowNetworkClock: true, localIdentity, startRequest: localNetworkRequest() });
    const rpc = await requestNetwork(refused);
    assert.ok(refused.of("play-reply").find(row => row.rpcId === rpc).error);
    assert.equal(refused.networkSessions.length, 0); assert.equal(refused.networks.length, 0);
    assert.equal(refused.locals[0].stops, 1); assert.equal(refused.locals[0].frees, 1);
  }
});

test("ordinal peers retain their own exact progress with unequal rosters and never send periodic member rows to Window", async () => {
  const h = await activeLocalNetwork({ networkRoster: new Uint32Array([800, 4]) });
  const game = h.locals[0], owner = h.networks[0];
  const untouched = structuredClone([...game.memberScores]);
  const words = groupNetworkWords([800, 4]);
  owner.emit({ kind: "group-progress", sequence: 0n, words });
  const progressUpdates = () => game.memberPeerUpdates.filter(row => row.words.length === 10);
  assert.deepEqual(progressUpdates().map(row => row.player), [99, 7]);
  assert.deepEqual(progressUpdates()[0].words, words.slice(1, 11));
  assert.deepEqual(progressUpdates()[1].words, words.slice(12, 22));
  owner.emit({ kind: "group-progress", sequence: 1n, words });
  owner.emit({ kind: "group-progress", sequence: 2n, words });
  assert.equal(progressUpdates().length, 2);
  h.setNetworkNow(1250); await h.runTimer(250);
  assert.equal(progressUpdates().length, 4, "one latest whole prefix refreshes both mapped rows at the shared cadence");
  owner.emit({ kind: "group-final-progress", sequence: 3n, words });
  assert.equal(progressUpdates().length, 6);
  assert.equal(game.memberPeerUpdates.some(row => row.player === 31 && row.words.length !== 0), false);
  assert.deepEqual([...game.memberScores], untouched);
  assert.equal(h.of("play-multiplayer").some(row => /progress|roster/.test(row.event.kind)), false);
  const receipt = await finishGroupNetwork(h);
  assert.equal(Object.hasOwn(receipt.multiplayer, "peer"), false, "group outcomes do not alias a primary scalar peer");
  assert.deepEqual(receipt.multiplayer.peers, [
    { player: 99, remotePlayer: 800, status: "stopped", final: true, error: null,
      progress: { songNs: -9223372036854775808n, hits: 18446744073709551615n, misses: 0n,
        combo: 18446744073709551615n, maxCombo: 18446744073709551615n } },
    { player: 7, remotePlayer: 4, status: "stopped", final: true, error: null,
      progress: { songNs: 9223372036854775807n, hits: 0n, misses: 18446744073709551615n, combo: 0n, maxCombo: 0n } },
    { player: 31, remotePlayer: null, status: "stopped", progress: null, final: false, error: null },
  ]);
  assert.equal(game.frees, 1); assert.equal(owner.closes, 1);

  const maximum = await activeLocalNetwork({ networkRoster: new Uint32Array(Array.from({ length: 64 }, (_, index) => index + 1)) });
  maximum.networks[0].emit({ kind: "group-final-progress", sequence: 0n,
    words: groupNetworkWords(Array.from({ length: 64 }, (_, index) => index + 1)) });
  const final = await finishGroupNetwork(maximum);
  assert.deepEqual(final.multiplayer.peers.map(row => row.remotePlayer), [1, 2, 3]);
  assert.equal(maximum.locals[0].memberPeerUpdates.filter(row => row.words.length === 10).length, 3);
});

test("invalid remote ownership fences only the active network while one member HUD failure preserves siblings and saved comparisons", async () => {
  for (const players of [new Uint32Array(), new Uint32Array([0]), new Uint32Array([8, 8]),
    new Uint32Array(65), new Uint32Array(new SharedArrayBuffer(4))]) {
    const h = await preparedNetwork({ startRequest: localNetworkRequest() });
    const rpc = await requestNetwork(h);
    h.networks[0].emit({ kind: "roster", players }); await flushJobs();
    assert.ok(h.of("play-reply").find(row => row.rpcId === rpc).error);
    assert.equal(h.locals[0].stops, 1); assert.equal(h.locals[0].frees, 1);
    assert.equal(h.networkSessions[0].frees, 1);
  }
  const missing = await preparedNetwork({ startRequest: localNetworkRequest({ multiplayer: {
    ...multiplayer(), peerTargets: new Uint32Array([99, 900]),
  } }) });
  await requestNetwork(missing);
  missing.networks[0].emit({ kind: "roster", players: new Uint32Array([800]) }); await flushJobs();
  assert.equal(missing.locals[0].frees, 1); assert.equal(missing.networks[0].closes, 1);

  for (const malformed of [
    { kind: "group-progress", sequence: 0n, words: groupNetworkWords([4, 800, 0xffffffff]) },
    { kind: "group-progress", sequence: 0n, words: groupNetworkWords([800, 4]) },
    { kind: "group-progress", sequence: -1n, words: groupNetworkWords([800, 4, 0xffffffff]) },
    { kind: "roster", players: new Uint32Array([800, 4, 0xffffffff]) },
    { kind: "progress", songNs: 0n, hits: 0n, misses: 0n, combo: 0n, maxCombo: 0n },
  ]) {
    const h = await activeLocalNetwork(); const game = h.locals[0], owner = h.networks[0];
    owner.emit({ kind: "group-progress", sequence: 0n, words: groupNetworkWords([800, 4, 0xffffffff]) });
    owner.emit(malformed); await flushJobs();
    assert.equal(owner.closes, 1); assert.equal(game.stops, 0); assert.equal(game.frees, 0);
    await h.send(step({ watermark: 2600000000n }));
    assert.equal(h.of("play-step-done").length, 1); assert.equal(h.of("play-error").length, 0);
    await h.send({ kind: "play-stop", playId: 7 });
    const receipt = h.of("play-stopped").at(-1);
    assert.equal(receipt.multiplayer.finalWritten, false);
    assert.ok(receipt.multiplayer.error);
    assert.deepEqual(receipt.multiplayer.peers.map(row => row.remotePlayer), [800, 4, 0xffffffff]);
    assert.ok(receipt.multiplayer.peers.every(row => row.status === "disconnected" && row.progress !== null));
    assert.equal(game.frees, 1);
  }

  const selected = replayFile();
  const h = await activeLocalNetwork({ localPeerUpdate(game, player, status, words) {
    if (player === 7 && words.length) throw new Error("member seven display failed");
  }, startRequest: localNetworkRequest({ recordReplay: true, opponents: [{ file: selected.file,
    sourceKey: "file:peer-independent", player: 7, own: false, label: "other prefix" }] }) });
  const game = h.locals[0], owner = h.networks[0];
  owner.emit({ kind: "group-progress", sequence: 0n, words: groupNetworkWords([800, 4, 0xffffffff]) });
  owner.emit({ kind: "group-final-progress", sequence: 1n, words: groupNetworkWords([800, 4, 0xffffffff]) });
  const notices = h.of("play-multiplayer").filter(row => row.event.kind === "peer-display-unavailable");
  assert.equal(notices.length, 1); assert.equal(notices[0].event.player, 7);
  assert.deepEqual([...game.memberPeerDisables], [[7, 1]]);
  assert.equal(game.memberHudDisables.size, 0); assert.equal(owner.closes, 0); assert.equal(game.stops, 0);
  const receipt = await finishGroupNetwork(h);
  assert.equal(receipt.multiplayer.error, null);
  assert.equal(receipt.multiplayer.finalWritten, true); assert.equal(receipt.multiplayer.finalAcknowledged, true);
  assert.match(receipt.multiplayer.peers[1].error, /member seven display failed/);
  assert.equal(receipt.multiplayer.peers[0].error, null); assert.equal(receipt.multiplayer.peers[2].error, null);
  assert.ok(receipt.multiplayer.peers.every(row => row.final && row.progress !== null));
  assert.equal(receipt.savedOpponents.localOpponents[1].opponents[0].label, "other prefix");
  assert.ok(receipt.replays.every(row => row.replay instanceof Uint8Array && row.replayError === null));
});

test("actual group word snapshots precede disposal and one pending write plus peer ACK retain final ownership", async () => {
  const output = groupNetworkWords();
  const original = output.slice();
  const h = await activeLocalNetwork({ localProgressWords: () => output, startRequest: localNetworkRequest({ recordReplay: true }) });
  const game = h.locals[0], owner = h.networks[0];
  h.setNetworkNow(1600); await h.send(step({ watermark: 2600000000n }));
  assert.equal(owner.submissions.length, 1); assert.equal(owner.submissions[0].group, true);
  assert.deepEqual(owner.submissions[0].value, original);
  h.setNetworkNow(1850); await h.send(step({ tickId: 2, watermark: 2850000000n }));
  assert.equal(owner.submissions.length, 1); assert.equal(game.groupProgressReads, 1);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.groupProgressReads, 2); assert.equal(game.stops, 1); assert.equal(game.frees, 1);
  assert.ok(game.disposals.lastIndexOf("group-progress") < game.disposals.indexOf("stop"));
  assert.equal(h.of("play-stopped").length, 0); assert.equal(owner.config.signal.aborted, false);
  output.fill(0);
  owner.emit({ kind: "group-final-progress", sequence: 0n, words: groupNetworkWords([800, 4, 0xffffffff]) });
  const hudWrites = game.memberPeerUpdates.length;
  owner.submissions[0].gate.resolve(); await flushJobs();
  assert.equal(owner.submissions.length, 2); assert.equal(owner.submissions[1].final, true);
  assert.deepEqual(owner.submissions[1].value, original, "final bytes were retained before the game was freed");
  owner.submissions[1].gate.resolve(); await flushJobs();
  assert.equal(owner.ackCalls, 1); assert.equal(h.of("play-stopped").length, 0);
  owner.ack.resolve(); await flushJobs();
  const receipt = h.of("play-stopped").at(-1);
  assert.equal(receipt.multiplayer.finalWritten, true); assert.equal(receipt.multiplayer.finalAcknowledged, true);
  assert.ok(receipt.multiplayer.peers.every(row => row.final && row.progress !== null));
  assert.equal(game.memberPeerUpdates.length, hudWrites);
  assert.equal(game.groupProgressReads, 2); assert.equal(h.networkSessions[0].frees, 1);

  for (const invalid of [() => { throw new Error("retained group snapshot refused"); },
    () => groupNetworkWords([7, 99, 31]), () => new Uint32Array(12),
    () => new Uint32Array(new SharedArrayBuffer(132))]) {
    const failed = await activeLocalNetwork({ localProgressWords: invalid, startRequest: localNetworkRequest({ recordReplay: true }) });
    await failed.send({ kind: "play-stop", playId: 7 });
    const result = failed.of("play-stopped").at(-1);
    assert.ok(result.multiplayer.error); assert.equal(result.multiplayer.finalWritten, false);
    assert.equal(result.multiplayer.finalAcknowledged, false); assert.equal(failed.networks[0].submissions.length, 0);
    assert.equal(failed.locals[0].stops, 1); assert.equal(failed.locals[0].frees, 1);
    assert.ok(result.replays.every(row => row.replay instanceof Uint8Array));
  }
});

test("cancelled group opens and stalled final drains cannot call freed members or publish into a newer gameplay owner", async () => {
  const opening = deferred();
  const h = await preparedNetwork({ networkOpenGate: opening, startRequest: localNetworkRequest() });
  const rpc = await requestNetwork(h); const old = h.networks[0], game = h.locals[0];
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.stops, 1); assert.equal(game.frees, 1);
  assert.ok(h.of("play-reply").find(row => row.rpcId === rpc).error);
  await h.send(startRequest({ playId: 8 }));
  opening.resolve(); await flushJobs();
  assert.equal(old.readyCalls, 0); assert.equal(old.closes, 1); assert.equal(h.networkSessions[0].frees, 1);
  const count = h.messages.length;
  old.emit({ kind: "roster", players: new Uint32Array([1]) });
  old.emit({ kind: "group-final-progress", sequence: 0n, words: groupNetworkWords([1]) });
  old.emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 0n });
  assert.equal(h.messages.length, count); assert.equal(h.games[0].stops, 0);
  await h.send({ kind: "play-stop", playId: 8 });

  for (const written of [false, true]) {
    const h = await activeLocalNetwork(); const owner = h.networks[0], game = h.locals[0];
    await h.send({ kind: "play-stop", playId: 7 });
    if (written) { owner.submissions[0].gate.resolve(); await flushJobs(); }
    await h.expireNetwork();
    const outcome = h.of("play-stopped").at(-1).multiplayer;
    assert.equal(outcome.finalWritten, written); assert.equal(outcome.finalAcknowledged, false);
    assert.match(outcome.error, /2 seconds|timed out/i);
    assert.equal(game.frees, 1); assert.equal(game.groupProgressReads, 1);
    await h.send(startRequest({ playId: 8 }));
    const count = h.messages.length, updates = game.memberPeerUpdates.length;
    owner.submissions[0].gate.resolve(); owner.ack.resolve();
    owner.emit({ kind: "final-acknowledged" }); owner.emit({ kind: "group-progress", sequence: 0n,
      words: groupNetworkWords([800, 4, 0xffffffff]) }); await flushJobs();
    assert.equal(h.messages.length, count); assert.equal(game.memberPeerUpdates.length, updates);
    assert.equal(h.games[0].stops, 0); assert.equal(h.networkSessions[0].frees, 1);
    await h.send({ kind: "play-stop", playId: 8 });
  }
});

test("peer progress updates only the Worker HUD, coalesces actual prefixes and retains final arrivals across gameplay disposal", async () => {
  for (const lateFinal of [false, true]) {
    const h = await activeNetwork(), owner = h.networks[0], game = h.games[0];
    assert.deepEqual(game.peerUpdates[0], { status: 0, words: new Uint32Array() });
    owner.emit({ kind: "connected" });
    const peer = { songNs: -1n, hits: 18446744073709551615n, misses: 0n, combo: 0n, maxCombo: 1n };
    owner.emit({ kind: "progress", ...peer });
    const progressUpdates = () => game.peerUpdates.filter(value => value.words.length === 10);
    assert.equal(progressUpdates().length, 1);
    assert.deepEqual(Array.from(progressUpdates()[0].words), [4294967295, 4294967295, 4294967295, 4294967295, 0, 0, 0, 0, 1, 0]);
    owner.emit({ kind: "progress", ...peer, songNs: 0n });
    owner.emit({ kind: "progress", ...peer, songNs: 604800000000001n });
    assert.equal(progressUpdates().length, 1, "rapid prefixes retain one latest pending display value");
    h.setNetworkNow(1250); await h.runTimer(250);
    assert.equal(progressUpdates().length, 2);
    const latest = progressUpdates().at(-1).words;
    assert.equal((BigInt(latest[1]) << 32n) | BigInt(latest[0]), 604800000000001n);
    const final = { ...peer, songNs: 604800000000002n };
    if (!lateFinal) {
      owner.emit({ kind: "final-progress", ...final });
      assert.equal(progressUpdates().length, 3, "a genuine final prefix bypasses the display cadence");
    }
    assert.equal(h.of("play-multiplayer").filter(value => ["progress", "final-progress"].includes(value.event.kind)).length, 0);
    await h.send({ kind: "play-stop", playId: 7 });
    assert.equal(game.frees, 1); assert.equal(owner.submissions.length, 1);
    assert.equal(h.of("play-stopped").length, 0);
    const afterFree = game.peerUpdates.length;
    if (lateFinal) owner.emit({ kind: "final-progress", ...final });
    assert.equal(game.peerUpdates.length, afterFree, "raw final evidence may arrive after Rust HUD ownership ended");
    owner.submissions[0].gate.resolve(); await flushJobs();
    assert.equal(owner.ackCalls, 1); assert.equal(h.of("play-stopped").length, 0);
    owner.emit({ kind: "final-acknowledged" }); owner.ack.resolve(); await flushJobs();
    const receipt = h.of("play-stopped").at(-1);
    assert.equal(receipt.multiplayer.finalWritten, true); assert.equal(receipt.multiplayer.finalAcknowledged, true);
    assert.equal(receipt.multiplayer.error, null);
    assert.deepEqual(receipt.multiplayer.peer, { status: "stopped", progress: final, final: true, error: null });
    assert.equal(game.peerDisables, 0); assertReleased(h);
    const messages = h.messages.length;
    owner.emit({ kind: "progress", ...peer }); await flushJobs();
    assert.equal(h.messages.length, messages); assert.equal(game.peerUpdates.length, afterFree);
  }
});

test("peer display failure is isolated from saved comparisons, local capture and genuine network final acknowledgements", async () => {
  for (const capability of ["missingPeerUpdate", "missingPeerDisable"]) {
    const refused = await catalogWorker({ [capability]: true });
    await refused.send(startRequest({ multiplayer: multiplayer() }));
    assert.ok(refused.of("play-reply").at(-1).error); assert.equal(refused.games.length, 0);
    assert.equal(refused.preparedOwners.at(-1).frees, 1); assert.equal(refused.networkSessions.length, 0);
    await refused.send(startRequest({ playId: 8 }));
    assert.equal(refused.of("play-reply").at(-1).result.kind, "prepared");
    await refused.send({ kind: "play-stop", playId: 8 });
    assert.equal(refused.games[0].peerUpdates.length, 0);
  }
  const selected = replayFile();
  const h = await activeNetwork({ disablePeerError: "peer hide failed",
    peerUpdate(game, status, words) { if (words.length) throw new Error("actual peer display failed"); },
    startRequest: startRequest({ multiplayer: multiplayer(), recordReplay: true,
      opponents: [{ file: selected.file, sourceKey: "file:1", own: true, label: "own" }] }) });
  const owner = h.networks[0], game = h.games[0]; owner.emit({ kind: "connected" });
  const first = { songNs: -1n, hits: 5n, misses: 0n, combo: 5n, maxCombo: 5n };
  owner.emit({ kind: "progress", ...first });
  const notices = () => h.of("play-multiplayer").filter(value => value.event.kind === "peer-display-unavailable");
  assert.equal(notices().length, 1); assert.match(notices()[0].event.error, /actual peer display failed.*peer hide failed/);
  assert.equal(game.peerDisables, 1); assert.equal(owner.closes, 0); assert.equal(game.stops, 0);
  const final = { ...first, songNs: 604800000000001n, hits: 6n, combo: 6n, maxCombo: 6n };
  owner.emit({ kind: "final-progress", ...final });
  h.setNetworkNow(1600); await h.send(step({ watermark: 2600000000n }));
  assert.equal(h.of("play-step-done").length, 1); assert.equal(game.savedReads, 1);
  assert.equal(game.hudDisables, 0); assert.equal(h.of("play-error").length, 0); assert.equal(notices().length, 1);
  owner.submissions[0].gate.resolve(); await flushJobs();
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(game.frees, 1); owner.submissions.at(-1).gate.resolve(); await flushJobs();
  owner.emit({ kind: "final-acknowledged" }); owner.ack.resolve(); await flushJobs();
  const receipt = h.of("play-stopped").at(-1);
  assert.deepEqual(receipt.multiplayer.peer.progress, final); assert.equal(receipt.multiplayer.peer.final, true);
  assert.match(receipt.multiplayer.peer.error, /actual peer display failed/);
  assert.equal(receipt.multiplayer.finalWritten, true); assert.equal(receipt.multiplayer.finalAcknowledged, true);
  assert.equal(receipt.multiplayer.error, null); assert.equal(receipt.savedOpponents.error, null);
  assert.equal(receipt.savedOpponents.opponents[0].label, "own");
  assert.ok(receipt.replay instanceof Uint8Array); assert.equal(receipt.replayError, null);
  assert.equal(receipt.replayComplete, false); assertReleased(h);
  await h.send(startRequest({ playId: 8 }));
  const count = h.messages.length; owner.emit({ kind: "final-progress", ...final });
  owner.disconnect(new Error("old callback")); await flushJobs();
  assert.equal(h.messages.length, count); assert.equal(h.games[1].peerUpdates.length, 0);
  await h.send({ kind: "play-stop", playId: 8 });
});

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

test("multiplayer identity and readiness follow sample exhaustion and actual initial command acknowledgements", async () => {
  const early = await started({ allowNetworkClock: true, startRequest: startRequest({ multiplayer: multiplayer() }) });
  assert.equal(early.networks.length, 0);
  assert.equal(early.games[0].calls.filter(call => call[0] === "identity").length, 0);
  const refused = await early.rpc("play-network-ready");
  assert.match(refused.error, /preparation|sample|readiness/i);
  assert.equal(early.networkSessions.length, 0);
  assertReleased(early);

  const opening = deferred();
  const h = await preparedNetwork({ batches: [batch(31n)], networkOpenGate: opening });
  assert.equal(h.networks.length, 0);
  const game = h.games[0];
  const rpcId = await requestNetwork(h);
  const owner = h.networks[0];
  assert.deepEqual(h.networkSessions[0].identity, [66, 75, 82, 0, 255]);
  assert.equal(h.networkSessions[0].host, true);
  assert.equal(h.networkSessions[0].preroll, 100000000n);
  assert.equal(owner.origin, 11000000000n, "Worker timeOrigin and performance.now retain their independent epoch");
  assert.equal(owner.readyCalls, 0, "pending transport open is not local readiness");
  assert.equal(h.of("play-reply").filter(value => value.rpcId === rpcId).length, 0);
  assert.ok(game.calls.findIndex(call => call[0] === "identity") > game.calls.findIndex(call => call[0] === "ack"));
  opening.resolve(); await flushJobs();
  assert.equal(owner.readyCalls, 1);
  owner.emit({ kind: "connected" }); owner.emit({ kind: "ready" });
  assert.equal(h.of("play-reply").filter(value => value.rpcId === rpcId).length, 0);
  owner.emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 7n });
  await flushJobs();
  assert.deepEqual(h.of("play-reply").find(value => value.rpcId === rpcId).result,
    { kind: "multiplayer-start", targetHostNs: 2500000000n, songTargetHostNs: 2600000000n, uncertaintyNs: 7n });
  const badActivation = await h.rpc("play-activate", { hostNs: 2500000000n,
    startFrame: 123456n, targetHostNs: 2500000001n });
  assert.ok(badActivation.error);
  assert.equal(game.calls.filter(call => call[0] === "activate").length, 0);
  assert.equal(owner.closes, 1);
  assert.equal(h.networkSessions[0].frees, 1);
  assertReleased(h);
});

test("actual score cadence stays bounded and remote or disconnected state never replaces local gameplay", async () => {
  const h = await activeNetwork();
  const game = h.games[0]; const owner = h.networks[0];
  h.setNetworkNow(1600);
  await h.send(step({ watermark: 2600000000n }));
  assert.equal(owner.submissions.length, 1);
  assert.deepEqual(owner.submissions[0].value, { songNs: SCORE.song_ns, hits: SCORE.hits,
    misses: SCORE.misses, combo: SCORE.combo, maxCombo: SCORE.max_combo });
  assert.equal(owner.submissions[0].final, false);
  h.setNetworkNow(1850);
  await h.send(step({ tickId: 2, watermark: 2850000000n }));
  assert.equal(owner.submissions.length, 1, "one pending write prevents an application queue from growing");
  owner.submissions[0].gate.resolve(); await flushJobs();
  game.score.hits = 18n; game.score.combo = 10n;
  h.setNetworkNow(1851);
  await h.send(step({ tickId: 3, watermark: 2851000000n }));
  assert.equal(owner.submissions.length, 2);
  assert.equal(owner.submissions[1].value.hits, 18n);
  owner.submissions[1].gate.resolve(); await flushJobs();
  h.setNetworkNow(2100);
  await h.send(step({ tickId: 4, watermark: 3100000000n }));
  assert.equal(owner.submissions.length, 2, "249 ms does not satisfy the score cadence");
  const peer = { songNs: -1n, hits: 18446744073709551615n, misses: 0n, combo: 0n, maxCombo: 1n };
  owner.emit({ kind: "progress", ...peer });
  owner.emit({ kind: "progress", ...peer, songNs: 0n });
  assert.equal(h.of("play-multiplayer").filter(value => value.event.kind === "progress").length, 0);
  owner.emit({ kind: "final-progress", ...peer, songNs: 1n });
  assert.equal(h.of("play-multiplayer").filter(value => value.event.kind === "final-progress").length, 0);
  assert.ok(game.peerUpdates.some(update => update.words.length === 10));
  assert.equal(game.score.hits, 18n);
  owner.disconnect(new Error("actual transport lost"));
  assert.match(h.of("play-multiplayer").at(-1).event.error, /transport lost/);
  assert.equal(game.stops, 0);
  await h.send(step({ tickId: 5, watermark: 3100000001n }));
  assert.equal(h.of("play-step-done").at(-1).tickId, 5);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(h.of("play-stopped").at(-1).multiplayer.finalWritten, false);
  assertReleased(h, game.score);
});

test("stop frees gameplay immediately but reports final write and peer ACK as separate bounded receipts", async () => {
  const h = await activeNetwork(); const owner = h.networks[0];
  h.setNetworkNow(1600);
  await h.send(step({ watermark: 2600000000n }));
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(h.games[0].stops, 1); assert.equal(h.games[0].frees, 1);
  assert.equal(owner.submissions.length, 1);
  assert.equal(owner.config.signal.aborted, false, "network final drain owns a separate lifetime after local game release");
  assert.equal(h.of("play-stopped").length, 0);
  owner.submissions[0].gate.resolve(); await flushJobs();
  assert.equal(owner.submissions.length, 2);
  assert.equal(owner.submissions[1].final, true);
  assert.equal(owner.submissions[1].value.maxCombo, SCORE.max_combo);
  owner.submissions[1].gate.resolve(); await flushJobs();
  assert.equal(owner.ackCalls, 1);
  assert.equal(h.of("play-stopped").length, 0, "local write alone cannot claim final application ACK");
  owner.emit({ kind: "final-acknowledged" }); owner.ack.resolve(); await flushJobs();
  const outcome = h.of("play-stopped")[0].multiplayer;
  assert.equal(outcome.finalWritten, true); assert.equal(outcome.finalAcknowledged, true); assert.equal(outcome.error, null);
  assert.equal(outcome.peer.progress, null); assert.equal(outcome.peer.final, false);
  assert.equal(owner.config.signal.aborted, true);
  assert.equal(owner.closes, 1); assert.equal(h.networkSessions[0].frees, 1);
  assertReleased(h);

  for (const wrote of [false, true]) {
    const stalled = await activeNetwork(); const old = stalled.networks[0];
    await stalled.send({ kind: "play-stop", playId: 7 });
    if (wrote) { old.submissions[0].gate.resolve(); await flushJobs(); }
    await stalled.expireNetwork();
    const receipt = stalled.of("play-stopped")[0];
    assert.equal(receipt.multiplayer.finalWritten, wrote);
    assert.equal(receipt.multiplayer.finalAcknowledged, false);
    assert.match(receipt.multiplayer.error, /2 seconds|timed out/i);
    await stalled.send(startRequest({ playId: 8 }));
    const newer = stalled.games[1];
    const messageCount = stalled.messages.length;
    old.submissions[0].gate.resolve(); old.ack.resolve();
    old.emit({ kind: "final-acknowledged" }); await flushJobs();
    assert.equal(stalled.messages.length, messageCount, "late final evidence cannot publish a second old-session receipt");
    assert.equal(newer.stops, 0);
    assert.equal(old.closes, 1); assert.equal(stalled.networkSessions[0].frees, 1);
    await stalled.send({ kind: "play-stop", playId: 8 });
  }
});

test("pre-activation failure and cancelled late network opens release identity and gameplay exactly once", async () => {
  for (const options of [{ identityError: "identity refused" }, { networkOpenError: "server refused" }]) {
    const h = await preparedNetwork(options);
    const rpcId = await requestNetwork(h);
    assert.ok(h.of("play-reply").find(value => value.rpcId === rpcId).error);
    assert.equal(h.of("play-error").length, 1);
    assertReleased(h);
    if (h.networkSessions.length) assert.equal(h.networkSessions[0].frees, 1);
  }
  const opening = deferred();
  const h = await preparedNetwork({ networkOpenGate: opening });
  const rpcId = await requestNetwork(h); const owner = h.networks[0];
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(owner.config.signal.aborted, true);
  assert.ok(h.of("play-reply").find(value => value.rpcId === rpcId).error);
  assertReleased(h);
  await h.send(startRequest({ playId: 8 }));
  opening.resolve(); await flushJobs();
  assert.equal(owner.readyCalls, 0);
  assert.equal(owner.closes, 1);
  assert.equal(h.networkSessions[0].closes, 1);
  assert.equal(h.networkSessions[0].frees, 1);
  const messages = h.messages.length;
  owner.emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 4n });
  assert.equal(h.messages.length, messages);
  assert.equal(h.games[1].stops, 0);
  await h.send({ kind: "play-stop", playId: 8 });
});


test("a pending resize keeps the ordered local frame ahead of a newer resize while input and audio continue", async () => {
  const request=localRequest({keyPairs:new Uint32Array(),recordReplay:true,
    localPlanWords:localPlan([[91,2n],[2,3n],[88,4n],[7,5n],[0xffffffff,6n]]),hidSetup:hidSetup([3n,4n,5n,6n])});
  const {h,port}=await directActive({startRequest:request}),game=h.locals[0];
  const base=h.of("render-geometry").at(-1).geometryVersion;
  h.renderPort.blocked=true;
  await h.send({kind:"resize",width:800,height:600,geometryVersion:base+1n});
  const firstResize=h.renderPort.posts.at(-1);
  assert.equal(firstResize.kind,"resize");assert.equal(firstResize.geometryVersion,base+1n);
  assert.equal((await h.rpc("play-page",{page:1,geometryVersion:base+2n})).result.page,1);
  await h.send({kind:"resize",width:960,height:720,geometryVersion:base+3n});
  const original=hidEvent({source:3n});
  await h.send(step({events:[original],watermark:ORIGIN+200000000n}));
  assert.equal(h.of("play-step-done").at(-1).pendingInputs,1);
  assert.deepEqual(game.pendingInput[0].bytes,encodeRawHidEvent(original));
  game.batches.push(batch(71n));
  await h.send(directObservation({presentedNs:0n,presentedHostNs:ORIGIN}));
  await port.acknowledge({report:renderReport()});
  if(port.posts.at(-1).kind==="commands")await port.acknowledge();
  assert.equal(h.of("play-render-done").at(-1).renderId,1);
  assert.ok(game.calls.some(row=>row[0]==="ack"&&row[1]===71n));
  assert.equal(h.renderPort.posts.at(-1),firstResize,"input and audio cannot reorder or wait for renderer controls");
  const baseline=game.visualBaseline;
  await h.renderPort.deliver(firstResize);
  const frame=h.renderPort.posts.at(-1);
  assert.equal(frame.kind,"packet");assert.equal(frame.geometryVersion,base+2n);
  const exported=h.visualExports.at(-1);
  assert.equal(exported.kind,2);assert.equal(exported.owner,game);assert.equal(exported.page,1);
  assert.equal(game.visualBaseline,baseline,"export alone cannot adopt pending state");
  await h.renderPort.deliver(frame);
  assert.equal(game.visualBaseline,exported.sequence,"exact full state ACK adopts the original snapshot");
  const frameGeometry=h.of("render-geometry").at(-1);
  assert.equal(frameGeometry.geometryVersion,base+2n);
  assert.deepEqual([frameGeometry.page,frameGeometry.width,frameGeometry.height],[1,800,600]);
  await h.tick();
  const finalResize=h.renderPort.posts.at(-1);
  assert.equal(finalResize.kind,"resize");assert.equal(finalResize.geometryVersion,base+3n);
  assert.ok(h.renderPort.posts.indexOf(frame)<h.renderPort.posts.indexOf(finalResize));
  await h.renderPort.deliver(finalResize);
  const resized=h.of("render-geometry").at(-1);
  assert.equal(resized.geometryVersion,base+3n);
  assert.deepEqual([resized.page,resized.width,resized.height],[1,960,720]);
  assert.equal(h.of("play-error").length,0);assert.equal(game.frees,0);
  await h.send({kind:"play-stop",playId:7});
  assert.ok(h.of("play-stopped").at(-1).replays.every(row=>row.replay instanceof Uint8Array));
});

test("a newer unsent page coalesces after an older queued resize without exporting the superseded frame", async () => {
  const request=localRequest({keyPairs:new Uint32Array(),
    localPlanWords:localPlan([[91,2n],[2,3n],[88,4n],[7,5n],[0xffffffff,6n]]),hidSetup:hidSetup([3n,4n,5n,6n])});
  const h=await active({startRequest:request}),game=h.locals[0];
  const base=h.of("render-geometry").at(-1).geometryVersion;
  h.renderPort.blocked=true;
  await h.send({kind:"resize",width:800,height:600,geometryVersion:base+1n});
  const firstResize=h.renderPort.posts.at(-1),exports=h.visualExports.length;
  assert.equal((await h.rpc("play-page",{page:1,geometryVersion:base+2n})).result.page,1);
  await h.send({kind:"resize",width:960,height:720,geometryVersion:base+3n});
  assert.equal((await h.rpc("play-page",{page:0,geometryVersion:base+4n})).result.page,0);
  assert.equal(h.visualExports.length,exports);
  await h.renderPort.deliver(firstResize);
  const resize=h.renderPort.posts.at(-1);
  assert.equal(resize.kind,"resize");assert.equal(resize.geometryVersion,base+3n);
  await h.renderPort.deliver(resize);
  const frame=h.renderPort.posts.at(-1);
  assert.equal(frame.kind,"packet");assert.equal(frame.geometryVersion,base+4n);
  assert.equal(h.visualExports.at(-1).page,0);
  assert.equal(h.visualExports.length,exports+1,"unsent page1 coalesces to the latest actual page0");
  const acknowledgedSequence=new DataView(frame.packet.buffer).getBigUint64(24,true);
  await h.renderPort.deliver(frame);
  assert.equal(game.visualBaseline,acknowledgedSequence,"producer adopts exactly the delivered frame even if a newer dirty snapshot is now pending");
  const geometry=h.of("render-geometry").at(-1);
  assert.equal(geometry.geometryVersion,base+4n);
  assert.deepEqual([geometry.page,geometry.width,geometry.height],[0,960,720]);
  assert.equal(h.of("play-error").length,0);
  await h.send({kind:"play-stop",playId:7});
});

test("every game mode registers explicitly including Local1", async () => {
  for (const [mode, request] of [["live",startRequest()],["local",roomStartRequest()],
    ["local",localRequest()],["replay",replayRequest(replayFile().file)]]) {
    const h=await started({startRequest:request});
    const registration=h.renderPort.posts.findLast(row=>row.kind==="packet"&&row.mode===mode);
    assert.ok(registration,`missing ${mode} registration`);
    assert.equal(new DataView(registration.packet.buffer).getUint16(6,true),1);
    const source=h.locals[0]??h.replays[0]??h.games[0];
    assert.equal(h.visualExports.findLast(row=>row.kind===1).owner,source);
    if(request.localPlanWords?.length===4)assert.equal(source.memberIds.length,1);
    assert.equal(h.of("fatal").length,0);
    await h.send({kind:"play-stop",playId:7});
  }
});

test("delayed local contacts retain their acquisition page after the current page changes", async () => {
  const request = localRequest({ keyPairs: new Uint32Array(), recordReplay: true,
    localPlanWords: localPlan([[91,2n],[2,3n],[88,4n],[7,5n],[0xffffffff,6n]]),
    hidSetup: hidSetup([3n,4n,5n,6n]) });
  const h = await active({ startRequest: request }), game = h.locals[0];
  const original = touchEvent({ page: 0, width: 320, height: 240, surfaceWidth: 640, surfaceHeight: 480,
    x: 101.25, y: 99.5, contact: 9007199254740993n });
  const move = { ...original, phase: 1, sequence: 2n, hostNs: ORIGIN + 1n, x: 115.75 };
  const release = { ...original, phase: 2, sequence: 3n, hostNs: ORIGIN + 2n, pressure: null };
  assert.equal((await h.rpc("play-page", { page: 1 })).result.page, 1);
  await h.send({ kind: "resize", width: 1920, height: 1080 });
  h.renderPort.blocked = true;
  await h.send(step({ events: [original, move, release], watermark: ORIGIN + 2n }));
  const routes = game.calls.filter(call => call[0] === "touch-page");
  assert.equal(routes.length, 3);
  assert.deepEqual(routes.map(call => call[1]), [0,0,0], "current page1 cannot reinterpret the original acquisition page0");
  for (const [index, event] of [original, move, release].entries()) {
    assert.deepEqual(routes[index][2], encodeTouchEvent(event));
    assert.deepEqual(routes[index].slice(3,7), [event.width,event.height,event.surfaceWidth,event.surfaceHeight]);
    assert.equal(game.pendingInput[index].host, event.hostNs);
    assert.equal(game.pendingInput[index].sequence, event.sequence);
    assert.equal(game.pendingInput[index].acquisitionPage, event.page);
  }
  assert.equal(h.of("play-error").length, 0);
  assert.equal(h.of("play-step-done").at(-1).pendingInputs, 3);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.ok(h.of("play-stopped").at(-1).replays.every(row => row.replay instanceof Uint8Array));
});

test("invalid or missing local acquisition page refuses the entire batch before any input mutation and preserves captures", async () => {
  for (const page of [undefined, null, -1, 0.5, 1, 4294967296, 0n, "0"]) {
    const h = await active({ startRequest: localRequest({recordReplay:true}) }), game = h.locals[0];
    const valid = touchEvent({page:0}), bad = touchEvent({page,hostNs:ORIGIN+1n,sequence:2n,contact:77n});
    if (page === undefined) delete bad.page;
    await h.send(step({events:[valid,bad],watermark:ORIGIN+1n}));
    assert.equal(game.calls.filter(call => ["input","blob","touch","touch-page"].includes(call[0])).length, 0);
    assert.equal(game.pendingInput.length, 0);
    const error = h.of("play-error").at(-1);
    assert.ok(error); assert.match(error.message, /page/i);
    assert.equal(game.frees, 1);
    assert.ok(error.replays.every(row => row.replay instanceof Uint8Array && row.replayComplete === false));
    assert.equal(h.of("play-step-done").length, 0);
  }
});
