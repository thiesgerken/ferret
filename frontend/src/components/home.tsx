"use client";

import { useTranslations } from "next-intl";
import { useQuery } from "urql";
import { BrandMark } from "@/components/brand-mark";
import { SearchField } from "@/components/search-field";
import { graphql } from "@/gql";

const IndexSummaryQuery = graphql(`
  query IndexSummary {
    indexSummary {
      documents
      chunks
      sources
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
  const { documents, sources, chunks } = data.indexSummary;
  return <p className="mt-5 h-4 text-xs text-muted-foreground tabular-nums">{t("summary", { documents, sources, chunks })}</p>;
}
