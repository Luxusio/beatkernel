// Navigation intent only; ownership and selection remain with the host/Worker.
function keyboardEventAvailable(event) {
  if (!event || event.isComposing || event.repeat || event.ctrlKey || event.altKey || event.metaKey) return false;
  const target = event.target;
  return !target?.isContentEditable && !["INPUT", "TEXTAREA", "SELECT"].includes(target?.tagName?.toUpperCase());
}
export function catalogKeyboardSelection(event, state) {
  if (!event || !state || state.route !== 1 || state.available !== true || state.playing === true
    || !Number.isInteger(state.count) || state.count < 1 || state.count > 8192
    || !Number.isInteger(state.selected) || state.selected < 0 || state.selected >= state.count
    || !keyboardEventAvailable(event)) return null;
  return keyboardSelection(event, state);
}
export function recordsKeyboardControl(event, state) {
  if (!state || state.route !== 4 || state.available !== true || state.playing === true
    || !Number.isInteger(state.count) || state.count < 0 || state.count > 256
    || !Number.isInteger(state.selected) || state.selected < 0
    || (state.count === 0 ? state.selected !== 0 : state.selected >= state.count)
    || !keyboardEventAvailable(event) || event.shiftKey) return null;
  if (event.code === "Escape") return 55n;
  if (state.count === 0) return null;
  const index = keyboardSelection(event, state);
  return index === null ? null : 50000n + BigInt(index);
}
function keyboardSelection(event, state) {
  let next;
  switch (event.code) {
    case "ArrowUp": next = Math.max(0, state.selected - 1); break;
    case "ArrowDown": next = Math.min(state.count - 1, state.selected + 1); break;
    case "Home": next = 0; break;
    case "End": next = state.count - 1; break;
    default: return null;
  }
  return next === state.selected ? null : next;
}
