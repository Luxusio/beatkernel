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
  numerator = 2n, denominator = 1n, outcome = 0n, boundary = 1n, paused = false,
  physicalFrame = 4801n, playbackFrame = 4801n } = {}) {
  const values = Array(40).fill(0n);
  Object.assign(values, { 0: 1n, 1: tag, 2: PROGRAM, 3: serial, 4: id, 5: generation,
    7: physicalFrame, 8: playbackFrame, 35: CONTEXT });
  if (tag === 4n) values[36] = BigInt(paused);
  else {
    Object.assign(values, { 26: physicalFrame, 28: 0n, 29: physicalFrame, 30: 48000n,
      31: physicalFrame, 32: 48000n, 33: 1n });
    if (tag === 1n || tag === 3n) Object.assign(values, { 22: 1000000000n,
      23: 1000000000n, 25: boundary });
    if (tag === 1n && boundary === 0n) Object.assign(values, {
      7: 0n, 8: 0n, 22: 0n, 23: 0n, 26: 0n, 29: 0n, 30: 1n, 31: 0n });
    if (tag === 2n || tag === 3n) Object.assign(values, { 9: revision - 1n,
      10: outcome === 2n ? revision - 1n : revision, 11: 1n, 12: 1n, 13: numerator, 14: denominator,
      15: outcome === 2n ? 1n : numerator, 16: outcome === 2n ? 1n : denominator,
      17: outcome, 18: outcome === 2n ? 1n : 0n,
      20: tag === 2n ? playbackFrame * 1000000000n / 48000n : 0n });
  }
  return words(values);
}
function snapshot(event = null, { frontier = 5000n, spans = [0n,0n,0n,0n,1n] } = {}) {
  const rate=event!==null&&(at(event,1)===2n||at(event,1)===3n);
  return { metadata: words([event===null?1n:at(event,5),rate?at(event,10):0n,
    rate?at(event,15):1n,rate?at(event,16):1n,1n,event!==null&&at(event,1)===4n&&at(event,36)===1n?0n:1n]),
    history: words([1n, PROGRAM, CONTEXT, 48000n, 0n, 0n, 0n, frontier, 0n,
      BigInt(spans.length / 5), ...spans]), event };
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
async function attach(h,{activate=true,startFrame=START,started=true,hostNs=ORIGIN}={}) {
  await upload(h);const audio=port(),rpcId=++h.nextRpc;
  await h.send({kind:"play-audio",playId:7,rpcId,port:audio,generation:7,queueCapacity:4096,timeoutMs:50});
  h.audio=audio;
  for(let count=0;!reply(h,rpcId)&&count<8;count++) await ackAudio(h);
  assert.equal(reply(h,rpcId)?.error,undefined);
  if(activate) {
    await h.send({kind:"play-activate",playId:7,rpcId:++h.nextRpc,hostNs,startFrame});
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
// Unlike deliverPoll, this may not settle another operation while seeking a
// poll: the interval after an event ACK is the ordering under test.
async function freshPoll(h,event=null,fields={}) {
  if(!h.audio.pending) {
    h.nextRender=Math.max(h.nextRender??0,...h.of("play-render-done").map(row=>row.renderId))+1;
    await h.send({kind:"play-render",playId:7,renderId:h.nextRender,presentedNs:null,presentedHostNs:null});
  }
  assert.equal(h.audio.pending?.kind,"retained-poll","a fresh observation must precede ordinary drain or retirement");
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
  assert.notEqual(h.audio.pending?.kind,"retained-history-retire");
  await freshPoll(h);
  const retire=await reachControl(h,"retained-history-retire");assert.equal(retire.pin,17n,"only Rust input pin permits retirement");
  await ackAudio(h);await stop(h);
});

for(const mode of ["solo","local"]) test(`${mode} paused rate preserves same-cut event order and yields after both ACKs until fresh observations`,async()=>{
  const h=await catalog();const owner=await begin(h,mode==="local"?{inputMode:"physical-contact",
    localPlanWords:new Uint32Array([99,1,1,0,7,1,2,0])}:{});
  await attach(h);owner.allowCommit=true;
  const pauseId=await request(h,"pause",{paused:true});await reachControl(h,"retained-pause");await ackAudio(h);
  const paused=envelope({tag:4n,id:BigInt(pauseId),serial:10n,paused:true,
    physicalFrame:105839n,playbackFrame:105839n});
  await deliverPoll(h,paused,{retained:snapshot(paused,{frontier:143000n,
    spans:[0n,1n,105839n,0n,1n,105839n,0n,0n,105839n,0n]})});
  await reachControl(h,"retained-event-ack");await ackAudio(h);
  const rateId=await request(h,"rate",{fraction:"3/2"});await reachControl(h,"retained-rate");await ackAudio(h);
  assert.equal(reply(h,rateId)?.result.kind,"retained-pending");
  const resumeId=await request(h,"pause",{paused:false});await reachControl(h,"retained-pause");await ackAudio(h);
  assert.equal(reply(h,resumeId)?.result.kind,"retained-pending");
  // Original twelfth-browser geometry: Resume is delivered before an already
  // applied Rate at physical143215/playback105839, with history pulled further.
  const spans=[0n,1n,105839n,0n,1n,105839n,1n,143215n,105839n,0n,
    143215n,0n,0n,105839n,1n];
  const resume=envelope({tag:4n,id:BigInt(resumeId),serial:11n,paused:false,
    physicalFrame:143215n,playbackFrame:105839n});
  await deliverPoll(h,resume,{retained:snapshot(resume,{frontier:143250n,spans})});
  await reachControl(h,"retained-event-ack");assert.equal(h.audio.pending.serial,11n);
  assert.equal(owner.calls.some(row=>row[0]==="retained-local-ack"&&row[1]===11n),false);
  const host=h.windowNowNs(),input={hostNs:host,key:2,down:true,sequence:17n};
  await h.send({kind:"play-step",playId:7,tickId:h.nextTick=(h.nextTick??0)+1,
    events:[input],watermark:host,nowNs:host,audioNs:0n});
  const queued=owner.pendingInput.find(row=>row.sequence===17n);assert.ok(queued);
  assert.equal(queued.host,host);assert.equal(queued.received,host);
  owner.retainedPin=143237n;
  let services=owner.calls.filter(row=>row[0]==="service").length;
  const retired=h.audio.posts.filter(row=>row.kind==="retained-history-retire").length;
  await ackAudio(h);
  assert.ok(owner.calls.some(row=>row[0]==="retained-local-ack"&&row[1]===11n));
  assert.equal(owner.calls.filter(row=>row[0]==="service").length,services,"Resume ACK cannot drain ordinary input before polling the same-cut Rate");
  assert.equal(h.audio.posts.filter(row=>row.kind==="retained-history-retire").length,retired);
  assert.ok(owner.pendingInput.includes(queued),"acquired input remains owned across Resume ACK");
  const changed=envelope({id:BigInt(rateId),serial:12n,numerator:3n,denominator:2n,
    physicalFrame:143215n,playbackFrame:105839n});
  await freshPoll(h,changed,{retained:snapshot(changed,{frontier:143250n,spans})});
  assert.deepEqual(h.of("play-retained-result").map(row=>[row.id,row.outcome]),[
    [BigInt(pauseId),"paused"],[BigInt(resumeId),"resumed"],[BigInt(rateId),"applied"]]);
  assert.deepEqual(h.of("play-retained-result").slice(-2).map(row=>[at(row.event,3),at(row.event,7),at(row.event,8)]),
    [[11n,143215n,105839n],[12n,143215n,105839n]]);
  await reachControl(h,"retained-event-ack");assert.equal(h.audio.pending.serial,12n);
  assert.equal(owner.calls.some(row=>row[0]==="retained-local-ack"&&row[1]===12n),false);
  services=owner.calls.filter(row=>row[0]==="service").length;
  await ackAudio(h);
  assert.ok(owner.calls.some(row=>row[0]==="retained-local-ack"&&row[1]===12n));
  assert.equal(owner.calls.filter(row=>row[0]==="service").length,services,"Rate ACK also yields before ordinary drain");
  assert.equal(h.audio.posts.filter(row=>row.kind==="retained-history-retire").length,retired);
  assert.ok(owner.pendingInput.includes(queued));
  // The later actual observation, rather than the ACK, permits a newly read pin.
  owner.retainedPin=143249n;
  const idle=snapshot(null,{frontier:143260n,spans});idle.metadata=words([1n,1n,3n,2n,1n,1n]);
  await freshPoll(h,null,{retained:idle});
  assert.ok(owner.calls.filter(row=>row[0]==="service").length>services);
  assert.equal((await reachControl(h,"retained-history-retire")).pin,143249n);
  await ackAudio(h);
  assert.equal(h.of("play-error").length,0);assert.equal(owner.frees,0);
  await stop(h);
});

test("validated nonterminal history refusal retains the last successful pin and retries only after another actual poll",async()=>{
  const h=await catalog();const owner=await begin(h);await attach(h);
  const retirements=()=>h.audio.posts.filter(row=>row.kind==="retained-history-retire");
  owner.retainedPin=17n;await freshPoll(h);
  assert.equal((await reachControl(h,"retained-history-retire")).pin,17n);await ackAudio(h);
  owner.retainedPin=23n;await freshPoll(h);
  assert.equal((await reachControl(h,"retained-history-retire")).pin,23n);
  await ackAudio(h,{status:1,error:"opaque concurrent owner refusal",terminal:false});
  const attempts=retirements().length;
  assert.equal(h.of("play-error").length,0);assert.equal(owner.frees,0);assert.equal(h.audio.closes,0);
  await flushJobs();assert.equal(retirements().length,attempts,"a refusal cannot busy retry");
  await serviceHeld(h);assert.equal(retirements().length,attempts,"ordinary acquisition cannot retry without a fresh poll");
  await freshPoll(h);
  assert.equal((await reachControl(h,"retained-history-retire")).pin,23n,"a failed pin must not enter the success cache");
  await ackAudio(h);
  await freshPoll(h);assert.equal(retirements().length,attempts+1,"only genuine success caches the pin");
  owner.retainedPin=31n;await freshPoll(h);
  assert.equal((await reachControl(h,"retained-history-retire")).pin,31n);
  await ackAudio(h,{status:1,error:"same refusal, different original input minimum",terminal:false});
  owner.retainedPin=29n;await freshPoll(h);
  assert.equal((await reachControl(h,"retained-history-retire")).pin,29n,"retry recomputes the original input pin instead of resending the refused request");
  await ackAudio(h);assert.equal(h.of("play-error").length,0);await stop(h);
});

for(const [name,fault] of [
  ["terminal refusal",{status:1,error:"retained-admission",terminal:true}],
  ["other remote status",{status:2,error:"retained-admission",terminal:false}],
  ["malformed admitted count",{status:1,error:"retained-admission",terminal:false,admitted:1}],
  ["uncorrelated Program",{status:1,error:"retained-admission",terminal:false,programId:PROGRAM+1n}],
  ["transport failure",null],
  ["timeout",null],
]) test(`history retirement preserves fatal ${name} behavior`,async()=>{
  const h=await catalog();const owner=await begin(h);await attach(h);
  owner.retainedPin=23n;await freshPoll(h);await reachControl(h,"retained-history-retire");
  if(name==="transport failure") { h.audio.onmessageerror({});await flushJobs(); }
  else if(name==="timeout") await h.runTimer(50);
  else await ackAudio(h,fault);
  assert.equal(h.of("play-error").length,1);assert.equal(owner.frees,1);assert.equal(h.audio.closes,1);
  const attempts=h.audio.posts.filter(row=>row.kind==="retained-history-retire").length;
  await flushJobs();assert.equal(h.audio.posts.filter(row=>row.kind==="retained-history-retire").length,attempts);
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

test("retained Worker forwards original signed pre-arm and positive raw pairs while playable admission, Started and input custody stay separate",async()=>{
  const h=await catalog();const owner=await begin(h,{rate:44100,recordReplay:true,
    captureBudget:{remainingRecords:128,remainingBytes:256*MiB}});
  await attach(h,{startFrame:16785n,hostNs:1502429999n,started:false});
  const score=owner.score,scoreBytes={...score};
  const registrations=h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length;
  // The port and binding are controlled wire endpoints. A held Started event
  // does not grant real Step/native authority; AC021 owns that actual proof.
  const started=envelope({tag:1n,boundary:0n,id:0n,serial:3n});started[70]=16785;started[71]=0;
  const retained={metadata:snapshot(started).metadata,
    history:words([1n,PROGRAM,16785n,44100n,0n,0n,0n,5000n,0n,1n,0n,0n,0n,0n,1n]),event:started};
  let renderId=0;
  async function observe(contextTime,performanceTime,observedNowMs) {
    await h.send({kind:"play-render",playId:7,renderId:++renderId,
      timestamp:{contextTime,performanceTime},observedNowMs});
    assert.equal(h.audio.pending?.kind,"retained-poll");
    await ackAudio(h,{report:renderReport({start:16785n,cursor:16914n}),retained});
  }
  await observe(0.375309,1530.9200000017881,1532.12);
  assert.deepEqual(owner.calls.filter(row=>row[0]==="retained-presentation"),
    [["retained-presentation",-5303245n,1530920000n]]);
  assert.equal(owner.calls.filter(row=>row[0]==="output").at(-1)[2],null,"negative actual clock is not playable output");
  assert.equal(h.of("play-render-done").at(-1).retainedCapability,null);
  await h.send({kind:"play-step",playId:7,tickId:1,events:[{hostNs:1531000000n,key:2,down:true,sequence:17n}],
    watermark:1531000000n,nowNs:1533000000n,audioNs:0n});
  await observe(0.375309,1531.9200000017881,1533.12); // Repeated output cannot refresh history.
  await observe(0.375308,1532.9200000017881,1534.12); // Regressing negative estimate.
  await observe(0.382072,1537.6850000023842,1540.08);
  await observe(0.382072,1538.6850000023842,1541.08);
  await observe(0.381,1539.6850000023842,1542.08);
  await observe(0.383,1537,1542.08); // Source advances while original HOST regresses.
  await observe(0,1542,1543); // Actual zero sentinel is still unavailable.
  await observe(0.384,500,1543); // Stale actual association.
  await observe(0.384,1545,1544); // Future estimate is not actual presentation yet.
  assert.deepEqual(owner.calls.filter(row=>row[0]==="retained-presentation"),[
    ["retained-presentation",-5303245n,1530920000n],["retained-presentation",1459755n,1537685000n]]);
  assert.deepEqual(owner.calls.filter(row=>row[0]==="output").map(row=>row[2]),
    [null,null,null,1459755n,null,null,null,null,null,null]);
  assert.deepEqual(owner.calls.filter(row=>row[0]==="input"),[["input",1531000000n,2,true,17n,1533000000n]]);
  assert.equal(owner.processedInput.length,0,"the wire fixture does not manufacture an acquired-prefix proof");
  assert.equal(owner.retained_capability_words().length,0);
  assert.ok(h.of("play-render-done").every(row=>row.retainedCapability===null));
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-event-ack"),false);
  assert.equal(h.of("play-retired-recordings").length,0);
  assert.equal(h.visualExports.filter(row=>row.kind===1&&row.owner===owner).length,registrations);
  assert.equal(owner.score,score);assert.deepEqual(owner.score,scoreBytes);
  assert.deepEqual(owner.retainedHeld,started);assert.equal(h.of("play-error").length,0);
  await stop(h);assert.equal(owner.replayTakes,1);
  const final=h.of("play-stopped").at(-1);assert.ok(final?.replay instanceof File);assert.equal(final.replayComplete,false);
});

for(const [name,timestamp,now] of [
  ["NaN context",{contextTime:NaN,performanceTime:1530.92},1532.12],
  ["context overflow",{contextTime:9223372037,performanceTime:1530.92},1532.12],
  ["HOST overflow",{contextTime:0.375309,performanceTime:9223372036855},9223372036855],
]) test(`retained raw ${name} refuses before forwarding an invented clock or Started ACK`,async()=>{
  const h=await catalog();const owner=await begin(h,{rate:44100});
  await attach(h,{startFrame:16785n,hostNs:1502429999n,started:false});
  await h.send({kind:"play-render",playId:7,renderId:1,timestamp,observedNowMs:now});
  assert.ok(h.of("play-error").at(-1));assert.equal(owner.calls.some(row=>row[0]==="retained-presentation"),false);
  assert.equal(h.audio.posts.some(row=>row.kind==="retained-event-ack"),false);assert.equal(owner.frees,1);
});

test("signed retained opt-in does not permit a caller-supplied negative projected presentation",async()=>{
  const h=await catalog();const owner=await begin(h,{rate:44100});
  await attach(h,{startFrame:16785n,hostNs:1502429999n,started:false});
  await h.send({kind:"play-render",playId:7,renderId:1,presentedNs:-5303245n,presentedHostNs:1530920000n});
  assert.match(h.of("play-error").at(-1)?.message??"",/presentation pair/i);
  assert.equal(owner.calls.some(row=>row[0]==="retained-presentation"),false);assert.equal(owner.frees,1);
});

test("ordinary Worker keeps genuine pre-arm output unavailable with the unchanged nonnegative public helper",async()=>{
  const h=await catalog();const owner=await begin(h,{rate:44100,retained:false});
  await attach(h,{startFrame:16785n,hostNs:1502429999n,started:false});
  await h.send({kind:"play-render",playId:7,renderId:1,
    timestamp:{contextTime:0.375309,performanceTime:1530.9200000017881},observedNowMs:1532.12});
  assert.equal(h.audio.pending?.kind,"poll");
  await ackAudio(h,{report:renderReport({start:16785n,cursor:16914n})});
  assert.equal(owner.calls.some(row=>row[0]==="presentation"||row[0]==="retained-presentation"),false);
  assert.equal(owner.calls.filter(row=>row[0]==="output").at(-1)[2],null);
  assert.equal(Object.hasOwn(h.of("play-render-done").at(-1),"retainedCapability"),false);
  assert.equal(h.of("play-error").length,0);await stop(h);
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
async function windowStart(h,{record=false,metadata={},ready=true,local=false}={}) {
  await h.preview();h.get("runtime-retained").checked=true;h.get("record").checked=record;h.get("live-end").value="2";
  if(local) {
    await localCount(h,2);h.click("local-discover");await flush();
    localAssign(h,1,1n);localAssign(h,2,2n);
  }
  const pending=await h.begin(),worker=h.workers.at(-1);
  await h.reply(pending,{kind:"prepared",mode:"live",title:"Retained",artist:"Fixture",notes:1,samples:0,
    lanes:[0x11],opponentCount:0,startNs:0n,endNs:2000000000n,retained:true,programId:PROGRAM,
    capability:null,liveSpeed:"1/1",liveSpeedNumerator:1n,liveSpeedDenominator:1n,
    ...(local?{localPlayers:[1,2],localPage:0}:{}),...metadata});
  const upload=worker.last("play-samples-upload");assert.ok(upload);await h.admitSamples(upload,0);
  await h.reply(upload,{kind:"samples-uploaded",count:0,bytes:0});
  await h.reply(worker.last("play-audio"),null);await h.reply(worker.last("play-activate"),null);
  await h.geometry({playId:pending.playId});
  h.lastSettledTick=0;
  if(ready)await windowCapability(h,pending,capability());
  return pending;
}
async function windowCapability(h,session,value) {
  const worker=h.workers.at(-1);
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

function windowKeyboardEvents(h) {
  return h.workers.at(-1).messages("play-step").flatMap(row=>row.events).filter(row=>Object.hasOwn(row,"key"));
}
async function settleWindowKeyboard(h,session) {
  const worker=h.workers.at(-1);
  for(let count=0;count<4;count++) {
    const step=worker.last("play-step");
    if(!step||step.tickId===h.lastSettledTick)break;
    await h.receive({kind:"play-step-done",playId:session.playId,tickId:step.tickId,
      commandsPending:false,pendingInputs:0,songNs:0n,hits:0n,misses:0n,combo:0n,preOriginInputs:0});
    h.lastSettledTick=step.tickId;
  }
  assert.equal(worker.last("play-step")?.tickId??0,h.lastSettledTick,"the actual keyboard prefix is acknowledged");
}
function focusWindowKeyboard(h,target) {
  target.focus();h.document.activeElement=target;target.emit("focus");
}
async function finishWindowKeyboard(h,session) {
  h.click("stop");await flush();
  await h.receive(session.localPlanWords?localFinal(session):finalScore(session.playId));
}

for(const local of [false,true])for(const editorId of ["runtime-rate","runtime-start","runtime-end"]) {
  const route=local?"local":"solo";
  test(`Window retained ${route} runtime editor ${editorId} delivers the original held-key release and allows the next press`,async()=>{
    const h=await harness({touchSupported:local});try {
      const session=await windowStart(h,{local}),worker=h.workers.at(-1),canvas=h.get("canvas"),editor=h.get(editorId);
      assert.deepEqual(Array.from(session.localPlanWords??[]),local?[1,1,1,0,2,1,2,0]:[]);
      h.setNow(1301);focusWindowKeyboard(h,canvas);
      const down=h.window.emit("keydown",{target:canvas,code:"KeyZ",repeat:false,timeStamp:1300.125});await flush();
      assert.equal(down.defaultPrevented,true);
      const first={hostNs:1300125000n,key:2,down:true,sequence:1n};
      assert.deepEqual(windowKeyboardEvents(h),[first]);
      await settleWindowKeyboard(h,session);
      focusWindowKeyboard(h,editor);await flush();
      assert.deepEqual(windowKeyboardEvents(h),[first],"focus alone cannot fabricate a release or clear the gameplay press");
      const repeat=h.window.emit("keydown",{target:editor,code:"KeyZ",repeat:true,timeStamp:1300.1875});await flush();
      assert.equal(repeat.defaultPrevented,false,"editor key presses retain their native typing action");
      const up=h.window.emit("keyup",{target:editor,code:"KeyZ",repeat:false,timeStamp:1300.25});await flush();
      const released={hostNs:1300250000n,key:2,down:false,sequence:2n};
      assert.deepEqual(windowKeyboardEvents(h),[first,released],"the real editor-targeted key-up keeps its original timestamp and sequence");
      await settleWindowKeyboard(h,session);
      h.window.emit("keyup",{target:editor,code:"KeyZ",repeat:false,timeStamp:1300.3125});await flush();
      assert.deepEqual(windowKeyboardEvents(h),[first,released],"a duplicate key-up cannot release the same press twice");
      focusWindowKeyboard(h,canvas);
      h.window.emit("keydown",{target:canvas,code:"KeyZ",repeat:false,timeStamp:1300.375});await flush();
      await settleWindowKeyboard(h,session);
      h.window.emit("keyup",{target:canvas,code:"KeyZ",repeat:false,timeStamp:1300.5});await flush();
      await settleWindowKeyboard(h,session);
      assert.deepEqual(windowKeyboardEvents(h),[first,released,
        {hostNs:1300375000n,key:2,down:true,sequence:3n},
        {hostNs:1300500000n,key:2,down:false,sequence:4n}],"the next genuine gameplay pair has no stuck press or hidden acquisition");
      assert.equal(worker.messages("play-stop").length,0);assert.equal(worker.messages("play-retained").length,0);
      await finishWindowKeyboard(h,session);
    }finally{await h.close();}
  });

  test(`Window retained ${route} runtime editor ${editorId} preserves fresh typing without gameplay acquisition`,async()=>{
    const h=await harness({touchSupported:local});try {
      const session=await windowStart(h,{local}),worker=h.workers.at(-1),editor=h.get(editorId),canvas=h.get("canvas");
      assert.deepEqual(Array.from(session.localPlanWords??[]),local?[1,1,1,0,2,1,2,0]:[]);
      const draft=editorId==="runtime-rate"?"003/002":"1.234567891";
      editor.value=draft;focusWindowKeyboard(h,editor);h.setNow(1301);
      for(const [type,repeat,timeStamp] of [["keydown",false,1300.125],["keydown",true,1300.1875],["keyup",false,1300.25]]) {
        const event=h.window.emit(type,{target:editor,code:"KeyZ",repeat,timeStamp});
        assert.equal(event.defaultPrevented,false,"fresh editor input keeps the native editing action");
      }
      editor.emit("input");await flush();
      assert.deepEqual(windowKeyboardEvents(h),[]);assert.equal(editor.value,draft);
      assert.equal(worker.messages("play-retained").length,0);assert.equal(worker.messages("play-stop").length,0);
      focusWindowKeyboard(h,canvas);
      h.window.emit("keydown",{target:canvas,code:"KeyZ",repeat:false,timeStamp:1300.375});await flush();
      await settleWindowKeyboard(h,session);
      h.window.emit("keyup",{target:canvas,code:"KeyZ",repeat:false,timeStamp:1300.5});await flush();
      await settleWindowKeyboard(h,session);
      assert.deepEqual(windowKeyboardEvents(h),[
        {hostNs:1300375000n,key:2,down:true,sequence:1n},
        {hostNs:1300500000n,key:2,down:false,sequence:2n}],"typing consumed no shared gameplay sequence or pressed state");
      await finishWindowKeyboard(h,session);
    }finally{await h.close();}
  });
}

for(const editorId of ["runtime-rate","runtime-start","runtime-end"])for(const code of ["Escape","F5","F9"]) {
  test(`Window runtime editor ${editorId} preserves ${code} Play control routing`,async()=>{
    const h=await harness();try {
      const session=await windowStart(h),worker=h.workers.at(-1),editor=h.get(editorId);
      focusWindowKeyboard(h,editor);h.setNow(1301);
      const event=h.window.emit("keydown",{target:editor,code,repeat:false,timeStamp:1300.125});await flush();
      assert.equal(event.defaultPrevented,true);assert.deepEqual(windowKeyboardEvents(h),[]);
      if(code==="Escape") {
        assert.equal(worker.last("play-stop").playId,session.playId);assert.equal(worker.messages("play-retained").length,0);
      } else {
        const request=worker.last("play-retained");assert.equal(request.playId,session.playId);
        assert.equal(request.action,code==="F5"?"restart-original":"pause");
        if(code==="F9")assert.equal(request.paused,true);
        assert.equal(worker.messages("play-retained").length,1);assert.equal(worker.messages("play-stop").length,0);
      }
      await finishWindowKeyboard(h,session);
    }finally{await h.close();}
  });
}

test("Window accepts retired cohort atomically with null scores, unique attempt tuples and custody before ACK",async()=>{
  const h=await harness();try {
    const session=await windowStart(h,{record:true}),worker=h.workers[0];
    const first=retired(session,21n);await h.receive(first);
    const ack=worker.last("play-retired-recordings-ack");assert.equal(ack.accepted,true);assert.equal(ack.boundarySerial,21n);
    const choices=()=>h.get("captured-replay").children.filter(option=>option.value!=="");
    assert.equal(ack.remainingRecords,127);assert.equal(choices().length,1);
    assert.equal(choices()[0].value,`${session.playId}:${PROGRAM}:21:1`);
    const second=retired(session,22n);await h.receive(second);
    assert.equal(worker.last("play-retired-recordings-ack").accepted,true);
    assert.equal(choices().length,2,"same player in later attempt has a distinct tuple");
    assert.equal(choices()[1].value,`${session.playId}:${PROGRAM}:22:1`);
    await h.receive(first);assert.equal(worker.last("play-retired-recordings-ack").accepted,false);
    assert.equal(choices().length,2);
    h.click("stop");await flush();await h.receive(finalScore(session.playId,{replay:new File([new Uint8Array([66,75,82,77])],"current.bkr"),replayComplete:false,replayError:null}));
    assert.equal(choices().length,3,"terminal capture key cannot alias a retired boundary");
    assert.equal(choices()[2].value,`${session.playId}:${PROGRAM}:current:1`);
    h.get("captured-replay").value=choices()[0].value;h.get("captured-replay").emit("change");
    h.click("records-save");await flush();
    const save=worker.messages("records-request").find(row=>row.action==="save")?.value;assert.ok(save);
    assert.equal(save.metadata.hits,null);assert.equal(save.metadata.misses,null);assert.equal(save.metadata.combo,null);
    assert.equal(save.metadata.complete,false);assert.equal(save.replayFile,first.replays[0].replay);
    assert.equal(save.archiveBlob,null);assert.equal(save.archivePlayer,null);
  }finally{await h.close();}
});

function windowCaptureChoices(h) {
  return h.get("captured-replay").children.filter(option=>option.value!=="");
}
function originalWindowCapture(name,marker) {
  const file=new File([Uint8Array.of(66,75,82,marker)],name);
  file.arrayBuffer=()=>assert.fail("Window must preserve the original recording File handle");
  return file;
}
function sizedWindowCapture(name,size,marker=1) {
  const file=originalWindowCapture(name,marker);
  // File metadata is the controlled endpoint, as in the existing retired
  // budget fixture; the Window must never allocate or acquire its body.
  Object.defineProperty(file,"size",{value:size});
  return file;
}
async function boundedWindowStash(h,{records=1,bytePressure=false}={}) {
  const session=await windowStart(h,{record:true}),worker=h.workers.at(-1),rows=[];
  const prefixes=bytePressure?3:Math.min(records,127);
  for(let index=0;index<prefixes;index++) {
    const serial=1000n+BigInt(index),file=sizedWindowCapture(`held-${index}.bkr`,bytePressure?64*MiB:4,index);
    await h.receive(retired(session,serial,[1],()=>file));
    assert.equal(worker.last("play-retired-recordings-ack").accepted,true,"fixture enters custody through actual retired acceptance");
    rows.push({key:`${session.playId}:${PROGRAM}:${serial}:1`,file});
  }
  const current=bytePressure&&records!==3?sizedWindowCapture("held-current.bkr",MiB)
    :records===128?sizedWindowCapture("held-current.bkr",4):null;
  h.click("stop");await flush();await h.receive(finalScore(session.playId,{replay:current}));
  if(current)rows.push({key:`${session.playId}:${PROGRAM}:current:1`,file:current});
  assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),rows.map(row=>row.key));
  assert.equal(h.get("play").disabled,false);
  return rows;
}
async function ordinaryWindowDraft(h,players=1,record=true) {
  h.get("runtime-retained").checked=false;h.get("live-end").value="";h.get("record").checked=record;
  await localCount(h,players);
  if(players>1) {
    h.click("local-discover");await flush();
    for(let player=1;player<=players;player++)localAssign(h,player,BigInt(player));
  }
}
async function finishOrdinaryWindow(h,session,files=[]) {
  h.click("stop");await flush();
  await h.receive(session.start.localPlanWords===undefined
    ?finalScore(session.id,{replay:files[0]??null})
    :localFinal(session.start,{replays:Array.from(session.start.localPlanWords).filter((_,index)=>index%4===0)
      .map((player,index)=>({player,replay:files[index]??null,replayComplete:false,replayError:null}))}));
}
async function selectWindowCapture(h,key,file) {
  assert.equal(windowCaptureChoices(h).filter(option=>option.value===key).length,1,"one selectable row per capture identity");
  h.get("captured-replay").value=key;h.get("captured-replay").emit("change");
  const count=windowCaptureChoices(h).length,downloads=h.downloads.length;
  h.click("export");await flush();
  assert.equal(h.urls.at(-1).blob,file,"selection downloads the exact original File");
  assert.equal(h.downloads.length,downloads+1);
  assert.equal(windowCaptureChoices(h).length,count,"download preserves unsaved custody");
}
async function saveWindowCapture(h,file,player=1,players=[1]) {
  const worker=h.workers.at(-1),before=worker.messages("records-request").filter(row=>row.action==="save").length;
  h.click("records-save");await flush();
  const requests=worker.messages("records-request").filter(row=>row.action==="save");
  assert.equal(requests.length,before+1,"explicit Save sends one storage request");
  const value=requests.at(-1).value;
  assert.equal(value.replayFile,file,"selection saves the exact original File");
  assert.equal(value.player,player);assert.deepEqual(value.players,players);
  assert.equal(value.archiveBlob,null);assert.equal(value.archivePlayer,null);
  assert.match(h.get("status").textContent,/Recording saved/);
  return value;
}

test("Window retained captures keep session identity when a replacement Worker reuses Program and boundary IDs",async()=>{
  const h=await harness({noWindowRecordBinary:true});try {
    const first=await windowStart(h,{record:true}),oldWorker=h.workers.at(-1);
    const oldPrefix=originalWindowCapture("old-prefix.bkr",31),oldCurrent=originalWindowCapture("old-current.bkr",32);
    await h.receive(retired(first,21n,[1],()=>oldPrefix));
    assert.equal(oldWorker.last("play-retired-recordings-ack").accepted,true);
    h.click("stop");await flush();await h.receive(finalScore(first.playId,{replay:oldCurrent}));
    const previousKeys=[`${first.playId}:${PROGRAM}:21:1`,`${first.playId}:${PROGRAM}:current:1`];
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),previousKeys);
    assert.equal(h.recordCalls.filter(row=>row.method==="save").length,0);

    h.window.emit("pagehide");await flush();assert.equal(oldWorker.terminations,1);
    h.window.emit("pageshow",{persisted:true});
    const second=await windowStart(h,{record:true}),worker=h.workers.at(-1);
    assert.notEqual(worker,oldWorker);assert.ok(second.playId>first.playId);
    const prefix=originalWindowCapture("replacement-prefix.bkr",33),current=originalWindowCapture("replacement-current.bkr",34);
    const cohort=retired(second,21n,[1],()=>prefix);await h.receive(cohort);
    const accepted=worker.last("play-retired-recordings-ack");
    assert.equal(accepted.accepted,true,"a new session may reuse the replacement Worker's Program/boundary/player");
    assert.equal(accepted.programId,PROGRAM);assert.equal(accepted.boundarySerial,21n);
    assert.equal(accepted.remainingRecords,125);
    const beforeDuplicate=windowCaptureChoices(h).map(option=>option.value);
    await h.receive(cohort);
    const duplicate=worker.last("play-retired-recordings-ack");
    assert.equal(duplicate.accepted,false,"the same session/Program/boundary/player still refuses");
    assert.equal(duplicate.remainingRecords,accepted.remainingRecords);assert.equal(duplicate.remainingBytes,accepted.remainingBytes);
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),beforeDuplicate,"duplicate refusal is atomic");
    h.click("stop");await flush();await h.receive(finalScore(second.playId,{replay:current}));
    const rows=[{key:previousKeys[0],file:oldPrefix},{key:previousKeys[1],file:oldCurrent},
      {key:`${second.playId}:${PROGRAM}:21:1`,file:prefix},{key:`${second.playId}:${PROGRAM}:current:1`,file:current}];
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),rows.map(row=>row.key));
    assert.equal(new Set(rows.map(row=>row.key)).size,4);
    for(const row of rows)await selectWindowCapture(h,row.key,row.file);
    assert.equal(h.recordCalls.filter(row=>row.method==="save").length,0,"downloads do not automatically save Files");
    for(let index=0;index<rows.length;index++) {
      const row=rows[index];await selectWindowCapture(h,row.key,row.file);
      await saveWindowCapture(h,row.file);
      assert.equal(windowCaptureChoices(h).length,rows.length-index-1,"successful Save releases only its selected retained File");
    }
  }finally{await h.close();}
});

