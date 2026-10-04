// Deferred real profile/adapter fixtures. Canonical bytes are checked against
// core BKPI v1 field offsets, without browser devices or a simulated judge.
import assert from "node:assert/strict";
import test from "node:test";
import { GamepadAdapter, automaticGamepadSetup, snapshotGamepadSetup } from "./gamepad-profile.mjs";

const SOURCE = 0x8877665544332211n;
const SEQUENCE = 0x0102030405060708n;
const U64_MAX = 18446744073709551615n;
function setup(rows = [[0x11, 0, 0]], devices = [{ source: SOURCE, buttons: 2, axes: 1 }]) {
  return { devices, bindingWords: new Uint32Array(rows.flatMap(([lane, type, index, source = SOURCE]) =>
    [lane, Number(source & 0xffffffffn), Number(source >> 32n), type, index])) };
}
function sample(fields = {}) {
  return { kind: "gamepad", source: SOURCE, index: 4, id: "controller description", mapping: "standard",
    timestampMs: 0, hostNs: 0n, sequence: SEQUENCE, axes: [0],
    buttons: [{ value: 0, pressed: false, touched: false }, { value: 0, pressed: false, touched: false }], ...fields };
}
const button = (pressed, value = 0, touched = false) => ({ pressed, value, touched });
function view(bytes) { return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); }
function code(bytes) { return view(bytes).getUint32(64, true); }

test("profile snapshots retain exact source and native type namespaces with independent bounded constructor bindings", () => {
  const raw = setup([[0x11, 0, 1], [0x12, 1, 0], [0x13, 2, 1], [0x14, 3, 0]]);
  const saved = snapshotGamepadSetup(raw);
  assert.deepEqual(saved.sources, [SOURCE]);
  assert.deepEqual(saved.lanes, [0x11], "only pressed-button fields cover ordinary press-chart lanes");
  assert.deepEqual(Array.from(saved.physicalWords), [
    0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 1,
    0x12, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x10000,
    0x13, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x20001,
    0x14, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x30000,
  ]);
  assert.ok(Object.isFrozen(saved) && Object.isFrozen(saved.devices) && saved.devices.every(Object.isFrozen));
  assert.ok(Object.isFrozen(saved.sources) && Object.isFrozen(saved.lanes));
  assert.notEqual(saved.bindingWords.buffer, raw.bindingWords.buffer);
  const adapter = new GamepadAdapter(saved);
  raw.devices[0].source = 3n; raw.devices[0].buttons = 0; raw.bindingWords.fill(0);
  saved.bindingWords.fill(0); saved.physicalWords.fill(0);
  const encoded = adapter.decode(sample({ buttons: [button(false), button(true)] }));
  assert.deepEqual(encoded.map(code), [1, 0x10000, 0x20001]);
  assert.ok(encoded.every(bytes => view(bytes).getBigUint64(7, true) === SOURCE));
  const empty = snapshotGamepadSetup({ devices: [], bindingWords: new Uint32Array() });
  assert.deepEqual(empty.sources, []); assert.equal(empty.physicalWords.length, 0);
  const unbound = new GamepadAdapter({ devices: [{ source: 3n, buttons: 0, axes: 0 }], bindingWords: new Uint32Array() });
  assert.deepEqual(unbound.decode(sample({ source: 3n, axes: [], buttons: [] })), []);
});

