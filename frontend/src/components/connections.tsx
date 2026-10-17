"use client";

import { ArrowRight, Cable, Pause, Pencil, Play, Plus, Trash2 } from "lucide-react";
import Link from "next/link";
import { useEffect, useState, type ReactNode } from "react";
import { useMutation, useQuery, type CombinedError } from "urql";
import {
  buttonClass,
  configSchemaOf,
  CreateConnectionForm,
  DeleteConnectionForm,
  EditConnectionForm,
  primaryButtonClass,
  UpdateConnectionMutation,
} from "@/components/connection-forms";
import { NamedIcon } from "@/components/named-icon";
import { Count, Pill, RunDots, STATUS_STYLES, type PillStyle } from "@/components/run-status";
import { graphql } from "@/gql";
import type { ConnectionsQuery as ConnectionsData } from "@/gql/graphql";
import { activityHref } from "@/lib/activity-url";
import { formatDuration, timeAgo } from "@/lib/format-time";
import { errorMessage } from "@/lib/graphql";
import { cn } from "@/lib/utils";

const ConnectionsQuery = graphql(`
  query Connections {
    pluginTypes {
      name
      configSchema
    }
    connections {
      id
      name
      icon
      plugin
      source
      config
      schedule
      enabled
      nextRunAt
      runs(limit: 12) {
        id
        status
        startedAt
        finishedAt
        stats {
          put
          unchanged
          deleted
        }
        error
      }
      exclusions {
        id
        source
        externalId
        thread
      }
    }
  }
`);

const RunConnectionNowMutation = graphql(`
  mutation RunConnectionNow($id: ID!) {
    runConnectionNow(id: $id)
  }
`);

const RemoveExclusionMutation = graphql(`
  mutation RemoveExclusion($id: ID!) {
    removeExclusion(id: $id)
  }
`);

type Connection = ConnectionsData["connections"][number];
type Run = Connection["runs"][number];

/** While a run is due or running, the page polls so its outcome shows up without a reload. */
const POLL_MS = 2000;
/** How often "3 minutes ago" moves on while nothing else changes. */
const CLOCK_MS = 30_000;

export function Connections() {
  const [{ data, error }, refetch] = useQuery({ query: ConnectionsQuery });
  const [creating, setCreating] = useState(false);
  const [now, setNow] = useState(() => new Date());
  const reload = () => refetch({ requestPolicy: "network-only" });

  useEffect(() => {
    const now = new Date();
    const busy = data?.connections.some(
      (connection) => connection.runs[0]?.status === "RUNNING" || (connection.enabled && new Date(connection.nextRunAt) <= now),
    );
    if (!busy) {
      return;
    }
    const timer = setTimeout(() => {
      setNow(new Date());
      refetch({ requestPolicy: "network-only" });
    }, POLL_MS);
    return () => clearTimeout(timer);
  }, [data, refetch]);

  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), CLOCK_MS);
    return () => clearInterval(timer);
  }, []);

  if (error) {
    return <p className="text-sm text-destructive">{errorMessage(error)}</p>;
  }
  if (!data) {
    return null;
  }

  const canCreate = data.pluginTypes.length > 0 && !creating;
  const newButton = (
    <button type="button" onClick={() => setCreating(true)} className={primaryButtonClass}>
      <Plus className="size-4" aria-hidden />
      New connection
    </button>
  );

  return (
    <div className="space-y-6">
      {data.pluginTypes.length === 0 && (
        <p className="rounded-xl bg-muted px-4 py-3 text-sm text-muted-foreground">
          No plugin worker has registered yet, so there&apos;s nothing to connect.
        </p>
      )}
      {data.connections.length > 0 && (
        <div className="flex items-center justify-between gap-3">
          <p className="text-sm text-muted-foreground">
            {data.connections.length} {data.connections.length === 1 ? "connection" : "connections"}
          </p>
          {canCreate && newButton}
        </div>
      )}
      {creating && (
        <CreateConnectionForm
          pluginTypes={data.pluginTypes}
          onCreated={() => {
            setCreating(false);
            reload();
          }}
          onCancel={() => setCreating(false)}
        />
      )}
      {data.connections.length === 0
        ? !creating && (
            <div className="flex flex-col items-center gap-3 rounded-2xl border border-dashed border-border px-6 py-14 text-center">
              <span className="flex size-12 items-center justify-center rounded-full bg-muted text-muted-foreground">
                <Cable className="size-6" aria-hidden />
              </span>
              <p className="font-medium">No connections yet</p>
              <p className="max-w-sm text-sm text-muted-foreground">
                A connection keeps a source in sync with where its documents live, like a git repository or Paperless.
              </p>
              {canCreate && newButton}
            </div>
          )
        : (
            <ul className="space-y-4">
              {data.connections.map((connection) => (
                <ConnectionCard
                  key={connection.id}
                  connection={connection}
                  configSchema={configSchemaOf(data.pluginTypes, connection.plugin)}
                  now={now}
                  onChanged={reload}
                />
              ))}
            </ul>
          )}
    </div>
  );
}

