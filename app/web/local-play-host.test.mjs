// Deferred source fixtures for the actual page roster and final capture boundary.
import assert from "node:assert/strict";
import test from "node:test";
import { LocalRoster, validateLocalPrepared, localReplayReceipt } from "./local-play-host.mjs";

const MAX = 18446744073709551615n;
const HIGH = 9007199254740993n;
function assigned(sources = [1n, 2n, MAX]) {
  const roster = new LocalRoster(); roster.setCount(sources.length);
  roster.players.forEach((player, index) => roster.assign(player, sources[index]));
  return { roster, plan: roster.snapshot(sources) };
}
function receipt(plan, alter = row => row) {
  return { kind: "play-stopped", replay: null, replayComplete: false,
    replays: plan.players.map(player => alter({ player, replay: Uint8Array.from([66, 75, 82, player]),
      replayError: null, replayComplete: false })) };
}
const admission = { recording: true, natural: false, bytesPerMember: 4 };

test("checked roster DTO retains original next identity and assignments across owner handoff", () => {
  const { roster } = assigned([0n, HIGH, MAX]);
  roster.setCount(2); roster.setCount(3); roster.assign(4, MAX);
  const state = roster.exportState();
  assert.deepEqual(state, { players: [1, 2, 4], nextPlayerId: 5,
    assignments: [[1, 0n], [2, HIGH], [4, MAX]] });
  assert.ok(Object.isFrozen(state)); assert.ok(Object.isFrozen(state.players));
  assert.ok(Object.isFrozen(state.assignments)); assert.ok(state.assignments.every(Object.isFrozen));
  const adopted = new LocalRoster(); adopted.importState(state);
  assert.deepEqual(adopted.exportState(), state);
  assert.deepEqual(adopted.snapshot([0n, HIGH, MAX]), roster.snapshot([0n, HIGH, MAX]));
  adopted.setCount(2); adopted.setCount(4);
  assert.deepEqual(adopted.players, [1, 2, 5, 6]);
  assert.deepEqual(roster.players, [1, 2, 4], "receiver mutation cannot change sender state");
  assert.equal(adopted.selected(1), 0n); assert.equal(adopted.selected(2), HIGH);
});

test("malformed roster imports preserve complete current allocator and assignments", () => {
  const { roster } = assigned([0n, HIGH, MAX]); const original = roster.exportState();
  const sparse = [1, 2]; sparse.length = 3;
  const hostile = [null, {}, { ...original, players: [] }, { ...original, players: [1, 1, 3] },
    { ...original, players: [0, 2, 3] }, { ...original, players: [1, 2, 4294967296] },
    { ...original, players: [1, 2, 3.5] }, { ...original, players: sparse },
    { ...original, players: new Uint32Array([1, 2, 3]) },
    { ...original, nextPlayerId: 3 }, { ...original, nextPlayerId: 4294967297 },
    { ...original, nextPlayerId: 4.5 }, { ...original, nextPlayerId: "4" },
    { ...original, assignments: [[1, 0n], [1, MAX]] },
    { ...original, assignments: [[1, 0n], [2, 0n]] },
    { ...original, assignments: [[999, HIGH]] }, { ...original, assignments: [[1, -1n]] },
    { ...original, assignments: [[1, MAX + 1n]] }, { ...original, assignments: [[1, 1]] },
    { ...original, assignments: [[1]] }, { ...original, assignments: [[1, HIGH, "extra"]] },
    { ...original, assignments: new Array(2) },
  ];
  for (const state of hostile) { assert.throws(() => roster.importState(state)); assert.deepEqual(roster.exportState(), original); }
  roster.setCount(4); assert.deepEqual(roster.players, [1, 2, 3, 4]);
});