test("pressed and absolute-axis packets match independent BKPI bytes and bound deltas preserve original acquisition metadata", () => {
  const adapter = new GamepadAdapter(setup([[0x11, 0, 1], [0x12, 1, 0]]));
  const packets = adapter.decode(sample({ axes: [-0.5], buttons: [button(false), button(true)] }));
  assert.equal(packets.length, 2);
  assert.deepEqual(Array.from(packets[0]), [
    0x42, 0x4b, 0x50, 0x49, 1, 0, 0,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    0, 0, 0, 0, 0, 0, 0, 0, 0x4e, 0x49, 0x57, 0,
    8, 7, 6, 5, 4, 3, 2, 1,
    1, 0x44, 0x50, 0x47, 0x57, 1, 1, 0, 0, 0, 1,
    0x4e, 0x49, 0x57, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 1, 0x44, 0x50, 0x47, 0x57, 1, 0, 0, 0, 0,
  ]);
  assert.deepEqual(Array.from(packets[1]), [
    0x42, 0x4b, 0x50, 0x49, 1, 0, 1,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    0, 0, 0, 0, 0, 0, 0, 0, 0x4e, 0x49, 0x57, 0,
    8, 7, 6, 5, 4, 3, 2, 1,
    1, 0x44, 0x50, 0x47, 0x57, 1, 0, 0, 1, 0, 1,
    0x4e, 0x49, 0x57, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 1, 0x44, 0x50, 0x47, 0x57, 0, 0, 1, 0,
    0, 0, 0, 0xbf, 0,
  ]);
  assert.deepEqual(adapter.decode(sample({ sequence: SEQUENCE + 1n, axes: [-0.5], buttons: [button(false), button(true)] })), []);
  const deltas = new GamepadAdapter(setup([[0x11, 0, 0], [0x12, 3, 0], [0x13, 2, 0], [0x14, 1, 0]]));
  const start = sample({ timestampMs: 1234.125, hostNs: 1234125000n,
    axes: [0.1], buttons: [button(false, 0.1, false), button(false)] });
  const initial = deltas.decode(start);
  assert.deepEqual(initial.map(code), [0x20000, 0x10000], "initial false levels emit no synthetic releases");
  const next = deltas.decode({ ...start, sequence: SEQUENCE + 1n, axes: [0.10000000000000002],
    buttons: [button(true, 0.10000000000000002, true), button(false)] });
  assert.deepEqual(next.map(code), [0, 0x30000, 0x20000, 0x10000]);
  assert.equal(view(next[2]).getUint32(68, true), 0x3dcccccd);
  assert.equal(view(next[3]).getUint32(68, true), 0x3dcccccd,
    "changed browser doubles remain acquisitions even when their explicit canonical f32 values agree");
  for (const bytes of next) {
    const actual = view(bytes);
    assert.equal(actual.getBigInt64(15, true), 1234125000n);
    assert.equal(actual.getUint32(23, true), 0x57494e);
    assert.equal(actual.getBigUint64(27, true), SEQUENCE + 1n);
    assert.equal(actual.getUint32(36, true), 0x57475044);
    assert.equal(actual.getUint32(41, true), code(bytes));
    assert.equal(actual.getBigInt64(50, true), 1234125000n);
    assert.equal(bytes[58], 0);
  }
  const up = deltas.decode({ ...start, sequence: SEQUENCE + 2n, axes: [-0], buttons: [button(false, 0, false), button(false)] });
  assert.equal(up[0][68], 1); assert.equal(up[1][68], 1);
  assert.equal(view(up[3]).getUint32(68, true), 0x80000000);
  const positiveZero = deltas.decode({ ...start, sequence: SEQUENCE + 3n, axes: [0], buttons: [button(false), button(false)] });
  assert.equal(positiveZero.length, 1); assert.equal(code(positiveZero[0]), 0x10000);
  assert.equal(view(positiveZero[0]).getUint32(68, true), 0);
});

test("forked batches and individual decode refusals leave prior levels and acquisition order unchanged", () => {
  const owner = new GamepadAdapter(setup());
  assert.deepEqual(owner.decode(sample()), []);
  const draft = owner.fork();
  const down = sample({ sequence: SEQUENCE + 1n, buttons: [button(true), button(false)] });
  assert.equal(draft.decode(down).length, 1);
  assert.throws(() => draft.decode(sample({ sequence: SEQUENCE + 2n, axes: [NaN] })));
  assert.equal(owner.decode(down).length, 1, "discarding a rejected batch cannot adopt a valid draft prefix");
  assert.deepEqual(owner.decode({ ...down, sequence: SEQUENCE + 2n }), []);
  assert.equal(draft.decode(sample({ sequence: SEQUENCE + 2n })).length, 1,
    "a rejected sample does not consume its sequence or change the prior held level");

  const fresh = new GamepadAdapter(setup());
  const malformed = sample({ index: 12, id: "not adopted", sequence: U64_MAX,
    buttons: [button(true), button(false, NaN)] });
  assert.throws(() => fresh.decode(malformed));
  const recovered = fresh.decode(sample({ sequence: 0n, buttons: [button(true), button(false)] }));
  assert.equal(recovered.length, 1, "even an unbound invalid control prevents metadata, order and level adoption");
  const accepted = fresh.fork();
  assert.equal(accepted.decode(sample({ sequence: 1n })).length, 1);
  assert.deepEqual(accepted.decode(sample({ sequence: 2n })), []);
  assert.equal(fresh.decode(sample({ sequence: 1n })).length, 1, "forks do not share mutable held state");
});

