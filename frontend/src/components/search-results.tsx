"use client";

import { useEffect, useMemo, useRef } from "react";
import { useQuery } from "urql";
import { SearchField } from "@/components/search-field";
import { graphql } from "@/gql";
import type { SearchHitsQuery } from "@/gql/graphql";
import { TIME_RANGES, searchHref, toggle, type SearchState, type TimeRange } from "@/lib/search-url";
import { pluginOf } from "@/lib/source-plugin";
import { cn } from "@/lib/utils";
import { HomeLink } from "@/components/brand-mark";
import { HitItem, type Hit } from "@/components/search-hit";

const SearchHitsQueryDocument = graphql(`
  query SearchHits($input: SearchInput!) {
    search(input: $input) {
      hits {
        documentId
        source
        externalId
        contentType
        title
        url
        authors
        createdAt
        collapsed
        judged
        keywordScore
        similarity
        chunk {
          startOffset
          endOffset
          parts {
            text
            highlight
          }
        }
      }
      semanticUnavailable
      model
      total
      totalIsLowerBound
      documentsInScope
      tookMs
    }
  }
`);

// Its own query: a search for a model that is no longer configured fails, and the select must
// stay to switch back.
const EmbeddingModelNamesQuery = graphql(`
  query EmbeddingModelNames {
    embeddingModels {
      name
    }
  }
`);

// The plugin icon in a hit's source line comes from the connection that owns its source.
const ConnectionSourcesQuery = graphql(`
  query ConnectionSources {
    connections {
      id
      source
      plugin
    }
  }
`);

type Result = SearchHitsQuery["search"];

// ponytail: docker/ferret.yaml's cut-off, until EmbeddingModel.minSimilarity
// is in the API.
const MIN_SIMILARITY = 0.42;

/** How many authors get a chip, most frequent among the hits first. */
const MAX_AUTHOR_CHIPS = 8;

export function SearchResults({
  state,
  onChange,
  onType,
}: {
  state: SearchState;
  onChange: (state: SearchState) => void;
  /** A query typed into the field, before it's submitted. */
  onType: (state: SearchState) => void;
}) {
  const input = useMemo(
    () => ({
      q: state.q,
      sources: state.sources.length > 0 ? state.sources : null,
      authors: state.authors.length > 0 ? state.authors : null,
      range: TIME_RANGES[state.range].preset,
      model: state.model,
    }),
    [state.q, state.sources, state.authors, state.range, state.model],
  );
  // Typing revisits queries all the time, and a cached answer would pass off an old `tookMs` and
  // `semanticUnavailable` as current.
  const [{ data, error, fetching }] = useQuery({
    query: SearchHitsQueryDocument,
    variables: { input },
    requestPolicy: "cache-and-network",
  });
  const [{ data: models }] = useQuery({ query: EmbeddingModelNamesQuery });
  const [{ data: connections }] = useQuery({ query: ConnectionSourcesQuery });
  const hits = data?.search.hits ?? [];
  const relevance = {
    minSimilarity: MIN_SIMILARITY,
    bestKeywordScore: Math.max(0, ...hits.map((hit) => hit.keywordScore ?? 0)),
  };

  // A query typed before a chip click or Back must not land on top of it: it was typed into a
  // search that's gone.
  const href = searchHref(state);
  const shownHref = useRef(href);
  useEffect(() => {
    shownHref.current = href;
  });
  function handleType(q: string) {
    if (shownHref.current === href) onType({ ...state, q });
  }
  const semanticUnavailable = data?.search.semanticUnavailable;

  return (
    <div className="min-h-dvh">
      <header className="flex items-center gap-6 border-b border-border px-4 py-3 pr-16">
        <HomeLink />
        <SearchField
          defaultValue={state.q}
          onSearch={(q) => onChange({ ...state, q })}
          onType={handleType}
        />
      </header>
      <main className="mx-auto max-w-3xl space-y-6 px-4 py-4">
        <FilterChips
          state={state}
          hits={hits}
          models={models?.embeddingModels.map((model) => model.name) ?? []}
          ranModel={data?.search.model}
          onChange={onChange}
        />
        {data && <Stats result={data.search} sources={state.sources} className={cn(fetching && "opacity-60")} />}
        {semanticUnavailable && (
          <p role="status" className="text-sm text-muted-foreground">
            Keyword matches only: {semanticUnavailable}.
          </p>
        )}
        {error ? (
          <p role="alert" className="text-sm text-destructive">
            {error.graphQLErrors[0]?.message ?? error.message}
          </p>
        ) : data && hits.length === 0 ? (
          <p className="text-sm text-muted-foreground">No documents match.</p>
        ) : (
          // Keyed by the search, so a hit's ✓ state never carries over into another search.
          <ol key={JSON.stringify(input)} className={cn("space-y-6", fetching && "opacity-60")}>
            {hits.map((hit) => (
              <li key={hit.documentId}>
                <HitItem
                  hit={hit}
                  search={input}
                  plugin={pluginOf(hit.source, connections?.connections ?? [])}
                  relevance={relevance}
                />
              </li>
            ))}
          </ol>
        )}
      </main>
    </div>
  );
}

