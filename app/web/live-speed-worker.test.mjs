// Root runs: node --experimental-vm-modules --test app/web/live-speed-worker.test.mjs
// Actual Worker/RenderClient/command client; generated owner edge is mocked.
// Actual WASM/PCM is covered separately by live-speed-export.test.mjs.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { encodeKeyboardEvent } from "./physical-input.mjs";
import { BMS_TIMING_PRESET_ID } from "./play-model.mjs";

// Reuse the established ownership harness without registering its test suite
// or exporting a new production/test seam. Only its generated owner fixture is
// extended; production Worker source and timing helpers are read unchanged.
const fixtureUrl = new URL("./play-worker.test.mjs", import.meta.url);
const original = await readFile(fixtureUrl, "utf8");
const marker = 'test("actual Worker settings codec';
assert.ok(original.includes(marker), "existing harness definition boundary remains explicit");
let fixture = original.slice(0, original.indexOf(marker));
fixture = fixture.replaceAll("import.meta.url", JSON.stringify(fixtureUrl.href));
fixture = fixture.replace(/from "(\.\/[^\"]+)"/g, (_, path) => `from ${JSON.stringify(new URL(path, fixtureUrl).href)}`);
function injectBefore(anchor, addition) {
  assert.equal(fixture.split(anchor).length, 2, `one fixture seam: ${anchor}`);
  fixture = fixture.replace(anchor, `${addition}\n${anchor}`);
}
injectBefore("  if (options.missingSectionConstructor)", `
  const liveSpeedConstructions = [];
  function installSpeed(ownerClass, names) {
    for (const [name, legacy] of names) {
      const construct = legacy ? ownerClass[legacy] : null;
      ownerClass[name] = function(prepared, ...args) {
        const fraction = args.pop();
        const owner = construct ? construct.call(this, prepared, ...args) : new this(prepared, ...args);
        const [numerator, denominator] = fraction.split("/").map(BigInt);
        owner.liveSelected = { numerator, denominator };
        liveSpeedConstructions.push({ name, fraction, owner, args });
        return owner;
      };
      if (options.missingLiveConstructor === name || options.missingAllLiveConstructors) ownerClass[name] = undefined;
    }
  }
  installSpeed(BrowserGame, [["new_and_speed", null], ["new_section_and_speed", "new_section"],
    ["new_physical_and_speed", "new_physical"], ["new_physical_contact_and_speed", "new_physical_contact"]]);
  const addSaved = BrowserGame.prototype.add_saved_opponent;
  BrowserGame.prototype.add_saved_opponent = function(...args) {
    if (options.savedAdmissionError) throw new Error(options.savedAdmissionError);
    return addSaved.call(this, ...args);
  };
  for (const [name, field] of [["speed_numerator", "numerator"], ["speed_denominator", "denominator"]]) {
    if (options.missingLiveGetters || options.missingLiveGetter === name) continue;
    Object.defineProperty(BrowserGame.prototype, name, { get() {
      this.live();
      if (Object.hasOwn(options, name)) return options[name];
      return this.liveSelected?.[field] ?? 1n;
    }});
  }
`);
injectBefore("  if (options.missingLocalTouchSurfacePage)", `
  installSpeed(BrowserLocalGame, [["new_physical_and_speed", "new_physical"]]);
`);
injectBefore("    completedOwners, renderPort, visualExports", "    liveSpeedConstructions,");
fixture += "\nexport { workerHarness, selectedFile, deferred, flushJobs, ORIGIN, START, SCORE, pairs, renderReport };\n";
const { workerHarness, selectedFile, deferred, flushJobs, ORIGIN, START, SCORE, pairs, renderReport } =
  await import(`data:text/javascript;base64,${Buffer.from(fixture).toString("base64")}`);