test("Window repeated ordinary solo captures retain distinct session selection beside unsaved retained Files",async()=>{
  const h=await harness({noWindowRecordBinary:true});try {
    const retainedSession=await windowStart(h,{record:true}),prefix=originalWindowCapture("retained-before-solo.bkr",41);
    await h.receive(retired(retainedSession,21n,[1],()=>prefix));
    assert.equal(h.workers.at(-1).last("play-retired-recordings-ack").accepted,true);
    h.click("stop");await flush();await h.receive(finalScore(retainedSession.playId));
    h.get("runtime-retained").checked=false;h.get("live-end").value="";
    const rows=[{key:`${retainedSession.playId}:${PROGRAM}:21:1`,file:prefix}];
    for(const marker of [42,43]) {
      const session=await h.launch(),file=originalWindowCapture(`ordinary-solo-${marker}.bkr`,marker);
      assert.equal(Object.hasOwn(session.start,"retained"),false);assert.equal(session.start.recordReplay,true);
      assert.ok(session.id>retainedSession.playId);
      h.click("stop");await flush();await h.receive(finalScore(session.id,{replay:file}));
      rows.push({key:`${session.id}:solo`,file});
      assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),rows.map(row=>row.key));
    }
    assert.equal(new Set(windowCaptureChoices(h).map(option=>option.value)).size,3);
    for(const row of rows) {
      await selectWindowCapture(h,row.key,row.file);await saveWindowCapture(h,row.file);
    }
  }finally{await h.close();}
});

