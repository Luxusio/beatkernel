// Deferred: node --experimental-vm-modules --test app/web/audio-host.test.mjs
// The real host module runs against controlled WebAudio endpoints, never an audio device.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule } from "node:vm";

const sourceUrl = new URL("./audio-host.mjs", import.meta.url);
const source = await readFile(sourceUrl, "utf8");
const failureUrl = new URL("./audio-failure.mjs", import.meta.url);
const failureSource = await readFile(failureUrl, "utf8");
const clockUrl = new URL("./native-audio-clock.mjs", import.meta.url);
const clockSource = await readFile(clockUrl, "utf8");
const nativeProbeUrl = new URL("./worklet-native-frame-probe.mjs", import.meta.url);
const nativeProbeSource = await readFile(nativeProbeUrl, "utf8");
// Independent expected program geometry, intentionally not imported from the adapter.
const TEST_PERIODS = [1048576, 1048575, 1048573];
const TEST_NAMES = ["__beatkernel_clock_marker", "__beatkernel_clock_0", "__beatkernel_clock_1", "__beatkernel_clock_2"];

// The clock peer here supplies control packets only. These tests establish Host
// scheduling and ownership, never native precision, WASM arithmetic or PCM.
function expectedInitial(origin, capacity, rate) {
  const smallest = Math.min(...TEST_PERIODS);
  const lowWater = Math.max(smallest, 4 * capacity);
  const cycles = Math.ceil((lowWater + 2 * capacity) / smallest) + 1;
  return { origin, h0: origin + cycles * TEST_PERIODS[0],
    h1: origin + cycles * TEST_PERIODS[1], h2: origin + cycles * TEST_PERIODS[2],
    cycles, lowWater, event: frame => (frame - 0.25) / rate };
}
async function beginNativeOpen(h, configuration = options({ audioLimits: { maxFrames: 4096 } }), quantum = 128) {
  const opening = observe(() => h.AudioHost.open(configuration));
  await flush();
  h.ready();
  h.clockPacket("clock-probe", { frames: quantum });
  await flush();
  assert.equal(h.clockSent.length, 1, "initial native geometry must first be reserved");
  const reserve = h.clockSent[0].message;
  assert.equal(reserve.kind, "clock-reserve");
  assert.equal(h.nativeCalls, 0, "no automation may precede reserve admission");
  assert.equal(opening.settled, false, "constructor ready and quantum alone cannot expose a player");
  return { opening, reserve };
}
async function admitNativeProgram(h, reserve) {
  h.clockReply(reserve);
  await flush();
  assert.equal(h.clockSent.length, 2);
  const commit = h.clockSent[1].message;
  assert.equal(commit.kind, "clock-commit");
  return commit;
}
async function exposeNativeOwner(h, opening, commit) {
  h.clockReply(commit);
  await flush();
  assert.equal(opening.settled, false, "whole commit ACK is not native callback proof");
  h.nativeReady({ revision: 0, origin: commit.origin });
  const opened = await opening.result;
  assert.equal(opened.ok, true, opened.error?.message);
  return opened.value;
}
async function nativeOpeningError(h, opening, code) {
  await flush();
  const stopping = h.sent.find(row => row.message.kind === "stop")?.message;
  if (stopping) h.reply(stopping);
  for (let index = 0; !opening.settled && index < 5; index++) {
    await flush();
    if (!opening.settled) await h.expire();
  }
  const error = failure(await opening.result, code);
  assertClean(h);
  return error;
}
function requestRefill(h, geometry, fields = {}) {
  const end = Math.min(geometry.h0, geometry.h1, geometry.h2) - geometry.lowWater;
  const high = Math.floor(end / 4294967296), low = end - high * 4294967296;
  h.clockPacket("clock-refill", { revision: 1, from0: geometry.h0, from1: geometry.h1,
    from2: geometry.h2, to0: geometry.h0 + TEST_PERIODS[0],
    to1: geometry.h1 + TEST_PERIODS[1], to2: geometry.h2 + TEST_PERIODS[2],
    frames: 128, endLow: low, endHigh: high, ...fields });
}
function nativeEvents(h, name) {
  return h.nodes[0].parameters.get(name).events;
}

test("native open reserves before automation and requires separate native readiness after whole commit", async () => {
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  const origin = Math.ceil((0 + 4 * 128) / 128) * 128 + 64;
  const expected = expectedInitial(origin, 4096, 48000);
  assert.deepEqual(structuredClone(reserve), { kind: "clock-reserve", generation: 17,
    clockVersion: 1, clockId: 1, revision: 0, origin,
    h0: expected.h0, h1: expected.h1, h2: expected.h2 });
  assert.ok(expected.h0 > expected.h1 && expected.h1 > expected.h2, "own horizons intentionally descend");
  const commit = await admitNativeProgram(h, reserve);
  assert.deepEqual(structuredClone(commit), { ...structuredClone(reserve), kind: "clock-commit", futureEligible: true });
  assert.deepEqual(nativeEvents(h, TEST_NAMES[0]), [{ kind: "set", value: 1, at: expected.event(origin) }]);
  for (let lane = 0; lane < 3; lane++) {
    const period = TEST_PERIODS[lane], events = [{ kind: "set", value: 1, at: expected.event(origin) }];
    for (let cycle = 1; cycle <= expected.cycles; cycle++) {
      const at = expected.event(origin + cycle * period);
      events.push({ kind: "ramp", value: period + 1, at }, { kind: "set", value: 1, at });
    }
    assert.deepEqual(nativeEvents(h, TEST_NAMES[lane + 1]), events);
  }
  assert.equal(h.sent.length, 0, "clock exchange must not consume general controls");
  const owner = await exposeNativeOwner(h, opening, commit);
  await acknowledged(h, () => owner.poll());
  assert.equal(h.last().sequence, 1, "the first public operation still owns sequence one");
  await stop(h, owner);
});

test("native startup measures acquisition through ready and quantum delivery instead of adding a fixed delay", async () => {
  for (const arrival of ["ready-first", "probe-first"]) {
    const h = await harness({ autoClock: false });
    const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
    await flush();
    h.contexts[0].currentTime = 0.25;
    if (arrival === "ready-first") h.ready(); else h.clockPacket("clock-probe", { frames: 128 });
    await flush();
    assert.equal(h.clockSent.length, 0, "both independently delivered facts are required");
    h.advanceBeforeDeadline(9);
    if (arrival === "ready-first") h.clockPacket("clock-probe", { frames: 128 }); else h.ready();
    await flush();
    const reserve = h.clockSent[0]?.message;
    assert.equal(reserve?.kind, "clock-reserve");
    const lead = Math.max(4 * 128, Math.ceil(2 * 9 * 48000 / 1000));
    assert.equal(reserve.origin, Math.ceil((Math.ceil(0.25 * 48000) + lead) / 128) * 128 + 64);
    assert.ok((reserve.origin / 48000) - 0.25 < 0.03, "no diagnostic 200ms startup delay");
    const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
    await stop(h, owner);
  }
});

test("maximum callback capacity reserves independently derived eight own periods", async () => {
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h, options({ audioLimits: { maxFrames: 1048576 } }));
  const expected = expectedInitial(reserve.origin, 1048576, 48000);
  assert.equal(expected.cycles, 8);
  for (let lane = 0; lane < 3; lane++) assert.equal(reserve["h" + lane], expected["h" + lane]);
  const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
  assert.equal(h.nativeCalls, 1 + 3 * (1 + 2 * 8));
  await stop(h, owner);
});

test("initial one-frame quantum refuses without scheduling an unauthenticated origin", async () => {
  const h = await harness({ autoClock: false });
  const opening = observe(() => h.AudioHost.open(options()));
  await flush(); h.ready(); h.clockPacket("clock-probe", { frames: 1 });
  await nativeOpeningError(h, opening);
  assert.equal(h.nativeCalls, 0);
  assert.equal(h.clockSent.length, 0);
});

test("constructor ready, reserve ACK and commit ACK cannot stand in for clock-ready", async () => {
  for (const stalled of ["probe", "reserve", "commit", "native-ready"]) {
    const h = await harness({ autoClock: false });
    const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
    await flush(); h.ready();
    if (stalled !== "probe") {
      h.clockPacket("clock-probe", { frames: 128 }); await flush();
      if (stalled !== "reserve") {
        const commit = await admitNativeProgram(h, h.clockSent[0].message);
        if (stalled === "native-ready") h.clockReply(commit);
      }
    }
    await flush(); assert.equal(opening.settled, false);
    await h.expire();
    await nativeOpeningError(h, opening, "timeout");
    assert.equal(h.sent.filter(row => row.message.kind === "stop").length, 1);
  }
});

test("native partial automation failure never publishes a whole commit or reuses the origin", async () => {
  for (const nativeThrowAt of [1, 2, 5, 22]) {
    const h = await harness({ autoClock: false, nativeThrowAt });
    const { opening, reserve } = await beginNativeOpen(h);
    h.clockReply(reserve);
    await nativeOpeningError(h, opening, "transport");
    assert.equal(h.nativeCalls, nativeThrowAt);
    assert.deepEqual(h.clockSent.map(row => row.message.kind), ["clock-reserve"]);
    assert.equal(h.nodes.length, 1);
    assert.ok(h.trace.filter(row => row[0] === "native").every(row => row[2] !== "cancel" && row[2] !== "hold"));
  }
});

test("late initial native program refuses both before writes and after a partially applied transaction", async () => {
  for (const when of ["before", "during"]) {
    const faults = { autoClock: false };
    const h = await harness(faults);
    const { opening, reserve } = await beginNativeOpen(h);
    const deadline = (reserve.origin - 0.25) / 48000;
    if (when === "before") h.contexts[0].currentTime = deadline;
    else faults.nativeAfterCall = ({ call, context }) => { if (call === 1) context.currentTime = deadline; };
    h.clockReply(reserve);
    await nativeOpeningError(h, opening);
    assert.equal(h.clockSent.some(row => row.message.kind === "clock-commit"), false);
    if (when === "before") assert.equal(h.nativeCalls, 0); else assert.ok(h.nativeCalls >= 1);
  }
});

test("missing native parameter and unrepresentable context geometry fail open without raw-clock fallback", async () => {
  for (const faults of [{ autoClock: false, missingParameter: TEST_NAMES[2] }, { autoClock: false }]) {
    const h = await harness(faults);
    const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
    await flush();
    if (!faults.missingParameter) h.contexts[0].currentTime = Number.MAX_SAFE_INTEGER / 48000;
    h.ready(); h.clockPacket("clock-probe", { frames: 128 });
    await flush();
    if (h.clockSent[0]) h.clockReply(h.clockSent[0].message);
    await nativeOpeningError(h, opening);
    assert.equal(h.clockSent.some(row => row.message.kind === "clock-commit"), false);
  }
});

test("native refill appends only new own endpoints and commits the whole geometry independently of public pending", async () => {
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  const commit = await admitNativeProgram(h, reserve);
  const owner = await exposeNativeOwner(h, opening, commit);
  const geometry = expectedInitial(reserve.origin, 4096, 48000);
  const eventCounts = TEST_NAMES.map(name => nativeEvents(h, name).length);
  const pending = observe(() => owner.poll()), publicRequest = h.last();
  requestRefill(h, geometry);
  await flush();
  const extended = h.clockSent.at(-1).message;
  assert.equal(extended.kind, "clock-commit"); assert.equal(extended.revision, 1);
  assert.equal(extended.origin, reserve.origin); assert.equal(extended.futureEligible, true);
  assert.equal(pending.settled, false, "native request cannot settle public pending");
  assert.equal(h.sent.length, 1);
  assert.equal(nativeEvents(h, TEST_NAMES[0]).length, eventCounts[0], "single fresh marker is never rewritten");
  for (let lane = 0; lane < 3; lane++) {
    const oldH = geometry["h" + lane], nextH = oldH + TEST_PERIODS[lane];
    assert.equal(extended["h" + lane], nextH);
    const appended = nativeEvents(h, TEST_NAMES[lane + 1]).slice(eventCounts[lane + 1]);
    assert.deepEqual(appended, [{ kind: "ramp", value: TEST_PERIODS[lane] + 1, at: geometry.event(nextH) },
      { kind: "set", value: 1, at: geometry.event(nextH) }]);
    assert.equal(appended.some(event => event.at === geometry.event(oldH)), false, "no duplicate OLD reset");
  }
  h.reply(publicRequest);
  const polled = await pending.result;
  assert.equal(polled.ok, true); assert.equal(polled.value.words.length, 58);
  h.clockReply(extended); await flush();
  await acknowledged(h, () => owner.finish());
  assert.equal(h.last().sequence, 2);
  await stop(h, owner);
});

test("refill transaction must finish before every OLD horizon, even though all new endpoints remain future", async () => {
  for (const when of ["before", "during"]) {
    const faults = { autoClock: false };
    const h = await harness(faults);
    const { opening, reserve } = await beginNativeOpen(h);
    const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
    const geometry = expectedInitial(reserve.origin, 4096, 48000);
    const before = h.nativeCalls, commits = h.clockSent.length;
    const oldDeadline = geometry.event(Math.min(geometry.h0, geometry.h1, geometry.h2));
    assert.ok(oldDeadline < geometry.event(geometry.h2 + TEST_PERIODS[2]));
    if (when === "before") h.contexts[0].currentTime = oldDeadline;
    else faults.nativeAfterCall = ({ call, context }) => { if (call === before + 1) context.currentTime = oldDeadline; };
    requestRefill(h, geometry); await flush();
    await localError(h, () => owner.poll(), "state");
    assert.equal(h.clockSent.length, commits, "failed partial transaction must publish no new commit");
    if (when === "before") assert.equal(h.nativeCalls, before);
    await stop(h, owner, "failed");
  }
});

test("a throw in any refill lane fences the generation and cannot commit a surviving prefix", async () => {
  for (const offset of [1, 3, 6]) {
    const faults = { autoClock: false };
    const h = await harness(faults);
    const { opening, reserve } = await beginNativeOpen(h);
    const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
    const before = h.nativeCalls, commits = h.clockSent.length;
    faults.nativeThrowAt = before + offset;
    requestRefill(h, expectedInitial(reserve.origin, 4096, 48000)); await flush();
    const original = await localError(h, () => owner.poll(), "transport");
    assert.equal(h.nativeCalls, before + offset); assert.equal(h.clockSent.length, commits);
    assert.equal(failure(await observe(() => owner.poll()).result), original);
    await stop(h, owner, "failed");
  }
});

test("successive refills advance each committed own horizon by exactly one period", async () => {
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
  const geometry = expectedInitial(reserve.origin, 4096, 48000);
  for (let revision = 1; revision <= 3; revision++) {
    requestRefill(h, geometry, { revision }); await flush();
    const extended = h.clockSent.at(-1).message;
    assert.equal(extended.kind, "clock-commit"); assert.equal(extended.revision, revision);
    for (let lane = 0; lane < 3; lane++) {
      assert.equal(extended["h" + lane], geometry["h" + lane] + TEST_PERIODS[lane]);
      geometry["h" + lane] = extended["h" + lane];
    }
    h.clockReply(extended); await flush();
  }
  await stop(h, owner);
});

test("stale clock generation ignores hostile nested fields without affecting pending operations", async () => {
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  let reads = 0;
  const stale = { kind: "clock-reserved", generation: 16 };
  for (const name of ["clockVersion", "clockId", "revision", "status", "error"]) Object.defineProperty(stale, name, {
    get() { reads++; throw new Error("stale field accessed"); } });
  h.nodes[0].port.emit("message", stale); await flush();
  assert.equal(reads, 0); assert.equal(h.nativeCalls, 0); assert.equal(opening.settled, false);
  const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
  const pending = observe(() => owner.poll()), sent = h.last();
  const count = h.clockSent.length, calls = h.nativeCalls;
  h.nodes[0].port.emit("message", { ...stale, kind: "clock-refill" }); await flush();
  assert.equal(pending.settled, false); assert.equal(h.clockSent.length, count); assert.equal(h.nativeCalls, calls);
  h.reply(sent); assert.equal((await pending.result).ok, true);
  await stop(h, owner);
});

