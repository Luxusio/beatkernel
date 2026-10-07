// Deferred source fixtures: actual profile representation/matching, not a HID decoder.
import assert from "node:assert/strict";
import test from "node:test";
import { hidSetupFromProfile, snapshotHidDevices } from "./hid-profile.mjs";

const U64_MAX = 18446744073709551615n;
const bytes = value => new TextEncoder().encode(typeof value === "string" ? value : JSON.stringify(value));
const device = (source = U64_MAX, vendorId = 1, productId = 2) => ({ source, vendorId, productId });
function profile(fields = {}) {
  return { vendorId: 1, productId: 2,
    bindingWords: [0x11, 2, 0xffffffff, 0xfffffffe],
    fieldWords: [1, 9, 2, 2, 0xffffffff, 0xfffffffe, 8, 8, 1, 1, 1, 1],
    axisParams: [0.5, -0.25], ...fields };
}
const document = (...profiles) => ({ version: 1, profiles });

test("profile matching injects exact full-width acquired sources in device order without mutating metadata or narrowing control words", () => {
  const supplied = [device(9n, 7, 8), device(U64_MAX), device(3n, 5, 6)];
  const snapshot = snapshotHidDevices(supplied);
  supplied[1].source = 4n; supplied[2].productId = 8;
  assert.ok(Object.isFrozen(snapshot) && snapshot.every(Object.isFrozen));
  assert.deepEqual(Array.from(snapshot), [device(9n, 7, 8), device(U64_MAX), device(3n, 5, 6)]);
  const setup = hidSetupFromProfile(bytes(document(profile({ vendorId: 5, productId: 6 }), profile(), profile({ vendorId: 123 }))), snapshot);
  assert.deepEqual(Array.from(setup.deviceWords), [0xffffffff, 0xffffffff, 1, 1, 1, 2, 3, 0, 1, 5, 1, 6]);
  assert.deepEqual(Array.from(setup.bindingWords), [0x11, 1, 0xffffffff, 0xffffffff, 2, 0xffffffff, 0xfffffffe,
    0x11, 1, 3, 0, 2, 0xffffffff, 0xfffffffe]);
  assert.deepEqual(Array.from(setup.fieldWords), [0, 1, 9, 2, 2, 0xffffffff, 0xfffffffe, 8, 8, 1, 1, 1, 1,
    1, 1, 9, 2, 2, 0xffffffff, 0xfffffffe, 8, 8, 1, 1, 1, 1]);
  assert.deepEqual(Array.from(setup.axisParams), [0.5, -0.25, 0.5, -0.25]);
  const first = setup.bindingWords[0]; setup.bindingWords.fill(0);
  const fresh = hidSetupFromProfile(bytes(document(profile())), [device()]);
  assert.equal(fresh.bindingWords[0], first);
  const signedZero = hidSetupFromProfile(bytes(JSON.stringify(document(profile())).replace('"axisParams":[0.5,-0.25]', '"axisParams":[-0,0]')), [device()]);
  assert.equal(Object.is(signedZero.axisParams[0], -0), true, "axis parameters retain their admitted f32 representation");
});

test("profile bytes and numeric dictionaries reject malformed encodings, unknown schema and unsafe typed-array casts before output", () => {
  for (const malformed of [new Uint8Array(), new Uint8Array(1048577), Uint8Array.from([0xc3, 0x28]), bytes("{"), bytes("null"), bytes("{} trailing")]) {
    assert.throws(() => hidSetupFromProfile(malformed, [device()]));
  }
  for (const value of [{ version: 2, profiles: [profile()] }, { ...document(profile()), extra: 1 }, document(), document(profile({ extra: 1 })),
    document(profile({ vendorId: 65536 })), document(profile({ productId: -1 })), document(profile({ bindingWords: [0x11, 0, 9] })),
    document(profile({ fieldWords: new Array(11).fill(0) })), document(profile({ axisParams: [1] })), document(profile({ axisParams: [1e39, 0] })),
    document(profile({ axisParams: [null, 0] })), document(profile({ axisParams: ["1", 0] }))]) {
    assert.throws(() => hidSetupFromProfile(bytes(value), [device()]));
  }
  for (const invalid of [-1, 0x100000000, 0.5, "9", null]) {
    const binding = profile(); binding.bindingWords[3] = invalid;
    const field = profile(); field.fieldWords[6] = invalid;
    assert.throws(() => hidSetupFromProfile(bytes(document(binding)), [device()]));
    assert.throws(() => hidSetupFromProfile(bytes(document(field)), [device()]));
  }
  for (const bad of [[], new Array(1), [device(2n)], [device(U64_MAX + 1n)], [device(3)], [device(3n, 65536)],
    [device(3n, 1, -1)], [device(), device()], Array.from({ length: 17 }, (_, index) => device(BigInt(index + 3)))]) {
    assert.throws(() => snapshotHidDevices(bad));
  }
  const narrow = hidSetupFromProfile(bytes(document(profile({ axisParams: [0.1, -0.1] }))), [device()]);
  assert.deepEqual(Array.from(narrow.axisParams), [Math.fround(0.1), Math.fround(-0.1)]);
});

test("matching requires one unambiguous profile per included device and bounds aggregate expansion without choosing an arbitrary winner", () => {
  assert.throws(() => hidSetupFromProfile(bytes(document(profile({ vendorId: 8 }))), [device()]));
  assert.throws(() => hidSetupFromProfile(bytes(document(profile(), profile({ productId: undefined }))), [device()]));
  assert.throws(() => hidSetupFromProfile(bytes(document(...Array.from({ length: 17 }, () => profile()))), [device()]));
  const any = profile({ vendorId: undefined, productId: undefined });
  const sixteen = Array.from({ length: 16 }, (_, index) => device(BigInt(index + 3), index, 0));
  any.bindingWords = Array.from({ length: 16 }, (_, index) => [0x11, 1, 9, index]).flat();
  any.fieldWords = Array.from({ length: 16 }, (_, index) => [1, 1, 2, 1, 9, index, index, 1, 0, 0, 0, 0]).flat();
  any.axisParams = new Array(32).fill(0);
  const atCapacity = hidSetupFromProfile(bytes(document(any)), sixteen);
  assert.equal(atCapacity.deviceWords.length, 16 * 6);
  assert.equal(atCapacity.bindingWords.length, 256 * 7);
  const over = { ...any, bindingWords: [...any.bindingWords, 0x11, 1, 9, 16] };
  assert.throws(() => hidSetupFromProfile(bytes(document(over)), sixteen));
  for (const oversized of [profile({ bindingWords: new Array(257 * 4).fill(0) }),
    profile({ fieldWords: new Array(513 * 12).fill(0), axisParams: new Array(513 * 2).fill(0) })]) {
    assert.throws(() => hidSetupFromProfile(bytes(document(oversized)), [device()]));
  }
  const exact = bytes(document(profile()));
  const backing = new Uint8Array(exact.length + 2); backing.set(exact, 1);
  assert.deepEqual(Array.from(hidSetupFromProfile(backing.subarray(1, -1), [device()]).deviceWords), [0xffffffff, 0xffffffff, 1, 1, 1, 2]);
  const maximum = new Uint8Array(1048576).fill(32); maximum.set(exact);
  assert.equal(hidSetupFromProfile(maximum, [device()]).deviceWords.length, 6);
});