function request(fields = {}) {
  return { kind: "play-start", playId: 7, rpcId: 1, libraryId: 1, path: "song/chart.bms", rate: 48000,
    seed: "18446744073709551615", keyPairs: pairs(), windowOriginNs: 10000000000n, ...fields };
}
async function catalog(options = {}, file = selectedFile("song/chart.bms")) {
  const h = await workerHarness(options);
  await h.send({ kind: "init", canvas: { transferred: true } });
  assert.equal(h.of("ready").length, 1);
  await h.send({ kind: "import", id: 1, files: [file] });
  await h.send({ kind: "accept-library", id: 1 });
  await h.send({ kind: "resize", width: 640, height: 480 });
  return h;
}
function prepared(h) {
  return h.of("play-reply").find(row => row.result?.kind === "prepared")?.result;
}
async function rpc(h, kind, fields = {}) {
  h.nextRpc = (h.nextRpc ?? 1) + 1;
  await h.send({ kind, playId: 7, rpcId: h.nextRpc, ...fields });
  const reply = h.of("play-reply").find(row => row.rpcId === h.nextRpc);
  assert.ok(reply, `reply for ${kind}`);
  return reply;
}
async function stop(h, playId = 7) {
  await h.send({ kind: "play-stop", playId });
  const owner = h.games.at(-1) ?? h.locals.at(-1);
  if (owner) { assert.equal(owner.stops, 1); assert.equal(owner.frees, 1); }
}
const grid = [
  ["02/04", "1/2", 1n, 2n, 148800n],
  ["2/2", "1/1", 1n, 1n, 76800n],
  ["4/2", "2/1", 2n, 1n, 40800n],
  ["6/4", "3/2", 3n, 2n, 52800n],
];

test("live Worker publishes canonical four-rate identity and literal original-section output fences", async () => {
  const startNs = 604800000000001n, endNs = startNs + 1500000000n;
  for (const [liveSpeed, fraction, numerator, denominator, endFrame] of grid) {
    const h = await catalog({ gameEnd: endNs, gameEndFrame: endFrame });
    await h.send(request({ liveSpeed, startNs, endNs, recordReplay: true }));
    const response = prepared(h);
    assert.ok(response, "prepared selected-speed owner");
    assert.equal(response.liveSpeed, fraction);
    assert.equal(response.liveSpeedNumerator, numerator);
    assert.equal(response.liveSpeedDenominator, denominator);
    assert.equal(response.startNs, startNs);
    assert.equal(response.endNs, endNs);
    assert.equal(response.endFrame, endFrame);
    if (fraction !== "1/1") {
      assert.equal(h.liveSpeedConstructions.length, 1);
      assert.equal(h.liveSpeedConstructions[0].name, "new_section_and_speed");
      assert.equal(h.liveSpeedConstructions[0].fraction, fraction);
    }
    assert.deepEqual(h.games[0].calls, [["capture", 64 * 1024 * 1024, 1000000]]);
    await stop(h);
  }
});

test("omitted and reduced normal live speed preserve old exports and absent getter compatibility", async () => {
  for (const fields of [{}, { liveSpeed: "2/2" }]) {
    const h = await catalog({ missingAllLiveConstructors: true, missingLiveGetters: true });
    await h.send(request(fields));
    assert.equal(prepared(h)?.liveSpeed, "1/1");
    assert.equal(prepared(h)?.liveSpeedNumerator, 1n);
    assert.equal(prepared(h)?.liveSpeedDenominator, 1n);
    assert.equal(h.liveSpeedConstructions.length, 0);
    assert.equal(h.games.length, 1);
    await stop(h);
  }
});

test("strict live fractions refuse before chart acquisition or preparation and keep replay controls separate", async () => {
  for (const liveSpeed of [null, "", "0/1", "1/0", "-1/2", "+1/2", "1.5/1", "1/2/3", "１/2", " 1/2",
    "9223372036854775808/1", "1/18446744073709551616"]) {
    let reads = 0;
    const h = await catalog({}, selectedFile("song/chart.bms", () => { reads++; throw new Error("unexpected read"); }));
    await h.send(request({ liveSpeed }));
    assert.equal(reads, 0);
    assert.equal(h.games.length, 0);
    assert.equal(h.libraries[0].preparations.length, 0);
    assert.equal(h.of("play-error").length, 1);
  }
  const h = await catalog();
  await h.send(request({ liveSpeed: "2/1", replaySpeed: "3/2" }));
  assert.equal(h.games.length, 0);
  assert.equal(h.of("play-error").length, 1);
});

