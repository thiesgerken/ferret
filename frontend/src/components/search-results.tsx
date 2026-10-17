"use client";

import { ExternalLink } from "lucide-react";
import Link from "next/link";
import { useMemo } from "react";
import { useQuery } from "urql";
import { SearchField } from "@/components/search-field";
import { graphql } from "@/gql";
import type { SearchHitsQuery } from "@/gql/graphql";
import { linkableUrl } from "@/lib/safe-url";
import { rangeStart, TIME_RANGES, toggle, type SearchState, type TimeRange } from "@/lib/search-url";
import { snippet } from "@/lib/snippet";
import { cn } from "@/lib/utils";

const SearchHitsQueryDocument = graphql(`
  query SearchHits($input: SearchInput!) {
    search(input: $input) {
      hits {
        documentId
        source
        externalId
        title
        url
        authors
        createdAt
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

type Hit = SearchHitsQuery["search"]["hits"][number];

/** How many authors get a chip, most frequent among the hits first. */
const MAX_AUTHOR_CHIPS = 8;

export function SearchResults({ state, onChange }: { state: SearchState; onChange: (state: SearchState) => void }) {
  const from = useMemo(() => rangeStart(state.range, new Date()), [state.range]);
  const input = useMemo(
    () => ({
      q: state.q,
      sources: state.sources.length > 0 ? state.sources : null,
      authors: state.authors.length > 0 ? state.authors : null,
      from,
      model: state.model,
    }),
    [state.q, state.sources, state.authors, from, state.model],
  );
  const [{ data, error, fetching }] = useQuery({ query: SearchHitsQueryDocument, variables: { input } });
  const [{ data: models }] = useQuery({ query: EmbeddingModelNamesQuery });
  const hits = data?.search.hits ?? [];
  const semanticUnavailable = data?.search.semanticUnavailable;

  return (
    <div className="min-h-dvh">
      <header className="flex items-center gap-6 border-b border-border px-4 py-3 pr-16">
        <Link href="/" className="text-2xl font-semibold tracking-tight text-brand select-none">
          ferret
        </Link>
        <SearchField defaultValue={state.q} onSearch={(q) => onChange({ ...state, q })} />
      </header>
      <main className="mx-auto max-w-3xl space-y-6 px-4 py-4">
        <FilterChips
          state={state}
          hits={hits}
          models={models?.embeddingModels.map((model) => model.name) ?? []}
          ranModel={data?.search.model}
          onChange={onChange}
        />
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
          <ol className={cn("space-y-6", fetching && "opacity-60")}>
            {hits.map((hit) => (
              <li key={hit.documentId}>
                <HitItem hit={hit} />
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

function HitItem({ hit }: { hit: Hit }) {
  const viewerHref = `/view?${new URLSearchParams({
    id: hit.documentId,
    start: String(hit.chunk.startOffset),
    end: String(hit.chunk.endOffset),
  })}`;
  const originUrl = linkableUrl(hit.url);
  return (
    <article className="space-y-1">
      <p className="truncate text-xs text-muted-foreground">
        {hit.source} · {hit.authors.join(", ")} · {new Date(hit.createdAt).toLocaleDateString()}
      </p>
      <Link href={viewerHref} className="text-lg font-medium text-brand hover:underline">
        {hit.title || hit.externalId}
      </Link>
      <p className="text-sm leading-relaxed">
        {snippet(hit.chunk.parts).map((part, index) =>
          part.highlight ? (
            <mark key={index} className="rounded bg-brand/20 px-0.5 text-foreground">
              {part.text}
            </mark>
          ) : (
            <span key={index}>{part.text}</span>
          ),
        )}
      </p>
      {originUrl && (
        <a href={originUrl} className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:underline">
          <ExternalLink className="size-3" aria-hidden />
          Open in origin
        </a>
      )}
    </article>
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