test("Window repeated ordinary local captures retain session and member selection beside unsaved retained Files",async()=>{
  const h=await harness({touchSupported:true,noWindowRecordBinary:true});try {
    const retainedSession=await windowStart(h,{record:true}),prefix=originalWindowCapture("retained-before-local.bkr",51);
    await h.receive(retired(retainedSession,21n,[1],()=>prefix));
    assert.equal(h.workers.at(-1).last("play-retired-recordings-ack").accepted,true);
    h.click("stop");await flush();await h.receive(finalScore(retainedSession.playId));
    h.get("runtime-retained").checked=false;h.get("live-end").value="";await localCount(h,2);
    const rows=[{key:`${retainedSession.playId}:${PROGRAM}:21:1`,file:prefix,player:1,players:[1]}];
    for(const marker of [52,54]) {
      h.click("local-discover");await flush();localAssign(h,1,1n);localAssign(h,2,2n);
      const session=await h.launch();assert.equal(Object.hasOwn(session.start,"retained"),false);assert.equal(session.start.recordReplay,true);
      const files=[originalWindowCapture(`ordinary-local-${marker}-1.bkr`,marker),
        originalWindowCapture(`ordinary-local-${marker}-2.bkr`,marker+1)];
      h.click("stop");await flush();await h.receive(localFinal(session.start,{replays:files.map((replay,index)=>({
        player:index+1,replay,replayComplete:false,replayError:null}))}));
      files.forEach((file,index)=>rows.push({key:`${session.id}:${index+1}`,file,player:index+1,players:[1,2]}));
      assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),rows.map(row=>row.key));
      assert.equal(h.get("records-save").disabled,true,"local member selection remains explicit");
    }
    assert.equal(new Set(windowCaptureChoices(h).map(option=>option.value)).size,5);
    for(const row of rows) {
      await selectWindowCapture(h,row.key,row.file);await saveWindowCapture(h,row.file,row.player,row.players);
    }
  }finally{await h.close();}
});