test("accepted owner handoff copies mutable source DTO and refuses allocator rollback or retired IDs", () => {
  const roster = new LocalRoster();
  const state = { players: [77, 9], nextPlayerId: 78, assignments: [[77, MAX], [9, 0n]] };
  roster.importState(state); const original = roster.exportState();
  state.players[0] = 1; state.assignments[0][1] = HIGH; state.nextPlayerId = 2;
  assert.deepEqual(roster.exportState(), original);
  roster.setCount(1); const shrunk = roster.exportState();
  assert.throws(() => roster.importState({ players: [77, 9], nextPlayerId: 78, assignments: [] }));
  assert.deepEqual(roster.exportState(), shrunk);
  assert.throws(() => roster.importState({ players: [77], nextPlayerId: 77, assignments: [] }));
  roster.setCount(2); assert.deepEqual(roster.players, [77, 78]);
});

test("maximum stable member ID retains exhausted sentinel and refuses growth atomically", () => {
  const roster = new LocalRoster();
  roster.importState({ players: [1, 0xffffffff], nextPlayerId: 4294967296,
    assignments: [[1, 0n], [0xffffffff, MAX]] });
  const original = roster.exportState();
  assert.deepEqual(roster.snapshot([0n, MAX]).players, [1, 0xffffffff]);
  assert.throws(() => roster.setCount(3)); assert.deepEqual(roster.exportState(), original);
  roster.setCount(1); const solo = roster.exportState();
  assert.equal(solo.nextPlayerId, 4294967296);
  assert.throws(() => roster.setCount(2)); assert.deepEqual(roster.exportState(), solo);
});

test("roster identities survive resizing without reuse while source snapshots retain exact owned u64 values", () => {
  const roster = new LocalRoster();
  assert.deepEqual(roster.players, [1]); assert.ok(Object.isFrozen(roster.players));
  assert.equal(roster.snapshot([]), null);
  roster.setCount(4);
  const priorPlayers = roster.players;
  const sources = [0n, HIGH, MAX, 2n];
  roster.players.forEach((player, index) => roster.assign(player, sources[index]));
  const snapshot = roster.snapshot(sources);
  assert.deepEqual(snapshot.players, [1, 2, 3, 4]); assert.deepEqual(snapshot.sources, sources);
  assert.deepEqual(Array.from(snapshot.words), [1, 1, 0, 0, 2, 1, 1, 0x200000,
    3, 1, 0xffffffff, 0xffffffff, 4, 1, 2, 0]);
  assert.ok(Object.isFrozen(snapshot)); assert.ok(Object.isFrozen(snapshot.players)); assert.ok(Object.isFrozen(snapshot.sources));
  sources[0] = 9n; snapshot.words.fill(0);
  assert.equal(roster.snapshot([0n, HIGH, MAX, 2n]).words[1], 1);
  roster.setCount(2); roster.setCount(4);
  assert.deepEqual(roster.players, [1, 2, 5, 6]); assert.deepEqual(priorPlayers, [1, 2, 3, 4]);
  assert.equal(roster.selected(1), 0n); assert.equal(roster.selected(2), HIGH);
  assert.equal(roster.selected(5), null); assert.equal(roster.selected(6), null);
  assert.throws(() => roster.snapshot([0n, HIGH, MAX, 2n]));
  roster.assign(5, MAX); roster.assign(6, 2n);
  assert.deepEqual(roster.snapshot([0n, HIGH, MAX, 2n]).players, [1, 2, 5, 6]);
  roster.setCount(1); assert.equal(roster.snapshot([]), null); assert.equal(roster.selected(1), null);
  roster.setCount(2); assert.deepEqual(roster.players, [1, 7]);
  roster.clearSources(); assert.equal(roster.selected(1), null);
  roster.setCount(64); assert.equal(roster.players.length, 64);
  assert.equal(new Set(roster.players).size, 64);
});