test("current malformed clock ACK correlation and premature readiness fence opening before native writes", async () => {
  for (const fields of [{ clockVersion: 2 }, { clockId: 2 }, { revision: 1 }, { status: "0" },
    { status: 0, error: "success with error" }, { kind: "clock-committed" }, { generation: "17" }]) {
    const h = await harness({ autoClock: false });
    const { opening, reserve } = await beginNativeOpen(h);
    h.clockReply(reserve, fields);
    await nativeOpeningError(h, opening, "protocol");
    assert.equal(h.nativeCalls, 0);
  }
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  h.clockPacket("clock-ready", { revision: 0, origin: reserve.origin });
  await nativeOpeningError(h, opening, "protocol");
  assert.equal(h.nativeCalls, 0);
});

test("malformed current refill geometry, revision and duplicate requests refuse before any new automation", async () => {
  for (const fields of [{ revision: 0 }, { revision: 2 }, { clockVersion: 2 }, { clockId: 2 },
    { from0: 0 }, { to1: 0 }, { frames: 0 }, { frames: 4097 }, { endLow: -1 }, { endHigh: "0" },
    { endHigh: 0x200000 }, { endLow: 0, endHigh: 0 }]) {
    const h = await harness({ autoClock: false });
    const { opening, reserve } = await beginNativeOpen(h);
    const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
    const before = h.nativeCalls;
    requestRefill(h, expectedInitial(reserve.origin, 4096, 48000), fields); await flush();
    await localError(h, () => owner.poll(), "protocol");
    assert.equal(h.nativeCalls, before);
    await stop(h, owner, "failed");
  }
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
  const geometry = expectedInitial(reserve.origin, 4096, 48000);
  requestRefill(h, geometry); await flush();
  const before = h.nativeCalls;
  requestRefill(h, geometry); await flush();
  await localError(h, () => owner.poll(), "protocol");
  assert.equal(h.nativeCalls, before);
  await stop(h, owner, "failed");
});

test("refill timeout rejects concurrent public work and late clock ACK cannot revive stopped authority", async () => {
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
  requestRefill(h, expectedInitial(reserve.origin, 4096, 48000)); await flush();
  const extended = h.clockSent.at(-1).message;
  const pending = observe(() => owner.poll());
  await h.expire();
  const original = failure(await pending.result, "timeout");
  h.clockReply(extended); await flush();
  assert.equal(failure(await observe(() => owner.poll()).result), original);
  await stop(h, owner, "failed");
  assertClean(h);
});

test("stop cancels native and arm pending independently and tolerates their valid late replies", async () => {
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
  await acknowledged(h, () => owner.finish());
  const arm = observe(() => owner.arm(10000n)), armRequest = h.last();
  requestRefill(h, expectedInitial(reserve.origin, 4096, 48000)); await flush();
  const extended = h.clockSent.at(-1).message;
  const stopping = observe(() => owner.stop()), stopped = h.last();
  failure(await arm.result, "closed");
  h.clockReply(extended); h.reply(armRequest); await flush();
  assert.equal(stopping.settled, false);
  h.reply(stopped); assert.equal((await stopping.result).ok, true);
  assert.equal(owner.state, "closed"); assertClean(h);
});

test("abort during native reserve or commit cancels opening and valid late replies never reacquire output", async () => {
  for (const phase of ["reserve", "commit"]) {
    const h = await harness({ autoClock: false }), controller = new AbortController();
    const { opening, reserve } = await beginNativeOpen(h, options({ signal: controller.signal,
      audioLimits: { maxFrames: 4096 } }));
    const pending = phase === "reserve" ? reserve : await admitNativeProgram(h, reserve);
    controller.abort(); await flush();
    const calls = h.nativeCalls;
    h.clockReply(pending); h.clockPacket("clock-ready", { revision: 0, origin: reserve.origin });
    await nativeOpeningError(h, opening, "aborted");
    assert.equal(h.nativeCalls, calls); assert.equal(h.nodes.length, 1);
  }
});

test("current clock getter reentrancy cannot schedule after abort or overwrite its first cause", async () => {
  const h = await harness({ autoClock: false }), controller = new AbortController();
  const { opening, reserve } = await beginNativeOpen(h, options({ signal: controller.signal,
    audioLimits: { maxFrames: 4096 } }));
  let reads = 0;
  const packet = { kind: "clock-reserved", generation: 17, clockVersion: 1, clockId: 1, status: 0, error: null };
  Object.defineProperty(packet, "revision", { get() { reads++; controller.abort(); return reserve.revision; } });
  h.nodes[0].port.emit("message", packet);
  await nativeOpeningError(h, opening, "aborted");
  assert.equal(reads, 1); assert.equal(h.nativeCalls, 0);
});

test("clock control posting failure clears its independent deadline and joins ordinary cleanup", async () => {
  for (const clockPostThrowsAt of [1, 2]) {
    const h = await harness({ autoClock: false, clockPostThrowsAt });
    const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
    await flush(); h.ready(); h.clockPacket("clock-probe", { frames: 128 }); await flush();
    if (clockPostThrowsAt === 2) h.clockReply(h.clockSent[0].message);
    await nativeOpeningError(h, opening, "transport");
    assert.equal(h.clockSent.some(row => row.message.kind === "clock-commit"), false);
  }
});

test("malformed initial native quantum rejects without reserving or programming lanes", async () => {
  for (const frames of [0, -1, 1.5, NaN, Infinity, "128", 4097, 4294967296]) {
    const h = await harness({ autoClock: false });
    const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
    await flush(); h.ready(); h.clockPacket("clock-probe", { frames });
    await nativeOpeningError(h, opening, "protocol");
    assert.equal(h.nativeCalls, 0); assert.equal(h.clockSent.length, 0);
  }
});

test("current native ready identity, origin and revision must match the complete reserved program", async () => {
  for (const fields of [{ origin: 0 }, { origin: 577 }, { revision: 1 }, { clockVersion: 2 },
    { clockId: 0 }, { origin: 576n }]) {
    const h = await harness({ autoClock: false });
    const { opening, reserve } = await beginNativeOpen(h);
    const commit = await admitNativeProgram(h, reserve);
    h.clockReply(commit); await flush();
    h.clockPacket("clock-ready", { revision: 0, origin: reserve.origin, ...fields });
    await nativeOpeningError(h, opening, "protocol");
  }
});

test("negative native reserve or commit ACK preserves its numeric remote refusal without exposing an owner", async () => {
  for (const phase of ["reserve", "commit"]) {
    const h = await harness({ autoClock: false });
    const { opening, reserve } = await beginNativeOpen(h);
    const pending = phase === "reserve" ? reserve : await admitNativeProgram(h, reserve);
    const before = h.nativeCalls;
    h.clockReply(pending, { status: 6, error: "chronology" });
    const original = await nativeOpeningError(h, opening, "remote");
    assert.equal(original.status, 6);
    assert.equal(h.nativeCalls, before); assert.equal(h.nodes.length, 1);
  }
});

test("duplicate ready or quantum after native admission fences rather than replacing clock authority", async () => {
  for (const kind of ["clock-ready", "clock-probe"]) {
    const h = await harness({ autoClock: false });
    const { opening, reserve } = await beginNativeOpen(h);
    const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
    const before = h.nativeCalls;
    h.clockPacket(kind, { revision: 0, origin: reserve.origin, frames: 128 }); await flush();
    await localError(h, () => owner.poll(), "protocol");
    assert.equal(h.nativeCalls, before);
    await stop(h, owner, "failed");
  }
});

test("future proof is checked after all native automation and before whole commit", async () => {
  const faults = { autoClock: false }, h = await harness(faults);
  const { opening, reserve } = await beginNativeOpen(h);
  const total = 1 + 3 * (1 + 2 * 3);
  faults.nativeAfterCall = ({ call, context }) => {
    if (call === total) context.currentTime = (reserve.origin - 0.25) / 48000;
  };
  h.clockReply(reserve);
  await nativeOpeningError(h, opening);
  assert.equal(h.nativeCalls, total);
  assert.deepEqual(h.clockSent.map(row => row.message.kind), ["clock-reserve"]);
});

test("duplicate current native ACK after completion fences and never schedules another program", async () => {
  for (const phase of ["reserve", "commit"]) {
    const h = await harness({ autoClock: false });
    const { opening, reserve } = await beginNativeOpen(h);
    const commit = await admitNativeProgram(h, reserve);
    const owner = await exposeNativeOwner(h, opening, commit);
    const before = h.nativeCalls;
    h.clockReply(phase === "reserve" ? reserve : commit); await flush();
    await localError(h, () => owner.poll(), "protocol");
    assert.equal(h.nativeCalls, before);
    await stop(h, owner, "failed");
  }
});

test("hostile current native metadata getter fails once and retains the original protocol error", async () => {
  const h = await harness({ autoClock: false });
  const { opening } = await beginNativeOpen(h);
  let reads = 0;
  const packet = { kind: "clock-reserved", generation: 17, clockVersion: 1, clockId: 1,
    revision: 0, error: null };
  const cause = new Error("clock metadata failed");
  Object.defineProperty(packet, "status", { get() { reads++; throw cause; } });
  h.nodes[0].port.emit("message", packet);
  const original = await nativeOpeningError(h, opening, "protocol");
  assert.equal(reads, 1); assert.equal(original.cause, cause); assert.equal(h.nativeCalls, 0);
});

test("abort after native commit waits for the actual stop ACK and keeps its original cause without cleanup recursion", async () => {
  const h = await harness({ autoClock: false }), controller = new AbortController();
  const { opening, reserve } = await beginNativeOpen(h, options({ signal: controller.signal,
    audioLimits: { maxFrames: 4096 } }));
  const commit = await admitNativeProgram(h, reserve);
  assert.equal(commit.kind, "clock-commit");
  controller.abort(); await flush();
  const stopRequest = h.last(), calls = h.nativeCalls;
  assert.equal(stopRequest.kind, "stop");
  assert.equal(opening.settled, false, "abort must still join the actual stop ACK");
  assert.equal(h.contexts[0].closes, 0, "native output cannot be released before stop ACK");
  h.clockReply(commit);
  h.clockPacket("clock-ready", { revision: 0, origin: reserve.origin });
  await flush();
  assert.equal(opening.settled, false, "late clock replies cannot retire the fresh stop request");
  assert.equal(h.contexts[0].closes, 0);
  assert.equal(h.nativeCalls, calls);
  assert.ok(h.timers.size > 0, "the bounded stop acknowledgement remains live");
  h.reply(stopRequest);
  const original = failure(await opening.result, "aborted");
  assert.ok(original.cleanupError == null, "a successful stop must not attach the original abort as cleanupError");
  assertClean(h);
});

test("stop during refill preserves its actual ACK across late commit and legitimate next-revision refill", async () => {
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
  const geometry = expectedInitial(reserve.origin, 4096, 48000);
  requestRefill(h, geometry); await flush();
  const refillCommit = h.clockSent.at(-1).message;
  assert.equal(refillCommit.kind, "clock-commit"); assert.equal(refillCommit.revision, 1);
  const stopping = observe(() => owner.stop()), stopRequest = h.last();
  assert.equal(stopRequest.kind, "stop");
  const calls = h.nativeCalls, posted = h.clockSent.length;
  assert.equal(stopping.settled, false);
  assert.equal(h.contexts[0].closes, 0);
  h.clockReply(refillCommit);
  const next = { ...geometry, h0: refillCommit.h0, h1: refillCommit.h1, h2: refillCommit.h2 };
  requestRefill(h, next, { revision: 2 });
  await flush();
  assert.equal(stopping.settled, false, "valid in-flight clock traffic cannot retire the fresh stop ACK");
  assert.equal(h.contexts[0].closes, 0);
  assert.equal(h.nativeCalls, calls, "no native scheduling after stop owns cleanup");
  assert.equal(h.clockSent.length, posted, "no next-revision commit after cancellation");
  assert.ok(h.timers.size > 0);
  h.reply(stopRequest);
  const result = await stopping.result;
  assert.equal(result.ok, true, result.error?.message);
  assert.equal(owner.state, "closed");
  assertClean(h);
});

test("remote rejection of the initial native commit waits for its stop ACK without making its cause a cleanup error", async () => {
  const h = await harness({ autoClock: false });
  const { opening, reserve } = await beginNativeOpen(h);
  const commit = await admitNativeProgram(h, reserve);
  h.clockReply(commit, { status: 6, error: "chronology" });
  await flush();
  const stopRequest = h.last();
  assert.equal(stopRequest.kind, "stop");
  assert.equal(opening.settled, false, "startup refusal still joins the actual stop acknowledgement");
  assert.equal(h.contexts[0].closes, 0);
  assert.ok(h.timers.size > 0);
  h.reply(stopRequest);
  const original = failure(await opening.result, "remote");
  assert.equal(original.status, 6);
  assert.ok(original.cleanupError == null, "successful cleanup cannot report its original remote refusal as cleanupError");
  assertClean(h);
});

test("stopping ignores valid queued progress but malformed current clock identity or ACK still fails cleanup", async () => {
  for (const [kind, fields] of [
    ["clock-ready", { clockVersion: 2 }],
    ["clock-refill", { clockId: 2 }],
    ["clock-committed", { revision: 2, status: 0, error: null }],
    ["clock-committed", { revision: 1, status: "0", error: null }],
  ]) {
    const h = await harness({ autoClock: false });
    const { opening, reserve } = await beginNativeOpen(h);
    const owner = await exposeNativeOwner(h, opening, await admitNativeProgram(h, reserve));
    requestRefill(h, expectedInitial(reserve.origin, 4096, 48000)); await flush();
    const stopping = observe(() => owner.stop());
    assert.equal(stopping.settled, false);
    const before = h.nativeCalls;
    h.clockPacket(kind, { revision: 1, origin: reserve.origin, ...fields });
    const cleanup = failure(await stopping.result, "protocol");
    assert.equal(cleanup.operation, "receive");
    assert.equal(h.nativeCalls, before);
    assert.equal(owner.state, "failed");
    assertClean(h);
  }
});

async function openingPausedAt(stage, extraFaults = {}) {
  const gate = deferred(), controller = new AbortController();
  const faults = { autoClock: false, ...extraFaults };
  if (stage === "initial-resume") faults.resumeGate = gate;
  if (stage === "suspend") faults.suspendGate = gate;
  if (stage === "resume") faults.secondResumeGate = gate;
  const h = await harness(faults);
  const opening = observe(() => h.AudioHost.open(options({ signal: controller.signal,
    audioLimits: { maxFrames: 4096 } })));
  await flush(); h.ready();
  let reserve, commit;
  if (stage !== "initial-resume") {
    assert.equal(h.contexts[0].state, "running", "the real quantum arrives after the gesture resume");
    h.clockPacket("clock-probe", { frames: 128 }); await flush();
    if (stage !== "suspend") {
      reserve = h.clockSent[0]?.message;
      assert.equal(reserve?.kind, "clock-reserve");
      if (stage !== "reserve") {
        commit = await admitNativeProgram(h, reserve);
        if (stage !== "commit") { h.clockReply(commit); await flush(); }
      }
    }
  }
  assert.equal(opening.settled, false);
  return { h, opening, gate, controller, reserve, commit };
}

