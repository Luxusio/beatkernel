// Deferred portable settings fixtures. Actual file codec; no browser/runtime ownership.
import assert from "node:assert/strict";
import { File } from "node:buffer";
import test from "node:test";
import { snapshotBrowserSettings, encodeBrowserSettings, decodeBrowserSettings } from "./settings-profile.mjs";

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
