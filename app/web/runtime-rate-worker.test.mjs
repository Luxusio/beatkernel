// Coordinator runs: node --experimental-vm-modules --test app/web/runtime-rate-worker.test.mjs
// Real Worker, Window, RenderClient and audio clients; controlled generated
// Rust/DOM/port boundaries. This is not WASM, PCM or real-browser evidence.
import assert from "node:assert/strict";
import { File } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";

// A large absolute context that still fits the real signed-nanosecond grid at
// 48kHz: 441000000000000 frames = 9187500000000000000ns.
const PROGRAM = 41n, CONTEXT = 441000000000000n;
const MiB = 1024 * 1024;
function words(values) {
  return new Uint32Array(values.flatMap(value => {
    const n = BigInt.asUintN(64, BigInt(value));
    return [Number(n & 0xffffffffn), Number(n >> 32n)];
  }));
}
function at(value, index) { return BigInt(value[index * 2]) | BigInt(value[index * 2 + 1]) << 32n; }
function capability({ generation = 1n, revision = 0n, numerator = 1n, denominator = 1n } = {}) {
  return words([1n, PROGRAM, generation, 0n, 2000000000n, revision, numerator, denominator]);
}
function envelope({ tag = 2n, serial = 9n, id = 3n, generation = 1n, revision = 1n,
  numerator = 2n, denominator = 1n, outcome = 0n, boundary = 1n, paused = false } = {}) {
  const values = Array(40).fill(0n);
  Object.assign(values, { 0: 1n, 1: tag, 2: PROGRAM, 3: serial, 4: id, 5: generation,
    7: 4801n, 8: 4801n, 35: CONTEXT });
  if (tag === 4n) values[36] = BigInt(paused);
  else {
    Object.assign(values, { 26: 4801n, 28: 0n, 29: 4801n, 30: 48000n,
      31: 4801n, 32: 48000n, 33: 1n });
    if (tag === 1n || tag === 3n) Object.assign(values, { 22: 1000000000n,
      23: 1000000000n, 25: boundary });
    if (tag === 1n && boundary === 0n) Object.assign(values, {
      7: 0n, 8: 0n, 22: 0n, 23: 0n, 26: 0n, 29: 0n, 30: 1n, 31: 0n });
    if (tag === 2n || tag === 3n) Object.assign(values, { 9: revision - 1n,
      10: outcome === 2n ? revision - 1n : revision, 11: 1n, 12: 1n, 13: numerator, 14: denominator,
      15: outcome === 2n ? 1n : numerator, 16: outcome === 2n ? 1n : denominator,
      17: outcome, 18: outcome === 2n ? 1n : 0n, 20: tag === 2n ? 100020833n : 0n });
  }
  return words(values);
}
function snapshot(event = null) {
  const rate=event!==null&&(at(event,1)===2n||at(event,1)===3n);
  return { metadata: words([event===null?1n:at(event,5),rate?at(event,10):0n,
    rate?at(event,15):1n,rate?at(event,16):1n,1n,event!==null&&at(event,1)===4n&&at(event,36)===1n?0n:1n]),
    history: words([1n, PROGRAM, CONTEXT, 48000n, 0n, 0n, 0n, 5000n, 0n, 1n,
      0n, 0n, 0n, 0n, 1n]), event };
}

// Extend only the generated binding fixture. The actual source modules are
// loaded unchanged by the established VM harness.
const workerFixtureUrl = new URL("./play-worker.test.mjs", import.meta.url);
const workerOriginal = await readFile(workerFixtureUrl, "utf8");
const boundary = 'test("actual Worker settings codec';
assert.equal(workerOriginal.split(boundary).length, 2);
let workerFixture = workerOriginal.slice(0, workerOriginal.indexOf(boundary));
workerFixture = workerFixture.replaceAll("import.meta.url", JSON.stringify(workerFixtureUrl.href));
workerFixture = workerFixture.replace(/from "(\.\/[^\"]+)"/g,
  (_, path) => `from ${JSON.stringify(new URL(path, workerFixtureUrl).href)}`);
function replaceFixture(anchor, replacement) {
  assert.equal(workerFixture.split(anchor).length, 2, `unique binding fixture seam: ${anchor}`);
  workerFixture = workerFixture.replace(anchor, replacement);
}
replaceFixture('const START = 9007199254741999n;', 'const START = 441000000000000n;');
replaceFixture('cursor = 9007199254742999n', 'cursor = 441000000001000n');
replaceFixture('        advances: 0, takes: 0, cancels: 0, frees: 0,', `
        retained: false, advances: 0, takes: 0, cancels: 0, frees: 0,
        enable_retained_practice() {
          assert.equal(job.advances, 0, "retained opt-in precedes first advance");
          assert.equal(job.retained, false); assert.equal(replay, false);
          job.retained = true;
        },`);
replaceFixture('          prepared = this[method](...args);', `
          prepared = this[method](...args);
          prepared.retained_practice = job.retained;`);
replaceFixture('  if (options.missingSectionConstructor)', `
  function fixtureWords(values) {
    return new Uint32Array(values.flatMap(value => {
      const n = BigInt.asUintN(64, BigInt(value));
      return [Number(n & 0xffffffffn), Number(n >> 32n)];
    }));
  }
  function fixtureAt(value, index) { return BigInt(value[index*2]) | BigInt(value[index*2+1]) << 32n; }
  function installSpeed(Type, names) {
    for (const [name, original] of names) Type[name] = function(prepared, ...args) {
      const fraction = args.pop();
      const owner = original ? original.call(this, prepared, ...args) : new this(prepared, ...args);
      owner.liveSelected = fraction.split("/").map(BigInt); return owner;
    };
  }
  installSpeed(BrowserGame, [["new_and_speed", null], ["new_section_and_speed", BrowserGame.new_section],
    ["new_physical_and_speed", BrowserGame.new_physical], ["new_physical_contact_and_speed", BrowserGame.new_physical_contact]]);
  Object.defineProperty(BrowserGame.prototype, "speed_numerator", { get() { return this.liveSelected?.[0] ?? 1n; } });
  Object.defineProperty(BrowserGame.prototype, "speed_denominator", { get() { return this.liveSelected?.[1] ?? 1n; } });
  Object.assign(BrowserGame.prototype, {
    configure_retained_limits(...limits) {
      this.live(); assert.equal(this.prepared.retained_practice, true);
      assert.equal(this.calls.some(row => row[0] === "capture"), false);
      this.retainedLimits = limits; this.calls.push(["retained-limits", ...limits]);
    },
    retained_capability_words() {
      this.live();
      if(options.capabilityWords)return options.capabilityWords.slice();
      if (!this.prepared.retained_practice || this.retainedReady !== true || this.retainedEnded === true) return new Uint32Array();
      return fixtureWords([1n,41n,this.retainedGeneration??1n,0n,2000000000n,
        this.retainedRevision??0n,this.retainedNumerator??1n,this.retainedDenominator??1n]);
    },
    retained_setup_words() {
      this.live(); assert.ok(this.retainedLimits, "negotiated limits precede setup export");
      this.calls.push(["retained-setup"]);
      return fixtureWords([1n,41n,this.prepared.start_ns,2000000000n,0n,1n,1n,100000000n,0n,2n,2n,...this.retainedLimits.map(BigInt)]);
    },
    retained_cue_count() { return 2; },
    retained_cue_words(index) {
      this.calls.push(["retained-cue",index]); assert.ok(index===0||index===1);
      // Original song cue IDs/timestamps, including the cue before a 1s start.
      return new Uint32Array([index+7,0,index===0?19:0xffffffff,index===0?0:0xffffffff,
        index===0?0:1500000000,0,0x3f000000,0,0]);
    },
    retained_prepare_rate(id,n,d) {
      this.live(); this.calls.push(["prepare-rate",id,n,d]);
      if (options.rateError) throw new Error(options.rateError);
      return fixtureWords([2n,41n,48000n,2n,n,d,2n,2000000000n,1n,77n]);
    },
    retained_prepare_region(id,start,end,repeat) {
      this.live(); this.calls.push(["prepare-region",id,start,end,repeat]);
    },
    retained_prepare_disable_loop(id) { this.calls.push(["prepare-disable-loop",id]); },
    retained_prepare_original_restart(id) {
      this.calls.push(["prepare-restart",id]); return fixtureWords([2n,41n,48000n,2n,1n,1n,2n,2000000000n,1n,77n]);
    },
    retained_prepare_pause(id,paused) { this.calls.push(["prepare-pause",id,paused]); },
    retained_admission(kind,id,accepted,error) { this.calls.push(["admission",kind,id,accepted,error]); },
    retained_observe_presentation(audio,host) { this.calls.push(["retained-presentation",audio,host]); },
    retained_history(value) { this.calls.push(["retained-history",value.slice()]); },
    retained_event(value) { this.calls.push(["retained-event",value.slice()]); this.retainedHeld=value.slice(); },
    retained_service(now,audio) {
      this.calls.push(["retained-service",now,audio]);
      if (!this.allowCommit || !this.retainedHeld) return undefined;
      const event=this.retainedHeld;
      if(fixtureAt(event,4)!==0n)this.retainedCommitted=event;
      this.retainedGeneration=fixtureAt(event,5);
      if(fixtureAt(event,1)===1n&&fixtureAt(event,25)===0n)this.retainedReady=true;
      if(fixtureAt(event,1)===1n&&fixtureAt(event,25)===3n)this.retainedEnded=true;
      if(fixtureAt(event,1)===2n||fixtureAt(event,1)===3n) {
        this.retainedRevision=fixtureAt(event,10);
        this.retainedNumerator=fixtureAt(event,15); this.retainedDenominator=fixtureAt(event,16);
      }
      return fixtureAt(event,3);
    },
    retained_response_words() {
      const result=this.retainedCommitted;
      this.retainedCommitted=null; return result?.slice() ?? new Uint32Array();
    },
    retained_event_ack(serial) {
      assert.ok(this.retainedHeld); assert.equal(serial,fixtureAt(this.retainedHeld,3));
      this.calls.push(["retained-local-ack",serial]); this.retainedHeld=null;
    },
    retained_history_pin() { this.calls.push(["retained-pin"]); return this.retainedPin; },
    take_retired_replay(player=1) {
      this.live(); this.calls.push(["take-retired",player]);
      if(options.retiredErrorPlayer===player) throw new Error("member prefix encoding failed");
      return this.retiredBytes?.delete(player) ? Uint8Array.from([66,75,82,player,9]) : undefined;
    },
  });
  if (options.missingSectionConstructor)`);
