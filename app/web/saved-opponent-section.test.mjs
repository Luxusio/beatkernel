// Genuine generated WASM preparation, capture and saved-comparison admission.
// Stopped prefixes prove setup compatibility; they do not claim completion.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { createContext, SourceTextModule, runInContext } from "node:vm";
import { keyboardBindingWords } from "./physical-input.mjs";
import { localBindingWords, snapshotLocalPlan } from "./local-play-model.mjs";

const packageUrl = process.env.BEATKERNEL_SAVED_OPPONENT_PACKAGE
  ? pathToFileURL(`${process.env.BEATKERNEL_SAVED_OPPONENT_PACKAGE.replace(/\/$/, "")}/`)
  : new URL("./pkg/", import.meta.url);
const bindingUrl = new URL("beatkernel_bms_runtime.js", packageUrl);
const context = createContext({ WebAssembly, TextEncoder, TextDecoder, Uint8Array, Uint32Array,
  Float32Array, Float64Array, ArrayBuffer, DataView, URL, Request, Response, console });
const binding = new SourceTextModule(await readFile(bindingUrl, "utf8"), { context,
  identifier: bindingUrl.href, initializeImportMeta(meta) { meta.url = bindingUrl.href; } });
await binding.link(specifier => { throw new Error(`unexpected saved-opponent binding import ${specifier}`); });
await binding.evaluate();
context.savedOpponentModule = new WebAssembly.Module(await readFile(new URL("beatkernel_bms_runtime_bg.wasm", packageUrl)));
binding.namespace.initSync(runInContext("({ module: savedOpponentModule })", context));
const { BrowserLibrary, BrowserGame, BrowserLocalGame } = binding.namespace;
assert.equal(typeof BrowserLibrary.prototype.begin_prepare_chart_at, "function");
assert.equal(typeof BrowserLibrary.prototype.begin_prepare_chart_with_policy_at, "function");

const START = 125000000n, END = 2500000000n, SEED = 18446744073709551615n;
const MAX_PLAYER = 0xffffffff, OTHER_PLAYER = 17;
const PLAYERS = [MAX_PLAYER, OTHER_PLAYER];
const PRESET = "beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1";
const CHART = "#TITLE Genuine section comparison\n#BPM 120\n#RANK 2\n#WAV01 key.wav\n#00111:0100\n";
const limits = [1024, 4096, 8];
const bytes = text => new TextEncoder().encode(text);
function wave() {
  const value = new Uint8Array(48), view = new DataView(value.buffer);
  value.set(bytes("RIFF")); view.setUint32(4, 40, true); value.set(bytes("WAVEfmt "), 8);
  view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
  view.setUint32(24, 24000, true); view.setUint32(28, 48000, true);
  view.setUint16(32, 2, true); view.setUint16(34, 16, true); value.set(bytes("data"), 36);
  view.setUint32(40, 4, true); view.setInt16(44, 8192, true); view.setInt16(46, -4096, true);
  return value;
}

function prepared(library, policy) {
  const files = new Map([["pack/chart.bms", bytes(CHART)], ["pack/key.wav", wave()]]);
  for (const [path, value] of files) library.declare_file(path, value.byteLength);
  library.add_file("pack/chart.bms", files.get("pack/chart.bms"));
  const acquisitions = new Map([["pack/chart.bms", 1]]);
  const job = policy
    ? library.begin_prepare_chart_with_policy_at("pack/chart.bms", 24000, 2, SEED, START, ...limits,
      PRESET, "rank-first", "groove", 0n)
    : library.begin_prepare_chart_at("pack/chart.bms", 24000, 2, SEED, START, ...limits);
  try {
    for (let turn = 0; turn < 512; turn++) {
      const path = job.pending_asset_path();
      if (path !== undefined && path !== null) {
        assert.ok(files.has(path), "actual job references a declared fixture asset");
        // Pending names persist across decode/resample stages after hydration.
        // Production acquisition also returns already-loaded assets unchanged.
        if (!acquisitions.has(path)) {
          library.add_file(path, files.get(path));
          acquisitions.set(path, 1);
        }
      }
      assert.equal(typeof job.stage, "string");
      if (!job.advance(library)) continue;
      assert.equal(job.stage, "ready");
      assert.equal(job.parse_count, 1);
      assert.equal(job.compile_count, 1);
      assert.equal(acquisitions.size, files.size);
      assert.ok([...acquisitions.values()].every(count => count === 1), "each declared fixture asset hydrates once");
      const value = job.take_prepared(library);
      assert.equal(value.start_ns, START);
      assert.deepEqual(Array.from(value.lanes), [0x11]);
      return value;
    }
    assert.fail("actual incremental preparation did not reach Ready");
  } finally { job.cancel(); job.free(); }
}

