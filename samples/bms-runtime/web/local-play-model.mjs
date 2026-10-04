// Portable stable player/source ownership, matching the common Rust row ABI.
const KEYBOARD = 0x574b4559;
const TOUCH = 0x57544f55;
const lane = value => Number.isInteger(value)
  && ((value >= 0x11 && value <= 0x19) || (value >= 0x21 && value <= 0x29));

function words(value, width, maximum, name) {
  if (!(value instanceof Uint32Array) || !(value.buffer instanceof ArrayBuffer)
    || value.buffer.resizable === true || value.length > width * maximum || value.length % width !== 0) {
    throw new Error(`${name} requires bounded complete ${width}-word rows.`);
  }
  return value.slice(); // Also refuses a detached backing store.
}

export function snapshotLocalPlan(value) {
  const snapshot = words(value, 4, 64, "Local source plan");
  if (snapshot.length === 0) throw new Error("Local source plan requires one to 64 members.");
  const players = new Set();
  const sources = new Set();
  const members = [];
  for (let index = 0; index < snapshot.length; index += 4) {
    const [player, selector, low, high] = snapshot.subarray(index, index + 4);
    if (player === 0 || players.has(player) || (selector !== 0 && selector !== 1)
      || (selector === 0 && (low !== 0 || high !== 0))) throw new Error("Invalid or repeated local player/source row.");
    const source = selector === 0 ? null : BigInt(low) | (BigInt(high) << 32n);
    if (source === null && snapshot.length !== 4) throw new Error("Multiple local players require exact sources.");
    if (source !== null && sources.has(source)) throw new Error("A local source cannot belong to multiple players.");
    players.add(player);
    if (source !== null) sources.add(source);
    members.push(Object.freeze({ player, source }));
  }
  return Object.freeze({ words: snapshot, members: Object.freeze(members) });
}

export function localBindingWords(plan, physicalWords, chartLanes, touchInput) {
  // The numerical plan is authoritative, including when a caller reconstructed
  // the helper result. No mutable members array can redirect source ownership.
  const selected = snapshotLocalPlan(plan?.words);
  const physical = words(physicalWords, 7, 256, "Physical bindings");
  if ((!Array.isArray(chartLanes) && !(chartLanes instanceof Uint8Array))
    || chartLanes.length > 18 || typeof touchInput !== "boolean") throw new Error("Invalid local lane or touch configuration.");
  const lanes = Array.from(chartLanes);
  if (lanes.some(value => !lane(value)) || new Set(lanes).size !== lanes.length) throw new Error("Local chart lanes must be valid and unique.");
  const automatic = selected.members.length === 1 && selected.members[0].source === null;
  const rows = selected.members.map(() => []);
  const duplicates = new Set();
  for (let offset = 0; offset < physical.length; offset += 7) {
    const row = Array.from(physical.subarray(offset, offset + 7));
    const [destination, selector, low, high, kind, namespace, code] = row;
    if (!lane(destination) || (selector !== 0 && selector !== 1)
      || (selector === 0 && (low !== 0 || high !== 0)) || kind > 2
      || (kind === 0 && (namespace > 65535 || code > 65535))) {
      throw new Error("Invalid local physical identity row.");
    }
    const key = row.join(",");
    if (duplicates.has(key)) throw new Error("Duplicate local physical binding.");
    duplicates.add(key);
    let member = 0;
    if (!automatic) {
      let source;
      if (selector === 0) {
        if (kind !== 1 || namespace !== KEYBOARD) throw new Error("Exact local members cannot borrow an Any physical binding.");
        source = 1n;
        row[1] = 1; row[2] = 1; row[3] = 0;
      } else source = BigInt(low) | (BigInt(high) << 32n);
      member = selected.members.findIndex(candidate => candidate.source === source);
      if (member < 0) continue; // Valid unassigned acquisitions never gain a member.
    }
    rows[member].push(row);
  }
  const touchIndex = !touchInput ? -1 : automatic ? 0
    : selected.members.findIndex(member => member.source === 2n);
  if (touchIndex >= 0) {
    for (const destination of lanes) {
      const row = [destination, automatic ? 0 : 1, automatic ? 0 : 2, 0, 1, TOUCH, 0];
      if (!rows[touchIndex].some(existing => existing.every((value, index) => value === row[index]))) {
        rows[touchIndex].push(row);
      }
    }
  }
  const count = rows.reduce((sum, member) => sum + member.length, 0);
  if (count > 256) throw new Error("Combined local bindings exceed 256 rows.");
  const output = new Uint32Array(count * 8);
  let offset = 0;
  for (let index = 0; index < rows.length; index++) {
    const seen = new Set();
    for (const row of rows[index]) {
      const key = row.join(",");
      if (seen.has(key)) throw new Error("Local source rewriting produced duplicate physical bindings.");
      seen.add(key);
      output.set([selected.members[index].player, ...row], offset);
      offset += 8;
    }
    if (lanes.some(destination => !rows[index].some(row => row[0] === destination))) {
      throw new Error(`Local player ${selected.members[index].player} lacks a prepared lane binding.`);
    }
  }
  return Object.freeze({ words: output, touchPlayer: touchIndex < 0 ? null : selected.members[touchIndex].player });
}
