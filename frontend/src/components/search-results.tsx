"use client";

import { useCallback, useEffect, useMemo, useRef } from "react";
import { useClient, useQuery } from "urql";
import { SearchField } from "@/components/search-field";
import { graphql } from "@/gql";
import type { SearchHitsQuery } from "@/gql/graphql";
import { TIME_RANGES, searchHref, type SearchState } from "@/lib/search-url";
import { sourceLabel } from "@/lib/source-label";
import { cn } from "@/lib/utils";
import { HomeLink } from "@/components/brand-mark";
import { HitItem } from "@/components/search-hit";
import { MoreHits } from "@/components/more-hits";
import { SearchFilters } from "@/components/search-filters";
import { SearchX, TriangleAlert } from "lucide-react";

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
        origin {
          kind
          id
          name
          icon
        }
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
      hasMore
      facets {
        sources {
          value
          count
          origin {
            kind
            id
            name
            icon
          }
        }
        authors {
          value
          count
        }
        areLowerBounds
      }
    }
  }
`);

// Its own query: a search for a model that is no longer configured fails, and the select must
// stay to switch back.
const EmbeddingModelNamesQuery = graphql(`
  query EmbeddingModelNames {
    embeddingModels {
      name
      minSimilarity
    }
  }
`);

// A connection's root, so a source reads as the connection's name and the path below it.
const ConnectionRootsQuery = graphql(`
  query ConnectionRoots {
    connections {
      id
      source
    }
  }
`);

type Result = SearchHitsQuery["search"];



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
  const [{ data: connections }] = useQuery({ query: ConnectionRootsQuery });
  const roots = useMemo(
    () => new Map((connections?.connections ?? []).map((connection) => [connection.id, connection.source])),
    [connections],
  );
  const hits = data?.search.hits ?? [];
  // The bars need the cut-off of the model that ran. Until the models are in, hits go without.
  const minSimilarity = models?.embeddingModels.find((model) => model.name === data?.search.model)?.minSimilarity;
  const relevance =
    minSimilarity === undefined
      ? null
      : { minSimilarity, bestKeywordScore: Math.max(0, ...hits.map((hit) => hit.keywordScore ?? 0)) };
  const client = useClient();
  const loadPage = useCallback(
    async (offset: number) => {
      const page = await client.query(SearchHitsQueryDocument, { input: { ...input, offset } }).toPromise();
      if (!page.data) throw page.error ?? new Error("the next page didn't load");
      return page.data.search;
    },
    [client, input],
  );


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

  const filtered = state.sources.length > 0 || state.authors.length > 0 || state.range !== "any";

  return (
    <div className="min-h-dvh">
      <header className="border-b border-border">
        <div className={cn(columns, "flex items-center gap-6 py-3 pr-16 lg:pr-4")}>
          <HomeLink />
          <SearchField
            defaultValue={state.q}
            onSearch={(q) => onChange({ ...state, q })}
            onType={handleType}
            className="min-w-0"
          />
        </div>
      </header>
      <div className={cn(columns, "pt-4 pb-16 lg:pt-6")}>
        <aside className="min-w-0 lg:sticky lg:top-6 lg:self-start">
          <SearchFilters
            state={state}
            facets={data?.search.facets}
            roots={roots}
            models={models?.embeddingModels.map((model) => model.name) ?? []}
            ranModel={data?.search.model}
            onChange={onChange}
          />
        </aside>
        <main className="mt-4 min-w-0 space-y-6 lg:mt-0">
          {(data || error) && (
            <div className="space-y-1">
              {data && <Stats result={data.search} sources={state.sources} className={cn(fetching && "opacity-60")} />}
              {semanticUnavailable && (
                <p role="status" className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <TriangleAlert className="size-3.5 shrink-0 text-warning" aria-hidden />
                  Keyword matches only: {semanticUnavailable}.
                </p>
              )}
            </div>
          )}
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {error.graphQLErrors[0]?.message ?? error.message}
            </p>
          ) : !data ? (
            <HitSkeletons />
          ) : hits.length === 0 ? (
            <NoMatches q={state.q} onClearFilters={filtered ? () => onChange({ ...state, sources: [], authors: [], range: "any" }) : null} />
          ) : (
            // Keyed by the search, so a hit's ✓ state never carries over into another search.
            <ol key={JSON.stringify(input)} className={cn("space-y-7 transition-opacity", fetching && "opacity-60")}>
              {hits.map((hit) => (
                <li key={hit.documentId}>
                  <HitItem hit={hit} search={input} source={sourceLabel(hit.source, hit.origin, roots)} relevance={relevance} />
                </li>
              ))}
              <MoreHits
                firstPage={hits}
                hasMore={data.search.hasMore}
                endIsCandidateLimit={data.search.totalIsLowerBound}
                loadPage={loadPage}
                renderHit={(hit) => (
                  <HitItem hit={hit} search={input} source={sourceLabel(hit.source, hit.origin, roots)} relevance={relevance} />
                )}
              />
            </ol>
          )}
        </main>
      </div>
    </div>
  );
}

/** The header lines the search field up with the hits, next to the filter column on wide screens. */
const columns = "mx-auto max-w-3xl px-4 lg:grid lg:max-w-6xl lg:grid-cols-[14rem_minmax(0,44rem)] lg:gap-x-12";

/** Placeholders shaped like hits, so the page doesn't jump when they arrive. */
function HitSkeletons() {
  return (
    <ol aria-busy="true" aria-label="Loading" className="space-y-7">
      {[0, 1, 2, 3].map((index) => (
        <li key={index} className="animate-pulse space-y-2.5">
          <div className="h-3 w-48 rounded bg-muted" />
          <div className="h-5 w-2/3 rounded bg-muted" />
          <div className="h-3.5 w-full rounded bg-muted" />
          <div className="h-3.5 w-5/6 rounded bg-muted" />
        </li>
      ))}
    </ol>
  );
}

function NoMatches({ q, onClearFilters }: { q: string; onClearFilters: (() => void) | null }) {
  return (
    <div className="flex flex-col items-center gap-3 py-16 text-center">
      <span className="flex size-12 items-center justify-center rounded-full bg-muted text-muted-foreground">
        <SearchX className="size-6" aria-hidden />
      </span>
      <p className="font-medium">No documents match “{q}”</p>
      <p className="max-w-sm text-sm text-muted-foreground">
        {onClearFilters ? "The filters may be too narrow." : "Try fewer or other words: every word has to match, or the meaning has to be close."}
      </p>
      {onClearFilters && (
        <button type="button" onClick={onClearFilters} className="rounded-full border border-border px-4 py-1.5 text-sm hover:bg-accent">
          Clear filters
        </button>
      )}
    </div>
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