test("configuration and per-source identity, timestamp, count and sequence bounds refuse atomically", () => {
  const invalid = [
    null, { devices: [], bindingWords: [] }, setup([[0x20, 0, 0]]), setup([[0x11, 4, 0]]),
    setup([[0x11, 0, 2]]), setup([[0x11, 1, 1]]), setup([[0x11, 0, 0, 3n]]),
    setup([[0x11, 0, 0], [0x11, 0, 0]]),
    { devices: [{ source: SOURCE, buttons: 2, axes: 1 }], bindingWords: new Uint32Array(4) },
    { devices: [{ source: 2n, buttons: 1, axes: 0 }], bindingWords: new Uint32Array() },
    { devices: [{ source: U64_MAX + 1n, buttons: 1, axes: 0 }], bindingWords: new Uint32Array() },
    { devices: [{ source: Number(SOURCE), buttons: 1, axes: 0 }], bindingWords: new Uint32Array() },
    { devices: [{ source: SOURCE, buttons: 129, axes: 0 }], bindingWords: new Uint32Array() },
    { devices: [{ source: SOURCE, buttons: 0, axes: 65 }], bindingWords: new Uint32Array() },
    { devices: [{ source: SOURCE, buttons: -1, axes: 0 }], bindingWords: new Uint32Array() },
    { devices: [{ source: SOURCE, buttons: 1.5, axes: 0 }], bindingWords: new Uint32Array() },
    setup([], [{ source: SOURCE, buttons: 1, axes: 0 }, { source: SOURCE, buttons: 1, axes: 0 }]),
    setup([], Array.from({ length: 17 }, (_, index) => ({ source: BigInt(index + 3), buttons: 0, axes: 0 }))),
  ];
  const detached = setup(); structuredClone(detached.bindingWords.buffer, { transfer: [detached.bindingWords.buffer] });
  invalid.push(detached);
  for (const value of invalid) assert.throws(() => snapshotGamepadSetup(value));

  const original = sample({ timestampMs: 10, hostNs: 10000000n, sequence: 10n });
  const badSamples = [
    { kind: "keyboard" }, { source: 3n }, { source: Number(SOURCE) }, { index: 64 }, { index: 3 },
    { id: "different description" }, { mapping: "" }, { id: "x".repeat(1025) },
    { timestampMs: 9, hostNs: 9000000n }, { timestampMs: NaN }, { hostNs: 10000001n },
    { hostNs: -1n }, { sequence: 10n }, { sequence: 9n }, { sequence: U64_MAX + 1n }, { sequence: 11 },
    { axes: [] }, { axes: new Float64Array([0]) }, { axes: [1.0000000000000002] },
    { buttons: [button(false)] }, { buttons: [button(false), button(false, 1.01)] },
    { buttons: [button(false), { value: 0, pressed: 0, touched: false }] },
  ];
  for (const fields of badSamples) {
    const adapter = new GamepadAdapter(setup());
    assert.deepEqual(adapter.decode(original), []);
    assert.throws(() => adapter.decode({ ...original, sequence: 11n, ...fields }));
    assert.equal(adapter.decode({ ...original, sequence: 11n, buttons: [button(true), button(false)] }).length, 1);
  }
  const sources = new GamepadAdapter(setup([[0x11, 0, 0], [0x12, 0, 0, 3n]],
    [{ source: SOURCE, buttons: 2, axes: 1 }, { source: 3n, buttons: 2, axes: 1 }]));
  assert.equal(sources.decode(sample({ sequence: U64_MAX, buttons: [button(true), button(false)] })).length, 1);
  assert.equal(sources.decode(sample({ source: 3n, sequence: 0n, buttons: [button(true), button(false)] })).length, 1,
    "adapter acquisition order belongs to each real source, not browser slot or product text");
});