function owner(local, policy, end) {
  const library = new BrowserLibrary(8, 1024 * 1024, 2 * 1024 * 1024, 64);
  let game;
  try {
    const value = prepared(library, policy);
    if (local) {
      const plan = snapshotLocalPlan(Uint32Array.from([MAX_PLAYER, 1, 1, 0, OTHER_PLAYER, 1, 2, 0]));
      const physical = keyboardBindingWords(Uint32Array.from([0x11, 4]));
      const bindings = localBindingWords(plan, physical, [0x11], true);
      game = BrowserLocalGame.new_physical(value, 0n, 100000000n, 50000000n, 50000000n, 0n,
        plan.words, bindings.words, end, true, 4096, 1024);
      assert.deepEqual(Array.from(game.players), PLAYERS);
    } else {
      const pairs = Uint32Array.from([0x11, 4]);
      game = end === undefined
        ? new BrowserGame(value, 0n, 100000000n, 50000000n, 50000000n, 0n, pairs)
        : BrowserGame.new_section(value, 0n, 100000000n, 50000000n, 50000000n, 0n, pairs, end);
    }
    return { game, close() { game.free(); library.free(); } };
  } catch (error) { game?.free(); library.free(); throw error; }
}

function captures(local, policy, end) {
  const source = owner(local, policy, end);
  try {
    if (local) for (const player of PLAYERS) source.game.configure_capture(player, 1024 * 1024, 1024);
    else source.game.configure_capture(1024 * 1024, 1024);
    source.game.stop();
    const rows = new Map();
    for (const player of local ? PLAYERS : [1]) {
      const replay = local ? source.game.take_replay(player) : source.game.take_replay();
      assert.ok(replay instanceof Uint8Array);
      assert.ok(replay.byteLength > 0);
      assert.equal(local ? source.game.take_replay(player) : source.game.take_replay(), undefined);
      rows.set(player, replay);
    }
    return rows;
  } finally { source.close(); }
}

function admit(game, local, replay, own, label, player = MAX_PLAYER) {
  return local ? game.add_saved_opponent(player, replay, own, label) : game.add_saved_opponent(replay, own, label);
}
function snapshot(game) { return structuredClone(game.saved_opponents()); }
function score(game, local) {
  return local ? PLAYERS.map(player => [player, game.member_song_ns(player), game.hits(player), game.misses(player), game.combo(player)])
    : [game.song_ns, game.hits, game.misses, game.combo];
}
function incompatible(action) {
  // Rust's typed CompetitionError is surfaced as this existing Error display.
  assert.throws(action, /saved opponent comparison: BMS competition: IncompatibleSetup/);
}

for (const local of [false, true]) {
  for (const policy of [false, true]) {
    for (const finite of [false, true]) {
      test(`actual ${local ? "local contact" : "solo button"} ${policy ? "policy" : "default"} ${finite ? "finite" : "open"} capture matches and refuses different sections atomically`, () => {
        const end = finite ? END : undefined;
        const original = captures(local, policy, end);
        const receiver = owner(local, policy, end);
        try {
          assert.equal(admit(receiver.game, local, original.get(local ? MAX_PLAYER : 1), true, "genuine matching own"), 0);
          if (local) assert.equal(admit(receiver.game, true, original.get(OTHER_PLAYER), false, "genuine matching other", OTHER_PLAYER), 0);
          const before = snapshot(receiver.game), beforeScore = score(receiver.game, local);
          if (local) {
            assert.deepEqual(before.map(row => row.player), PLAYERS);
            assert.ok(before.every(row => row.error === null && row.opponents.length === 1));
          } else assert.equal(before.length, 1);
          // All candidates come from independently configured, genuine owners.
          for (const wrongEnd of finite ? [END + 1n, undefined] : [END]) {
            const candidate = captures(local, policy, wrongEnd);
            incompatible(() => admit(receiver.game, local, candidate.get(local ? MAX_PLAYER : 1), false, "wrong section"));
            assert.deepEqual(snapshot(receiver.game), before);
            assert.deepEqual(score(receiver.game, local), beforeScore);
          }
          if (local) {
            assert.throws(() => admit(receiver.game, true, original.get(MAX_PLAYER), true, "unknown member", 0xfffffffe), /unknown local player/);
            assert.deepEqual(snapshot(receiver.game), before);
          }
          // Refusals do not consume a slot or poison the existing usable owner.
          assert.equal(admit(receiver.game, local, original.get(local ? MAX_PLAYER : 1), false, "matching retry"), 1);
        } finally { receiver.close(); }
      });
    }
  }
}

test("actual local original member captures share exactly eight admission slots across the roster", () => {
  const original = captures(true, true, END), receiver = owner(true, true, END);
  try {
    for (let index = 0; index < 8; index++) {
      const player = PLAYERS[index % 2];
      assert.equal(admit(receiver.game, true, original.get(player), index % 2 === 0, `valid record ${index}`, player), Math.floor(index / 2));
    }
    const before = snapshot(receiver.game), beforeScore = score(receiver.game, true);
    assert.deepEqual(before.map(row => [row.player, row.opponents.length, row.error]), [[MAX_PLAYER, 4, null], [OTHER_PLAYER, 4, null]]);
    for (const player of PLAYERS) {
      assert.throws(() => admit(receiver.game, true, original.get(player), true, "ninth valid record", player), /local saved opponent capacity reached/);
      assert.deepEqual(snapshot(receiver.game), before);
      assert.deepEqual(score(receiver.game, true), beforeScore);
    }
  } finally { receiver.close(); }
});
