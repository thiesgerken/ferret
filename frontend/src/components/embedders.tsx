"use client";

import { Circle } from "lucide-react";
import { useFormatter, useLocale, useTranslations } from "next-intl";
import { ModelEndpointCards } from "@/components/endpoint-card";
import type { ActivityOverviewQuery, EmbedderStatusQuery } from "@/gql/graphql";
import { formatDuration } from "@/lib/format-time";
import { cn } from "@/lib/utils";

type Model = ActivityOverviewQuery["embeddingModels"][number];
type GlobalModel = EmbedderStatusQuery["embeddingStatus"][number];

/**
 * Each embedding model's progress over the user's chunks: how much is embedded, how fast the pump
 * goes and when it's done. Admins also see the whole index and the model's endpoints.
 */
export function Embedders({ models, global }: { models: Model[]; global: GlobalModel[] | undefined }) {
  return (
    <div className="space-y-4">
      {models.map((model) => (
        <ModelProgress
          key={model.id}
          model={model}
          global={global?.find(({ id }) => id === model.id)}
        />
      ))}
    </div>
  );
}

function ModelProgress({ model, global }: { model: Model; global: GlobalModel | undefined }) {
  const t = useTranslations();
  const locale = useLocale();
  const format = useFormatter();
  const share = model.chunks === 0 ? 1 : model.embedded / model.chunks;
  return (
    <div className="space-y-4 rounded-xl border border-border p-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <p className="font-medium">
          {model.id}
          {model.configuredDefault && <span className="text-xs font-normal text-muted-foreground"> · {t("common.default")}</span>}
        </p>
        <p className="flex items-center gap-3 text-xs text-muted-foreground">
          <Health label={t("embedders.queries")} online={model.online} />
          <Health label={t("embedders.embedding")} online={model.embedding} />
        </p>
      </div>
      <div>
        <div
          role="progressbar"
          aria-label={t("embedders.progressLabel", { model: model.id })}
          aria-valuenow={Math.floor(share * 100)}
          className="h-2 overflow-hidden rounded-full bg-muted"
        >
          <div className="h-full rounded-full bg-brand transition-[width] duration-700" style={{ width: `${share * 100}%` }} />
        </div>
        <p className="mt-1.5 flex flex-wrap justify-between gap-x-4 text-xs text-muted-foreground tabular-nums">
          <span>
            {t("embedders.coverage", { embedded: model.embedded, chunks: model.chunks, share: Math.floor(share * 100) / 100 })}
          </span>
          <span>
            {model.pending === 0
              ? t("embedders.allEmbedded")
              : model.etaSeconds == null
                ? t("embedders.pending", { pending: model.pending })
                : t("embedders.pendingEta", { pending: model.pending, duration: formatDuration(model.etaSeconds, locale) })}
          </span>
        </p>
      </div>
      <dl className="grid grid-cols-3 gap-3 text-sm">
        <Stat
          label={t("embedders.rate")}
          value={t("embedders.perMinute", { rate: Math.round(model.ratePerMinute) })}
          hint={t("embedders.rateHint")}
        />
        <Stat label={t("embedders.pendingLabel")} value={format.number(model.pending)} />
        <Stat
          label={t("embedders.refused")}
          value={format.number(model.failed)}
          hint={t("embedders.refusedHint")}
          warn={model.failed > 0}
        />
      </dl>
      {global && <GlobalProgress model={global} />}
    </div>
  );
}

/** Admins only: the whole index, and the model's endpoints. */
function GlobalProgress({ model }: { model: GlobalModel }) {
  const t = useTranslations("embedders");
  const locale = useLocale();
  return (
    <div className="space-y-3 border-t border-border pt-4">
      <p className="text-xs text-muted-foreground tabular-nums">
        {model.etaSeconds == null
          ? t("allUsers", { embedded: model.embedded, chunks: model.chunks })
          : t("allUsersEta", { embedded: model.embedded, chunks: model.chunks, duration: formatDuration(model.etaSeconds, locale) })}
      </p>
      <ModelEndpointCards embeddingApi={model.embeddingApi} queryApi={model.queryApi} />
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
  const t = useTranslations("common");
  return (
    <span className="inline-flex items-center gap-1.5">
      <Circle className={cn("size-2", online ? "fill-emerald-500 text-emerald-500" : "fill-destructive text-destructive")} aria-hidden />
      {label}
      <span className="sr-only"> {online ? t("online") : t("offline")}</span>
    </span>
  );
}