test("native opening waits for exact suspension before selecting frozen geometry and resumes only after whole ACK", async () => {
  const suspendGate = deferred(), secondResumeGate = deferred();
  const faults = { autoClock: false, suspendGate, secondResumeGate, suspendedTime: 0.512 };
  const h = await harness(faults);
  const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
  await flush();
  h.advanceBeforeDeadline(7);
  h.ready(); h.clockPacket("clock-probe", { frames: 128 }); await flush();
  assert.equal(h.contexts[0].suspends, 1);
  assert.equal(h.contexts[0].state, "running", "a requested suspend is not a fulfilled transition");
  assert.equal(h.clockSent.length, 0); assert.equal(h.nativeCalls, 0);
  h.contexts[0].currentTime = 0.25;
  h.advanceBeforeDeadline(5);
  suspendGate.resolve(); await flush();
  const reserve = h.clockSent[0]?.message;
  assert.equal(reserve?.kind, "clock-reserve");
  const lead = Math.max(4 * 128, Math.ceil(2 * 7 * 48000 / 1000));
  const origin = Math.ceil((Math.ceil(0.512 * 48000) + lead) / 128) * 128 + 64;
  assert.equal(reserve.origin, origin, "origin uses frozen native time and the original delivery measurement");
  assert.equal(h.contexts[0].state, "suspended"); assert.equal(h.contexts[0].resumes, 1);
  const commit = await admitNativeProgram(h, reserve);
  assert.equal(h.contexts[0].resumes, 1, "all events and whole commit await admission while suspended");
  assert.equal(h.nodes[0].nativeObservations.length, 0);
  assert.ok(h.nativeSnapshots.length > 0);
  assert.ok(h.nativeSnapshots.every(value => value.state === "suspended" && value.time === 0.512));
  assert.ok(h.clockSnapshots.every(value => value.state === "suspended" && value.time === 0.512));
  const program = structuredClone(commit);
  h.clockReply(commit); await flush();
  assert.equal(h.contexts[0].resumes, 2); assert.equal(h.contexts[0].state, "suspended");
  assert.equal(opening.settled, false);
  assert.equal(h.nodes[0].nativeObservations.length, 0, "paused ACKs produce no callback certificate");
  secondResumeGate.resolve(); await flush();
  assert.equal(h.contexts[0].state, "running");
  assert.equal(opening.settled, false, "native callbacks still own readiness after resume");
  assert.deepEqual(structuredClone(commit), program, "resume does not rewrite origin or horizons");
  assert.equal(h.clockSent.length, 2);
  h.nativeReady();
  const opened = await opening.result;
  assert.equal(opened.ok, true, opened.error?.message);
  assert.deepEqual(h.nodes[0].nativeObservations, [
    { kind: "partial", frames: 128, firstIndex: 64 }, { kind: "contiguous", frames: 128, firstIndex: 0 }]);
  await stop(h, opened.value);
});

test("rejected and throwing native suspension fail through stop without reserve or native program", async () => {
  for (const mode of ["throw", "reject"]) {
    const gate = deferred();
    const h = await harness({ autoClock: false, ...(mode === "throw" ? { suspendThrows: true } : { suspendGate: gate }) });
    const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
    await flush(); h.ready(); h.clockPacket("clock-probe", { frames: 128 }); await flush();
    if (mode === "reject") gate.reject(new Error("native suspend rejection"));
    const original = await nativeOpeningError(h, opening, "transport");
    assert.ok(original.cleanupError == null, "successful cleanup has no error");
    assert.equal(h.contexts[0].suspends, 1); assert.equal(h.contexts[0].resumes, 1);
    assert.equal(h.clockSent.length, 0); assert.equal(h.nativeCalls, 0);
  }
});

test("a fulfilled suspend in the wrong native state never reserves an origin", async () => {
  for (const suspendState of ["running", "interrupted", "closed"]) {
    const h = await harness({ autoClock: false, suspendState });
    const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
    await flush(); h.ready(); h.clockPacket("clock-probe", { frames: 128 });
    await nativeOpeningError(h, opening, "state");
    assert.equal(h.clockSent.length, 0); assert.equal(h.nativeCalls, 0);
    assert.equal(h.contexts[0].resumes, 1);
  }
});

test("rejected and throwing second resume retain the admitted program and join actual cleanup", async () => {
  for (const mode of ["throw", "reject"]) {
    const gate = deferred();
    const faults = { autoClock: false, ...(mode === "throw" ? { secondResumeThrows: true } : { secondResumeGate: gate }) };
    const h = await harness(faults);
    const { opening, reserve } = await beginNativeOpen(h);
    const commit = await admitNativeProgram(h, reserve), calls = h.nativeCalls;
    h.clockReply(commit); await flush();
    if (mode === "reject") gate.reject(new Error("native second resume rejection"));
    const original = await nativeOpeningError(h, opening, "transport");
    assert.ok(original.cleanupError == null, "successful cleanup has no error");
    assert.equal(h.nativeCalls, calls); assert.equal(h.clockSent.length, 2);
    assert.equal(h.contexts[0].suspends, 1); assert.equal(h.contexts[0].resumes, 2);
    assert.equal(h.nodes[0].nativeObservations.length, 0);
  }
});

test("a fulfilled second resume in the wrong state cannot supply native readiness", async () => {
  for (const secondResumeState of ["suspended", "interrupted", "closed"]) {
    const h = await harness({ autoClock: false, secondResumeState });
    const { opening, reserve } = await beginNativeOpen(h);
    const commit = await admitNativeProgram(h, reserve);
    h.clockReply(commit);
    await nativeOpeningError(h, opening, "state");
    assert.equal(h.nodes[0].nativeObservations.length, 0); assert.equal(h.contexts[0].resumes, 2);
  }
});

test("abort at each native opening await preserves the actual stop ACK and forbids subsequent activation", async () => {
  for (const stage of ["initial-resume", "suspend", "reserve", "commit", "resume", "native-ready"]) {
    const { h, opening, gate, controller, reserve, commit } = await openingPausedAt(stage);
    controller.abort(); await flush();
    const stopRequest = h.last(), calls = h.nativeCalls, posts = h.clockSent.length;
    const resumes = h.contexts[0].resumes, suspends = h.contexts[0].suspends;
    assert.equal(stopRequest.kind, "stop", stage);
    assert.equal(opening.settled, false, stage + " abort still joins stop");
    assert.equal(h.contexts[0].closes, 0);
    if (["initial-resume", "suspend", "resume"].includes(stage)) gate.resolve();
    if (stage === "reserve") h.clockReply(reserve);
    if (stage === "commit") h.clockReply(commit);
    if (stage === "native-ready") h.clockPacket("clock-ready", { revision: 0, origin: reserve.origin });
    await flush();
    assert.equal(opening.settled, false, stage + " late completion cannot retire stop");
    assert.equal(h.contexts[0].closes, 0);
    assert.equal(h.nativeCalls, calls); assert.equal(h.clockSent.length, posts);
    assert.equal(h.contexts[0].resumes, resumes); assert.equal(h.contexts[0].suspends, suspends);
    h.reply(stopRequest);
    const original = failure(await opening.result, "aborted");
    assert.ok(original.cleanupError == null, "successful cleanup has no error");
    assertClean(h);
  }
});

test("the original opening deadline covers stalled native suspension, ACKs and second resume", async () => {
  for (const stage of ["initial-resume", "suspend", "reserve", "commit", "resume", "native-ready"]) {
    const { h, opening, gate, reserve, commit } = await openingPausedAt(stage);
    await h.expire(); await flush();
    assert.equal(h.now, 50, stage + " cannot renew the original deadline");
    const stopRequest = h.last(), calls = h.nativeCalls, posts = h.clockSent.length;
    const resumes = h.contexts[0].resumes, suspends = h.contexts[0].suspends;
    assert.equal(stopRequest.kind, "stop");
    assert.equal(opening.settled, false); assert.equal(h.contexts[0].closes, 0);
    if (["initial-resume", "suspend", "resume"].includes(stage)) gate.resolve();
    if (stage === "reserve") h.clockReply(reserve);
    if (stage === "commit") h.clockReply(commit);
    if (stage === "native-ready") h.clockPacket("clock-ready", { revision: 0, origin: reserve.origin });
    await flush();
    assert.equal(opening.settled, false); assert.equal(h.contexts[0].closes, 0);
    assert.equal(h.nativeCalls, calls); assert.equal(h.clockSent.length, posts);
    assert.equal(h.contexts[0].resumes, resumes); assert.equal(h.contexts[0].suspends, suspends);
    h.reply(stopRequest);
    const original = failure(await opening.result, "timeout");
    assert.ok(original.cleanupError == null, "successful cleanup has no error");
    assertClean(h);
  }
});

test("a transition fulfilled only after close cannot reacquire or call the retired native owner", async () => {
  for (const stage of ["initial-resume", "suspend", "resume"]) {
    const { h, opening, gate, controller } = await openingPausedAt(stage);
    controller.abort(); await flush();
    h.reply(h.last());
    const original = failure(await opening.result, "aborted");
    assert.ok(original.cleanupError == null, "successful cleanup has no error");
    assertClean(h);
    const resumes = h.contexts[0].resumes, suspends = h.contexts[0].suspends;
    const calls = h.nativeCalls, posts = h.clockSent.length;
    gate.resolve(); await flush();
    assert.equal(h.contexts[0].state, "closed"); assert.equal(h.contexts[0].resumes, resumes);
    assert.equal(h.contexts[0].suspends, suspends); assert.equal(h.nativeCalls, calls);
    assert.equal(h.clockSent.length, posts);
    assertClean(h);
  }
});

test("suspension and the second resume share the original wall deadline instead of restarting setup budgets", async () => {
  const suspendGate = deferred(), secondResumeGate = deferred();
  const h = await harness({ autoClock: false, suspendGate, secondResumeGate });
  const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
  await flush(); h.advanceBeforeDeadline(20); h.ready(); h.clockPacket("clock-probe", { frames: 128 });
  await flush(); h.advanceBeforeDeadline(15); suspendGate.resolve(); await flush();
  const reserve = h.clockSent[0].message;
  h.advanceBeforeDeadline(5); h.clockReply(reserve); await flush();
  const commit = h.clockSent[1].message;
  h.advanceBeforeDeadline(5); h.clockReply(commit); await flush();
  assert.equal(h.contexts[0].resumes, 2); assert.equal(h.now, 45);
  await h.expire(); await flush();
  assert.equal(h.now, 50);
  const stopRequest = h.last();
  assert.equal(stopRequest.kind, "stop"); assert.equal(opening.settled, false);
  h.reply(stopRequest);
  const original = failure(await opening.result, "timeout");
  assert.ok(original.cleanupError == null, "successful cleanup has no error");
  secondResumeGate.resolve(); await flush();
  assertClean(h);
});

test("native transition failure retains a genuine distinct context-close error", async () => {
  for (const stage of ["suspend", "resume"]) {
    const closeGate = deferred();
    const faults = { autoClock: false, closeGate, ...(stage === "suspend" ? { suspendThrows: true } : { secondResumeThrows: true }) };
    const h = await harness(faults);
    const opening = observe(() => h.AudioHost.open(options({ audioLimits: { maxFrames: 4096 } })));
    await flush(); h.ready(); h.clockPacket("clock-probe", { frames: 128 }); await flush();
    if (stage === "resume") {
      const commit = await admitNativeProgram(h, h.clockSent[0].message);
      h.clockReply(commit); await flush();
    }
    const stopping = h.last(); assert.equal(stopping.kind, "stop");
    h.reply(stopping); await flush();
    assert.equal(opening.settled, false); assert.equal(h.contexts[0].closes, 1);
    const cause = new Error("actual native close rejection");
    closeGate.reject(cause);
    const original = failure(await opening.result, "transport");
    assert.notEqual(original.cause, cause);
    assert.equal(original.cleanupError.code, "transport");
    assert.equal(original.cleanupError.operation, "close");
    assert.equal(original.cleanupError.cause, cause);
    assert.notEqual(original.cleanupError, original);
    assertClean(h);
  }
});

test("late rejected native transitions cannot replace abort or timeout while the fresh stop ACK is pending", async () => {
  for (const stage of ["initial-resume", "suspend", "resume"]) for (const cancellation of ["abort", "timeout"]) {
    const { h, opening, gate, controller } = await openingPausedAt(stage);
    if (cancellation === "abort") controller.abort(); else await h.expire();
    await flush();
    const stopRequest = h.last(), calls = h.nativeCalls, posts = h.clockSent.length;
    assert.equal(stopRequest.kind, "stop");
    assert.equal(opening.settled, false);
    const late = new Error("late " + stage + " native rejection");
    gate.reject(late); await flush();
    assert.equal(opening.settled, false, stage + " late rejection must not retire the stop ACK");
    assert.equal(h.contexts[0].closes, 0);
    assert.equal(h.nativeCalls, calls); assert.equal(h.clockSent.length, posts);
    h.reply(stopRequest);
    const original = failure(await opening.result, cancellation === "abort" ? "aborted" : "timeout");
    assert.notEqual(original.cause, late);
    assert.ok(original.cleanupError == null, "late transition rejection is not a fresh cleanup failure");
    assertClean(h);
  }
});

test("malformed terminal during pending stop reports a distinct cleanup protocol error while retaining original diagnostics", async () => {
  const h = await harness(), owner = await open(h);
  await acknowledged(h, () => owner.finish());
  const pending = observe(() => owner.commands([command(), command({ kind: 1 })]));
  h.reply(h.last(), { status: 6, admitted: 1, error: "chronology", diagnostics: diagnosticFacts() });
  const original = failure(await pending.result, "remote");
  const retained = original.diagnostics;
  const first = owner.stop(), stopping = observe(() => first);
  assert.equal(owner.stop(), first);
  assert.equal(h.last().kind, "stop");
  h.nodes[0].port.emit("message", diagnosticTerminal({ diagnosticVersion: 2 }));
  const cleanup = failure(await stopping.result, "protocol");
  assert.notEqual(cleanup, original);
  assert.equal(cleanup.operation, "receive");
  assert.equal(cleanup.diagnostics, null);
  assert.equal(cleanup.cause.name, "TypeError");
  assert.equal(failure(await observe(() => owner.poll()).result), original);
  assert.equal(original.status, 6); assert.equal(original.sequence, 2); assert.equal(original.admitted, 1);
  assert.equal(original.diagnostics, retained);
  assert.deepEqual({ ...retained }, diagnosticFacts());
  assert.equal(owner.stop(), first); assert.equal(owner.state, "failed");
  assertClean(h);
});

test("reentrant rejected ACK diagnostic getters cannot replace the first terminal cause or revive a closed owner", async () => {
  for (const action of ["close", "terminal"]) {
    const h = await harness(), owner = await open(h); await acknowledged(h, () => owner.finish());
    const pending = observe(() => owner.commands([command(), command({ kind: 1 })]));
    const stale = h.nodes[0].port.onmessage;
    const packet = { kind: "ack", generation: 17, sequence: 2, operation: "commands",
      status: 6, admitted: 1, error: "chronology", report: null };
    let reads = 0;
    Object.defineProperty(packet, "diagnostics", { get() {
      reads++;
      if (action === "close") owner.stop();
      else stale({ data: { kind: "terminal", generation: 17, status: 9 } });
      return diagnosticFacts();
    } });
    h.nodes[0].port.emit("message", packet);
    const original = failure(await pending.result);
    assert.equal(reads, 1); assert.equal(original.code, action === "close" ? "closed" : "processor");
    assert.equal(original.diagnostics, null);
    if (action === "terminal") assert.equal(original.status, 9);
    if (action === "close") { const stopping = observe(() => owner.stop()); h.reply(h.last()); assert.equal((await stopping.result).ok, true); assert.equal(owner.state, "closed"); assertClean(h); } else { await stop(h, owner, "failed"); assert.equal(failure(await observe(() => owner.poll()).result), original); }
    assert.equal(h.timers.size, 0);
  }
});


