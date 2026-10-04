import { snapshotFiles, nanoseconds, seconds } from "./host_model.mjs";
import { AudioHost } from "./audio-host.mjs";
import { RecordsStore } from "./record-store.mjs";
import { HidInputOwner } from "./hid-input.mjs";
import { GamepadInputOwner } from "./gamepad-input.mjs";
import { LocalRoster, validateLocalPrepared, localReplayReceipt } from "./local-play-host.mjs";
import { snapshotHidDevices } from "./hid-profile.mjs";
import { SavedOpponentSelection, opponentLabel, validateOpponentSnapshot } from "./saved-opponents.mjs";
import { KEY_BINDINGS, KEY_CHOICES, PLAY_PCM_SAMPLES, snapshotBindings, bindingsFor, timingFromMilliseconds, audioOutputFromFields, audioLimitsFromFields, sectionFromSeconds, validateStart, replayOutputFromMetadata, millisecondsToNanos, startProjection, committedStartProjection } from "./play-model.mjs";

const byId = id => document.getElementById(id);
const ui = Object.fromEntries(["folder", "files", "chart", "rate", "seed", "prepare", "position", "seek", "title", "details", "status", "viewport", "play", "stop", "keys", "record", "export", "replay-file", "replay-play", "replay-name", "records", "records-refresh", "records-save", "records-use", "records-delete", "multiplayer", "multiplayer-url", "multiplayer-role", "multiplayer-status", "opponents-kind", "opponents-label", "opponents-add", "records-opponent", "opponents-clear", "opponents-list", "opponents-status", "opponents-results", "judge-early", "judge-late", "judge-offset", "live-start", "live-end", "bindings", "bindings-reset", "output-latency", "output-latency-ms", "output-rate", "audio-queue", "audio-voices", "audio-pending", "audio-frames", "audio-commands", "touch-input", "hid-input", "hid-authorize", "hid-profile", "hid-profile-name", "hid-status", "gamepad-profile", "gamepad-profile-name", "gamepad-profile-clear"].map(id => [id, byId(id)]));
for (const id of ["local-count", "local-discover", "local-release", "local-sources", "local-status", "local-page", "local-results", "captured-replay"]) ui[id] = byId(id);
let canvas = byId("canvas");
let cssExtent = [0, 0];
ui["touch-input"].checked = typeof window.PointerEvent === "function" && globalThis.navigator?.maxTouchPoints > 0;
let worker = null;
let observer = null;
let density = null;
let owner = 0;
let serial = 0;
let importId = 0;
let libraryId = 0;
let selectId = 0;
let selectedId = 0;
let seekId = 0;
let initialized = false;
let importing = false;
let preparing = false;
let hasPreview = false;
let audioModule = null;
let activePlay = null;
let lastReplay = null;
let capturedReplays = [];
let replayURL = null;
let replayURLTimer = null;
let selectedReplay = null;
let recordsStore = null;
let recordsOperation = null;
let selectedHidProfile = null;
let selectedGamepadProfile = null;
let hidPermission = null;
let hidOwnershipFailed = false;
const opponents = new SavedOpponentSelection();
let selectedReplayKey = null;
let importedReplayKeys = new WeakMap();
let importedReplayId = 0;
let opponentButtons = [];
let opponentResultRows = [];
const bindingFields = createBindingFields();
const localRoster = new LocalRoster();
let localSetup = null;
let localCleanup = null;
let localDiscovery = null;
let localFields = [];

function inputOwnerCurrent(session) {
  return (activePlay === session || localSetup === session) && session.owner === owner && session.phase !== "closing";
}

function showLocalRoster() {
  const fields = document.createDocumentFragment();
  localFields = localRoster.players.length === 1 ? [] : localRoster.players.map(player => {
    const label = document.createElement("label");
    label.textContent = `Player ${player}`;
    const select = document.createElement("select");
    select.id = `local-source-${player}`;
    select.append(new Option("Choose an acquired source", ""));
    for (const row of localSetup?.inventory ?? []) select.append(new Option(row.label, row.source.toString()));
    select.value = localRoster.selected(player)?.toString() ?? "";
    select.addEventListener("change", () => {
      if (activePlay || localSetup?.phase !== "ready") return;
      try {
        const source = select.value === "" ? null : BigInt(select.value);
        if (source !== null && !localSetup.inventory.some(row => row.source === source)) throw new Error("Choose an acquired source.");
        localRoster.assign(player, source);
        const touch = localRoster.players.findIndex(id => localRoster.selected(id) === 2n);
        if (touch >= 0) ui["local-page"].value = String(Math.floor(touch / 4));
        ui["local-status"].textContent = "Selections retained for this acquired source inventory.";
      } catch (error) {
        select.value = localRoster.selected(player)?.toString() ?? "";
        ui["local-status"].textContent = String(error.message).slice(0, 4096);
      }
    });
    label.append(select);
    fields.append(label);
    return select;
  });
  ui["local-sources"].replaceChildren(fields);
  const previous = ui["local-page"].value;
  ui["local-page"].replaceChildren();
  for (let page = 0; page < Math.ceil(localRoster.players.length / 4); page++) {
    ui["local-page"].append(new Option(`Players ${localRoster.players.slice(page * 4, page * 4 + 4).join(", ")}`, String(page)));
  }
  ui["local-page"].value = Number(previous) < Math.ceil(localRoster.players.length / 4) ? previous || "0" : "0";
}

function releaseLocalSources(reason = "Discover sources again before local play.", cancelDiscovery = true) {
  if (cancelDiscovery) localDiscovery = null;
  const setup = localSetup;
  if (!setup) { controls(); return localCleanup ?? Promise.resolve(); }
  localSetup = null;
  setup.phase = "closing";
  localRoster.clearSources();
  showLocalRoster();
  let gamepadError = null;
  try { setup.gamepadOwner?.close(); gamepadError = setup.gamepadOwner?.cleanupFailure; }
  catch (error) { gamepadError = error; }
  let closed;
  try { closed = setup.hidOwner?.close() ?? Promise.resolve(); }
  catch (error) { closed = Promise.reject(error); }
  const cleanup = Promise.resolve(closed).then(() => { if (gamepadError) throw gamepadError; }).catch(error => {
    hidOwnershipFailed = true;
    fatal(new Error(`Input cleanup failed: ${String(error.message).slice(0, 4096)} Reload the page.`));
  }).finally(() => {
    if (localCleanup === cleanup) localCleanup = null;
    if (setup.owner === owner && !hidOwnershipFailed) ui["local-status"].textContent = reason;
    controls();
  });
  localCleanup = cleanup;
  controls();
  return cleanup;
}

async function discoverLocalSources() {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed || localCleanup || localDiscovery
    || localRoster.players.length === 1) return;
  const operation = { owner, players: localRoster.players, hid: ui["hid-input"].checked,
    hidProfile: selectedHidProfile, gamepadProfile: selectedGamepadProfile, touch: ui["touch-input"].checked };
  localDiscovery = operation;
  controls();
  await releaseLocalSources(undefined, false);
  if (localDiscovery !== operation || operation.owner !== owner || !initialized || activePlay || hidOwnershipFailed || localCleanup || document.hidden
    || operation.players.join(",") !== localRoster.players.join(",") || operation.hid !== ui["hid-input"].checked
    || operation.hidProfile !== selectedHidProfile || operation.gamepadProfile !== selectedGamepadProfile || operation.touch !== ui["touch-input"].checked) {
    if (localDiscovery === operation) localDiscovery = null;
    controls();
    return;
  }
  const setup = { owner, phase: "preparing", sequence: 0n, nextSource: 3n, gamepadOwner: null, gamepadDevices: [],
    gamepadSources: null, gamepadProfileFile: selectedGamepadProfile, hidOwner: null, hidDevices: null, hidSources: null,
    hidProfileFile: ui["hid-input"].checked ? selectedHidProfile : null, touchInput: ui["touch-input"].checked === true, inventory: [] };
  localSetup = setup;
  controls();
  ui["local-status"].textContent = "Acquiring local input sources…";
  try {
    if (ui["hid-input"].checked && (!hidCapable() || setup.hidProfileFile === null)) throw new Error("Select an HID profile and authorize its devices before discovery.");
    if (setup.gamepadProfileFile !== null && typeof navigator.getGamepads !== "function") throw new Error("Gamepad acquisition is unavailable.");
    if (setup.touchInput && typeof window.PointerEvent !== "function") throw new Error("Touch input is unavailable.");
    if (typeof navigator.getGamepads === "function") {
      setup.gamepadOwner = createSessionGamepads(setup);
      setup.gamepadOwner.poll();
      if (!inputOwnerCurrent(setup)) return;
    }
    setup.gamepadDevices = Object.freeze(setup.gamepadDevices);
    if (setup.hidProfileFile !== null) {
      setup.hidOwner = createSessionHid(setup);
      const devices = await setup.hidOwner.connectAuthorized();
      if (!inputOwnerCurrent(setup)) return;
      setup.hidDevices = snapshotHidDevices(devices.map(({ source, device }) => ({ source, vendorId: device.vendorId, productId: device.productId })));
    }
    setup.inventory = Object.freeze([
      Object.freeze({ source: 1n, label: "Keyboard · source 1" }),
      ...(setup.touchInput ? [Object.freeze({ source: 2n, label: "Touch surface · source 2" })] : []),
      ...(setup.hidDevices ?? []).map(device => Object.freeze({ source: device.source, label: `HID ${device.vendorId}:${device.productId} · source ${device.source}` })),
      ...setup.gamepadDevices.map(device => Object.freeze({ source: device.source, label: `Gamepad ${device.id.slice(0, 128)} · source ${device.source}` })),
    ]);
    setup.phase = "ready";
    localDiscovery = null;
    showLocalRoster();
    ui["local-status"].textContent = `${setup.inventory.length} acquired source(s). Choose a distinct source for each player. Device descriptions are labels, not identities.`;
    controls();
  } catch (error) {
    if (localSetup === setup) await releaseLocalSources(`Source discovery failed: ${String(error.message).slice(0, 4096)}`);
  }
}

function createBindingFields() {
  const rows = document.createDocumentFragment();
  const fields = KEY_BINDINGS.map(([lane, code]) => {
    const label = document.createElement("label");
    label.textContent = `Lane ${lane.toString(16).toUpperCase()}`;
    const field = document.createElement("select");
    field.id = `binding-${lane.toString(16)}`;
    field.disabled = true;
    field.append(new Option("Unbound", ""));
    for (const [choice] of KEY_CHOICES) field.append(new Option(choice, choice));
    field.value = code;
    label.append(field);
    rows.append(label);
    return [lane, field];
  });
  ui.bindings.append(rows);
  return fields;
}

function status(text, error = false) {
  ui.status.textContent = text;
  ui.status.dataset.error = String(error);
}

function hidCapable() {
  const hid = globalThis.navigator?.hid;
  return !!hid && ["getDevices", "requestDevice", "addEventListener", "removeEventListener"].every(name => typeof hid[name] === "function");
}

function cancelHidPermission() {
  const operation = hidPermission;
  if (!operation) return;
  operation.cancelled = true;
  // The authorization task below joins this same close, including late opens.
  operation.input?.close().catch(() => {});
}

