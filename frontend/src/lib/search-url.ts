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

/** The server resolves a range against its own clock, and judgments keep the preset. */
export const TIME_RANGES = {
  any: { label: "Any time", preset: null },
  day: { label: "Past day", preset: "DAY" },
  week: { label: "Past week", preset: "WEEK" },
  month: { label: "Past month", preset: "MONTH" },
  year: { label: "Past year", preset: "YEAR" },
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
