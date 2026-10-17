const UNITS = [
  ["days", 86_400],
  ["hours", 3_600],
  ["minutes", 60],
  ["seconds", 1],
] as const;

/** "45 sec", "3 min", "2 hr, 5 min", "3 days, 4 hr": the two largest units, for durations and ETAs. */
export function formatDuration(seconds: number, locale: string): string {
  const whole = Math.max(0, Math.round(seconds));
  let rest = whole;
  const nonzero = UNITS.flatMap(([unit, size]) => {
    const count = Math.floor(rest / size);
    rest -= count * size;
    return count > 0 ? [[unit, count] as const] : [];
  });
  // Seconds beside hours or days are noise.
  const shown = nonzero.slice(0, 2).filter(([unit]) => unit !== "seconds" || whole < 3_600);
  // A zero duration still reads as one, "0 sec".
  const zero = shown.length === 0;
  return new Intl.DurationFormat(locale, { style: "short", secondsDisplay: zero ? "always" : "auto" }).format(
    zero ? { seconds: 0 } : Object.fromEntries(shown),
  );
}

/** "now", "3 minutes ago", "yesterday": how long ago `iso` was at `now`. */
export function timeAgo(iso: string, now: Date, locale: string): string {
  const relative = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const seconds = (new Date(iso).getTime() - now.getTime()) / 1000;
  if (Math.abs(seconds) < 45) return relative.format(0, "second");
  for (const [unit, size] of [
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ] as const) {
    if (Math.abs(seconds) >= size) return relative.format(Math.round(seconds / size), unit);
  }
  return relative.format(Math.round(seconds / 60), "minute");
}