async function authorizeHid() {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed || localSetup || localDiscovery || localCleanup) return;
  const operation = { owner, input: null, cancelled: false, failure: null };
  hidPermission = operation;
  controls();
  let count = 0;
  let failure = null;
  try {
    if (!hidCapable()) throw new Error("WebHID is unavailable in this browser.");
    operation.input = new HidInputOwner({ hid: navigator.hid, nextSequence: () => 0n,
      onReport: () => {}, onDisconnect: () => {}, onError: error => { operation.failure ??= error; } });
    // Native permission must be requested within this click's user gesture.
    const selected = operation.input.requestDevices([]);
    count = (await selected).length;
  } catch (error) { failure = error; }
  finally {
    try { await operation.input?.close(); }
    catch (error) {
      hidOwnershipFailed = true;
      fatal(new Error(`HID cleanup failed: ${String(error.message).slice(0, 4096)}`));
    }
    if (hidPermission === operation) {
      hidPermission = null;
      controls();
      if (operation.owner === owner && !hidOwnershipFailed) {
        const error = failure ?? operation.failure;
        ui["hid-status"].textContent = operation.cancelled ? "HID authorization stopped."
          : error ? `HID authorization failed: ${String(error.message).slice(0, 4096)}`
            : `Browser permission ready for ${count} interface(s). Live play opens matching authorized devices automatically.`;
      }
    }
  }
}

function nextInputSequence(session) {
  if (!inputOwnerCurrent(session)) throw new Error("Input owner is no longer active.");
  const sequence = session.sequence + 1n;
  if (sequence > 18446744073709551615n) throw new Error("Input acquisition sequence exhausted.");
  session.sequence = sequence;
  return sequence;
}

function nextInputSource(session) {
  if (!inputOwnerCurrent(session)) throw new Error("Input owner is no longer active.");
  const source = session.nextSource;
  if (source > 18446744073709551615n) throw new Error("Input source identity exhausted.");
  session.nextSource++;
  return source;
}

function createSessionGamepads(session) {
  return new GamepadInputOwner({ navigator, eventTarget: window,
    nextSource: () => nextInputSource(session), nextSequence: () => nextInputSequence(session),
    onSample: event => {
      if (!inputOwnerCurrent(session)) return;
      if (session.phase === "preparing") {
        if (session.gamepadDevices.length >= 16) throw new Error("Gamepad descriptor capacity exceeded.");
        session.gamepadDevices.push(Object.freeze({ source: event.source, index: event.index, id: event.id,
          mapping: event.mapping, buttons: event.buttons.length, axes: event.axes.length }));
        return;
      }
      if (session.phase !== "playing" || !session.gamepadSources?.has(event.source)) return;
      if (session.events.length >= 1024) throw new Error("Pending input capacity exceeded.");
      // Old unchanged Gamepad timestamps are legitimate; Worker determines
      // whether this sample changes any admitted controls before chronology.
      session.events.push(event);
      session.completionReady = false;
    },
    onDisconnect: event => {
      if (!inputOwnerCurrent(session)) return;
      if (localSetup === session) { void releaseLocalSources("A discovered Gamepad disconnected. Discover sources again."); return; }
      if (session.localSources && !session.localSources.has(event.source)) return;
      const participates = session.gamepadSources === null
        ? session.gamepadDevices?.some(device => device.source === event.source
          && (session.gamepadProfileFile !== null || (device.mapping === "standard" && device.buttons >= 9)))
        : session.gamepadSources.has(event.source);
      if (participates) {
        void stopPlay("Playback stopped after a Gamepad disconnected.", true);
      }
    },
    onError: error => {
      if (localSetup === session && inputOwnerCurrent(session)) {
        if (error.cleanupError) {
          hidOwnershipFailed = true;
          fatal(new Error(`Gamepad input cleanup failed: ${String(error.cleanupError.message).slice(0, 4096)} Reload the page.`));
          return;
        }
        void releaseLocalSources(`Gamepad discovery failed: ${String(error.message).slice(0, 4096)}`);
        return;
      }
      if (activePlay === session && session.owner === owner && session.phase !== "closing") {
        if (error.cleanupError) hidOwnershipFailed = true;
        void stopPlay(`Gamepad input failed: ${String(error.message).slice(0, 4096)}`
          + (error.cleanupError ? " Input cleanup failed. Reload the page before playing again." : ""), true);
      }
    },
  });
}

function createSessionHid(session) {
  return new HidInputOwner({ hid: navigator.hid,
    nextSource: () => nextInputSource(session),
    nextSequence: () => {
      if (activePlay !== session || session.owner !== owner || session.phase !== "playing") return session.sequence;
      return nextInputSequence(session);
    },
    onReport: event => {
      if (activePlay !== session || session.owner !== owner || session.phase !== "playing"
        || !session.hidSources?.has(event.source)) return;
      if (session.events.length >= 1024) throw new Error("Pending input capacity exceeded.");
      if (event.hostNs < session.lastHost) throw new Error("HID input arrived behind the accepted gameplay watermark.");
      session.events.push(event);
      session.completionReady = false;
      pumpInput(session);
    },
    onDisconnect: event => {
      if (!inputOwnerCurrent(session)) return;
      if (localSetup === session) { void releaseLocalSources("A discovered HID interface disconnected. Discover sources again."); return; }
      if (session.localSources && !session.localSources.has(event.source)) return;
      if (session.hidSources === null || session.hidSources.has(event.source)) {
        void stopPlay("Playback stopped after an HID interface disconnected.", true);
      }
    },
    onError: error => {
      if (localSetup === session && inputOwnerCurrent(session)) {
        void releaseLocalSources(`HID discovery failed: ${String(error.message).slice(0, 4096)}`);
        return;
      }
      if (activePlay === session && session.owner === owner && session.phase !== "closing") {
        void stopPlay(`HID input failed: ${String(error.message).slice(0, 4096)}`, true);
      }
    },
  });
}

function controls() {
  const playing = activePlay !== null;
  const busy = recordsOperation !== null || hidPermission !== null || hidOwnershipFailed || localCleanup !== null || localDiscovery !== null;
  ui.folder.disabled = !initialized || preparing || playing || busy || !("webkitdirectory" in ui.folder);
  ui.files.disabled = !initialized || preparing || playing || busy;
  for (const field of [ui.chart, ui.rate, ui.seed, ui.prepare]) field.disabled = !initialized || !libraryId || importing || preparing || playing || busy;
  ui.position.disabled = ui.seek.disabled = !initialized || !hasPreview || importing || preparing || playing || busy;
  ui.play.disabled = !initialized || !hasPreview || importing || preparing || playing || busy || !audioModule;
  ui.stop.disabled = !playing || activePlay.phase === "closing";
  ui.record.disabled = ui.play.disabled;
  ui.multiplayer.disabled = ui.play.disabled;
  ui["multiplayer-url"].disabled = ui["multiplayer-role"].disabled = ui.play.disabled || !ui.multiplayer.checked;
  ui.export.disabled = playing || busy || lastReplay === null;
  ui["replay-file"].disabled = !initialized || importing || preparing || playing || busy;
  ui["replay-play"].disabled = ui.play.disabled || selectedReplay === null;
  const recordsDisabled = !initialized || importing || preparing || playing || busy;
  const inputLocked = recordsDisabled || localSetup !== null;
  ui["local-count"].disabled = recordsDisabled;
  ui["local-discover"].disabled = recordsDisabled || localRoster.players.length === 1;
  ui["local-release"].disabled = playing || localSetup === null;
  for (const field of localFields) field.disabled = recordsDisabled || localSetup?.phase !== "ready";
  ui["local-page"].disabled = activePlay ? activePlay.phase !== "playing" || !activePlay.localPlan
    || activePlay.localSources.has(2n) || activePlay.pageChanging || activePlay.rpc !== null
    : recordsDisabled || localRoster.players.length === 1;
  ui["captured-replay"].disabled = playing || busy || capturedReplays.length === 0;
  ui["bindings-reset"].disabled = recordsDisabled;
  ui["touch-input"].disabled = inputLocked;
  for (const id of ["hid-input", "hid-authorize", "hid-profile"]) ui[id].disabled = inputLocked || !hidCapable();
  ui["gamepad-profile"].disabled = inputLocked || typeof globalThis.navigator?.getGamepads !== "function";
  ui["gamepad-profile-clear"].disabled = inputLocked || selectedGamepadProfile === null;
  for (const [, field] of bindingFields) field.disabled = recordsDisabled;
  for (const field of [ui["judge-early"], ui["judge-late"], ui["judge-offset"], ui["live-start"], ui["live-end"]]) field.disabled = recordsDisabled;
  ui["output-latency"].disabled = ui["output-rate"].disabled = recordsDisabled;
  ui["output-latency-ms"].disabled = recordsDisabled || ui["output-latency"].value !== "custom";
  for (const id of ["audio-queue", "audio-voices", "audio-pending", "audio-frames", "audio-commands"]) ui[id].disabled = recordsDisabled;
  ui.records.disabled = ui["records-refresh"].disabled = recordsDisabled;
  ui["records-save"].disabled = recordsDisabled || lastReplay === null;
  ui["records-use"].disabled = ui["records-delete"].disabled = recordsDisabled || !ui.records.value;
  ui["opponents-kind"].disabled = ui["opponents-label"].disabled = recordsDisabled;
  ui["opponents-add"].disabled = recordsDisabled || !selectedReplay || opponents.size >= 8;
  ui["records-opponent"].disabled = recordsDisabled || !ui.records.value || opponents.size >= 8;
  ui["opponents-clear"].disabled = recordsDisabled || opponents.size === 0;
  for (const button of opponentButtons) button.disabled = recordsDisabled;
}
function stop() {
  revokeReplayURL();
  closeRecords();
  cancelHidPermission();
  void releaseLocalSources("Local sources released with the page.");
  if (activePlay?.phase !== "closing") void stopPlay("Playback stopped with the page.");
  ++owner;
  worker?.terminate();
  if (activePlay) releasePlayWorker(activePlay);
  worker = null;
  observer?.disconnect();
  observer = null;
  density?.removeEventListener("change", densityChanged);
  density = null;
  selectedReplay = selectedReplayKey = null;
  selectedHidProfile = null;
  selectedGamepadProfile = null;
  importedReplayKeys = new WeakMap();
  importedReplayId = 0;
  opponents.clear();
  showOpponentSelection();
  clearOpponentResults("No saved opponents selected.");
  initialized = false;
  controls();
}
function fatal(error) {
  stop();
  canvas.hidden = true;
  status(`${String(error?.message ?? error).slice(0, 4096)} Reload this page to initialize a new preview.`, true);
}

function resize() {
  if (!worker) return;
  const box = ui.viewport.getBoundingClientRect();
  cssExtent = [box.width, box.height];
  const dpr = window.devicePixelRatio || 1;
  // Do not assign canvas backing dimensions here; the renderer validates first.
  const dimensions = [box.width, box.height].map(value => Math.round(value * dpr));
  if (dimensions.some(value => !Number.isSafeInteger(value) || value < 0 || value > 0xffffffff)) return fatal(new Error("Canvas dimensions are outside the supported range."));
  worker.postMessage({ kind: "resize", width: dimensions[0], height: dimensions[1] });
}
function densityChanged() {
  density?.removeEventListener("change", densityChanged);
  density = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
  density.addEventListener("change", densityChanged);
  resize();
}

