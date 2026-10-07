// Deferred real profile/adapter fixtures. Canonical bytes are checked against
// core BKPI v1 field offsets, without browser devices or a simulated judge.
import assert from "node:assert/strict";
import test from "node:test";
import { GamepadAdapter, automaticGamepadSetup, gamepadSetupFromProfile, snapshotGamepadDevices, snapshotGamepadSetup } from "./gamepad-profile.mjs";

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
  assert.deepEqual(saved.lanes, [0x11, 0x14], "pressed and touched Button fields cover lanes; both axis kinds remain excluded");
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

test("touched-only bindings cover press lanes through their own canonical Button namespace and original sample transitions", () => {
  const saved = snapshotGamepadSetup(setup([[0x12, 3, 1]]));
  assert.deepEqual(saved.lanes, [0x12]);
  assert.deepEqual(Array.from(saved.physicalWords), [0x12, 1, 0x44332211, 0x88776655, 1, 0x57475044, 0x30001]);
  const owner = new GamepadAdapter(saved);
  const initial = sample({ buttons: [button(false), button(true, 1, false)] });
  assert.deepEqual(owner.decode(initial), [], "initial untouched state emits no Up even when the independent pressed level is true");
  assert.deepEqual(owner.decode({ ...initial, sequence: SEQUENCE + 1n,
    buttons: [button(false), button(false, 0.75, false)] }), [], "unbound pressed/value changes cannot impersonate touched input");
  const touch = { ...initial, sequence: SEQUENCE + 2n, timestampMs: 1234.125, hostNs: 1234125000n,
    buttons: [button(false), button(false, 0, true)] };
  const [down] = owner.decode(touch);
  assert.equal(down.length, 69); assert.equal(down[6], 0);
  assert.deepEqual(Array.from(down.slice(59)), [1, 0x44, 0x50, 0x47, 0x57, 1, 0, 3, 0, 0]);
  const metadata = view(down);
  assert.equal(metadata.getBigUint64(7, true), SOURCE);
  assert.equal(metadata.getBigInt64(15, true), 1234125000n);
  assert.equal(metadata.getUint32(23, true), 0x57494e);
  assert.equal(metadata.getBigUint64(27, true), SEQUENCE + 2n);
  assert.equal(down[35], 1); assert.equal(metadata.getUint32(36, true), 0x57475044);
  assert.equal(down[40], 1); assert.equal(metadata.getUint32(41, true), 0x30001);
  assert.equal(down[45], 1); assert.equal(metadata.getUint32(46, true), 0x57494e);
  assert.equal(metadata.getBigInt64(50, true), 1234125000n); assert.equal(down[58], 0);
  assert.deepEqual(owner.decode({ ...touch, sequence: SEQUENCE + 3n }), [], "equal-time unchanged contact level is not another Down");
  const [up] = owner.decode({ ...touch, sequence: SEQUENCE + 4n, timestampMs: 1234.25, hostNs: 1234250000n,
    buttons: [button(false), button(true, 1, false)] });
  assert.equal(up.length, 69); assert.equal(up[6], 0); assert.equal(code(up), 0x30001); assert.equal(up[68], 1);
  assert.equal(view(up).getBigUint64(7, true), SOURCE); assert.equal(view(up).getBigInt64(15, true), 1234250000n);
  assert.equal(view(up).getBigUint64(27, true), SEQUENCE + 4n);
  const pressed = new GamepadAdapter(setup([[0x11, 0, 1]]));
  const [separate] = pressed.decode({ ...touch, buttons: [button(false), button(true, 1, true)] });
  assert.equal(code(separate), 1, "pressed and touched retain distinct Native control identities on the same button");
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

const profileBytes = value => new TextEncoder().encode(typeof value === "string" ? value : JSON.stringify(value));
const profileDocument = (...profiles) => ({ version: 1, profiles });
const customProfile = (fields = {}) => ({ id: "非標準 controller", mapping: "", buttons: 2, axes: 1,
  bindingWords: [0x11, 0, 1, 0x12, 1, 0, 0x13, 2, 0, 0x14, 3, 1], ...fields });
const customDevice = (source = SOURCE, index = 7, fields = {}) => ({ source, index, id: "非標準 controller", mapping: "", buttons: 2, axes: 1, ...fields });

test("custom profiles snapshot real descriptors and preserve device order, full identities and explicit physical control types", () => {
  const raw = [customDevice(9n, 0, { id: "unmatched standard", mapping: "standard", buttons: 9 }), customDevice(), customDevice(3n, 1)];
  const devices = snapshotGamepadDevices(raw);
  raw[1].source = 4n; raw[1].id = "mutated"; raw.length = 0;
  assert.ok(Object.isFrozen(devices) && devices.every(Object.isFrozen));
  assert.equal(devices[1].source, SOURCE); assert.equal(devices[1].id, "非標準 controller");
  const bytes = profileBytes(profileDocument(customProfile(), customProfile({ id: "unused profile" })));
  const output = gamepadSetupFromProfile(bytes, devices);
  assert.deepEqual(output.sources, [SOURCE, 3n]);
  assert.deepEqual(output.devices, [{ source: SOURCE, buttons: 2, axes: 1 }, { source: 3n, buttons: 2, axes: 1 }]);
  assert.deepEqual(output.lanes, [0x11, 0x14], "both Button kinds cover lanes while axis and button-value fields remain axes");
  assert.deepEqual(Array.from(output.bindingWords), [
    0x11, 0x44332211, 0x88776655, 0, 1, 0x12, 0x44332211, 0x88776655, 1, 0,
    0x13, 0x44332211, 0x88776655, 2, 0, 0x14, 0x44332211, 0x88776655, 3, 1,
    0x11, 3, 0, 0, 1, 0x12, 3, 0, 1, 0, 0x13, 3, 0, 2, 0, 0x14, 3, 0, 3, 1,
  ]);
  assert.deepEqual(Array.from(output.physicalWords.slice(0, 7)), [0x11, 1, 0x44332211, 0x88776655, 1, 0x57475044, 1]);
  assert.deepEqual(Array.from(output.physicalWords.slice(-7)), [0x14, 1, 3, 0, 1, 0x57475044, 0x30001]);
  const owner = new GamepadAdapter(output);
  const packets = owner.decode(sample({ mapping: "", id: "非標準 controller", buttons: [button(false), button(true)] }));
  assert.deepEqual(packets.map(code), [1, 0x10000, 0x20000]);
  const backing = new Uint8Array(bytes.length + 4); backing.set(bytes, 2);
  assert.deepEqual(gamepadSetupFromProfile(backing.subarray(2, -2), devices).sources, [SOURCE, 3n]);
  const maximum = new Uint8Array(1048576).fill(32); maximum.set(bytes);
  assert.deepEqual(gamepadSetupFromProfile(maximum, devices).sources, [SOURCE, 3n]);
  const rows = Array.from({ length: 128 }, (_, index) => [0x11, 0, index]).flat();
  const broad = profileDocument({ bindingWords: rows });
  const pair = [customDevice(SOURCE, 0, { buttons: 128 }), customDevice(3n, 1, { buttons: 128 })];
  assert.equal(gamepadSetupFromProfile(profileBytes(broad), pair).bindingWords.length, 256 * 5);
  assert.throws(() => gamepadSetupFromProfile(profileBytes(profileDocument({ bindingWords: [...rows, 0x12, 3, 0] })), pair));
});

test("profile files refuse malformed UTF-8, ambiguous or unmatched selection and every invalid row including unused profiles", () => {
  const devices = [customDevice()];
  for (const bytes of [new Uint8Array(), new Uint8Array(1048577), Uint8Array.from([0xc3, 0x28]),
    profileBytes("{"), profileBytes("null"), profileBytes('{"version":1,"profiles":[]} trailing'), [1, 2]]) {
    assert.throws(() => gamepadSetupFromProfile(bytes, devices));
  }
  const detached = profileBytes(profileDocument(customProfile())); structuredClone(detached.buffer, { transfer: [detached.buffer] });
  assert.throws(() => gamepadSetupFromProfile(detached, devices));
  const invalid = [
    { version: 2, profiles: [customProfile()] }, { ...profileDocument(customProfile()), extra: true },
    profileDocument(), profileDocument(...Array.from({ length: 17 }, () => customProfile())),
    profileDocument(customProfile({ extra: true })), profileDocument(customProfile({ id: 1 })),
    profileDocument(customProfile({ id: "x".repeat(1025) })), profileDocument(customProfile({ mapping: "vendor" })),
    profileDocument(customProfile({ buttons: 129 })), profileDocument(customProfile({ axes: 65 })),
    profileDocument(customProfile({ bindingWords: [] })), profileDocument(customProfile({ bindingWords: [0x11, 0] })),
    profileDocument(customProfile({ bindingWords: [0x20, 0, 0] })), profileDocument(customProfile({ bindingWords: [0x11, 4, 0] })),
    profileDocument(customProfile({ bindingWords: [0x11, 0, 2] })), profileDocument(customProfile({ bindingWords: [0x11, 1, 1] })),
    profileDocument(customProfile({ bindingWords: [0x11, 0, 0, 0x11, 0, 0] })),
    profileDocument(customProfile(), { id: "unused", bindingWords: [0x11, 0, 128] }),
    profileDocument(customProfile(), { id: "unused", bindingWords: [0x11, 1, 64] }),
    profileDocument(customProfile(), { id: "unused", axes: 0, bindingWords: [0x11, 1, 0] }),
    profileDocument(customProfile({ id: "no matching product" })),
    profileDocument(customProfile(), { bindingWords: [0x11, 0, 0] }),
  ];
  for (const value of invalid) assert.throws(() => gamepadSetupFromProfile(profileBytes(value), devices));
  for (const value of [-1, 0x100000000, 0.5, "1", null]) {
    assert.throws(() => gamepadSetupFromProfile(profileBytes(profileDocument(customProfile({ bindingWords: [0x11, 0, value] }))), devices));
  }
  const dynamicBounds = profileBytes(profileDocument({ bindingWords: [0x11, 0, 127] }));
  assert.throws(() => gamepadSetupFromProfile(dynamicBounds, devices), "statically valid controls still require actual matched device capacity");
  for (const raw of [new Array(1), [customDevice(2n)], [customDevice(), customDevice(SOURCE, 0)],
    [customDevice(), customDevice(3n, 7)], [customDevice(SOURCE, 64)]]) assert.throws(() => snapshotGamepadDevices(raw));
  assert.throws(() => gamepadSetupFromProfile(profileBytes(profileDocument(customProfile())), []));
});
