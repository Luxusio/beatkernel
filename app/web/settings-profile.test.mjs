// Deferred portable settings fixtures. Actual file codec; no browser/runtime ownership.
import assert from "node:assert/strict";
import { File } from "node:buffer";
import test from "node:test";
import { snapshotBrowserSettings, snapshotBrowserSettingsScalars, encodeBrowserSettings, decodeBrowserSettings } from "./settings-profile.mjs";
import * as settingsProfile from "./settings-profile.mjs";
import { BMS_TIMING_PRESET_ID } from "./play-model.mjs";

globalThis.File ??= File;
const FileType = globalThis.File;
function settings() {
  return { kind: "beatkernel-browser-settings", version: 1,
    timing: { earlyMs: "12.345678", lateMs: "0.000001", offsetMs: "-0.000001" },
    output: { latency: "custom", latencyMs: "10.000001", rate: "044100" },
    capacities: { queueCapacity: "65536", maxVoices: "4096", pendingCapacity: "4096", maxFrames: "4096", maxCommandsPerRender: "65536" },
    section: { startSeconds: "604800.000000001", endSeconds: "604800.000000002" },
    bindings: [[17, "KeyA"], [18, ""], [19, "KeyX"], [20, "KeyD"], [21, "KeyC"],
      [22, "ShiftLeft"], [23, "Space"], [24, "KeyF"], [25, "KeyV"],
      [33, "KeyN"], [34, "KeyJ"], [35, "KeyM"], [36, "KeyK"], [37, "Comma"],
      [38, "ShiftRight"], [39, "Slash"], [40, "KeyL"], [41, "Period"]] };
}
const bytes = value => new TextEncoder().encode(JSON.stringify(value));
const file = data => new FileType([data], "portable.json", { type: "application/json" });
const scalarDraft = value => ({ timing: value.timing, output: value.output, capacities: value.capacities, section: value.section });
// Literal menu order is an independent contract oracle, not derived from the mapper.
const legacyMenuFields = () => ["12.345678", "0.000001", "-0.000001", "custom", "10.000001", "044100",
  "65536", "4096", "4096", "4096", "65536", "604800.000000001", "604800.000000002"];

test("settings menu mapping emits sixteen immutable strings with exact legacy order and absent-profile defaults", () => {
  assert.equal(typeof settingsProfile.settingsMenuFields, "function");
  const expected = [...legacyMenuFields(), "", "rank-first", "beatkernel"];
  for (const original of [settings(), scalarDraft(settings())]) {
    const before = structuredClone(original);
    const fields = settingsProfile.settingsMenuFields(original);
    assert.deepEqual(fields, expected);
    assert.ok(Object.isFrozen(fields));
    assert.equal(fields.length, 16);
    assert.ok(fields.every(value => typeof value === "string"));
    assert.deepEqual(original, before);
    original.timing.earlyMs = "73";
    assert.deepEqual(fields, expected);
    assert.throws(() => { fields[13] = BMS_TIMING_PRESET_ID; }, TypeError);
  }
});

test("thirteen menu fields omit policy keys and produce independent deeply frozen scalar ownership", () => {
  assert.equal(typeof settingsProfile.settingsMenuSnapshot, "function");
  const fields = legacyMenuFields(), before = [...fields];
  const snapshot = settingsProfile.settingsMenuSnapshot(fields);
  assert.deepEqual(snapshot, scalarDraft(settings()));
  assert.deepEqual(Object.keys(snapshot.timing), ["earlyMs", "lateMs", "offsetMs"]);
  for (const value of [snapshot, ...Object.values(snapshot)]) assert.ok(Object.isFrozen(value));
  assert.equal(Object.hasOwn(snapshot, "bindings"), false);
  assert.deepEqual(fields, before);
  fields[0] = "73";
  assert.equal(snapshot.timing.earlyMs, "12.345678");
  assert.throws(() => { snapshot.section.startSeconds = "0"; }, TypeError);
});

