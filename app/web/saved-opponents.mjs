// Files are immutable handles here. Selection and snapshots never acquire bytes.
export const OPPONENT_LIMITS = Object.freeze({ count: 8, bytes: 64 * 1024 * 1024, labelBytes: 256, sourceKeyBytes: 1024 });
const encoder = new TextEncoder();
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;
const U64_MAX = 18446744073709551615n;

function text(value, limit, field) {
  if (typeof value !== "string" || value.length === 0 || value.length > limit
    || /[\u0000-\u001f\u007f-\u009f]/u.test(value)) throw new TypeError(`Invalid opponent ${field}.`);
  for (const scalar of value) {
    const code = scalar.codePointAt(0);
    if (code >= 0xd800 && code <= 0xdfff) throw new TypeError(`Invalid opponent ${field} Unicode.`);
  }
  if (encoder.encode(value).byteLength > limit) throw new RangeError(`Opponent ${field} is too long.`);
  return value;
}

function descriptor(value) {
  if (!value || !(value.file instanceof File) || typeof value.own !== "boolean") throw new TypeError("An opponent needs an actual File and explicit Own/Other choice.");
  const size = value.file.size;
  if (!Number.isSafeInteger(size) || size < 1 || size > OPPONENT_LIMITS.bytes) throw new RangeError("Opponent recording must contain 1 byte to 64 MiB.");
  const player = value.player ?? null;
  if (player !== null && (!Number.isInteger(player) || player < 1 || player > 0xffffffff)) throw new TypeError("Invalid saved opponent player.");
  return Object.freeze({ file: value.file, ...(player === null ? {} : { player }),
    sourceKey: text(value.sourceKey, OPPONENT_LIMITS.sourceKeyBytes, "source key"),
    own: value.own, label: text(value.label, OPPONENT_LIMITS.labelBytes, "label") });
}

export function validateSelections(value) {
  if (!Array.isArray(value) || value.length > OPPONENT_LIMITS.count) throw new RangeError("Choose at most eight saved opponents.");
  const keys = new Set();
  let bytes = 0;
  const entries = [];
  for (const entry of value) {
    const accepted = descriptor(entry);
    if (keys.has(accepted.sourceKey)) throw new Error("That recording is already selected as an opponent.");
    keys.add(accepted.sourceKey);
    bytes += accepted.file.size;
    if (bytes > OPPONENT_LIMITS.bytes) throw new RangeError("Selected opponent recordings exceed 64 MiB.");
    entries.push(accepted);
  }
  return Object.freeze(entries);
}

export class SavedOpponentSelection {
  #entries = Object.freeze([]);
  get size() { return this.#entries.length; }
  get byteLength() { return this.#entries.reduce((sum, entry) => sum + entry.file.size, 0); }
  add(value) {
    const next = validateSelections([...this.#entries, value]);
    this.#entries = next;
    return next[next.length - 1];
  }
  remove(sourceKey) {
    const index = this.#entries.findIndex(entry => entry.sourceKey === sourceKey);
    if (index < 0) return false;
    this.#entries = Object.freeze(this.#entries.filter((_, position) => position !== index));
    return true;
  }
  setPlayer(sourceKey, player) {
    const index = this.#entries.findIndex(entry => entry.sourceKey === sourceKey);
    if (index < 0) throw new Error("Selected saved opponent is no longer available.");
    const next = this.#entries.map((entry, position) => position === index ? { ...entry, player } : entry);
    this.#entries = validateSelections(next);
  }
  clear() { this.#entries = Object.freeze([]); }
  snapshot() { return Object.freeze(this.#entries.slice()); }
}

export function opponentLabel(filename) {
  if (typeof filename !== "string") throw new TypeError("A recording filename is required.");
  let label = "";
  let bytes = 0;
  for (const scalar of filename) {
    const code = scalar.codePointAt(0);
    const clean = code < 0x20 || (code >= 0x7f && code <= 0x9f) || (code >= 0xd800 && code <= 0xdfff) ? " " : scalar;
    const count = encoder.encode(clean).byteLength;
    if (bytes + count > OPPONENT_LIMITS.labelBytes) break;
    label += clean;
    bytes += count;
  }
  return label.trim() || "Saved replay";
}

export function validateOpponentSnapshot(value, expectedCount) {
  if (!Number.isInteger(expectedCount) || expectedCount < 1 || expectedCount > OPPONENT_LIMITS.count
    || !Array.isArray(value) || value.length !== expectedCount) throw new RangeError("Saved opponent result count changed.");
  const time = item => typeof item === "bigint" && item >= I64_MIN && item <= I64_MAX;
  const counter = item => typeof item === "bigint" && item >= 0n && item <= U64_MAX;
  return Object.freeze(Array.from(value, row => {
    if (!row || (row.kind !== "own" && row.kind !== "other") || !time(row.songNs)
      || (row.recordedUntilNs !== null && !time(row.recordedUntilNs))
      || !counter(row.hits) || !counter(row.misses) || !counter(row.combo) || !counter(row.maxCombo)
      || row.combo > row.maxCombo || row.maxCombo > row.hits) throw new TypeError("Invalid saved opponent result.");
    return Object.freeze({ kind: row.kind, label: text(row.label, OPPONENT_LIMITS.labelBytes, "label"),
      songNs: row.songNs, recordedUntilNs: row.recordedUntilNs,
      hits: row.hits, misses: row.misses, combo: row.combo, maxCombo: row.maxCombo });
  }));
}

export function validateOpponentTargets(selections, players = null) {
  if (!Array.isArray(selections) || selections.length > OPPONENT_LIMITS.count
    || (players !== null && (!Array.isArray(players) || players.length < 1 || players.length > 64
      || Array.from(players).some(player => !Number.isInteger(player) || player < 1 || player > 0xffffffff)
      || new Set(players).size !== players.length))) throw new Error("Invalid saved comparison roster or selections.");
  for (const entry of selections) {
    if (!entry || (players === null ? entry.player != null : !players.includes(entry.player))) {
      throw new Error("Assign each saved opponent to a current local player, or clear its target for solo play.");
    }
  }
}

export function validateLocalOpponentSnapshot(value, players, selections) {
  validateOpponentTargets(selections, players);
  if (!Array.isArray(value) || value.length !== players.length
    || Array.from(value).some((row, index) => !row || row.player !== players[index])) {
    throw new Error("Saved opponent member ownership changed.");
  }
  return Object.freeze(Array.from(value, row => {
    const selected = selections.filter(entry => entry.player === row.player);
    const count = selected.length;
    try {
      if (row.error !== null) {
        if (typeof row.error !== "string" || row.error.length < 1 || row.error.length > 4096 || row.opponents !== null) throw new Error("Invalid member comparison failure.");
        return Object.freeze({ player: row.player, opponents: null, error: row.error });
      }
      const opponents = count === 0
        ? Array.isArray(row.opponents) && row.opponents.length === 0 ? Object.freeze([]) : null
        : validateOpponentSnapshot(row.opponents, count);
      if (opponents === null) throw new Error("Unexpected member saved opponents.");
      if (opponents.some((opponent, index) => opponent.label !== selected[index].label
        || opponent.kind !== (selected[index].own ? "own" : "other"))) throw new Error("Member saved recording selection changed.");
      return Object.freeze({ player: row.player, opponents, error: null });
    } catch (error) {
      return Object.freeze({ player: row.player, opponents: null, error: String(error.message).slice(0, 4096) });
    }
  }));
}