test("invalid counts assignments inventories and pages cannot replace a retained distinct source plan", () => {
  const { roster, plan } = assigned();
  for (const count of [0, 65, 1.5, NaN, Infinity, "3", null]) {
    assert.throws(() => roster.setCount(count)); assert.deepEqual(roster.players, plan.players);
  }
  for (const [player, source] of [[0, 3n], [4, 3n], [1, -1n], [1, MAX + 1n], [1, 1], [1, undefined], [1, 2n]]) {
    assert.throws(() => roster.assign(player, source));
    assert.deepEqual(roster.snapshot([1n, 2n, MAX]).sources, plan.sources);
  }
  const sparse = [1n, 2n, MAX]; sparse.length = 4;
  for (const owned of [null, new BigUint64Array([1n, 2n, MAX]), [1n, 2n], [1n, 2n, MAX, MAX],
    [1n, 2n, MAX, -1n], [1n, 2n, MAX, 3], sparse,
    Array.from({ length: 35 }, (_, index) => BigInt(index))]) {
    assert.throws(() => roster.snapshot(owned));
  }
  for (const page of [-1, 1, 0.5, "0", NaN]) assert.throws(() => roster.snapshot([1n, 2n, MAX], page));
  roster.assign(2, null); assert.throws(() => roster.snapshot([1n, 2n, MAX]));
  roster.assign(2, 2n); assert.deepEqual(roster.snapshot([1n, 2n, MAX]).sources, plan.sources);
  const large = assigned([1n, 2n, 3n, 4n, 5n]);
  assert.equal(large.roster.snapshot([1n, 2n, 3n, 4n, 5n], 1).page, 1);
});

test("explicit solo inclusion owns one stable automatic Any plan without changing exact multi-player admission", () => {
  const roster = new LocalRoster();
  assert.equal(roster.snapshot([]), null); assert.equal(roster.snapshot([], 0, false), null);
  roster.assign(1, MAX);
  const automatic = roster.snapshot([], 0, true);
  assert.deepEqual(automatic, { words: new Uint32Array([1, 0, 0, 0]), players: [1], sources: [], page: 0, automatic: true });
  assert.ok(Object.isFrozen(automatic)); assert.ok(Object.isFrozen(automatic.players)); assert.ok(Object.isFrozen(automatic.sources));
  assert.equal(roster.selected(1), MAX, "automatic scope does not mutate a retained draft assignment");
  automatic.words.fill(0);
  assert.deepEqual(Array.from(roster.snapshot([], 0, true).words), [1, 0, 0, 0]);
  for (const includeSolo of [null, 0, 1, "true", {}, []]) assert.throws(() => roster.snapshot([], 0, includeSolo));
  for (const page of [-1, 1, 0.5, "0", NaN]) assert.throws(() => roster.snapshot([], page, true));
  const metadata = { localPlayers: [1], localPage: 0, recordLimits: { bytes: 67108864, records: 1000000 } };
  assert.deepEqual(validateLocalPrepared(roster.snapshot([], 0, true), metadata, true),
    { page: 0, recordLimits: { bytes: 67108864, records: 1000000 } });
  roster.setCount(2); roster.assign(2, HIGH);
  const exact = roster.snapshot([MAX, HIGH], 0, true);
  assert.deepEqual(exact.players, [1, 2]); assert.deepEqual(exact.sources, [MAX, HIGH]);
  assert.deepEqual(Array.from(exact.words), [1, 1, 0xffffffff, 0xffffffff, 2, 1, 1, 0x200000]);
  assert.notEqual(exact.automatic, true);
  assert.throws(() => roster.snapshot([], 0, true));
  roster.setCount(1); roster.setCount(2); assert.deepEqual(roster.players, [1, 3]);
  roster.setCount(1);
  assert.deepEqual(roster.snapshot([], 0, true).players, [1]); assert.equal(roster.snapshot([]), null);
});

