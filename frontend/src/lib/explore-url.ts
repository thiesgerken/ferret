import type { ContentType, DocumentOrder } from "@/gql/graphql";

/** Explore's state lives in the URL (`/explore?source=...&month=2026-01&sort=TITLE&asc`), so a
 * filtered table survives reloads and can be linked. */
export type ExploreState = {
  sources: string[];
  contentTypes: ContentType[];
  authors: string[];
  /** `YYYY-MM`, in UTC like the months facet. */
  month: string | null;
  orderBy: DocumentOrder;
  descending: boolean;
  /** Zero-based. */
  page: number;
};

export const PAGE_SIZE = 50;

const ORDERS: DocumentOrder[] = ["CREATED_AT", "MODIFIED_AT", "TITLE", "SOURCE"];
const CONTENT_TYPES: ContentType[] = ["MARKDOWN", "PLAIN", "EMAIL"];

export function parseExplore(params: URLSearchParams): ExploreState {
  const orderBy = params.get("sort") as DocumentOrder;
  const month = params.get("month");
  const page = Number(params.get("page") ?? 0);
  return {
    sources: params.getAll("source"),
    contentTypes: params.getAll("type").filter((type): type is ContentType => CONTENT_TYPES.includes(type as ContentType)),
    authors: params.getAll("author"),
    month: month && /^\d{4}-\d{2}$/.test(month) ? month : null,
    orderBy: ORDERS.includes(orderBy) ? orderBy : "CREATED_AT",
    descending: !params.has("asc"),
    page: Number.isInteger(page) && page > 0 ? page : 0,
  };
}

export function exploreHref(state: ExploreState): string {
  const params = new URLSearchParams();
  state.sources.forEach((source) => params.append("source", source));
  state.contentTypes.forEach((type) => params.append("type", type));
  state.authors.forEach((author) => params.append("author", author));
  if (state.month) params.set("month", state.month);
  if (state.orderBy !== "CREATED_AT") params.set("sort", state.orderBy);
  if (!state.descending) params.set("asc", "");
  if (state.page > 0) params.set("page", String(state.page));
  const query = params.toString();
  return query ? `/explore?${query}` : "/explore";
}

/** The month as the `from`/`to` of a time filter: its first instant and the next month's, in UTC. */
export function monthRange(month: string): { from: string; to: string } {
  const [year, monthNumber] = month.split("-").map(Number);
  return {
    from: new Date(Date.UTC(year, monthNumber - 1, 1)).toISOString(),
    to: new Date(Date.UTC(year, monthNumber, 1)).toISOString(),
  };
}