test("complete menu policies preserve exact strings including explicit defaults and latent rank precedence", () => {
  assert.equal(typeof settingsProfile.settingsMenuSnapshot, "function");
  assert.equal(typeof settingsProfile.settingsMenuFields, "function");
  const policies = [["", "rank-first", "beatkernel"], ["", "defexrank-first", "beatkernel"],
    ...["rank-first", "defexrank-first"].flatMap(rank =>
      ["beatkernel", "assist-easy", "easy", "groove", "hard", "ex-hard", "hazard"]
        .map(gauge => [BMS_TIMING_PRESET_ID, rank, gauge]))];
  for (const [presetId, rankPrecedence, gauge] of policies) {
    const fields = [...legacyMenuFields(), presetId, rankPrecedence, gauge], before = [...fields];
    const expected = scalarDraft(settings());
    Object.assign(expected.timing, { presetId, rankPrecedence, gauge });
    const snapshot = settingsProfile.settingsMenuSnapshot(fields);
    assert.deepEqual(snapshot, expected);
    for (const value of [snapshot, ...Object.values(snapshot)]) assert.ok(Object.isFrozen(value));
    assert.deepEqual(settingsProfile.settingsMenuFields(snapshot), before);
    assert.deepEqual(fields, before);
    fields[14] = "changed-after-snapshot";
    assert.equal(snapshot.timing.rankPrecedence, rankPrecedence);
  }
});

test("menu mapping rejects malformed counts nonstrings field bounds and policy values without changing inputs", () => {
  assert.equal(typeof settingsProfile.settingsMenuSnapshot, "function");
  const valid = [...legacyMenuFields(), BMS_TIMING_PRESET_ID, "rank-first", "beatkernel"];
  const malformed = [null, {}, "settings", [], ...[12, 14, 15, 17].map(length =>
    Array.from({ length }, (_, index) => valid[index] ?? ""))];
  const limits = [21, 21, 21, 16, 21, 10, 5, 5, 5, 5, 5, 20, 20, 96, 16, 16];
  for (let index = 0; index < valid.length; index++) {
    for (const value of [null, 1, undefined, "x".repeat(limits[index] + 1)]) {
      const fields = [...valid]; fields[index] = value; malformed.push(fields);
    }
    const sparse = [...valid]; delete sparse[index]; malformed.push(sparse);
  }
  for (const [index, value] of [[0, "-1"], [1, "1e2"], [2, "0.0000001"], [3, "automatic"],
    [4, "60000.000001"], [5, "0"], [6, "65537"], [7, "4097"], [8, "0"], [9, "4097"],
    [10, "65537"], [11, "+0"], [12, "604800.000000001"], [13, "unknown/v1"],
    [14, "last-header"], [15, "unknown"]]) {
    const fields = [...valid]; fields[index] = value; malformed.push(fields);
  }
  const missingPreset = [...valid]; missingPreset[13] = ""; missingPreset[15] = "groove";
  malformed.push(missingPreset);
  for (const fields of malformed) {
    const before = structuredClone(fields);
    assert.throws(() => settingsProfile.settingsMenuSnapshot(fields));
    assert.deepEqual(fields, before);
  }
});

test("menu conversion keeps complete profile binding validation and eighteen-lane codec round trips unchanged", async () => {
  assert.equal(typeof settingsProfile.settingsMenuFields, "function");
  assert.equal(typeof settingsProfile.settingsMenuSnapshot, "function");
  for (const policy of [undefined,
    { presetId: BMS_TIMING_PRESET_ID, rankPrecedence: "defexrank-first", gauge: "hard" },
    { presetId: "", rankPrecedence: "defexrank-first", gauge: "beatkernel" }]) {
    const original = settings();
    if (policy) Object.assign(original.timing, policy);
    const before = structuredClone(original);
    const fields = settingsProfile.settingsMenuFields(original);
    assert.deepEqual(fields, [...legacyMenuFields(), ...(policy
      ? [policy.presetId, policy.rankPrecedence, policy.gauge] : ["", "rank-first", "beatkernel"])]);
    const scalar = settingsProfile.settingsMenuSnapshot(fields);
    assert.equal(Object.hasOwn(scalar, "bindings"), false);
    assert.deepEqual(original, before);
    assert.deepEqual(await decodeBrowserSettings(file(encodeBrowserSettings(original))), before);
    assert.equal(original.bindings.length, 18);
    assert.deepEqual(original.bindings, settings().bindings);
  }
  for (const mutate of [value => { value.bindings.pop(); }, value => { value.bindings = []; },
    value => { value.bindings[1][1] = "KeyA"; }, value => { value.timing.gauge = "hard"; },
    value => { value.timing.earlyMs = "-1"; }, value => { value.kind = "foreign"; }]) {
    const malformed = settings(); mutate(malformed);
    const before = structuredClone(malformed);
    assert.throws(() => settingsProfile.settingsMenuFields(malformed));
    assert.deepEqual(malformed, before);
  }
});