test("each selected nonnormal binding is required before any chart read with no normal fallback", async () => {
  for (const [fields, name] of [
    [{}, "new_and_speed"], [{ endNs: 1n }, "new_section_and_speed"],
    [{ inputMode: "physical" }, "new_physical_and_speed"],
    [{ inputMode: "physical-contact" }, "new_physical_contact_and_speed"],
  ]) {
    let reads = 0;
    const h = await catalog({ missingLiveConstructor: name }, selectedFile("song/chart.bms", () => { reads++; throw new Error("unexpected read"); }));
    await h.send(request({ liveSpeed: "3/2", ...fields }));
    assert.equal(reads, 0);
    assert.equal(h.games.length, 0);
    assert.equal(h.liveSpeedConstructions.length, 0);
    assert.equal(prepared(h), undefined);
    assert.equal(h.of("play-error").length, 1);
  }
});

test("actual owner speed getters are mandatory exact BigInts before capture or sample effects", async () => {
  for (const options of [
    { missingLiveGetters: true }, { missingLiveGetter: "speed_denominator" },
    { speed_numerator: 2 }, { speed_denominator: 1 },
    { speed_numerator: 1n }, { speed_denominator: 2n },
  ]) {
    const h = await catalog(options);
    await h.send(request({ liveSpeed: "2/1", recordReplay: true }));
    assert.equal(prepared(h), undefined);
    assert.equal(h.of("play-error").length, 1);
    assert.equal(h.games.length, 1);
    assert.equal(h.games[0].calls.some(row => row[0] === "capture"), false);
    assert.equal(h.games[0].sampleIndex, 0);
    assert.equal(h.games[0].stops, 1);
    assert.equal(h.games[0].frees, 1);
  }
});

test("mixed artifact normal fence is rejected for double speed while quantized zero song-span retains physical preroll fence", async () => {
  const wrong = await catalog({ gameEnd: 1500000000n, gameEndFrame: 76800n });
  await wrong.send(request({ liveSpeed: "2/1", endNs: 1500000000n }));
  assert.equal(prepared(wrong), undefined);
  assert.equal(wrong.of("play-error").length, 1);
  const zero = await catalog({ gameEnd: 1n, gameEndFrame: 4800n });
  await zero.send(request({ liveSpeed: "2/1", endNs: 1n }));
  assert.equal(prepared(zero)?.endFrame, 4800n);
  await stop(zero);
});

test("full width valid speed fractions cross Worker into consuming export without Number narrowing", async () => {
  for (const [liveSpeed, numerator, denominator] of [
    ["9223372036854775807/9223372036854775806", 9223372036854775807n, 9223372036854775806n],
    ["1/18446744073709551615", 1n, 18446744073709551615n],
  ]) {
    const h = await catalog();
    await h.send(request({ liveSpeed }));
    assert.equal(prepared(h)?.liveSpeedNumerator, numerator);
    assert.equal(prepared(h)?.liveSpeedDenominator, denominator);
    assert.equal(h.liveSpeedConstructions[0].fraction, liveSpeed);
    await stop(h);
  }
});

test("live speed snapshot survives held chart acquisition and cancelled old generation cannot replace a retry", async () => {
  const gate = deferred();
  const h = await catalog({}, selectedFile("song/chart.bms", () => gate.promise));
  const draft = request({ liveSpeed: "6/4" });
  await h.send(draft);
  draft.liveSpeed = "1/2";
  gate.resolve(new TextEncoder().encode("#BPM 120").buffer);
  await flushJobs();
  assert.equal(prepared(h)?.liveSpeed, "3/2");
  assert.equal(h.liveSpeedConstructions[0].fraction, "3/2");
  await stop(h);

  const late = deferred();
  const next = await catalog({}, selectedFile("song/chart.bms", () => late.promise));
  await next.send(request({ liveSpeed: "2/1" }));
  await next.send({ kind: "play-stop", playId: 7 });
  late.resolve(new TextEncoder().encode("#BPM 120").buffer);
  await flushJobs();
  assert.equal(next.games.length, 0);
  await next.send(request({ playId: 8, rpcId: 9, liveSpeed: "1/2" }));
  assert.equal(prepared(next)?.liveSpeed, "1/2");
  await next.send({ kind: "play-stop", playId: 7 });
  assert.equal(next.games[0].stops, 0, "stale stop cannot dispose retry");
  await stop(next, 8);
});