test("rejected ACK preserves its actual sequence and admitted prefix with immutable diagnostics before terminal cleanup", async () => {
  for (const hasDiagnostics of [false, true]) {
    const h = await harness(), owner = await open(h); await acknowledged(h, () => owner.finish());
    const pending = observe(() => owner.commands([command(), command({ kind: 1 })]));
    const stale = h.nodes[0].port.onmessage;
    const diagnostics = diagnosticFacts();
    const fields = { status: 6, admitted: 1, error: "chronology", ...(hasDiagnostics ? { diagnostics } : {}) };
    h.reply(h.last(), fields);
    const original = failure(await pending.result);
    assert.equal(original.code, "remote"); assert.equal(original.status, 6);
    assert.equal(original.generation, 17); assert.equal(original.sequence, 2);
    assert.equal(original.admitted, 1); assert.ok(original.message.includes("chronology"));
    if (hasDiagnostics) { assert.deepEqual({ ...original.diagnostics }, diagnosticFacts()); assert.ok(Object.isFrozen(original.diagnostics)); }
    else assert.equal(original.diagnostics, null);
    const retained = original.diagnostics;
    diagnostics.expectedFrameHigh = 0; diagnostics.currentFrame = 0;
    stale({ data: diagnosticTerminal({ status: 9 }) });
    assert.equal(failure(await observe(() => owner.poll()).result), original); assert.equal(original.diagnostics, retained);
    if (hasDiagnostics) assert.deepEqual({ ...retained }, diagnosticFacts());
    await stop(h, owner, "failed"); assertClean(h);
    stale({ data: diagnosticTerminal({ diagnosticVersion: 2 }) });
    assert.equal(failure(await observe(() => owner.poll()).result), original); assert.equal(original.diagnostics, retained);
  }
});

test("malformed rejected ACK diagnostics and unsolicited success diagnostics are protocol failures", async () => {
  for (const [status, diagnostics] of [
    [6, {}], [6, null], [6, diagnosticFacts({ diagnosticVersion: 2 })],
    [6, diagnosticFacts({ expectedFrameHigh: "0" })],
    [6, diagnosticFacts({ successfulArmFramePresent: 0, successfulArmFrame: 1 })],
    [0, diagnosticFacts()],
  ]) {
    const h = await harness(), owner = await open(h); await acknowledged(h, () => owner.finish());
    const pending = observe(() => owner.commands([command(), command({ kind: 1 })]));
    const fields = { status, admitted: status === 0 ? 2 : 0,
      error: status === 0 ? null : "chronology", diagnostics };
    h.reply(h.last(), fields);
    const original = failure(await pending.result);
    assert.equal(original.code, "protocol"); assert.equal(original.diagnostics, null);
    await stop(h, owner, "failed"); assertClean(h);
    assert.equal(failure(await observe(() => owner.poll()).result), original);
  }
});

test("stale rejected ACK never reads nested diagnostics while hostile correlated diagnostics fail closed", async () => {
  const h = await harness(), owner = await open(h); await acknowledged(h, () => owner.finish());
  const pending = observe(() => owner.commands([command(), command({ kind: 1 })]));
  let reads = 0;
  // Deliver a raw packet so the fixture itself does not observe the hostile property.
  const terminal = { kind: "ack", generation: 16, sequence: 2,
    operation: "commands", status: 6, admitted: 0, error: "chronology", report: null };
  Object.defineProperty(terminal, "diagnostics", { get() { reads++; throw new Error("stale diagnostics"); } });
  h.nodes[0].port.emit("message", terminal);
  await flush(); assert.equal(reads, 0); assert.equal(pending.settled, false);
  terminal.generation = 17;
  h.nodes[0].port.emit("message", terminal);
  const original = failure(await pending.result);
  assert.equal(reads, 1); assert.equal(original.code, "protocol");
  await stop(h, owner, "failed"); assertClean(h);
  assert.equal(failure(await observe(() => owner.poll()).result), original);
});


test("each diagnostic field is observed once and a missing versioned numeric field fails closed", async () => {
  for (const missing of [false, true]) {
    const h = await harness(), owner = await open(h);
    const pending = observe(() => owner.poll());
    const facts = diagnosticFacts(), reads = new Map();
    const terminal = { kind: "terminal", generation: 17, status: 6 };
    for (const [field, value] of Object.entries(facts)) {
      if (missing && field === "expectedFrameHigh") continue;
      Object.defineProperty(terminal, field, { enumerable: true, get() {
        reads.set(field, (reads.get(field) ?? 0) + 1); return value;
      } });
    }
    h.nodes[0].port.emit("message", terminal);
    const original = failure(await pending.result);
    assert.equal(original.code, missing ? "protocol" : "processor");
    for (const [field, count] of reads) assert.equal(count, 1, field + " must be read once");
    if (!missing) assert.deepEqual({ ...original.diagnostics }, facts);
    await stop(h, owner, "failed"); assertClean(h);
    assert.equal(failure(await observe(() => owner.poll()).result), original);
  }
});


function diagnosticFacts(fields = {}) {
  return { diagnosticVersion: 1, origin: 2, ownerPhase: 3,
    currentFramePresent: 1, currentFrame: 9007199254740991,
    blockFramesPresent: 1, blockFrames: 128,
    expectedFramePresent: 1, expectedFrameLow: 0xffffffff, expectedFrameHigh: 0x80000000,
    startFramePresent: 1, startFrameLow: 0, startFrameHigh: 0xffffffff,
    successfulArmFramePresent: 1, successfulArmFrame: 4294967297, ...fields };
}
function diagnosticTerminal(fields = {}) {
  return { kind: "terminal", generation: 17, status: 6, ...diagnosticFacts(), ...fields };
}

test("versioned terminal retains exact immutable known-only high-word facts through pending failure and cleanup", async () => {
  const h = await harness(), owner = await open(h);
  const pending = observe(() => owner.poll());
  const stale = h.nodes[0].port.onmessage;
  const terminal = diagnosticTerminal();
  Object.defineProperty(terminal, "untrustedExtra", { enumerable: true, get() { throw new Error("unknown fields must not be read"); } });
  h.nodes[0].port.emit("message", terminal);
  const original = failure(await pending.result);
  assert.equal(original.code, "processor"); assert.equal(original.status, 6);
  assert.equal(original.generation, 17); assert.equal(original.sequence, null); assert.equal(original.admitted, null);
  assert.deepEqual({ ...original.diagnostics }, diagnosticFacts());
  assert.ok(Object.isFrozen(original.diagnostics)); assert.notEqual(original.diagnostics, terminal);
  assert.throws(() => { original.diagnostics.expectedFrameHigh = 0; }, TypeError);
  terminal.expectedFrameHigh = 0; terminal.startFrameHigh = 1; terminal.currentFrame = 0;
  const retained = original.diagnostics;
  stale({ data: diagnosticTerminal({ status: 9, expectedFrameHigh: 7 }) });
  assert.equal(failure(await observe(() => owner.poll()).result), original);
  assert.equal(original.diagnostics, retained);
  assert.deepEqual({ ...retained }, diagnosticFacts());
  await stop(h, owner, "failed"); assertClean(h);
  assert.equal(failure(await observe(() => owner.poll()).result), original); assert.equal(original.diagnostics, retained);
  assert.equal(h.sent.filter(row => row.message.kind === "poll").length, 1); assert.equal(h.timers.size, 0);
});

test("legacy terminals and canonical absent or present-zero diagnostic facts remain distinct", async () => {
  for (const fields of [null,
    diagnosticFacts({ origin: 0, ownerPhase: 0, currentFramePresent: 0, currentFrame: 0,
      blockFramesPresent: 0, blockFrames: 0, expectedFramePresent: 0, expectedFrameLow: 0, expectedFrameHigh: 0,
      startFramePresent: 0, startFrameLow: 0, startFrameHigh: 0, successfulArmFramePresent: 0, successfulArmFrame: 0 }),
    diagnosticFacts({ currentFrame: 0, blockFrames: 0, expectedFrameLow: 0, expectedFrameHigh: 0,
      startFrameLow: 0, startFrameHigh: 0, successfulArmFrame: 0 })]) {
    const h = await harness(), owner = await open(h);
    const pending = observe(() => owner.poll());
    const terminal = { kind: "terminal", generation: 17, status: 6, ...(fields ?? {}) };
    h.nodes[0].port.emit("message", terminal);
    const original = failure(await pending.result);
    assert.equal(original.code, "processor"); assert.equal(original.status, 6);
    if (fields === null) assert.equal(original.diagnostics, null);
    else { assert.deepEqual({ ...original.diagnostics }, fields); assert.ok(Object.isFrozen(original.diagnostics)); }
    await stop(h, owner, "failed"); assertClean(h);
    assert.equal(failure(await observe(() => owner.poll()).result), original);
  }
});

test("malformed diagnostic version, types, presence and absent values produce sticky protocol failure", async () => {
  for (const fields of [
    { diagnosticVersion: 2 }, { diagnosticVersion: null }, { diagnosticVersion: "1" },
    { origin: 3 }, { ownerPhase: 4 }, { currentFramePresent: true }, { currentFrame: 1n },
    { currentFrame: Number.MAX_SAFE_INTEGER + 1 }, { blockFrames: 4294967296 },
    { expectedFrameHigh: -1 }, { startFrameLow: "0" }, { successfulArmFrame: Infinity },
    { currentFramePresent: 0, currentFrame: 1 }, { blockFramesPresent: 0, blockFrames: 128 },
    { expectedFramePresent: 0, expectedFrameHigh: 1 }, { startFramePresent: 0, startFrameLow: 1 },
    { successfulArmFramePresent: 0, successfulArmFrame: 1 },
  ]) {
    const h = await harness(), owner = await open(h);
    const pending = observe(() => owner.poll());
    const terminal = diagnosticTerminal(fields);
    h.nodes[0].port.emit("message", terminal);
    const original = failure(await pending.result);
    assert.equal(original.code, "protocol");
    assert.equal(original.diagnostics, null);
    await stop(h, owner, "failed"); assertClean(h);
    assert.equal(failure(await observe(() => owner.poll()).result), original); assert.equal(h.timers.size, 0);
  }
});

test("old generation never reads diagnostics or settles pending work", async () => {
  const h = await harness(), owner = await open(h);
  const pending = observe(() => owner.poll());
  let reads = 0;
  const terminal = { kind: "terminal", generation: 16, status: 6 };
  Object.defineProperty(terminal, "diagnosticVersion", { get() { reads++; throw new Error("obsolete diagnostics"); } });
  h.nodes[0].port.emit("message", terminal);
  await flush(); assert.equal(reads, 0); assert.equal(pending.settled, false);
  h.reply(h.last());
  assert.equal((await pending.result).ok, true);
  await stop(h, owner);
});

test("hostile diagnostic getter rejects once while reentrant close or terminal preserves its first cause", async () => {
  for (const action of ["throw", "close", "terminal"]) {
    const h = await harness(), owner = await open(h);
    const pending = observe(() => owner.poll());
    const stale = h.nodes[0].port.onmessage;
    const terminal = diagnosticTerminal();
    let reads = 0;
    Object.defineProperty(terminal, "expectedFrameLow", { get() {
      reads++;
      if (action === "throw") throw new Error("hostile diagnostic read");
      if (action === "close") owner.stop();
      else stale({ data: { kind: "terminal", generation: 17, status: 9 } });
      return 0xffffffff;
    } });
    h.nodes[0].port.emit("message", terminal);
    const original = failure(await pending.result);
    assert.equal(reads, 1);
    assert.equal(original.code, action === "throw" ? "protocol" : action === "close" ? "closed" : "processor");
    assert.equal(original.diagnostics, null);
    if (action === "terminal") assert.equal(original.status, 9);
    if (action === "close") { const joined = observe(() => owner.stop()); h.reply(h.last()); assert.equal((await joined.result).ok, true); assert.equal(owner.state, "closed"); assertClean(h); } else { await stop(h, owner, "failed"); assertClean(h); }
    stale({ data: diagnosticTerminal({ diagnosticVersion: 2 }) });
    if (action !== "close") assert.equal(failure(await observe(() => owner.poll()).result), original);
    assert.equal(h.timers.size, 0);
  }
});

const emptyModule = new WebAssembly.Module(Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0]));
const U64_MAX = 18446744073709551615n;
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;

function options(overrides = {}) {
  return {
    module: emptyModule, generation: 17, channels: 2, timeoutMs: 50, ...overrides,
    pcmLimits: { maxAssetBytes: 16, maxTotalBytes: 24, maxSamples: 2, ...overrides.pcmLimits },
    audioLimits: { queueCapacity: 4, maxVoices: 2, pendingCapacity: 4, maxFrames: 5,
      maxCommandsPerRender: 4, ...overrides.audioLimits },
  };
}
function sample(overrides = {}) {
  return { id: 1n, rate: 44100, channels: 2,
    pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]), ...overrides };
}
function command(overrides = {}) {
  return { kind: 0, voice: U64_MAX, sample: 1n, at: I64_MIN,
    gain: -0.5, value: I64_MAX, denominator: 0n, ...overrides };
}
function report(available = false) {
  const words = new Uint32Array(58);
  words[0] = Number(available);
  return { available, words };
}
function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function observe(action) {
  const watched = { settled: false };
  let returned;
  try { returned = action(); } catch (error) { returned = Promise.reject(error); }
  watched.result = Promise.resolve(returned).then(
    value => { watched.settled = true; return { ok: true, value }; },
    error => { watched.settled = true; return { ok: false, error }; },
  );
  return watched;
}
async function flush() {
  for (let index = 0; index < 32; index++) await Promise.resolve();
}
function failure(result, code) {
  assert.equal(result.ok, false, "operation unexpectedly succeeded");
  if (code !== undefined) assert.equal(result.error.code, code);
  return result.error;
}