test("explicit timing policies round-trip while legacy profiles keep their old shape", async () => {
  const legacy = settings();
  assert.deepEqual(await decodeBrowserSettings(file(encodeBrowserSettings(legacy))), legacy);
  assert.equal(Object.hasOwn(snapshotBrowserSettings(legacy).timing, "presetId"), false);
  for (const gauge of ["beatkernel", "assist-easy", "easy", "groove", "hard", "ex-hard", "hazard"]) {
    const selected = settings();
    Object.assign(selected.timing, { presetId: BMS_TIMING_PRESET_ID, rankPrecedence: "defexrank-first", gauge });
    const loaded = await decodeBrowserSettings(file(encodeBrowserSettings(selected)));
    assert.deepEqual(loaded, selected);
    assert.ok(Object.isFrozen(loaded.timing));
  }
  const policy = { presetId: BMS_TIMING_PRESET_ID, rankPrecedence: "rank-first", gauge: "beatkernel" };
  for (const mutate of [
    value => { delete value.presetId; }, value => { delete value.rankPrecedence; }, value => { delete value.gauge; },
    value => { value.presetId = "unknown/v1"; }, value => { value.rankPrecedence = "last-header"; },
    value => { value.gauge = "unknown"; }, value => { value.presetId = ""; value.gauge = "groove"; },
  ]) {
    const selected = settings(), fields = { ...policy }; mutate(fields);
    Object.assign(selected.timing, fields);
    assert.throws(() => snapshotBrowserSettings(selected));
  }
});

test("scalar menu snapshots preserve all thirteen fields independently without weakening complete file bindings", () => {
  const original = scalarDraft(settings()), before = structuredClone(original);
  const snapshot = snapshotBrowserSettingsScalars(original);
  assert.deepEqual(snapshot, before);
  for (const value of [snapshot, ...Object.values(snapshot)]) assert.ok(Object.isFrozen(value));
  original.timing.earlyMs = "73";
  assert.equal(snapshot.timing.earlyMs, before.timing.earlyMs);
  for (const invalid of [null, [], { ...before, bindings: [] }, { ...before, kind: "beatkernel-browser-settings" },
    { timing: before.timing, output: before.output, capacities: before.capacities },
    { ...before, timing: { ...before.timing, earlyNs: "1" } }]) assert.throws(() => snapshotBrowserSettingsScalars(invalid));
  const full = settings(); full.bindings = [];
  assert.deepEqual(snapshotBrowserSettingsScalars(scalarDraft(full)), before);
  assert.throws(() => snapshotBrowserSettings(full), /eighteen/);
  assert.throws(() => encodeBrowserSettings(full), /eighteen/);
});

