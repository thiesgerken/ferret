"use client";

import { Info } from "lucide-react";
import type { ReactNode } from "react";
import { graphql, useFragment, type FragmentType } from "@/gql";
import type { TeiState } from "@/gql/graphql";
import { formatDuration } from "@/lib/format-time";
import { cn } from "@/lib/utils";

export const TeiCardFields = graphql(`
  fragment TeiCardFields on Tei {
    url
    external
    enabled
    queries
    embedding
    state
    problem
    lastError
    lastErrorAt
    busy
    pumpBatchSize
    queueSize
    batchSize
    throughputPerMinute
    failures
    load
    etaSeconds
  }
`);

export const TEI_STATES: Record<TeiState, { label: string; className: string }> = {
  ONLINE: { label: "Online", className: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400" },
  OFFLINE: { label: "Offline", className: "bg-destructive/15 text-destructive" },
  UNUSED: { label: "Unused", className: "bg-warning/20 text-warning-foreground dark:text-warning" },
  UNSUITABLE: { label: "Unsuitable", className: "bg-warning/20 text-warning-foreground dark:text-warning" },
};

/**
 * One TEI of the pool: its state, what it's used for and its figures. `controls` replaces the line
 * of what it's used for, e.g. with switches on the Admin page.
 */
export function TeiCard({ tei: fragment, controls }: { tei: FragmentType<typeof TeiCardFields>; controls?: ReactNode }) {
  const tei = useFragment(TeiCardFields, fragment);
  const state = TEI_STATES[tei.state];
  const number = (value: number | null | undefined, format: (value: number) => string) =>
    value === null || value === undefined ? "–" : format(value);
  const uses = [tei.queries && "Queries", tei.embedding && "Embedding"].filter(Boolean).join(" · ");
  // An unused or unreachable TEI has no figures worth a row of dashes.
  const measured = [tei.load, tei.queueSize, tei.batchSize, tei.throughputPerMinute, tei.failures].some((value) => value != null);
  return (
    <div className={cn("space-y-3 rounded-lg bg-muted/50 p-3 text-sm", !tei.enabled && "opacity-70")}>
      <div className="flex items-start justify-between gap-2">
        <p className="min-w-0 truncate font-mono text-xs leading-5" title={tei.url}>
          {tei.url}
        </p>
        <span className="flex shrink-0 items-center gap-1.5">
          {tei.external && <span className="rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground">External</span>}
          <span className={cn("rounded-full px-2 py-0.5 text-xs font-medium", state.className)}>{state.label}</span>
        </span>
      </div>
      {tei.problem && <p className="text-xs text-warning-foreground first-letter:uppercase dark:text-warning">{tei.problem}</p>}
      {controls ?? (
        <p className="text-xs text-muted-foreground">{!tei.enabled ? "Switched off" : uses || "Neither queries nor embedding"}</p>
      )}
      {tei.load != null && (
        <div>
          <p className="flex items-center gap-1 text-xs text-muted-foreground">
            Load
            <span title="Share of the last minute TEI spent in inference, a stand-in for its CPU load. It can exceed 100% with batches in parallel. Real CPU usage is the cluster's to measure (metrics-server, Prometheus). Behind a Service with several pods, this is the whole Service.">
              <Info className="size-3" aria-label="What load means" />
            </span>
            <span className="ml-auto tabular-nums">{number(tei.load, (load) => `${Math.round(load * 100)}%`)}</span>
          </p>
          <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-background">
            <div
              className={cn("h-full rounded-full", (tei.load ?? 0) > 0.9 ? "bg-warning" : "bg-brand")}
              style={{ width: `${Math.min(tei.load ?? 0, 1) * 100}%` }}
            />
          </div>
        </div>
      )}
      {measured && (
        <dl className="grid grid-cols-2 gap-x-3 gap-y-1.5 text-xs">
          <Figure label="Queue" value={number(tei.queueSize, String)} />
          <Figure label="Batch" value={number(tei.batchSize, (size) => size.toFixed(1))} />
          <Figure label="Throughput" value={number(tei.throughputPerMinute, (rate) => `${Math.round(rate)} / min`)} />
          <Figure label="Failures" value={number(tei.failures, String)} />
          {tei.embedding && tei.state === "ONLINE" && (
            <>
              <Figure label="Pump batch" value={number(tei.pumpBatchSize, (size) => `${size}${tei.busy ? " · busy" : ""}`)} />
              <Figure label="Backlog done in" value={number(tei.etaSeconds, formatDuration)} />
            </>
          )}
        </dl>
      )}
      {tei.lastError && (
        <p className="text-xs text-destructive">
          Last error{tei.lastErrorAt && ` (${new Date(tei.lastErrorAt).toLocaleString()})`}: {tei.lastError}
        </p>
      )}
    </div>
  );
}

function Figure({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="text-right tabular-nums">{value}</dd>
    </>
  );
}