async function harness(faults = {}) {
  const contexts = [];
  const nodes = [];
  const trace = [];
  const channels = [];
  const timers = new Map();
  const nativeSnapshots = [];
  const clockSnapshots = [];
  let now = 0;
  let timerId = 0;
  let postAttempts = 0;
  let clockPostAttempts = 0;
  let nativeCalls = 0;
  function observeNativeReady(node, fields = {}) {
    assert.equal(node.context.state, "running", "native readiness needs an actually resumed context");
    assert.ok(node.context.resumes >= 2, "the opening native pause must have resumed");
    const quantum = node.probeFrames ?? faults.probeFrames ?? 4;
    node.nativeObservations.push({ kind: "partial", frames: quantum, firstIndex: Math.floor(quantum / 2) },
      { kind: "contiguous", frames: quantum, firstIndex: 0 });
    trace.push(["native-callback", "partial"], ["native-callback", "contiguous"]);
    node.port.emit("message", { kind: "clock-ready", generation: node.configuration.processorOptions.generation,
      clockVersion: 1, clockId: 1, revision: 0, origin: node.clockProgram.origin, ...fields });
  }
  function maybeNativeReady(node) {
    if (faults.autoClock === false || !node?.clockProgram || node.nativeReadyQueued
      || node.context.state !== "running" || node.context.resumes < 2) return;
    node.nativeReadyQueued = true;
    Promise.resolve().then(() => {
      if (!node.port.closes && node.context.state === "running") observeNativeReady(node);
    });
  }

  class EventTarget {
    constructor() { this.listeners = new Map(); }
    addEventListener(type, handler) {
      if (!this.listeners.has(type)) this.listeners.set(type, new Set());
      this.listeners.get(type).add(handler);
    }
    removeEventListener(type, handler) { this.listeners.get(type)?.delete(handler); }
    emit(type, data) {
      const event = type === "message" ? { data } : { type, ...data };
      if (typeof this[`on${type}`] === "function") this[`on${type}`](event);
      for (const handler of [...(this.listeners.get(type) ?? [])]) handler.call(this, event);
    }
    handlers() {
      return [...this.listeners.values()].reduce((count, values) => count + values.size, 0)
        + ["onmessage", "onmessageerror", "onprocessorerror", "onstatechange"]
          .filter(key => typeof this[key] === "function").length;
    }
  }
  class Port extends EventTarget {
    constructor() { super(); this.sent = []; this.closes = 0; this.starts = 0; }
    postMessage(message, transfer = []) {
      const isClock = typeof message.kind === "string" && message.kind.startsWith("clock-");
      if (isClock) clockPostAttempts++; else postAttempts++;
      if ((!isClock && postAttempts === faults.postThrowsAt)
        || (isClock && clockPostAttempts === faults.clockPostThrowsAt)) throw new Error("injected postMessage failure");
      assert.equal(this.closes, 0, "posting after port cleanup");
      const transfers = Array.isArray(transfer) ? transfer : transfer.transfer ?? [];
      const snapshot = ["attach-commands", "attach-samples"].includes(message.kind)
        ? { ...structuredClone({ ...message, port: undefined }), port: message.port }
        : structuredClone(message, { transfer: transfers });
      this.sent.push({ message: snapshot, transfers: [...transfers] });
      trace.push(["post", message.kind, message.sequence]);
      if (isClock) {
        const native = nodes.find(node => node.port === this)?.context;
        clockSnapshots.push({ kind: message.kind, state: native?.state, time: native?.currentTime });
      }
      // Existing public-control regressions get a separate, explicit clock peer.
      // New clock tests disable it and drive every protocol step themselves.
      if (isClock && faults.autoClock !== false) Promise.resolve().then(() => {
        if (this.closes) return;
        const node = nodes.find(candidate => candidate.port === this);
        if (message.kind === "clock-commit" && message.revision === 0) node.clockProgram = message;
        this.emit("message", { kind: message.kind === "clock-reserve" ? "clock-reserved" : "clock-committed",
          generation: message.generation, clockVersion: 1, clockId: 1,
          revision: message.revision, status: 0, error: null });
        maybeNativeReady(node);
      });
    }
    start() { this.starts++; }
    close() { this.closes++; trace.push(["port-close"]); }
  }
  class MessageChannel {
    constructor() {
      if (faults.channelThrows) throw new Error("channel constructor failed");
      this.port1 = new Port(); this.port2 = new Port(); channels.push(this);
    }
  }
  class AudioContext extends EventTarget {
    constructor(configuration) {
      super();
      trace.push(["context", configuration]);
      if (faults.contextThrows) throw new Error("context constructor failed");
      this.sampleRate = faults.sampleRate ?? 48000;
      this.currentTime = 0;
      this.state = "suspended";
      this.destination = { context: this };
      this.closes = 0;
      this.handlersAtClose = [];
      this.outputReads = 0;
      this.getOutputTimestamp = () => {
        this.outputReads++;
        return Object.hasOwn(faults, "outputEvidence") ? faults.outputEvidence
          : { contextTime: 0, performanceTime: 0 };
      };
      this.resumes = 0;
      this.suspends = 0;
      this.audioWorklet = {
        addModule: url => {
          trace.push(["add-module", String(url)]);
          if (faults.moduleThrows) throw new Error("addModule failed");
          return faults.moduleGate?.promise ?? Promise.resolve();
        },
      };
      contexts.push(this);
    }
    resume() {
      this.resumes++;
      trace.push(["resume"]);
      const first = this.resumes === 1;
      if (first ? faults.resumeThrows : faults.secondResumeThrows) throw new Error("resume failed");
      return ((first ? faults.resumeGate : faults.secondResumeGate)?.promise ?? Promise.resolve()).then(() => {
        if (this.state !== "closed") this.state = (first ? faults.resumeState : faults.secondResumeState) ?? "running";
        this.emit("statechange", {});
        maybeNativeReady(nodes.find(node => node.context === this));
      });
    }
    suspend() {
      this.suspends++;
      trace.push(["suspend"]);
      if (faults.suspendThrows) throw new Error("suspend failed");
      return (faults.suspendGate?.promise ?? Promise.resolve()).then(() => {
        if (this.state !== "closed") {
          if (faults.suspendedTime !== undefined) this.currentTime = faults.suspendedTime;
          this.state = faults.suspendState ?? "suspended";
        }
        this.emit("statechange", {});
      });
    }
    close() {
      this.closes++;
      this.handlersAtClose.push(this.handlers());
      trace.push(["context-close"]);
      if (faults.closeThrows) throw new Error("close failed");
      return (faults.closeGate?.promise ?? Promise.resolve()).then(() => {
        this.state = "closed";
        this.emit("statechange", {});
      });
    }
  }
  class AudioWorkletNode extends EventTarget {
    constructor(context, name, configuration) {
      super();
      trace.push(["node", name]);
      if (faults.nodeThrows) throw new Error("node constructor failed");
      this.context = context;
      this.name = name;
      this.configuration = configuration;
      this.port = new Port();
      this.nativeObservations = [];
      this.parameters = new Map(TEST_NAMES.map(name => {
        const parameter = { events: [],
          setValueAtTime(value, at) { return write("set", value, at); },
          linearRampToValueAtTime(value, at) { return write("ramp", value, at); },
          cancelScheduledValues(at) { return write("cancel", undefined, at); },
          cancelAndHoldAtTime(at) { return write("hold", undefined, at); },
        };
        let value = 0;
        Object.defineProperty(parameter, "value", { get() { return value; }, set(next) { value = next; write("value", next, null); } });
        function write(kind, next, at) {
          nativeCalls++;
          const event = { kind, value: next, at };
          parameter.events.push(event);
          trace.push(["native", name, kind, next, at]);
          nativeSnapshots.push({ name, kind, value: next, at, state: context.state, time: context.currentTime });
          if (faults.nativeThrowAt === nativeCalls) throw new Error("injected native automation failure");
          faults.nativeAfterCall?.({ name, ...event, call: nativeCalls, context });
          return parameter;
        }
        return [name, parameter];
      }));
      if (faults.missingParameter) this.parameters.delete(faults.missingParameter);
      this.connections = [];
      this.disconnects = 0;
      nodes.push(this);
    }
    connect(destination) {
      this.connections.push(destination);
      trace.push(["connect"]);
      if (faults.connectThrows) throw new Error("connect failed");
      return destination;
    }
    disconnect() { this.disconnects++; trace.push(["disconnect"]); }
  }
  const context = createContext({
    AudioContext: faults.unsupported ? undefined : AudioContext,
    AudioWorkletNode: faults.unsupported ? undefined : AudioWorkletNode,
    MessageChannel: faults.noMessageChannel ? undefined : MessageChannel,
    AbortController, AbortSignal, ArrayBuffer, SharedArrayBuffer, DataView,
    Float32Array, Uint32Array, Uint8Array, WebAssembly, URL, DOMException, structuredClone,
    Date: class extends Date {
      constructor(...args) { super(...(args.length ? args : [now])); }
      static now() { return now; }
    },
    performance: { now: () => {
      const value = faults.performanceTimes?.length ? faults.performanceTimes.shift() : now;
      trace.push(["performance-now", value]);
      return value;
    } },
    setTimeout(callback, delay, ...args) {
      const id = ++timerId;
      timers.set(id, { at: now + delay, callback: () => callback(...args) });
      return id;
    },
    clearTimeout(id) { timers.delete(id); },
  });
  const actual = new SourceTextModule(source, {
    context, identifier: sourceUrl.href,
    initializeImportMeta(meta) { meta.url = sourceUrl.href; },
  });
  const dependencies = new Map([[failureUrl.href, failureSource], [clockUrl.href, clockSource], [nativeProbeUrl.href, nativeProbeSource]]);
  const linked = new Map();
  await actual.link((specifier, parent) => {
    const identifier = new URL(specifier, parent.identifier).href;
    if (dependencies.has(identifier)) {
      if (!linked.has(identifier)) linked.set(identifier, new SourceTextModule(dependencies.get(identifier), { context, identifier }));
      return linked.get(identifier);
    }
    throw new Error(`Unexpected host import: ${specifier}`);
  });
  await actual.evaluate();
  return {
    AudioHost: actual.namespace.AudioHost, AudioHostError: actual.namespace.AudioHostError,
    faults, contexts, nodes, channels, timers, trace, nativeSnapshots, clockSnapshots,
    get now() { return now; },
    get sent() { return (nodes.at(-1)?.port.sent ?? []).filter(row => !row.message.kind.startsWith("clock-")); },
    get clockSent() { return (nodes.at(-1)?.port.sent ?? []).filter(row => row.message.kind.startsWith("clock-")); },
    get allSent() { return nodes.at(-1)?.port.sent ?? []; },
    get nativeCalls() { return nativeCalls; },
    last() { return this.sent.at(-1)?.message; },
    ready(fields = {}) {
      const node = nodes.at(-1);
      node.port.emit("message", { kind: "ready", generation: node.configuration.processorOptions.generation,
        sampleRate: node.context.sampleRate, channels: node.configuration.processorOptions.channels, ...fields });
      if (faults.autoClock !== false && !node.clockProbed) {
        node.clockProbed = true;
        node.probeFrames = faults.probeFrames ?? 4;
        node.port.emit("message", { kind: "clock-probe",
          generation: node.configuration.processorOptions.generation, clockVersion: 1, clockId: 1, frames: faults.probeFrames ?? 4 });
      }
    },
    clockPacket(kind, fields = {}) {
      const node = nodes.at(-1);
      if (kind === "clock-probe") node.probeFrames = fields.frames;
      node.port.emit("message", { kind, generation: node.configuration.processorOptions.generation,
        clockVersion: 1, clockId: 1, ...fields });
    },
    clockReply(message, fields = {}) {
      if (message.kind === "clock-commit" && message.revision === 0) nodes.at(-1).clockProgram = message;
      this.clockPacket(message.kind === "clock-reserve" ? "clock-reserved" : "clock-committed",
        { revision: message.revision, status: 0, error: null, ...fields });
    },
    nativeReady(fields = {}) { observeNativeReady(nodes.at(-1), fields); },
    reply(message, fields = {}) {
      nodes.at(-1).port.emit("message", {
        kind: "ack", generation: message.generation, sequence: message.sequence,
        operation: message.kind, status: 0,
        admitted: message.kind === "commands" ? message.commands.length : 0,
        error: null, report: message.kind === "poll" ? report() : null, ...fields,
      });
    },
    async expire() {
      assert.ok(timers.size, "expected a finite host deadline");
      const [id, timer] = [...timers].sort((left, right) => left[1].at - right[1].at)[0];
      timers.delete(id);
      now = timer.at;
      timer.callback();
      await flush();
    },
    advanceBeforeDeadline(elapsed) {
      const target = now + elapsed;
      assert.ok([...timers.values()].every(timer => timer.at > target));
      now = target;
    },
  };
}

function assertClean(h) {
  assert.equal(h.timers.size, 0, "cleanup must clear every deadline");
  for (const context of h.contexts) {
    assert.equal(context.closes, 1);
    assert.deepEqual(context.handlersAtClose, [0], "context listeners must be cleared before close");
    assert.equal(context.handlers(), 0);
  }
  for (const node of h.nodes) {
    assert.equal(node.disconnects, 1);
    assert.equal(node.port.closes, 1);
    assert.equal(node.handlers(), 0);
    assert.equal(node.port.handlers(), 0);
  }
}
async function open(h, configuration = options()) {
  const opening = observe(() => h.AudioHost.open(configuration));
  await flush();
  assert.equal(h.nodes.length, 1);
  assert.equal(opening.settled, false, "ready is required before exposing the owner");
  h.ready();
  const result = await opening.result;
  assert.equal(result.ok, true, result.error?.message);
  assert.equal(result.value.state, "setup");
  return result.value;
}
async function acknowledged(h, action, fields = {}) {
  const count = h.sent.length;
  const pending = observe(action);
  await flush();
  assert.equal(h.sent.length, count + 1, "one operation must post exactly one control");
  const message = h.last();
  h.reply(message, fields);
  const result = await pending.result;
  assert.equal(result.ok, true, result.error?.message);
  return result.value;
}
async function localError(h, action, code = "validation") {
  const count = h.sent.length;
  const error = failure(await observe(action).result, code);
  assert.ok(error instanceof h.AudioHostError);
  assert.equal(h.sent.length, count, "local rejection must not post or consume sequence");
  return error;
}
async function stop(h, owner, expectedState = "closed") {
  const first = owner.stop();
  assert.equal(owner.stop(), first, "concurrent stop shares the cleanup promise");
  const pending = observe(() => first);
  await flush();
  assert.equal(h.last().kind, "stop");
  h.reply(h.last());
  const result = await pending.result;
  assert.equal(result.ok, true, result.error?.message);
  assert.equal(result.value, undefined);
  assert.equal(owner.stop(), first, "settled stop also retains its promise");
  assert.equal(owner.state, expectedState);
  assertClean(h);
}


test("sample handoff requires empty setup and transfers exclusive upload authority only after its own ACK", async () => {
  const h = await harness(), owner = await open(h);
  const pending = observe(() => owner.openSamplePort()), attachment = h.last(), channel = h.channels[0];
  assert.equal(attachment.kind, "attach-samples"); assert.equal(attachment.sequence, 1);
  assert.equal(attachment.generation, 17); assert.equal(attachment.port, channel.port2);
  assert.deepEqual(h.sent.at(-1).transfers, [channel.port2]);
  await flush(); assert.equal(pending.settled, false);
  await localError(h, () => owner.sample(sample()), "busy");
  await localError(h, () => owner.openSamplePort(), "busy");
  h.reply(attachment, { generation: 16 }); await flush(); assert.equal(pending.settled, false);
  h.reply(attachment); const adopted = await pending.result; assert.equal(adopted.ok, true);
  const descriptor = adopted.value;
  assert.equal(descriptor.port, channel.port1); assert.equal(descriptor.generation, 17);
  assert.equal(descriptor.channels, 2); assert.equal(descriptor.timeoutMs, 50);
  assert.deepEqual({ ...descriptor.pcmLimits }, { maxAssetBytes: 16, maxTotalBytes: 24, maxSamples: 2 });
  assert.ok(Object.isFrozen(descriptor)); assert.ok(Object.isFrozen(descriptor.pcmLimits));
  const untransferred = sample();
  await localError(h, () => owner.sample(untransferred), "state");
  assert.equal(untransferred.pcm.byteLength, 16);
  await localError(h, () => owner.openSamplePort(), "state");
  // The processor alone validates end-samples; the host consumes its ordinary finish ACK.
  await acknowledged(h, () => owner.finish(7n)); assert.equal(h.last().sequence, 2);
  assert.equal(h.last().endFrame, 7n);
  await acknowledged(h, () => owner.arm(11n));
  descriptor.port.close(); // Adopted client endpoint is caller-owned.
  await stop(h, owner); assert.equal(channel.port1.closes, 1);
  assert.equal(h.sent.filter(item => item.message.kind === "sample").length, 0);

  for (const phase of ["sample", "finish"]) {
    const used = await harness(), previous = await open(used);
    if (phase === "sample") await acknowledged(used, () => previous.sample(sample({ pcm: new Float32Array(0) })));
    else await acknowledged(used, () => previous.finish());
    await localError(used, () => previous.openSamplePort(), "state");
    assert.equal(used.channels.length, 0); await stop(used, previous);
  }
});