// The local class inherits all additive retained exports, but its speed
// constructor must keep the actual local roster fixture rather than solo.
const localAnchor = '  if (options.missingLocalTouchSurfacePage)';
replaceFixture(localAnchor, `  installSpeed(BrowserLocalGame, [["new_physical_and_speed", BrowserLocalGame.new_physical]]);\n${localAnchor}`);
replaceFixture('      transfers.push([...transfer]);', `
      if(options.postFailure?.(value)) throw new Error("retired File post failed");
      transfers.push([...transfer]);`);
workerFixture += workerOriginal.slice(workerOriginal.indexOf('function hidSetup('),workerOriginal.indexOf('function hidEvent('));
workerFixture += '\nexport { workerHarness, selectedFile, deferred, flushJobs, pairs, renderReport, ORIGIN, START, hidSetup };\n';
const { workerHarness, selectedFile, deferred, flushJobs, pairs, renderReport, ORIGIN, START, hidSetup } =
  await import(`data:text/javascript;base64,${Buffer.from(workerFixture).toString("base64")}`);

function start(fields = {}) {
  return { kind:"play-start",playId:7,rpcId:1,libraryId:1,path:"song/chart.bms",rate:48000,
    seed:"7",keyPairs:pairs(),windowOriginNs:10000000000n,retained:true,
    audioLimits:{queueCapacity:4096,maxVoices:17,pendingCapacity:4096,maxFrames:257,maxCommandsPerRender:71},
    ...fields };
}
async function catalog(options = {},file=selectedFile("song/chart.bms")) {
  const h=await workerHarness(options);
  await h.send({kind:"init",canvas:{transferred:true}});
  await h.send({kind:"import",id:1,files:[file]});
  await h.send({kind:"accept-library",id:1});
  await h.send({kind:"resize",width:640,height:480});
  h.nextRpc=1; return h;
}
function reply(h,id) { return h.of("play-reply").find(row=>row.rpcId===id); }
async function request(h,action,fields={}) {
  const rpcId=++h.nextRpc;
  await h.send({kind:"play-retained",version:1,playId:7,rpcId,action,...fields});
  return rpcId;
}
function port() {
  return {posts:[],pending:null,onmessage:null,onmessageerror:null,closes:0,start(){},close(){this.closes++;},
    postMessage(value,transfer=[]) { const message=structuredClone(value,{transfer});this.posts.push(message);this.pending=message; },
    async ack(request=this.pending,fields={}) {
      assert.ok(request);
      if(this.pending===request)this.pending=null;
      const retained=request.kind.startsWith("retained-");
      this.onmessage?.({data:{kind:"ack",generation:request.generation,sequence:request.sequence,
        operation:request.kind,status:0,admitted:0,error:null,report:null,
        ...(retained?{version:1,programId:PROGRAM,terminal:false,retained:null}:{}),...fields}});
      await flushJobs();
    },
  };
}
async function begin(h,fields={}) {
  await h.send(start(fields));
  assert.ok(reply(h,1)?.result,"retained owner prepared");
  h.owner=h.locals[0]??h.games[0]; assert.ok(h.owner);
  return h.owner;
}
async function upload(h) {
  const samples=port(),rpcId=++h.nextRpc;
  await h.send({kind:"play-samples-upload",playId:7,rpcId,port:samples,generation:7,channels:2,
    pcmLimits:{maxAssetBytes:64*MiB,maxTotalBytes:256*MiB,maxSamples:7940},timeoutMs:50});
  for(let count=0;!reply(h,rpcId)&&count<12;count++) await samples.ack();
  assert.equal(reply(h,rpcId)?.error,undefined); assert.ok(reply(h,rpcId));
  h.samples=samples;return samples;
}
async function attach(h,{activate=true,startFrame=START,started=true}={}) {
  await upload(h);const audio=port(),rpcId=++h.nextRpc;
  await h.send({kind:"play-audio",playId:7,rpcId,port:audio,generation:7,queueCapacity:4096,timeoutMs:50});
  h.audio=audio;
  for(let count=0;!reply(h,rpcId)&&count<8;count++) await ackAudio(h);
  assert.equal(reply(h,rpcId)?.error,undefined);
  if(activate) {
    await h.send({kind:"play-activate",playId:7,rpcId:++h.nextRpc,hostNs:ORIGIN,startFrame});
    if(started)await startOwner(h);
  }
  return audio;
}
async function ackAudio(h,fields={}) {
  const message=h.audio.pending;assert.ok(message,`a real serial command is pending; Worker errors: ${h.of("play-error").map(row=>row.message).join("; ")}`);
  await h.audio.ack(message,message.kind==="retained-poll"
    ? {report:renderReport(),retained:snapshot(),...fields}:fields);
}
async function deliverPoll(h,event,fields={}) {
  if(!h.audio.pending) {
    h.nextRender=Math.max(h.nextRender??0,...h.of("play-render-done").map(row=>row.renderId))+1;
    await h.send({kind:"play-render",playId:7,renderId:h.nextRender,presentedNs:null,presentedHostNs:null});
  }
  for(let count=0;h.audio.pending?.kind!=="retained-poll"&&count<8;count++) await ackAudio(h);
  assert.equal(h.audio.pending?.kind,"retained-poll");
  await ackAudio(h,{report:renderReport(),retained:snapshot(event),...fields});
}
async function reachControl(h,kind) {
  await flushJobs();
  for(let count=0;h.audio.pending?.kind!==kind&&count<8;count++) await ackAudio(h);
  assert.equal(h.audio.pending?.kind,kind);
  return h.audio.pending;
}
async function serviceHeld(h) {
  const now=h.windowNowNs();
  await h.send({kind:"play-step",playId:7,tickId:h.nextTick=(h.nextTick??0)+1,
    events:[],watermark:now,nowNs:now,audioNs:0n});
}
async function startOwner(h) {
  const previous=h.owner.allowCommit;
  h.owner.allowCommit=true;
  await deliverPoll(h,envelope({tag:1n,serial:1n,id:0n,boundary:0n}));
  await reachControl(h,"retained-event-ack");
  assert.equal(h.audio.pending.serial,1n);
  await ackAudio(h);
  h.owner.allowCommit=previous;
  assert.equal(h.owner.retainedReady,true,"only an actual Started service makes the controlled binding ready");
  assert.equal(h.of("play-retired-recordings").length,0,"Started never retires a capture cohort");
}
async function stop(h) { await h.send({kind:"play-stop",playId:7}); }

