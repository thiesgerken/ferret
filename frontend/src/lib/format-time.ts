/** "45 s", "3 min", "2 h 5 min", "3 d 4 h": the two largest units, for durations and ETAs. */
export function formatDuration(seconds: number): string {
  const units: [string, number][] = [
    ["d", 86_400],
    ["h", 3_600],
    ["min", 60],
    ["s", 1],
  ];
  const whole = Math.max(0, Math.round(seconds));
  const parts: string[] = [];
  let rest = whole;
  for (const [unit, size] of units) {
    const count = Math.floor(rest / size);
    if (count > 0 || (unit === "s" && parts.length === 0)) parts.push(`${count} ${unit}`);
    rest -= count * size;
    if (parts.length === 2) break;
  }
  // Seconds beside hours or days are noise.
  if (parts.length === 2 && parts[1].endsWith(" s") && whole >= 3_600) parts.pop();
  return parts.join(" ");
}

const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

/** "just now", "3 minutes ago", "yesterday": how long ago `iso` was at `now`. */
export function timeAgo(iso: string, now: Date): string {
  const seconds = (new Date(iso).getTime() - now.getTime()) / 1000;
  if (Math.abs(seconds) < 45) return "just now";
  for (const [unit, size] of [
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ] as const) {
    if (Math.abs(seconds) >= size) return relative.format(Math.round(seconds / size), unit);
  }
  return relative.format(Math.round(seconds / 60), "minute");
}