function ConnectionCard({
  connection,
  configSchema,
  now,
  onChanged,
}: {
  connection: Connection;
  configSchema: string;
  now: Date;
  onChanged: () => void;
}) {
  const [, update] = useMutation(UpdateConnectionMutation);
  const [, runNow] = useMutation(RunConnectionNowMutation);
  const [, removeExclusion] = useMutation(RemoveExclusionMutation);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const lastRun = connection.runs[0];

  async function mutate(mutation: () => Promise<{ error?: CombinedError }>) {
    const result = await mutation();
    setError(result.error ? errorMessage(result.error) : "");
    onChanged();
  }

  const setEnabled = (enabled: boolean) =>
    mutate(() =>
      update({
        id: connection.id,
        input: {
          name: connection.name,
          icon: connection.icon,
          config: connection.config,
          schedule: connection.schedule,
          enabled,
        },
      }),
    );

  return (
    <li className={cn("rounded-2xl border border-border", !connection.enabled && "bg-muted/40")}>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-3 p-5">
        <span className="flex size-11 shrink-0 items-center justify-center rounded-xl bg-brand/10 text-brand">
          <NamedIcon name={connection.icon} className="size-5" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="truncate text-base font-semibold">{connection.name}</span>
            <Pill {...connectionStatus(lastRun, connection.enabled)} />
          </p>
          <p className="text-sm text-muted-foreground">
            {connection.plugin} · <code className="font-mono text-xs">{connection.source}</code>
          </p>
        </div>
        <div className="flex flex-wrap gap-1">
          <button
            type="button"
            disabled={!connection.enabled || lastRun?.status === "RUNNING"}
            onClick={() => mutate(() => runNow({ id: connection.id }))}
            aria-label={`Run ${connection.source} now`}
            className={cn(buttonClass, "border border-border")}
          >
            <Play aria-hidden />
            Run now
          </button>
          <button type="button" onClick={() => setEditing(!editing)} aria-label={`Edit ${connection.source}`} className={buttonClass}>
            <Pencil aria-hidden />
            Edit
          </button>
          <button
            type="button"
            onClick={() => setEnabled(!connection.enabled)}
            aria-label={`${connection.enabled ? "Disable" : "Enable"} ${connection.source}`}
            className={buttonClass}
          >
            {connection.enabled ? <Pause aria-hidden /> : <Play aria-hidden />}
            {connection.enabled ? "Disable" : "Enable"}
          </button>
          <button
            type="button"
            onClick={() => setDeleting(true)}
            aria-label={`Delete ${connection.source}`}
            className={cn(buttonClass, "text-destructive hover:bg-destructive/10")}
          >
            <Trash2 aria-hidden />
            Delete
          </button>
        </div>
      </div>
      {error && (
        <p role="alert" className="px-5 pb-3 text-sm text-destructive">
          {error}
        </p>
      )}
      {deleting && (
        <div className="px-5 pb-5">
          <DeleteConnectionForm connection={connection} onCancel={() => setDeleting(false)} onDeleted={onChanged} />
        </div>
      )}
      {editing ? (
        <div className="px-5 pb-5">
          <EditConnectionForm
            connection={connection}
            configSchema={configSchema}
            onCancel={() => setEditing(false)}
            onSaved={() => {
              setEditing(false);
              onChanged();
            }}
          />
        </div>
      ) : (
        <RunFacts connection={connection} now={now} />
      )}
      <div className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 border-t border-border px-5 py-3 text-sm">
        {connection.exclusions.length > 0 ? (
          <Exclusions exclusions={connection.exclusions} onInclude={(id) => mutate(() => removeExclusion({ id }))} />
        ) : (
          <span />
        )}
        <Link href={activityHref(connection.id)} className="inline-flex items-center gap-1 text-muted-foreground hover:text-foreground">
          Runs and logs on Activity
          <ArrowRight className="size-3.5" aria-hidden />
        </Link>
      </div>
    </li>
  );
}

