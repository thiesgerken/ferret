"use client";

import Link from "next/link";
import { useMemo, type ReactNode } from "react";
import { useQuery } from "urql";
import { graphql } from "@/gql";
import type { ContentType, DocumentOrder, ExploreDocumentsQuery } from "@/gql/graphql";
import { errorMessage } from "@/lib/graphql";
import { PAGE_SIZE, monthRange, type ExploreState } from "@/lib/explore-url";
import { NamedIcon } from "@/components/named-icon";
import { useConnectionRoots } from "@/lib/connection-roots";
import { toggle } from "@/lib/search-url";
import { sourceLabel } from "@/lib/source-label";
import { SourceName } from "@/components/source-name";
import { cn } from "@/lib/utils";
import { viewerHref } from "@/lib/viewer-url";

const ExploreDocumentsQueryDocument = graphql(`
  query ExploreDocuments($input: ExploreInput!) {
    explore(input: $input) {
      total
      documents {
        id
        source
        writer {
          kind
          id
          name
          icon
        }
        externalId
        contentType
        title
        authors
        createdAt
        modifiedAt
      }
      facets {
        sources {
          value
          count
          writer {
            kind
            id
            name
            icon
          }
        }
        contentTypes {
          contentType
          count
        }
        authors {
          value
          count
        }
        months {
          value
          count
        }
      }
    }
  }
`);

type Exploration = ExploreDocumentsQuery["explore"];

const CONTENT_TYPE_LABELS: Record<ContentType, string> = {
  MARKDOWN: "Markdown",
  PLAIN: "Plain text",
  EMAIL: "Email",
};

const COLUMNS: { label: string; order: DocumentOrder | null }[] = [
  { label: "Title", order: "TITLE" },
  { label: "Source", order: "SOURCE" },
  { label: "Type", order: null },
  { label: "Authors", order: null },
  { label: "Created", order: "CREATED_AT" },
  { label: "Modified", order: "MODIFIED_AT" },
];

export function Explore({ state, onChange }: { state: ExploreState; onChange: (state: ExploreState) => void }) {
  const input = useMemo(
    () => ({
      sources: state.sources.length > 0 ? state.sources : null,
      contentTypes: state.contentTypes.length > 0 ? state.contentTypes : null,
      authors: state.authors.length > 0 ? state.authors : null,
      ...(state.month ? monthRange(state.month) : {}),
      orderBy: state.orderBy,
      descending: state.descending,
      offset: state.page * PAGE_SIZE,
      limit: PAGE_SIZE,
    }),
    [state],
  );
  const [{ data, error, fetching }] = useQuery({ query: ExploreDocumentsQueryDocument, variables: { input } });
  const roots = useConnectionRoots();
  // Any change but paging starts over at the first page.
  const narrow = (change: Partial<ExploreState>) => onChange({ ...state, ...change, page: 0 });

  if (error) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {errorMessage(error)}
      </p>
    );
  }
  if (!data) {
    return null;
  }
  const { total, documents, facets } = data.explore;
  const writerOf = (source: string) => facets.sources.find((facet) => facet.value === source)?.writer ?? null;

  return (
    <div className="grid gap-8 md:grid-cols-[14rem_1fr]">
      <aside className="space-y-6">
        <FacetList
          title="Sources"
          values={withSelected(facets.sources, state.sources)}
          selected={state.sources}
          label={(source) => sourceLabel(source, writerOf(source), roots).text}
          icon={(source) => <NamedIcon name={writerOf(source)?.icon ?? "circle-dashed"} className="size-3.5 shrink-0" />}
          onToggle={(source) => narrow({ sources: toggle(state.sources, source) })}
        />
        <FacetList
          title="Type"
          values={withSelected(
            facets.contentTypes.map(({ contentType, count }) => ({ value: contentType, count })),
            state.contentTypes,
          )}
          selected={state.contentTypes}
          label={(value) => CONTENT_TYPE_LABELS[value as ContentType]}
          onToggle={(type) => narrow({ contentTypes: toggle(state.contentTypes, type) as ContentType[] })}
        />
        <FacetList
          title="Authors"
          values={withSelected(facets.authors, state.authors)}
          selected={state.authors}
          onToggle={(author) => narrow({ authors: toggle(state.authors, author) })}
        />
        <FacetList
          title="Month"
          values={withSelected(facets.months, state.month ? [state.month] : [])}
          selected={state.month ? [state.month] : []}
          onToggle={(month) => narrow({ month: month === state.month ? null : month })}
        />
      </aside>
      <section className={cn("min-w-0 space-y-4", fetching && "opacity-60")}>
        {documents.length === 0 ? (
          <p className="text-sm text-muted-foreground">No documents match.</p>
        ) : (
          <div className="overflow-x-auto">
            <DocumentTable documents={documents} roots={roots} state={state} onSort={narrow} />
          </div>
        )}
        <Pager total={total} state={state} onChange={onChange} />
      </section>
    </div>
  );
}