test("scalar validation retains timing endpoints capacity bounds output categories and inactive Custom policy", () => {
  const edge = scalarDraft(settings());
  edge.timing = { earlyMs: "9223372036854.775807", lateMs: "0", offsetMs: "-9223372036854.775808" };
  edge.output = { latency: "playback", latencyMs: "60000.000000", rate: "4294967295" };
  edge.section = { startSeconds: "9223372034.854775806", endSeconds: "9223372034.854775807" };
  assert.deepEqual(snapshotBrowserSettingsScalars(edge), edge);
  const minimum = scalarDraft(settings());
  minimum.timing = { earlyMs: "0", lateMs: "0", offsetMs: "0" };
  minimum.output = { latency: "interactive", latencyMs: "0", rate: "" };
  minimum.section = { startSeconds: "0", endSeconds: "" };
  for (const name of Object.keys(minimum.capacities)) minimum.capacities[name] = "1";
  assert.deepEqual(snapshotBrowserSettingsScalars(minimum), minimum);
  for (const mutate of [
    value => { value.timing.earlyMs = "9223372036854.775808"; },
    value => { value.timing.offsetMs = "-9223372036854.775809"; },
    value => { value.section.endSeconds = "9223372034.854775808"; },
    ...Object.keys(minimum.capacities).map(name => value => { value.capacities[name] = "0"; }),
    ...["interactive", "balanced", "playback"].flatMap(latency => [
      value => { value.output.latency = latency; value.output.latencyMs = "invalid"; },
      value => { value.output.latency = latency; value.output.latencyMs = "60000.000001"; },
    ]),
  ]) {
    const invalid = structuredClone(edge); mutate(invalid);
    assert.throws(() => snapshotBrowserSettingsScalars(invalid));
  }
});

test("full settings snapshots keep exact decimal text, canonical eighteen rows and independent frozen ownership", async () => {
  const original = settings();
  original.bindings.reverse();
  const snapshot = snapshotBrowserSettings(original);
  assert.deepEqual(snapshot, settings());
  for (const value of [snapshot, snapshot.timing, snapshot.output, snapshot.capacities, snapshot.section,
    snapshot.bindings, ...snapshot.bindings]) assert.ok(Object.isFrozen(value));
  original.timing.earlyMs = "99"; original.bindings[17][1] = "KeyB";
  assert.deepEqual(snapshot, settings());
  assert.throws(() => { snapshot.section.startSeconds = "0"; }, TypeError);
  const encoded = encodeBrowserSettings(snapshot);
  assert.ok(encoded instanceof Uint8Array);
  assert.ok(encoded.byteLength > 0 && encoded.byteLength <= 16384);
  assert.deepEqual(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(encoded)), settings());
  const loaded = await decodeBrowserSettings(file(encoded));
  assert.deepEqual(loaded, settings());
  assert.notEqual(loaded, snapshot);
  assert.notEqual(loaded.bindings, snapshot.bindings);
  encoded.fill(0);
  assert.deepEqual(loaded, settings());
  const unbound = settings(); unbound.bindings.forEach(row => { row[1] = ""; });
  unbound.section = { startSeconds: "0", endSeconds: "" };
  unbound.output = { latency: "interactive", latencyMs: "0", rate: "" };
  assert.deepEqual(await decodeBrowserSettings(file(encodeBrowserSettings(unbound))), unbound);
});

test("exact schema and complete binding ownership reject foreign, partial, duplicate and authority-bearing records atomically", async () => {
  const changes = [
    value => { value.kind = "beatkernel-player-profile"; },
    value => { value.version = 2; }, value => { value.version = "1"; },
    value => { delete value.timing; }, value => { value.chart = "secret/chart.bms"; },
    value => { value.output.deviceId = "device"; }, value => { value.timing.extra = "0"; },
    value => { delete value.capacities.maxFrames; }, value => { value.section.endNs = "1"; },
    value => { value.bindings.pop(); }, value => { value.bindings.push([17, "KeyB"]); },
    value => { value.bindings = []; },
    value => { value.bindings[17] = [17, "KeyB"]; }, value => { value.bindings[0] = [0, "KeyA"]; },
    value => { value.bindings[1][1] = "KeyA"; }, value => { value.bindings[0][1] = "Escape"; },
    value => { value.bindings[0] = [17, "KeyA", 19]; }, value => { value.bindings[0][0] = "17"; },
    value => { value.bindings[0][1] = null; }, value => { delete value.bindings[4]; },
  ];
  for (const change of changes) {
    const malformed = settings(); change(malformed);
    const before = structuredClone(malformed);
    assert.throws(() => snapshotBrowserSettings(malformed));
    assert.throws(() => encodeBrowserSettings(malformed));
    await assert.rejects(decodeBrowserSettings(file(bytes(malformed))));
    assert.deepEqual(malformed, before);
  }
  for (const malformed of [null, [], "settings", 1]) assert.throws(() => snapshotBrowserSettings(malformed));
  const symbol = settings(); symbol[Symbol("unknown")] = 1;
  assert.throws(() => snapshotBrowserSettings(symbol));
});