test("retained opt-in precedes first advance and actual limits precede capture/setup while original samples and cues cross unchanged",async()=>{
  const h=await catalog();const owner=await begin(h,{startNs:1000000000n,recordReplay:true,
    captureBudget:{remainingRecords:128,remainingBytes:256*MiB}});
  assert.equal(h.preparationJobs[0].retained,true);
  assert.deepEqual(owner.retainedLimits,[17,71,64,64,4096]);
  assert.ok(owner.calls.findIndex(row=>row[0]==="retained-limits")<owner.calls.findIndex(row=>row[0]==="capture"));
  const sample=await upload(h);
  assert.deepEqual(sample.posts.map(row=>row.kind),["retained-setup","sample","sample","retained-cue","retained-cue","end-samples"]);
  assert.equal(at(sample.posts[0].words,1),PROGRAM);
  assert.deepEqual(sample.posts.filter(row=>row.kind==="sample").map(row=>row.id),[19n,18446744073709551615n]);
  assert.deepEqual(sample.posts.filter(row=>row.kind==="sample").map(row=>[row.rate,Array.from(row.pcm)]),[
    [44100,[0.25,-0.25,0.5,-0.5]],[96000,[1,-1]]]);
  assert.deepEqual(sample.posts.filter(row=>row.kind==="retained-cue").map(row=>[row.index,Array.from(row.words)]),[
    [0,[7,0,19,0,0,0,0x3f000000,0,0]],[1,[8,0,0xffffffff,0xffffffff,1500000000,0,0x3f000000,0,0]]]);
  assert.equal(reply(h,1).result.retained,true);assert.equal(reply(h,1).result.programId,PROGRAM);
  assert.equal(Object.hasOwn(reply(h,1).result,"cues"),false);
  await stop(h);
});

test("missing retained opt-in and recorded replay preserve legacy preparation and sample wire",async()=>{
  const h=await catalog();await h.send(start({retained:false}));
  assert.equal(h.preparationJobs[0].retained,false);assert.equal(h.games[0].retainedLimits,undefined);
  const sample=await upload(h);assert.deepEqual(sample.posts.map(row=>row.kind),["sample","sample","end-samples"]);
  await stop(h);
  const replay=await catalog();await replay.send(start({mode:"replay",retained:false,
    replayFile:new File([Uint8Array.from([66,75,82,1])],"replay.bkr")}));
  assert.equal(replay.preparationJobs[0].retained,false);assert.equal(replay.replays.length,1);await stop(replay);
});

test("Worker prepares canonical scalar rate, settles admission pending and publishes only the genuine owner commit",async()=>{
  const h=await catalog();const owner=await begin(h);await attach(h);
  const rpcId=await request(h,"rate",{fraction:"04/02"});
  const sent=await reachControl(h,"retained-rate");
  assert.deepEqual(owner.calls.find(row=>row[0]==="prepare-rate"),["prepare-rate",BigInt(rpcId),2n,1n]);
  assert.equal(sent.attempt,1n);assert.equal(sent.revision,0n);assert.equal(sent.programId,PROGRAM);
  assert.equal(sent.preparation.length,20);assert.equal(reply(h,rpcId),undefined);
  await ackAudio(h);assert.equal(reply(h,rpcId)?.result.kind,"retained-pending");
  assert.equal(h.of("play-retained-result").length,0);
  const event=envelope({id:BigInt(rpcId)});await deliverPoll(h,event);
  assert.equal(h.of("play-retained-result").length,0,"latched Worklet event is not a committed result");
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-event-ack"&&row.serial===9n),false);
  owner.allowCommit=true;await serviceHeld(h);
  const result=h.of("play-retained-result").at(-1);assert.ok(result);
  assert.deepEqual(result.event,event);assert.equal(result.outcome,"applied");assert.equal(result.id,BigInt(rpcId));
  assert.equal(at(result.capability,5),1n);assert.equal(at(result.capability,6),2n);
  await reachControl(h,"retained-event-ack");
  assert.equal(owner.calls.some(row=>row[0]==="retained-local-ack"&&row[1]===9n),false,"Worklet ACK precedes Rust latch ACK");
  owner.retainedPin=17n;
  await ackAudio(h);assert.deepEqual(owner.calls.find(row=>row[0]==="retained-local-ack"&&row[1]===9n),["retained-local-ack",9n]);
  const retire=await reachControl(h,"retained-history-retire");assert.equal(retire.pin,17n,"only Rust input pin permits retirement");
  await ackAudio(h);await stop(h);
});

test("paused rate admission leaves room for resume and retains event chronology",async()=>{
  const h=await catalog();const owner=await begin(h);await attach(h);owner.allowCommit=true;
  const pauseId=await request(h,"pause",{paused:true});await reachControl(h,"retained-pause");await ackAudio(h);
  await deliverPoll(h,envelope({tag:4n,id:BigInt(pauseId),serial:10n,paused:true}));
  await reachControl(h,"retained-event-ack");await ackAudio(h);
  const rateId=await request(h,"rate",{fraction:"3/2"});await reachControl(h,"retained-rate");await ackAudio(h);
  assert.equal(reply(h,rateId)?.result.kind,"retained-pending");
  const resumeId=await request(h,"pause",{paused:false});await reachControl(h,"retained-pause");await ackAudio(h);
  assert.equal(reply(h,resumeId)?.result.kind,"retained-pending");
  await deliverPoll(h,envelope({tag:4n,id:BigInt(resumeId),serial:11n,paused:false}));
  await reachControl(h,"retained-event-ack");await ackAudio(h);
  await deliverPoll(h,envelope({id:BigInt(rateId),serial:12n,numerator:3n,denominator:2n}));
  assert.deepEqual(h.of("play-retained-result").map(row=>row.outcome),["paused","resumed","applied"]);
  await stop(h);
});

test("normal rate refusal clears its reservation and preserves the same owner opponent and next request",async()=>{
  const h=await catalog();const owner=await begin(h);await attach(h);
  const id=await request(h,"rate",{fraction:"2/1"});await reachControl(h,"retained-rate");
  await ackAudio(h,{status:1,error:"denominator capacity",terminal:false});
  assert.match(reply(h,id)?.error??"",/denominator capacity/);assert.equal(h.of("play-retained-result").length,0);
  assert.ok(owner.calls.some(row=>row[0]==="admission"&&row[2]===BigInt(id)&&row[3]===false));
  const retry=await request(h,"rate",{fraction:"1/2"});await reachControl(h,"retained-rate");await ackAudio(h);
  assert.equal(reply(h,retry)?.result.kind,"retained-pending");assert.equal(owner.frees,0);await stop(h);
});

test("stale play, duplicate busy draft, seek and stop cannot enqueue another transaction or revive a stopped result",async()=>{
  const h=await catalog();const owner=await begin(h);await attach(h);
  await h.send({kind:"play-retained",version:1,playId:6,rpcId:90,action:"rate",fraction:"2/1"});
  assert.equal(owner.calls.some(row=>row[0]==="prepare-rate"),false);
  const id=await request(h,"rate",{fraction:"2/1"});await reachControl(h,"retained-rate");await ackAudio(h);
  const duplicate=await request(h,"rate",{fraction:"3/2"});assert.match(reply(h,duplicate)?.error??"",/pending|busy|outstanding/i);
  assert.equal(owner.calls.filter(row=>row[0]==="prepare-rate").length,1);
  await h.send({kind:"seek",id:99,songNs:0n});assert.equal(owner.frees,0);
  const previous=h.of("play-retained-result").length;await stop(h);
  await h.send({kind:"play-retained",version:1,playId:7,rpcId:91,action:"rate",fraction:"1/2"});
  assert.equal(h.of("play-retained-result").length,previous);assert.equal(owner.frees,1);
});

