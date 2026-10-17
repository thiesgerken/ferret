"use client";

import { Check } from "lucide-react";
import type { ReactNode } from "react";
import type { Hit } from "@/components/search-hit";
import { TIME_RANGES, toggle, type SearchState, type TimeRange } from "@/lib/search-url";
import { cn } from "@/lib/utils";

/** How many authors get a filter, most frequent among the hits first. */
const MAX_AUTHORS = 8;

/**
 * The filters. One set of elements for both layouts: a single horizontally scrolling chip row on
 * narrow screens, a column of labelled lists next to the hits on wide ones.
 */
export function SearchFilters({
  state,
  hits,
  models,
  ranModel,
  onChange,
}: {
  state: SearchState;
  hits: Hit[];
  models: string[];
  ranModel: string | undefined;
  onChange: (state: SearchState) => void;
}) {
  // Filters come from the current hits; selected ones stay even when no hit has them anymore.
  const sources = unique([...state.sources, ...hits.map((hit) => hit.source)]);
  const authors = unique([...state.authors, ...byFrequency(hits.flatMap((hit) => hit.authors))]).slice(
    0,
    Math.max(MAX_AUTHORS, state.authors.length),
  );

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
      {sources.length > 0 && (
        <FilterGroup title="Sources">
          {sources.map((source) => (
            <FilterButton
              key={source}
              label={source}
              pressed={state.sources.includes(source)}
              onClick={() => onChange({ ...state, sources: toggle(state.sources, source) })}
            />
          ))}
        </FilterGroup>
      )}
      {authors.length > 0 && (
        <FilterGroup title="Authors">
          {authors.map((author) => (
            <FilterButton
              key={author}
              label={author}
              pressed={state.authors.includes(author)}
              onClick={() => onChange({ ...state, authors: toggle(state.authors, author) })}
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

function FilterButton({ label, pressed, onClick }: { label: string; pressed: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      onClick={onClick}
      className={cn(
        "flex shrink-0 items-center gap-2 rounded-full border px-3 py-1 text-sm whitespace-nowrap transition-colors",
        "lg:rounded-md lg:border-transparent lg:px-2 lg:text-left",
        pressed
          ? "border-foreground bg-foreground text-background lg:border-transparent lg:bg-accent lg:font-medium lg:text-foreground"
          : "border-border bg-background hover:bg-accent lg:text-muted-foreground lg:hover:text-foreground",
      )}
    >
      <span className="min-w-0 truncate">{label}</span>
      {pressed && <Check className="ml-auto hidden size-3.5 shrink-0 lg:block" aria-hidden />}
    </button>
  );
}

function selectClass(active: boolean) {
  return cn(
    "shrink-0 rounded-full border px-3 py-1 text-sm transition-colors lg:w-full lg:rounded-md lg:px-2",
    active ? "border-foreground bg-foreground text-background lg:bg-accent lg:text-foreground" : "border-border bg-background hover:bg-accent",
  );
}

function unique(values: string[]): string[] {
  return [...new Set(values)];
}

function byFrequency(values: string[]): string[] {
  const counts = new Map<string, number>();
  values.forEach((value) => counts.set(value, (counts.get(value) ?? 0) + 1));
  return [...counts.keys()].sort((a, b) => counts.get(b)! - counts.get(a)!);
}