test("failed or cancelled sample handoff closes unreturned ports and never restores a second sample producer", async () => {
  const closing = deferred(), h = await harness({ closeGate: closing });
  const owner = await open(h), opening = observe(() => owner.openSamplePort()), attachment = h.last();
  const joined = observe(() => owner.stop()); await flush();
  failure(await opening.result, "closed");
  assert.equal(h.channels[0].port1.closes, 1); assert.equal(h.channels[0].port2.closes, 1);
  const stopMessage = h.last(); assert.equal(stopMessage.kind, "stop");
  h.reply(attachment); await flush(); assert.equal(joined.settled, false);
  h.reply(stopMessage); await flush(); assert.equal(joined.settled, false);
  assert.equal(h.contexts[0].closes, 1);
  closing.resolve(); assert.equal((await joined.result).ok, true); assertClean(h);
  await localError(h, () => owner.sample(sample()), "closed");

  for (const kind of ["remote", "protocol", "timeout", "post"]) {
    const failed = await harness(kind === "post" ? { postThrowsAt: 1 } : {}), current = await open(failed);
    const pending = observe(() => current.openSamplePort());
    if (kind === "remote") failed.reply(failed.last(), { status: 3, error: "sample-port refused" });
    else if (kind === "protocol") failed.reply(failed.last(), { admitted: 1 });
    else if (kind === "timeout") await failed.expire();
    const error = failure(await pending.result, kind === "post" ? "transport" : kind);
    assert.equal(failed.channels[0].port1.closes, 1); assert.equal(failed.channels[0].port2.closes, 1);
    assert.equal(await localError(failed, () => current.sample(sample()), error.code), error);
    assert.equal(await localError(failed, () => current.openSamplePort(), error.code), error);
    await stop(failed, current, "failed");
    assert.equal(failed.sent.filter(item => item.message.kind === "sample").length, 0);
  }
});

test("command handoff is a single acknowledged transfer while host polling, arming and stop retain their own control sequence", async () => {
  const h = await harness(), owner = await open(h);
  await localError(h, () => owner.openCommandPort(), "state"); assert.equal(h.channels.length, 0);
  await acknowledged(h, () => owner.finish());
  const polling = observe(() => owner.poll()), prior = h.last();
  await localError(h, () => owner.openCommandPort(), "busy"); assert.equal(h.channels.length, 0);
  h.reply(prior); assert.equal((await polling.result).ok, true);
  const pending = observe(() => owner.openCommandPort());
  const attachment = h.last(), channel = h.channels[0];
  assert.equal(attachment.kind, "attach-commands"); assert.equal(attachment.sequence, prior.sequence + 1);
  assert.equal(attachment.port, channel.port2);
  assert.deepEqual(h.sent.at(-1).transfers, [channel.port2]);
  await flush(); assert.equal(pending.settled, false, "port creation alone cannot prove processor adoption");
  await localError(h, () => owner.openCommandPort(), "busy");
  h.reply(attachment); const accepted = await pending.result; assert.equal(accepted.ok, true);
  const descriptor = accepted.value;
  assert.equal(descriptor.port, channel.port1); assert.equal(descriptor.generation, 17);
  assert.equal(descriptor.queueCapacity, 4); assert.equal(descriptor.timeoutMs, 50);
  assert.ok(Object.isFrozen(descriptor)); assert.equal(channel.port1.closes, 0);
  await localError(h, () => owner.commands([command()]), "state");
  await localError(h, () => owner.openCommandPort(), "state"); assert.equal(h.channels.length, 1);
  const actualReport = report(); actualReport.words[46] = 0xffffffff;
  const value = await acknowledged(h, () => owner.poll(), { report: actualReport });
  assert.equal(value.available, false); assert.equal(value.words[46], 0xffffffff);
  assert.equal(h.last().sequence, attachment.sequence + 1);
  await acknowledged(h, () => owner.arm(U64_MAX));
  await localError(h, () => owner.openCommandPort(), "state");
  await localError(h, () => owner.commands([command()]), "state");
  descriptor.port.close(); // The transferred client endpoint is now caller-owned.
  await stop(h, owner); assert.equal(channel.port1.closes, 1);
  assert.equal(h.sent.filter(item => item.message.kind === "commands").length, 0);
  for (const fault of [{ noMessageChannel: true }, { channelThrows: true }]) {
    const unsupported = await harness(fault), usable = await open(unsupported);
    await acknowledged(unsupported, () => usable.finish());
    await localError(unsupported, () => usable.openCommandPort(), fault.noMessageChannel ? "unsupported" : "transport");
    assert.equal(usable.state, "allocated");
    assert.equal((await acknowledged(unsupported, () => usable.commands([command()]))).admitted, 1);
    await stop(unsupported, usable);
  }
});

test("cancelled, refused and timed-out command attachments close unreturned endpoints without reclaiming command authority", async () => {
  const h = await harness(), owner = await open(h); await acknowledged(h, () => owner.finish());
  const transfer = observe(() => owner.openCommandPort()), attached = h.last();
  const stopping = observe(() => owner.stop()); await flush();
  failure(await transfer.result, "closed");
  assert.equal(h.channels[0].port1.closes, 1); assert.equal(h.channels[0].port2.closes, 1);
  const stopMessage = h.last(); assert.equal(stopMessage.kind, "stop");
  h.reply(attached); await flush(); assert.equal(stopping.settled, false);
  h.reply(stopMessage); assert.equal((await stopping.result).ok, true); assertClean(h);
  await localError(h, () => owner.commands([command()]), "closed");
  for (const failureKind of ["remote", "protocol", "timeout", "post"]) {
    const failed = await harness(failureKind === "post" ? { postThrowsAt: 2 } : {});
    const current = await open(failed); await acknowledged(failed, () => current.finish());
    const admission = observe(() => current.openCommandPort());
    if (failureKind === "remote") failed.reply(failed.last(), { status: 3, admitted: 0, error: "attachment refused" });
    else if (failureKind === "protocol") failed.reply(failed.last(), { admitted: 1 });
    else if (failureKind === "timeout") await failed.expire();
    const error = failure(await admission.result, failureKind === "post" ? "transport" : failureKind);
    assert.equal(failed.channels[0].port1.closes, 1); assert.equal(failed.channels[0].port2.closes, 1);
    assert.equal(current.state, "failed");
    const later = await localError(failed, () => current.commands([command()]), error.code);
    assert.equal(later, error);
    await stop(failed, current, "failed");
    assert.equal(failed.sent.filter(item => item.message.kind === "commands").length, 0);
  }
});

test("open resumes during the gesture, snapshots configuration, and requires exact ready evidence", async () => {
  const moduleGate = deferred();
  const h = await harness({ moduleGate });
  const configuration = options();
  const opening = observe(() => h.AudioHost.open(configuration));
  assert.deepEqual(h.trace.slice(0, 2).map(entry => entry[0]), ["context", "resume"]);
  assert.equal(h.contexts[0].resumes, 1);
  assert.equal(h.nodes.length, 0);
  configuration.generation = 99;
  configuration.channels = 1;
  configuration.pcmLimits.maxSamples = 999;
  configuration.audioLimits.maxFrames = 999;
  await flush();
  assert.equal(h.trace.findIndex(entry => entry[0] === "add-module") > 1, true);
  moduleGate.resolve();
  await flush();
  const node = h.nodes[0];
  assert.equal(node.name, "beatkernel-audio");
  assert.equal(node.configuration.numberOfInputs, 0);
  assert.equal(node.configuration.numberOfOutputs, 1);
  assert.deepEqual(Array.from(node.configuration.outputChannelCount), [2]);
  assert.equal(node.configuration.processorOptions.module, emptyModule);
  assert.equal(node.configuration.processorOptions.generation, 17);
  assert.equal(node.configuration.processorOptions.pcmLimits.maxSamples, 2);
  assert.equal(node.configuration.processorOptions.audioLimits.maxFrames, 5);
  assert.equal(node.connections[0], h.contexts[0].destination);
  assert.equal(h.trace.find(entry => entry[0] === "add-module")[1], new URL("./audio-worklet.js", sourceUrl).href);
  h.ready({ generation: 99 });
  await flush();
  assert.equal(opening.settled, false, "stale generation cannot satisfy readiness");
  h.ready();
  const opened = await opening.result;
  assert.equal(opened.ok, true);
  const owner = opened.value;
  assert.equal(owner.generation, 17);
  assert.equal(owner.channels, 2);
  assert.equal(owner.sampleRate, 48000);
  assert.equal(owner.state, "setup");
  await stop(h, owner);
});

test("context preferences are snapshotted before synchronous resume and actual sample rate remains authoritative", async () => {
  const defaults = await harness();
  const defaultOwner = await open(defaults);
  assert.deepEqual(structuredClone(defaults.trace.find(entry => entry[0] === "context")[1]), { latencyHint: "interactive" });
  await stop(defaults, defaultOwner);
  for (const contextOptions of [{}, { sampleRate: 48000 }, { latencyHint: undefined, sampleRate: undefined }]) {
    const compatible = await harness();
    const owner = await open(compatible, options({ contextOptions }));
    assert.deepEqual(structuredClone(compatible.trace.find(entry => entry[0] === "context")[1]), {
      latencyHint: "interactive", ...(contextOptions.sampleRate === undefined ? {} : { sampleRate: contextOptions.sampleRate }),
    });
    await stop(compatible, owner);
  }
  const resumed = deferred();
  const h = await harness({ resumeGate: resumed, sampleRate: 44100 });
  const request = { latencyHint: 0.012345678, sampleRate: 96000 };
  const opening = observe(() => h.AudioHost.open(options({ contextOptions: request })));
  assert.deepEqual(h.trace.slice(0, 2).map(entry => entry[0]), ["context", "resume"]);
  const passed = h.trace.find(entry => entry[0] === "context")[1];
  assert.notEqual(passed, request);
  assert.ok(Object.isFrozen(passed));
  request.latencyHint = "playback"; request.sampleRate = 8000;
  assert.deepEqual(structuredClone(passed), { latencyHint: 0.012345678, sampleRate: 96000 });
  await flush();
  h.ready();
  await flush();
  assert.equal(opening.settled, false, "ready does not substitute for the still-pending real resume");
  resumed.resolve();
  const result = await opening.result;
  assert.equal(result.ok, true, result.error?.message);
  assert.equal(result.value.sampleRate, 44100, "requested rate is not evidence of the context's actual output grid");
  assert.equal(h.contexts.length, 1);
  await stop(h, result.value);
  for (const contextOptions of [{ latencyHint: "balanced" }, { latencyHint: "playback", sampleRate: 1 },
    { latencyHint: 0 }, { latencyHint: 60, sampleRate: 4294967295 }]) {
    const accepted = await harness();
    const owner = await open(accepted, options({ contextOptions }));
    assert.deepEqual(structuredClone(accepted.trace.find(entry => entry[0] === "context")[1]), contextOptions);
    await stop(accepted, owner);
  }
});

test("AudioHost independently rejects invalid context options before acquisition and never retries a refused constructor", async () => {
  for (const contextOptions of [null, [], "interactive", { renderSizeHint: 128 }, { latencyHint: "custom" }, { latencyHint: "interactive\n" },
    { latencyHint: null }, { latencyHint: "0.01" }, { latencyHint: NaN }, { latencyHint: Infinity },
    { latencyHint: -0.000001 }, { latencyHint: 60.000001 }, { latencyHint: "balanced", sampleRate: 0 },
    { latencyHint: "interactive", sampleRate: "48000" }, { latencyHint: 0, sampleRate: 1.5 },
    { latencyHint: 0, sampleRate: 4294967296 }, { latencyHint: 0, sampleRate: null }]) {
    const h = await harness();
    const error = failure(await observe(() => h.AudioHost.open(options({ contextOptions }))).result, "validation");
    assert.ok(error instanceof h.AudioHostError);
    assert.equal(error.operation, "open");
    assert.equal(h.trace.some(entry => entry[0] === "context"), false);
    assert.equal(h.nodes.length, 0);
    assertClean(h);
  }
  const refused = await harness({ contextThrows: true });
  const result = await observe(() => refused.AudioHost.open(options({ contextOptions: { latencyHint: 0.001, sampleRate: 12345 } }))).result;
  failure(result, "transport");
  const attempts = refused.trace.filter(entry => entry[0] === "context");
  assert.equal(attempts.length, 1);
  assert.deepEqual(structuredClone(attempts[0][1]), { latencyHint: 0.001, sampleRate: 12345 });
  assert.equal(refused.trace.some(entry => ["resume", "add-module", "node"].includes(entry[0])), false);
  assertClean(refused);
});

test("successful setup transfers exact PCM ownership and preserves command widths and genuine reports", async () => {
  const h = await harness();
  const owner = await open(h);
  const first = sample();
  const backing = first.pcm.buffer;
  const pending = observe(() => owner.sample(first));
  assert.equal(first.pcm.byteLength, 0, "sample admission transfers the standalone backing");
  assert.equal(h.sent[0].transfers[0], backing);
  assert.equal(h.last().rate, 44100, "source rate must not be rewritten to context rate");
  assert.deepEqual(Array.from(h.last().pcm), [0.25, -0.25, 0.5, -0.5]);
  h.reply(h.last());
  assert.equal((await pending.result).ok, true);
  await acknowledged(h, () => owner.sample(sample({ id: 2n, rate: 96000, pcm: new Float32Array([1, -1]) })));
  const unavailable = await acknowledged(h, () => owner.poll());
  assert.equal(unavailable.available, false);
  assert.deepEqual(Array.from(unavailable.words), Array(58).fill(0));
  await acknowledged(h, () => owner.finish());
  assert.equal(owner.state, "allocated");
  const frame = 9007199254741999n;
  await acknowledged(h, () => owner.arm(frame));
  assert.equal(h.last().frame, frame);
  assert.equal(owner.state, "armed");
  const commands = [0, 1, 2, 3].map(kind => command({ kind }));
  const ack = await acknowledged(h, () => owner.commands(commands));
  assert.equal(ack.admitted, 4);
  assert.deepEqual(structuredClone(h.last().commands), commands);
  const evidence = report(true);
  evidence.words[3] = 0xffffffff;
  evidence.words[21] = 0x80000000;
  evidence.words[48] = 1234567890;
  evidence.words[49] = 0x100000;
  const actual = await acknowledged(h, () => owner.poll(), { report: evidence });
  assert.equal(actual.available, true);
  assert.deepEqual(Array.from(actual.words), Array.from(evidence.words));
  assert.deepEqual(h.sent.map(entry => entry.message.sequence), [1, 2, 3, 4, 5, 6, 7]);
  assert.ok(h.sent.every(entry => entry.message.generation === 17));
  await stop(h, owner);
});

test("finish preserves the unlimited wire shape and transmits finite endpoints as exact optional u64 values", async () => {
  for (const args of [[], [undefined], [0n], [1n], [9007199254740993n], [U64_MAX]]) {
    const h = await harness();
    const owner = await open(h);
    const pending = observe(() => owner.finish(...args));
    const sent = h.last();
    assert.equal(sent.kind, "finish");
    assert.equal(sent.generation, 17);
    assert.equal(sent.sequence, 1);
    assert.equal(Object.hasOwn(sent, "endFrame"), args[0] !== undefined);
    if (args[0] !== undefined) assert.equal(sent.endFrame, args[0]);
    else assert.deepEqual(Object.keys(sent).sort(), ["generation", "kind", "sequence"]);
    assert.equal(h.sent[0].transfers.length, 0);
    assert.equal(owner.state, "setup", "a submitted endpoint is not successful allocation evidence");
    await localError(h, () => owner.finish(2n), "busy");
    assert.equal(pending.settled, false);
    h.reply(sent);
    const result = await pending.result;
    assert.equal(result.ok, true);
    assert.equal(result.value.admitted, 0);
    assert.equal(owner.state, "allocated");
    await localError(h, () => owner.finish(2n), "state");
    await acknowledged(h, () => owner.arm(0n));
    assert.equal(owner.state, "armed");
    assert.deepEqual(h.sent.map(entry => entry.message.kind), ["finish", "arm"]);
    await stop(h, owner);
  }
});