function prepare() {
  if (!worker || !libraryId || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  try {
    const rate = Number(ui.rate.value);
    const seed = ui.seed.value;
    if (!Number.isInteger(rate) || rate < 1 || rate > 0xffffffff) throw new Error("Sample rate must be a positive 32-bit integer.");
    if (!/^\d{1,20}$/.test(seed) || BigInt(seed) > 0xffffffffffffffffn) throw new Error("Chart seed must fit an unsigned 64-bit integer.");
    if (!ui.chart.value) throw new Error("Select a chart first.");
    selectId = ++serial;
    preparing = true;
    canvas.hidden = true;
    controls();
    status("Preparing chart, sounds and backgrounds…");
    worker.postMessage({ kind: "select", id: selectId, libraryId, path: ui.chart.value, rate, seed });
  } catch (error) { preparing = false; controls(); status(error.message, true); }
}

function received(data) {
  if (data.kind.startsWith("play-")) { receivePlay(data); return; }
  if (data.kind === "ready") {
    initialized = true;
    controls();
    status("Choose a song folder, or select a chart and its resources together.");
    void loadAudio(owner);
  } else if (data.kind === "fatal") fatal(new Error(data.message));
  else if (data.kind === "import-progress" && data.id === importId) status(`Reading files: ${data.read} / ${data.total}`);
  else if (data.kind === "catalog" && data.id === importId) {
    worker.postMessage({ kind: "accept-library", id: data.id });
    importing = false;
    libraryId = data.id;
    ui.chart.replaceChildren();
    const options = document.createDocumentFragment();
    for (const path of data.charts) {
      const option = document.createElement("option");
      option.value = path;
      option.textContent = path;
      options.append(option);
    }
    ui.chart.append(options);
    controls();
    status(`${data.charts.length} chart(s) loaded. Choose a sample rate and prepare a chart.`);
  } else if (data.kind === "import-error" && data.id === importId) {
    importing = false;
    controls();
    status(`${data.message} Previous library and preview are retained.`, true);
  } else if (data.kind === "selected" && data.id === selectId && data.libraryId === libraryId) {
    preparing = false;
    hasPreview = true;
    selectedId = data.id;
    seekId = 0;
    ui.title.textContent = data.title || data.path;
    ui.details.textContent = `${data.artist || "Unknown artist"} · ${data.notes} notes · ${data.samples} sounds · ${data.images} images · last note: ${seconds(data.duration)} s`;
    ui.position.value = "0";
    controls();
    status("Chart prepared at 0 seconds. This preview does not play audio or judge input.");
  } else if (data.kind === "drawn" && data.selectedId === selectedId && !preparing) canvas.hidden = false;
  else if (data.kind === "selection-error" && data.id === selectId) {
    preparing = false;
    canvas.hidden = !hasPreview;
    controls();
    status(`${data.message} ${hasPreview ? "The previous preview is retained." : "No chart has been prepared."}`, true);
  } else if (data.kind === "position" && data.id === seekId && data.selectedId === selectedId) {
    ui.position.value = seconds(data.ns);
    status(`Preview position: ${seconds(data.ns)} seconds.`);
  } else if (data.kind === "seek-error" && data.id === seekId && data.selectedId === selectedId) status(data.message, true);
  else if (data.kind === "render-wait" && data.selectedId === selectedId) status("The graphics surface is not ready. Resize the view or choose Show position to retry.", true);
}

function start() {
  stop();
  if (hidOwnershipFailed) { status("Input cleanup failed. Reload the page before playing again.", true); return; }
  libraryId = importId = selectId = selectedId = seekId = 0;
  importing = preparing = hasPreview = false;
  audioModule = null;
  selectedReplay = null;
  ui["replay-file"].value = "";
  ui["replay-name"].textContent = "Choose a recording and prepare its matching chart. Replay uses the recorded seed and section.";
  ui["hid-input"].checked = false;
  ui["hid-profile"].value = "";
  ui["hid-profile-name"].textContent = "Choose a version 1 HID profile for live play.";
  ui["gamepad-profile"].value = "";
  ui["gamepad-profile-name"].textContent = "Automatic standard Gamepad bindings; choose an optional version 1 profile to customize.";
  ui["hid-status"].textContent = hidCapable() ? "Authorize devices if needed; live play uses matching authorized interfaces automatically." : "WebHID is unavailable in this browser.";
  ui.records.replaceChildren(new Option("Refresh to browse saved records", ""));
  ui.keys.textContent = "";
  ui.folder.value = ui.files.value = "";
  ui.chart.replaceChildren(new Option("Choose files first", ""));
  ui.title.textContent = "No chart prepared";
  ui.details.textContent = "Select a song folder to begin.";
  ui.position.value = "0";
  const fresh = document.createElement("canvas");
  fresh.id = "canvas";
  fresh.width = 960;
  fresh.height = 720;
  fresh.hidden = true;
  fresh.setAttribute("aria-label", "Chart lanes and background at the selected song time");
  canvas.replaceWith(fresh);
  canvas = fresh;
  cssExtent = [0, 0];
  for (const [name, phase] of [["pointerdown", 0], ["pointermove", 1], ["pointerup", 2], ["pointercancel", 3]]) {
    fresh.addEventListener(name, event => touch(event, phase, fresh), { passive: false });
  }
  fresh.addEventListener("lostpointercapture", event => touch(event, 3, fresh, true), { passive: false });
  controls();
  status("Initializing graphics…");
  try {
    if (!window.isSecureContext || !window.Worker || !window.OffscreenCanvas || !canvas.transferControlToOffscreen || !window.ResizeObserver) throw new Error("This preview needs a secure context, Workers and OffscreenCanvas support.");
    const generation = owner;
    worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    worker.addEventListener("message", event => { if (generation === owner) received(event.data); });
    worker.addEventListener("error", event => {
      if (generation !== owner) return;
      event.preventDefault();
      fatal(new Error(event.message || "Could not load the browser Worker and generated WASM package."));
    });
    worker.addEventListener("messageerror", () => { if (generation === owner) fatal(new Error("Could not receive a Worker response.")); });
    const surface = canvas.transferControlToOffscreen();
    worker.postMessage({ kind: "init", canvas: surface }, [surface]);
    observer = new ResizeObserver(resize);
    observer.observe(ui.viewport);
    densityChanged();
  } catch (error) { fatal(error); }
}

function choose(event) {
  if (!initialized || preparing || !worker || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  const files = event.target.files;
  if (!files?.length) return;
  if (files.length > 32768) return status("Select no more than 32,768 files.", true);
  void releaseLocalSources("Song library changed. Discover local sources again.");
  importId = ++serial;
  importing = true;
  controls();
  status("Checking the selected files…");
  try { worker.postMessage({ kind: "import", id: importId, files: snapshotFiles(files) }); }
  catch (error) { importing = false; controls(); status(error.message, true); }
  // A subsequent selection of the same folder still triggers change.
  event.target.value = "";
}
ui.folder.addEventListener("change", choose);
ui.files.addEventListener("change", choose);
byId("prepare-form").addEventListener("submit", event => { event.preventDefault(); prepare(); });
byId("seek-form").addEventListener("submit", event => {
  event.preventDefault();
  if (!worker || !hasPreview || preparing || importing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  try {
    const ns = nanoseconds(ui.position.value);
    seekId = ++serial;
    worker.postMessage({ kind: "seek", id: seekId, selectedId, ns });
  } catch (error) { status(error.message, true); }
});
window.addEventListener("pagehide", stop);
window.addEventListener("pageshow", event => { if (event.persisted) start(); });
window.addEventListener("resize", resize);
ui.play.addEventListener("click", () => { void play("live"); });
ui["replay-play"].addEventListener("click", () => { void play("replay"); });
ui["local-count"].addEventListener("change", () => {
  if (activePlay) return;
  try {
    if (!/^[0-9]{1,2}$/.test(ui["local-count"].value)) throw new Error("Choose one to 64 local players.");
    localRoster.setCount(Number(ui["local-count"].value));
    localRoster.clearSources();
    void releaseLocalSources(localRoster.players.length === 1 ? "One player uses inputs automatically. No source selection is needed." : "Discover sources before assigning local players.");
    showLocalRoster();
    ui["local-status"].textContent = localRoster.players.length === 1 ? "One player uses inputs automatically. No source selection is needed." : "Discover sources before assigning local players.";
    controls();
  } catch (error) { ui["local-count"].value = String(localRoster.players.length); status(error.message, true); }
});
ui["local-discover"].addEventListener("click", () => { void discoverLocalSources(); });
ui["local-release"].addEventListener("click", () => { if (!activePlay) void releaseLocalSources(); });
ui["local-page"].addEventListener("change", () => { void changeLocalPage(); });
ui["captured-replay"].addEventListener("change", () => {
  if (activePlay || recordsOperation) return;
  lastReplay = capturedReplays.find(record => String(record.player ?? "solo") === ui["captured-replay"].value) ?? null;
  ui.export.textContent = lastReplay ? `Download ${lastReplay.player === undefined ? "last replay" : `player ${lastReplay.player} replay`} (${lastReplay.complete ? "complete" : "prefix"})` : "Choose a captured replay";
  controls();
});
ui.multiplayer.addEventListener("change", () => {
  if (activePlay) return;
  controls();
  ui["multiplayer-status"].textContent = ui.multiplayer.checked
    ? "Live Play will wait for the peer's compatible setup and committed start. Replay stays local."
    : "Solo play selected.";
});
ui["replay-file"].addEventListener("change", event => {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  try {
    const files = event.target.files;
    if (!files?.length) return;
    const file = files[0];
    if (files.length !== 1 || !(file instanceof File) || !Number.isSafeInteger(file.size)
      || file.size < 1 || file.size > 64 * 1024 * 1024) throw new Error("Choose one nonempty replay no larger than 64 MiB.");
    if (!importedReplayKeys.has(file)) {
      if (!Number.isSafeInteger(importedReplayId + 1)) throw new Error("Imported replay selection identity exhausted.");
      importedReplayKeys.set(file, `file:${++importedReplayId}`);
    }
    selectedReplay = file;
    selectedReplayKey = importedReplayKeys.get(file);
    ui["replay-name"].textContent = `${file.name.slice(0, 256)} · ${file.size} bytes · uses recorded seed and section`;
    controls();
    status("Replay selected. Prepare its matching chart, then choose Play replay.");
  } catch (error) { status(String(error.message).slice(0, 4096), true); }
  finally { event.target.value = ""; }
});
ui["hid-authorize"].addEventListener("click", () => { void authorizeHid(); });
ui["hid-profile"].addEventListener("change", event => {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  try {
    if (!hidCapable()) throw new Error("WebHID is unavailable in this browser.");
    const files = event.target.files;
    if (!files?.length) return;
    const file = files[0];
    if (files.length !== 1 || !(file instanceof File) || !Number.isSafeInteger(file.size)
      || file.size < 1 || file.size > 1024 * 1024) throw new Error("Choose one nonempty HID profile no larger than 1 MiB.");
    selectedHidProfile = file;
    ui["hid-input"].checked = true;
    ui["hid-profile-name"].textContent = `${file.name.slice(0, 256)} · ${file.size} bytes`;
    ui["hid-status"].textContent = "Profile selected. Live play checks it against authorized interfaces on the Worker.";
    controls();
  } catch (error) { status(String(error.message).slice(0, 4096), true); }
  finally { event.target.value = ""; }
});
ui["gamepad-profile"].addEventListener("change", event => {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  try {
    const file = event.target.files?.[0];
    if (typeof navigator.getGamepads !== "function" || !(file instanceof File) || !Number.isSafeInteger(file.size)
      || file.size < 1 || file.size > 1024 * 1024) throw new Error("Choose one nonempty Gamepad profile no larger than 1 MiB on a browser with Gamepad support.");
    selectedGamepadProfile = file;
    ui["gamepad-profile-name"].textContent = `${file.name.slice(0, 256)} · ${file.size} bytes · checked when starting play`;
    controls();
  } catch (error) { status(String(error.message).slice(0, 4096), true); }
  finally { event.target.value = ""; }
});
ui["gamepad-profile-clear"].addEventListener("click", () => {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  selectedGamepadProfile = null;
  ui["gamepad-profile"].value = "";
  ui["gamepad-profile-name"].textContent = "Automatic standard Gamepad bindings; choose an optional version 1 profile to customize.";
  controls();
});
ui.stop.addEventListener("click", () => { void stopPlay("Playback stopped."); });
ui["bindings-reset"].addEventListener("click", () => {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  for (let index = 0; index < bindingFields.length; index++) bindingFields[index][1].value = KEY_BINDINGS[index][1];
  status("Keyboard bindings reset to defaults.");
});
ui.export.addEventListener("click", downloadReplay);
ui.records.addEventListener("change", controls);
ui["output-latency"].addEventListener("change", controls);
for (const [id, action] of [["records-refresh", "refresh"], ["records-save", "save"], ["records-use", "use"], ["records-delete", "delete"], ["records-opponent", "opponent"]]) {
  ui[id].addEventListener("click", () => { void recordAction(action); });
}
ui["opponents-add"].addEventListener("click", () => {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed || !selectedReplay) return;
  try { addOpponent(selectedReplay, selectedReplayKey, opponentChoice()); }
  catch (error) { opponentStatus(String(error.message).slice(0, 4096), true); }
});
ui["opponents-clear"].addEventListener("click", () => {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  opponents.clear();
  showOpponentSelection();
  clearOpponentResults("No saved opponents selected.");
  controls();
});
window.addEventListener("blur", () => { cancelHidPermission(); void releaseLocalSources("Local sources released after losing focus."); void stopPlay("Playback stopped after losing focus."); });
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    void releaseLocalSources("Local sources released while the page is hidden.");
    void stopPlay("Playback stopped while the page is hidden.");
    cancelHidPermission();
    const pending = recordsOperation !== null;
    closeRecords();
    controls();
    if (pending) status("Record library operation stopped while the page is hidden.");
  }
});
window.addEventListener("keydown", event => key(event, true));
window.addEventListener("keyup", event => key(event, false));

