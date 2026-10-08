import init, { BrowserView } from "./pkg/beatkernel_bms_runtime.js";
import { preflightPacket, preflightMenuPayload, menuOpponentProjection, nativePacketView, validateRenderLimits, unsignedIdentity, boundedU32 } from "./render-protocol.mjs";

let view = null;
let port = null;
let loading = false;
let disposed = false;
let failed = false;
let epoch = 0;
let maxPacketBytes = 0;
let maxDiagnosticBytes = 0;
let generationFloor = 0n;
let current = null;
let extent = [0, 0];
let geometryVersion = 0n;
let submittedGeometry = 0n;
let redraw = null;
let retries = 0;
let initTimer = null;

function send(message) { port?.postMessage(message); }
function stopDraw() {
  if (redraw === null) return;
  if (redraw.animation) self.cancelAnimationFrame(redraw.id);
  else clearTimeout(redraw.id);
  redraw = null;
}
function release(binding) {
  if (!binding) return;
  try { binding.dispose_menu_motion(); binding.retire_visual(); } finally { binding.free(); }
}
function diagnostic(error) {
  const text = error instanceof Error ? error.message : String(error);
  // Bound UTF-8 diagnostics as well as JS code units.
  const encoder = new TextEncoder();
  let output = text.slice(0, maxDiagnosticBytes);
  while (encoder.encode(output).length > maxDiagnosticBytes) output = output.slice(0, Math.floor(output.length / 2));
  return output;
}
function fail(error, operationId = null, identity = current) {
  if (failed || disposed) return;
  failed = true;
  stopDraw();
  const report = { kind: "render-error", generation: identity?.generation ?? 0n, content: identity?.content ?? 0n,
    operationId, mode: identity?.mode ?? "initializing", message: diagnostic(error) };
  try { send(report); } catch {}
  try { self.postMessage(report); } catch {}
  // Gameplay decides capture-preserving failed stop. This owner has no game.
}
function wait(identity) {
  send({ kind: "render-wait", generation: identity.generation, content: identity.content, geometryVersion });
}
function scheduleDraw(reset = true) {
  if (disposed || failed || !view || !current || !current.drawable) return;
  if (reset) retries = 0;
  if (extent.includes(0)) { stopDraw(); wait(current); return; }
  if (redraw !== null) return;
  const identity = current;
  const ownerEpoch = epoch;
  const draw = () => {
    redraw = null;
    if (disposed || failed || epoch !== ownerEpoch || current !== identity) return;
    try {
      let presented;
      if (identity.mode === "menu") {
        presented = view.draw_menu_at(performance.now());
        if (typeof presented !== "boolean") throw new Error("Invalid menu presentation result.");
      } else {
        view.draw_visual();
        presented = !view.needs_redraw();
      }
      if (presented) {
        retries = 0;
        send({ kind: "drawn", generation: identity.generation, content: identity.content, sequence: identity.sequence });
        if (geometryVersion > submittedGeometry) {
          const page = identity.mode === "menu" ? view.menu_page() : view.visual_page();
          const [width, height] = extent;
          if (!boundedU32(page)) throw new Error("Invalid applied visual page.");
          submittedGeometry = geometryVersion;
          const evidence = { kind: "geometry-ack", generation: identity.generation, content: identity.content, geometryVersion,
            page, width, height, ...(identity.menu ? { menuGeneration: identity.menu.generation,
              screen: identity.menu.screen, revision: identity.menu.revision, details: view.menu_details() } : {}) };
          send(evidence);
          self.postMessage(evidence);
        }
        if (view.needs_redraw() || (identity.mode === "menu" && view.menu_motion_active())) scheduleDraw(false);
      } else if (view.needs_redraw()) {
        if (++retries <= 3) scheduleDraw(false);
        else wait(identity);
      } else {
        wait(identity);
      }
    } catch (error) { fail(error, null, identity); }
  };
  if (typeof self.requestAnimationFrame === "function") {
    try { redraw = { animation: true, id: self.requestAnimationFrame(draw) }; return; }
    catch (error) { if (error?.name !== "NotSupportedError") { fail(error); return; } }
  }
  redraw = { animation: false, id: setTimeout(draw, 16) };
}
function menu(message) {
  const header = preflightMenuPayload(message.packet, message.recordPreview);
  const opponents = menuOpponentProjection(message.opponents);
  const { generation, content, operationId, geometryVersion: version } = message;
  if (!unsignedIdentity(generation) || !unsignedIdentity(content) || !unsignedIdentity(operationId)) throw new Error("Invalid menu render identity.");
  const replacing = !current || current.generation !== generation || current.content !== content;
  if (replacing ? generation <= generationFloor : current.mode !== "menu" || operationId <= current.operationId
    || header.generation !== current.menu.generation || header.revision <= current.menu.revision) return;
  if (version !== undefined && (!unsignedIdentity(version) || version <= geometryVersion)) throw new Error("Menu geometry version must increase.");
  const applied = view.import_menu_packet(nativePacketView(message.packet), message.recordPreview == null ? undefined : nativePacketView(message.recordPreview));
  if (applied !== header.revision) throw new Error("Menu importer returned a different applied revision.");
  view.set_menu_opponents(opponents.count, opponents.own, opponents.other);
  if (replacing) {
    stopDraw(); submittedGeometry = 0n; generationFloor = generation;
    current = { generation, content, mode: "menu", drawable: true, sequence: header.revision, operationId, menu: header, lastAction: 0n };
  } else { current.menu = header; current.sequence = header.revision; current.operationId = operationId; }
  if (version !== undefined) geometryVersion = version;
  send({ kind: "menu-ack", operationId, generation, content, menuGeneration: header.generation, screen: header.screen, revision: header.revision });
  scheduleDraw();
}
function menuInput(message) {
  if (!current || current.mode !== "menu" || current.generation !== message.generation || current.content !== message.content
    || message.menuGeneration !== current.menu.generation || message.screen !== current.menu.screen || message.revision !== current.menu.revision
    || message.geometryVersion !== submittedGeometry || geometryVersion !== submittedGeometry
    || !unsignedIdentity(message.actionId) || message.actionId <= current.lastAction || !Number.isFinite(message.x) || !Number.isFinite(message.y)
    || extent.includes(0)) return;
  current.lastAction = message.actionId;
  const control = view.menu_hit(message.x, message.y);
  if (control === 0n) return;
  if ((message.downX !== undefined || message.downY !== undefined)
    && (!Number.isFinite(message.downX) || !Number.isFinite(message.downY) || view.menu_hit(message.downX, message.downY) !== control)) return;
  if (!unsignedIdentity(control)) throw new Error("Invalid menu hit control.");
  send({ kind: "menu-action", generation: current.generation, content: current.content,
    menuGeneration: current.menu.generation, screen: current.menu.screen, revision: current.menu.revision, actionId: message.actionId, control });
}
function packet(message) {
  const { packet: input, operationId, mode } = message;
  const header = preflightPacket(input, maxPacketBytes);
  if (!unsignedIdentity(operationId)) throw new Error("Invalid render operation identity.");
  if (message.generation !== header.generation || message.content !== header.content) throw new Error("Render message and packet identities differ.");
  const cold = [1, 4, 5, 6].includes(header.kind);
  const combinedRoom = header.kind === 6 && current?.mode === "results"
    && current.generation === header.generation && current.content === header.content;
  if (cold) {
    if (header.generation <= generationFloor && !combinedRoom) return;
    if (header.kind === 1 && !["preview", "live", "local", "replay"].includes(mode)) throw new Error("Visual registration requires explicit mode.");
  } else {
    if (!current || header.generation !== current.generation || header.content !== current.content) return;
    if (header.sequence <= current.sequence || operationId <= current.operationId) return;
    if ((header.kind === 3) !== (current.mode === "preview")) throw new Error("Visual state does not match presentation mode.");
  }
  if (combinedRoom && operationId <= current.operationId) return;
  const version = message.geometryVersion;
  if (version !== undefined && (!unsignedIdentity(version) || version <= geometryVersion)) throw new Error("Packet geometry version must increase.");
  // Pass a fresh native view: caller properties cannot change WASM admission.
  const bytes = nativePacketView(input);
  const applied = header.kind === 1
    ? view.import_visual_registration(bytes, maxPacketBytes, maxDiagnosticBytes,
      { preview: 0, live: 1, local: 2, replay: 3 }[mode])
    : view.import_visual_packet(bytes, maxPacketBytes, maxDiagnosticBytes);
  if (applied !== header.sequence) throw new Error("Visual importer returned a different applied sequence.");
  if (cold && !combinedRoom) {
    stopDraw();
    current = { generation: header.generation, content: header.content, sequence: header.sequence, operationId,
      mode: header.kind === 1 ? mode : ({ 4: "history", 5: "results", 6: "room" }[header.kind]),
      drawable: header.kind !== 5 && (header.kind !== 1 || mode === "preview") };
    submittedGeometry = 0n;
    generationFloor = header.generation;
  } else {
    current.sequence = header.sequence;
    current.operationId = operationId;
    if (!combinedRoom) current.drawable = true;
  }
  if (version !== undefined) geometryVersion = version;
  send({ kind: "state-ack", operationId, generation: header.generation, content: header.content,
    sequence: header.sequence, packetKind: header.kind });
  scheduleDraw();
}
function control(message) {
  const { kind, generation, content, operationId, width, height, page, comparisons, details, geometryVersion: version } = message;
  if (!current || generation !== current.generation || content !== current.content) return;
  if (!unsignedIdentity(operationId)) throw new Error("Invalid render control identity.");
  if (operationId <= current.operationId) return;
  if (kind === "retire") {
    stopDraw();
    view.dispose_menu_motion();
    view.retire_visual();
    generationFloor = generationFloor > generation ? generationFloor : generation;
    current = null;
    send({ kind: "control-ack", operation: kind, operationId, generation, content });
    return;
  }
  if (kind === "menu-motion" && (current.mode !== "menu" || message.menuGeneration !== current.menu.generation
    || message.screen !== current.menu.screen || message.revision !== current.menu.revision)) return;
  if (!unsignedIdentity(version) || version <= geometryVersion) throw new Error("Geometry version must increase.");
  if (kind === "resize") {
    if (!boundedU32(width) || !boundedU32(height)) throw new Error("Invalid surface extent.");
    view.resize(width, height);
    extent = [width, height];
    if (current.mode === "menu") {
      if (extent.includes(0)) view.suspend_menu_motion(performance.now());
      else view.resume_menu_motion(performance.now());
    }
  } else if (kind === "menu-motion") {
    const values = message.transforms;
    if (!unsignedIdentity(message.control) || !ArrayBuffer.isView(values)
      || Object.prototype.toString.call(values) !== "[object Float32Array]" || values.length !== 10
      || !values.every(Number.isFinite) || !Number.isFinite(message.durationMs) || message.durationMs < 0
      || !Number.isInteger(message.easing) || message.easing < 0 || message.easing > 3) throw new Error("Invalid menu motion.");
    view.request_menu_motion(message.screen, message.revision, message.control,
      values, message.durationMs, message.easing, performance.now());
  } else if (kind === "page") {
    if (!boundedU32(page) || typeof comparisons !== "boolean") throw new Error("Invalid visual page.");
    view.set_visual_page(page, comparisons);
    if (current.mode === "results") current.drawable = true;
  } else if (kind === "room-page") {
    if (!boundedU32(page)) throw new Error("Invalid room page.");
    view.set_visual_room_page(page);
  } else if (kind === "menu-details") {
    if (current.mode !== "menu" || typeof details !== "boolean" || !boundedU32(page)) throw new Error("Invalid menu details page.");
    view.set_menu_record_details(details, page);
  } else throw new Error("Unknown render control.");
  geometryVersion = version;
  current.operationId = operationId;
  send({ kind: "control-ack", operation: kind, operationId, generation, content, geometryVersion: version });
  scheduleDraw();
}
function receive(event) {
  if (disposed || failed) return;
  const input = event.data;
  if (!input || typeof input !== "object") { fail(new Error("Invalid render message.")); return; }
  // Snapshot accessor-bearing records once before invoking native/WASM work.
  let message;
  try {
    message = { kind: input.kind, packet: input.packet, operationId: input.operationId, generation: input.generation,
      content: input.content, mode: input.mode, width: input.width, height: input.height, page: input.page,
      comparisons: input.comparisons, geometryVersion: input.geometryVersion, menuGeneration: input.menuGeneration,
      screen: input.screen, revision: input.revision, actionId: input.actionId, x: input.x, y: input.y,
      downX: input.downX, downY: input.downY, recordPreview: input.recordPreview, details: input.details, opponents: input.opponents,
      control: input.control, transforms: input.transforms, durationMs: input.durationMs, easing: input.easing };
    if (unsignedIdentity(message.generation) && message.generation < generationFloor) return;
    if (!current && unsignedIdentity(message.generation) && message.generation <= generationFloor) return;
    if (message.kind === "menu") menu(message);
    else if (message.kind === "menu-input") menuInput(message);
    else if (message.kind === "packet") packet(message);
    else control(message);
  } catch (error) {
    const identity = unsignedIdentity(message?.generation) && unsignedIdentity(message?.content) ? message : current;
    fail(error, message?.operationId, identity);
  }
}
function dispose() {
  if (disposed) return;
  disposed = true;
  ++epoch;
  clearTimeout(initTimer);
  stopDraw();
  current = null;
  const owned = view;
  view = null;
  try { release(owned); } catch (error) { try { self.postMessage({ kind: "dispose-error", message: diagnostic(error) }); } catch {} }
  try { if (port) { port.onmessage = null; port.onmessageerror = null; port.close(); } } catch {}
  self.postMessage({ kind: "disposed" });
}
async function initialize(input) {
  if (loading || view || disposed || failed) return;
  const { canvas, port: channel, maxPacketBytes: packetLimit, maxDiagnosticBytes: diagnosticLimit, timeoutMs = 10000 } = input;
  validateRenderLimits(packetLimit, diagnosticLimit);
  if (!channel || !["postMessage", "start", "close"].every(name => typeof channel[name] === "function")
    || !Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 60000) throw new Error("Invalid renderer initialization.");
  loading = true;
  port = channel;
  maxPacketBytes = packetLimit;
  maxDiagnosticBytes = diagnosticLimit;
  const ownerEpoch = ++epoch;
  const currentOwner = () => !disposed && !failed && epoch === ownerEpoch;
  initTimer = setTimeout(() => { if (currentOwner()) fail(new Error("Renderer initialization timed out.")); }, timeoutMs);
  await init();
  if (!currentOwner()) return;
  const created = await BrowserView.create(canvas);
  if (!currentOwner()) { release(created); return; }
  view = created;
  extent = [canvas.width, canvas.height];
  if (!extent.every(boundedU32)) throw new Error("Invalid initial canvas extent.");
  clearTimeout(initTimer);
  port.onmessage = receive;
  port.onmessageerror = () => fail(new Error("Visual transport message could not be decoded."));
  port.start();
  send({ kind: "ready" });
  self.postMessage({ kind: "ready" });
}
self.onmessage = event => {
  const input = event.data;
  if (input?.kind === "dispose") { dispose(); return; }
  if (input?.kind === "init") initialize(input).catch(error => fail(error));
};
