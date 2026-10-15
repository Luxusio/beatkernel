// Coordinator runs: node --experimental-vm-modules --test app/web/live-speed-host.test.mjs
// Actual main.js and shared numeric modules; controlled DOM/Worker/audio edges.
// These fixtures do not claim actual WASM, Worklet or browser acceptance.
import assert from "node:assert/strict";
import { File } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";

// Reuse the existing host harness, without registering its unrelated tests or
// copying production code. Only the fake DOM and NORMAL Worker reply change.
const fixtureUrl = new URL("./play-host.test.mjs", import.meta.url);
const source = await readFile(fixtureUrl, "utf8");
function between(start, end) {
  assert.equal(source.split(start).length, 2, `unique fixture start: ${start}`);
  assert.equal(source.split(end).length, 2, `unique fixture end: ${end}`);
  const first = source.indexOf(start), last = source.indexOf(end, first);
  assert.ok(last > first);
  return source.slice(first, last);
}
let fixture = between('// Deferred: node --experimental-vm-modules', 'function speedPrepared(')
  + between('function savedRecord(fields = {}) {', 'function portableSettings() {')
  + between('async function harness(faults = {}) {', 'test("Window record RPC: immutable capture')
  + between('async function localCount(h, count) {', 'async function pagedTouchSession() {');
fixture = fixture.replaceAll("import.meta.url", JSON.stringify(fixtureUrl.href));
fixture = fixture.replace(/from "(\.\/[^\"]+)"/g, (_, path) => `from ${JSON.stringify(new URL(path, fixtureUrl).href)}`);
fixture += '\nexport { harness, deferred, flush, finalScore, selectedRecording, chooseRecording, localCount, localAssign, localFinal };\n';
const { harness, deferred, flush, finalScore, selectedRecording, chooseRecording, localCount, localAssign, localFinal } =
  await import(`data:text/javascript;base64,${Buffer.from(fixture).toString("base64")}`);

function prepared(fraction = "1/1", numerator = 1n, denominator = 1n, endFrame = 100800n, fields = {}) {
  return { kind: "prepared", mode: "live", title: "Live fixture", artist: "Fixture",
    notes: 1, samples: 0, lanes: [0x11], opponentCount: 0, startNs: 0n, endNs: 2000000000n,
    endFrame, liveSpeed: fraction, liveSpeedNumerator: numerator, liveSpeedDenominator: denominator, ...fields };
}
async function admit(h, start, metadata) {
  const worker = h.workers[0];
  await h.reply(start, metadata);
  const samples = worker.last("play-samples-upload"); assert.ok(samples);
  await h.admitSamples(samples, 0);
  await h.reply(samples, { kind: "samples-uploaded", count: 0, bytes: 0 });
  await h.reply(worker.last("play-audio"), null);
  await h.reply(worker.last("play-activate"), null);
  await h.geometry({ playId: start.playId });
}
async function stop(h, start) {
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
}

test("valid MAX/1 syntax with unrepresentable live preroll refuses before AudioContext and Worker preparation", async () => {
  const h = await harness();
  try {
    await h.preview();
    let reads = 0;
    for (const file of h.get("files").files) file.arrayBuffer = () => { reads++; throw new Error("cold refusal must not acquire files"); };
    h.get("live-speed").value = "9223372036854775807/1";
    h.get("live-start").value = "0";
    const worker = h.workers[0], posts = worker.posts.length;
    assert.equal(await h.begin(), undefined);
    assert.equal(h.opens.length, 0); assert.equal(reads, 0);
    assert.equal(worker.messages("play-start").length, 0);
    assert.equal(worker.posts.length, posts, "refusal precedes Worker setup or history ownership effects");
    assert.equal(h.get("status").dataset.error, "true");
    assert.match(h.get("status").textContent, /preroll|phase|signed|represent/i);
    assert.equal(h.get("live-speed").focuses, 1);
    assert.equal(h.get("live-speed").disabled, false);
    h.get("live-speed").value = "1/1";
    const retry = await h.launch(); assert.equal(retry.start.liveSpeed, "1/1");
    await stop(h, retry.start);
  } finally { await h.close(); }
});

test("fixed preroll validates actual original start and freezes it across the AudioHost await", async () => {
  const gate = deferred(), h = await harness({ openGate: gate });
  try {
    await h.preview(); h.get("live-speed").value = "92233720369/1";
    h.get("live-start").value = "0";
    assert.equal(await h.begin(), undefined); assert.equal(h.opens.length, 0);
    h.get("live-start").value = "0.1";
    h.click("play"); await flush();
    assert.equal(h.opens.length, 1, "a representable phase is admitted despite the huge numerator");
    h.get("live-start").value = "0"; h.get("live-speed").value = "9223372036854775807/1";
    gate.resolve(h.audio); await flush();
    const start = h.workers[0].last("play-start"); assert.ok(start);
    assert.equal(start.startNs, 100000000n); assert.equal(start.liveSpeed, "92233720369/1");
    await admit(h, start, prepared(start.liveSpeed, 92233720369n, 1n, undefined,
      { startNs: 100000000n, endNs: undefined, endFrame: undefined }));
    assert.deepEqual(h.audio.finishArgs, [[]]); await stop(h, start);
  } finally { gate.resolve(h.audio); await h.close(); }
});

