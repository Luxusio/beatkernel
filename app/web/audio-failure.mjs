// Diagnostic facts only: these values never authorize audio output or clocks.
export const AUDIO_FAILURE_DIAGNOSTIC_VERSION = 1;
export const AUDIO_FAILURE_ORIGIN_CONTROL = 0;
export const AUDIO_FAILURE_ORIGIN_ARM = 1;
export const AUDIO_FAILURE_ORIGIN_PROCESS = 2;

const integer = (value, max) => Number.isSafeInteger(value) && value >= 0 && value <= max;
const flag = value => value === 0 || value === 1;
const frame = (present, value, max) => flag(present) && integer(value, max)
  && (present === 1 || value === 0);
const words = (present, low, high) => flag(present)
  && integer(low, 0xffffffff) && integer(high, 0xffffffff)
  && (present === 1 || (low === 0 && high === 0));
const malformed = () => new TypeError("malformed audio failure diagnostics");

// Consumers call this outside process(), after validating terminal identity/status.
export function readAudioFailureDiagnostics(terminal) {
  if (!terminal || typeof terminal !== "object") throw malformed();
  let snapshot;
  try {
    const diagnosticVersion = terminal.diagnosticVersion;
    if (diagnosticVersion === undefined) return null;
    if (diagnosticVersion !== AUDIO_FAILURE_DIAGNOSTIC_VERSION) throw malformed();
    // Copy only known fields, once each, before validating the copied values.
    snapshot = {
      diagnosticVersion,
      origin: terminal.origin,
      ownerPhase: terminal.ownerPhase,
      currentFramePresent: terminal.currentFramePresent,
      currentFrame: terminal.currentFrame,
      blockFramesPresent: terminal.blockFramesPresent,
      blockFrames: terminal.blockFrames,
      expectedFramePresent: terminal.expectedFramePresent,
      expectedFrameLow: terminal.expectedFrameLow,
      expectedFrameHigh: terminal.expectedFrameHigh,
      startFramePresent: terminal.startFramePresent,
      startFrameLow: terminal.startFrameLow,
      startFrameHigh: terminal.startFrameHigh,
      successfulArmFramePresent: terminal.successfulArmFramePresent,
      successfulArmFrame: terminal.successfulArmFrame,
    };
  } catch {
    throw malformed();
  }
  if (!integer(snapshot.origin, AUDIO_FAILURE_ORIGIN_PROCESS)
    || !integer(snapshot.ownerPhase, 3)
    || !frame(snapshot.currentFramePresent, snapshot.currentFrame, Number.MAX_SAFE_INTEGER)
    || !frame(snapshot.blockFramesPresent, snapshot.blockFrames, 0xffffffff)
    || !words(snapshot.expectedFramePresent, snapshot.expectedFrameLow, snapshot.expectedFrameHigh)
    || !words(snapshot.startFramePresent, snapshot.startFrameLow, snapshot.startFrameHigh)
    || !frame(snapshot.successfulArmFramePresent, snapshot.successfulArmFrame, Number.MAX_SAFE_INTEGER)) {
    throw malformed();
  }
  return Object.freeze(snapshot);
}
