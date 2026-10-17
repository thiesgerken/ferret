/** The search's state lives in the URL (`/?q=...&source=...&author=...&range=...&model=...`), so
 * results survive reloads and can be linked. */
export type SearchState = {
  q: string;
  sources: string[];
  authors: string[];
  range: TimeRange;
  /** The embedding model, null for the user's default. */
  model: string | null;
};

export const TIME_RANGES = {
  any: { label: "Any time", days: null },
  day: { label: "Past day", days: 1 },
  week: { label: "Past week", days: 7 },
  month: { label: "Past month", days: 31 },
  year: { label: "Past year", days: 366 },
} as const;

export type TimeRange = keyof typeof TIME_RANGES;

export function parseSearch(params: URLSearchParams): SearchState {
  const range = params.get("range") ?? "any";
  return {
    q: params.get("q") ?? "",
    sources: params.getAll("source"),
    authors: params.getAll("author"),
    range: range in TIME_RANGES ? (range as TimeRange) : "any",
    model: params.get("model"),
  };
}

export function searchHref(state: SearchState): string {
  const params = new URLSearchParams({ q: state.q });
  state.sources.forEach((source) => params.append("source", source));
  state.authors.forEach((author) => params.append("author", author));
  if (state.range !== "any") params.set("range", state.range);
  if (state.model) params.set("model", state.model);
  return `/?${params}`;
}

/** Adds `value` to `values`, or removes it if it's there. */
export function toggle(values: string[], value: string): string[] {
  return values.includes(value) ? values.filter((v) => v !== value) : [...values, value];
}

/** The start of `range` relative to `now`, rounded down to the hour so it stays stable while the
 * page is open (it's part of the query's variables). */
export function rangeStart(range: TimeRange, now: Date): string | null {
  const days = TIME_RANGES[range].days;
  if (days === null) return null;
  const start = new Date(now.getTime() - days * 24 * 60 * 60 * 1000);
  start.setMinutes(0, 0, 0);
  return start.toISOString();
}