test("an underflowing live draft leaves independent replay selection available", async () => {
  const h = await harness();
  try {
    await h.preview(); h.get("live-speed").value = "9223372036854775807/1";
    h.get("replay-speed").value = "4/2";
    chooseRecording(h, [selectedRecording().file]);
    const start = await h.begin("replay"); assert.ok(start);
    assert.equal(start.replaySpeed, "2/1"); assert.equal(Object.hasOwn(start, "liveSpeed"), false);
    assert.equal(h.opens.length, 1); assert.equal(h.get("live-speed").focuses, 0);
    await stop(h, start);
  } finally { await h.close(); }
});

test("initial live speed markup exposes labelled bounded default and separate pitch hints", async () => {
  const html = await readFile(new URL("./index.html", import.meta.url), "utf8");
  const label = html.match(/<label\b[^>]*for="live-speed"[^>]*>([^<]*)(?=<input\b|<\/label>)/)?.[1];
  assert.ok(label); assert.match(label, /\binitial\b/i); assert.match(label, /\blive speed\b/i);
  assert.match(label, /\(NUM\/DEN\)/);
  const field = html.match(/<input\b[^>]*id="live-speed"[^>]*>/)?.[0]; assert.ok(field);
  assert.match(field, /value="1\/1"/); assert.match(field, /maxlength="40"/);
  assert.match(field, /aria-describedby="live-speed-hint"/);
  assert.match(html, /id="live-speed-control"/); assert.match(html, /id="replay-speed-control"/);
  assert.match(html, /id="live-speed-hint"[^>]*>[\s\S]*?pitch/i);
});

for (const invalid of ["", "2", "0/1", "1/0", "-1/2", "1.5/1", " 1/1", "1/1 ",
  "1/1/1", "9223372036854775808/1", "1/18446744073709551616", "0".repeat(40) + "1/1"]) {
  test(`live speed ${JSON.stringify(invalid)} refuses before audio and focuses its draft`, async () => {
    const h = await harness();
    try {
      await h.preview(); h.get("live-speed").value = invalid;
      assert.equal(await h.begin(), undefined);
      assert.equal(h.opens.length, 0); assert.equal(h.workers[0].messages("play-start").length, 0);
      assert.equal(h.get("status").dataset.error, "true"); assert.equal(h.get("live-speed").focuses, 1);
      assert.equal(h.get("live-speed").disabled, false);
    } finally { await h.close(); }
  });
}

// Literal 2-second original section + unchanged 100 ms preroll at 48000 Hz.
for (const [draft, canonical, numerator, denominator, endFrame] of [
  ["01/02", "1/2", 1n, 2n, 196800n], ["2/2", "1/1", 1n, 1n, 100800n],
  ["4/2", "2/1", 2n, 1n, 52800n], ["003/002", "3/2", 3n, 2n, 68800n],
]) {
  test(`live ${draft} owns normalized snapshot, finite fence and once cleanup`, async () => {
    const h = await harness();
    try {
      await h.preview(); h.get("live-speed").value = draft; h.get("live-end").value = "2";
      const start = await h.begin(); assert.equal(start.liveSpeed, canonical);
      assert.equal(Object.hasOwn(start, "replaySpeed"), false);
      assert.equal(h.get("live-speed").disabled, true);
      assert.equal(h.get("replay-speed-control").hidden, true);
      assert.equal(h.get("live-speed-control").hidden, false);
      h.get("live-speed").value = "9/1";
      await admit(h, start, prepared(canonical, numerator, denominator, endFrame));
      assert.deepEqual(h.audio.finishArgs, [[endFrame]]); assert.equal(h.audio.arms.length, 1);
      assert.match(h.get("status").textContent, new RegExp(canonical));
      assert.match(h.get("details").textContent, new RegExp(canonical));
      assert.equal(h.get("live-speed").disabled, true);
      const audio = h.audio;
      h.click("stop"); h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
      assert.equal(audio.stopStarts, 1); assert.equal(h.workers[0].messages("play-stop").length, 1);
      assert.equal(h.get("live-speed").disabled, false);
      assert.equal(h.get("replay-speed-control").hidden, false);
      assert.equal(h.get("live-speed-control").hidden, false);
      h.get("live-speed").value = "1/1"; h.get("live-end").value = "";
      const retry = await h.launch(); assert.ok(retry.id > start.playId);
      assert.equal(retry.start.liveSpeed, "1/1"); assert.deepEqual(h.audio.finishArgs, [[]]);
      await stop(h, retry.start);
    } finally { await h.close(); }
  });
}

