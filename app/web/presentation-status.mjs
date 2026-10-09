const positiveId = value => Number.isSafeInteger(value) && value > 0;
const visualId = value => typeof value === "bigint" && value > 0n && value <= 0xffffffffffffffffn;

function valid(identity) {
  if (identity === null || typeof identity !== "object") return false;
  const menu = visualId(identity.menuGeneration) && visualId(identity.screen) && visualId(identity.revision);
  return Number.isSafeInteger(identity.owner) && identity.owner >= 0
    && identity.worker !== null && (typeof identity.worker === "object" || typeof identity.worker === "function")
    && Number.isSafeInteger(identity.selectedId) && identity.selectedId >= 0
    && (identity.playId === undefined || positiveId(identity.playId))
    && visualId(identity.generation) && visualId(identity.content)
    && (identity.menuGeneration === undefined && identity.screen === undefined && identity.revision === undefined
      || menu);
}

function sameScope(left, right) {
  return left.owner === right.owner && left.worker === right.worker
    && left.selectedId === right.selectedId && left.playId === right.playId
    && left.menuGeneration === right.menuGeneration && left.screen === right.screen && left.revision === right.revision;
}

/** Restore ordinary feedback only when its waiting surface actually presents. */
export class PresentationStatus {
  constructor(show, initialText = "", initialError = false) {
    this.show = show;
    this.text = initialText;
    this.error = initialError;
    this.pending = null;
  }

  message(text, error = false) {
    this.invalidate();
    this.text = text;
    this.error = error;
    this.show(text, error);
  }

  wait(identity, text) {
    if (!valid(identity)) return;
    const previous = this.pending;
    if (previous && sameScope(previous, identity)) {
      if (identity.generation < previous.generation
        || (identity.generation === previous.generation && identity.content !== previous.content)) return;
      if (identity.generation === previous.generation && text === previous.waitText) return;
    }
    this.pending = {
      owner: identity.owner, worker: identity.worker, selectedId: identity.selectedId,
      playId: identity.playId, generation: identity.generation, content: identity.content,
      menuGeneration: identity.menuGeneration, screen: identity.screen, revision: identity.revision,
      waitText: text,
    };
    this.show(text, true);
  }

  drawn(identity) {
    const pending = this.pending;
    if (!pending || !valid(identity) || !sameScope(pending, identity)
      || identity.generation !== pending.generation || identity.content !== pending.content) return false;
    this.pending = null;
    this.show(this.text, this.error);
    return true;
  }

  invalidate(restore = false) {
    const pending = this.pending;
    this.pending = null;
    if (restore && pending) this.show(this.text, this.error);
  }
}