test("finish endpoint validation leaves setup reusable while remote or malformed ACK failure never retries unlimited output", async () => {
  const local = await harness();
  const reusable = await open(local);
  for (const end of [null, 0, 1, "1", -1n, U64_MAX + 1n, 1.5, Infinity, NaN, {}, []]) {
    const error = await localError(local, () => reusable.finish(end));
    assert.equal(error.operation, "finish");
    assert.equal(reusable.state, "setup");
  }
  await acknowledged(local, () => reusable.finish(0n));
  assert.equal(local.last().sequence, 1, "invalid endpoints consume neither control slots nor sequence identities");
  assert.equal(local.last().endFrame, 0n);
  await stop(local, reusable);

  for (const [reply, code] of [[{ status: 9, admitted: 0, error: "audio" }, "remote"],
    [{ status: 0, admitted: 1 }, "protocol"]]) {
    const h = await harness();
    const owner = await open(h);
    const pending = observe(() => owner.finish(9007199254740993n));
    const sent = h.last();
    h.reply(sent, reply);
    const original = failure(await pending.result, code);
    if (code === "remote") {
      assert.equal(original.operation, "finish");
      assert.equal(original.sequence, sent.sequence);
      assert.equal(original.status, 9);
      assert.equal(original.admitted, 0);
    }
    await flush();
    assert.equal(h.sent.filter(entry => entry.message.kind === "finish").length, 1);
    assert.equal(sent.endFrame, 9007199254740993n);
    assert.equal(h.last().kind, "stop");
    assert.equal(await localError(h, () => owner.finish(), code), original);
    await stop(h, owner, "failed");
  }
});

test("currentFrame is a checked context-time estimate independent of command and render evidence", async () => {
  const h = await harness();
  const owner = await open(h);
  const context = h.contexts[0];
  assert.equal(owner.currentFrame, 0n);
  context.currentTime = 1.000005;
  assert.equal(owner.currentFrame, 48000n);
  context.currentTime = (4294967296 + 128.75) / context.sampleRate;
  assert.equal(owner.currentFrame, 4294967424n);
  assert.equal(h.sent.length, 0, "reading a scheduling estimate cannot manufacture a report");
  const pending = observe(() => owner.poll());
  assert.equal(owner.currentFrame, 4294967424n, "a pending control does not invalidate context time");
  h.reply(h.last());
  assert.equal((await pending.result).ok, true);
  for (const value of [-1, NaN, Infinity, "1", null, 2 * Number.MAX_SAFE_INTEGER / context.sampleRate]) {
    context.currentTime = value;
    await localError(h, () => owner.currentFrame, "state");
    assert.equal(owner.state, "setup");
  }
  context.currentTime = 0;
  assert.equal(owner.currentFrame, 0n);
  await stop(h, owner);
  await localError(h, () => owner.currentFrame, "state");
});

test("control clock snapshots Window brackets while outputTimestamp returns separate native evidence", async () => {
  const h = await harness();
  const owner = await open(h);
  const context = h.contexts[0];
  context.currentTime = 2.5;
  h.faults.performanceTimes = [123456.125, 123456.375];
  assert.deepEqual(structuredClone(owner.controlClock()), {
    beforeMs: 123456.125, contextTime: 2.5, afterMs: 123456.375, sampleRate: 48000,
  });
  assert.equal(h.sent.length, 0);
  assert.equal(context.outputReads, 0, "a control snapshot is not presentation evidence");
  for (const bracket of [[2, 1], [-1, 1], [1, Infinity], [NaN, 2]]) {
    h.faults.performanceTimes = [...bracket];
    await localError(h, () => owner.controlClock(), "state");
  }
  for (const evidence of [{ contextTime: 0, performanceTime: 10 },
    { contextTime: 1, performanceTime: 0 }, { contextTime: 0, performanceTime: 0 }]) {
    h.faults.outputEvidence = evidence;
    await localError(h, () => owner.outputTimestamp(), "unavailable");
  }
  for (const evidence of [null, {}, { contextTime: -1, performanceTime: 1 },
    { contextTime: 3, performanceTime: 1 },
    { contextTime: 1, performanceTime: NaN }, { contextTime: "1", performanceTime: 1 }]) {
    h.faults.outputEvidence = evidence;
    await localError(h, () => owner.outputTimestamp(), "state");
  }
  const evidence = { contextTime: 2.25, performanceTime: 123400.5 };
  h.faults.outputEvidence = evidence;
  const captured = owner.outputTimestamp();
  assert.deepEqual(structuredClone(captured), evidence);
  evidence.contextTime = 99;
  assert.equal(captured.contextTime, 2.25);
  h.faults.outputEvidence = { contextTime: 2.5, performanceTime: 123400.5 };
  assert.equal(owner.outputTimestamp().contextTime, context.currentTime);
  const reads = context.outputReads;
  context.getOutputTimestamp = undefined;
  await localError(h, () => owner.outputTimestamp(), "unsupported");
  assert.equal(context.outputReads, reads);
  await stop(h, owner);
  await localError(h, () => owner.controlClock(), "state");
  await localError(h, () => owner.outputTimestamp(), "state");
});

test("sample preflight rejects unsafe buffers and bounds without posting, and overlap never builds a queue", async () => {
  const h = await harness();
  const owner = await open(h);
  const detached = new Float32Array(0);
  structuredClone(detached.buffer, { transfer: [detached.buffer] });
  for (const invalid of [
    sample({ id: 1 }), sample({ id: -1n }), sample({ id: U64_MAX + 1n }),
    sample({ rate: 0 }), sample({ rate: 4294967296 }), sample({ channels: 1 }),
    sample({ pcm: new Uint8Array(4) }), sample({ pcm: new Float32Array(3) }),
    sample({ pcm: new Float32Array(6) }),
    sample({ pcm: new Float32Array(new ArrayBuffer(24), 4, 4) }),
    sample({ pcm: new Float32Array(new SharedArrayBuffer(16)) }),
    sample({ pcm: detached }),
  ]) {
    const bytes = invalid.pcm.byteLength;
    await localError(h, () => owner.sample(invalid));
    assert.equal(invalid.pcm.byteLength, bytes, "rejection cannot detach caller data");
    assert.equal(owner.state, "setup");
  }
  const pending = observe(() => owner.sample(sample()));
  const first = h.last();
  await localError(h, () => owner.poll(), "busy");
  await localError(h, () => owner.finish(), "busy");
  await localError(h, () => owner.sample(sample({ id: 2n })), "busy");
  assert.equal(h.sent.length, 1);
  h.reply(first);
  assert.equal((await pending.result).ok, true);
  await localError(h, () => owner.sample(sample()), "validation");
  await localError(h, () => owner.sample(sample({ id: 2n })), "validation");
  await acknowledged(h, () => owner.sample(sample({ id: 2n, pcm: new Float32Array(2) })));
  await localError(h, () => owner.sample(sample({ id: 3n, pcm: new Float32Array(0) })), "validation");
  assert.deepEqual(h.sent.map(entry => entry.message.sequence), [1, 2]);
  await stop(h, owner);
});

test("nonfinite PCM transfers once and only the correlated remote refusal fences admission and joins cleanup", async () => {
  for (const value of [NaN, Infinity, -Infinity]) {
    const closing = deferred();
    const h = await harness({ closeGate: closing });
    const owner = await open(h);
    await acknowledged(h, () => owner.sample(sample()));
    const invalid = sample({ id: 2n, pcm: new Float32Array([0.25, value]) });
    const backing = invalid.pcm.buffer;
    const pending = observe(() => owner.sample(invalid));
    assert.equal(invalid.pcm.byteLength, 0, "the original nonfinite backing has transferred");
    const upload = h.last();
    assert.equal(upload.kind, "sample"); assert.equal(upload.id, 2n);
    assert.equal(upload.sequence, 2); assert.equal(upload.generation, 17);
    assert.equal(h.sent.at(-1).transfers[0], backing);
    assert.equal(upload.pcm[0], 0.25); assert.ok(Object.is(upload.pcm[1], value));
    await flush(); assert.equal(pending.settled, false, "transfer is not sample admission evidence");
    h.reply(upload, { generation: 16 });
    await flush(); assert.equal(pending.settled, false, "stale success cannot commit sample accounting");
    // This is an explicit remote ACK boundary, not a second host-side PCM validator.
    h.reply(upload, { status: 100, admitted: 0, error: "sample-pcm" });
    h.nodes[0].port.emit("message", { kind: "terminal", generation: 17, status: 100 });
    const error = failure(await pending.result, "remote");
    assert.equal(error.operation, "sample"); assert.equal(error.sequence, 2);
    assert.equal(error.generation, 17); assert.equal(error.status, 100); assert.equal(error.admitted, 0);
    assert.equal(owner.state, "failed");
    await localError(h, () => owner.sample(invalid), "remote");
    await localError(h, () => owner.finish(), "remote");
    assert.equal(h.sent.filter(entry => entry.message.kind === "sample").length, 2, "consumed PCM is never retried");
    const joined = owner.stop(); assert.equal(owner.stop(), joined);
    const waiting = observe(() => joined);
    await flush(); assert.equal(h.last().kind, "stop"); assert.equal(h.last().sequence, 3);
    h.reply(h.last()); await flush();
    assert.equal(h.contexts[0].closes, 1);
    assert.equal(waiting.settled, false, "stop ACK does not substitute for context close completion");
    closing.resolve(); assert.equal((await waiting.result).ok, true);
    assert.equal(owner.stop(), joined); assert.equal(owner.state, "failed");
    assert.equal(invalid.pcm.byteLength, 0); assertClean(h);
  }
});

test("valid empty PCM retains exclusive transfer ownership and consumes identity capacity only after its ACK", async () => {
  const h = await harness();
  const owner = await open(h, options({ pcmLimits: { maxTotalBytes: 16 } }));
  const empty = sample({ pcm: new Float32Array(0) });
  const backing = empty.pcm.buffer;
  const pending = observe(() => owner.sample(empty));
  assert.equal(h.last().pcm.length, 0); assert.equal(h.sent[0].transfers[0], backing);
  assert.throws(() => new Float32Array(backing, 0, 0), TypeError, "empty backing is genuinely detached");
  await localError(h, () => owner.sample(sample()), "busy");
  h.reply(h.last()); assert.equal((await pending.result).ok, true);
  await localError(h, () => owner.sample(sample({ pcm: new Float32Array(0) })), "validation");
  await acknowledged(h, () => owner.sample(sample({ id: 2n })));
  await localError(h, () => owner.sample(sample({ id: 3n, pcm: new Float32Array(0) })), "validation");
  assert.deepEqual(h.sent.map(entry => entry.message.id), [1n, 2n]);
  await acknowledged(h, () => owner.finish());
  await stop(h, owner);
});

test("command batches preflight every record atomically and state transitions remain one-shot", async () => {
  const h = await harness();
  const owner = await open(h);
  await localError(h, () => owner.commands([command()]), "state");
  await localError(h, () => owner.arm(1n), "state");
  await acknowledged(h, () => owner.finish());
  await localError(h, () => owner.finish(), "state");
  await localError(h, () => owner.sample(sample()), "state");
  const missing = command();
  delete missing.value;
  const inherited = Object.create({ denominator: 1n });
  Object.assign(inherited, missing, { value: 0n });
  delete inherited.denominator;
  for (const invalid of [
    null, missing, inherited, command({ extra: true }), command({ kind: 4 }),
    command({ voice: 1 }), command({ voice: -1n }), command({ sample: U64_MAX + 1n }),
    command({ at: I64_MIN - 1n }), command({ value: I64_MAX + 1n }),
    command({ gain: Infinity }), command({ gain: NaN }), command({ gain: 1e40 }),
    command({ denominator: -1n }),
  ]) await localError(h, () => owner.commands([command(), invalid]));
  for (const batch of [[], [command(), command(), command(), command(), command()]]) {
    await localError(h, () => owner.commands(batch));
  }
  for (const frame of [-1n, U64_MAX + 1n, 100]) await localError(h, () => owner.arm(frame));
  await acknowledged(h, () => owner.arm(U64_MAX));
  await localError(h, () => owner.arm(U64_MAX), "state");
  assert.equal(owner.state, "armed");
  assert.deepEqual(h.sent.map(entry => entry.message.kind), ["finish", "arm"]);
  await stop(h, owner);
});

test("remote command failure exposes the exact admitted prefix, fences, and never retries", async () => {
  for (const [status, admitted] of [[3, 0], [3, 1], [106, 2]]) {
    const h = await harness();
    const owner = await open(h);
    await acknowledged(h, () => owner.finish());
    const pending = observe(() => owner.commands([command(), command({ kind: 1 }), command({ kind: 2 })]));
    const sent = h.last();
    h.reply(sent, { status, admitted, error: "admission" });
    h.nodes[0].port.emit("message", diagnosticTerminal({ status }));
    const error = failure(await pending.result, "remote");
    assert.ok(error instanceof h.AudioHostError);
    assert.equal(error.operation, "commands");
    assert.equal(error.generation, 17);
    assert.equal(error.sequence, sent.sequence);
    assert.equal(error.status, status);
    assert.equal(error.admitted, admitted);
    await flush();
    assert.equal(h.sent.filter(entry => entry.message.kind === "commands").length, 1);
    const later = await localError(h, () => owner.poll(), "remote");
    assert.equal(later.admitted, admitted);
    assert.equal(later.sequence, sent.sequence);
    assert.equal(h.last().kind, "stop");
    assert.equal(h.last().sequence, sent.sequence + 1);
    await stop(h, owner, "failed");
  }
});

test("stale generations are ignored while malformed correlation or report evidence fences", async () => {
  const h = await harness();
  const owner = await open(h);
  const pending = observe(() => owner.poll());
  const sent = h.last();
  h.reply(sent, { generation: 16 });
  h.nodes[0].port.emit("message", { kind: "terminal", generation: 16, status: 8 });
  await flush();
  assert.equal(pending.settled, false);
  assert.equal(h.sent.length, 1);
  h.reply(sent);
  assert.equal((await pending.result).ok, true);
  h.reply(sent);
  await flush();
  await localError(h, () => owner.poll(), "protocol");
  await stop(h, owner, "failed");

  const highFlag = report(true);
  highFlag.words[1] = 1;
  const nonBooleanFlag = report();
  nonBooleanFlag.words[0] = 2;
  const detached = new Uint32Array(58);
  structuredClone(detached.buffer, { transfer: [detached.buffer] });
  const resizable = new ArrayBuffer(232, { maxByteLength: 464 });
  for (const fields of [
    { sequence: 2 }, { operation: "finish" }, { status: "0" }, { admitted: 1 },
    { kind: "unknown" }, { report: null },
    { report: { available: false, words: new Uint32Array(55) } },
    { report: { available: false, words: new Uint32Array(56) } },
    { report: { available: false, words: new Uint32Array(59) } },
    { report: { available: false, words: new Uint32Array(new ArrayBuffer(236), 0, 58) } },
    { report: { available: false, words: new Uint32Array(new ArrayBuffer(236), 4, 58) } },
    { report: { available: false, words: new Uint32Array(new SharedArrayBuffer(232)) } },
    { report: { available: false, words: new Uint32Array(resizable, 0, 58) } },
    { report: { available: false, words: detached } },
    { report: { available: false, words: new Int32Array(58) } },
    { report: { available: true, words: new Uint32Array(58) } },
    { report: highFlag }, { report: nonBooleanFlag },
  ]) {
    const invalid = await harness();
    const live = await open(invalid);
    const operation = observe(() => live.poll());
    invalid.reply(invalid.last(), fields);
    failure(await operation.result, "protocol");
    await flush();
    assert.equal(invalid.last().kind, "stop");
    await stop(invalid, live, "failed");
  }
  for (const fields of [{ admitted: 0 }, { status: 3, admitted: 2, error: "admission" }]) {
    const invalid = await harness();
    const live = await open(invalid);
    await acknowledged(invalid, () => live.finish());
    const operation = observe(() => live.commands([command()]));
    invalid.reply(invalid.last(), fields);
    failure(await operation.result, "protocol");
    await stop(invalid, live, "failed");
  }
});

