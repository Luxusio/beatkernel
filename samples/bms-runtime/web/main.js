import { snapshotFiles, nanoseconds, seconds } from "./host_model.mjs";
import { AudioHost } from "./audio-host.mjs";
import { RecordsStore } from "./record-store.mjs";
import { HidInputOwner } from "./hid-input.mjs";
import { GamepadInputOwner } from "./gamepad-input.mjs";
import { PointerInputOwner } from "./pointer-input.mjs";
import { LocalRoster, validateLocalPrepared, localReplayReceipt } from "./local-play-host.mjs";
import { snapshotHidDevices } from "./hid-profile.mjs";
import { snapshotBrowserSettings } from "./settings-profile.mjs";
import { SavedOpponentSelection, opponentLabel, validateOpponentSnapshot, validateOpponentTargets, validateLocalOpponentSnapshot } from "./saved-opponents.mjs";
import { KEY_BINDINGS, KEY_CHOICES, PLAY_PCM_SAMPLES, snapshotBindings, bindingsFor, timingFromMilliseconds, audioOutputFromFields, audioLimitsFromFields, sectionFromSeconds, validateStart, replayOutputFromMetadata, millisecondsToNanos, startProjection, committedStartProjection } from "./play-model.mjs";

const byId = id => document.getElementById(id);
const ui = Object.fromEntries(["folder", "files", "chart", "rate", "seed", "prepare", "position", "seek", "title", "details", "status", "viewport", "play", "stop", "keys", "record", "export", "replay-file", "replay-play", "replay-name", "records", "records-refresh", "records-save", "records-use", "records-delete", "multiplayer", "multiplayer-url", "multiplayer-role", "multiplayer-status", "opponents-kind", "opponents-label", "opponents-add", "records-opponent", "opponents-clear", "opponents-list", "opponents-status", "opponents-results", "judge-early", "judge-late", "judge-offset", "live-start", "live-end", "bindings", "bindings-reset", "output-latency", "output-latency-ms", "output-rate", "audio-queue", "audio-voices", "audio-pending", "audio-frames", "audio-commands", "touch-input", "hid-input", "hid-authorize", "hid-profile", "hid-profile-name", "hid-status", "gamepad-profile", "gamepad-profile-name", "gamepad-profile-clear"].map(id => [id, byId(id)]));
for (const id of ["local-count", "local-discover", "local-release", "local-sources", "local-status", "local-page", "local-results", "captured-replay"]) ui[id] = byId(id);
for (const id of ["multiplayer-mode", "room-seal", "room-ready", "room-leave", "room-score-prev", "room-score-next", "room-score-page"]) ui[id] = byId(id);
for (const id of ["settings-save", "settings-load", "settings-status"]) ui[id] = byId(id);
for (const id of ["pointer-input", "pointer-bindings"]) ui[id] = byId(id);
let canvas = byId("canvas");
let cssExtent = [0, 0];
let surfaceExtent = [0, 0];
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
let roomResults = null;
let settingsOperation = null;
let settingsURL = null;
let settingsURLTimer = null;
let seeking = false;
const settingsFields = Object.freeze([
  ["timing", "earlyMs", "judge-early", 21], ["timing", "lateMs", "judge-late", 21], ["timing", "offsetMs", "judge-offset", 21],
  ["output", "latency", "output-latency", 16], ["output", "latencyMs", "output-latency-ms", 21], ["output", "rate", "output-rate", 10],
  ["capacities", "queueCapacity", "audio-queue", 5], ["capacities", "maxVoices", "audio-voices", 5],
  ["capacities", "pendingCapacity", "audio-pending", 5], ["capacities", "maxFrames", "audio-frames", 5],
  ["capacities", "maxCommandsPerRender", "audio-commands", 5],
  ["section", "startSeconds", "live-start", 20], ["section", "endSeconds", "live-end", 20],
].map(row => Object.freeze(row)));

function settingsStatus(text, error = false) {
  ui["settings-status"].textContent = text;
  ui["settings-status"].dataset.error = String(error);
}
function settingsIdle() {
  return initialized && worker !== null && !importing && !preparing && !seeking && !activePlay
    && !recordsOperation && !hidPermission && !hidOwnershipFailed && !localSetup && !localCleanup && !localDiscovery
    && !roomResults?.rpc && !roomResults?.scoreChanging;
}
function settingsCurrent(operation) {
  return settingsOperation === operation && operation.owner === owner && operation.worker === worker;
}
function captureSettingsDraft() {
  const draft = { kind: "beatkernel-browser-settings", version: 1, timing: {}, output: {}, capacities: {}, section: {} };
  for (const [group, name, id, maximum] of settingsFields) {
    const value = ui[id].value;
    if (typeof value !== "string" || value.length > maximum) throw new Error(`Settings ${name} exceeds its field limit.`);
    draft[group][name] = value;
  }
  draft.bindings = Object.freeze(bindingFields.map(([lane, field]) => {
    const code = field.value;
    if (typeof code !== "string" || code.length > 32) throw new Error("Keyboard settings exceed their field limit.");
    return Object.freeze([lane, code]);
  }));
  for (const group of ["timing", "output", "capacities", "section"]) Object.freeze(draft[group]);
  return Object.freeze(draft);
}
function sameSettingsDraft(left, right) {
  return settingsFields.every(([group, name]) => left[group][name] === right[group][name])
    && left.bindings.length === right.bindings.length
    && left.bindings.every((row, index) => row[0] === right.bindings[index][0] && row[1] === right.bindings[index][1]);
}
function revokeSettingsURL() {
  clearTimeout(settingsURLTimer);
  settingsURLTimer = null;
  if (settingsURL !== null) URL.revokeObjectURL(settingsURL);
  settingsURL = null;
}
function cancelSettings(reason) {
  const operation = settingsOperation;
  if (!operation) return;
  settingsOperation = null;
  clearTimeout(operation.timer);
  settingsStatus(reason, true);
  controls();
}
function requestSettings(kind, file = null) {
  if (settingsOperation || !settingsIdle()) return;
  const generation = owner, target = worker;
  let operation = null;
  try {
    const draft = captureSettingsDraft();
    if (kind === "settings-profile-load" && (!(file instanceof File) || !Number.isSafeInteger(file.size)
      || file.size < 1 || file.size > 16384)) throw new Error("Choose one nonempty settings file no larger than 16 KiB.");
    if (generation !== owner || target !== worker || settingsOperation || !settingsIdle()) return;
    if (!Number.isSafeInteger(serial + 1)) throw new Error("Settings request identity exhausted.");
    operation = { id: ++serial, owner, worker, kind, draft, timer: null };
    settingsOperation = operation;
    operation.timer = setTimeout(() => {
      if (settingsOperation === operation) cancelSettings("Settings request timed out; the previous draft is retained.");
    }, 10000);
    controls();
    settingsStatus(kind === "settings-profile-save" ? "Saving settings…" : "Loading settings…");
    operation.worker.postMessage(kind === "settings-profile-save"
      ? { kind, id: operation.id, settings: draft } : { kind, id: operation.id, file });
  } catch (error) {
    if (generation !== owner || target !== worker) return;
    if (operation && settingsOperation !== operation) return;
    if (operation) { settingsOperation = null; clearTimeout(operation.timer); }
    settingsStatus(String(error?.message ?? error).slice(0, 4096), true);
    controls();
  }
}
function receiveSettings(data) {
  const operation = settingsOperation;
  if (!operation || data.id !== operation.id || !settingsCurrent(operation)) return;
  let link = null;
  try {
    const draft = captureSettingsDraft();
    if (!settingsCurrent(operation) || !settingsIdle() || !sameSettingsDraft(operation.draft, draft)) throw new Error("Settings draft or ownership changed; nothing was applied.");
    if (data.kind === "settings-profile-error") {
      if (typeof data.message !== "string" || !data.message.length || data.message.length > 4096) throw new Error("Invalid settings error response.");
      throw new Error(data.message);
    }
    if (operation.kind === "settings-profile-load" && data.kind === "settings-profile-loaded") {
      const settings = snapshotBrowserSettings(data.settings);
      const bindings = new Map(settings.bindings);
      const currentDraft = captureSettingsDraft();
      if (!settingsCurrent(operation) || !settingsIdle() || !sameSettingsDraft(operation.draft, currentDraft)) throw new Error("Settings draft or ownership changed; nothing was applied.");
      // All values and the unchanged owner/draft are checked before native DOM setters.
      for (const [group, name, id] of settingsFields) ui[id].value = settings[group][name];
      for (const [lane, field] of bindingFields) field.value = bindings.get(lane);
    } else if (operation.kind === "settings-profile-save" && data.kind === "settings-profile-saved") {
      const bytes = data.bytes;
      if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer) || bytes.buffer.resizable
        || bytes.byteOffset !== 0 || bytes.byteLength < 1 || bytes.byteLength > 16384 || bytes.byteLength !== bytes.buffer.byteLength) {
        throw new Error("Invalid settings download bytes.");
      }
      const currentDraft = captureSettingsDraft();
      if (!settingsCurrent(operation) || !settingsIdle() || !sameSettingsDraft(operation.draft, currentDraft)) throw new Error("Settings draft or ownership changed; nothing was downloaded.");
      revokeSettingsURL();
      const url = URL.createObjectURL(new Blob([bytes], { type: "application/json" }));
      if (settingsOperation !== operation || operation.owner !== owner || operation.worker !== worker) {
        URL.revokeObjectURL(url);
        return;
      }
      settingsURL = url;
      settingsURLTimer = setTimeout(() => { if (settingsURL === url) revokeSettingsURL(); }, 60000);
      link = document.createElement("a");
      link.href = url;
      link.download = "beatkernel-browser-settings.json";
      document.body.appendChild(link);
      link.click();
    } else throw new Error("Unexpected settings response.");
    if (settingsOperation === operation) settingsStatus(operation.kind === "settings-profile-save" ? "Settings downloaded." : "Settings loaded. The complete draft was replaced.");
  } catch (error) {
    if (settingsOperation === operation) {
      if (operation.kind === "settings-profile-save") revokeSettingsURL();
      settingsStatus(String(error?.message ?? error).slice(0, 4096), true);
    }
  } finally {
    link?.remove();
    if (settingsOperation === operation) {
      clearTimeout(operation.timer);
      settingsOperation = null;
      controls();
    }
  }
}