async function loadAudio(generation) {
  try {
    const response = await fetch(new URL("./audio-pkg/beatkernel_bms_runtime_bg.wasm", import.meta.url));
    if (!response.ok || !response.body) throw new Error("Build the separate browser-audio package to enable Play.");
    const reader = response.body.getReader();
    const chunks = [];
    let bytes = 0;
    try {
      for (;;) {
        const next = await reader.read();
        if (next.done) break;
        bytes += next.value.byteLength;
        if (bytes > 64 * 1024 * 1024) throw new Error("Audio WASM exceeds the 64 MiB setup limit.");
        chunks.push(next.value);
      }
    } catch (error) { await reader.cancel().catch(() => {}); throw error; }
    finally { reader.releaseLock(); }
    const binary = new Uint8Array(bytes);
    let offset = 0;
    for (const chunk of chunks) { binary.set(chunk, offset); offset += chunk.byteLength; }
    const compiled = await WebAssembly.compile(binary);
    if (generation !== owner) return;
    audioModule = compiled;
    controls();
  } catch (error) {
    if (generation === owner) status(`Chart preview remains available. ${String(error.message).slice(0, 4096)}`, true);
  }
}

function multiplayerConfiguration() {
  const raw = ui["multiplayer-url"].value;
  const role = ui["multiplayer-role"].value;
  if (typeof raw !== "string" || raw.length === 0 || raw.length > 4096
    || !["host", "join"].includes(role)) throw new Error("Choose a multiplayer HTTPS server and start role.");
  const url = new URL(raw);
  if (url.protocol !== "https:" || url.username || url.password || url.hash || url.href.length > 4096) {
    throw new Error("Multiplayer requires an HTTPS URL without credentials or a fragment.");
  }
  return { url: url.href, host: role === "host", windowOriginNs: millisecondsToNanos(performance.timeOrigin) };
}

function playRpc(session, kind, fields = {}, transfer = []) {
  if (activePlay !== session || session.phase === "closing" || !worker) return Promise.reject(new Error("Playback owner is closed."));
  if (session.rpc) return Promise.reject(new Error("A playback setup operation is already pending."));
  const rpcId = ++serial;
  if (!Number.isSafeInteger(rpcId)) return Promise.reject(new Error("Playback request identity exhausted."));
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      if (session.rpc?.rpcId !== rpcId) return;
      session.rpc = null;
      reject(new Error("Playback Worker operation timed out."));
    }, 10000);
    session.rpc = { rpcId, timer, resolve, reject };
    try { worker.postMessage({ kind, playId: session.id, rpcId, ...fields }, transfer); }
    catch (error) { clearTimeout(timer); session.rpc = null; reject(error); }
  });
}

async function changeLocalPage() {
  const session = activePlay;
  if (!session) return;
  if (!session.localPlan || session.phase !== "playing" || session.pageChanging || session.rpc || session.localSources.has(2n)) {
    ui["local-page"].value = String(session.localPage);
    return;
  }
  const page = Number(ui["local-page"].value);
  if (!Number.isInteger(page) || page < 0 || page >= Math.ceil(session.localPlan.players.length / 4)) {
    ui["local-page"].value = String(session.localPage);
    return;
  }
  session.pageChanging = true;
  controls();
  try {
    const result = await playRpc(session, "play-page", { page });
    if (activePlay !== session || session.owner !== owner || session.phase !== "playing") return;
    if (result?.kind !== "local-page" || result.page !== page) {
      void stopPlay("Local page response changed its requested identity.", true);
      return;
    }
    session.localPage = page;
    ui["local-status"].textContent = `Showing players ${session.localPlan.players.slice(page * 4, page * 4 + 4).join(", ")}.`;
  } catch (error) {
    if (activePlay === session && session.owner === owner && session.phase === "playing") {
      ui["local-status"].textContent = `Page unchanged: ${String(error.message).slice(0, 4096)}`;
    }
  } finally {
    session.pageChanging = false;
    if (activePlay === session && session.owner === owner) {
      ui["local-page"].value = String(session.localPage);
      controls();
    }
  }
}

