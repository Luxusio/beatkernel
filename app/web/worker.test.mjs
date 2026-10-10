// Deferred: node --experimental-vm-modules --test app/web/*.test.mjs
// Loads the actual Worker and helper sources; no generated WASM or browser.
import assert from "node:assert/strict";
import { File as NodeFile } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { ROOM_SESSION_METHODS } from "./room-owner.mjs";
import { preflightMenuPacket } from "./render-protocol.mjs";
import { LocalRoster } from "./local-play-host.mjs";
import { KEY_BINDINGS } from "./play-model.mjs";
import { createContext, SourceTextModule, SyntheticModule, runInContext } from "node:vm";

const FileType = globalThis.File ?? NodeFile;
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
    self, structuredClone, DataView, File: FileType, TextEncoder, TextDecoder, Uint8Array, Uint32Array, Float32Array, ArrayBuffer,
    performance: self.performance,
    setTimeout(callback) { const id = ++timerId; timers.set(id, callback); return id; },
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
  const playHelpers = new SourceTextModule(await readFile(new URL("./play-model.mjs", import.meta.url), "utf8"), { context });
  const settingsHelpers = new SourceTextModule(await readFile(new URL("./settings-profile.mjs", import.meta.url), "utf8"), { context });
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
    if (specifier === "./render-protocol.mjs") return renderClient;
    if (specifier === "./pkg/beatkernel_bms_runtime.js") return wasm;
    if (specifier === "./host_model.mjs") return helpers;
    if (specifier === "./completed-results-model.mjs") return completedHelpers;
    if (specifier === "./play-model.mjs") return playHelpers;
    if (specifier === "./settings-profile.mjs") return settingsHelpers;
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
  return {
    renderPort, visualExports, visualAcks, messages, libraries, views, timers, games, preparedOwners, calls,
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
    await worker.send({ kind: "settings-profile-load", id: 1, file: profileFile });
    assert.equal(worker.of("settings-profile-loaded").length, 0);
    const errors = worker.of("menu-error").length;
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(accepted), fields: acquiredMenuFields([]) });
    assert.equal(worker.of("menu-error").length, errors + 1);
    assert.deepEqual(worker.of("menu-state").at(-1), accepted);
    assert.equal(worker.renderPort.posts.filter(post => post.kind === "menu").length, publications);
    await worker.send({ kind: "menu-action", ...acquiredMenuToken(accepted), actionId: 1n, control: 35n });
    assert.equal(worker.of("menu-error").length, errors + 2);
    assert.deepEqual(worker.of("menu-state").at(-1), accepted);
    read.resolve(new TextEncoder().encode(profile).buffer);
    await flushJobs();
    assert.equal(worker.of("settings-profile-loaded").length, 1);
    assert.equal(worker.of("settings-profile-error").length, 0);
    await worker.send({ kind: "menu-fields", ...acquiredMenuToken(accepted), fields: acquiredMenuFields([]) });
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
  await flushJobs();
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
  await flushJobs();
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
    await worker.send({ kind: "menu-record-preview", ...token, chartPath: "pending.bms", key: "prefix", replay: Uint8Array.from([66,75,82,1]) });
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
  assert.deepEqual(Array.from(final.replay), [66,75,82]);
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
