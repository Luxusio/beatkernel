// Profile parsing is Worker setup work. Window may only snapshot device metadata.
const U64_MAX = 18446744073709551615n;
const MAX_BYTES = 1024 * 1024;

function integer(value, max) {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= max;
}

function object(value, allowed, required, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || Reflect.ownKeys(value).some(key => !allowed.includes(key))
    || required.some(key => !Object.hasOwn(value, key))) throw new Error(`Invalid ${label} properties.`);
}

export function snapshotHidDevices(devices) {
  if (!Array.isArray(devices) || devices.length < 1 || devices.length > 16) throw new Error("HID setup requires one to sixteen owned devices.");
  const sources = new Set();
  const snapshot = [];
  for (const device of devices) {
    object(device, ["source", "vendorId", "productId"], ["source", "vendorId", "productId"], "HID device");
    const { source, vendorId, productId } = device;
    if (typeof source !== "bigint" || source < 3n || source > U64_MAX || sources.has(source)
      || !integer(vendorId, 65535) || !integer(productId, 65535)) throw new Error("HID device identity or hardware IDs are invalid.");
    sources.add(source);
    snapshot.push(Object.freeze({ source, vendorId, productId }));
  }
  return Object.freeze(snapshot);
}

function words(value, width, rows, label) {
  if (!Array.isArray(value) || value.length > rows * width || value.length % width !== 0) throw new Error(`Invalid bounded ${label} rows.`);
  for (const word of value) if (!integer(word, 0xffffffff)) throw new Error(`${label} words must be unsigned 32-bit integers.`);
}

export function hidSetupFromProfile(bytes, devices) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength < 1 || bytes.byteLength > MAX_BYTES) throw new Error("HID profile must contain one to 1048576 UTF-8 bytes.");
  // Constructing this view also rejects detached buffers, without reading any file.
  const input = new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(input));
  object(parsed, ["version", "profiles"], ["version", "profiles"], "HID profile document");
  if (parsed.version !== 1 || !Array.isArray(parsed.profiles) || parsed.profiles.length < 1 || parsed.profiles.length > 16) {
    throw new Error("HID profile version 1 requires one to sixteen profiles.");
  }
  for (const profile of parsed.profiles) {
    object(profile, ["vendorId", "productId", "bindingWords", "fieldWords", "axisParams"],
      ["bindingWords", "fieldWords", "axisParams"], "HID profile");
    for (const key of ["vendorId", "productId"]) {
      if (Object.hasOwn(profile, key) && !integer(profile[key], 65535)) throw new Error("HID profile hardware matchers must fit unsigned sixteen bits.");
    }
    words(profile.bindingWords, 4, 256, "HID binding");
    words(profile.fieldWords, 12, 512, "HID field");
    if (!Array.isArray(profile.axisParams) || profile.axisParams.length !== profile.fieldWords.length / 12 * 2) {
      throw new Error("HID fields require exactly two floating parameters per row.");
    }
    for (const parameter of profile.axisParams) {
      if (typeof parameter !== "number" || !Number.isFinite(parameter) || !Number.isFinite(Math.fround(parameter))) {
        throw new Error("HID axis parameters must be finite float32 values.");
      }
    }
  }
  const selected = [];
  let bindingRows = 0;
  let fieldRows = 0;
  for (const device of snapshotHidDevices(devices)) {
    let matched = null;
    for (const profile of parsed.profiles) {
      if ((profile.vendorId === undefined || profile.vendorId === device.vendorId)
        && (profile.productId === undefined || profile.productId === device.productId)) {
        if (matched !== null) throw new Error("An owned HID device matches more than one profile.");
        matched = profile;
      }
    }
    if (matched === null) continue;
    bindingRows += matched.bindingWords.length / 4;
    fieldRows += matched.fieldWords.length / 12;
    if (bindingRows > 256 || fieldRows > 16 * 512) throw new Error("Matched HID profiles exceed the combined binding or field capacity.");
    selected.push({ device, profile: matched });
  }
  if (selected.length === 0) throw new Error("No authorized HID device matches the selected profile.");
  const bindingWords = new Uint32Array(bindingRows * 7);
  const deviceWords = new Uint32Array(selected.length * 6);
  const fieldWords = new Uint32Array(fieldRows * 13);
  const axisParams = new Float32Array(fieldRows * 2);
  let bindingOffset = 0;
  let fieldOffset = 0;
  let parameterOffset = 0;
  for (let index = 0; index < selected.length; index++) {
    const { device, profile } = selected[index];
    const low = Number(device.source & 0xffffffffn);
    const high = Number(device.source >> 32n);
    deviceWords.set([low, high, 1, device.vendorId, 1, device.productId], index * 6);
    for (let row = 0; row < profile.bindingWords.length; row += 4) {
      bindingWords.set([profile.bindingWords[row], 1, low, high,
        profile.bindingWords[row + 1], profile.bindingWords[row + 2], profile.bindingWords[row + 3]], bindingOffset);
      bindingOffset += 7;
    }
    for (let row = 0; row < profile.fieldWords.length; row += 12) {
      fieldWords[fieldOffset++] = index;
      for (let word = 0; word < 12; word++) fieldWords[fieldOffset++] = profile.fieldWords[row + word];
    }
    axisParams.set(profile.axisParams, parameterOffset);
    parameterOffset += profile.axisParams.length;
  }
  return { bindingWords, deviceWords, fieldWords, axisParams };
}
