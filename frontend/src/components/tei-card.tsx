"use client";

import { Info } from "lucide-react";
import { useFormatter, useLocale, useTranslations } from "next-intl";
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

const STATE_CLASSES: Record<TeiState, string> = {
  ONLINE: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400",
  OFFLINE: "bg-destructive/15 text-destructive",
  UNUSED: "bg-warning/20 text-warning-foreground dark:text-warning",
  UNSUITABLE: "bg-warning/20 text-warning-foreground dark:text-warning",
};

/**
 * One TEI of the pool: its state, what it's used for and its figures. `controls` replaces the line
 * of what it's used for, e.g. with switches on the Admin page.
 */
export function TeiCard({ tei: fragment, controls }: { tei: FragmentType<typeof TeiCardFields>; controls?: ReactNode }) {
  const t = useTranslations("tei");
  const locale = useLocale();
  const format = useFormatter();
  const tei = useFragment(TeiCardFields, fragment);
  const number = (value: number | null | undefined, show: (value: number) => string) =>
    value === null || value === undefined ? "–" : show(value);
  const uses = [tei.queries && t("queries"), tei.embedding && t("embedding")].filter(Boolean).join(" · ");
  // An unused or unreachable TEI has no figures worth a row of dashes.
  const measured = [tei.load, tei.queueSize, tei.batchSize, tei.throughputPerMinute, tei.failures].some((value) => value != null);
  return (
    <div className={cn("space-y-3 rounded-lg bg-muted/50 p-3 text-sm", !tei.enabled && "opacity-70")}>
      <div className="flex items-start justify-between gap-2">
        <p className="min-w-0 truncate font-mono text-xs leading-5" title={tei.url}>
          {tei.url}
        </p>
        <span className="flex shrink-0 items-center gap-1.5">
          {tei.external && <span className="rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground">{t("external")}</span>}
          <span className={cn("rounded-full px-2 py-0.5 text-xs font-medium", STATE_CLASSES[tei.state])}>{t(`state.${tei.state}`)}</span>
        </span>
      </div>
      {tei.problem && <p className="text-xs text-warning-foreground first-letter:uppercase dark:text-warning">{tei.problem}</p>}
      {controls ?? (
        <p className="text-xs text-muted-foreground">{!tei.enabled ? t("switchedOff") : uses || t("unused")}</p>
      )}
      {tei.load != null && (
        <div>
          <p className="flex items-center gap-1 text-xs text-muted-foreground">
            {t("load")}
            <span title={t("loadHint")}>
              <Info className="size-3" aria-label={t("loadInfo")} />
            </span>
            <span className="ml-auto tabular-nums">{number(tei.load, (load) => format.number(load, { style: "percent" }))}</span>
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
          <Figure label={t("queue")} value={number(tei.queueSize, format.number)} />
          <Figure
            label={t("batch")}
            value={number(tei.batchSize, (size) => format.number(size, { minimumFractionDigits: 1, maximumFractionDigits: 1 }))}
          />
          <Figure label={t("throughput")} value={number(tei.throughputPerMinute, (rate) => `${format.number(Math.round(rate))} / min`)} />
          <Figure label={t("failures")} value={number(tei.failures, format.number)} />
          {tei.embedding && tei.state === "ONLINE" && (
            <>
              <Figure label={t("pumpBatch")} value={number(tei.pumpBatchSize, (size) => `${size}${tei.busy ? ` · ${t("busy")}` : ""}`)} />
              <Figure label={t("backlogDoneIn")} value={number(tei.etaSeconds, (eta) => formatDuration(eta, locale))} />
            </>
          )}
        </dl>
      )}
      {tei.lastError && (
        <p className="text-xs text-destructive">
          {tei.lastErrorAt
            ? t("lastError", { date: new Date(tei.lastErrorAt), error: tei.lastError })
            : t("lastErrorUndated", { error: tei.lastError })}
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