test("prepared receipts require the exact stable roster page and divided capture budget before PCM admission", () => {
  const { plan } = assigned();
  const metadata = { localPlayers: [...plan.players], localPage: 0,
    recordLimits: { bytes: 22369621, records: 333333 } };
  const saved = validateLocalPrepared(plan, metadata, true);
  assert.deepEqual(saved, { page: 0, recordLimits: { bytes: 22369621, records: 333333 } });
  assert.ok(Object.isFrozen(saved)); assert.ok(Object.isFrozen(saved.recordLimits));
  metadata.recordLimits.bytes = 1; assert.equal(saved.recordLimits.bytes, 22369621);
  for (const bad of [null, {}, { ...metadata, localPlayers: new Uint32Array(plan.players) },
    { ...metadata, localPlayers: [2, 1, 3] }, { ...metadata, localPlayers: [1, 2, 2] },
    { ...metadata, localPlayers: [1, 2] }, { ...metadata, localPlayers: new Array(3) },
    { ...metadata, localPage: 1 }, { ...metadata, localPage: "0" },
    { ...metadata, recordLimits: undefined }, { ...metadata, recordLimits: { bytes: 22369622, records: 333333 } },
    { ...metadata, recordLimits: { bytes: 22369621, records: 333334 } }]) {
    assert.throws(() => validateLocalPrepared(plan, bad, true));
  }
  assert.deepEqual(validateLocalPrepared(plan, { localPlayers: [...plan.players], localPage: 0 }, false), { page: 0, recordLimits: null });
});

test("one malformed member export cannot replace other exact owned prefixes or fabricate natural completion", () => {
  const { plan } = assigned();
  const complete = receipt(plan, row => ({ ...row, replayComplete: true }));
  const valid = localReplayReceipt(plan, complete, { ...admission, natural: true });
  assert.ok(valid.every(row => row.replayComplete && row.replayError === null));
  valid.forEach((row, index) => assert.equal(row.replay, complete.replays[index].replay));
  for (const change of [row => ({ ...row, player: 1 }), row => ({ ...row, replay: null, replayComplete: true }),
    row => ({ ...row, replay: new Uint8Array(0) }), row => ({ ...row, replay: new Uint8Array(5) }),
    row => ({ ...row, replay: new Uint8Array(5).subarray(1) }), row => ({ ...row, replay: [1, 2] }),
    row => ({ ...row, replayError: "codec refused", replayComplete: false }),
    row => ({ ...row, replayError: "x".repeat(4097) }), row => ({ ...row, replayComplete: true })]) {
    const data = receipt(plan); data.replays[1] = change(data.replays[1]);
    const rows = localReplayReceipt(plan, data, admission);
    assert.equal(rows[1].replay, null); assert.equal(rows[1].replayComplete, false); assert.equal(typeof rows[1].replayError, "string");
    assert.equal(rows[0].replay, data.replays[0].replay); assert.equal(rows[2].replay, data.replays[2].replay);
  }
  const duplicate = receipt(plan); duplicate.replays[1].replay = duplicate.replays[0].replay;
  const kept = localReplayReceipt(plan, duplicate, admission);
  assert.equal(kept[0].replay, duplicate.replays[0].replay); assert.equal(kept[1].replay, null);
  assert.equal(kept[2].replay, duplicate.replays[2].replay);
  const prefix = receipt(plan); prefix.kind = "play-error";
  prefix.replays[1] = { player: 2, replay: null, replayError: "actual member export failure", replayComplete: false };
  const failed = localReplayReceipt(plan, prefix, { ...admission, natural: true });
  assert.equal(failed[1].replayError, "actual member export failure"); assert.equal(failed[0].replay, prefix.replays[0].replay);
  assert.ok(failed.every(row => row.replayComplete === false));
  const unrecorded = receipt(plan, row => ({ ...row, replay: null }));
  assert.ok(localReplayReceipt(plan, unrecorded, { ...admission, recording: false }).every(row => row.replay === null && row.replayError === null));
  for (const cap of [undefined, NaN, Infinity, 0, -1, 1.5, "4", 22369622]) {
    assert.ok(localReplayReceipt(plan, receipt(plan), { ...admission, bytesPerMember: cap })
      .every(row => row.replay === null && typeof row.replayError === "string"));
  }
  for (const data of [{}, { ...prefix, replay: new Uint8Array([1]) }, { ...prefix, replayComplete: true },
    { ...prefix, replays: prefix.replays.slice(0, 2) }]) {
    assert.ok(localReplayReceipt(plan, data, admission).every(row => row.replay === null && typeof row.replayError === "string"));
  }
});
