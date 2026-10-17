"use client";

import { Brain, Database, FileText, Layers, ListTree, Rows3 } from "lucide-react";
import { useSearchParams } from "next/navigation";
import { useEffect, useState, type ReactNode } from "react";
import { useQuery } from "urql";
import { Embedders } from "@/components/embedders";
import { IngestFeed } from "@/components/ingest-feed";
import { NamedIcon } from "@/components/named-icon";
import { Section } from "@/components/page-shell";
import { RunDots } from "@/components/run-status";
import { graphql } from "@/gql";
import type { ActivityOverviewQuery } from "@/gql/graphql";
import { activityHref } from "@/lib/activity-url";
import { timeAgo } from "@/lib/format-time";
import { errorMessage } from "@/lib/graphql";
import { cn } from "@/lib/utils";

const ActivityOverviewQueryDocument = graphql(`
  query ActivityOverview {
    me {
      isAdmin
    }
    explore(input: { limit: 1 }) {
      total
      facets {
        sources {
          writer {
            id
          }
        }
      }
    }
    embeddingModels {
      name
      configuredDefault
      embedded
      chunks
      pending
      failed
      ratePerMinute
      etaSeconds
      online
      embedding
    }
    connections {
      id
      name
      icon
      enabled
      nextRunAt
      runs(limit: 12) {
        id
        status
        startedAt
      }
    }
  }
`);

// Admins only: the server refuses it for everyone else, so it's its own query.
const EmbedderStatusQueryDocument = graphql(`
  query EmbedderStatus {
    embeddingStatus {
      name
      embedded
      chunks
      etaSeconds
    }
    teis {
      url
      model
      ...TeiCardFields
    }
  }
`);

/** Often enough that a run's counts and the pump's progress visibly move. */
const POLL_MS = 5000;

type Connection = ActivityOverviewQuery["connections"][number];

/** Activity with its writer filter in the URL, so a connection can link to its own feed. Needs a
 * Suspense boundary in a static export, for `useSearchParams`. */
export function ActivityRoute() {
  const params = useSearchParams();
  return (
    <Activity
      writer={params?.get("writer") ?? null}
      // Next's router follows `pushState` and updates `useSearchParams`.
      onWriterChange={(writer) => window.history.pushState(null, "", activityHref(writer))}
    />
  );
}

/**
 * What ferret ingested and embedded, live: the index's size, the embedders, and every ingest,
 * optionally only one writer's.
 */
export function Activity({ writer, onWriterChange }: { writer: string | null; onWriterChange: (writer: string | null) => void }) {
  const [{ data, error }, refetch] = useQuery({ query: ActivityOverviewQueryDocument });
  const isAdmin = data?.me.isAdmin ?? false;
  const [{ data: embedders }, refetchEmbedders] = useQuery({ query: EmbedderStatusQueryDocument, pause: !isAdmin });
  const [tick, setTick] = useState(0);
  const [now, setNow] = useState(() => new Date());
  const [allUsers, setAllUsers] = useState(false);

  useEffect(() => {
    // A tab in the background would poll for nothing.
    const timer = setInterval(() => {
      if (document.hidden) return;
      refetch({ requestPolicy: "network-only" });
      if (isAdmin) refetchEmbedders({ requestPolicy: "network-only" });
      setNow(new Date());
      setTick((tick) => tick + 1);
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [refetch, refetchEmbedders, isAdmin]);

  if (error) return <p className="text-sm text-destructive">{errorMessage(error)}</p>;
  if (!data) return null;

  const defaultModel = data.embeddingModels.find((model) => model.configuredDefault) ?? data.embeddingModels[0];
  const sources = new Set(data.explore.facets.sources.map((source) => source.writer?.id ?? null)).size;

  return (
    <div className="space-y-8">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Tile icon={<FileText />} label="Documents" value={data.explore.total} />
        <Tile icon={<Layers />} label="Sources" value={sources} />
        <Tile icon={<Rows3 />} label="Chunks" value={defaultModel?.chunks ?? 0} />
        <Tile icon={<Database />} label="Embedded" value={defaultModel ? `${Math.floor((defaultModel.chunks ? defaultModel.embedded / defaultModel.chunks : 1) * 100)}%` : "–"} />
      </div>
      <Section title="Embedding" icon={<Brain />} description="How far each model has embedded your chunks, and how fast it goes.">
        <Embedders models={data.embeddingModels} global={embedders && { models: embedders.embeddingStatus, teis: embedders.teis }} />
      </Section>
      {data.connections.length > 0 && (
        <Section title="Connections" icon={<ListTree />} description="Each connection's last runs, newest on the right, and its next one.">
          <ul className="divide-y divide-border">
            {data.connections.map((connection) => (
              <ConnectionHistory
                key={connection.id}
                connection={connection}
                now={now}
                selected={writer === connection.id}
                onSelect={() => onWriterChange(writer === connection.id ? null : connection.id)}
              />
            ))}
          </ul>
        </Section>
      )}
      <section className="space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="text-base font-medium">Ingests</h2>
          <div className="flex items-center gap-2 text-sm">
            {writer && (
              <button type="button" onClick={() => onWriterChange(null)} className="rounded-full border border-border px-3 py-1 hover:bg-accent">
                {data.connections.find((connection) => connection.id === writer)?.name ?? "One writer"} ✕
              </button>
            )}
            {isAdmin && (
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={allUsers} onChange={(event) => setAllUsers(event.target.checked)} />
                Every user
              </label>
            )}
          </div>
        </div>
        {/* Keyed by the filter, so its older pages start over. */}
        <IngestFeed key={`${writer}:${allUsers}`} writer={writer} allUsers={allUsers} now={now} refresh={tick} />
      </section>
    </div>
  );
}

function Tile({ icon, label, value }: { icon: ReactNode; label: string; value: number | string }) {
  return (
    <div className="rounded-xl border border-border p-4">
      <p className="flex items-center gap-2 text-xs text-muted-foreground [&>svg]:size-3.5">
        {icon}
        {label}
      </p>
      <p className="mt-1 text-2xl font-semibold tracking-tight tabular-nums">{typeof value === "number" ? value.toLocaleString() : value}</p>
    </div>
  );
}

/** A connection's recent runs as dots, oldest left, with its last and next run. Selecting it
 * narrows the ingest feed to it. */
function ConnectionHistory({
  connection,
  now,
  selected,
  onSelect,
}: {
  connection: Connection;
  now: Date;
  selected: boolean;
  onSelect: () => void;
}) {
  const last = connection.runs[0];
  return (
    <li>
      <button
        type="button"
        aria-pressed={selected}
        onClick={onSelect}
        className={cn("flex w-full items-center gap-3 rounded-lg px-2 py-2.5 text-left text-sm hover:bg-accent/50", selected && "bg-accent")}
      >
        <NamedIcon name={connection.icon} className="size-4 shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1">
          <span className="block truncate font-medium">{connection.name}</span>
          <span className="block text-xs text-muted-foreground">
            {last ? `last ${timeAgo(last.startedAt, now)}` : "never ran"}
            {" · "}
            {connection.enabled ? `next ${timeAgo(connection.nextRunAt, now)}` : "disabled"}
          </span>
        </span>
        <RunDots runs={connection.runs} />
      </button>
    </li>
  );
}