test("original timing, output, capacity and section limits retain precision without accepting lossy or ignored custom values", async () => {
  const changes = [
    value => { value.timing.earlyMs = 50; }, value => { value.timing.earlyMs = "-1"; },
    value => { value.timing.lateMs = "1e2"; }, value => { value.timing.offsetMs = "0.0000001"; },
    value => { value.output.latency = "automatic"; }, value => { value.output.rate = "4294967296"; },
    value => { value.output.rate = "0"; }, value => { value.output.latencyMs = "60000.000001"; },
    value => { value.output.latency = "balanced"; value.output.latencyMs = "not used?"; },
    value => { value.output.latencyMs = "10.0000001"; },
    value => { value.capacities.queueCapacity = "65537"; }, value => { value.capacities.maxVoices = "4097"; },
    value => { value.capacities.pendingCapacity = "0"; }, value => { value.capacities.maxFrames = "4097"; },
    value => { value.capacities.maxCommandsPerRender = "65537"; },
    value => { value.section.endSeconds = value.section.startSeconds; },
    value => { value.section.startSeconds = "0.0000000001"; },
    value => { value.section.startSeconds = "+0"; }, value => { value.section.endSeconds = "1e6"; },
  ];
  for (const change of changes) {
    const malformed = settings(); change(malformed);
    assert.throws(() => snapshotBrowserSettingsScalars(scalarDraft(malformed)));
    assert.throws(() => snapshotBrowserSettings(malformed));
    await assert.rejects(decodeBrowserSettings(file(bytes(malformed))));
  }
  const edge = settings();
  edge.output = { latency: "playback", latencyMs: "60000.000000", rate: "4294967295" };
  edge.section = { startSeconds: "9223372034.854775806", endSeconds: "9223372034.854775807" };
  assert.deepEqual(await decodeBrowserSettings(file(encodeBrowserSettings(edge))), edge);
});

test("File metadata and actual bytes obey the independent 16 KiB limit and strict UTF-8 without partial results", async () => {
  for (const size of [0, -1, 1.5, NaN, 16385]) {
    let reads = 0;
    const selected = file(bytes(settings()));
    Object.defineProperty(selected, "size", { value: size });
    selected.arrayBuffer = () => { reads++; throw new Error("metadata must refuse first"); };
    await assert.rejects(decodeBrowserSettings(selected));
    assert.equal(reads, 0);
  }
  const valid = bytes(settings());
  const maximum = new Uint8Array(16384).fill(32); maximum.set(valid);
  assert.deepEqual(await decodeBrowserSettings(file(maximum)), settings());
  for (const malformed of [Uint8Array.from([0xc3, 0x28]),
    new TextEncoder().encode("{} trailing"), new TextEncoder().encode("null")]) {
    await assert.rejects(decodeBrowserSettings(file(malformed)), "equal metadata extent still requires strict UTF-8 and a valid document");
  }
  for (const actual of [new Uint8Array(16385).fill(32), Uint8Array.from([0xc3, 0x28]),
    new TextEncoder().encode("{} trailing"), new TextEncoder().encode("null"), new Uint8Array()]) {
    let reads = 0;
    const selected = file(valid);
    selected.arrayBuffer = async () => { reads++; return actual.buffer; };
    await assert.rejects(decodeBrowserSettings(selected));
    assert.equal(reads, 1);
  }
  const rejected = file(valid);
  rejected.arrayBuffer = async () => { throw new Error("actual file read failed"); };
  await assert.rejects(decodeBrowserSettings(rejected), /actual file read failed/);
  await assert.rejects(decodeBrowserSettings({ size: valid.length, arrayBuffer: async () => valid.buffer }));
});