async function play(mode = "live") {
  if (!initialized || !hasPreview || !audioModule || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed || localDiscovery || localCleanup) return;
  if (mode === "replay" && selectedReplay === null) return;
  let retained = null;
  let localPlan = null;
  let releasedSources = null;
  if (mode === "live" && localRoster.players.length > 1) {
    try {
      if (ui.multiplayer.checked || opponents.size) throw new Error("Local players currently cannot combine network or saved opponents.");
      retained = localSetup;
      if (!retained || retained.phase !== "ready" || retained.owner !== owner) throw new Error("Discover and assign local input sources before playing.");
      if (retained.touchInput !== (ui["touch-input"].checked === true)
        || retained.hidProfileFile !== (ui["hid-input"].checked ? selectedHidProfile : null)
        || retained.gamepadProfileFile !== selectedGamepadProfile) throw new Error("Input configuration changed. Discover local sources again.");
      // Poll this exact owner before freezing the plan. A changed native
      // connection retires its source and invalidates this discovered roster.
      retained.gamepadOwner?.poll();
      if (localSetup !== retained || retained.phase !== "ready") throw new Error("Local source ownership changed. Discover sources again.");
      localPlan = localRoster.snapshot(retained.inventory.map(row => row.source), Number(ui["local-page"].value));
      const touchIndex = localPlan.sources.indexOf(2n);
      if (touchIndex >= 0 && Math.floor(touchIndex / 4) !== localPlan.page) throw new Error("The touch player must be on the visible starting page.");
      localSetup = null;
    } catch (error) { status(String(error.message).slice(0, 4096), true); return; }
  } else if (localSetup) releasedSources = releaseLocalSources();
  const acquired = retained ? { sequence: retained.sequence, nextSource: retained.nextSource,
    gamepadOwner: retained.gamepadOwner, gamepadDevices: retained.gamepadDevices,
    hidOwner: retained.hidOwner, hidDevices: retained.hidDevices } : {};
  const session = Object.assign(retained ?? {}, { id: ++serial, owner, mode, phase: "preparing", controller: new AbortController(), audio: null, opening: null,
    rpc: null, timer: null, events: [], pressed: new Set(), bindings: [], sequence: 0n, nextSource: 3n, inputPumping: false,
    canvas, touchInput: false, contacts: new Map(), nextContact: 0n,
    hidOwner: null, hidConnecting: null, hidDevices: null, hidSources: null, hidProfileFile: null,
    gamepadOwner: null, gamepadDevices: null, gamepadSources: null, gamepadProfileFile: null,
    tickId: 0, tickPending: null, commandsPending: true, startFrame: null,
    origin: null, lastHost: 0n, stopping: null, renderId: 0, renderPending: null,
    workerStarted: false, workerReleased: false, workerStop: null, finalScore: null,
    completionReady: false, completionTick: null, cleanupError: null, peerDisplayFailed: false,
    recordReplay: mode === "live" && ui.record.checked === true, replay: null, replayError: null, naturalFinishRequested: false,
    replayFile: mode === "replay" ? selectedReplay : null,
    opponentSelection: mode === "live" && opponents.size ? opponents.snapshot() : null,
    opponentCount: mode === "live" ? opponents.size : 0, opponentsFailed: false, opponentError: null,
    chartPath: ui.chart.value,
    preview: { title: ui.title.textContent, details: ui.details.textContent, position: ui.position.value } }, acquired,
    { localPlan, localSources: localPlan ? new Set(localPlan.sources) : null, localReplays: null, localScores: null, recordLimits: null,
      localPage: localPlan?.page ?? 0, pageChanging: false });
  activePlay = session;
  controls();
  status(mode === "replay" ? "Preparing recorded replay and audio…" : "Preparing playable chart and audio…");
  try {
    session.touchInput = mode === "live" && ui["touch-input"].checked === true;
    if (session.touchInput && (typeof window.PointerEvent !== "function"
      || typeof session.canvas.setPointerCapture !== "function" || typeof session.canvas.releasePointerCapture !== "function")) {
      throw new Error("Touch play requires Pointer Events and canvas pointer capture support.");
    }
    session.inputMode = session.touchInput ? "physical-contact" : "physical";
    if (session.touchInput && (!session.localSources || session.localSources.has(2n))) session.canvas.dataset.touchInput = "true";
    if (mode === "live" && ui["hid-input"].checked === true) {
      if (!hidCapable() || !(selectedHidProfile instanceof File) || !Number.isSafeInteger(selectedHidProfile.size)
        || selectedHidProfile.size < 1 || selectedHidProfile.size > 1024 * 1024) {
        throw new Error("HID play requires WebHID and a selected nonempty profile no larger than 1 MiB.");
      }
      session.hidProfileFile = selectedHidProfile;
    }
    if (mode === "live" && selectedGamepadProfile !== null) {
      if (typeof navigator.getGamepads !== "function" || !(selectedGamepadProfile instanceof File)
        || !Number.isSafeInteger(selectedGamepadProfile.size) || selectedGamepadProfile.size < 1 || selectedGamepadProfile.size > 1024 * 1024) {
        throw new Error("Live Gamepad profiles require Gamepad support and a nonempty file no larger than 1 MiB.");
      }
      session.gamepadProfileFile = selectedGamepadProfile;
    }
    session.contextOptions = audioOutputFromFields(ui["output-latency"].value, ui["output-latency-ms"].value, ui["output-rate"].value);
    session.audioLimits = audioLimitsFromFields({ queueCapacity: ui["audio-queue"].value, maxVoices: ui["audio-voices"].value,
      pendingCapacity: ui["audio-pending"].value, maxFrames: ui["audio-frames"].value, maxCommandsPerRender: ui["audio-commands"].value });
    session.commandBatchLimit = Math.min(256, session.audioLimits.queueCapacity);
    session.timing = mode === "live" ? timingFromMilliseconds(ui["judge-early"].value, ui["judge-late"].value, ui["judge-offset"].value) : null;
    const section = mode === "live" ? sectionFromSeconds(ui["live-start"].value, ui["live-end"].value) : null;
    session.startNs = section?.startNs ?? null;
    session.requestedEndNs = section?.endNs;
    session.bindingSelection = mode === "live" ? snapshotBindings(bindingFields.map(([lane, field]) => [lane, field.value])) : null;
    session.multiplayer = mode === "live" && ui.multiplayer.checked === true ? multiplayerConfiguration() : null;
    ui["multiplayer-status"].textContent = session.multiplayer ? "Preparing local audio before connecting…"
      : mode === "replay" ? "Local replay · no multiplayer connection."
        : session.localPlan ? "Local players · no network connection." : "Solo play selected.";
    clearOpponentResults(session.opponentSelection ? "Preparing selected saved opponents…"
      : mode === "replay" ? "Saved comparisons are inactive during replay playback." : "No saved opponents selected.");
    if (!retained && session.hidProfileFile !== null) session.hidOwner = createSessionHid(session);
    if (!retained && mode === "live" && typeof navigator.getGamepads === "function") {
      session.gamepadDevices = [];
      session.gamepadOwner = createSessionGamepads(session);
      session.gamepadOwner.poll();
      if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
      session.gamepadDevices = Object.freeze(session.gamepadDevices);
    }
    // open invokes resume synchronously here, inside the button's user gesture.
    const opening = AudioHost.open({ module: audioModule, generation: session.id, channels: 2,
      contextOptions: session.contextOptions,
      pcmLimits: { maxAssetBytes: 64 * 1024 * 1024, maxTotalBytes: 256 * 1024 * 1024, maxSamples: PLAY_PCM_SAMPLES },
      audioLimits: session.audioLimits,
      timeoutMs: 10000, signal: session.controller.signal });
    session.opening = opening;
    if (!retained && session.hidOwner !== null) {
      session.hidConnecting = session.hidOwner.connectAuthorized();
      session.hidConnecting.catch(() => {});
    }
    session.audio = await opening;
    if (activePlay !== session || session.phase === "closing") { await session.audio.stop(); return; }
    if (releasedSources) await releasedSources;
    if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
    if (hidOwnershipFailed) throw new Error("Input ownership cleanup failed. Reload the page.");
    if (!retained && session.hidOwner !== null) {
      const devices = await session.hidConnecting;
      if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
      session.hidDevices = snapshotHidDevices(devices.map(({ source, device }) => ({ source, vendorId: device.vendorId, productId: device.productId })));
    }
    if (session.localPlan) {
      session.hidDevices = session.hidDevices?.filter(device => session.localSources.has(device.source)) ?? null;
      session.gamepadDevices = session.gamepadDevices?.filter(device => session.localSources.has(device.source)) ?? null;
    }
    session.requestHid = session.hidOwner !== null && (!session.localPlan || session.hidDevices?.length > 0);
    session.requestGamepad = session.gamepadOwner !== null && (!session.localPlan || session.gamepadDevices?.length > 0);
    if (session.localPlan && !session.requestHid) session.hidSources = new Set();
    if (session.localPlan && !session.requestGamepad) session.gamepadSources = new Set();
    session.workerStarted = true;
    const source = mode === "replay" ? { mode, replayFile: session.replayFile }
      : { mode, inputMode: session.inputMode, seed: ui.seed.value, recordReplay: session.recordReplay, timing: session.timing, startNs: session.startNs,
        ...(session.requestedEndNs === undefined ? {} : { endNs: session.requestedEndNs }),
        ...(session.multiplayer ? { multiplayer: session.multiplayer } : {}),
        ...(session.opponentSelection ? { opponents: session.opponentSelection } : {}),
        ...(session.localPlan ? { localPlanWords: session.localPlan.words, localPage: session.localPlan.page } : {}),
        ...(session.requestHid ? { hidProfileFile: session.hidProfileFile, hidDevices: session.hidDevices } : {}),
        ...(session.requestGamepad ? { gamepadDevices: session.gamepadDevices } : {}),
        ...(session.requestGamepad && session.gamepadProfileFile ? { gamepadProfileFile: session.gamepadProfileFile } : {}),
        keyPairs: Uint32Array.from(session.bindingSelection.flatMap(row => [row[0], row[2]])) };
    const prepared = await playRpc(session, "play-start", { libraryId, path: ui.chart.value,
      rate: session.audio.sampleRate, commandBatchLimit: session.commandBatchLimit, ...source });
    if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
    if (mode === "replay" ? prepared.mode !== "replay" : prepared.mode !== undefined && prepared.mode !== "live") throw new Error("Playback preparation mode changed.");
    if (mode === "live" && prepared.inputMode !== session.inputMode) throw new Error("Preparation did not admit the requested physical input route.");
    if (session.localPlan) {
      const local = validateLocalPrepared(session.localPlan, prepared, session.recordReplay);
      session.localPage = local.page;
      session.recordLimits = local.recordLimits;
    } else if (prepared.localPlayers !== undefined || prepared.localPage !== undefined) throw new Error("Preparation unexpectedly created local players.");
    if (session.requestHid) {
      const count = prepared.hidSourceCount;
      const sources = prepared.hidSources;
      if (!Number.isInteger(count) || count < 1 || count > 16 || !Array.isArray(sources) || sources.length !== count
        || (session.localPlan && count !== session.hidDevices.length)) {
        throw new Error("Preparation omitted the exact admitted HID sources.");
      }
      const admitted = new Set();
      for (const source of sources) {
        if (typeof source !== "bigint" || source < 3n || source > 18446744073709551615n || admitted.has(source)
          || !session.hidDevices.some(device => device.source === source)) throw new Error("Preparation changed an owned HID source identity.");
        admitted.add(source);
      }
      session.hidSources = admitted;
      ui["hid-status"].textContent = `${admitted.size} matching HID interface(s) prepared automatically.`;
    } else if (prepared.hidSourceCount !== undefined || prepared.hidSources !== undefined) {
      throw new Error("Preparation admitted HID without an owned device session.");
    }
    if (session.requestGamepad) {
      const sources = prepared.gamepadSources;
      const custom = session.gamepadProfileFile !== null;
      const eligible = custom || session.localPlan ? session.gamepadDevices : session.gamepadDevices.filter(device => device.mapping === "standard" && device.buttons >= 9);
      if (!Array.isArray(sources) || (session.localPlan ? sources.length !== eligible.length
        : custom ? sources.length < 1 || sources.length > eligible.length : sources.length !== eligible.length)) {
        throw new Error("Preparation omitted the admitted Gamepad profile sources.");
      }
      const admitted = new Set();
      const owned = session.gamepadOwner.devices;
      for (const source of sources) {
        if (typeof source !== "bigint" || source < 3n || source > 18446744073709551615n || admitted.has(source)
          || !eligible.some(device => device.source === source) || !owned.some(device => device.source === source)
          || session.hidSources?.has(source)) throw new Error("Preparation changed an owned Gamepad source identity.");
        admitted.add(source);
      }
      session.gamepadSources = admitted;
    } else if (prepared.gamepadSources !== undefined) throw new Error("Preparation admitted Gamepads without an owned input session.");
    const preparedStart = prepared.startNs === undefined && mode === "live" && session.startNs === 0n ? 0n : prepared.startNs;
    if (typeof preparedStart !== "bigint") throw new Error("Preparation omitted its actual song start.");
    validateStart(preparedStart);
    if (mode === "live" && preparedStart !== session.startNs) throw new Error("Prepared live section start changed.");
    if (mode === "replay") session.startNs = preparedStart;
    const output = replayOutputFromMetadata(preparedStart, prepared.endNs, prepared.endFrame, session.audio.sampleRate);
    if (mode === "live" && output.endNs !== session.requestedEndNs) throw new Error("Prepared live section end changed.");
    session.endNs = output.endNs;
    session.endFrame = output.endFrame;
    const opponentCount = prepared.opponentCount === undefined ? 0 : prepared.opponentCount;
    if (!Number.isInteger(opponentCount) || opponentCount !== (session.opponentSelection?.length ?? 0)) throw new Error("Prepared saved opponent count changed.");
    session.opponentCount = opponentCount;
    session.opponentSelection = null;
    ui.title.textContent = prepared.title || ui.chart.value;
    ui.details.textContent = `${prepared.artist || "Unknown artist"} · ${prepared.notes} notes · ${prepared.samples} sounds · ${session.audio.sampleRate} Hz output · start ${seconds(preparedStart.toString())} s`
      + (session.endNs === undefined ? "" : ` · ${mode === "replay" ? "recorded end" : "end"} ${seconds(session.endNs.toString())} s`)
      + (session.localPlan ? ` · ${session.localPlan.players.length} local players` : "");
    if (session.localPlan || session.hidOwner !== null || session.gamepadSources?.size > 0) {
      bindingsFor(prepared.lanes); // Validate actual lane shape; Worker proved combined coverage.
      session.bindings = session.localSources && !session.localSources.has(1n) ? []
        : bindingsFor(prepared.lanes.filter(lane => session.bindingSelection.some(row => row[0] === lane)), session.bindingSelection);
    } else session.bindings = mode === "replay" ? [] : bindingsFor(prepared.lanes, session.bindingSelection);
    ui.keys.textContent = mode === "replay" ? "Recorded input playback · Escape stops the replay."
      : session.bindings.map(row => `${row[0].toString(16).toUpperCase()}: ${row[1]}`).join(" · ")
        + (session.touchInput && (!session.localSources || session.localSources.has(2n)) ? " · Touch lanes enabled" : "")
        + (session.hidSources ? ` · ${session.hidSources.size} HID interface(s)` : "")
        + (session.gamepadSources ? ` · ${session.gamepadSources.size} ${session.gamepadProfileFile ? "profile-configured" : "automatic standard"} Gamepad(s); ${session.gamepadDevices.length - session.gamepadSources.size} unmatched device(s) ignored` : "");
    for (let index = 0; index < prepared.samples; index++) {
      const sample = await playRpc(session, "play-sample");
      if (sample?.kind !== "sample") throw new Error("Prepared audio asset count changed.");
      await session.audio.sample(sample);
    }
    const end = await playRpc(session, "play-sample");
    if (end?.kind !== "samples-end") throw new Error("Prepared audio assets exceeded their declared count.");
    if (session.endFrame === undefined) await session.audio.finish();
    else await session.audio.finish(session.endFrame);
    const descriptor = await session.audio.openCommandPort();
    try {
      if (activePlay !== session || session.owner !== owner || session.phase === "closing") {
        descriptor?.port?.close();
        return;
      }
      if (!descriptor?.port || descriptor.generation !== session.id
        || descriptor.queueCapacity !== session.audioLimits.queueCapacity
        || !Number.isSafeInteger(descriptor.timeoutMs) || descriptor.timeoutMs < 1 || descriptor.timeoutMs > 60000) {
        throw new Error("Audio command handoff did not preserve its owner configuration.");
      }
      const ready = await playRpc(session, "play-audio", descriptor, [descriptor.port]);
      if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
      if (ready?.kind !== "audio-ready" || ready.commandsPending !== false) throw new Error("Direct audio commands were not fully acknowledged.");
      session.commandsPending = false;
    } catch (error) {
      // A throwing transfer may still leave this endpoint locally owned. After
      // a successful transfer its detached wrapper cannot close the Worker port.
      try { descriptor?.port?.close(); } catch {}
      throw error;
    }
    if (session.multiplayer) {
      ui["multiplayer-status"].textContent = "Audio ready · waiting for the peer and committed start…";
      const schedule = await playRpc(session, "play-network-ready");
      if (schedule?.kind !== "multiplayer-start" || typeof schedule.targetHostNs !== "bigint"
        || typeof schedule.uncertaintyNs !== "bigint"
        || typeof schedule.songTargetHostNs !== "bigint" || schedule.songTargetHostNs > 9223372036854775807n
        || schedule.songTargetHostNs - schedule.targetHostNs !== 100000000n) throw new Error("Invalid committed multiplayer preroll schedule.");
      const clock = session.audio.controlClock();
      if (clock.sampleRate !== session.audio.sampleRate) throw new Error("Multiplayer audio clock changed its sample rate.");
      const projected = committedStartProjection(clock, schedule.targetHostNs, performance.now(), schedule.uncertaintyNs);
      session.targetHostNs = schedule.targetHostNs;
      session.startFrame = projected.startFrame;
      session.origin = projected.origin;
      if (session.startFrame <= session.audio.currentFrame) throw new Error("Committed multiplayer output frame was already rendered.");
    } else {
      const clock = session.audio.controlClock();
      session.startFrame = session.audio.currentFrame + BigInt(Math.ceil(session.audio.sampleRate / 4));
      session.origin = startProjection(clock, session.startFrame);
    }
    await session.audio.arm(session.startFrame);
    await playRpc(session, "play-activate", { hostNs: session.origin, startFrame: session.startFrame,
      ...(session.multiplayer ? { targetHostNs: session.targetHostNs } : {}) });
    if (millisecondsToNanos(performance.now()) >= session.origin) throw new Error("Playback activation missed its chosen start. Start a fresh session.");
    session.phase = "playing";
    ui.rate.value = String(session.audio.sampleRate);
    controls();
    ui.stop.focus();
    status(mode === "replay" ? "Playing recorded replay. Stop ends this session." : "Playing. Stop ends this session; leaving the page stops playback.");
    session.timer = setInterval(() => { if (session.mode === "live") pumpInput(session); pumpPresentation(session); }, 8);
  } catch (error) {
    if (activePlay === session && session.phase !== "closing") await stopPlay(`Playback failed: ${String(error.message).slice(0, 4096)}`, true);
  }
}

