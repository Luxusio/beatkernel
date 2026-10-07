const U64_MAX = 18446744073709551615n;
const sourceId = value => typeof value === "bigint" && value >= 0n && value <= U64_MAX;
const failure = error => String(error?.message ?? error).slice(0, 4096);

// IDs belong to this roster, not its visible row index or a device description.
export class LocalRoster {
  #players = [1];
  #next = 2;
  #sources = new Map();
  get players() { return Object.freeze([...this.#players]); }
  setCount(count) {
    if (!Number.isInteger(count) || count < 1 || count > 64) throw new Error("Choose one to 64 local players.");
    const added = Math.max(0, count - this.#players.length);
    if (this.#next + added - 1 > 0xffffffff) throw new Error("Local player identities exhausted.");
    const players = this.#players.slice(0, count);
    for (let index = 0; index < added; index++) players.push(this.#next++);
    for (const player of this.#players) if (!players.includes(player)) this.#sources.delete(player);
    this.#players = players;
    if (count === 1) this.#sources.clear();
  }
  assign(player, source) {
    if (!this.#players.includes(player) || !(source === null || sourceId(source))) throw new Error("Invalid local player or source selection.");
    if (source !== null && [...this.#sources].some(([other, value]) => other !== player && value === source)) {
      throw new Error("Each local player needs a different acquired source.");
    }
    if (source === null) this.#sources.delete(player);
    else this.#sources.set(player, source);
  }
  selected(player) { return this.#sources.get(player) ?? null; }
  clearSources() { this.#sources.clear(); }
  snapshot(ownedSources, page = 0, includeSolo = false) {
    if (typeof includeSolo !== "boolean") throw new Error("Invalid automatic local plan option.");
    if (!Number.isInteger(page) || page < 0 || page >= Math.ceil(this.#players.length / 4)) throw new Error("Invalid local player page.");
    if (this.#players.length === 1) {
      if (!includeSolo) return null;
      return Object.freeze({ words: new Uint32Array([this.#players[0], 0, 0, 0]), players: this.players,
        sources: Object.freeze([]), page: 0, automatic: true });
    }
    if (!Array.isArray(ownedSources) || ownedSources.length > 34
      || Array.from(ownedSources).some(source => !sourceId(source)) || new Set(ownedSources).size !== ownedSources.length) {
      throw new Error("Invalid acquired source inventory.");
    }
    const sources = this.#players.map(player => this.selected(player));
    if (sources.some(source => source === null || !ownedSources.includes(source)) || new Set(sources).size !== sources.length) {
      throw new Error("Assign one distinct acquired source to every local player.");
    }
    const words = new Uint32Array(this.#players.length * 4);
    sources.forEach((source, index) => words.set([this.#players[index], 1, Number(source & 0xffffffffn), Number(source >> 32n)], index * 4));
    return Object.freeze({ words, players: this.players, sources: Object.freeze(sources), page });
  }
}

export function validateLocalPrepared(plan, metadata, recording) {
  if (typeof recording !== "boolean" || !Array.isArray(metadata?.localPlayers) || metadata.localPlayers.length !== plan.players.length
    || Array.from(metadata.localPlayers).some((player, index) => player !== plan.players[index]) || metadata.localPage !== plan.page) {
    throw new Error("Preparation changed the local player roster or page.");
  }
  let recordLimits = null;
  if (recording) {
    const bytes = Math.floor(64 * 1024 * 1024 / plan.players.length);
    const records = Math.floor(1000000 / plan.players.length);
    if (metadata.recordLimits?.bytes !== bytes || metadata.recordLimits?.records !== records) {
      throw new Error("Preparation changed the per-player recording budget.");
    }
    recordLimits = Object.freeze({ bytes, records });
  }
  return Object.freeze({ page: metadata.localPage, recordLimits });
}

export function localReplayReceipt(plan, data, { recording, natural, bytesPerMember }) {
  const empty = (player, error = null) => ({ player, replay: null, replayError: error, replayComplete: false });
  const budget = Math.floor(64 * 1024 * 1024 / plan.players.length);
  if (!Array.isArray(data?.replays) || data.replays.length !== plan.players.length
    || data.replay !== null || data.replayComplete !== false || typeof recording !== "boolean" || typeof natural !== "boolean"
    || !Number.isSafeInteger(bytesPerMember) || bytesPerMember < 1 || bytesPerMember > budget) {
    return plan.players.map(player => empty(player, "Invalid local replay receipt."));
  }
  let total = 0;
  const buffers = new Set();
  return plan.players.map((player, index) => {
    try {
      const row = data.replays[index];
      if (!row || row.player !== player || typeof row.replayComplete !== "boolean"
        || !(row.replayError === null || (typeof row.replayError === "string" && row.replayError.length <= 4096))) {
        throw new Error("Invalid player replay metadata.");
      }
      if (row.replay === null) {
        if (row.replayComplete) throw new Error("Complete player replay has no data.");
        return empty(player, row.replayError);
      }
      const bytes = row.replay;
      if (!recording || row.replayError !== null || !(bytes instanceof Uint8Array)
        || !(bytes.buffer instanceof ArrayBuffer) || bytes.buffer.resizable === true
        || bytes.byteOffset !== 0 || bytes.length !== bytes.buffer.byteLength || bytes.length === 0
        || bytes.length > bytesPerMember || total + bytes.length > 64 * 1024 * 1024 || buffers.has(bytes.buffer)
        || (row.replayComplete && (!natural || data.kind !== "play-stopped"))) {
        throw new Error("Invalid player replay ownership or bounded layout.");
      }
      total += bytes.length;
      buffers.add(bytes.buffer);
      return { player, replay: bytes, replayError: null, replayComplete: row.replayComplete };
    } catch (error) { return empty(player, failure(error)); }
  });
}