for (const fault of ["missing", "partial", "fraction", "unreduced", "numerator", "denominator", "number", "grid", "number-frame"]) {
  test(`live ${fault} prepared identity cannot upload samples or arm output`, async () => {
    const h = await harness();
    try {
      await h.preview(); h.get("live-speed").value = "3/2"; h.get("live-end").value = "2";
      const start = await h.begin(), worker = h.workers[0];
      const metadata = prepared("3/2", 3n, 2n, 68800n);
      if (fault === "missing") for (const name of ["liveSpeed", "liveSpeedNumerator", "liveSpeedDenominator"]) delete metadata[name];
      if (fault === "partial") delete metadata.liveSpeedDenominator;
      if (fault === "fraction") metadata.liveSpeed = "2/1";
      if (fault === "unreduced") metadata.liveSpeed = "6/4";
      if (fault === "numerator") metadata.liveSpeedNumerator = 2n;
      if (fault === "denominator") metadata.liveSpeedDenominator = 3n;
      if (fault === "number") metadata.liveSpeedNumerator = 3;
      if (fault === "grid") metadata.endFrame = 100800n;
      if (fault === "number-frame") metadata.endFrame = 68800;
      await h.reply(start, metadata);
      assert.equal(worker.messages("play-samples-upload").length, 0);
      assert.deepEqual(h.audio.arms, []); assert.deepEqual(h.audio.finishArgs, []);
      assert.equal(h.audio.stopStarts, 1); assert.equal(worker.last("play-stop").playId, start.playId);
      await h.receive(finalScore(start.playId)); assert.equal(h.get("status").dataset.error, "true");
      assert.equal(h.get("live-speed").disabled, false); assert.equal(h.get("play").disabled, false);
    } finally { await h.close(); }
  });
}

test("live draft freezes before held AudioHost await and stale prepared cannot replace retry", async () => {
  const gate = deferred(), h = await harness({ openGate: gate });
  try {
    await h.preview(); h.get("live-speed").value = "4/2"; h.get("live-end").value = "2";
    h.click("play"); await flush(); assert.equal(h.opens.length, 1);
    assert.equal(h.get("live-speed").disabled, true); h.get("live-speed").value = "1/2";
    gate.resolve(h.audio); await flush();
    const worker = h.workers[0], old = worker.last("play-start"); assert.equal(old.liveSpeed, "2/1");
    await stop(h, old); delete h.faults.openGate;
    const retry = await h.begin(); assert.equal(retry.liveSpeed, "1/2");
    await h.reply(old, prepared("2/1", 2n, 1n, 52800n));
    assert.equal(worker.messages("play-samples-upload").length, 0);
    await admit(h, retry, prepared("1/2", 1n, 2n, 196800n)); await stop(h, retry);
  } finally { gate.resolve(h.audio); await h.close(); }
});

test("live and replay drafts validate independently and preserve default live payload", async () => {
  const h = await harness();
  try {
    await h.preview(); h.get("replay-speed").value = "invalid";
    const live = await h.launch(); assert.equal(live.start.liveSpeed, "1/1");
    assert.equal(Object.hasOwn(live.start, "replaySpeed"), false); await stop(h, live.start);
    h.get("live-speed").value = "invalid"; h.get("replay-speed").value = "4/2";
    chooseRecording(h, [selectedRecording().file]); const replay = await h.begin("replay");
    assert.equal(replay.replaySpeed, "2/1"); assert.equal(Object.hasOwn(replay, "liveSpeed"), false);
    assert.equal(h.get("live-speed-control").hidden, true); assert.equal(h.get("replay-speed-control").hidden, false);
    await stop(h, replay);
  } finally { await h.close(); }
});

test("maximum exact fraction components pass as BigInt with a literal one-frame song span", async () => {
  const h = await harness();
  try {
    await h.preview(); h.get("live-speed").value = "9223372036854775807/18446744073709551615";
    h.get("live-end").value = "0.000000001";
    const start = await h.begin(); assert.equal(start.liveSpeed, "9223372036854775807/18446744073709551615");
    await admit(h, start, prepared(start.liveSpeed, 9223372036854775807n, 18446744073709551615n, 4801n, { endNs: 1n }));
    assert.deepEqual(h.audio.finishArgs, [[4801n]]); await stop(h, start);
  } finally { await h.close(); }
});