function key(event, down) {
  const session = activePlay;
  if (!session || session.phase === "closing") return;
  if (event.code === "Escape" && down) { event.preventDefault(); void stopPlay("Playback stopped."); return; }
  if (session.localSources && !session.localSources.has(1n)) return;
  if (session.phase !== "playing") return;
  const binding = session.bindings.find(row => row[1] === event.code);
  if (!binding) return;
  event.preventDefault();
  if (event.repeat || (down && session.pressed.has(event.code)) || (!down && !session.pressed.has(event.code))) return;
  try {
    if (session.events.length >= 1024) throw new Error("Pending keyboard input capacity exceeded.");
    const hostNs = millisecondsToNanos(event.timeStamp);
    if (hostNs < session.lastHost) throw new Error("Keyboard input arrived behind the accepted gameplay watermark.");
    if (down) session.pressed.add(event.code); else session.pressed.delete(event.code);
    session.events.push({ hostNs, key: binding[2], down, sequence: nextInputSequence(session) });
    session.completionReady = false;
    pumpInput(session);
  } catch (error) { void stopPlay(`Playback failed: ${error.message}`, true); }
}

function finiteTouchSample(value) {
  return typeof value === "number" && Number.isFinite(value) && Number.isFinite(Math.fround(value));
}

function touch(event, phase, surface, lost = false) {
  const session = activePlay;
  if (!session || session.phase !== "playing" || !session.touchInput || session.mode !== "live"
    || surface !== canvas || surface !== session.canvas || session.owner !== owner) return;
  if (session.localSources && !session.localSources.has(2n)) return;
  if (!lost && event.pointerType !== "touch") return;
  const id = event.pointerId;
  const previous = session.contacts.get(id);
  if (lost && !previous) return;
  if ((phase === 0 && previous) || (phase !== 0 && !previous)) return;
  event.preventDefault();
  try {
    if (!Number.isInteger(id) || id < -2147483648 || id > 2147483647) throw new Error("Touch pointer identity exceeds signed 32 bits.");
    if (session.events.length >= 1024) throw new Error("Pending input capacity exceeded.");
    if (phase === 0 && session.contacts.size >= 256) throw new Error("Touch contact capacity exceeded.");
    const hostNs = millisecondsToNanos(event.timeStamp);
    if (hostNs < session.lastHost) throw new Error("Touch input arrived behind the accepted gameplay watermark.");
    const x = lost && !finiteTouchSample(event.offsetX) ? previous.x : event.offsetX;
    const y = lost && !finiteTouchSample(event.offsetY) ? previous.y : event.offsetY;
    const pressure = lost && !finiteTouchSample(event.pressure) ? previous.pressure : event.pressure;
    const width = lost && !(Number.isFinite(cssExtent[0]) && cssExtent[0] > 0) ? previous.width : cssExtent[0];
    const height = lost && !(Number.isFinite(cssExtent[1]) && cssExtent[1] > 0) ? previous.height : cssExtent[1];
    if (!finiteTouchSample(x) || !finiteTouchSample(y) || !finiteTouchSample(pressure)
      || !Number.isFinite(width) || width <= 0 || !Number.isFinite(height) || height <= 0) {
      throw new Error("Touch input requires finite coordinates, pressure and a positive canvas extent.");
    }
    const contact = previous?.contact ?? session.nextContact + 1n;
    if (contact > 18446744073709551615n) throw new Error("Touch acquisition identity exhausted.");
    const current = { contact, x, y, pressure, width, height };
    if (phase === 0) {
      // Capture belongs to this contact before any event can reach the Worker.
      surface.setPointerCapture(id);
      session.contacts.set(id, current);
      session.nextContact = contact;
    } else if (phase === 2 || phase === 3) {
      session.contacts.delete(id);
      // Native release may emit lost capture; the removed owner cannot cancel twice.
      if (!lost) surface.releasePointerCapture(id);
    } else session.contacts.set(id, current);
    const sequence = nextInputSequence(session);
    session.events.push({ kind: "touch", hostNs, sequence, contact, phase, code: id >>> 0,
      x, y, pressure, width, height });
    session.completionReady = false;
    pumpInput(session);
  } catch (error) { void stopPlay(`Playback failed: ${String(error.message).slice(0, 4096)}`, true); }
}

function releaseTouches(session) {
  delete session.canvas.dataset.touchInput;
  // Remove ownership before releasing any capture, including synchronous callbacks.
  const ids = [...session.contacts.keys()];
  session.contacts.clear();
  for (const id of ids) {
    try { session.canvas.releasePointerCapture(id); } catch { /* Browser may already have released it. */ }
  }
}

function outputTimestamp(session) {
  try { return session.audio.outputTimestamp(); }
  catch (error) {
    if (error.code === "unsupported" || error.code === "unavailable") return null;
    throw error;
  }
}
function finishPlay(session) {
  if (activePlay === session && session.phase === "playing" && session.completionReady
    && session.events.length === 0 && session.tickPending === null && session.renderPending === null
    && !session.commandsPending && session.completionTick === session.tickId) {
    void stopPlay(session.mode === "replay" ? "Recorded replay ended."
      : session.endNs === undefined ? "Song completed." : "Section completed.", false, true);
  }
}
function pumpInput(session) {
  if (activePlay !== session || session.mode !== "live" || session.phase !== "playing" || session.tickPending !== null || session.inputPumping) return;
  session.inputPumping = true;
  try {
    session.gamepadOwner?.poll();
    if (activePlay !== session || session.owner !== owner || session.phase !== "playing") return;
    const events = session.events.splice(0, 256);
    let lastInput = session.lastHost;
    for (const event of events) if (event.hostNs > lastInput) lastInput = event.hostNs;
    let watermark = null;
    if (!session.events.length) {
      watermark = millisecondsToNanos(Math.max(0, performance.now() - 12));
      if (watermark < lastInput) watermark = lastInput;
    }
    const tickId = ++session.tickId;
    if (!Number.isSafeInteger(tickId)) throw new Error("Gameplay step identity exhausted.");
    session.completionReady = false;
    const timer = setTimeout(() => { if (session.tickPending?.tickId === tickId) void stopPlay("Gameplay Worker stopped responding.", true); }, 10000);
    session.tickPending = { tickId, timer, watermark, lastInput };
    worker.postMessage({ kind: "play-step", playId: session.id, tickId, events, watermark, contextFrame: session.audio.currentFrame });
  } catch (error) { void stopPlay(`Playback failed: ${error.message}`, true); }
  finally { session.inputPumping = false; }
}

function pumpPresentation(session) {
  if (activePlay !== session || session.phase !== "playing" || session.renderPending) return;
  try {
    const renderId = ++session.renderId;
    if (!Number.isSafeInteger(renderId)) throw new Error("Audio report identity exhausted.");
    const timestamp = outputTimestamp(session);
    const observedNowMs = performance.now();
    const timer = setTimeout(() => { if (session.renderPending?.renderId === renderId) void stopPlay("Audio report Worker stopped responding.", true); }, 10000);
    session.renderPending = { renderId, timer };
    worker.postMessage({ kind: "play-render", playId: session.id, renderId, timestamp, observedNowMs });
  } catch (error) {
    if (session.phase === "playing") void stopPlay(`Playback failed: ${String(error.message).slice(0, 4096)}`, true);
  }
}

function receiveMultiplayer(session, event) {
  if (!session.multiplayer || session.phase === "closing" || session.owner !== owner || !event) return;
  const field = ui["multiplayer-status"];
  if (event.kind === "progress" || event.kind === "final-progress") return;
  if (event.kind === "peer-display-unavailable") {
    if (session.peerDisplayFailed) return;
    session.peerDisplayFailed = true;
    const reason = typeof event.error === "string" && event.error.length > 0 && event.error.length <= 4096
      ? event.error : "Invalid peer presentation failure notice.";
    field.textContent = `Peer display unavailable: ${reason} Local play and multiplayer transport continue.`;
  } else if (event.kind === "disconnected") {
    field.textContent = `Multiplayer disconnected: ${String(event.error ?? "Connection lost").slice(0, 4096)}${session.phase === "playing" ? " · local play continues." : "."}`;
  } else if (event.kind === "connected") field.textContent = "Connected · checking compatible setup and readiness…";
  else if (event.kind === "ready") field.textContent = "Peer ready · agreeing on the start…";
  else if (event.kind === "start") field.textContent = "Shared software start committed · preparing output…";
  else if (event.kind === "final-acknowledged") field.textContent = "Peer acknowledged the final score prefix.";
}