test("cancelled retained preparation cannot acquire an owner or replace a later generation",async()=>{
  const gate=deferred();const h=await catalog({},selectedFile("song/chart.bms",()=>gate.promise));
  await h.send(start());await stop(h);gate.resolve(new TextEncoder().encode("#BPM 120").buffer);await flushJobs();
  assert.equal(h.games.length+h.locals.length,0);assert.equal(reply(h,1)?.result,undefined);
  await h.send(start({playId:8,rpcId:2}));assert.equal(reply(h,2)?.result.retained,true);
  assert.equal(h.games.length,1);const owner=h.games[0];
  assert.ok(h.preparationJobs.every(job=>job.retained),"every actual live job opts in before advance");
  await h.send({kind:"play-stop",playId:7});assert.equal(owner.stops,0,"cancelled old generation cannot dispose retry");
  await h.send({kind:"play-stop",playId:8});assert.equal(owner.stops,1);assert.equal(owner.frees,1);
});

test("retained report/history/event use original output timestamp pair and service before ack; nonfresh rate retains visual registration",async()=>{
  const h=await catalog();const owner=await begin(h);await attach(h);
  const initial=h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length;
  await h.send({kind:"play-render",playId:7,renderId:2,
    timestamp:{contextTime:9187500001.25,performanceTime:9007199255},observedNowMs:9007199255,
    controlClock:{beforeMs:9007199255,afterMs:9007199255,contextTime:9999,sampleRate:48000}});
  const id=await request(h,"rate",{fraction:"2/1"});await reachControl(h,"retained-rate");await ackAudio(h);
  const event=envelope({id:BigInt(id)});owner.allowCommit=true;await deliverPoll(h,event);
  const held=owner.calls.findIndex(row=>row[0]==="retained-event"&&at(row[1],3)===9n);
  const history=owner.calls.findLastIndex((row,index)=>row[0]==="retained-history"&&index<held);
  assert.ok(history>=0&&held>history);assert.ok(owner.calls.slice(held+1).some(row=>row[0]==="retained-service"));
  assert.deepEqual(owner.calls.find(row=>row[0]==="retained-presentation"),["retained-presentation",1250000000n,9007199255000000n]);
  assert.equal(h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length,initial);
  await stop(h);
});

test("retained raw observation refuses an absolute context frame outside signed nanoseconds without fabricating presentation",async()=>{
  const h=await catalog();const owner=await begin(h);await attach(h,{startFrame:9007199254741999n,started:false});
  await h.send({kind:"play-render",playId:7,renderId:1,
    timestamp:{contextTime:187649984476,performanceTime:9007199255},observedNowMs:9007199255});
  const failure=h.of("play-error").at(-1);assert.ok(failure);assert.match(failure.message,/Audio frame exceeds signed nanoseconds/);
  assert.equal(owner.calls.some(row=>row[0]==="retained-presentation"),false);
  assert.equal(owner.frees,1);
});

test("cold retained None stays unavailable through preparation and uncommitted Started, then publishes only the serviced capability",async()=>{
  const h=await catalog();const owner=await begin(h,{recordReplay:true,
    captureBudget:{remainingRecords:1,remainingBytes:64*MiB}});
  assert.equal(reply(h,1).result.capability,null);
  assert.equal(owner.retained_capability_words().length,0);
  await attach(h,{started:false});
  const registrations=h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length;
  const score=owner.score;
  const early=await request(h,"rate",{fraction:"2/1"});
  assert.match(reply(h,early)?.error??"",/capability|available|started/i);
  assert.equal(owner.calls.some(row=>row[0]==="prepare-rate"),false);
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-rate"),false);
  const started=envelope({tag:1n,serial:1n,id:0n,boundary:0n});
  await deliverPoll(h,started);
  assert.equal(h.of("play-render-done").at(-1).retainedCapability,null);
  assert.equal(owner.retained_capability_words().length,0,"receiving Started is not business commit");
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-event-ack"&&row.serial===1n),false);
  owner.allowCommit=true;await serviceHeld(h);
  await reachControl(h,"retained-event-ack");assert.equal(h.audio.pending.serial,1n);await ackAudio(h);
  await deliverPoll(h,null);
  assert.deepEqual(h.of("play-render-done").at(-1).retainedCapability,capability());
  assert.equal(owner.retainedGeneration,1n);
  assert.equal(h.of("play-retired-recordings").length,0,"Started does not spend final-current capture room");
  assert.equal(h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length,registrations);
  assert.equal(owner.score,score);
  const id=await request(h,"rate",{fraction:"2/1"});await reachControl(h,"retained-rate");await ackAudio(h);
  assert.equal(reply(h,id)?.result.capability.length,16,"actual admission still requires a live capability");
  await stop(h);assert.equal(owner.replayTakes,1);
});

test("cold capability accepts only the actual empty view or strict sixteen words, never a malformed extent as None",async()=>{
  const h=await catalog({capabilityWords:new Uint32Array([1,0])});await h.send(start());
  assert.equal(reply(h,1)?.result,undefined);
  assert.match(h.of("play-error").at(-1)?.message??"",/capability|words/i);
  assert.equal(h.games[0].frees,1);assert.equal(h.audio,undefined);
});

test("Ended revokes capability without retiring current capture or resetting visuals, and late Rate Refused remains correlated with null permission",async()=>{
  const h=await catalog();const owner=await begin(h,{recordReplay:true,
    captureBudget:{remainingRecords:128,remainingBytes:256*MiB}});await attach(h);owner.allowCommit=true;
  const registrations=h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length;
  const score=owner.score,snapshotScore={...score};
  const id=await request(h,"rate",{fraction:"2/1"});await reachControl(h,"retained-rate");await ackAudio(h);
  assert.equal(reply(h,id)?.result.kind,"retained-pending");
  await deliverPoll(h,envelope({tag:1n,boundary:3n,generation:2n,serial:40n,id:0n}));
  await reachControl(h,"retained-event-ack");assert.equal(h.audio.pending.serial,40n);await ackAudio(h);
  assert.equal(h.of("play-render-done").at(-1).retainedCapability,null);
  assert.equal(owner.retained_capability_words().length,0);
  assert.equal(owner.retainedGeneration,2n,"the serviced original Ended event owns generation");
  assert.equal(h.of("play-retired-recordings").length,0);
  assert.equal(owner.calls.some(row=>row[0]==="take-retired"),false);
  assert.equal(h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length,registrations);
  assert.equal(owner.score,score);assert.deepEqual(owner.score,snapshotScore);
  const refused=envelope({id:BigInt(id),generation:2n,serial:41n,outcome:2n});
  await deliverPoll(h,refused);
  const response=h.of("play-retained-result").at(-1);assert.ok(response);
  assert.equal(response.id,BigInt(id));assert.equal(response.action,"rate");assert.equal(response.outcome,"refused");
  assert.equal(response.capability,null);assert.deepEqual(response.event,refused);
  assert.equal(at(response.event,10),0n,"refusal preserves committed revision");
  await reachControl(h,"retained-event-ack");await ackAudio(h);
  assert.equal(h.of("play-retired-recordings").length,0);
  assert.equal(h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length,registrations);
  await stop(h);const terminal=h.of("play-stopped").at(-1);assert.ok(terminal?.replay instanceof File);
  assert.equal(owner.replayTakes,1);assert.equal(terminal.replayComplete,false);
  assert.equal(terminal.hits,snapshotScore.hits);assert.equal(terminal.combo,snapshotScore.combo);
});