function clearRoomResults() {
  const previous = roomResults;
  roomResults = null;
  if (previous?.rpc) {
    clearTimeout(previous.rpc.timer);
    previous.rpc.reject(new Error("Room Results were replaced."));
    previous.rpc = null;
  }
}

function roomResultsMetadata(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || typeof value.failed !== "boolean" || !Number.isInteger(value.pages) || value.pages < 1 || value.pages > 1008
    || !Number.isInteger(value.page) || value.page < 0 || value.page >= value.pages) {
    throw new Error("Room Results page metadata unavailable.");
  }
  return { scorePage: value.page, scorePages: value.pages, scoreFailed: value.failed };
}

function retainRoomResults(session) {
  clearRoomResults();
  if (!session.room || session.owner !== owner || !worker || session.cleanupError !== null) return;
  const metadata = session.roomResultsNotice ?? session.finalScore?.roomResults;
  if (metadata === null || metadata === undefined) return;
  let scores;
  try { scores = roomResultsMetadata(metadata); }
  catch { scores = { scorePage: 0, scorePages: 0, scoreFailed: true }; }
  roomResults = { id: session.id, owner, ...scores, rpc: null, scoreChanging: false };
}
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
const pointerFields = createPointerFields();
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
      if (settingsOperation || activePlay || localSetup?.phase !== "ready") return;
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
  let pointerError = null;
  try { setup.pointerOwner?.close(); pointerError = setup.pointerOwner?.cleanupFailure; }
  catch (error) { pointerError = error; }
  if (setup.canvas === canvas && (!activePlay || activePlay === setup)) delete setup.canvas.dataset.touchInput;
  let closed;
  try { closed = setup.hidOwner?.close() ?? Promise.resolve(); }
  catch (error) { closed = Promise.reject(error); }
  const cleanup = Promise.resolve(closed).then(() => { if (gamepadError || pointerError) throw gamepadError ?? pointerError; }).catch(error => {
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
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed || localCleanup || localDiscovery
    || localRoster.players.length === 1) return;
  const operation = { owner, players: localRoster.players, hid: ui["hid-input"].checked,
    hidProfile: selectedHidProfile, gamepadProfile: selectedGamepadProfile, touch: ui["touch-input"].checked,
    pointer: ui["pointer-input"].checked };
  localDiscovery = operation;
  controls();
  await releaseLocalSources(undefined, false);
  if (localDiscovery !== operation || operation.owner !== owner || !initialized || activePlay || hidOwnershipFailed || localCleanup || document.hidden
    || operation.players.join(",") !== localRoster.players.join(",") || operation.hid !== ui["hid-input"].checked
    || operation.hidProfile !== selectedHidProfile || operation.gamepadProfile !== selectedGamepadProfile || operation.touch !== ui["touch-input"].checked
    || operation.pointer !== ui["pointer-input"].checked) {
    if (localDiscovery === operation) localDiscovery = null;
    controls();
    return;
  }
  const setup = { owner, phase: "preparing", sequence: 0n, nextSource: 3n, gamepadOwner: null, gamepadDevices: [],
    gamepadSources: null, gamepadProfileFile: selectedGamepadProfile, hidOwner: null, hidDevices: null, hidSources: null,
    canvas, pointerInput: operation.pointer === true, pointerOwner: null, pointerDevices: null, pointerSources: null, pointerSelection: null,
    hidProfileFile: ui["hid-input"].checked ? selectedHidProfile : null, touchInput: ui["touch-input"].checked === true, inventory: [] };
  localSetup = setup;
  controls();
  ui["local-status"].textContent = "Acquiring local input sources…";
  try {
    if (ui["hid-input"].checked && (!hidCapable() || setup.hidProfileFile === null)) throw new Error("Select an HID profile and authorize its devices before discovery.");
    if (setup.gamepadProfileFile !== null && typeof navigator.getGamepads !== "function") throw new Error("Gamepad acquisition is unavailable.");
    if (setup.touchInput && typeof window.PointerEvent !== "function") throw new Error("Touch input is unavailable.");
    if (setup.pointerInput) {
      if (typeof window.PointerEvent !== "function") throw new Error("Pointer input is unavailable.");
      setup.pointerSelection = snapshotPointerChoices();
      setup.pointerOwner = createSessionPointers(setup);
      if (!inputOwnerCurrent(setup)) { setup.pointerOwner.close(); return; }
      setup.pointerDevices = setup.pointerOwner.devices;
    }
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
      ...(setup.pointerDevices ?? []).map(device => Object.freeze({ source: device.source, label: `Window ${device.pointerType} aggregate · source ${device.source}` })),
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

function createPointerFields() {
  const rows = document.createDocumentFragment();
  const fields = [];
  for (const [lane] of KEY_BINDINGS) for (const pointerType of ["mouse", "pen"]) {
    const label = document.createElement("label");
    label.textContent = `Lane ${lane.toString(16).toUpperCase()} ${pointerType}`;
    const field = document.createElement("select");
    field.id = `pointer-${pointerType}-${lane.toString(16)}`;
    field.disabled = true;
    field.append(new Option("Unbound", ""));
    for (let control = 1; control <= 32; control++) field.append(new Option(`Button ${control}`, String(control)));
    field.value = lane >= 0x11 && lane <= (pointerType === "mouse" ? 0x13 : 0x12) ? String(lane - 0x10) : "";
    label.append(field);
    rows.append(label);
    fields.push([lane, pointerType, field]);
  }
  ui["pointer-bindings"].append(rows);
  return fields;
}

function snapshotPointerChoices() {
  const rows = [];
  const seen = new Set();
  for (const [lane, pointerType, field] of pointerFields) {
    const value = field.value;
    if (value === "") continue;
    if (typeof value !== "string" || !/^(?:[1-9]|[12][0-9]|3[0-2])$/.test(value)) throw new Error("Choose an unbound pointer control or button 1 through 32.");
    const control = Number(value);
    const key = `${pointerType}:${control}`;
    if (seen.has(key)) throw new Error("A mouse or pen button can bind only one lane.");
    seen.add(key);
    rows.push(Object.freeze([lane, pointerType, control]));
  }
  if (!rows.length) throw new Error("Enabled pointer input needs at least one button binding.");
  return Object.freeze(rows);
}

function samePointerChoices(left, right) {
  return left.length === right.length && left.every((row, index) => row.every((value, field) => value === right[index][field]));
}

function pointerSetupFor(session) {
  if (!session.pointerOwner || !session.pointerDevices?.length) return null;
  const words = [];
  for (const [lane, pointerType, control] of session.pointerSelection) {
    const device = session.pointerDevices.find(candidate => candidate.pointerType === pointerType);
    if (!device) continue;
    words.push(lane, Number(device.source & 0xffffffffn), Number(device.source >> 32n), control);
  }
  if (!words.length) throw new Error("Assigned pointer sources need button bindings for their chart lanes.");
  return Object.freeze({ devices: session.pointerDevices, bindingWords: Uint32Array.from(words) });
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
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed || localSetup || localDiscovery || localCleanup) return;
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

function createSessionPointers(session) {
  // Reuse the existing touch-action rule for pen direct manipulation. Actual
  // touch admission remains controlled independently by session.touchInput.
  session.canvas.dataset.touchInput = "true";
  return new PointerInputOwner({ target: session.canvas,
    nextSource: () => nextInputSource(session), nextSequence: () => nextInputSequence(session),
    isCurrent: () => inputOwnerCurrent(session),
    onBatch: batch => {
      if (!inputOwnerCurrent(session) || session.phase !== "playing" || session.mode !== "live") return;
      if (!Array.isArray(batch) || batch.length < 1 || batch.length > 1024) throw new Error("Invalid pointer acquisition batch.");
      const accepted = [];
      let sequence = session.pointerSequence ?? null;
      let previousHost = null;
      for (const sample of batch) {
        if (!sample || typeof sample !== "object" || Array.isArray(sample)) throw new Error("Invalid pointer acquisition sample.");
        const { kind, pointerType, hostNs, source, sequence: currentSequence, code, control } = sample;
        if ((kind !== "pointer" && kind !== "pointer-button")
          || !session.pointerOwner.devices.some(device => device.source === source && device.pointerType === pointerType)
          || typeof hostNs !== "bigint" || hostNs < 0n || hostNs > 9223372036854775807n
          || (previousHost !== null && hostNs < previousHost)
          || typeof currentSequence !== "bigint" || currentSequence < 0n || currentSequence > 18446744073709551615n
          || (sequence !== null && currentSequence <= sequence) || currentSequence > session.sequence
          || !Number.isInteger(code) || code < 0 || code > 0xffffffff) throw new Error("Pointer acquisition identity changed.");
        const event = kind === "pointer"
          ? { kind, pointerType, hostNs, source, sequence: currentSequence, code, control, mode: sample.mode, x: sample.x, y: sample.y }
          : { kind, pointerType, hostNs, source, sequence: currentSequence, code, control, state: sample.state };
        if (kind === "pointer" ? control !== 0 || event.mode !== 0 || !finiteTouchSample(event.x) || !finiteTouchSample(event.y)
          : !Number.isInteger(control) || control < 1 || control > 32 || (event.state !== 0 && event.state !== 1 && event.state !== 2)) {
          throw new Error("Invalid acquired pointer position or button state.");
        }
        sequence = currentSequence;
        previousHost = hostNs;
        const admitted = session.pointerSources?.get(source);
        if (!admitted || (kind === "pointer-button" && !admitted.controls.has(control))) continue;
        if (hostNs < session.lastHost) throw new Error("Pointer input arrived behind the accepted gameplay watermark.");
        accepted.push(Object.freeze(event));
      }
      if (!inputOwnerCurrent(session) || session.phase !== "playing") return;
      if (session.events.length + accepted.length > 1024) throw new Error("Pending input capacity exceeded.");
      session.pointerSequence = sequence;
      if (!accepted.length) return;
      session.events.push(...accepted);
      session.completionReady = false;
      pumpInput(session);
    },
    onError: error => {
      if (!inputOwnerCurrent(session)) return;
      if (error.cleanupError) hidOwnershipFailed = true;
      if (localSetup === session) {
        if (error.cleanupError) fatal(new Error("Pointer input cleanup failed. Reload the page."));
        else void releaseLocalSources(`Pointer discovery failed: ${String(error.message).slice(0, 4096)}`);
      } else void stopPlay(`Pointer input failed: ${String(error.message).slice(0, 4096)}`
        + (error.cleanupError ? " Input cleanup failed. Reload the page." : ""), true);
    },
  });
}

function controls() {
  const playing = activePlay !== null;
  const busy = settingsOperation !== null || recordsOperation !== null || hidPermission !== null || hidOwnershipFailed || localCleanup !== null || localDiscovery !== null;
  ui["settings-save"].disabled = ui["settings-load"].disabled = settingsOperation !== null || !settingsIdle();
  ui.folder.disabled = !initialized || preparing || playing || busy || !("webkitdirectory" in ui.folder);
  ui.files.disabled = !initialized || preparing || playing || busy;
  for (const field of [ui.chart, ui.rate, ui.seed, ui.prepare]) field.disabled = !initialized || !libraryId || importing || preparing || playing || busy;
  ui.position.disabled = ui.seek.disabled = !initialized || !hasPreview || importing || preparing || playing || busy;
  ui.play.disabled = !initialized || !hasPreview || importing || preparing || playing || busy || !audioModule;
  ui.stop.disabled = !playing || activePlay.phase === "closing";
  ui.record.disabled = ui.play.disabled;
  ui.multiplayer.disabled = ui.play.disabled;
  ui["multiplayer-url"].disabled = ui["multiplayer-mode"].disabled = ui.play.disabled || !ui.multiplayer.checked;
  ui["multiplayer-role"].disabled = ui.play.disabled || !ui.multiplayer.checked || ui["multiplayer-mode"].value === "room";
  const room = activePlay?.room;
  const lobby = room && activePlay.mode === "live" && activePlay.phase === "preparing" && room.start === null;
  const roomBusy = !lobby || !room.opened || room.leaving || activePlay.rpc !== null || room.control !== null;
  const ownMember = room?.snapshot?.members.find(member => member.participant === room.participant);
  for (const id of ["room-seal", "room-ready", "room-leave"]) ui[id].hidden = !lobby;
  ui["room-seal"].disabled = roomBusy || room.snapshot?.phase !== 0 || room.snapshot.members.length < 2
    || room.snapshot.members[0].participant !== room.participant || room.sealRequested;
  ui["room-ready"].disabled = roomBusy || room.snapshot?.phase !== 1 || !ownMember || ownMember.prepared || room.readyRequested;
  ui["room-leave"].disabled = roomBusy;
  const scores = room && activePlay.phase !== "closing" ? room : !playing ? roomResults : null;
  const scoresVisible = scores && (scores.scorePages > 0 || scores.scoreFailed);
  for (const id of ["room-score-prev", "room-score-next", "room-score-page"]) ui[id].hidden = !scoresVisible;
  const scoresBusy = !scoresVisible || scores.scoreFailed || (playing ? activePlay.rpc !== null : scores.rpc !== null)
    || scores.scoreChanging || scores.leaving || busy || importing || preparing;
  ui["room-score-prev"].disabled = scoresBusy || scores.scorePage === 0;
  ui["room-score-next"].disabled = scoresBusy || scores.scorePage + 1 >= scores.scorePages;
  ui["room-score-page"].textContent = scoresVisible
    ? scores.scoreFailed ? "Room scores unavailable." : `Room ${playing ? "scores" : "results"} ${scores.scorePage + 1} / ${scores.scorePages}` : "";
  ui.export.disabled = playing || busy || lastReplay === null;
  ui["replay-file"].disabled = !initialized || importing || preparing || playing || busy;
  ui["replay-play"].disabled = ui.play.disabled || selectedReplay === null;
  const recordsDisabled = !initialized || importing || preparing || playing || busy;
  const inputLocked = recordsDisabled || localSetup !== null;
  ui["local-count"].disabled = recordsDisabled;
  ui["local-discover"].disabled = recordsDisabled || localRoster.players.length === 1;
  ui["local-release"].disabled = playing || localSetup === null;
  for (const field of localFields) field.disabled = recordsDisabled || localSetup?.phase !== "ready";
  ui["local-page"].disabled = activePlay ? activePlay.phase !== "playing" || !activePlay.localPlan || activePlay.localPlan.automatic === true
    || activePlay.pageChanging || activePlay.rpc !== null
    : recordsDisabled || localRoster.players.length === 1;
  ui["captured-replay"].disabled = playing || busy || capturedReplays.length === 0;
  ui["bindings-reset"].disabled = recordsDisabled;
  ui["touch-input"].disabled = inputLocked;
  ui["pointer-input"].disabled = inputLocked || typeof window.PointerEvent !== "function";
  for (const [, , field] of pointerFields) field.disabled = inputLocked || !ui["pointer-input"].checked || typeof window.PointerEvent !== "function";
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
  cancelSettings("Settings request cancelled with the page.");
  revokeSettingsURL();
  seeking = false;
  clearRoomResults();
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
  const dpr = window.devicePixelRatio || 1;
  // Do not assign canvas backing dimensions here; the renderer validates first.
  const dimensions = [box.width, box.height].map(value => Math.round(value * dpr));
  if (dimensions.some(value => !Number.isSafeInteger(value) || value < 0 || value > 0xffffffff)) return fatal(new Error("Canvas dimensions are outside the supported range."));
  cssExtent = [box.width, box.height];
  surfaceExtent = dimensions;
  worker.postMessage({ kind: "resize", width: dimensions[0], height: dimensions[1] });
}
function densityChanged() {
  density?.removeEventListener("change", densityChanged);
  density = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
  density.addEventListener("change", densityChanged);
  resize();
}

function prepare() {
  if (settingsOperation || !worker || !libraryId || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  try {
    const rate = Number(ui.rate.value);
    const seed = ui.seed.value;
    if (!Number.isInteger(rate) || rate < 1 || rate > 0xffffffff) throw new Error("Sample rate must be a positive 32-bit integer.");
    if (!/^\d{1,20}$/.test(seed) || BigInt(seed) > 0xffffffffffffffffn) throw new Error("Chart seed must fit an unsigned 64-bit integer.");
    if (!ui.chart.value) throw new Error("Select a chart first.");
    selectId = ++serial;
    seeking = false;
    clearRoomResults();
    preparing = true;
    canvas.hidden = true;
    controls();
    status("Preparing chart, sounds and backgrounds…");
    worker.postMessage({ kind: "select", id: selectId, libraryId, path: ui.chart.value, rate, seed });
  } catch (error) { preparing = false; controls(); status(error.message, true); }
}

function received(data) {
  if ((settingsOperation && data?.id === settingsOperation.id)
    || (typeof data?.kind === "string" && data.kind.startsWith("settings-profile-"))) { receiveSettings(data); return; }
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
    seeking = false;
    controls();
    ui.position.value = seconds(data.ns);
    status(`Preview position: ${seconds(data.ns)} seconds.`);
  } else if (data.kind === "seek-error" && data.id === seekId && data.selectedId === selectedId) {
    seeking = false;
    controls();
    status(data.message, true);
  }
  else if (data.kind === "render-wait" && data.selectedId === selectedId) status("The graphics surface is not ready. Resize the view or choose Show position to retry.", true);
}

function start() {
  stop();
  if (hidOwnershipFailed) { status("Input cleanup failed. Reload the page before playing again.", true); return; }
  libraryId = importId = selectId = selectedId = seekId = 0;
  importing = preparing = hasPreview = seeking = false;
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
  surfaceExtent = [0, 0];
  for (const [name, phase] of [["pointerdown", 0], ["pointermove", 1], ["pointerup", 2], ["pointercancel", 3]]) {
    fresh.addEventListener(name, event => touch(event, phase, fresh), { passive: false });
  }
  fresh.addEventListener("lostpointercapture", event => touch(event, 3, fresh, true), { passive: false });
  fresh.addEventListener("contextmenu", event => {
    const session = activePlay;
    const current = () => session && activePlay === session && session.owner === owner
      && canvas === fresh && session.canvas === fresh && session.mode === "live" && session.phase === "playing"
      && session.pointerInput && !session.pointerOwner?.closed && session.pointerSources?.size > 0;
    if (!current()) return;
    try {
      const preventDefault = event.preventDefault;
      if (!current()) return;
      if (typeof preventDefault !== "function") throw new Error("Pointer context menu cannot be suppressed.");
      preventDefault.call(event);
      if (!current()) return;
    } catch (error) {
      if (current()) void stopPlay(`Pointer input failed: ${String(error?.message ?? error).slice(0, 4096)}`, true);
    }
  }, { passive: false });
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
  if (settingsOperation || !initialized || preparing || !worker || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  const files = event.target.files;
  if (!files?.length) return;
  if (files.length > 32768) return status("Select no more than 32,768 files.", true);
  void releaseLocalSources("Song library changed. Discover local sources again.");
  importId = ++serial;
  seeking = false;
  clearRoomResults();
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
  if (settingsOperation || !worker || !hasPreview || preparing || importing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  try {
    const ns = nanoseconds(ui.position.value);
    seekId = ++serial;
    seeking = true;
    clearRoomResults();
    controls();
    worker.postMessage({ kind: "seek", id: seekId, selectedId, ns });
  } catch (error) { seeking = false; controls(); status(error.message, true); }
});
window.addEventListener("pagehide", stop);
window.addEventListener("pageshow", event => { if (event.persisted) start(); });
window.addEventListener("resize", resize);
ui.play.addEventListener("click", () => { void play("live"); });
ui["replay-play"].addEventListener("click", () => { void play("replay"); });
ui["settings-save"].addEventListener("click", () => requestSettings("settings-profile-save"));
ui["settings-load"].addEventListener("change", event => {
  if (settingsOperation || !settingsIdle()) return;
  try {
    const files = event.target.files;
    if (!files?.length) return;
    if (files.length !== 1) throw new Error("Choose one settings file.");
    requestSettings("settings-profile-load", files[0]);
  } catch (error) { settingsStatus(String(error?.message ?? error).slice(0, 4096), true); }
  finally { event.target.value = ""; }
});
ui["local-count"].addEventListener("change", () => {
  if (settingsOperation || activePlay) return;
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
ui["pointer-input"].addEventListener("change", () => {
  if (settingsOperation || activePlay || localSetup || localDiscovery || localCleanup) return;
  controls();
});
ui["local-release"].addEventListener("click", () => { if (!settingsOperation && !activePlay) void releaseLocalSources(); });
ui["local-page"].addEventListener("change", () => { void changeLocalPage(); });
ui["captured-replay"].addEventListener("change", () => {
  if (settingsOperation || activePlay || recordsOperation) return;
  lastReplay = capturedReplays.find(record => String(record.player ?? "solo") === ui["captured-replay"].value) ?? null;
  ui.export.textContent = lastReplay ? `Download ${lastReplay.player === undefined ? "last replay" : `player ${lastReplay.player} replay`} (${lastReplay.complete ? "complete" : "prefix"})` : "Choose a captured replay";
  controls();
});
function multiplayerSelectionChanged() {
  if (settingsOperation || activePlay) return;
  controls();
  ui["multiplayer-status"].textContent = ui.multiplayer.checked
    ? ui["multiplayer-mode"].value === "room"
      ? "Live Play opens a room lobby after audio preparation, with score progress, final acknowledgements and coordinated drain."
      : "Live Play will wait for the peer's compatible setup and committed start. Replay stays local."
    : "Solo play selected.";
}
ui.multiplayer.addEventListener("change", multiplayerSelectionChanged);
ui["multiplayer-mode"].addEventListener("change", multiplayerSelectionChanged);
for (const operation of ["seal", "ready", "leave"]) ui[`room-${operation}`].addEventListener("click", () => { void roomControl(operation); });
ui["room-score-prev"].addEventListener("click", () => { void changeRoomScorePage(-1); });
ui["room-score-next"].addEventListener("click", () => { void changeRoomScorePage(1); });
ui["replay-file"].addEventListener("change", event => {
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
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
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
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
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
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
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
  selectedGamepadProfile = null;
  ui["gamepad-profile"].value = "";
  ui["gamepad-profile-name"].textContent = "Automatic standard Gamepad bindings; choose an optional version 1 profile to customize.";
  controls();
});
ui.stop.addEventListener("click", () => { void stopPlay("Playback stopped."); });
ui["bindings-reset"].addEventListener("click", () => {
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
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
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed || !selectedReplay) return;
  try { addOpponent(selectedReplay, selectedReplayKey, opponentChoice()); }
  catch (error) { opponentStatus(String(error.message).slice(0, 4096), true); }
});
ui["opponents-clear"].addEventListener("click", () => {
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
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
  const mode = ui["multiplayer-mode"].value;
  const role = ui["multiplayer-role"].value;
  if (typeof raw !== "string" || raw.length === 0 || raw.length > 4096
    || !["peer", "room"].includes(mode) || (mode === "peer" && !["host", "join"].includes(role))) {
    throw new Error("Choose a multiplayer HTTPS server and connection mode.");
  }
  const url = new URL(raw);
  if (url.protocol !== "https:" || url.username || url.password || url.hash || url.href.length > 4096) {
    throw new Error("Multiplayer requires an HTTPS URL without credentials or a fragment.");
  }
  const windowOriginNs = millisecondsToNanos(performance.timeOrigin);
  if (mode === "room") {
    if (raw !== url.href || !url.hostname || url.port === "0" || raw.includes("?") || raw.includes("#")
      || !/^\/rooms\/[A-Za-z0-9_-]{1,1024}$/.test(url.pathname)) {
      throw new Error("Choose a canonical HTTPS /rooms/key URL without query, credentials or fragment.");
    }
    return { mode, config: { url: url.href, windowOriginNs } };
  }
  return { mode, config: { url: url.href, host: role === "host", windowOriginNs } };
}

function playRpc(session, kind, fields = {}, transfer = [], expectedSamples = null) {
  if (activePlay !== session || session.phase === "closing" || !worker) return Promise.reject(new Error("Playback owner is closed."));
  if (session.rpc) return Promise.reject(new Error("A playback setup operation is already pending."));
  if (kind === "play-samples-upload" && (!Number.isSafeInteger(expectedSamples)
    || expectedSamples < 0 || expectedSamples > PLAY_PCM_SAMPLES)) {
    return Promise.reject(new Error("Sample upload requires its actual prepared count."));
  }
  const rpcId = ++serial;
  if (!Number.isSafeInteger(rpcId)) return Promise.reject(new Error("Playback request identity exhausted."));
  return new Promise((resolve, reject) => {
    // Upload keeps this deadline until Worker proves actual producer admission.
    // Individual sample/EOS deadlines then bound the remaining operation.
    const timer = setTimeout(() => {
      if (session.rpc?.rpcId !== rpcId || session.rpc.admitted) return;
      session.rpc = null;
      if (activePlay === session && session.owner === owner) controls();
      reject(new Error("Playback Worker operation timed out."));
    }, 10000);
    session.rpc = { rpcId, kind, admitted: false, expectedSamples, timer, resolve, reject };
    controls();
    try { worker.postMessage({ kind, playId: session.id, rpcId, ...fields }, transfer); }
    catch (error) { clearTimeout(timer); session.rpc = null; controls(); reject(error); }
  });
}

function settleRoomStart(session, error, schedule) {
  const waiter = session.room?.waiter;
  if (!waiter) return;
  session.room.waiter = null;
  clearTimeout(waiter.timer);
  if (error) waiter.reject(error);
  else waiter.resolve(schedule);
}

function waitRoomStart(session) {
  const promise = new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      if (activePlay === session && session.owner === owner && session.phase !== "closing") {
        void stopPlay("Room lobby timed out after 60 seconds. Start a fresh session.", true);
      }
    }, 60000);
    session.room.waiter = { resolve, reject, timer };
  });
  // The start or failure may arrive while the open RPC is still pending.
  promise.catch(() => {});
  return promise;
}

function roomControl(operation) {
  const session = activePlay;
  const room = session?.room;
  if (!room || session.owner !== owner || session.mode !== "live" || session.phase !== "preparing"
    || !room.opened || room.leaving || room.start !== null || session.rpc || room.control) return;
  const own = room.snapshot?.members.find(member => member.participant === room.participant);
  if (operation === "seal" && (room.snapshot?.phase !== 0 || room.snapshot.members.length < 2
    || room.snapshot.members[0].participant !== room.participant || room.sealRequested)) return;
  if (operation === "ready" && (room.snapshot?.phase !== 1 || !own || own.prepared || room.readyRequested)) return;
  if (!["seal", "ready", "leave"].includes(operation)) return;
  if (operation === "leave") room.leaving = true;
  const pending = playRpc(session, `play-room-${operation}`);
  room.control = (async () => {
    try {
      let result;
      try { result = await pending; }
      catch (error) {
        if (activePlay === session && session.owner === owner && session.phase !== "closing") {
          if (error.playbackRefusal === true) ui["multiplayer-status"].textContent = `Room ${operation} refused: ${String(error.message).slice(0, 4096)}`;
          else void stopPlay(`Room ${operation} failed: ${String(error.message).slice(0, 4096)}`, true);
        }
        return;
      }
      if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
      if (operation === "leave" ? result?.kind !== "room-left" || result.leaveWritten !== true
        : result?.kind !== "room-requested" || result.operation !== operation) {
        void stopPlay("Room control response did not match its request.", true);
        return;
      }
      if (operation === "seal") room.sealRequested = true;
      if (operation === "ready") room.readyRequested = true;
    } finally {
      room.control = null;
      if (activePlay === session && session.owner === owner && session.phase !== "closing") {
        if (operation === "leave") await stopPlay("Left the room.");
        else controls();
      }
    }
  })();
  controls();
  return room.control;
}

async function changeRoomScorePage(delta) {
  if (settingsOperation) return;
  const session = activePlay;
  if (!session) {
    const results = roomResults;
    if (!results || results.owner !== owner || !worker || results.scoreFailed || results.rpc
      || results.scoreChanging || importing || preparing || recordsOperation || hidOwnershipFailed) return;
    const page = results.scorePage + delta;
    if (!Number.isInteger(page) || page < 0 || page >= results.scorePages) return;
    const rpcId = ++serial;
    if (!Number.isSafeInteger(rpcId)) return;
    results.scoreChanging = true;
    try {
      const response = await new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          if (results.rpc?.rpcId !== rpcId) return;
          results.rpc = null;
          reject(new Error("Room Results page request timed out."));
        }, 10000);
        results.rpc = { rpcId, timer, resolve, reject };
        controls();
        try { worker.postMessage({ kind: "play-room-page", playId: results.id, rpcId, page }); }
        catch (error) { clearTimeout(timer); results.rpc = null; reject(error); }
      });
      if (roomResults !== results || results.owner !== owner || activePlay) return;
      if (response?.kind !== "room-page" || response.page !== page || response.pages !== results.scorePages) {
        throw new Error("Room Results page response did not match its request.");
      }
      results.scorePage = page;
    } catch (error) {
      if (roomResults === results && results.owner === owner && !activePlay) {
        results.scoreFailed = true;
        ui["multiplayer-status"].textContent += ` Room Results display unavailable: ${String(error.message).slice(0, 4096)}`;
      }
    } finally {
      results.scoreChanging = false;
      if (roomResults === results && results.owner === owner) controls();
    }
    return;
  }
  const room = session?.room;
  if (!room || session.owner !== owner || session.phase === "closing" || room.leaving || room.scoreFailed
    || room.scoreChanging || session.rpc || !room.scorePages) return;
  const page = room.scorePage + delta;
  if (!Number.isInteger(page) || page < 0 || page >= room.scorePages) return;
  room.scoreChanging = true;
  try {
    const result = await playRpc(session, "play-room-page", { page });
    if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
    if (result?.kind !== "room-page" || result.page !== page || result.pages !== room.scorePages) {
      throw new Error("Room score page response did not match its request.");
    }
    room.scorePage = page;
  } catch (error) {
    if (activePlay === session && session.owner === owner && session.phase !== "closing") {
      room.scoreFailed = true;
      ui["multiplayer-status"].textContent = `Room score display unavailable: ${String(error.message).slice(0, 4096)} Local play continues.`;
    }
  } finally {
    room.scoreChanging = false;
    if (activePlay === session && session.owner === owner) controls();
  }
}