test("physical input remains original acquisition bytes and raw audio commands retain exact core credit", async () => {
  const original = { sequence: 55n, commands: [
    { kind: 3, voice: 0n, sample: 0n, at: 0n, gain: 1, value: 3n, denominator: 2n },
    { kind: 0, voice: 7n, sample: 19n, at: 17000000n, gain: 0.5, value: 0n, denominator: 1n },
  ] };
  const h = await catalog({ batches: [original, null] });
  await h.send(request({ liveSpeed: "3/2", inputMode: "physical", recordReplay: true }));
  assert.ok(prepared(h));
  await rpc(h, "play-sample"); await rpc(h, "play-sample"); await rpc(h, "play-sample");
  const pending = await rpc(h, "play-commands");
  assert.deepEqual(structuredClone(pending.result), original);
  await rpc(h, "play-ack", { sequence: 55n, admitted: 2, success: true });
  await rpc(h, "play-commands");
  await rpc(h, "play-activate", { hostNs: ORIGIN, startFrame: START });
  const event = { hostNs: ORIGIN + 17n, key: 2, down: true, sequence: 1n };
  await h.send({ kind: "play-step", playId: 7, tickId: 1, events: [event], watermark: event.hostNs,
    nowNs: event.hostNs, audioNs: 17000000n });
  const blob = h.games[0].calls.find(row => row[0] === "blob");
  assert.deepEqual(Array.from(blob[1]), Array.from(encodeKeyboardEvent(event)));
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(h.of("play-stopped").at(-1).replayComplete, false);
  assert.equal(h.games[0].frees, 1);
});

test("local members share one selected speed owner while capture remains independent per member", async () => {
  const h = await catalog();
  await h.send(request({ liveSpeed: "6/4", inputMode: "physical-contact", recordReplay: true,
    localPlanWords: new Uint32Array([99, 1, 1, 0, 7, 1, 2, 0]) }));
  assert.equal(prepared(h)?.liveSpeed, "3/2");
  assert.equal(h.games.length, 0);
  assert.equal(h.locals.length, 1);
  assert.equal(h.liveSpeedConstructions.length, 1);
  assert.equal(h.liveSpeedConstructions[0].name, "new_physical_and_speed");
  assert.deepEqual(h.locals[0].memberIds, [99, 7]);
  assert.deepEqual([...h.locals[0].memberCaptures], [99, 7]);
  await stop(h);
  assert.deepEqual([...h.locals[0].memberReplayTakes], [[99, 1], [7, 1]]);
  assert.equal(h.of("play-stopped").at(-1).replayComplete, false);
});