for(const kind of ["requested","looped","restart"]) test(`only actual ${kind} fresh service re-registers visual content`,async()=>{
  const h=await catalog();const owner=await begin(h);await attach(h);owner.allowCommit=true;
  const initial=h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length;
  let id=0;
  if(kind!=="looped") {
    id=await request(h,kind==="restart"?"restart-original":"region",
      kind==="restart"?{}:{startSeconds:"1",endSeconds:"2",repeat:true});
    await reachControl(h,kind==="restart"?"retained-restart-original":"retained-region");await ackAudio(h);
  }
  await deliverPoll(h,envelope({tag:kind==="restart"?3n:1n,boundary:kind==="looped"?2n:1n,
    id:BigInt(id),generation:2n,serial:50n,numerator:1n}));
  await reachControl(h,"retained-event-ack");await ackAudio(h);
  assert.ok(h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length>initial);
  assert.equal(h.of("play-render-done").at(-1).retainedCapability.length,16);
  assert.equal(owner.retainedGeneration,2n);await stop(h);
});

for(const count of [2,4]) for(const capture of [false,true]) test(`fresh boundary drains entire ${count}-member roster capture=${capture} before acceptance and Worklet ACK`,async()=>{
  const players=Array.from({length:count},(_,index)=>99-index*7);
  const h=await catalog();const owner=await begin(h,{inputMode:"physical-contact",
    localPlanWords:new Uint32Array(players.flatMap((player,index)=>[player,1,index+1,0])),recordReplay:capture,
    ...(count===4?{hidSetup:hidSetup([3n,4n])}:{}),
    ...(capture?{captureBudget:{remainingRecords:128,remainingBytes:256*MiB}}:{})});
  await attach(h);owner.allowCommit=true;owner.retiredBytes=new Set(players);
  assert.deepEqual([...owner.memberCaptures],capture?players:[]);
  if(capture)assert.deepEqual(owner.calls.filter(row=>row[0]==="member-capture"),
    players.map(player=>["member-capture",player,Math.floor(64*MiB/count),Math.floor(1000000/count)]));
  const registrations=h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length;
  const id=await request(h,"region",{startSeconds:"1.000000001",endSeconds:"2",repeat:true});
  await reachControl(h,"retained-region");await ackAudio(h);
  await deliverPoll(h,envelope({tag:1n,generation:2n,id:BigInt(id),serial:21n}));
  if(capture) {
    const retired=h.of("play-retired-recordings").at(-1);assert.ok(retired);
    assert.deepEqual(retired.players,players);assert.deepEqual(retired.replays.map(row=>row.player),players);
    assert.ok(retired.replays.every(row=>row.replay instanceof File&&row.replayComplete===false));
    assert.equal(retired.bytes,players.length*5);assert.equal(retired.boundarySerial,21n);
    assert.equal(h.audio.posts.some(row=>row.kind==="retained-event-ack"&&row.serial===21n),false);
    await h.send({kind:"play-retired-recordings-ack",version:1,playId:7,programId:PROGRAM,boundarySerial:20n,
      accepted:true,error:null,remainingRecords:126,remainingBytes:192*MiB});
    assert.equal(owner.calls.some(row=>row[0]==="retained-local-ack"&&row[1]===21n),false);
    await h.send({kind:"play-retired-recordings-ack",version:1,playId:7,programId:PROGRAM,boundarySerial:21n,
      accepted:true,error:null,remainingRecords:128-count,remainingBytes:256*MiB-count*5});
  } else {
    assert.equal(h.of("play-retired-recordings").length,0);
    assert.equal(owner.calls.some(row=>row[0]==="take-retired"),false);
  }
  await reachControl(h,"retained-event-ack");assert.equal(owner.calls.some(row=>row[0]==="retained-local-ack"&&row[1]===21n),false);
  await ackAudio(h);assert.ok(owner.calls.some(row=>row[0]==="retained-local-ack"&&row[1]===21n));
  assert.ok(h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length>registrations,"fresh generation re-registers content before its next frame");
  await stop(h);
});

test("capture budget preflight refuses fresh explicit control before Rust preparation or Worklet enqueue",async()=>{
  const h=await catalog();const owner=await begin(h,{recordReplay:true,captureBudget:{remainingRecords:1,remainingBytes:64*MiB}});await attach(h);
  const id=await request(h,"restart-original");assert.match(reply(h,id)?.error??"",/capacity|budget|capture|record/i);
  assert.equal(owner.calls.some(row=>row[0]==="prepare-restart"),false);
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-restart-original"),false);
  assert.equal(owner.frees,0);await stop(h);
});

for(const competition of ["saved","network"]) test(`actual admitted fixed ${competition} competition refuses rate before enqueue and preserves opponents`,async()=>{
  const h=await catalog({allowNetworkClock:competition==="network"});
  const fields=competition==="saved"?{opponents:[{file:new File([Uint8Array.from([66,75,82,17])],"opponent.bkr"),own:true,label:"past",sourceKey:"file:original"}]}
    :{multiplayer:{url:"https://example.test:4433/competition",host:true,windowOriginNs:10000000000n}};
  const owner=await begin(h,fields);await attach(h,{activate:competition!=="network"});
  if(competition==="network") {
    const rpcId=++h.nextRpc;await h.send({kind:"play-network-ready",playId:7,rpcId});
    const endpoint=h.networks[0];assert.ok(endpoint,"actual network owner admitted after original samples");
    endpoint.emit({kind:"connected"});endpoint.emit({kind:"ready"});
    endpoint.emit({kind:"start",targetNs:500000000n,songTargetNs:600000000n,uncertaintyNs:4n});await flushJobs();
    const target=reply(h,rpcId)?.result?.targetHostNs;assert.equal(typeof target,"bigint");
    await h.send({kind:"play-activate",playId:7,rpcId:++h.nextRpc,hostNs:target+100n,startFrame:START,targetHostNs:target});
    await startOwner(h);
  }
  const saved=owner.saved.map(row=>({...row})),network=h.networks[0];
  const id=await request(h,"rate",{fraction:"2/1"});
  assert.match(reply(h,id)?.error??"",/fixed|competition|opponent|network/i);
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-rate"),false);
  assert.deepEqual(owner.saved,saved);assert.equal(owner.hudDisables,0);assert.equal(owner.peerDisables,0);
  if(competition==="network")assert.equal(h.networks[0],network);
  assert.equal(owner.frees,0);await stop(h);
});

test("autonomous fresh boundary without old/current capture room fails before commit and preserves both exports",async()=>{
  const h=await catalog();const owner=await begin(h,{recordReplay:true,
    captureBudget:{remainingRecords:128,remainingBytes:256*MiB}});await attach(h);
  owner.allowCommit=true;owner.retiredBytes=new Set([1]);
  const id=await request(h,"region",{startSeconds:"1",endSeconds:"2",repeat:true});await reachControl(h,"retained-region");await ackAudio(h);
  await deliverPoll(h,envelope({tag:1n,id:BigInt(id),generation:2n,serial:30n}));
  const accepted=h.of("play-retired-recordings").at(-1);assert.ok(accepted?.replays[0].replay instanceof File);
  await h.send({kind:"play-retired-recordings-ack",version:1,playId:7,programId:PROGRAM,boundarySerial:30n,
    accepted:true,error:null,remainingRecords:1,remainingBytes:64*MiB});
  await reachControl(h,"retained-event-ack");await ackAudio(h);
  await deliverPoll(h,envelope({tag:1n,id:0n,generation:3n,serial:31n,boundary:2n}));
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-event-ack"&&row.serial===31n),false);
  assert.equal(owner.retainedGeneration,2n,"capacity failure cannot commit the next fresh business state");
  const failure=h.of("play-error").at(-1);assert.ok(failure);assert.match(failure.message,/capacity|capture|record|budget/i);
  assert.equal(accepted.replays[0].replay.size,5,"previous accepted File remains independently owned");
  assert.equal(owner.replayTakes,1,"final current capture survives autonomous loop capacity failure");
});