function finalPeerText(peer) {
  try {
    if (!peer || typeof peer !== "object" || Array.isArray(peer)
      || !["waiting", "connected", "disconnected", "stopped"].includes(peer.status)
      || typeof peer.final !== "boolean"
      || !(peer.error === null || (typeof peer.error === "string" && peer.error.length > 0 && peer.error.length <= 4096))) {
      throw new Error("Final peer summary unavailable or malformed.");
    }
    const display = peer.error === null ? "" : ` Live peer display unavailable: ${peer.error}`;
    if (peer.progress === null) {
      if (peer.final) throw new Error("A final peer prefix has no reported progress.");
      return ` Peer status ${peer.status} · no received score prefix.${display}`;
    }
    const row = peer.progress;
    if (!row || typeof row !== "object" || Array.isArray(row) || peer.status === "waiting"
      || typeof row.songNs !== "bigint" || row.songNs < -9223372036854775808n || row.songNs > 9223372036854775807n
      || [row.hits, row.misses, row.combo, row.maxCombo].some(value => typeof value !== "bigint" || value < 0n || value > 18446744073709551615n)
      || row.combo > row.maxCombo || row.maxCombo > row.hits || row.hits + row.misses > 18446744073709551615n) {
      throw new Error("Final peer score prefix was malformed.");
    }
    return ` Peer self-reported${peer.final ? " final prefix" : " prefix"} · ${peer.status} · ${seconds(row.songNs.toString())} s`
      + ` · Hits ${row.hits} · Misses ${row.misses} · Combo ${row.combo} · Max combo ${row.maxCombo}.${display}`;
  } catch (error) {
    return ` ${String(error.message).slice(0, 4096)} Local result unchanged.`;
  }
}

function receivePlay(data) {
  const session = activePlay;
  if (!session || data.playId !== session.id) return;
  if (data.kind === "play-opponents") {
    receiveOpponents(session, data);
  } else if (data.kind === "play-multiplayer") {
    receiveMultiplayer(session, data.event);
  } else if (data.kind === "play-reply") {
    const request = session.rpc;
    if (!request || data.rpcId !== request.rpcId) return;
    session.rpc = null;
    clearTimeout(request.timer);
    if (typeof data.error === "string") request.reject(new Error(data.error));
    else request.resolve(data.result);
  } else if (data.kind === "play-stopped") {
    session.finalScore = data;
    replayReceipt(session, data);
    releasePlayWorker(session);
    if (session.phase !== "closing") void stopPlay("Gameplay stopped without the Window cleanup request.", true);
  } else if (data.kind === "play-error") {
    session.finalScore = data;
    replayReceipt(session, data);
    if (data.released === false) {
      session.cleanupError = String(data.message).slice(0, 4096);
      stop();
    } else releasePlayWorker(session);
    void stopPlay(`Playback failed: ${data.message}` + (session.localPlan ? "" : ` · Hits ${data.hits}, misses ${data.misses}`), true);
  } else if (data.kind === "play-commands" && session.phase !== "closing") {
    void stopPlay("Unexpected Window command relay after direct audio handoff.", true);
  } else if (data.kind === "play-render-done" && session.phase === "playing") {
    if (!session.renderPending || data.renderId !== session.renderPending.renderId) { void stopPlay("Audio report response was not correlated.", true); return; }
    if (typeof data.completed !== "boolean" || typeof data.commandsPending !== "boolean"
      || !Number.isSafeInteger(data.observedTick) || data.observedTick < 0 || data.observedTick > session.tickId
      || (data.completed && data.commandsPending)) { void stopPlay("Song completion evidence was malformed.", true); return; }
    clearTimeout(session.renderPending.timer);
    session.renderPending = null;
    session.commandsPending = data.commandsPending;
    session.completionTick = data.observedTick;
    session.completionReady = data.completed && data.observedTick === session.tickId && !data.commandsPending;
    finishPlay(session);
  } else if (data.kind === "play-step-done" && session.phase === "playing") {
    const pending = session.tickPending;
    if (!pending || data.tickId !== pending.tickId) { void stopPlay("Gameplay step response was not correlated.", true); return; }
    if (typeof data.commandsPending !== "boolean") { void stopPlay("Gameplay command ownership was malformed.", true); return; }
    clearTimeout(pending.timer);
    session.tickPending = null;
    session.lastHost = pending.watermark ?? pending.lastInput;
    session.commandsPending = data.commandsPending;
    if (data.commandsPending) session.completionReady = false;
    if (session.events.length) pumpInput(session);
    finishPlay(session);
  }
}

function releasePlayWorker(session) {
  session.workerReleased = true;
  if (session.workerStop) {
    clearTimeout(session.workerStop.timer);
    session.workerStop.resolve();
    session.workerStop = null;
  }
}

function stopPlay(reason, failed = false, completed = false) {
  const session = activePlay;
  if (!session) return Promise.resolve();
  if (session.stopping) return session.stopping;
  const hadHid = session.hidOwner !== null;
  session.naturalFinishRequested = completed;
  session.phase = "closing";
  let gamepadsStopped;
  try {
    session.gamepadOwner?.close();
    if (session.gamepadOwner?.cleanupFailure) throw session.gamepadOwner.cleanupFailure;
    gamepadsStopped = Promise.resolve();
  } catch (error) { gamepadsStopped = Promise.reject(error); }
  gamepadsStopped.catch(() => {});
  // Detach acquisition synchronously; the returned promise also owns any late
  // authorized open. Join it alongside audio and Worker release below.
  let hidStopped;
  try { hidStopped = session.hidOwner?.close() ?? Promise.resolve(); }
  catch (error) { hidStopped = Promise.reject(error); }
  hidStopped.catch(() => {});
  session.controller.abort();
  clearInterval(session.timer);
  if (session.tickPending) clearTimeout(session.tickPending.timer);
  session.tickPending = null;
  if (session.renderPending) clearTimeout(session.renderPending.timer);
  session.renderPending = null;
  if (session.rpc) {
    clearTimeout(session.rpc.timer);
    session.rpc.reject(new Error("Playback operation cancelled."));
    session.rpc = null;
  }
  session.events.length = 0;
  session.pressed.clear();
  releaseTouches(session);
  let workerStopped = Promise.resolve();
  if (session.workerStarted && !session.workerReleased && worker) {
    workerStopped = new Promise(resolve => {
      const timer = setTimeout(() => {
        // Termination establishes ownership release if the stop receipt never arrives.
        stop();
        failed = true;
        reason = "Gameplay cleanup timed out. Reload the page before playing again.";
      }, 10000);
      session.workerStop = { timer, resolve };
    });
    try { worker.postMessage({ kind: "play-stop", playId: session.id, completed }); }
    catch { stop(); failed = true; reason = "Gameplay Worker could not stop. Reload the page."; }
  }
  controls();
  status(reason, failed);
  session.stopping = (async () => {
    try {
      await Promise.all([
        (async () => {
          try {
            const audio = session.audio ?? await session.opening?.catch(error => {
              if (error?.cleanupError) throw error.cleanupError;
              return null;
            });
            await audio?.stop();
          } catch (error) {
            stop();
            failed = true;
            reason += ` Audio cleanup failed: ${String(error.message).slice(0, 4096)} Reload the page before playing again.`;
          }
        })(),
        hidStopped.catch(error => {
          hidOwnershipFailed = true;
          stop();
          failed = true;
          reason += ` HID cleanup failed: ${String(error.message).slice(0, 4096)} Reload the page before playing again.`;
        }),
        gamepadsStopped.catch(error => {
          hidOwnershipFailed = true;
          stop();
          failed = true;
          reason += ` Gamepad cleanup failed: ${String(error.message).slice(0, 4096)} Reload the page before playing again.`;
        }),
      ]);
    }
    finally {
      await workerStopped;
      session.hidOwner = session.hidConnecting = session.hidDevices = session.hidSources = session.hidProfileFile = null;
      session.gamepadOwner = session.gamepadDevices = session.gamepadSources = session.gamepadProfileFile = null;
      if (session.cleanupError !== null) {
        failed = true;
        reason = `Gameplay cleanup failed: ${session.cleanupError} Reload the page before playing again.`;
      }
      if (activePlay === session) {
        if (session.owner === owner) {
          ui.title.textContent = session.preview.title;
          ui.details.textContent = session.preview.details;
          ui.position.value = session.preview.position;
          ui.keys.textContent = "";
          if (hadHid) ui["hid-status"].textContent = "HID session released. The selected profile is retained for live play.";
          clearOpponentResults(opponents.size ? "Saved comparison stopped; selections retained for the next live play." : "No saved opponents selected.");
        }
        const score = session.finalScore;
        if (session.owner === owner) finalOpponentResults(session, score?.savedOpponents);
        if (session.multiplayer && session.owner === owner) {
          const outcome = score?.multiplayer;
          const localOutcome = outcome?.finalAcknowledged === true && outcome?.finalWritten === true
            ? "Final score prefix written and acknowledged by the peer."
            : outcome?.finalWritten === true ? `Final score prefix written · peer ACK unavailable${outcome.error ? `: ${String(outcome.error).slice(0, 4096)}` : "."}`
              : `Multiplayer ended without a confirmed final score write${outcome?.error ? `: ${String(outcome.error).slice(0, 4096)}` : "."}`;
          ui["multiplayer-status"].textContent = localOutcome + finalPeerText(outcome?.peer);
        }
        const result = !session.localPlan && score && typeof score.hits === "bigint" && typeof score.misses === "bigint"
          ? ` Hits ${score.hits} · Misses ${score.misses} · Combo ${score.combo ?? "unavailable"}.` : "";
        if (session.replayError !== null) {
          failed = true;
          reason += ` Replay export failed: ${session.replayError}`;
        }
        if (session.localPlan && session.owner === owner) {
          showLocalResults(session, failed);
          localRoster.clearSources();
          showLocalRoster();
          ui["local-status"].textContent = "Local input sources released. Discover again before the next local session.";
        } else if (session.replay !== null) {
          revokeReplayURL();
          lastReplay = { bytes: session.replay.bytes, complete: session.replay.complete && !failed, id: session.id,
            chartPath: session.chartPath, hits: score?.hits ?? null, misses: score?.misses ?? null, combo: score?.combo ?? null };
          ui.export.textContent = `Download last replay (${lastReplay.complete ? "complete" : "prefix"})`;
          capturedReplays = [lastReplay];
          showCapturedReplays(true);
        }
        activePlay = null;
        session.opponentSelection = null;
        controls();
        if (session.owner === owner || failed) status(reason + result, failed);
      }
    }
  })();
  return session.stopping;
}