async function changeLocalPage() {
  const session = activePlay;
  if (!session) return;
  if (!session.localPlan || session.localPlan.automatic === true || session.phase !== "playing" || session.pageChanging || session.rpc) {
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
    await drainPageInput(session);
    if (activePlay !== session || session.owner !== owner || session.phase !== "playing") return;
    const result = await playRpc(session, "play-page", { page });
    if (activePlay !== session || session.owner !== owner || session.phase !== "playing") return;
    const touchSlot = session.localPlan.sources.indexOf(2n);
    const touchVisible = touchSlot >= 0 && Math.floor(touchSlot / 4) === page;
    if (result?.kind !== "local-page" || result.page !== page
      || (touchSlot >= 0 ? result.touchVisible !== touchVisible : result.touchVisible !== undefined)) {
      void stopPlay("Local page response changed its requested identity.", true);
      return;
    }
    session.localPage = page;
    ui["local-status"].textContent = `Showing players ${session.localPlan.players.slice(page * 4, page * 4 + 4).join(", ")}.`
      + (touchSlot >= 0 && !touchVisible ? " New touch contacts are unbound while the touch player is offscreen; held contacts retain their lane." : "");
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
  if (settingsOperation || !initialized || !hasPreview || !audioModule || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed || localDiscovery || localCleanup) return;
  if (mode === "replay" && selectedReplay === null) return;
  if (mode === "live" && localRoster.players.length === 1) {
    try { validateOpponentTargets(opponents.snapshot()); }
    catch (error) { status(String(error.message).slice(0, 4096), true); return; }
  }
  let retained = null;
  let localPlan = null;
  let releasedSources = null;
  if (mode === "live" && localRoster.players.length > 1) {
    try {
      validateOpponentTargets(opponents.snapshot(), localRoster.players);
      retained = localSetup;
      if (!retained || retained.phase !== "ready" || retained.owner !== owner) throw new Error("Discover and assign local input sources before playing.");
      if (retained.touchInput !== (ui["touch-input"].checked === true)
        || retained.pointerInput !== (ui["pointer-input"].checked === true)
        || (retained.pointerInput && !samePointerChoices(retained.pointerSelection, snapshotPointerChoices()))
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
  } else {
    if (mode === "live" && ui.multiplayer.checked === true) localPlan = localRoster.snapshot([], 0, true);
    if (localSetup) releasedSources = releaseLocalSources();
  }
  const acquired = retained ? { sequence: retained.sequence, nextSource: retained.nextSource,
    gamepadOwner: retained.gamepadOwner, gamepadDevices: retained.gamepadDevices,
    pointerOwner: retained.pointerOwner, pointerDevices: retained.pointerDevices, pointerSelection: retained.pointerSelection,
    hidOwner: retained.hidOwner, hidDevices: retained.hidDevices } : {};
  const session = Object.assign(retained ?? {}, { id: ++serial, owner, mode, phase: "preparing", controller: new AbortController(), audio: null, opening: null,
    rpc: null, timer: null, events: [], pressed: new Set(), bindings: [], sequence: 0n, nextSource: 3n, inputPumping: false,
    canvas, touchInput: false, contacts: new Map(), nextContact: 0n,
    hidOwner: null, hidConnecting: null, hidDevices: null, hidSources: null, hidProfileFile: null,
    gamepadOwner: null, gamepadDevices: null, gamepadSources: null, gamepadProfileFile: null,
    pointerInput: false, pointerOwner: null, pointerDevices: null, pointerSources: null, pointerSelection: null, pointerSequence: null,
    tickId: 0, tickPending: null, commandsPending: true, startFrame: null,
    origin: null, lastHost: 0n, stopping: null, renderId: 0, renderPending: null,
    workerStarted: false, workerReleased: false, workerStop: null, finalScore: null,
    completionReady: false, completionTick: null, cleanupError: null, peerDisplayFailed: false, peerDisplayFailures: new Set(),
    recordReplay: mode === "live" && ui.record.checked === true, replay: null, replayError: null, naturalFinishRequested: false,
    replayFile: mode === "replay" ? selectedReplay : null,
    opponentSelection: mode === "live" && opponents.size ? opponents.snapshot() : null,
    opponentCount: mode === "live" ? opponents.size : 0, opponentsFailed: false, opponentError: null,
    chartPath: ui.chart.value,
    preview: { title: ui.title.textContent, details: ui.details.textContent, position: ui.position.value } }, acquired,
    { localPlan, localSources: localPlan && localPlan.automatic !== true ? new Set(localPlan.sources) : null, localReplays: null, localScores: null, recordLimits: null,
      localPage: localPlan?.page ?? 0, pageChanging: false, pageInputWaiter: null, lastAckSequence: 0n });
  clearRoomResults();
  activePlay = session;
  controls();
  status(mode === "replay" ? "Preparing recorded replay and audio…" : "Preparing playable chart and audio…");
  try {
    if (session.localPlan?.automatic === true && session.opponentSelection) {
      session.opponentSelection = Object.freeze(session.opponentSelection.map(entry =>
        Object.freeze({ ...entry, player: session.localPlan.players[0] })));
    }
    session.touchInput = mode === "live" && ui["touch-input"].checked === true;
    session.pointerInput = mode === "live" && ui["pointer-input"].checked === true;
    if (session.pointerInput) {
      if (typeof window.PointerEvent !== "function") throw new Error("Pointer input is unavailable.");
      if (!retained) session.pointerSelection = snapshotPointerChoices();
    }
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
    const connection = mode === "live" && ui.multiplayer.checked === true ? multiplayerConfiguration() : null;
    session.multiplayer = connection?.mode === "peer" ? connection.config : null;
    session.room = connection?.mode === "room" ? { ...connection.config, opened: false, opening: false, closed: false,
      participant: null, snapshot: null, start: null, waiter: null, control: null,
      sealRequested: false, readyRequested: false, leaving: false,
      scorePages: 0, scorePage: 0, scoreFailed: false, scoreChanging: false } : null;
    controls();
    ui["multiplayer-status"].textContent = session.multiplayer || session.room ? "Preparing local audio before connecting…"
      : mode === "replay" ? "Local replay · no multiplayer connection."
        : session.localPlan ? "Local players · no network connection." : "Solo play selected.";
    clearOpponentResults(session.opponentSelection ? "Preparing selected saved opponents…"
      : mode === "replay" ? "Saved comparisons are inactive during replay playback." : "No saved opponents selected.");
    if (!retained && session.hidProfileFile !== null) session.hidOwner = createSessionHid(session);
    if (!retained && session.pointerInput) {
      session.pointerOwner = createSessionPointers(session);
      if (!inputOwnerCurrent(session)) { session.pointerOwner.close(); return; }
      session.pointerDevices = session.pointerOwner.devices;
    }
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
    if (session.localSources) {
      session.hidDevices = session.hidDevices?.filter(device => session.localSources.has(device.source)) ?? null;
      session.gamepadDevices = session.gamepadDevices?.filter(device => session.localSources.has(device.source)) ?? null;
      session.pointerDevices = session.pointerDevices === null ? null
        : Object.freeze(session.pointerDevices.filter(device => session.localSources.has(device.source)));
    }
    session.pointerSetup = pointerSetupFor(session);
    session.requestHid = session.hidOwner !== null && (!session.localSources || session.hidDevices?.length > 0);
    session.requestGamepad = session.gamepadOwner !== null && (!session.localSources || session.gamepadDevices?.length > 0);
    if (session.localSources && !session.requestHid) session.hidSources = new Set();
    if (session.localSources && !session.requestGamepad) session.gamepadSources = new Set();
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
        ...(session.pointerSetup ? { pointerSetup: session.pointerSetup } : {}),
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
        || (session.localSources && count !== session.hidDevices.length)) {
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
      const eligible = custom || session.localSources ? session.gamepadDevices : session.gamepadDevices.filter(device => device.mapping === "standard" && device.buttons >= 9);
      if (!Array.isArray(sources) || (session.localSources ? sources.length !== eligible.length
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
    if (session.pointerSetup) {
      const devices = prepared.pointerDevices;
      const expected = session.pointerSetup.devices;
      const owned = session.pointerOwner.devices;
      if (session.pointerOwner.closed || owned.length !== 2 || !Array.isArray(devices) || devices.length !== expected.length) {
        throw new Error("Preparation omitted the exact owned pointer aggregates.");
      }
      const admitted = new Map();
      for (let index = 0; index < devices.length; index++) {
        const device = devices[index];
        if (!device || device.source !== expected[index].source || device.pointerType !== expected[index].pointerType
          || !owned.some(row => row.source === device.source && row.pointerType === device.pointerType)
          || admitted.has(device.source) || session.hidSources?.has(device.source) || session.gamepadSources?.has(device.source)) {
          throw new Error("Preparation changed an owned pointer source identity.");
        }
        admitted.set(device.source, { pointerType: device.pointerType,
          controls: new Set(session.pointerSelection.filter(row => row[1] === device.pointerType).map(row => row[2])) });
      }
      session.pointerSources = admitted;
    } else if (prepared.pointerDevices !== undefined) throw new Error("Preparation admitted pointer input without requested ownership.");
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
    session.opponentTargets = session.opponentSelection ?? [];
    session.opponentSelection = null;
    ui.title.textContent = prepared.title || ui.chart.value;
    ui.details.textContent = `${prepared.artist || "Unknown artist"} · ${prepared.notes} notes · ${prepared.samples} sounds · ${session.audio.sampleRate} Hz output · start ${seconds(preparedStart.toString())} s`
      + (session.endNs === undefined ? "" : ` · ${mode === "replay" ? "recorded end" : "end"} ${seconds(session.endNs.toString())} s`)
      + (session.localPlan ? ` · ${session.localPlan.players.length} local players` : "");
    if (session.localPlan || session.hidOwner !== null || session.gamepadSources?.size > 0 || session.pointerSources?.size > 0) {
      bindingsFor(prepared.lanes); // Validate actual lane shape; Worker proved combined coverage.
      session.bindings = session.localSources && !session.localSources.has(1n) ? []
        : bindingsFor(prepared.lanes.filter(lane => session.bindingSelection.some(row => row[0] === lane)), session.bindingSelection);
    } else session.bindings = mode === "replay" ? [] : bindingsFor(prepared.lanes, session.bindingSelection);
    ui.keys.textContent = mode === "replay" ? "Recorded input playback · Escape stops the replay."
      : session.bindings.map(row => `${row[0].toString(16).toUpperCase()}: ${row[1]}`).join(" · ")
        + (session.touchInput && (!session.localSources || session.localSources.has(2n)) ? " · Touch lanes enabled" : "")
        + (session.hidSources ? ` · ${session.hidSources.size} HID interface(s)` : "")
        + (session.pointerSources ? ` · ${session.pointerSources.size} Window mouse/pen aggregate source(s)` : "")
        + (session.gamepadSources ? ` · ${session.gamepadSources.size} ${session.gamepadProfileFile ? "profile-configured" : "automatic standard"} Gamepad(s); ${session.gamepadDevices.length - session.gamepadSources.size} unmatched device(s) ignored` : "");
    const preparedSamples = prepared.samples;
    if (!Number.isSafeInteger(preparedSamples) || preparedSamples < 0 || preparedSamples > PLAY_PCM_SAMPLES) {
      throw new Error("Prepared audio asset count exceeds the bounded section capacity.");
    }
    const sampleDescriptor = await session.audio.openSamplePort();
    try {
      if (activePlay !== session || session.owner !== owner || session.phase === "closing") {
        sampleDescriptor?.port?.close();
        return;
      }
      const limits = sampleDescriptor?.pcmLimits;
      if (!sampleDescriptor?.port
        || !["postMessage", "start", "close"].every(name => typeof sampleDescriptor.port[name] === "function")
        || sampleDescriptor.generation !== session.id || sampleDescriptor.channels !== session.audio.channels
        || sampleDescriptor.channels !== 2 || limits?.maxAssetBytes !== 64 * 1024 * 1024
        || limits?.maxTotalBytes !== 256 * 1024 * 1024 || limits?.maxSamples !== PLAY_PCM_SAMPLES
        || !Number.isSafeInteger(sampleDescriptor.timeoutMs) || sampleDescriptor.timeoutMs < 1 || sampleDescriptor.timeoutMs > 60000) {
        throw new Error("Audio sample handoff did not preserve its owner configuration.");
      }
      const uploaded = await playRpc(session, "play-samples-upload", sampleDescriptor, [sampleDescriptor.port], preparedSamples);
      if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
      if (uploaded?.kind !== "samples-uploaded" || uploaded.count !== preparedSamples
        || !Number.isSafeInteger(uploaded.bytes) || uploaded.bytes < 0
        || uploaded.bytes > limits.maxTotalBytes || uploaded.bytes % (2 * 4) !== 0
        || uploaded.bytes > uploaded.count * limits.maxAssetBytes) {
        throw new Error("Direct audio upload did not acknowledge the exact bounded prepared bank.");
      }
    } catch (error) {
      try { sampleDescriptor?.port?.close(); } catch {}
      throw error;
    }
    if (session.endFrame === undefined) await session.audio.finish();
    else await session.audio.finish(session.endFrame);
    if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
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
    if (session.multiplayer || session.room) {
      let schedule;
      if (session.room) {
        const room = session.room;
        const start = waitRoomStart(session);
        room.opening = true;
        ui["multiplayer-status"].textContent = "Audio ready · opening the room lobby…";
        const opened = await playRpc(session, "play-room-open", { url: room.url, windowOriginNs: room.windowOriginNs });
        if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
        if (opened?.kind !== "room-opened") throw new Error("Room opening was not acknowledged.");
        room.opened = true;
        controls();
        schedule = await start;
        // A genuine start may precede the queued Ready response. Finish that
        // existing RPC before issuing activation on the same control lane.
        await room.control;
      } else {
        ui["multiplayer-status"].textContent = "Audio ready · waiting for the peer and committed start…";
        schedule = await playRpc(session, "play-network-ready");
      }
      if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
      if ((!session.room && schedule?.kind !== "multiplayer-start") || typeof schedule?.targetHostNs !== "bigint"
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
    if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
    await playRpc(session, "play-activate", { hostNs: session.origin, startFrame: session.startFrame,
      ...(session.multiplayer || session.room ? { targetHostNs: session.targetHostNs } : {}) });
    if (activePlay !== session || session.owner !== owner || session.phase === "closing") return;
    if (millisecondsToNanos(performance.now()) >= session.origin) throw new Error("Playback activation missed its chosen start. Start a fresh session.");
    session.phase = "playing";
    if (session.room) ui["multiplayer-status"].textContent = "Room software start activated · reported scores appear below the playfields. Coordinated final drain is enabled.";
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
  try {
    if (!lost && event.pointerType !== "touch") return;
    const id = event.pointerId;
    const previous = session.contacts.get(id);
    if (phase === 0 && session.pageChanging && !previous) return;
    if ((phase === 0 && previous) || (phase !== 0 && !previous)) return;
    const owned = () => activePlay === session && session.phase === "playing" && session.owner === owner
      && surface === canvas && surface === session.canvas;
    const current = () => owned() && session.contacts.get(id) === previous;
    if (!current()) return;
    event.preventDefault();
    if (!current()) return;
    if (!Number.isInteger(id) || id < -2147483648 || id > 2147483647) throw new Error("Touch pointer identity exceeds signed 32 bits.");
    if (session.events.length >= 1024) throw new Error("Pending input capacity exceeded.");
    if (phase === 0 && session.contacts.size >= 256) throw new Error("Touch contact capacity exceeded.");
    const timeStamp = event.timeStamp;
    const hostNs = millisecondsToNanos(timeStamp);
    if (hostNs < session.lastHost) throw new Error("Touch input arrived behind the accepted gameplay watermark.");
    const [cssWidth, cssHeight] = cssExtent;
    const [backingWidth, backingHeight] = surfaceExtent;
    const geometryAvailable = Number.isFinite(cssWidth) && cssWidth > 0 && Number.isFinite(cssHeight) && cssHeight > 0
      && Number.isInteger(backingWidth) && backingWidth > 0 && backingWidth <= 0xffffffff
      && Number.isInteger(backingHeight) && backingHeight > 0 && backingHeight <= 0xffffffff;
    const width = lost && !geometryAvailable ? previous.width : cssWidth;
    const height = lost && !geometryAvailable ? previous.height : cssHeight;
    const surfaceWidth = lost && !geometryAvailable ? previous.surfaceWidth : backingWidth;
    const surfaceHeight = lost && !geometryAvailable ? previous.surfaceHeight : backingHeight;
    if (!Number.isFinite(width) || width <= 0 || !Number.isFinite(height) || height <= 0
      || !Number.isInteger(surfaceWidth) || surfaceWidth <= 0 || surfaceWidth > 0xffffffff
      || !Number.isInteger(surfaceHeight) || surfaceHeight <= 0 || surfaceHeight > 0xffffffff) {
      throw new Error("Touch input requires finite coordinates, pressure and positive CSS and backing extents.");
    }
    if (phase === 1) {
      const coalesced = event.getCoalescedEvents;
      if (!current()) return;
      if (coalesced != null && typeof coalesced !== "function") throw new Error("Invalid coalesced touch acquisition method.");
      const samples = coalesced == null ? [] : coalesced.call(event);
      if (!current()) return;
      const count = Array.isArray(samples) ? samples.length : -1;
      if (!current()) return;
      if (!Number.isInteger(count) || count < 0 || count > 256) throw new Error("Coalesced touch sample capacity exceeded or malformed list.");
      if (count > 0) {
        if (count > 1024 - session.events.length) throw new Error("Pending input capacity exceeded.");
        const offsetX = event.offsetX, offsetY = event.offsetY;
        const clientX = event.clientX, clientY = event.clientY;
        const primary = event.isPrimary;
        if (!current()) return;
        if (!finiteTouchSample(offsetX) || !finiteTouchSample(offsetY)
          || !finiteTouchSample(clientX) || !finiteTouchSample(clientY) || typeof primary !== "boolean") {
          throw new Error("Coalesced touch input requires a finite dispatched coordinate anchor.");
        }
        const snapshots = [];
        let previousTime = -Infinity;
        for (let index = 0; index < count; index++) {
          const sample = samples[index];
          const pointerId = sample?.pointerId, pointerType = sample?.pointerType, isPrimary = sample?.isPrimary;
          const sampleTime = sample?.timeStamp, sampleX = sample?.clientX, sampleY = sample?.clientY;
          const pressure = sample?.pressure;
          if (!current()) return;
          if (pointerId !== id || pointerType !== "touch" || isPrimary !== primary
            || !finiteTouchSample(sampleX) || !finiteTouchSample(sampleY) || !finiteTouchSample(pressure)) {
            throw new Error("Coalesced touch input changed its pointer identity or finite sample fields.");
          }
          const sampleNs = millisecondsToNanos(sampleTime);
          if (sampleTime < previousTime || sampleTime > timeStamp || sampleNs < session.lastHost) {
            throw new Error("Coalesced touch input has invalid acquisition chronology.");
          }
          // Children were not dispatched on canvas: use the parent's CSS anchor.
          const x = offsetX + (sampleX - clientX), y = offsetY + (sampleY - clientY);
          if (!finiteTouchSample(x) || !finiteTouchSample(y)) throw new Error("Coalesced touch coordinates exceed the physical input range.");
          snapshots.push(Object.freeze({ hostNs: sampleNs, x, y, pressure, width, height, surfaceWidth, surfaceHeight }));
          previousTime = sampleTime;
        }
        Object.freeze(snapshots);
        if (!current()) return;
        if (count > 1024 - session.events.length || snapshots[0].hostNs < session.lastHost) {
          throw new Error("Coalesced touch prefix no longer fits the current input frontier.");
        }
        const sequence = session.sequence;
        const lastSequence = sequence + BigInt(count);
        if (lastSequence > 18446744073709551615n) throw new Error("Input acquisition sequence exhausted.");
        const contact = previous.contact;
        const batch = snapshots.map((sample, index) => Object.freeze({ ...sample,
          kind: "touch", sequence: sequence + BigInt(index + 1), contact, phase, code: id >>> 0 }));
        const last = snapshots[count - 1];
        session.sequence = lastSequence;
        session.events.push(...batch);
        session.contacts.set(id, Object.freeze({ contact, x: last.x, y: last.y, pressure: last.pressure,
          width, height, surfaceWidth, surfaceHeight }));
        session.completionReady = false;
        pumpInput(session);
        return;
      }
    }
    const offsetX = event.offsetX, offsetY = event.offsetY, sampledPressure = event.pressure;
    if (!current()) return;
    const x = lost && !finiteTouchSample(offsetX) ? previous.x : offsetX;
    const y = lost && !finiteTouchSample(offsetY) ? previous.y : offsetY;
    const pressure = lost && !finiteTouchSample(sampledPressure) ? previous.pressure : sampledPressure;
    if (!finiteTouchSample(x) || !finiteTouchSample(y) || !finiteTouchSample(pressure)) {
      throw new Error("Touch input requires finite coordinates and pressure.");
    }
    const contact = previous?.contact ?? session.nextContact + 1n;
    if (contact > 18446744073709551615n) throw new Error("Touch acquisition identity exhausted.");
    const sample = { contact, x, y, pressure, width, height, surfaceWidth, surfaceHeight };
    if (phase === 0) {
      // Capture belongs to this contact before any event can reach the Worker.
      surface.setPointerCapture(id);
      if (!current()) { try { surface.releasePointerCapture(id); } catch {} return; }
      session.contacts.set(id, sample);
      session.nextContact = contact;
    } else if (phase === 2 || phase === 3) {
      session.contacts.delete(id);
      // Native release may emit lost capture; the removed owner cannot cancel twice.
      if (!lost) surface.releasePointerCapture(id);
      if (!owned()) return;
    } else session.contacts.set(id, sample);
    const sequence = nextInputSequence(session);
    session.events.push({ kind: "touch", hostNs, sequence, contact, phase, code: id >>> 0,
      x, y, pressure, width, height, surfaceWidth, surfaceHeight });
    session.completionReady = false;
    pumpInput(session);
  } catch (error) {
    if (activePlay === session && session.owner === owner && session.phase === "playing") {
      void stopPlay(`Playback failed: ${String(error?.message ?? error).slice(0, 4096)}`, true);
    }
  }
}

function releaseTouches(session) {
  if (session.canvas === canvas && (!activePlay || activePlay === session)) delete session.canvas.dataset.touchInput;
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
    && !session.commandsPending && !session.pageChanging && session.completionTick === session.tickId) {
    void stopPlay(session.mode === "replay" ? "Recorded replay ended."
      : session.endNs === undefined ? "Song completed." : "Section completed.", false, true);
  }
}
function settlePageInput(session) {
  const waiter = session.pageInputWaiter;
  if (!waiter || session.lastAckSequence < waiter.boundary) return;
  session.pageInputWaiter = null;
  clearTimeout(waiter.timer);
  waiter.resolve();
}

function drainPageInput(session) {
  let boundary = session.tickPending?.lastSequence ?? session.lastAckSequence;
  for (const event of session.events) if (event.sequence > boundary) boundary = event.sequence;
  if (session.lastAckSequence >= boundary) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const waiter = { boundary, resolve, reject, timer: null };
    waiter.timer = setTimeout(() => {
      if (session.pageInputWaiter !== waiter) return;
      session.pageInputWaiter = null;
      reject(new Error("Acquired input prefix did not finish before page change."));
    }, 10000);
    session.pageInputWaiter = waiter;
    pumpInput(session);
  });
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
    let lastSequence = session.lastAckSequence;
    for (const event of events) if (event.sequence > lastSequence) lastSequence = event.sequence;
    session.tickPending = { tickId, timer, watermark, lastInput, lastSequence };
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

function receiveRoom(session, event) {
  const room = session.room;
  if (!room || session.owner !== owner || session.phase === "closing" || room.leaving
    || (room.closed && event?.kind !== "display-unavailable")) return;
  try {
    if (!room.opening || !event || typeof event !== "object" || Array.isArray(event)) throw new Error("Invalid room event.");
    if (event.kind === "snapshot") {
      const participant = event.participant;
      const snapshot = event.snapshot;
      if (typeof participant !== "bigint" || participant < 1n || participant > 18446744073709551615n
        || (room.participant !== null && participant !== room.participant)
        || !snapshot || typeof snapshot !== "object" || Array.isArray(snapshot)
        || !Number.isInteger(snapshot.phase) || snapshot.phase < 0 || snapshot.phase > 2
        || (snapshot.phase === 2 ? snapshot.deadlineNs !== null
          : typeof snapshot.deadlineNs !== "bigint" || snapshot.deadlineNs < 0n || snapshot.deadlineNs > 9223372036854775807n)
        || !Array.isArray(snapshot.members) || snapshot.members.length < 1 || snapshot.members.length > 64) {
        throw new Error("Invalid room roster metadata.");
      }
      const seen = new Set();
      const members = [];
      for (const member of snapshot.members) {
        if (!member || typeof member !== "object" || Array.isArray(member)
          || typeof member.participant !== "bigint" || member.participant < 1n || member.participant > 18446744073709551615n
          || seen.has(member.participant) || typeof member.prepared !== "boolean"
          || !(member.players instanceof Uint32Array) || !(member.players.buffer instanceof ArrayBuffer)
          || member.players.buffer.resizable === true || member.players.buffer.byteLength > 256
          || member.players.length < 1 || member.players.length > 64 || member.players.byteLength !== member.players.length * 4) {
          throw new Error("Invalid room host or local player roster.");
        }
        const players = Array.from(member.players);
        if (players.some(player => player === 0) || new Set(players).size !== players.length) throw new Error("Invalid room local player identities.");
        seen.add(member.participant);
        members.push(Object.freeze({ participant: member.participant, prepared: member.prepared, players: Object.freeze(players) }));
      }
      const own = members.find(member => member.participant === participant);
      if (!own || own.players.length !== session.localPlan.players.length
        || own.players.some((player, index) => player !== session.localPlan.players[index])
        || (snapshot.phase === 0 ? members.some(member => member.prepared)
          : members.length < 2 || (snapshot.phase === 1 ? members.every(member => member.prepared) : members.some(member => !member.prepared)))) {
        throw new Error("Room roster does not match the prepared local players or phase.");
      }
      room.participant = participant;
      room.snapshot = Object.freeze({ phase: snapshot.phase, deadlineNs: snapshot.deadlineNs, members: Object.freeze(members) });
      const phase = ["Collecting hosts", "Roster sealed · select Ready for this host", "All hosts prepared · agreeing on the software start"][snapshot.phase];
      ui["multiplayer-status"].textContent = `${phase}. You are host ${participant}. `
        + members.map(member => `Host ${member.participant}: players ${member.players.join(", ")} · ${member.prepared ? "ready" : "not ready"}`).join("; ");
      controls();
    } else if (event.kind === "score-pages") {
      const remote = room.snapshot?.members.filter(member => member.participant !== room.participant)
        .reduce((sum, member) => sum + member.players.length, 0);
      if (room.snapshot?.phase !== 2 || room.scorePages !== 0 || event.page !== 0
        || !Number.isInteger(event.pages) || event.pages < 1 || event.pages > 1008
        || event.pages !== Math.ceil(remote / 4)) throw new Error("Invalid room score page metadata.");
      room.scorePage = 0; room.scorePages = event.pages;
      controls();
    } else if (event.kind === "display-unavailable") {
      if (typeof event.error !== "string" || event.error.length < 1 || event.error.length > 4096) throw new Error("Invalid room display failure notice.");
      if (!room.scoreFailed) {
        room.scoreFailed = true;
        ui["multiplayer-status"].textContent = `Room score display unavailable: ${event.error} Local play continues.`;
        controls();
      }
    } else if (event.kind === "start") {
      if (room.start !== null || !room.waiter || session.phase !== "preparing" || room.snapshot?.phase !== 2
        || typeof event.targetHostNs !== "bigint" || event.targetHostNs < 0n || event.targetHostNs > 9223372036854775807n
        || typeof event.songTargetHostNs !== "bigint" || event.songTargetHostNs < 0n || event.songTargetHostNs > 9223372036854775807n
        || event.songTargetHostNs - event.targetHostNs !== 100000000n
        || typeof event.uncertaintyNs !== "bigint" || event.uncertaintyNs < 0n || event.uncertaintyNs > 100000000n) {
        throw new Error("Invalid or repeated committed room start.");
      }
      room.start = Object.freeze({ targetHostNs: event.targetHostNs, songTargetHostNs: event.songTargetHostNs, uncertaintyNs: event.uncertaintyNs });
      settleRoomStart(session, null, room.start);
      ui["multiplayer-status"].textContent = "Shared room software start committed · preparing output…";
      controls();
    } else if (event.kind === "closed") {
      if (typeof event.error !== "string" || event.error.length < 1 || event.error.length > 4096) throw new Error("Invalid room closure notice.");
      if (session.phase === "playing") {
        room.closed = true;
        room.error = event.error;
        ui["multiplayer-status"].textContent = `Room disconnected: ${event.error} · local play continues.`;
        controls();
        return;
      }
      throw new Error(`Room closed: ${event.error}`);
    } else throw new Error("Unknown room event.");
  } catch (error) {
    if (event?.kind === "score-pages" || event?.kind === "display-unavailable") {
      room.scoreFailed = true;
      ui["multiplayer-status"].textContent = `Room score display unavailable: ${String(error.message).slice(0, 4096)} Local play continues.`;
      controls();
      return;
    }
    room.error = String(error.message).slice(0, 4096);
    void stopPlay(room.error, true);
  }
}

function receiveMultiplayer(session, event) {
  if (!session.multiplayer || session.phase === "closing" || session.owner !== owner || !event) return;
  const field = ui["multiplayer-status"];
  if (["progress", "final-progress", "roster", "group-progress", "group-final-progress"].includes(event.kind)) return;
  if (event.kind === "peer-display-unavailable") {
    let label = "Peer";
    if (session.localPlan) {
      if (!Number.isInteger(event.player) || event.player < 1 || event.player > 0xffffffff
        || !session.localPlan.players.includes(event.player) || session.peerDisplayFailures.has(event.player)) return;
      session.peerDisplayFailures.add(event.player);
      label = `Player ${event.player} peer`;
    } else {
      if (session.peerDisplayFailed) return;
      session.peerDisplayFailed = true;
    }
    const reason = typeof event.error === "string" && event.error.length > 0 && event.error.length <= 4096
      ? event.error : "Invalid peer presentation failure notice.";
    field.textContent = `${label} display unavailable: ${reason} Local play and multiplayer transport continue.`;
  } else if (event.kind === "disconnected") {
    field.textContent = `Multiplayer disconnected: ${String(event.error ?? "Connection lost").slice(0, 4096)}${session.phase === "playing" ? " · local play continues." : "."}`;
  } else if (event.kind === "connected") field.textContent = "Connected · checking compatible setup and readiness…";
  else if (event.kind === "ready") field.textContent = "Peer ready · agreeing on the start…";
  else if (event.kind === "start") field.textContent = "Shared software start committed · preparing output…";
  else if (event.kind === "final-acknowledged") field.textContent = "Peer acknowledged the final score prefix.";
}

function checkedPeerText(peer) {
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
}

function finalPeerText(peer) {
  try {
    return checkedPeerText(peer);
  } catch (error) {
    return ` ${String(error.message).slice(0, 4096)} Local result unchanged.`;
  }
}

function finalLocalPeerText(peers, players) {
  try {
    if (!Array.isArray(peers) || peers.length < 1 || peers.length > 64 || peers.length !== players.length) {
      throw new Error("Invalid local peer roster.");
    }
    for (let index = 0; index < players.length; index++) {
      const peer = peers[index];
      if (!peer || typeof peer !== "object" || Array.isArray(peer)
        || !Number.isInteger(peer.player) || peer.player < 1 || peer.player > 0xffffffff
        || peer.player !== players[index]
        || !(peer.remotePlayer === null || (Number.isInteger(peer.remotePlayer) && peer.remotePlayer >= 1 && peer.remotePlayer <= 0xffffffff))
        || (peer.remotePlayer === null && (peer.progress !== null || peer.final !== false))) {
        throw new Error("Invalid local peer assignment.");
      }
    }
    // Build the entire result before its single DOM assignment. A malformed
    // sibling never publishes an apparently accepted partial group summary.
    return peers.map(peer => ` Player ${peer.player} · ${peer.remotePlayer === null
      ? "unassigned remote player" : `remote Player ${peer.remotePlayer}`}.${checkedPeerText(peer)}`).join("");
  } catch {
    return " Final local peer summaries unavailable or malformed. Local result unchanged.";
  }
}

function receivePlay(data) {
  const session = activePlay;
  const results = roomResults;
  if (!session && results?.id === data.playId && results.owner === owner) {
    if (data.kind === "play-reply" && results.rpc?.rpcId === data.rpcId) {
      const request = results.rpc;
      results.rpc = null;
      clearTimeout(request.timer);
      if (typeof data.error === "string") request.reject(new Error(data.error));
      else request.resolve(data.result);
    } else if (data.kind === "play-room-results") {
      try {
        const metadata = roomResultsMetadata(data);
        if (!metadata.scoreFailed || metadata.scorePages !== results.scorePages) throw new Error("Invalid Results failure notice.");
        Object.assign(results, metadata);
      } catch { results.scoreFailed = true; }
      if (results.rpc) {
        const request = results.rpc;
        results.rpc = null;
        clearTimeout(request.timer);
        request.reject(new Error("Room Results display unavailable."));
      }
      controls();
    }
    return;
  }
  if (!session || data.playId !== session.id) return;
  if (data.kind === "play-room-results") {
    try {
      roomResultsMetadata(data);
      if (data.failed !== true) throw new Error("Invalid Results failure notice.");
      session.roomResultsNotice = { page: data.page, pages: data.pages, failed: true };
    } catch { session.roomResultsNotice = { page: 0, pages: 0, failed: true }; }
  } else if (data.kind === "play-opponents") {
    receiveOpponents(session, data);
  } else if (data.kind === "play-multiplayer") {
    receiveMultiplayer(session, data.event);
  } else if (data.kind === "play-room") {
    receiveRoom(session, data.event);
  } else if (data.kind === "play-samples-admitted") {
    const request = session.rpc;
    if (!request || data.rpcId !== request.rpcId) return;
    if (request.kind !== "play-samples-upload" || request.admitted
      || !Number.isSafeInteger(data.count) || data.count !== request.expectedSamples) {
      session.rpc = null;
      clearTimeout(request.timer);
      if (session.owner === owner) controls();
      request.reject(new Error("Sample upload admission was malformed or repeated."));
      return;
    }
    request.admitted = true;
    clearTimeout(request.timer);
    request.timer = null;
  } else if (data.kind === "play-reply") {
    const request = session.rpc;
    if (!request || data.rpcId !== request.rpcId) return;
    session.rpc = null;
    clearTimeout(request.timer);
    if (session.owner === owner) controls();
    if (typeof data.error === "string") request.reject(Object.assign(new Error(data.error), { playbackRefusal: true }));
    else if (request.kind === "play-samples-upload" && !request.admitted) {
      request.reject(new Error("Sample upload completed before producer admission."));
    } else request.resolve(data.result);
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
    session.lastAckSequence = pending.lastSequence;
    settlePageInput(session);
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
  if (session.stopping) {
    if (!completed && session.room && session.naturalFinishRequested && !session.workerReleased && !session.room.drainCancelled) {
      session.room.drainCancelled = true;
      try { worker?.postMessage({ kind: "play-stop", playId: session.id, completed: false }); } catch {}
    }
    return session.stopping;
  }
  const hadHid = session.hidOwner !== null;
  session.naturalFinishRequested = completed;
  session.phase = "closing";
  settleRoomStart(session, new Error("Room start wait cancelled."));
  let pointersStopped;
  try {
    session.pointerOwner?.close();
    if (session.pointerOwner?.cleanupFailure) throw session.pointerOwner.cleanupFailure;
    pointersStopped = Promise.resolve();
  } catch (error) { pointersStopped = Promise.reject(error); }
  pointersStopped.catch(() => {});
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
  if (session.pageInputWaiter) {
    clearTimeout(session.pageInputWaiter.timer);
    session.pageInputWaiter.reject(new Error("Page input admission was cancelled."));
    session.pageInputWaiter = null;
  }
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
      }, completed && session.room ? 20000 : 10000);
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
        pointersStopped.catch(error => {
          hidOwnershipFailed = true;
          stop();
          failed = true;
          reason += ` Pointer cleanup failed: ${String(error.message).slice(0, 4096)} Reload the page before playing again.`;
        }),
      ]);
    }
    finally {
      await workerStopped;
      session.hidOwner = session.hidConnecting = session.hidDevices = session.hidSources = session.hidProfileFile = null;
      session.gamepadOwner = session.gamepadDevices = session.gamepadSources = session.gamepadProfileFile = null;
      session.pointerOwner = session.pointerDevices = session.pointerSources = session.pointerSelection = session.pointerSetup = null;
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
          ui["multiplayer-status"].textContent = localOutcome + (session.localPlan
            ? finalLocalPeerText(outcome?.peers, session.localPlan.players) : finalPeerText(outcome?.peer));
        }
        if (session.room && session.owner === owner) {
          const outcome = score?.room;
          const receipt = outcome?.finalWritten === true && outcome.finalAcknowledged === true
            ? "Final score prefix written and acknowledged by the room relay."
            : outcome?.finalWritten === true ? "Final score prefix written · aggregate ACK unavailable."
              : outcome?.finalQueued === true ? "Final score prefix queued · full write unconfirmed."
                : "Room ended without a confirmed final score write.";
          const error = typeof outcome?.error === "string" ? outcome.error.slice(0, 4096) : session.room.error;
          const drain = outcome?.finalDrain === "complete"
            ? " Coordinated room drain completed."
            : outcome?.finalDrain === "failed" ? " Coordinated room drain failed; the local result is retained."
              : outcome?.finalDrain === "cancelled" ? " Coordinated room drain cancelled."
                : " Room drain result unavailable.";
          ui["multiplayer-status"].textContent = receipt + (error ? ` ${error}` : "")
            + drain;
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
          ui["local-status"].textContent = session.localPlan.automatic === true ? "Automatic input sources released."
            : "Local input sources released. Discover again before the next local session.";
        } else if (session.replay !== null) {
          revokeReplayURL();
          lastReplay = { bytes: session.replay.bytes, complete: session.replay.complete && !failed, id: session.id,
            chartPath: session.chartPath, hits: score?.hits ?? null, misses: score?.misses ?? null, combo: score?.combo ?? null };
          ui.export.textContent = `Download last replay (${lastReplay.complete ? "complete" : "prefix"})`;
          capturedReplays = [lastReplay];
          showCapturedReplays(true);
        }
        activePlay = null;
        retainRoomResults(session);
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
  if (settingsOperation || activePlay !== null || recordsOperation !== null || hidPermission || hidOwnershipFailed || lastReplay === null) return;
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
      if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
      opponents.remove(entry.sourceKey);
      showOpponentSelection();
      clearOpponentResults(opponents.size ? `${opponents.size} saved opponent(s) selected.` : "No saved opponents selected.");
      controls();
    });
    opponentButtons.push(button);
    const target = document.createElement("select");
    target.id = `opponent-player-${entry.sourceKey}`;
    target.setAttribute("aria-label", `Comparison player for ${entry.label}`);
    target.append(new Option("Solo / choose local player", ""));
    if (localRoster.players.length > 1) for (const player of localRoster.players) target.append(new Option(`Player ${player}`, String(player)));
    if (entry.player != null && !localRoster.players.includes(entry.player)) target.append(new Option(`Removed player ${entry.player}`, String(entry.player)));
    target.value = entry.player == null ? "" : String(entry.player);
    target.addEventListener("change", () => {
      if (settingsOperation || activePlay || recordsOperation || importing || preparing || hidPermission || hidOwnershipFailed) return;
      try {
        const player = target.value === "" ? null : Number(target.value);
        if (player !== null && (localRoster.players.length === 1 || !localRoster.players.includes(player))) throw new Error("Choose a current local player.");
        opponents.setPlayer(entry.sourceKey, player);
        showOpponentSelection();
        controls();
      } catch (error) { target.value = entry.player == null ? "" : String(entry.player); opponentStatus(error.message, true); }
    });
    opponentButtons.push(target);
    row.append(label, target, button);
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
    if (session.localPlan && data.player !== undefined) {
      if (!session.localPlan.players.includes(data.player) || typeof data.error !== "string" || data.error.length < 1 || data.error.length > 4096 || data.opponents !== null) throw new Error("Invalid member comparison failure message.");
      opponentStatus(`Player ${data.player} comparisons stopped: ${data.error} Other members continue.`, true);
      return;
    }
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
    if (session.localPlan) {
      const groups = validateLocalOpponentSnapshot(result.localOpponents, session.localPlan.players, session.opponentTargets ?? session.opponentSelection ?? []);
      const rows = document.createDocumentFragment();
      for (const group of groups) {
        if (group.error !== null) {
          const item = document.createElement("li"); item.textContent = `Player ${group.player} · Comparison unavailable: ${group.error}`; rows.append(item);
        } else for (const row of group.opponents) {
          const item = document.createElement("li");
          item.textContent = `Player ${group.player} · ${row.kind === "own" ? "Own" : "Other"} · ${row.label} · Hits ${row.hits} · Misses ${row.misses} · Combo ${row.combo} · Best ${row.maxCombo}`;
          rows.append(item);
        }
      }
      ui["opponents-results"].replaceChildren(rows);
      opponentStatus("Final saved comparison prefixes by local player. Labels are not verified identities.");
      return;
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
  if (settingsOperation || !initialized || importing || preparing || activePlay || recordsOperation || hidPermission || hidOwnershipFailed) return;
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