test("direct command channel retains selected rate and raw keysound until exact generation ACK", async () => {
  const prefix = { sequence: 71n, commands: [
    { kind: 3, voice: 0n, sample: 0n, at: 0n, gain: 1, value: 2n, denominator: 1n },
    { kind: 0, voice: 9n, sample: 19n, at: 17000000n, gain: 1, value: 0n, denominator: 1n },
  ] };
  const h = await catalog({ batches: [prefix, null] });
  await h.send(request({ liveSpeed: "2/1" }));
  assert.ok(prepared(h));
  for (let i = 0; i < 3; i++) await rpc(h, "play-sample");
  const port = { posts: [], onmessage: null, onmessageerror: null, closes: 0,
    start() {}, close() { this.closes++; }, postMessage(value) { this.posts.push(structuredClone(value)); } };
  h.nextRpc = (h.nextRpc ?? 1) + 1;
  const audioRpc = h.nextRpc;
  await h.send({ kind: "play-audio", playId: 7, rpcId: audioRpc, port, generation: 7,
    queueCapacity: 4096, timeoutMs: 50 });
  const sent = port.posts.at(-1);
  assert.deepEqual(sent.commands, prefix.commands);
  assert.equal(h.games[0].calls.some(row => row[0] === "ack"), false);
  const ack = { kind: "ack", generation: sent.generation, sequence: sent.sequence,
    operation: sent.kind, status: 0, admitted: 2, error: null, report: null };
  port.onmessage({ data: { ...ack, generation: 6 } });
  await flushJobs();
  assert.equal(h.games[0].calls.some(row => row[0] === "ack"), false);
  port.onmessage({ data: ack }); await flushJobs();
  assert.ok(h.games[0].calls.some(row => row[0] === "ack" && row[1] === 71n));
  assert.equal(h.of("play-reply").find(row => row.rpcId === audioRpc)?.error, undefined);
  await stop(h);
  assert.equal(port.closes, 1);
});

test("original song end scalar and source rendering never replace actual owner completion permission", async () => {
  const endNs = 1500000000n;
  const h = await catalog({ gameEnd: endNs, gameEndFrame: 40800n,
    observeOutput(owner) { owner.score.song_ns = endNs; return false; } });
  await h.send(request({ liveSpeed: "2/1", endNs, recordReplay: true }));
  assert.ok(prepared(h));
  for (let i = 0; i < 3; i++) await rpc(h, "play-sample");
  await rpc(h, "play-commands");
  await rpc(h, "play-activate", { hostNs: ORIGIN, startFrame: START });
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(),
    presentedNs: 850000000n, presentedHostNs: ORIGIN + 1n });
  assert.equal(h.of("play-render-done").at(-1).completed, false);
  await h.send({ kind: "play-stop", playId: 7, completed: true });
  assert.equal(h.of("play-stopped").length, 0, "an unproven completed stop is refused");
  assert.equal(h.of("play-error").at(-1).replayComplete, false);
  assert.equal(h.of("play-error").at(-1).released, true);
  assert.equal(h.games[0].stops, 1);
  assert.equal(h.games[0].frees, 1);
});

test("selected speed preserves explicit policy and original opponent bytes and respects binding condition refusal", async () => {
  const policy = { presetId: BMS_TIMING_PRESET_ID, rankPrecedence: "defexrank-first", gauge: "hard" };
  const bytes = Uint8Array.from([66, 75, 82, 19, 255]);
  const FileType = selectedFile("record.bkr").file.constructor;
  const selected = { file: new FileType([bytes], "record.bkr") };
  for (const savedAdmissionError of [undefined, "recorded source speed differs"]) {
    const h = await catalog({ savedAdmissionError });
    await h.send(request({ liveSpeed: "3/2", timingPolicy: policy, recordReplay: true,
      timing: { earlyNs: 11n, lateNs: 22n, offsetNs: -37n },
      opponents: [{ file: selected.file, sourceKey: "file:speed", own: true, label: "past play" }] }));
    assert.equal(h.libraries[0].policyPreparations.length, 1);
    assert.deepEqual(h.games[0].prepared.timingPolicy, { ...policy, offsetNs: -37n });
    assert.equal(h.liveSpeedConstructions[0].fraction, "3/2");
    if (savedAdmissionError) {
      assert.equal(prepared(h), undefined);
      assert.match(h.of("play-error").at(-1).message, /recorded source speed differs/);
      assert.equal(h.games[0].calls.some(row => row[0] === "capture"), false);
      assert.equal(h.games[0].frees, 1);
    } else {
      assert.equal(prepared(h)?.opponentCount, 1);
      const row = h.games[0].calls.find(row => row[0] === "add-opponent");
      assert.deepEqual(Array.from(row[1]), Array.from(bytes));
      assert.ok(h.games[0].calls.indexOf(row) < h.games[0].calls.findIndex(row => row[0] === "capture"));
      await stop(h);
    }
  }
});