test("retired File post failure retains owned prefixes and current capture instead of acknowledging a lost cohort",async()=>{
  let reject=true;
  const h=await catalog({postFailure:value=>value.kind==="play-retired-recordings"&&reject});
  const owner=await begin(h,{recordReplay:true,captureBudget:{remainingRecords:128,remainingBytes:256*MiB}});await attach(h);
  owner.allowCommit=true;owner.retiredBytes=new Set([1]);
  const id=await request(h,"region",{startSeconds:"1",endSeconds:"2",repeat:true});await reachControl(h,"retained-region");await ackAudio(h);
  await deliverPoll(h,envelope({tag:1n,generation:2n,id:BigInt(id),serial:32n}));
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-event-ack"&&row.serial===32n),false);
  assert.equal(owner.calls.some(row=>row[0]==="retained-local-ack"&&row[1]===32n),false);
  reject=false;await stop(h);
  const transfers=h.of("play-retired-recordings");
  const terminal=[...h.of("play-error"),...h.of("play-stopped")];
  function filesIn(value) {
    if(value instanceof File)return [value];
    if(!value||typeof value!=="object"||ArrayBuffer.isView(value))return [];
    return Object.values(value).flatMap(filesIn);
  }
  const oldOwned=filesIn([...transfers,...terminal]).some(file=>file.size===5);
  assert.ok(oldOwned||terminal.every(row=>row.released!==true),"never claim release while old File custody is missing");
  assert.equal(owner.replayTakes,1,"failed old transfer cannot consume final current capture");
});

test("one retired member encoding failure cannot erase another member or permit whole-cohort ACK",async()=>{
  const h=await catalog({retiredErrorPlayer:7});const owner=await begin(h,{inputMode:"physical-contact",recordReplay:true,
    localPlanWords:new Uint32Array([99,1,1,0,7,1,2,0]),captureBudget:{remainingRecords:128,remainingBytes:256*MiB}});await attach(h);
  owner.allowCommit=true;owner.retiredBytes=new Set([99,7]);
  const id=await request(h,"region",{startSeconds:"1",endSeconds:"2",repeat:true});await reachControl(h,"retained-region");await ackAudio(h);
  await deliverPoll(h,envelope({tag:1n,generation:2n,id:BigInt(id),serial:33n}));
  assert.ok(owner.calls.some(row=>row[0]==="take-retired"&&row[1]===99));
  assert.ok(owner.calls.some(row=>row[0]==="take-retired"&&row[1]===7));
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-event-ack"&&row.serial===33n),false);
  const terminal=[...h.of("play-error"),...h.of("play-stopped")];
  assert.ok(terminal.every(row=>row.released!==true),"unexported member remains owned after encoding failure");
  if(!owner.stops)await stop(h);
  assert.deepEqual([...owner.memberReplayTakes],[[99,1],[7,1]],"other/current member captures remain independently exportable");
});

// Reuse the existing Window VM fixture, including its actual retained DOM.
const hostFixtureUrl=new URL("./play-host.test.mjs",import.meta.url);
const hostOriginal=await readFile(hostFixtureUrl,"utf8");
function between(source,start,end) {
  assert.equal(source.split(start).length,2);assert.equal(source.split(end).length,2);
  return source.slice(source.indexOf(start),source.indexOf(end,source.indexOf(start)));
}
let hostFixture=between(hostOriginal,'// Deferred: node --experimental-vm-modules','function speedPrepared(')
  +between(hostOriginal,'function savedRecord(fields = {}) {','function portableSettings() {')
  +between(hostOriginal,'async function harness(faults = {}) {','test("Window record RPC: immutable capture')
  +between(hostOriginal,'async function localCount(h, count) {','async function pagedTouchSession() {');
hostFixture=hostFixture.replaceAll("import.meta.url",JSON.stringify(hostFixtureUrl.href));
hostFixture=hostFixture.replace(/from "(\.\/[^\"]+)"/g,(_,path)=>`from ${JSON.stringify(new URL(path,hostFixtureUrl).href)}`);
hostFixture+='\nexport { harness, flush, finalScore, selectedRecording, chooseRecording, localCount, localAssign, localFinal };\n';
const {harness,flush,finalScore:baseFinalScore,selectedRecording,chooseRecording,localCount,localAssign,localFinal}=
  await import(`data:text/javascript;base64,${Buffer.from(hostFixture).toString("base64")}`);
function finalScore(playId,fields={}) {
  return baseFinalScore(playId,{replay:null,replayComplete:false,replayError:null,...fields});
}
async function windowStart(h,{record=false,metadata={},ready=true}={}) {
  await h.preview();h.get("runtime-retained").checked=true;h.get("record").checked=record;h.get("live-end").value="2";
  const pending=await h.begin(),worker=h.workers[0];
  await h.reply(pending,{kind:"prepared",mode:"live",title:"Retained",artist:"Fixture",notes:1,samples:0,
    lanes:[0x11],opponentCount:0,startNs:0n,endNs:2000000000n,retained:true,programId:PROGRAM,
    capability:null,liveSpeed:"1/1",liveSpeedNumerator:1n,liveSpeedDenominator:1n,...metadata});
  const upload=worker.last("play-samples-upload");assert.ok(upload);await h.admitSamples(upload,0);
  await h.reply(upload,{kind:"samples-uploaded",count:0,bytes:0});
  await h.reply(worker.last("play-audio"),null);await h.reply(worker.last("play-activate"),null);
  await h.geometry({playId:pending.playId});
  if(ready)await windowCapability(h,pending,capability());
  return pending;
}
async function windowCapability(h,session,value) {
  const worker=h.workers[0];
  await h.advance(8);
  for(let count=0;count<4;count++) {
    const step=worker.last("play-step");
    if(!step||step.tickId===(h.lastSettledTick??0))break;
    await h.receive({kind:"play-step-done",playId:session.playId,tickId:step.tickId,
      commandsPending:false,pendingInputs:0,songNs:0n,hits:0n,misses:0n,combo:0n,preOriginInputs:0});
    h.lastSettledTick=step.tickId;
  }
  const step=worker.last("play-step");
  assert.equal(step?.tickId??0,h.lastSettledTick??0,"the actual acquired prefix is acknowledged before its render reply");
  const observation=worker.last("play-render");assert.ok(observation,"existing render request precedes capability publication");
  await h.receive({kind:"play-render-done",playId:session.playId,renderId:observation.renderId,
    completed:false,commandsPending:false,observedTick:step?.tickId??0,pendingInputs:0,retainedCapability:value});
}
function resultFor(start,request,{outcome="applied",serial=9n,tag=2n,generation=1n,numerator=2n,denominator=1n}={}) {
  return {kind:"play-retained-result",version:1,playId:start.playId,id:BigInt(request.rpcId),action:request.action,
    event:envelope({id:BigInt(request.rpcId),serial,tag,generation,numerator,denominator,paused:outcome==="paused"}),
    capability:capability({generation,revision:tag===2n?1n:0n,numerator,denominator}),outcome,reason:null};
}
async function pendingReply(h,request) {
  await h.reply(request,{kind:"retained-pending",version:1,id:BigInt(request.rpcId),action:request.action,capability:capability()});
}
function retired(start,serial,players=[1],fileFor=player=>new File([Uint8Array.from([66,75,82,player])],`prefix-${serial}-${player}.bkr`)) {
  const replays=players.map(player=>({player,replay:fileFor(player),replayComplete:false,replayError:null}));
  return {kind:"play-retired-recordings",version:1,playId:start.playId,programId:PROGRAM,boundarySerial:serial,
    players,replays,bytes:replays.reduce((sum,row)=>sum+(row.replay?.size??0),0)};
}

test("Window opt-in passes negotiated limits and bounded capture budget; retained finish is explicit",async()=>{
  const h=await harness();try {
    const session=await windowStart(h,{record:true});
    assert.equal(session.retained,true);assert.deepEqual(session.captureBudget,{remainingRecords:128,remainingBytes:256*MiB});
    assert.equal(session.audioLimits.maxVoices,h.audio.configuration.audioLimits.maxVoices);
    assert.deepEqual(structuredClone(h.audio.finishArgs),[[undefined,{mode:"retained"}]]);
    assert.equal(h.audio.polls,0);h.click("stop");await flush();await h.receive(finalScore(session.playId));
  }finally{await h.close();}
});