test("processor, message, terminal and posting failures reject pending work and release the owner", async () => {
  for (const [kind, code] of [["processorerror", "processor"], ["messageerror", "message"], ["terminal", "processor"], ["post", "transport"]]) {
    const h = await harness();
    const owner = await open(h);
    if (kind === "post") h.faults.postThrowsAt = 1;
    const pending = observe(() => owner.poll());
    if (kind === "processorerror") h.nodes[0].emit(kind, {});
    if (kind === "messageerror") h.nodes[0].port.emit(kind, {});
    if (kind === "terminal") h.nodes[0].port.emit("message", { kind, generation: 17, status: 8 });
    failure(await pending.result, code);
    await flush();
    assert.equal(h.last().kind, "stop");
    assert.equal(h.sent.filter(entry => entry.message.kind === "stop").length, 1);
    await stop(h, owner, "failed");
  }
});

test("rejected ACK diagnostics retain bounded descriptor and exact first cause across terminal and cleanup", async () => {
  for (const descriptor of [null, "", "queue-admission", "x".repeat(4096), "x".repeat(8192)]) {
    const h = await harness(), owner = await open(h);
    await acknowledged(h, () => owner.finish());
    const pending = observe(() => owner.commands([command(), command({ kind: 1 })]));
    const request = h.last();
    h.reply(request, { status: 4294967295, admitted: 1, error: descriptor });
    const original = failure(await pending.result, "remote");
    assert.equal(original.operation, "commands"); assert.equal(original.status, 4294967295);
    assert.equal(original.generation, 17); assert.equal(original.sequence, request.sequence);
    const boundedDescriptor = descriptor === null ? null : descriptor.slice(0, 4096);
    assert.equal(original.admitted, 1); assert.equal(original.remoteError, boundedDescriptor);
    assert.ok(original.message.includes("4294967295"));
    if (boundedDescriptor !== null && boundedDescriptor.length > 0) {
      assert.ok(original.message.includes(boundedDescriptor));
      assert.ok(original.message.indexOf("4294967295") < original.message.indexOf(boundedDescriptor));
      if (descriptor.length > 4096) assert.equal(original.message.includes(descriptor), false);
    }
    h.nodes[0].port.emit("message", { kind: "terminal", generation: 17, status: 8 });
    assert.equal(await localError(h, () => owner.poll(), "remote"), original);
    await stop(h, owner, "failed");
    assert.equal(await localError(h, () => owner.poll(), "remote"), original);
    assert.equal(original.remoteError, boundedDescriptor); assert.equal(original.status, 4294967295);
    assert.equal(h.sent.filter(row => row.message.kind === "commands").length, 1);
    assert.equal(h.sent.filter(row => row.message.kind === "stop").length, 1);
  }
});

test("terminal diagnostics expose the validated original numeric cause without inventing ACK evidence", async () => {
  const h = await harness(), owner = await open(h);
  const pending = observe(() => owner.poll());
  h.nodes[0].port.emit("message", { kind: "terminal", generation: 16, status: "unvalidated" });
  await flush(); assert.equal(pending.settled, false); assert.equal(h.sent.length, 1);
  h.nodes[0].port.emit("message", { kind: "terminal", generation: 17, status: 4294967295 });
  const original = failure(await pending.result, "processor");
  assert.equal(original.operation, "render"); assert.equal(original.generation, 17);
  assert.equal(original.status, 4294967295); assert.equal(original.sequence, null);
  assert.equal(original.admitted, null); assert.equal(original.remoteError, null);
  assert.ok(original.message.includes("4294967295"));
  h.nodes[0].port.emit("message", { kind: "terminal", generation: 17, status: 2 });
  assert.equal(await localError(h, () => owner.poll(), "processor"), original);
  await stop(h, owner, "failed");
  assert.equal(await localError(h, () => owner.poll(), "processor"), original);
  assert.equal(h.sent.filter(row => row.message.kind === "poll").length, 1);
  assert.equal(h.sent.filter(row => row.message.kind === "stop").length, 1);
});

test("diagnostic propagation cannot admit malformed descriptors or terminal statuses", async () => {
  for (const fields of [
    { kind: "terminal", status: 0 }, { kind: "terminal", status: "8" },
    { kind: "terminal", status: 4294967296 },
    { kind: "ack", error: { message: "unvalidated" } },
  ]) {
    const h = await harness(), owner = await open(h);
    await acknowledged(h, () => owner.finish());
    const pending = observe(() => owner.commands([command()]));
    const request = h.last();
    h.reply(request, { generation: 16, status: 8, admitted: 0, ...fields });
    await flush(); assert.equal(pending.settled, false);
    assert.equal(h.sent.filter(row => row.message.kind === "stop").length, 0);
    h.reply(request, { status: 8, admitted: 0, ...fields });
    const original = failure(await pending.result, "protocol");
    assert.equal(original.remoteError, null);
    assert.equal(await localError(h, () => owner.poll(), "protocol"), original);
    await stop(h, owner, "failed");
    assert.equal(h.sent.filter(row => row.message.kind === "commands").length, 1);
  }
});

test("operation and shutdown deadlines remain finite even when ACK or close never settles", async () => {
  const h = await harness();
  const owner = await open(h);
  const pending = observe(() => owner.poll());
  await h.expire();
  const original = failure(await pending.result, "timeout");
  assert.equal(original.operation, "poll");
  assert.equal(original.admitted, null);
  assert.equal(h.last().kind, "stop");
  const cleanup = observe(() => owner.stop());
  await h.expire();
  failure(await cleanup.result, "timeout");
  assert.equal(original.operation, "poll", "cleanup cannot replace the original failure");
  assertClean(h);

  const closeGate = deferred();
  const stalled = await harness({ closeGate });
  const live = await open(stalled);
  const stopping = observe(() => live.stop());
  stalled.reply(stalled.last());
  await flush();
  assert.equal(stalled.contexts[0].closes, 1);
  assert.equal(stopping.settled, false);
  await stalled.expire();
  failure(await stopping.result, "timeout");
  assertClean(stalled);

  for (const broken of ["stop-ack", "close"]) {
    const failing = await harness(broken === "close" ? { closeThrows: true } : {});
    const active = await open(failing);
    const first = active.stop();
    const cleanup = observe(() => first);
    failing.reply(failing.last(), broken === "stop-ack" ? { status: 106, error: "exception" } : {});
    failure(await cleanup.result, broken === "stop-ack" ? "remote" : "transport");
    assert.equal(active.stop(), first);
    assertClean(failing);
  }
});

test("stop cancels setup work, tolerates its valid late ACK, and shares one cleanup promise", async () => {
  const h = await harness();
  const owner = await open(h);
  const admission = observe(() => owner.sample(sample()));
  const canceled = h.last();
  const first = owner.stop();
  const stopping = observe(() => first);
  assert.equal(owner.stop(), first);
  h.contexts[0].state = "interrupted";
  h.contexts[0].emit("statechange", {});
  failure(await admission.result, "closed");
  await flush();
  const fresh = h.last();
  assert.equal(fresh.kind, "stop");
  assert.equal(fresh.sequence, canceled.sequence + 1);
  h.reply(canceled);
  await flush();
  assert.equal(stopping.settled, false, "old valid ACK cannot finish or fail fresh stop");
  h.reply(fresh);
  assert.equal((await stopping.result).ok, true);
  assert.equal(owner.stop(), first);
  await localError(h, () => owner.finish(), "closed");
  assertClean(h);
});

test("invalid options and aborted setup reject before resources, and abort ownership ends after ready", async () => {
  for (const configuration of [
    options({ module: {} }), options({ generation: 0 }), options({ channels: 0 }),
    options({ timeoutMs: 0 }), options({ timeoutMs: 60001 }), options({ timeoutMs: NaN }),
    options({ pcmLimits: { maxAssetBytes: 25 } }), options({ pcmLimits: { maxSamples: 0 } }),
    options({ audioLimits: { queueCapacity: 0 } }), options({ audioLimits: { maxFrames: 1048577 } }),
  ]) {
    const h = await harness();
    failure(await observe(() => h.AudioHost.open(configuration)).result, "validation");
    assert.equal(h.contexts.length, 0);
    assert.equal(h.timers.size, 0);
  }
  const early = new AbortController();
  early.abort();
  const h = await harness();
  failure(await observe(() => h.AudioHost.open(options({ signal: early.signal }))).result, "aborted");
  assert.equal(h.contexts.length, 0);
  const moduleGate = deferred();
  const waiting = await harness({ moduleGate });
  const controller = new AbortController();
  const opening = observe(() => waiting.AudioHost.open(options({ signal: controller.signal })));
  controller.abort();
  failure(await opening.result, "aborted");
  assertClean(waiting);
  moduleGate.resolve();
  await flush();
  assert.equal(waiting.nodes.length, 0, "late module completion cannot recreate canceled ownership");

  const accepted = await harness();
  const after = new AbortController();
  const owner = await open(accepted, options({ signal: after.signal }));
  after.abort();
  await acknowledged(accepted, () => owner.poll());
  await stop(accepted, owner);
});

test("opening failures and one shared setup deadline clean every partially created resource", async () => {
  for (const faults of [
    { contextThrows: true }, { resumeThrows: true }, { moduleThrows: true },
    { nodeThrows: true }, { connectThrows: true }, { unsupported: true },
  ]) {
    const h = await harness(faults);
    const opening = observe(() => h.AudioHost.open(options()));
    await flush();
    for (let turn = 0; !opening.settled && turn < 4; turn++) await h.expire();
    failure(await opening.result);
    assertClean(h);
  }
  for (const stage of ["resume", "module"]) {
    const gate = deferred();
    const h = await harness(stage === "resume" ? { resumeGate: gate } : { moduleGate: gate });
    const opening = observe(() => h.AudioHost.open(options()));
    await flush();
    gate.reject(new Error(`asynchronous ${stage} failure`));
    await flush();
    for (let turn = 0; !opening.settled && turn < 4; turn++) await h.expire();
    failure(await opening.result, "transport");
    assertClean(h);
  }
  for (const stalledStage of ["resume", "module", "ready"]) {
    const gate = deferred();
    const h = await harness(stalledStage === "resume" ? { resumeGate: gate }
      : stalledStage === "module" ? { moduleGate: gate } : {});
    const opening = observe(() => h.AudioHost.open(options()));
    await flush();
    await h.expire();
    // If the processor exists, let its cleanup time out too; no fabricated ready/ACK.
    for (let turn = 0; !opening.settled && turn < 4; turn++) await h.expire();
    failure(await opening.result, "timeout");
    assertClean(h);
    gate.resolve();
    await flush();
    assertClean(h);
  }
  const moduleGate = deferred();
  const h = await harness({ moduleGate });
  const opening = observe(() => h.AudioHost.open(options()));
  await flush();
  h.advanceBeforeDeadline(40);
  moduleGate.resolve();
  await flush();
  assert.equal(h.nodes.length, 1);
  await h.expire();
  assert.equal(h.now, 50, "module completion must not renew the original setup deadline");
  for (let turn = 0; !opening.settled && turn < 4; turn++) await h.expire();
  failure(await opening.result, "timeout");
  assertClean(h);
});

test("failed or cancelled open preserves its original error and exposes failed close evidence", async () => {
  for (const cancelled of [false, true]) {
    const moduleGate = deferred();
    const closeGate = deferred();
    const controller = new AbortController();
    const h = await harness({ moduleGate, closeGate });
    const opening = observe(() => h.AudioHost.open(options({ signal: controller.signal })));
    await flush();
    const setupCause = new Error("original module acquisition failure");
    if (cancelled) controller.abort();
    else moduleGate.reject(setupCause);
    await flush();
    assert.equal(h.nodes.length, 0);
    assert.equal(h.contexts[0].closes, 1);
    assert.equal(opening.settled, false, "open must await its context cleanup outcome");

    const closeCause = new Error("actual context close rejection");
    if (cancelled) await h.expire();
    else closeGate.reject(closeCause);
    const error = failure(await opening.result, cancelled ? "aborted" : "transport");
    assert.equal(error.operation, "open");
    assert.equal(error.generation, 17);
    assert.equal(error.status, null);
    assert.equal(error.admitted, null);
    if (cancelled) assert.match(error.message, /cancelled/i);
    else {
      assert.equal(error.cause, setupCause, "cleanup cannot replace the original module failure");
      assert.match(error.message, /module loading failed/i);
    }
    const cleanup = error.cleanupError;
    assert.ok(cleanup instanceof h.AudioHostError);
    assert.notEqual(cleanup, error);
    assert.equal(cleanup.code, cancelled ? "timeout" : "transport");
    assert.equal(cleanup.operation, "close");
    assert.equal(cleanup.generation, 17);
    if (!cancelled) assert.equal(cleanup.cause, closeCause);
    else assert.match(cleanup.message, /close timed out/i);
    assertClean(h);

    // Settling abandoned browser promises cannot erase recorded cleanup failure
    // or construct a late processor after the rejected opening has returned.
    moduleGate.resolve();
    closeGate.resolve();
    await flush();
    assert.equal(error.cleanupError, cleanup);
    assert.equal(h.nodes.length, 0);
    assertClean(h);
  }
});

test("ready must match actual context rate and channels before an owner becomes usable", async () => {
  for (const fields of [{ sampleRate: 44100 }, { channels: 1 }, { sampleRate: NaN }]) {
    const h = await harness();
    const opening = observe(() => h.AudioHost.open(options()));
    await flush();
    h.ready(fields);
    await flush();
    const stopMessage = h.sent.find(entry => entry.message.kind === "stop")?.message;
    if (stopMessage) h.reply(stopMessage);
    failure(await opening.result, "protocol");
    assertClean(h);
  }
});

test("a fulfilled resume is insufficient when the context is not running, and later interruption fences", async () => {
  const initial = await harness({ resumeState: "suspended" });
  const opening = observe(() => initial.AudioHost.open(options()));
  await flush();
  assert.equal(opening.settled, false, "opening statechanges do not bypass the ready handshake");
  initial.ready();
  await flush();
  assert.equal(initial.last().kind, "stop");
  initial.reply(initial.last());
  const rejected = failure(await opening.result, "state");
  assert.equal(rejected.operation, "open");
  assertClean(initial);

  for (const state of ["suspended", "interrupted", "closed"]) {
    const h = await harness();
    const owner = await open(h);
    const pending = observe(() => owner.poll());
    h.contexts[0].state = state;
    h.contexts[0].emit("statechange", {});
    const error = failure(await pending.result, "state");
    assert.equal(error.operation, "context");
    assert.equal(error.admitted, null);
    await flush();
    assert.equal(h.last().kind, "stop");
    const retained = await localError(h, () => owner.currentFrame, "state");
    assert.equal(retained.operation, "context");
    await stop(h, owner, "failed");
  }
});
