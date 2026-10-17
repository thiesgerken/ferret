import { useFormatter, useLocale, useTranslations } from "next-intl";
import type { RunStatus } from "@/gql/graphql";
import { formatDuration } from "@/lib/format-time";
import { cn } from "@/lib/utils";

/** A run's status, or a connection's when it's disabled or hasn't run yet. */
export type PillStatus = RunStatus | "DISABLED" | "NEW";

const PILL_CLASSES: Record<PillStatus, string> = {
  RUNNING: "bg-brand/15 text-brand",
  SUCCEEDED: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400",
  FAILED: "bg-destructive/15 text-destructive",
  LOST: "bg-warning/20 text-warning-foreground dark:text-warning",
  DISABLED: "bg-muted text-muted-foreground",
  NEW: "bg-muted text-muted-foreground",
};

export function Pill({ status }: { status: PillStatus }) {
  const t = useTranslations("runs.status");
  return <span className={cn("rounded-full px-2 py-0.5 text-xs font-medium", PILL_CLASSES[status])}>{t(status)}</span>;
}

/** Runs as dots, oldest left, given newest first as the server lists them. */
export function RunDots({ runs }: { runs: { id: string; status: RunStatus; startedAt: string }[] }) {
  const t = useTranslations("runs");
  const format = useFormatter();
  return (
    <span className="flex shrink-0 items-end gap-1" aria-label={t("last", { count: runs.length })}>
      {[...runs].reverse().map((run) => (
        <span
          key={run.id}
          title={`${t(`status.${run.status}`)} ${format.dateTime(new Date(run.startedAt), { dateStyle: "short", timeStyle: "medium" })}`}
          className={cn(
            "size-2.5 rounded-full",
            run.status === "SUCCEEDED" && "bg-emerald-500",
            run.status === "FAILED" && "bg-destructive",
            run.status === "LOST" && "bg-warning",
            run.status === "RUNNING" && "animate-pulse bg-brand",
          )}
        />
      ))}
    </span>
  );
}

/** What a run did to the documents, e.g. "12 put". */
export function Count({ change, value }: { change: "put" | "unchanged" | "deleted"; value: number }) {
  const t = useTranslations("runs.changes");
  const format = useFormatter();
  return (
    <span className={cn(value === 0 && "opacity-50")}>
      <span className="font-medium text-foreground">{format.number(value)}</span> {t(change)}
    </span>
  );
}

/** "took 3 min", or "for 3 min" while it's still running. */
export function RunDuration({ startedAt, finishedAt, now }: { startedAt: string; finishedAt: string | null; now: Date }) {
  const t = useTranslations("runs");
  const locale = useLocale();
  const end = finishedAt ? new Date(finishedAt) : now;
  const duration = formatDuration((end.getTime() - new Date(startedAt).getTime()) / 1000, locale);
  return finishedAt ? t("took", { duration }) : t("for", { duration });
}
