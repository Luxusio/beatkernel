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

// Immutable handle admission only. The Worker owns binary validation and construction.
export function validateCapturedReplay(message, { playId, recording, natural, mode }) {
  if (!message || typeof message !== "object" || !identity(playId) || message.playId !== playId
    || typeof recording !== "boolean" || typeof natural !== "boolean"
    || (mode !== "live" && mode !== "replay")) throw new Error("Invalid captured replay owner.");
  const { replay, replayComplete, replayError } = message;
  if (!recording && replay === undefined && replayComplete === undefined && replayError === undefined) {
    return { file: null, complete: false, error: null };
  }
  if (typeof replayComplete !== "boolean" || !(replayError === null
    || (typeof replayError === "string" && replayError.length <= 4096))) {
    throw new Error("Invalid replay export metadata.");
  }
  if ((!recording || mode !== "live") && (replay !== null || replayError !== null)) {
    throw new Error("Captured replay does not belong to a recorded live owner.");
  }
  if (replay === null) {
    if (replayComplete) throw new Error("Complete replay has no encoded data.");
    return { file: null, complete: false, error: replayError };
  }
  if (typeof File !== "function" || !(replay instanceof File)
    || (replayComplete && (!natural || message.kind !== "play-stopped"))) {
    throw new Error("Invalid replay export ownership or immutable handle.");
  }
  // Native metadata getters reject prototype-shaped impostors without reading the body.
  Object.getOwnPropertyDescriptor(File.prototype, "name").get.call(replay);
  const size = Object.getOwnPropertyDescriptor(Blob.prototype, "size").get.call(replay);
  if (size < 1 || size > 64 * 1024 * 1024) throw new Error("Invalid captured replay size.");
  return { file: replay, complete: replayComplete, error: replayError };
}

export function validateCompletedArchive(message, { playId, players, recording, mode }) {
  if (!message || typeof message !== "object" || !Number.isSafeInteger(playId) || playId <= 0
    || message.playId !== playId || !Array.isArray(players) || players.length < 1 || players.length > 64
    || Array.from(players).some(player => !Number.isInteger(player) || player < 1 || player > 0xffffffff)
    || new Set(players).size !== players.length || typeof recording !== "boolean"
    || (mode !== "live" && mode !== "replay")) throw new Error("Invalid completed archive owner or original roster.");
  const { completedArchive, archivePlayers, archiveError } = message;
  if (completedArchive === undefined && archivePlayers === undefined && archiveError === undefined) {
    return { blob: null, error: null };
  }
  if (!(archiveError === null || (typeof archiveError === "string" && archiveError.length >= 1 && archiveError.length <= 4096))) {
    throw new Error("Invalid completed archive diagnostic.");
  }
  if ((!recording || mode !== "live") && (completedArchive !== null || archiveError !== null)) {
    throw new Error("Completed archive does not belong to a recorded live owner.");
  }
  if (completedArchive === null) {
    if (archivePlayers !== null) throw new Error("Absent completed archive has a roster.");
    return { blob: null, error: archiveError };
  }
  const blob = completedArchive;
  if (archiveError !== null || typeof Blob !== "function" || !(blob instanceof Blob)
    || !Array.isArray(archivePlayers) || archivePlayers.length !== players.length
    || players.some((player,index) => archivePlayers[index] !== player)) {
    throw new Error("Invalid completed archive immutable handle or original roster.");
  }
  const size = Object.getOwnPropertyDescriptor(Blob.prototype, "size").get.call(blob);
  if (size < 1 || size > 5 * 1024 * 1024) throw new Error("Invalid completed archive size.");
  return { blob, error: null };
}
