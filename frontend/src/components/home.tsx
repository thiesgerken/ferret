"use client";

import { useTranslations } from "next-intl";
import { useQuery } from "urql";
import { BrandMark } from "@/components/brand-mark";
import { SearchField } from "@/components/search-field";
import { graphql } from "@/gql";

// Explore without filters counts what the user can read, and its source facet names each
// source's writer (a connection or an API key). Every model counts the same chunks.
const IndexSummaryQuery = graphql(`
  query IndexSummary {
    explore(input: { limit: 1 }) {
      total
      facets {
        sources {
          value
          writer {
            id
          }
        }
      }
    }
    embeddingModels {
      chunks
    }
  }
`);

/** The start page: the mark, the search field and how much there is to search. */
export function Home({ onSearch }: { onSearch: (q: string) => void }) {
  return (
    <main className="flex min-h-dvh flex-col items-center bg-[radial-gradient(ellipse_70%_45%_at_50%_0%,color-mix(in_oklch,var(--brand)_14%,transparent),transparent)] px-4 pt-[16vh]">
      <BrandMark className="h-24 w-auto drop-shadow-sm select-none" />
      <h1 className="mt-4 font-wordmark text-5xl font-semibold text-brand select-none">ferret</h1>
      <SearchField onSearch={onSearch} className="mt-10" />
      <IndexSummary />
    </main>
  );
}

function IndexSummary() {
  const t = useTranslations("home");
  const [{ data }] = useQuery({ query: IndexSummaryQuery });
  if (!data) return <p className="mt-5 h-4" />;
  const documents = data.explore.total;
  // Sources without a known writer (from before keys were recorded) count as one more.
  const sources = new Set(data.explore.facets.sources.map((source) => source.writer?.id ?? null)).size;
  const chunks = data.embeddingModels[0].chunks;
  return <p className="mt-5 h-4 text-xs text-muted-foreground tabular-nums">{t("summary", { documents, sources, chunks })}</p>;
}