for(const players of [1,2])for(const pressure of ["slots","bytes"]) {
  const route=players===1?"solo":"local";
  test(`Window ordinary ${route} refuses recording admission before ownership when retained ${pressure} cannot reserve its final export`,async()=>{
    const h=await harness({touchSupported:true,noWindowRecordBinary:true});try {
      const rows=await boundedWindowStash(h,pressure==="slots"?{records:129-players}:{bytePressure:true});
      await ordinaryWindowDraft(h,players);
      const worker=h.workers.at(-1),starts=worker.messages("play-start").length,
        stops=worker.messages("play-stop").length,opens=h.opens.length;
      h.click("play");await flush();
      assert.equal(worker.messages("play-start").length,starts,"capacity refusal precedes Worker playback admission");
      assert.equal(worker.messages("play-stop").length,stops,"refused admission creates no gameplay owner to stop");
      assert.equal(h.opens.length,opens,"capacity refusal precedes a new audio owner");
      assert.equal(worker.terminations,0);
      assert.match(h.get("status").textContent,/save|download/i,"refusal explains how existing custody can be recovered");
      assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),rows.map(row=>row.key));
      assert.equal(h.get("captured-replay").disabled,false,"old File selection remains usable after refusal");
      assert.equal(h.recordCalls.filter(row=>row.method==="save").length,0);assert.equal(h.downloads.length,0);
      await selectWindowCapture(h,rows[0].key,rows[0].file);
      await saveWindowCapture(h,rows[0].file);
      assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),rows.slice(1).map(row=>row.key),"Save releases only its selected held row");
      const retry=await h.launch();assert.equal(retry.start.recordReplay,true);
      await finishOrdinaryWindow(h,retry);
    }finally{await h.close();}
  });

  test(`Window ordinary ${route} completes the exactly reserved final export beside retained ${pressure}`,async()=>{
    const h=await harness({touchSupported:true,noWindowRecordBinary:true});try {
      const held=await boundedWindowStash(h,pressure==="slots"?{records:128-players}:{records:3,bytePressure:true});
      await ordinaryWindowDraft(h,players);const session=await h.launch();
      assert.equal(session.start.recordReplay,true);assert.equal(Object.hasOwn(session.start,"retained"),false);
      const files=Array.from({length:players},(_,index)=>sizedWindowCapture(`exact-${route}-${index}.bkr`,64*MiB/players,index+11));
      await finishOrdinaryWindow(h,session,files);
      const added=files.map((file,index)=>({key:`${session.id}:${players===1?"solo":index+1}`,file}));
      assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),[...held,...added].map(row=>row.key));
      assert.equal(h.get("play").disabled,false,"the original final receipt completes the lifecycle");
      assert.equal(h.audio.stopStarts,1);assert.equal(h.workers.at(-1).terminations,0);
      for(let index=0;index<added.length;index++) {
        const row=added[index],before=windowCaptureChoices(h).length;
        await selectWindowCapture(h,row.key,row.file);
        await saveWindowCapture(h,row.file,index+1,Array.from({length:players},(_,member)=>member+1));
        assert.equal(windowCaptureChoices(h).length,before-1,"ordinary appended rows release exact bounded custody after Save ACK");
      }
      assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),held.map(row=>row.key));
    }finally{await h.close();}
  });
}