test("exact fanout capacity emits each physical field once even when several lanes bind it", () => {
  const devices = [{ source: U64_MAX, buttons: 128, axes: 64 }];
  const rows = Array.from({ length: 128 }, (_, index) => [0x11, 0, index, U64_MAX])
    .concat(Array.from({ length: 128 }, (_, index) => [0x12, 3, index, U64_MAX]));
  const adapter = new GamepadAdapter(setup(rows, devices));
  const values = sample({ source: U64_MAX, axes: new Array(64).fill(1),
    buttons: Array.from({ length: 128 }, () => button(true, 1, true)) });
  const outputs = adapter.decode(values);
  assert.equal(outputs.length, 256);
  assert.equal(new Set(outputs.map(code)).size, 256);
  assert.ok(outputs.every(bytes => bytes.length === 69 && bytes[68] === 0
    && view(bytes).getBigUint64(7, true) === U64_MAX && view(bytes).getBigUint64(27, true) === SEQUENCE));
  assert.deepEqual(adapter.decode({ ...values, sequence: SEQUENCE + 1n }), []);
  assert.throws(() => snapshotGamepadSetup(setup([...rows, [0x13, 1, 0, U64_MAX]], devices)));
  const fanout = new GamepadAdapter(setup([[0x11, 0, 0], [0x12, 0, 0], [0x13, 0, 0]]));
  assert.equal(fanout.decode(sample({ buttons: [button(true), button(false)] })).length, 1,
    "BindingMap owns lane fanout; the profile emits one genuine physical event");
});

test("automatic standard profiles bind nine genuine buttons with exact sources and reject malformed ignored descriptors", () => {
  const standard = { source: U64_MAX, index: 63, id: "same product", mapping: "standard", buttons: 128, axes: 64 };
  const raw = [{ source: 3n, index: 0, id: "same product", mapping: "", buttons: 9, axes: 0 }, standard,
    { source: 4n, index: 1, id: "small standard controller", mapping: "standard", buttons: 8, axes: 2 }];
  const automatic = automaticGamepadSetup(raw);
  assert.deepEqual(automatic.devices, [{ source: U64_MAX, buttons: 128, axes: 64 }]);
  assert.deepEqual(automatic.sources, [U64_MAX]);
  assert.deepEqual(Array.from(automatic.bindingWords), [
    0x11, 0xffffffff, 0xffffffff, 0, 0, 0x12, 0xffffffff, 0xffffffff, 0, 1,
    0x13, 0xffffffff, 0xffffffff, 0, 2, 0x14, 0xffffffff, 0xffffffff, 0, 3,
    0x15, 0xffffffff, 0xffffffff, 0, 4, 0x16, 0xffffffff, 0xffffffff, 0, 5,
    0x17, 0xffffffff, 0xffffffff, 0, 6, 0x18, 0xffffffff, 0xffffffff, 0, 7,
    0x19, 0xffffffff, 0xffffffff, 0, 8,
  ]);
  assert.deepEqual(automatic.lanes, [0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19]);
  assert.equal(automatic.physicalWords.length, 63);
  for (let index = 0; index < 9; index++) {
    assert.deepEqual(Array.from(automatic.physicalWords.slice(index * 7, index * 7 + 7)),
      [0x11 + index, 1, 0xffffffff, 0xffffffff, 1, 0x57475044, index]);
  }
  standard.source = 5n; standard.buttons = 0; raw.length = 0;
  assert.deepEqual(automatic.sources, [U64_MAX]);
  assert.equal(automatic.devices[0].buttons, 128);
  assert.ok(Object.isFrozen(automatic) && Object.isFrozen(automatic.devices));
  assert.deepEqual(automaticGamepadSetup([]).sources, []);
  const many = Array.from({ length: 16 }, (_, index) => ({ source: BigInt(index + 3), index,
    id: "one model", mapping: "standard", buttons: 9, axes: 0 }));
  assert.equal(automaticGamepadSetup(many).bindingWords.length, 16 * 9 * 5);
  assert.throws(() => automaticGamepadSetup([...many, { ...many[0], source: 19n, index: 16 }]));
  const ignored = { source: 3n, index: 0, id: "unmapped", mapping: "", buttons: 0, axes: 0 };
  for (const fields of [{ source: 2n }, { source: 3 }, { source: U64_MAX + 1n }, { index: 64 },
    { id: "x".repeat(1025) }, { mapping: "unknown" }, { buttons: 129 }, { axes: 65 }]) {
    assert.throws(() => automaticGamepadSetup([{ ...ignored, ...fields }]));
  }
  assert.throws(() => automaticGamepadSetup([ignored, { ...ignored, source: 4n }]));
  assert.throws(() => automaticGamepadSetup([ignored, { ...ignored, index: 1 }]));
  assert.throws(() => automaticGamepadSetup(new Array(1)));
});
