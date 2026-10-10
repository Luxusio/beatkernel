// Catalog navigation intent only; ownership and selection remain with the host/Worker.
export function catalogKeyboardSelection(event, state) {
  if (!event || !state || state.route !== 1 || state.available !== true || state.playing === true
    || !Number.isInteger(state.count) || state.count < 1 || state.count > 8192
    || !Number.isInteger(state.selected) || state.selected < 0 || state.selected >= state.count
    || event.isComposing || event.repeat || event.ctrlKey || event.altKey || event.metaKey) return null;
  const target = event.target;
  if (target?.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target?.tagName?.toUpperCase())) return null;
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
