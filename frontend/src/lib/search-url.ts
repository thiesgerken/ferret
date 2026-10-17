/** The search's state lives in the URL (`/?q=...&source=...&author=...&range=...&model=...&mode=...`), so
 * results survive reloads and can be linked. */
export type SearchState = {
  q: string;
  sources: string[];
  authors: string[];
  range: TimeRange;
  /** The embedding model, null for the user's default. */
  model: string | null;
  /** Which half of the search runs: both, or only keyword or only semantic matching. */
  mode: SearchMode;
};

export const SEARCH_MODES = ["hybrid", "keyword", "semantic"] as const;

export type SearchMode = (typeof SEARCH_MODES)[number];

/** The mode with keyword or semantic matching switched on or off. Hybrid is both on; switching
 * off the only one left keeps it, so a search always matches somehow. */
export function toggleMatching(mode: SearchMode, matching: "keyword" | "semantic"): SearchMode {
  const other = matching === "keyword" ? "semantic" : "keyword";
  if (mode === "hybrid") return other;
  return mode === other ? "hybrid" : mode;
}

export function matchingOn(mode: SearchMode, matching: "keyword" | "semantic"): boolean {
  return mode === "hybrid" || mode === matching;
}

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
  const mode = params.get("mode") ?? "hybrid";
  return {
    q: params.get("q") ?? "",
    sources: params.getAll("source"),
    authors: params.getAll("author"),
    range: range in TIME_RANGES ? (range as TimeRange) : "any",
    model: params.get("model"),
    mode: SEARCH_MODES.includes(mode as SearchMode) ? (mode as SearchMode) : "hybrid",
  };
}

export function searchHref(state: SearchState): string {
  const params = new URLSearchParams({ q: state.q });
  state.sources.forEach((source) => params.append("source", source));
  state.authors.forEach((author) => params.append("author", author));
  if (state.range !== "any") params.set("range", state.range);
  if (state.model) params.set("model", state.model);
  if (state.mode !== "hybrid") params.set("mode", state.mode);
  return `/?${params}`;
}

/** Adds `value` to `values`, or removes it if it's there. */
export function toggle(values: string[], value: string): string[] {
  return values.includes(value) ? values.filter((v) => v !== value) : [...values, value];
}
