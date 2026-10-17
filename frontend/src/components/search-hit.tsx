"use client";

import { Check, ExternalLink, FileText, GitBranch, KeyRound, Plug, Shell, type LucideIcon } from "lucide-react";
import Link from "next/link";
import { useState } from "react";
import { useMutation } from "urql";
import { graphql } from "@/gql";
import type { SearchHitsQuery, SearchInput } from "@/gql/graphql";
import { RELEVANCE_BARS, relevanceBars, relevanceLabel } from "@/lib/relevance";
import { linkableUrl } from "@/lib/safe-url";
import { plainMarkdown, snippet } from "@/lib/snippet";
import { viewerHref } from "@/lib/viewer-url";
import { cn } from "@/lib/utils";

const SetJudgmentMutation = graphql(`
  mutation SetJudgment($input: JudgmentInput!) {
    setJudgment(input: $input)
  }
`);

export type Hit = SearchHitsQuery["search"]["hits"][number];

export type RelevanceContext = { minSimilarity: number; bestKeywordScore: number };

const PLUGIN_ICONS: Record<string, LucideIcon> = {
  carapace: Shell,
  git: GitBranch,
  paperless: FileText,
};

/** Shown on hover only, where there is hover: on a touch screen they stay. */
const onHover = "transition-opacity [@media(hover:hover)]:opacity-0 group-hover:opacity-100 group-focus-within:opacity-100";

/** One hit: relevance and where it lives, the title into the viewer, the snippet. */
export function HitItem({
  hit,
  search,
  plugin,
  relevance,
}: {
  hit: Hit;
  search: SearchInput;
  /** The plugin that owns the hit's source, null when an API key filled it. */
  plugin: string | null;
  relevance: RelevanceContext;
}) {
  const originUrl = linkableUrl(hit.url);
  const meta = [
    hit.authors.join(", "),
    new Date(hit.createdAt).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }),
  ].filter(Boolean);

  return (
    <article className="group">
      <div className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
        <RelevanceBars hit={hit} context={relevance} />
        <SourceIcon plugin={plugin} />
        <p className="min-w-0 truncate">
          <span className="text-foreground/80">{hit.source.split("/").join(" › ")}</span>
          {meta.map((part) => ` · ${part}`)}
        </p>
      </div>
      <div className="mt-1 flex items-start gap-2">
        <Link
          href={viewerHref(hit.documentId, { start: hit.chunk.startOffset, end: hit.chunk.endOffset })}
          className="text-lg leading-snug font-medium text-brand decoration-brand/40 underline-offset-4 hover:underline"
        >
          {hit.title || hit.externalId}
        </Link>
        <div className="flex shrink-0 items-center gap-1 pt-1">
          <JudgeButton documentId={hit.documentId} judged={hit.judged} search={search} />
          {originUrl && (
            <a
              href={originUrl}
              aria-label="Open in origin"
              title="Open in origin"
              className={cn("rounded-full p-1 text-muted-foreground hover:bg-accent hover:text-foreground", onHover)}
            >
              <ExternalLink className="size-3.5" aria-hidden />
            </a>
          )}
        </div>
      </div>
      <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
        {snippet(hit.contentType === "MARKDOWN" ? plainMarkdown(hit.chunk.parts) : hit.chunk.parts).map((part, index) =>
          part.highlight ? (
            <mark key={index} className="bg-transparent font-semibold text-foreground">
              {part.text}
            </mark>
          ) : (
            <span key={index}>{part.text}</span>
          ),
        )}
      </p>
      {hit.collapsed > 0 && (
        <p className="mt-1 text-xs text-muted-foreground">
          {hit.collapsed} more in this thread
        </p>
      )}
    </article>
  );
}

/**
 * Four signal bars instead of numbers: the document's best similarity (not necessarily the
 * snippet's), in the brand color when every word matched as well, grey when only the meaning did.
 * The exact values are in the tooltip.
 */
function RelevanceBars({ hit, context }: { hit: Hit; context: RelevanceContext }) {
  const filled = relevanceBars(hit, context);
  const label = relevanceLabel(hit);
  return (
    <span
      tabIndex={0}
      aria-label={label}
      className={cn(
        "group/bars relative inline-flex h-3.5 shrink-0 items-end gap-[2px] rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring",
        hit.keywordScore != null ? "text-brand" : "text-muted-foreground",
      )}
    >
      {Array.from({ length: RELEVANCE_BARS }, (_, index) => (
        <span
          key={index}
          className={cn("w-[3px] rounded-[1px]", index < filled ? "bg-current" : "bg-border")}
          style={{ height: `${((index + 1) / RELEVANCE_BARS) * 100}%` }}
        />
      ))}
      <span
        role="tooltip"
        aria-hidden
        className="pointer-events-none absolute top-full left-0 z-10 mt-1.5 hidden rounded-md bg-foreground px-2 py-1 text-xs whitespace-nowrap text-background shadow-md group-hover/bars:block group-focus-visible/bars:block"
      >
        {label}
      </span>
    </span>
  );
}

function SourceIcon({ plugin }: { plugin: string | null }) {
  const Icon = plugin === null ? KeyRound : (PLUGIN_ICONS[plugin] ?? Plug);
  return (
    <span
      title={plugin ?? "API"}
      className="inline-flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground"
    >
      <Icon className="size-3" aria-hidden />
    </span>
  );
}

/** ✓ marks the hit as the right one for this search, for evaluating embedding models. */
function JudgeButton({ documentId, judged: initial, search }: { documentId: string; judged: boolean; search: SearchInput }) {
  const [judged, setJudged] = useState(initial);
  const [{ fetching, error }, setJudgment] = useMutation(SetJudgmentMutation);

  async function toggle() {
    const result = await setJudgment({ input: { search, documentId, judged: !judged } });
    if (result.data) setJudged(result.data.setJudgment);
  }

  return (
    <>
      <button
        type="button"
        aria-pressed={judged}
        aria-label="Right hit for this search"
        title={judged ? "Marked as the right hit for this search" : "Mark as the right hit for this search"}
        disabled={fetching}
        onClick={toggle}
        className={cn(
          "rounded-full border p-0.5",
          judged ? "border-brand bg-brand text-white" : cn("border-border text-muted-foreground hover:text-foreground", onHover),
        )}
      >
        <Check className="size-3.5" aria-hidden />
      </button>
      {error && (
        <span role="alert" className="text-xs text-destructive">
          {error.graphQLErrors[0]?.message ?? error.message}
        </span>
      )}
    </>
  );
}
