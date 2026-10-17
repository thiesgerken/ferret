"use client";

import { Circle, Info } from "lucide-react";
import type { ActivityOverviewQuery, EmbedderStatusQuery, TeiRole } from "@/gql/graphql";
import { formatDuration } from "@/lib/format-time";
import { cn } from "@/lib/utils";

type Model = ActivityOverviewQuery["embeddingModels"][number];
type GlobalModel = EmbedderStatusQuery["embeddingStatus"][number];
type Tei = GlobalModel["teis"][number];

const ROLES: Record<TeiRole, string> = {
  QUERIES: "Queries",
  PUMP: "Pump",
  BOTH: "Queries and pump",
};

/**
 * Each embedding model's progress over the user's chunks: how much is embedded, how fast the pump
 * goes and when it's done. Admins also see the whole index and every TEI behind the model.
 */
export function Embedders({ models, global }: { models: Model[]; global: GlobalModel[] | undefined }) {
  return (
    <div className="space-y-4">
      {models.map((model) => (
        <ModelProgress key={model.name} model={model} global={global?.find(({ name }) => name === model.name)} />
      ))}
    </div>
  );
}

function ModelProgress({ model, global }: { model: Model; global: GlobalModel | undefined }) {
  const share = model.chunks === 0 ? 1 : model.embedded / model.chunks;
  return (
    <div className="space-y-4 rounded-xl border border-border p-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <p className="font-medium">
          {model.name}
          {model.configuredDefault && <span className="text-xs font-normal text-muted-foreground"> · default</span>}
        </p>
        <p className="flex items-center gap-3 text-xs text-muted-foreground">
          <Health label="Queries" online={model.online} />
          {model.bulkOnline !== null && model.bulkOnline !== undefined && <Health label="Pump" online={model.bulkOnline} />}
        </p>
      </div>
      <div>
        <div
          role="progressbar"
          aria-label={`${model.name} embedded`}
          aria-valuenow={Math.floor(share * 100)}
          className="h-2 overflow-hidden rounded-full bg-muted"
        >
          <div className="h-full rounded-full bg-brand transition-[width] duration-700" style={{ width: `${share * 100}%` }} />
        </div>
        <p className="mt-1.5 flex flex-wrap justify-between gap-x-4 text-xs text-muted-foreground tabular-nums">
          <span>
            {model.embedded.toLocaleString()} of {model.chunks.toLocaleString()} chunks embedded ({Math.floor(share * 100)}%)
          </span>
          <span>
            {model.pending > 0
              ? `${model.pending.toLocaleString()} pending${model.etaSeconds != null ? `, done in about ${formatDuration(model.etaSeconds)}` : ""}`
              : "Everything is embedded"}
          </span>
        </p>
      </div>
      <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4">
        <Stat label="Rate" value={`${Math.round(model.ratePerMinute).toLocaleString()} / min`} hint="Across users: the pump is shared." />
        <Stat label="Batch size" value={String(model.batchSize)} hint="Chunks per pump batch. It shrinks while TEI is too slow for full ones." />
        <Stat label="Pending" value={model.pending.toLocaleString()} />
        <Stat label="Refused" value={model.failed.toLocaleString()} hint="Chunks TEI refused. The pump leaves them out." warn={model.failed > 0} />
      </dl>
      {global && <GlobalProgress model={global} />}
    </div>
  );
}

/** Admins only: the whole index, the pump's last failure and every TEI behind the model. */
function GlobalProgress({ model }: { model: GlobalModel }) {
  return (
    <div className="space-y-3 border-t border-border pt-4">
      <p className="text-xs text-muted-foreground tabular-nums">
        All users: {model.embedded.toLocaleString()} of {model.chunks.toLocaleString()} chunks embedded
        {model.etaSeconds != null && `, done in about ${formatDuration(model.etaSeconds)}`}
      </p>
      {model.lastError && (
        <p className="text-sm text-destructive">
          Last error{model.lastErrorAt && ` (${new Date(model.lastErrorAt).toLocaleString()})`}: {model.lastError}
        </p>
      )}
      <div className="grid gap-3 sm:grid-cols-2">
        {model.teis.map((tei) => (
          <TeiCard key={tei.url} tei={tei} />
        ))}
      </div>
    </div>
  );
}

function TeiCard({ tei }: { tei: Tei }) {
  const number = (value: number | null | undefined, format: (value: number) => string) =>
    value === null || value === undefined ? "–" : format(value);
  return (
    <div className="space-y-3 rounded-lg bg-muted/50 p-3 text-sm">
      <div className="flex items-center justify-between gap-2">
        <span className="font-medium">{ROLES[tei.role]}</span>
        <Health online={tei.online} />
      </div>
      <p className="truncate font-mono text-xs text-muted-foreground" title={tei.url}>
        {tei.url}
      </p>
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
      <dl className="grid grid-cols-2 gap-x-3 gap-y-1.5 text-xs">
        <TeiFigure label="Queue" value={number(tei.queueSize, String)} />
        <TeiFigure label="Batch" value={number(tei.batchSize, (size) => size.toFixed(1))} />
        <TeiFigure label="Throughput" value={number(tei.throughputPerMinute, (rate) => `${Math.round(rate)} / min`)} />
        <TeiFigure label="Failures" value={number(tei.failures, String)} />
        {tei.role !== "QUERIES" && <TeiFigure label="Backlog done in" value={number(tei.etaSeconds, formatDuration)} />}
      </dl>
    </div>
  );
}

function TeiFigure({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="text-right tabular-nums">{value}</dd>
    </>
  );
}

function Stat({ label, value, hint, warn }: { label: string; value: string; hint?: string; warn?: boolean }) {
  return (
    <div title={hint}>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className={cn("font-medium tabular-nums", warn && "text-destructive")}>{value}</dd>
    </div>
  );
}

/** A green or red dot with what it's for, or just "Online"/"Offline" without a label. */
function Health({ label, online }: { label?: string; online: boolean }) {
  const state = online ? "online" : "offline";
  return (
    <span className="inline-flex items-center gap-1.5">
      <Circle className={cn("size-2", online ? "fill-emerald-500 text-emerald-500" : "fill-destructive text-destructive")} aria-hidden />
      {label ? (
        <>
          {label}
          <span className="sr-only"> {state}</span>
        </>
      ) : (
        state[0].toUpperCase() + state.slice(1)
      )}
    </span>
  );
}
