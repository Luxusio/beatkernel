export function validateBrowserDisplayFields(fields) {
  if (!Array.isArray(fields) || fields.length !== 4) {
    throw new Error("Browser Display requires four fields.");
  }
  const accepted = Array.from(fields);
  if (accepted.some(value => typeof value !== "string")
    || accepted[0] !== "auto" || accepted[1] !== "fifo") {
    throw new Error("Browser Display supports auto/fifo and one to 16384 CSS pixels.");
  }
  for (const value of accepted.slice(2)) {
    if (!/^[0-9]{1,5}$/.test(value) || Number(value) < 1 || Number(value) > 16384) {
      throw new Error("Browser Display supports auto/fifo and one to 16384 CSS pixels.");
    }
  }
  return Object.freeze(accepted);
}
