import type { RunStatus } from "@/gql/graphql";
import { cn } from "@/lib/utils";

export type PillStyle = { label: string; className: string };

export const STATUS_STYLES: Record<RunStatus, PillStyle> = {
  RUNNING: { label: "Running", className: "bg-brand/15 text-brand" },
  SUCCEEDED: { label: "OK", className: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400" },
  FAILED: { label: "Failed", className: "bg-destructive/15 text-destructive" },
  LOST: { label: "Lost", className: "bg-warning/20 text-warning-foreground dark:text-warning" },
};

export function Pill({ label, className }: PillStyle) {
  return <span className={cn("rounded-full px-2 py-0.5 text-xs font-medium", className)}>{label}</span>;
}

/** Runs as dots, oldest left, given newest first as the server lists them. */
export function RunDots({ runs }: { runs: { id: string; status: RunStatus; startedAt: string }[] }) {
  return (
    <span className="flex shrink-0 items-end gap-1" aria-label={`Last ${runs.length} runs`}>
      {[...runs].reverse().map((run) => (
        <span
          key={run.id}
          title={`${STATUS_STYLES[run.status].label} ${new Date(run.startedAt).toLocaleString()}`}
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

export function Count({ label, value }: { label: string; value: number }) {
  return (
    <span className={cn(value === 0 && "opacity-50")}>
      <span className="font-medium text-foreground">{value.toLocaleString()}</span> {label}
    </span>
  );
}