test("Window cold null permission enables only from actual render capability, preserves drafts and held input through Ended and settles late refusal",async()=>{
  const h=await harness();try {
    const session=await windowStart(h,{record:true,ready:false}),worker=h.workers[0];
    assert.equal(h.get("runtime-rate-apply").disabled,true);assert.equal(h.get("runtime-pause").disabled,true);
    h.get("runtime-rate").value="003/002";h.get("runtime-start").value="0.123456789";
    h.get("runtime-end").value="1.999999999";
    await windowCapability(h,session,null);
    assert.equal(h.get("runtime-rate-apply").disabled,true);assert.equal(worker.messages("play-retained").length,0);
    await windowCapability(h,session,capability());
    assert.equal(h.get("runtime-rate-apply").disabled,false);assert.equal(h.get("runtime-pause").disabled,false);
    assert.equal(h.get("runtime-rate").value,"003/002");assert.equal(h.get("runtime-start").value,"0.123456789");
    h.setNow(1300);h.window.emit("keydown",{code:"KeyZ",repeat:false,timeStamp:1300});await flush();
    const down=worker.last("play-step").events.find(event=>event.key===2&&event.down===true);assert.ok(down);
    h.click("runtime-rate-apply");await flush();const rate=worker.last("play-retained");
    assert.equal(rate.fraction,"003/002");await pendingReply(h,rate);
    const title=h.get("title").textContent;
    await windowCapability(h,session,null);
    assert.equal(h.get("runtime-rate-apply").disabled,true);assert.equal(h.get("runtime-pause").disabled,true);
    assert.equal(h.get("runtime-rate").value,"003/002");assert.equal(h.get("runtime-end").value,"1.999999999");
    assert.equal(h.get("title").textContent,title);assert.equal(worker.messages("play-stop").length,0);
    const refused={kind:"play-retained-result",version:1,playId:session.playId,id:BigInt(rate.rpcId),action:"rate",
      event:envelope({id:BigInt(rate.rpcId),generation:2n,serial:41n,outcome:2n,numerator:3n,denominator:2n}),
      capability:null,outcome:"refused",reason:"Ended before pending rate could apply"};
    await h.receive(refused);
    assert.match(h.get("runtime-status").textContent,/refused|Ended/);
    assert.equal(h.get("runtime-rate-apply").disabled,true,"settled late refusal cannot revive ended permission");
    const status=h.get("runtime-status").textContent;await h.receive(refused);
    assert.equal(h.get("runtime-status").textContent,status,"late result is consumed once");
    h.window.emit("keyup",{code:"KeyZ",repeat:false,timeStamp:1308});await flush();
    const up=worker.last("play-step").events.find(event=>event.key===2&&event.down===false);assert.ok(up);
    assert.ok(up.sequence>down.sequence,"Ended capability publication does not reset held-key acquisition");
    h.click("stop");await flush();const current=new File([Uint8Array.of(66,75,82,37)],"ended-current.bkr");
    await h.receive(finalScore(session.playId,{replay:current}));
    const choices=h.get("captured-replay").children.filter(option=>option.value!=="");assert.equal(choices.length,1);
    h.get("captured-replay").value=choices[0].value;h.get("captured-replay").emit("change");
    h.click("export");await flush();assert.equal(h.urls.at(-1).blob,current);
  }finally{await h.close();}
});

test("Window rejects a null admission capability while preserving its raw draft and actual display",async()=>{
  const h=await harness();try {
    const session=await windowStart(h),worker=h.workers[0];
    h.get("runtime-rate").value="3/2";h.click("runtime-rate-apply");await flush();const rate=worker.last("play-retained");
    await h.reply(rate,{kind:"retained-pending",version:1,id:BigInt(rate.rpcId),action:"rate",capability:null});
    assert.match(h.get("runtime-status").textContent,/capability|available|admission/i);
    assert.equal(h.get("runtime-rate").value,"3/2");assert.equal(worker.messages("play-stop").length,0);
    h.click("stop");await flush();await h.receive(finalScore(session.playId));
  }finally{await h.close();}
});

test("Window finite retained end admits no immutable frame while legacy finite playback retains its exact fence",async()=>{
  const retained=await harness();try {
    const session=await windowStart(retained);assert.deepEqual(structuredClone(retained.audio.finishArgs),[[undefined,{mode:"retained"}]]);
    retained.click("stop");await flush();await retained.receive(finalScore(session.playId));
  }finally{await retained.close();}
  const legacy=await harness();try {
    await legacy.preview();legacy.get("runtime-retained").checked=false;legacy.get("live-end").value="2";
    const session=await legacy.begin();await legacy.reply(session,{kind:"prepared",mode:"live",title:"Fixed",artist:"Fixture",
      notes:1,samples:0,lanes:[0x11],opponentCount:0,startNs:0n,endNs:2000000000n,endFrame:100800n,
      liveSpeed:"1/1",liveSpeedNumerator:1n,liveSpeedDenominator:1n});
    const worker=legacy.workers[0],upload=worker.last("play-samples-upload");assert.ok(upload);
    await legacy.admitSamples(upload,0);await legacy.reply(upload,{kind:"samples-uploaded",count:0,bytes:0});
    await legacy.reply(worker.last("play-audio"),null);await legacy.reply(worker.last("play-activate"),null);
    assert.deepEqual(legacy.audio.finishArgs,[[100800n]]);
    legacy.click("stop");await flush();await legacy.receive(finalScore(session.playId));
  }finally{await legacy.close();}
});

for(const retained of [true,false]) test(`Window refuses ${retained?"retained immutable frame":"legacy missing finite frame"} before PCM upload`,async()=>{
  const h=await harness();try {
    await h.preview();h.get("runtime-retained").checked=retained;h.get("live-end").value="2";
    const session=await h.begin(),worker=h.workers[0];
    await h.reply(session,{kind:"prepared",mode:"live",title:"Invalid frame",artist:"Fixture",notes:1,samples:0,
      lanes:[0x11],opponentCount:0,startNs:0n,endNs:2000000000n,
      ...(retained?{retained:true,programId:PROGRAM,capability:capability(),endFrame:100800n}:{}),
      liveSpeed:"1/1",liveSpeedNumerator:1n,liveSpeedDenominator:1n});
    assert.equal(worker.messages("play-samples-upload").length,0);assert.equal(h.audio.arms.length,0);
    assert.ok(worker.last("play-stop"));await h.receive(finalScore(session.playId));
  }finally{await h.close();}
});

test("Window forwards original getOutputTimestamp pair while currentTime is only an arm clock",async()=>{
  const h=await harness({outputEvidence:{contextTime:1.25,performanceTime:1300.125},
    contextFrame:479952000n,controlClock:{beforeMs:1000,afterMs:1000,contextTime:9999,sampleRate:48000}});try {
    const session=await windowStart(h);h.setNow(1300.125);await h.advance(8);
    const observation=h.workers[0].last("play-render");assert.ok(observation);
    assert.deepEqual(observation.timestamp,{contextTime:1.25,performanceTime:1300.125});
    assert.equal(Object.hasOwn(observation,"presentedNs"),false);assert.equal(Object.hasOwn(observation,"presentedHostNs"),false);
    h.click("stop");await flush();await h.receive(finalScore(session.playId));
  }finally{await h.close();}
});

test("Window paused rate pending releases RPC slot for resume; applied display waits for matching owner result",async()=>{
  const h=await harness();try {
    const session=await windowStart(h),worker=h.workers[0];
    h.click("runtime-pause");await flush();const pause=worker.last("play-retained");
    assert.equal(pause.action,"pause");assert.equal(pause.paused,true);await pendingReply(h,pause);
    await h.receive(resultFor(session,pause,{tag:4n,outcome:"paused",numerator:1n}));
    h.get("runtime-rate").value="003/002";h.click("runtime-rate-apply");await flush();
    const rate=worker.last("play-retained");assert.equal(rate.fraction,"003/002");await pendingReply(h,rate);
    assert.equal(h.get("runtime-rate-apply").disabled,true);
    assert.doesNotMatch(h.get("runtime-status").textContent,/applied.*3\/2/i);
    h.click("runtime-pause");await flush();const resume=worker.last("play-retained");
    assert.equal(resume.action,"pause");assert.equal(resume.paused,false);assert.ok(resume.rpcId>rate.rpcId);
    await pendingReply(h,resume);await h.receive(resultFor(session,resume,{tag:4n,outcome:"resumed",numerator:1n,serial:10n}));
    await h.receive(resultFor(session,rate,{numerator:3n,denominator:2n,serial:11n}));
    assert.equal(h.get("runtime-rate-apply").disabled,false);
    assert.match(h.get("runtime-status").textContent,/3\/2/);
    const settled=h.get("runtime-status").textContent;await h.receive(resultFor({...session,playId:session.playId-1},rate));
    assert.equal(h.get("runtime-status").textContent,settled);
    h.click("stop");await flush();await h.receive(finalScore(session.playId));
  }finally{await h.close();}
});