test("selected live speed leaves keyboard acquisition bytes and render ownership untouched", async () => {
  const h = await harness();
  try {
    await h.preview(); h.get("live-speed").value = "2/1"; h.get("live-end").value = "2";
    const start = await h.begin(); await admit(h, start, prepared("2/1", 2n, 1n, 52800n));
    const worker = h.workers[0], renders = worker.messages("play-render").length, reads = h.layoutReads;
    h.setNow(1300.125); h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.125 }); await flush();
    const step = worker.last("play-step"); assert.ok(step);
    assert.deepEqual(step.events, [{ hostNs: 1300125000n, key: 2, down: true, sequence: 1n }]);
    assert.equal(worker.messages("play-render").length, renders); assert.equal(h.layoutReads, reads);
    assert.equal(h.audio.polls, 0); assert.deepEqual(h.audio.commandsSeen, []); await stop(h, start);
  } finally { await h.close(); }
});

test("live rate preserves original touch and HID reports beside keyboard without rendering work", async () => {
  const h = await harness({ hidSupported: true, touchSupported: true, hidAdmittedSources: [3n] });
  try {
    await h.preview(); h.get("live-speed").value = "3/2"; h.get("live-end").value = "2";
    const profile = new File(['{"version":1}'], "controller-profile.json");
    let reads = 0; profile.arrayBuffer = () => { reads++; throw new Error("Window must not acquire profile bytes"); };
    h.get("hid-profile").files = [profile]; h.get("hid-profile").emit("change");
    const start = await h.begin(); assert.equal(start.liveSpeed, "3/2"); assert.equal(start.inputMode, "physical-contact");
    assert.equal(start.hidProfileFile, profile);
    await admit(h, start, prepared("3/2", 3n, 2n, 68800n, { hidSources: [3n], hidSourceCount: 1 }));
    const worker = h.workers[0], renders = worker.messages("play-render").length, layout = h.layoutReads;
    h.setNow(1300.5);
    const bytes = Uint8Array.from([99, 7, 255, 0, 88]), device = h.hidDevices[0];
    device.emit("inputreport", { device, reportId: 7, timeStamp: 1300.125, data: new DataView(bytes.buffer, 1, 3) });
    const first = worker.last("play-step"); assert.ok(first);
    assert.deepEqual(first.events, [{ kind: "hid", hostNs: 1300125000n, source: 3n, sequence: 1n,
      reportId: 7, data: Uint8Array.from([7, 255, 0]) }]);
    bytes.fill(0); assert.deepEqual(Array.from(first.events[0].data), [7, 255, 0]);
    h.get("canvas").emit("pointerdown", { pointerType: "touch", pointerId: 1, timeStamp: 1300.25,
      offsetX: 120, offsetY: 180, pressure: 0.5 });
    h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.5 });
    await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId,
      tickId: first.tickId, songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
    const mixed = worker.last("play-step");
    assert.deepEqual(mixed.events.map(event => event.sequence), [2n, 3n]);
    assert.deepEqual(mixed.events.map(event => event.hostNs), [1300250000n, 1300500000n]);
    assert.deepEqual(mixed.events[0], { kind: "touch", hostNs: 1300250000n, sequence: 2n, contact: 1n,
      phase: 0, code: 1, x: 120, y: 180, pressure: 0.5, width: 960, height: 720, surfaceWidth: 960, surfaceHeight: 720 });
    assert.deepEqual(mixed.events[1], { hostNs: 1300500000n, key: 2, down: true, sequence: 3n });
    assert.equal(worker.messages("play-render").length, renders); assert.equal(h.layoutReads, layout);
    assert.equal(h.audio.polls, 0); assert.equal(reads, 0); await stop(h, start);
    assert.ok(h.hidDevices.every(native => native.closes === 1));
  } finally { await h.close(); }
});

test("two local members share one canonical live rate and one finite output fence", async () => {
  const h = await harness({ touchSupported: true });
  try {
    await h.preview(); await localCount(h, 2); h.click("local-discover"); await flush();
    localAssign(h, 1, 1n); localAssign(h, 2, 2n);
    h.get("live-speed").value = "6/4"; h.get("live-end").value = "2";
    const start = await h.begin(); assert.equal(start.liveSpeed, "3/2");
    assert.deepEqual(Array.from(start.localPlanWords), [1, 1, 1, 0, 2, 1, 2, 0]);
    await admit(h, start, prepared("3/2", 3n, 2n, 68800n, { localPlayers: [1, 2], localPage: 0 }));
    assert.equal(h.opens.length, 1); assert.deepEqual(h.audio.finishArgs, [[68800n]]);
    assert.equal(h.audio.arms.length, 1); assert.match(h.get("details").textContent, /3\/2.*2 local players/);
    h.click("stop"); await flush(); await h.receive(localFinal(start));
    assert.equal(h.get("live-speed").disabled, false);
  } finally { await h.close(); }
});
