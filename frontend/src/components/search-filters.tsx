"use client";

import { Check, CircleDashed } from "lucide-react";
import { Fragment, type ReactNode } from "react";
import type { SearchHitsQuery } from "@/gql/graphql";
import { NamedIcon } from "@/components/named-icon";
import { TIME_RANGES, toggle, type SearchState, type TimeRange } from "@/lib/search-url";
import { sourceLabel, type SourceLabel } from "@/lib/source-label";
import { cn } from "@/lib/utils";

/** How many authors get a filter, most matches first. The facet has more, the column stays short. */
const MAX_AUTHORS = 8;

type Facets = SearchHitsQuery["search"]["facets"];
type FacetCount = { value: string; count: number };

/**
 * The filters. One set of elements for both layouts: a single horizontally scrolling chip row on
 * narrow screens, a column of labelled lists next to the hits on wide ones.
 */
export function SearchFilters({
  state,
  facets,
  roots,
  models,
  ranModel,
  onChange,
}: {
  state: SearchState;
  /** Undefined until the first answer. */
  facets: Facets | undefined;
  /** Connection ids to their source roots, for the sources' labels. Undefined until loaded. */
  roots: Map<string, string> | undefined;
  models: string[];
  ranModel: string | undefined;
  onChange: (state: SearchState) => void;
}) {
  // Selected values stay even when nothing matches them anymore, so they can be unselected.
  const sourceFacets = facets?.sources ?? [];
  const writerOf = (source: string) => sourceFacets.find((facet) => facet.value === source)?.writer ?? null;
  const groups = groupByWriter(withSelected(sourceFacets, state.sources), writerOf);
  const authors = withSelected(facets?.authors ?? [], state.authors).slice(0, Math.max(MAX_AUTHORS, state.authors.length));
  const counted = (count: number | null) => (count === null ? null : `${count}${facets?.areLowerBounds ? "+" : ""}`);

  return (
    <nav
      aria-label="Filters"
      className="-mx-4 flex items-center gap-2 overflow-x-auto px-4 [scrollbar-width:none] lg:mx-0 lg:flex-col lg:items-stretch lg:gap-7 lg:overflow-visible lg:px-0"
    >
      <FilterGroup title="Time">
        <select
          aria-label="Time range"
          value={state.range}
          onChange={(event) => onChange({ ...state, range: event.target.value as TimeRange })}
          className={selectClass(state.range !== "any")}
        >
          {Object.entries(TIME_RANGES).map(([range, { label }]) => (
            <option key={range} value={range}>
              {label}
            </option>
          ))}
        </select>
      </FilterGroup>
      {/* With one model there's nothing to choose. The default option names the model that ran. */}
      {models.length > 1 && (
        <FilterGroup title="Model">
          <select
            aria-label="Embedding model"
            value={state.model ?? ""}
            onChange={(event) => onChange({ ...state, model: event.target.value || null })}
            className={selectClass(state.model !== null)}
          >
            <option value="">{!state.model && ranModel ? `Default model (${ranModel})` : "Default model"}</option>
            {models.map((model) => (
              <option key={model} value={model}>
                {model}
              </option>
            ))}
          </select>
        </FilterGroup>
      )}
      {groups.length > 0 && (
        <FilterGroup title="Sources">
          {groups.map(({ writer, sources }) => {
            // A writer with several sources gets a heading in the column, and its sources read as
            // the path below it. The chip row has no headings, so chips keep the full label.
            const grouped = writer !== null && sources.length > 1;
            return (
              <Fragment key={writer?.id ?? ""}>
                {grouped && (
                  <p className="mt-1.5 hidden items-center gap-2 px-2 text-xs font-medium text-muted-foreground lg:flex">
                    <NamedIcon name={writer.icon} className="size-3.5 shrink-0" />
                    {writer.name}
                  </p>
                )}
                {sources.map(({ value, count }) => {
                  const label = sourceLabel(value, writer, roots);
                  return (
                    <FilterButton
                      key={value}
                      label={label}
                      short={grouped ? label.text.slice(writer.name.length + " › ".length) || "All" : undefined}
                      title={value}
                      count={counted(count)}
                      pressed={state.sources.includes(value)}
                      onClick={() => onChange({ ...state, sources: toggle(state.sources, value) })}
                    />
                  );
                })}
              </Fragment>
            );
          })}
        </FilterGroup>
      )}
      {authors.length > 0 && (
        <FilterGroup title="Authors">
          {authors.map(({ value, count }) => (
            <FilterButton
              key={value}
              label={value}
              count={counted(count)}
              pressed={state.authors.includes(value)}
              onClick={() => onChange({ ...state, authors: toggle(state.authors, value) })}
            />
          ))}
        </FilterGroup>
      )}
    </nav>
  );
}

