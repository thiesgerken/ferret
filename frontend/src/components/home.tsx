"use client";

import { useQuery } from "urql";
import { BrandMark } from "@/components/brand-mark";
import { SearchField } from "@/components/search-field";
import { graphql } from "@/gql";

// Explore without filters counts what the user can read, and its source facet lists the sources.
const IndexSummaryQuery = graphql(`
  query IndexSummary {
    explore(input: { limit: 1 }) {
      total
      facets {
        sources {
          value
        }
      }
    }
  }
`);

/** The start page: the mark, the search field and how much there is to search. */
export function Home({ onSearch }: { onSearch: (q: string) => void }) {
  return (
    <main className="flex min-h-dvh flex-col items-center bg-[radial-gradient(ellipse_70%_45%_at_50%_0%,color-mix(in_oklch,var(--brand)_14%,transparent),transparent)] px-4 pt-[16vh]">
      <BrandMark className="h-24 w-auto drop-shadow-sm select-none" />
      <h1 className="mt-4 text-5xl font-semibold tracking-tight text-brand select-none">ferret</h1>
      <p className="mt-3 text-center text-muted-foreground">Your notes, documents, mail and chats in one search.</p>
      <SearchField onSearch={onSearch} placeholder="Try “the invoice for the heating repair”" className="mt-10" />
      <IndexSummary />
    </main>
  );
}

function IndexSummary() {
  const [{ data }] = useQuery({ query: IndexSummaryQuery });
  if (!data) return <p className="mt-5 h-4" />;
  const documents = data.explore.total;
  // ponytail: a source's first path segment stands for its connection or API key, until #85's
  // origins are in the API.
  const sources = new Set(data.explore.facets.sources.map((source) => source.value.split("/")[0])).size;
  return (
    <p className="mt-5 h-4 text-xs text-muted-foreground tabular-nums">
      {documents.toLocaleString()} {documents === 1 ? "document" : "documents"} · {sources}{" "}
      {sources === 1 ? "source" : "sources"}
    </p>
  );
}
