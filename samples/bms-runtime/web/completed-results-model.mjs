// Finite result metadata only; scores, gauges and time remain Rust-owned exact integers.
const identity = value => Number.isSafeInteger(value) && value > 0;
export function validateCompletedResults(value) {
  if (!value || typeof value !== "object" || Array.isArray(value) || value.proof !== true
    || !Array.isArray(value.players) || value.players.length < 1 || value.players.length > 64
    || value.players.some(player => !Number.isInteger(player) || player < 1 || player > 4294967295)
    || new Set(value.players).size !== value.players.length
    || typeof value.comparisons !== "boolean" || typeof value.hasComparisons !== "boolean"
    || typeof value.failed !== "boolean" || !Number.isInteger(value.page) || !Number.isInteger(value.pages)
    || value.pages < 0 || value.pages > 4294967295 || value.page < 0
    || (value.failed ? value.pages !== 0 || value.page !== 0 : value.pages < 1 || value.page >= value.pages)
    || (value.comparisons && !value.hasComparisons)) throw new Error("Invalid completed Results metadata.");
  const detailPages = value.detailPages ?? value.pages;
  const comparisonPages = value.comparisonPages ?? (value.hasComparisons ? value.pages : 0);
  if (![detailPages, comparisonPages].every(count => Number.isInteger(count) && count >= 0 && count <= 4294967295)
    || (value.failed ? detailPages !== 0 || comparisonPages !== 0 : detailPages < 1)
    || (value.hasComparisons !== (comparisonPages > 0))
    || value.pages !== (value.comparisons ? comparisonPages : detailPages)) throw new Error("Invalid Results mode page counts.");
  return Object.freeze({ proof: true, players: Object.freeze([...value.players]), page: value.page,
    pages: value.pages, detailPages, comparisonPages, comparisons: value.comparisons, hasComparisons: value.hasComparisons, failed: value.failed });
}
export function resultRequest(results, request) {
  if (!results || !request || !identity(results.id) || request.playId !== results.id
    || !identity(request.rpcId) || !Number.isSafeInteger(results.lastRpc) || request.rpcId <= results.lastRpc
    || results.ready !== true || results.failed) throw new Error("Completed Results request has no released matching owner.");
  if (request.kind === "play-results-present") return { ...results, lastRpc: request.rpcId, shown: true };
  if (request.kind !== "play-results-page" || results.shown !== true || typeof request.comparisons !== "boolean"
    || (request.comparisons && !results.hasComparisons) || !Number.isInteger(request.page) || request.page < 0)
    throw new Error("Invalid completed Results page request.");
  const pages = request.comparisons ? (results.comparisonPages ?? results.pages) : (results.detailPages ?? results.pages);
  if (request.page >= pages) throw new Error("Completed Results page is out of range.");
  return { ...results, lastRpc: request.rpcId, page: request.page, pages, comparisons: request.comparisons };
}

// Opaque transfer admission only. Rust owns binary decoding and completion classification.
export function validateCompletedArchive(message, { playId, players, recording, mode }) {
  if (!message || typeof message !== "object" || !Number.isSafeInteger(playId) || playId <= 0
    || message.playId !== playId || !Array.isArray(players) || players.length < 1 || players.length > 64
    || Array.from(players).some(player => !Number.isInteger(player) || player < 1 || player > 0xffffffff)
    || new Set(players).size !== players.length || typeof recording !== "boolean"
    || (mode !== "live" && mode !== "replay")) throw new Error("Invalid completed archive owner or original roster.");
  const { completedArchive, archivePlayers, archiveError } = message;
  if (completedArchive === undefined && archivePlayers === undefined && archiveError === undefined) {
    return { bytes: null, error: null };
  }
  if (!(archiveError === null || (typeof archiveError === "string" && archiveError.length >= 1 && archiveError.length <= 4096))) {
    throw new Error("Invalid completed archive diagnostic.");
  }
  if ((!recording || mode !== "live") && (completedArchive !== null || archiveError !== null)) {
    throw new Error("Completed archive does not belong to a recorded live owner.");
  }
  if (completedArchive === null) {
    if (archivePlayers !== null) throw new Error("Absent completed archive has a roster.");
    return { bytes: null, error: archiveError };
  }
  const bytes = completedArchive;
  if (archiveError !== null || !(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
    || bytes.buffer.resizable === true || bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength
    || bytes.byteLength < 1 || bytes.byteLength > 5 * 1024 * 1024
    || !Array.isArray(archivePlayers) || archivePlayers.length !== players.length
    || players.some((player,index) => archivePlayers[index] !== player)) {
    throw new Error("Invalid completed archive transferable layout or original roster.");
  }
  if (message.replay?.buffer === bytes.buffer || (Array.isArray(message.replays)
    && (message.replays.length > 64 || message.replays.some(row => row?.replay?.buffer === bytes.buffer)))) {
    throw new Error("Completed archive must own a separate transferable buffer.");
  }
  return { bytes, error: null };
}