/** On narrow screens the group dissolves into the chip row, so only wide screens see its title. */
function FilterGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="contents lg:flex lg:flex-col lg:gap-0.5">
      <h2 className="mb-1.5 hidden px-2 text-xs font-medium tracking-wide text-muted-foreground uppercase lg:block">
        {title}
      </h2>
      {children}
    </div>
  );
}

function FilterButton({
  label,
  short,
  title,
  count,
  pressed,
  onClick,
}: {
  /** A plain label, or a source's label with its writer's icon. */
  label: string | SourceLabel;
  /** What the column shows under a writer's heading instead of the full label. */
  short?: string;
  title?: string;
  /** Null for a selected value the facet doesn't list. */
  count: string | null;
  pressed: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      onClick={onClick}
      title={title}
      className={cn(
        "flex shrink-0 items-center gap-2 rounded-full border px-3 py-1 text-sm whitespace-nowrap transition-colors",
        "lg:rounded-md lg:border-transparent lg:px-2 lg:text-left",
        pressed
          ? "border-foreground bg-foreground text-background lg:border-transparent lg:bg-accent lg:font-medium lg:text-foreground"
          : "border-border bg-background hover:bg-accent lg:text-muted-foreground lg:hover:text-foreground",
      )}
    >
      {typeof label !== "string" && (
        <span className={cn("shrink-0", short !== undefined && "lg:invisible")}>
          {label.icon ? <NamedIcon name={label.icon} className="size-3.5" /> : <CircleDashed className="size-3.5" aria-hidden />}
        </span>
      )}
      <span className="min-w-0 truncate" data-label>
        <span className={cn(short !== undefined && "lg:hidden")}>{typeof label === "string" ? label : label.text}</span>
        {short !== undefined && <span className="hidden lg:inline">{short}</span>}
      </span>
      {count !== null && <span className="ml-auto text-xs tabular-nums opacity-60">{count}</span>}
      {pressed && <Check className={cn("hidden size-3.5 shrink-0 lg:block", count === null && "ml-auto")} aria-hidden />}
    </button>
  );
}

function selectClass(active: boolean) {
  return cn(
    "shrink-0 rounded-full border px-3 py-1 text-sm transition-colors lg:w-full lg:rounded-md lg:px-2",
    active ? "border-foreground bg-foreground text-background lg:bg-accent lg:text-foreground" : "border-border bg-background hover:bg-accent",
  );
}

type SourceWriter = NonNullable<Facets["sources"][number]["writer"]>;

/** Sources by writer, in the order each writer first appears; those without a writer last. */
function groupByWriter<T extends { value: string }>(
  sources: T[],
  writerOf: (source: string) => SourceWriter | null,
): { writer: SourceWriter | null; sources: T[] }[] {
  const groups = new Map<string, { writer: SourceWriter | null; sources: T[] }>();
  for (const source of sources) {
    const writer = writerOf(source.value);
    const key = writer?.id ?? "";
    if (!groups.has(key)) groups.set(key, { writer, sources: [] });
    groups.get(key)!.sources.push(source);
  }
  return [...groups.values()].sort((a, b) => Number(a.writer === null) - Number(b.writer === null));
}

function withSelected(counts: FacetCount[], selected: string[]): { value: string; count: number | null }[] {
  const missing = selected.filter((value) => !counts.some((count) => count.value === value));
  return [...missing.map((value) => ({ value, count: null })), ...counts];
}
