"use client";

import { Circle } from "lucide-react";
import { TeiCard } from "@/components/tei-card";
import type { ActivityOverviewQuery, EmbedderStatusQuery } from "@/gql/graphql";
import { formatDuration } from "@/lib/format-time";
import { cn } from "@/lib/utils";

type Model = ActivityOverviewQuery["embeddingModels"][number];
type GlobalModel = EmbedderStatusQuery["embeddingStatus"][number];
type Tei = EmbedderStatusQuery["teis"][number];

/** What admins see beyond their own chunks: the whole index, and the TEIs of the pool. */
type Global = { models: GlobalModel[]; teis: Tei[] };

/**
 * Each embedding model's progress over the user's chunks: how much is embedded, how fast the pump
 * goes and when it's done. Admins also see the whole index and every TEI serving the model.
 */
export function Embedders({ models, global }: { models: Model[]; global: Global | undefined }) {
  return (
    <div className="space-y-4">
      {models.map((model) => (
        <ModelProgress
          key={model.name}
          model={model}
          global={global?.models.find(({ name }) => name === model.name)}
          teis={global?.teis.filter((tei) => tei.model === model.name) ?? []}
        />
      ))}
    </div>
  );
}

function ModelProgress({ model, global, teis }: { model: Model; global: GlobalModel | undefined; teis: Tei[] }) {
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
          <Health label="Embedding" online={model.embedding} />
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
      <dl className="grid grid-cols-3 gap-3 text-sm">
        <Stat label="Rate" value={`${Math.round(model.ratePerMinute).toLocaleString()} / min`} hint="Across users: the pump is shared." />
        <Stat label="Pending" value={model.pending.toLocaleString()} />
        <Stat label="Refused" value={model.failed.toLocaleString()} hint="Chunks TEI refused. The pump leaves them out." warn={model.failed > 0} />
      </dl>
      {global && <GlobalProgress model={global} teis={teis} />}
    </div>
  );
}

/** Admins only: the whole index, and every TEI serving the model. */
function GlobalProgress({ model, teis }: { model: GlobalModel; teis: Tei[] }) {
  return (
    <div className="space-y-3 border-t border-border pt-4">
      <p className="text-xs text-muted-foreground tabular-nums">
        All users: {model.embedded.toLocaleString()} of {model.chunks.toLocaleString()} chunks embedded
        {model.etaSeconds != null && `, done in about ${formatDuration(model.etaSeconds)}`}
      </p>
      {teis.length > 0 && (
        <div className="grid gap-3 sm:grid-cols-2">
          {teis.map((tei) => (
            <TeiCard key={tei.url} tei={tei} />
          ))}
        </div>
      )}
    </div>
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

/** A green or red dot with what it's for. */
function Health({ label, online }: { label: string; online: boolean }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <Circle className={cn("size-2", online ? "fill-emerald-500 text-emerald-500" : "fill-destructive text-destructive")} aria-hidden />
      {label}
      <span className="sr-only"> {online ? "online" : "offline"}</span>
    </span>
  );
}