for(const mode of ["capture-off","replay"])test(`Window ${mode} bypasses full retained recording custody`,async()=>{
  const h=await harness({noWindowRecordBinary:true});try {
    const held=await boundedWindowStash(h,{records:128});
    await ordinaryWindowDraft(h,1,mode==="replay");
    const replayFile=mode==="replay"?selectedRecording().file:null;
    if(replayFile)chooseRecording(h,[replayFile]);
    const session=await h.launch(0,mode==="replay"?"replay":"live");
    if(mode==="replay") {
      assert.equal(session.start.mode,"replay");assert.equal(Object.hasOwn(session.start,"recordReplay"),false);
      assert.equal(session.start.replayFile,replayFile,"replay preserves the original selected File handle");
    } else assert.equal(session.start.recordReplay,false);
    assert.equal(Object.hasOwn(session.start,"captureBudget"),false);
    assert.equal(Object.hasOwn(session.start,"retained"),false);
    await finishOrdinaryWindow(h,session);
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),held.map(row=>row.key));
    assert.equal(h.get("play").disabled,false);assert.equal(h.recordCalls.filter(row=>row.method==="save").length,0);
    assert.equal(h.downloads.length,0);
  }finally{await h.close();}
});

test("Window saving the last Program row preserves bounded ordinary custody through the next play",async()=>{
  const h=await harness({noWindowRecordBinary:true});try {
    const [program]=await boundedWindowStash(h);
    await ordinaryWindowDraft(h);const first=await h.launch(),one=originalWindowCapture("bounded-ordinary-one.bkr",71);
    await finishOrdinaryWindow(h,first,[one]);
    const firstKey=`${first.id}:solo`;
    await selectWindowCapture(h,program.key,program.file);await saveWindowCapture(h,program.file);
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),[firstKey]);
    const second=await h.launch(),two=originalWindowCapture("bounded-ordinary-two.bkr",72);
    await finishOrdinaryWindow(h,second,[two]);
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),[firstKey,`${second.id}:solo`],"remaining unsaved ordinary row keeps append policy active");
    await selectWindowCapture(h,firstKey,one);await saveWindowCapture(h,one);
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),[`${second.id}:solo`]);
    await selectWindowCapture(h,`${second.id}:solo`,two);await saveWindowCapture(h,two);
    assert.equal(windowCaptureChoices(h).length,0);
  }finally{await h.close();}
});

test("Window ordinary-only legacy recordings still replace the previous last capture",async()=>{
  const h=await harness({noWindowRecordBinary:true});try {
    await h.preview();await ordinaryWindowDraft(h);
    const first=await h.launch(),one=originalWindowCapture("legacy-one.bkr",81);
    await finishOrdinaryWindow(h,first,[one]);
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),[`${first.id}:solo`]);
    const second=await h.launch(),two=originalWindowCapture("legacy-two.bkr",82);
    await finishOrdinaryWindow(h,second,[two]);
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),[`${second.id}:solo`]);
    await selectWindowCapture(h,`${second.id}:solo`,two);await saveWindowCapture(h,two);
    assert.deepEqual(windowCaptureChoices(h).map(option=>option.value),[`${second.id}:solo`],"legacy last capture remains available after Save");
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