function FilterChips({
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
  // Chips come from the current hits; selected ones stay even when no hit has them anymore.
  const sources = unique([...state.sources, ...hits.map((hit) => hit.source)]);
  const authors = unique([...state.authors, ...byFrequency(hits.flatMap((hit) => hit.authors))]).slice(
    0,
    Math.max(MAX_AUTHOR_CHIPS, state.authors.length),
  );

  return (
    <div className="flex flex-wrap items-center gap-2">
      <select
        aria-label="Time range"
        value={state.range}
        onChange={(event) => onChange({ ...state, range: event.target.value as TimeRange })}
        className={chipClass(state.range !== "any")}
      >
        {Object.entries(TIME_RANGES).map(([range, { label }]) => (
          <option key={range} value={range}>
            {label}
          </option>
        ))}
      </select>
      {/* With one model there's nothing to choose. The default option names the model that ran. */}
      {models.length > 1 && (
        <select
          aria-label="Embedding model"
          value={state.model ?? ""}
          onChange={(event) => onChange({ ...state, model: event.target.value || null })}
          className={chipClass(state.model !== null)}
        >
          <option value="">{!state.model && ranModel ? `Default model (${ranModel})` : "Default model"}</option>
          {models.map((model) => (
            <option key={model} value={model}>
              {model}
            </option>
          ))}
        </select>
      )}
      {sources.map((source) => (
        <Chip
          key={`source:${source}`}
          label={source}
          pressed={state.sources.includes(source)}
          onClick={() => onChange({ ...state, sources: toggle(state.sources, source) })}
        />
      ))}
      {authors.map((author) => (
        <Chip
          key={`author:${author}`}
          label={author}
          pressed={state.authors.includes(author)}
          onClick={() => onChange({ ...state, authors: toggle(state.authors, author) })}
        />
      ))}
    </div>
  );
}

function Chip({ label, pressed, onClick }: { label: string; pressed: boolean; onClick: () => void }) {
  return (
    <button type="button" aria-pressed={pressed} onClick={onClick} className={chipClass(pressed)}>
      {label}
    </button>
  );
}

function chipClass(active: boolean) {
  return cn(
    "rounded-full border px-3 py-1 text-sm transition-colors",
    active ? "border-foreground bg-foreground text-background" : "border-border bg-background hover:bg-accent",
  );
}

/** "37 matches · 526 documents in carapace, demo · 84 ms", to judge the results at a glance. */
function Stats({ result, sources, className }: { result: Result; sources: string[]; className?: string }) {
  const matches = `${result.total}${result.totalIsLowerBound ? "+" : ""} ${result.total === 1 ? "match" : "matches"}`;
  const documents = `${result.documentsInScope} ${result.documentsInScope === 1 ? "document" : "documents"}`;
  const scope = sources.length > 0 ? sources.join(", ") : "all sources";
  return (
    <p className={cn("text-xs text-muted-foreground", className)}>
      {matches} · {documents} in {scope} · {result.tookMs} ms
    </p>
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