test("Window retains raw region drafts and touch acquisition order through pending controls and F5",async()=>{
  const h=await harness({touchSupported:true});try {
    h.get("touch-input").checked=true;const session=await windowStart(h),worker=h.workers[0];
    h.get("runtime-start").value="1.000000001";h.get("runtime-end").value="";h.get("runtime-loop").checked=true;
    h.click("runtime-region");await flush();const region=worker.last("play-retained");
    assert.equal(region.startSeconds,"1.000000001");assert.equal(region.endSeconds,"");assert.equal(region.repeat,true);
    await pendingReply(h,region);
    h.setNow(1300);h.get("canvas").emit("pointerdown",{pointerType:"touch",pointerId:4,timeStamp:1300,offsetX:110,offsetY:200,pressure:0.375});await flush();
    const step=worker.last("play-step");assert.ok(step);assert.ok(step.events.length>0);
    h.window.emit("keydown",{code:"F5",repeat:false,timeStamp:1300});await flush();
    assert.equal(worker.messages("play-retained").length,1,"one business transaction despite restart gesture");
    h.click("stop");await flush();await h.receive(finalScore(session.playId));
  }finally{await h.close();}
});

test("Window accepts retired cohort atomically with null scores, unique attempt tuples and custody before ACK",async()=>{
  const h=await harness();try {
    const session=await windowStart(h,{record:true}),worker=h.workers[0];
    const first=retired(session,21n);await h.receive(first);
    const ack=worker.last("play-retired-recordings-ack");assert.equal(ack.accepted,true);assert.equal(ack.boundarySerial,21n);
    const choices=()=>h.get("captured-replay").children.filter(option=>option.value!=="");
    assert.equal(ack.remainingRecords,127);assert.equal(choices().length,1);
    const second=retired(session,22n);await h.receive(second);
    assert.equal(worker.last("play-retired-recordings-ack").accepted,true);
    assert.equal(choices().length,2,"same player in later attempt has a distinct tuple");
    await h.receive(first);assert.equal(worker.last("play-retired-recordings-ack").accepted,false);
    assert.equal(choices().length,2);
    h.click("stop");await flush();await h.receive(finalScore(session.playId,{replay:new File([new Uint8Array([66,75,82,77])],"current.bkr"),replayComplete:false,replayError:null}));
    assert.equal(choices().length,3,"terminal capture key cannot alias a retired boundary");
    h.get("captured-replay").value=choices()[0].value;h.get("captured-replay").emit("change");
    h.click("records-save");await flush();
    const save=worker.messages("records-request").find(row=>row.action==="save")?.value;assert.ok(save);
    assert.equal(save.metadata.hits,null);assert.equal(save.metadata.misses,null);assert.equal(save.metadata.combo,null);
    assert.equal(save.metadata.complete,false);assert.equal(save.replayFile,first.replays[0].replay);
    assert.equal(save.archiveBlob,null);assert.equal(save.archivePlayer,null);
  }finally{await h.close();}
});

test("Window malformed current File receipt keeps Worker custody and accepted retired rows until a complete receipt arrives",async()=>{
  const h=await harness({noWindowRecordBinary:true});try {
    const session=await windowStart(h,{record:true}),worker=h.workers[0];
    const old=retired(session,51n);await h.receive(old);
    assert.equal(worker.last("play-retired-recordings-ack").accepted,true);
    const current=new File([Uint8Array.of(66,75,82,23)],"current-original.bkr");
    current.arrayBuffer=()=>assert.fail("Window cannot acquire the current File body");
    const choices=()=>h.get("captured-replay").children.filter(option=>option.value!=="");
    h.click("stop");await flush();assert.ok(worker.last("play-stop"));
    await h.receive(finalScore(session.playId,{replay:current,replayError:undefined}));
    assert.match(h.get("status").textContent,/custody.*pending|Invalid replay export metadata/i);
    assert.equal(h.get("play").disabled,true,"malformed File metadata cannot release the live owner");
    assert.equal(worker.terminations,0);assert.equal(choices().length,1,"accepted old recording remains owned");
    h.click("play");await flush();assert.equal(worker.messages("play-start").length,1,"held custody blocks replacement");
    await h.receive(finalScore(session.playId,{replay:current}));
    assert.equal(choices().length,2,"valid current receipt joins the accepted old tuple");
    assert.equal(h.get("play").disabled,false);
    h.get("captured-replay").value=choices().at(-1).value;h.get("captured-replay").emit("change");
    h.click("export");await flush();assert.equal(h.urls.at(-1).blob,current,"the original immutable current File is preserved");
    h.get("captured-replay").value=choices()[0].value;h.get("captured-replay").emit("change");
    h.click("export");await flush();assert.equal(h.urls.at(-1).blob,old.replays[0].replay);
    assert.equal(choices().length,2,"download does not release either unsaved capture");
  }finally{await h.close();}
});

test("Window retired budget always reserves final current bytes and failed Save/download preserve custody",async()=>{
  const h=await harness({recordsSaveError:new Error("quota refused prefix")});try {
    const session=await windowStart(h,{record:true}),worker=h.workers[0];
    function largePrefix(player) {
      const file=new File([Uint8Array.from([66,75,82,player])],"bounded-prefix.bkr");
      // Size accounting seam; no Window body read or huge fixture allocation.
      Object.defineProperty(file,"size",{value:64*MiB});return file;
    }
    for(const serial of [41n,42n,43n]) {
      await h.receive(retired(session,serial,[1],largePrefix));
      assert.equal(worker.last("play-retired-recordings-ack").accepted,true);
    }
    const remaining=worker.last("play-retired-recordings-ack").remainingBytes;
    assert.equal(remaining,64*MiB);
    await h.receive(retired(session,44n,[1],largePrefix));
    const denied=worker.last("play-retired-recordings-ack");assert.equal(denied.accepted,false);
    assert.equal(denied.remainingBytes,remaining,"normal refusal preserves accepted custodian bytes");
    assert.equal(h.recordCalls.filter(row=>row.method==="save").length,0,"custody never automatically saves or downloads");
    assert.equal(h.downloads.length,0);
    h.click("stop");await flush();await h.receive(finalScore(session.playId,{replay:new File([Uint8Array.of(66,75,82,7)],"current.bkr"),replayComplete:false,replayError:null}));
    const choices=()=>h.get("captured-replay").children.filter(option=>option.value!=="");
    assert.equal(choices().length,4);h.get("captured-replay").value=choices()[0].value;h.get("captured-replay").emit("change");
    h.click("records-save");await flush();assert.match(h.get("status").textContent,/quota refused prefix/);
    assert.equal(choices().length,4);h.click("export");await flush();assert.equal(choices().length,4);
  }finally{await h.close();}
});

for(const fault of ["missing-member","duplicate-player","wrong-program","wrong-size"]) test(`Window refuses ${fault} retired cohort atomically and preserves current export room`,async()=>{
  const h=await harness();try {
    const session=await windowStart(h,{record:true}),worker=h.workers[0];
    const value=retired(session,21n);
    if(fault==="missing-member") value.replays=[];
    if(fault==="duplicate-player") {value.players=[1,1];value.replays.push({...value.replays[0]});value.bytes*=2;}
    if(fault==="wrong-program") value.programId=42n;
    if(fault==="wrong-size") value.bytes++;
    await h.receive(value);assert.equal(worker.last("play-retired-recordings-ack").accepted,false);
    assert.equal(h.get("captured-replay").children.filter(option=>option.value!=="").length,0);
    h.click("stop");await flush();await h.receive(finalScore(session.playId));
  }finally{await h.close();}
});