/** The connection's state at a glance: disabled, or how its last run went. */
function connectionStatus(run: Run | undefined, enabled: boolean): PillStyle {
  if (!enabled) return { label: "Disabled", className: "bg-muted text-muted-foreground" };
  return run ? STATUS_STYLES[run.status] : { label: "New", className: "bg-muted text-muted-foreground" };
}

/** The last run, what it changed, the next one and the recent history, with a failed run's error. */
function RunFacts({ connection, now }: { connection: Connection; now: Date }) {
  const lastRun = connection.runs[0];
  const failed = lastRun?.status === "FAILED" || lastRun?.status === "LOST";
  return (
    <div className="space-y-3 border-t border-border px-5 py-4">
      <dl className="grid grid-cols-2 gap-x-6 gap-y-4 text-sm sm:grid-cols-4">
        <Fact label="Last run">
          {lastRun ? (
            <>
              {timeAgo(lastRun.startedAt, now)}
              <span className="block text-xs font-normal text-muted-foreground">{runDuration(lastRun, now)}</span>
            </>
          ) : (
            "Never"
          )}
        </Fact>
        <Fact label="Changes">
          {lastRun?.stats ? (
            <span className="flex flex-col text-xs font-normal text-muted-foreground tabular-nums">
              <Count label="put" value={lastRun.stats.put} />
              <Count label="unchanged" value={lastRun.stats.unchanged} />
              <Count label="deleted" value={lastRun.stats.deleted} />
            </span>
          ) : (
            <span className="text-muted-foreground">–</span>
          )}
        </Fact>
        <Fact label="Next run">
          {connection.enabled ? timeAgo(connection.nextRunAt, now) : <span className="text-muted-foreground">Disabled</span>}
          <code className="block font-mono text-xs font-normal text-muted-foreground">{connection.schedule}</code>
        </Fact>
        <Fact label="History">
          {connection.runs.length > 0 ? <RunDots runs={connection.runs} /> : <span className="text-muted-foreground">–</span>}
        </Fact>
      </dl>
      {failed && lastRun.error && (
        <p className="line-clamp-2 rounded-lg bg-destructive/10 px-3 py-2 text-sm text-destructive">{lastRun.error}</p>
      )}
    </div>
  );
}

function runDuration(run: Run, now: Date): string {
  const end = run.finishedAt ? new Date(run.finishedAt) : now;
  const seconds = (end.getTime() - new Date(run.startedAt).getTime()) / 1000;
  return `${run.finishedAt ? "took" : "for"} ${formatDuration(seconds)}`;
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="mt-1 font-medium">{children}</dd>
    </div>
  );
}

function Exclusions({ exclusions, onInclude }: { exclusions: Connection["exclusions"]; onInclude: (id: string) => void }) {
  return (
    <details className="min-w-0 flex-1">
      <summary className="cursor-pointer text-muted-foreground hover:text-foreground">Excluded from the index ({exclusions.length})</summary>
      <p className="mt-2 text-xs text-muted-foreground">
        The connection drops these whenever it finds them. Removing one makes the next run start over from scratch and
        ingest it again.
      </p>
      <ul className="mt-2 divide-y divide-border">
        {exclusions.map((exclusion) => (
          <li key={exclusion.id} className="flex items-center gap-2 py-1.5">
            <span className="min-w-0 flex-1 truncate">
              {exclusion.externalId ?? `Thread ${exclusion.thread}`}{" "}
              <span className="text-xs text-muted-foreground">in {exclusion.source}</span>
            </span>
            <button
              type="button"
              onClick={() => onInclude(exclusion.id)}
              aria-label={`Include ${exclusion.externalId ?? `thread ${exclusion.thread}`} again`}
              className={buttonClass}
            >
              Include again
            </button>
          </li>
        ))}
      </ul>
    </details>
  );
}
