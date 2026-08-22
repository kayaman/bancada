// Two local-time formatters shared by the panels that stamp rows and name
// exported files. Lifted out of `ObsLog.tsx` so the Serial Monitor can stamp
// its rows with exactly the same clock the MQTT/WS feed uses — a difference
// of even a digit between panels reads as a bug when they sit side by side.
//
// Deliberately hand-rolled rather than `Intl.DateTimeFormat`: these run per
// row on a feed that can push thousands of lines a second, and the locale's
// idea of a separator is not wanted here.

const two = (n: number) => String(n).padStart(2, "0");

/** Epoch-ms → HH:MM:SS.mmm local time. */
export function hms(ts: number): string {
  const d = new Date(ts);
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}.${String(
    d.getMilliseconds(),
  ).padStart(3, "0")}`;
}

/** Date → YYYYMMDD-HHMMSS local time, for default export filenames. */
export function fileStamp(d: Date): string {
  return (
    `${d.getFullYear()}${two(d.getMonth() + 1)}${two(d.getDate())}-` +
    `${two(d.getHours())}${two(d.getMinutes())}${two(d.getSeconds())}`
  );
}