function replayReceipt(session, data) {
  if (session.localPlan) {
    session.localReplays = localReplayReceipt(session.localPlan, data, { recording: session.recordReplay,
      natural: session.naturalFinishRequested, bytesPerMember: Math.floor(64 * 1024 * 1024 / session.localPlan.players.length) });
    session.localScores = localScoreRows(session.localPlan, data.localScores);
    return;
  }
  try {
    // A non-recording older peer may omit export fields, but cannot publish bytes.
    if (!session.recordReplay && data.replay === undefined && data.replayComplete === undefined
      && data.replayError === undefined) return;
    if (typeof data.replayComplete !== "boolean" || !(data.replayError === null
      || (typeof data.replayError === "string" && data.replayError.length <= 4096))) throw new Error("Invalid replay export metadata.");
    if (data.replay === null) {
      if (data.replayComplete) throw new Error("Complete replay has no encoded data.");
    } else {
      const bytes = data.replay;
      if (!session.recordReplay || !(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
        || bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength
        || bytes.length === 0 || bytes.length > 64 * 1024 * 1024
        || (data.replayComplete && (data.kind !== "play-stopped" || !session.naturalFinishRequested))) {
        throw new Error("Invalid replay export ownership or layout.");
      }
      session.replay = { bytes, complete: data.replayComplete };
    }
    if (data.replayError !== null) session.replayError = data.replayError;
  } catch (error) {
    session.replay = null;
    session.replayError = String(error.message).slice(0, 4096);
  }
}

function localScoreRows(plan, rows) {
  return plan.players.map((player, index) => {
    const unavailable = { player, songNs: null, hits: null, misses: null, combo: null, maxCombo: null };
    if (!Array.isArray(rows) || rows.length !== plan.players.length) return unavailable;
    const row = rows[index];
    if (!row || row.player !== player || !(row.songNs === null || (typeof row.songNs === "bigint"
      && row.songNs >= -9223372036854775808n && row.songNs <= 9223372036854775807n))) return unavailable;
    if ([row.hits, row.misses, row.combo, row.maxCombo].some(value => !(value === null
      || (typeof value === "bigint" && value >= 0n && value <= 18446744073709551615n)))) return unavailable;
    if ((row.combo !== null && row.maxCombo !== null && row.combo > row.maxCombo)
      || (row.maxCombo !== null && row.hits !== null && row.maxCombo > row.hits)
      || (row.hits !== null && row.misses !== null && row.hits + row.misses > 18446744073709551615n)) return unavailable;
    return { player, songNs: row.songNs, hits: row.hits, misses: row.misses, combo: row.combo, maxCombo: row.maxCombo };
  });
}

function showCapturedReplays(selectSolo = false) {
  ui["captured-replay"].replaceChildren(new Option("Choose a captured replay", ""));
  for (const record of capturedReplays) {
    ui["captured-replay"].append(new Option(`${record.player === undefined ? "Solo" : `Player ${record.player}`} · ${record.complete ? "complete" : "prefix"}`, String(record.player ?? "solo")));
  }
  ui["captured-replay"].value = selectSolo ? "solo" : "";
  if (!selectSolo) { lastReplay = null; ui.export.textContent = "Choose a captured replay"; }
}

function showLocalResults(session, failed) {
  const scores = session.localScores ?? localScoreRows(session.localPlan, null);
  const rows = document.createDocumentFragment();
  const recordings = [];
  for (let index = 0; index < session.localPlan.players.length; index++) {
    const player = session.localPlan.players[index];
    const score = scores[index];
    const replay = session.localReplays?.[index];
    const item = document.createElement("li");
    item.textContent = `Player ${player} · Hits ${score.hits ?? "unavailable"} · Misses ${score.misses ?? "unavailable"}`
      + ` · Combo ${score.combo ?? "unavailable"} · Max combo ${score.maxCombo ?? "unavailable"}`
      + (score.songNs === null ? "" : ` · ${seconds(score.songNs.toString())} s`)
      + (replay?.replayError ? ` · Replay unavailable: ${replay.replayError}`
        : replay?.replay ? ` · ${replay.replayComplete && !failed ? "Complete recording" : "Recorded prefix"}` : " · No recording");
    rows.append(item);
    if (replay?.replay) recordings.push({ player, bytes: replay.replay, complete: replay.replayComplete && !failed,
      id: session.id, chartPath: session.chartPath, hits: score.hits, misses: score.misses, combo: score.combo });
  }
  ui["local-results"].replaceChildren(rows);
  if (session.recordReplay) {
    revokeReplayURL();
    capturedReplays = recordings;
    showCapturedReplays(); // A member recording is never selected by aliasing the first row.
  }
}

function replayFilename(record) {
  return `beatkernel-${record.id}${record.player === undefined ? "" : `-player-${record.player}`}-${record.complete ? "complete" : "prefix"}.bkr`;
}
function revokeReplayURL() {
  clearTimeout(replayURLTimer);
  replayURLTimer = null;
  if (replayURL !== null) URL.revokeObjectURL(replayURL);
  replayURL = null;
}
function downloadReplay() {
  if (activePlay !== null || recordsOperation !== null || hidPermission || hidOwnershipFailed || lastReplay === null) return;
  let link = null;
  try {
    revokeReplayURL();
    replayURL = URL.createObjectURL(new Blob([lastReplay.bytes], { type: "application/octet-stream" }));
    link = document.createElement("a");
    link.href = replayURL;
    link.download = replayFilename(lastReplay);
    document.body.appendChild(link);
    link.click();
    replayURLTimer = setTimeout(revokeReplayURL, 60000);
  } catch (error) {
    revokeReplayURL();
    status(`Replay download failed: ${String(error.message).slice(0, 4096)}`, true);
  } finally { link?.remove(); }
}

function opponentStatus(text, error = false) {
  ui["opponents-status"].textContent = text;
  ui["opponents-status"].dataset.error = String(error);
}
function clearOpponentResults(text) {
  opponentResultRows = [];
  ui["opponents-results"].replaceChildren();
  opponentStatus(text);
}
function opponentChoice() {
  const kind = ui["opponents-kind"].value;
  if (kind !== "own" && kind !== "other") throw new Error("Choose Own or Other for the selected recording.");
  return { own: kind === "own", label: ui["opponents-label"].value };
}
function addOpponent(file, sourceKey, choice) {
  opponents.add({ file, sourceKey, own: choice.own, label: choice.label || opponentLabel(file.name) });
  showOpponentSelection();
  opponentStatus(`${opponents.size} saved opponent(s) selected · ${opponents.byteLength} bytes. Compatibility is checked on Live Play.`);
  controls();
}
function showOpponentSelection() {
  opponentButtons = [];
  const rows = document.createDocumentFragment();
  for (const entry of opponents.snapshot()) {
    const row = document.createElement("li");
    const label = document.createElement("span");
    label.textContent = `${entry.own ? "Own" : "Other"} · ${entry.label} · ${entry.file.size} bytes `;
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = "Remove";
    button.addEventListener("click", () => {
      if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
      opponents.remove(entry.sourceKey);
      showOpponentSelection();
      clearOpponentResults(opponents.size ? `${opponents.size} saved opponent(s) selected.` : "No saved opponents selected.");
      controls();
    });
    opponentButtons.push(button);
    row.append(label, button);
    rows.append(row);
  }
  ui["opponents-list"].replaceChildren(rows);
}
function receiveOpponents(session, data) {
  if (session.mode !== "live" || session.phase !== "playing" || session.owner !== owner
    || !session.opponentCount || session.opponentsFailed) return;
  // Actual periodic counters are rendered by the Worker-owned common HUD.
  if (data.error === null) return;
  try {
    if (typeof data.error !== "string" || data.error.length === 0 || data.error.length > 4096 || data.opponents !== null) throw new Error("Invalid saved comparison failure message.");
    throw new Error(data.error);
  } catch (error) {
    session.opponentsFailed = true;
    session.opponentError = String(error.message).slice(0, 4096);
    opponentStatus(`Saved comparisons stopped: ${session.opponentError} Local play continues.`, true);
  }
}

function finalOpponentResults(session, result) {
  if (session.mode !== "live" || !session.opponentCount) return;
  try {
    if (session.opponentsFailed) throw new Error(session.opponentError || "Saved comparisons were disabled.");
    if (!result || typeof result !== "object" || Array.isArray(result)) throw new Error("Final saved comparison results are unavailable.");
    if (result.error !== null) {
      if (typeof result.error !== "string" || result.error.length === 0 || result.error.length > 4096 || result.opponents !== null) throw new Error("Invalid final saved comparison failure.");
      throw new Error(result.error);
    }
    const rows = validateOpponentSnapshot(result.opponents, session.opponentCount);
    if (opponentResultRows.length !== rows.length) {
      opponentResultRows = rows.map(() => document.createElement("li"));
      ui["opponents-results"].replaceChildren(...opponentResultRows);
    }
    for (let index = 0; index < rows.length; index++) {
      const row = rows[index];
      const prefix = row.recordedUntilNs === null ? "empty recording" : `recorded through ${seconds(row.recordedUntilNs.toString())} s`;
      opponentResultRows[index].textContent = `${row.kind === "own" ? "Own" : "Other"} · ${row.label} · Hits ${row.hits} · Misses ${row.misses} · Combo ${row.combo} · Best ${row.maxCombo} · ${prefix}`;
    }
    opponentStatus("Final saved comparison prefixes. Own/Other labels are your choices, not verified identities.");
  } catch (error) {
    clearOpponentResults(`Final saved comparisons unavailable: ${String(error.message).slice(0, 4096)}`);
    opponentStatus(`Final saved comparisons unavailable: ${String(error.message).slice(0, 4096)} Local result unchanged.`, true);
  }
}

function closeRecords() {
  recordsOperation?.controller.abort();
  recordsOperation = null;
  const previous = recordsStore;
  recordsStore = null;
  previous?.close();
}
function recordCurrent(operation) {
  return recordsOperation === operation && operation.owner === owner && !operation.controller.signal.aborted;
}
async function openRecords(operation) {
  if (recordsStore && !recordsStore.closed) return recordsStore;
  const opened = await RecordsStore.open({ signal: operation.controller.signal });
  if (!recordCurrent(operation)) {
    opened.close();
    throw new Error("Record library operation was cancelled.");
  }
  recordsStore = opened;
  return opened;
}
function showRecords(entries) {
  const previous = ui.records.value;
  const options = document.createDocumentFragment();
  for (const record of entries) {
    options.append(new Option(`${record.name} · ${record.complete ? "complete capture" : "prefix"} · Hits ${record.hits ?? "unavailable"} · Misses ${record.misses ?? "unavailable"}`, String(record.id)));
  }
  ui.records.replaceChildren(options);
  if (!entries.length) ui.records.append(new Option("No saved records", ""));
  else if (entries.some(record => String(record.id) === previous)) ui.records.value = previous;
}
async function recordAction(action) {
  if (!initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  const captured = lastReplay;
  if (action === "save" && captured === null) return;
  const id = Number(ui.records.value);
  if ((action === "use" || action === "delete" || action === "opponent") && (!Number.isSafeInteger(id) || id < 1)) return;
  const operation = { owner, controller: new AbortController() };
  recordsOperation = operation;
  controls();
  status(action === "save" ? "Saving the captured recording…" : "Opening saved records…");
  let committed = "";
  try {
    const choice = action === "opponent" ? opponentChoice() : null;
    const store = await openRecords(operation);
    if (!recordCurrent(operation)) return;
    if (action === "use" || action === "opponent") {
      const loaded = await store.load(id);
      if (!recordCurrent(operation)) return;
      const file = new File([loaded.bytes], loaded.metadata.name, { type: "application/octet-stream" });
      if (action === "opponent") {
        addOpponent(file, `record:${id}`, choice);
        status("Saved opponent selected. Live Play checks it against the prepared chart.");
      } else {
        selectedReplay = file;
        selectedReplayKey = `record:${id}`;
        ui["replay-file"].value = "";
        ui["replay-name"].textContent = `${file.name} · ${file.size} bytes · matching chart: ${loaded.metadata.chartPath}`;
        status("Saved replay selected. Prepare its matching chart, then choose Play replay.");
      }
    } else {
      if (action === "save") {
        await store.save({ bytes: captured.bytes, name: replayFilename(captured),
          chartPath: captured.chartPath, complete: captured.complete,
          hits: captured.hits, misses: captured.misses, combo: captured.combo });
        if (!recordCurrent(operation)) return;
        committed = "Recording saved. ";
      } else if (action === "delete") {
        const removed = await store.remove(id);
        if (!recordCurrent(operation)) return;
        committed = removed ? "Selected record deleted. " : "Selected record was already absent. ";
      }
      const entries = await store.list();
      if (!recordCurrent(operation)) return;
      showRecords(entries);
      status(`${committed}${entries.length} saved record(s).`);
    }
  } catch (error) {
    if (recordCurrent(operation)) status(`${committed}Record library failed: ${String(error.message).slice(0, 4096)} Current replay and download remain available.`, true);
  } finally {
    if (recordsOperation === operation) {
      recordsOperation = null;
      controls();
    }
  }
}
start();
