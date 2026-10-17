"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Hit } from "@/components/search-hit";

/** How far below the viewport the next page starts loading, so scrolling rarely waits for it. */
const PRELOAD_MARGIN = "600px";

type Page = { hits: Hit[]; hasMore: boolean };

/**
 * The hits after the first page, loaded when the end of the list scrolls into view. Keyed by the
 * search, so a new search starts over and a late page of the old one lands nowhere.
 */
export function MoreHits({
  firstPage,
  hasMore: firstHasMore,
  endIsCandidateLimit,
  loadPage,
  renderHit,
}: {
  firstPage: Hit[];
  hasMore: boolean;
  /** Whether the search stopped at its candidate limits rather than at the end of the index. */
  endIsCandidateLimit: boolean;
  /** The page of the ranking that starts at `offset`. */
  loadPage: (offset: number) => Promise<Page>;
  renderHit: (hit: Hit) => ReactNode;
}) {
  const [more, setMore] = useState<Hit[]>([]);
  const [hasMore, setHasMore] = useState(firstHasMore);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const end = useRef<HTMLLIElement>(null);

  const offset = firstPage.length + more.length;
  useEffect(() => {
    if (!hasMore || loading || error || !end.current) return;
    const observer = new IntersectionObserver(
      async ([entry]) => {
        if (!entry.isIntersecting) return;
        observer.disconnect();
        setLoading(true);
        try {
          const page = await loadPage(offset);
          // The ranking is stable, but the index may change between pages: never show a hit twice.
          setMore((shown) => {
            const seen = new Set([...firstPage, ...shown].map((hit) => hit.documentId));
            return [...shown, ...page.hits.filter((hit) => !seen.has(hit.documentId))];
          });
          setHasMore(page.hasMore);
        } catch (failure) {
          setError(failure instanceof Error ? failure.message : String(failure));
        } finally {
          setLoading(false);
        }
      },
      { rootMargin: PRELOAD_MARGIN },
    );
    observer.observe(end.current);
    return () => observer.disconnect();
  }, [hasMore, loading, error, offset, firstPage, loadPage]);

  return (
    <>
      {more.map((hit) => (
        <li key={hit.documentId}>{renderHit(hit)}</li>
      ))}
      {hasMore && !loading && !error ? (
        <li ref={end} aria-hidden />
      ) : (
        <li ref={end} className="flex items-center gap-3 pt-2 text-xs text-muted-foreground">
          <span className="h-px flex-1 bg-border" />
          {error ? (
            <span role="alert" className="text-destructive">
              {error}
            </span>
          ) : loading ? (
            <span aria-live="polite">Loading more…</span>
          ) : (
            <span>{endIsCandidateLimit ? "That's as deep as this search goes" : "That's all"}</span>
          )}
          <span className="h-px flex-1 bg-border" />
        </li>
      )}
    </>
  );
}