/** The facet's values, plus selected ones the counts no longer have, so they can be unselected. */
function withSelected(values: { value: string; count: number }[], selected: string[]) {
  const missing = selected.filter((value) => !values.some((facet) => facet.value === value));
  return [...values, ...missing.map((value) => ({ value, count: 0 }))];
}

function FacetList({
  title,
  values,
  selected,
  label = (value) => value,
  icon,
  onToggle,
}: {
  title: string;
  values: { value: string; count: number }[];
  selected: string[];
  label?: (value: string) => string;
  icon?: (value: string) => ReactNode;
  onToggle: (value: string) => void;
}) {
  if (values.length === 0) {
    return null;
  }
  return (
    <div className="space-y-1">
      <h2 className="text-xs font-medium text-muted-foreground">{title}</h2>
      <ul>
        {values.map(({ value, count }) => {
          const pressed = selected.includes(value);
          return (
            <li key={value}>
              <button
                type="button"
                aria-pressed={pressed}
                onClick={() => onToggle(value)}
                className={cn(
                  "flex w-full items-center justify-between gap-2 rounded-md px-2 py-1 text-left text-sm hover:bg-accent",
                  pressed && "bg-accent font-medium",
                )}
              >
                <span className="flex min-w-0 items-center gap-2" title={icon ? value : undefined}>
                  {icon?.(value)}
                  <span className="truncate">{label(value)}</span>
                </span>
                <span className="text-xs text-muted-foreground">{count}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function DocumentTable({
  documents,
  roots,
  state,
  onSort,
}: {
  documents: Exploration["documents"];
  roots: Map<string, string> | undefined;
  state: ExploreState;
  onSort: (change: Partial<ExploreState>) => void;
}) {
  return (
    <table className="w-full text-left text-sm">
      <thead className="border-b border-border text-xs text-muted-foreground">
        <tr>
          {COLUMNS.map(({ label, order }) => {
            const sorted = order === state.orderBy;
            return (
              <th
                key={label}
                scope="col"
                aria-sort={sorted ? (state.descending ? "descending" : "ascending") : undefined}
                className="px-2 py-2 font-medium whitespace-nowrap"
              >
                {order ? (
                  <button
                    type="button"
                    // A new column starts in its natural direction: newest dates, A to Z.
                    onClick={() =>
                      onSort({
                        orderBy: order,
                        descending: sorted ? !state.descending : order === "CREATED_AT" || order === "MODIFIED_AT",
                      })
                    }
                    className={cn("hover:text-foreground", sorted && "text-foreground")}
                  >
                    {label} {sorted && (state.descending ? "↓" : "↑")}
                  </button>
                ) : (
                  label
                )}
              </th>
            );
          })}
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {documents.map((document) => (
          <tr key={document.id}>
            <td className="max-w-xs truncate px-2 py-2">
              <Link
                href={viewerHref(document.id)}
                className="text-brand hover:underline"
              >
                {document.title || document.externalId}
              </Link>
            </td>
            <td className="max-w-56 px-2 py-2 whitespace-nowrap">
              <SourceName source={document.source} writer={document.writer} roots={roots} />
            </td>
            <td className="px-2 py-2 whitespace-nowrap">{CONTENT_TYPE_LABELS[document.contentType]}</td>
            <td className="max-w-48 truncate px-2 py-2">{document.authors.join(", ")}</td>
            <td className="px-2 py-2 whitespace-nowrap">{formatDate(document.createdAt)}</td>
            <td className="px-2 py-2 whitespace-nowrap">{formatDate(document.modifiedAt)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Pager({
  total,
  state,
  onChange,
}: {
  total: number;
  state: ExploreState;
  onChange: (state: ExploreState) => void;
}) {
  const first = state.page * PAGE_SIZE;
  const last = Math.min(first + PAGE_SIZE, total);
  const buttonClass = "rounded-md px-2 py-1 hover:bg-accent disabled:opacity-50 disabled:hover:bg-transparent";
  return (
    <nav aria-label="Pages" className="flex items-center justify-between text-sm text-muted-foreground">
      <span>{total === 0 ? "0 documents" : `${first + 1}–${last} of ${total} documents`}</span>
      <span className="flex gap-1">
        <button
          type="button"
          disabled={state.page === 0}
          onClick={() => onChange({ ...state, page: state.page - 1 })}
          className={buttonClass}
        >
          Previous
        </button>
        <button
          type="button"
          disabled={last >= total}
          onClick={() => onChange({ ...state, page: state.page + 1 })}
          className={buttonClass}
        >
          Next
        </button>
      </span>
    </nav>
  );
}

/** In UTC, like the month facet, so a document shows the date of the month it's counted in. */
function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, { dateStyle: "medium", timeZone: "UTC" });
}
