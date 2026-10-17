"use client";

import { ChevronRight, CircleDashed, KeyRound, Play } from "lucide-react";
import { useLocale, useTranslations } from "next-intl";
import { useEffect, useState } from "react";
import { useQuery } from "urql";
import { NamedIcon } from "@/components/named-icon";
import { Count, Pill, RunDuration } from "@/components/run-status";
import { graphql } from "@/gql";
import type { ActivityFeedQuery } from "@/gql/graphql";
import { timeAgo } from "@/lib/format-time";
import { errorMessage } from "@/lib/graphql";
import { cn } from "@/lib/utils";

const ActivityFeedQueryDocument = graphql(`
  query ActivityFeed($writer: ID, $before: String, $allUsers: Boolean) {
    activity(writer: $writer, before: $before, limit: 25, allUsers: $allUsers) {
      id
      cursor
      kind
      writer {
        kind
        id
        name
        icon
      }
      status
      startedAt
      finishedAt
      put
      unchanged
      deleted
      error
      log
      owner
    }
  }
`);

type Ingest = ActivityFeedQuery["activity"][number];

/**
 * Plugin runs and API keys' bursts of writes, newest first, one page at a time. A run opens to
 * its log. `refresh` changes on every poll, so a running ingest's counts keep moving.
 */
export function IngestFeed({
  writer,
  allUsers,
  now,
  refresh,
}: {
  /** Only this connection's or key's ingests, or everyone's when null. */
  writer: string | null;
  /** Admins: every user's ingests, with their owner. */
  allUsers: boolean;
  now: Date;
  refresh: number;
}) {
  // Older pages, by the cursor each one starts after. Polling refreshes only the newest page.
  const [cursors, setCursors] = useState<string[]>([]);
  return (
    <div className="space-y-2">
      <ol className="divide-y divide-border rounded-xl border border-border">
        <FeedPage writer={writer} allUsers={allUsers} before={null} now={now} refresh={refresh} onMore={(cursor) => setCursors([cursor])} last={cursors.length === 0} />
        {cursors.map((cursor, index) => (
          <FeedPage
            key={cursor}
            writer={writer}
            allUsers={allUsers}
            before={cursor}
            now={now}
            refresh={0}
            onMore={(next) => setCursors([...cursors.slice(0, index + 1), next])}
            last={index === cursors.length - 1}
          />
        ))}
      </ol>
    </div>
  );
}

const PAGE_SIZE = 25;

function FeedPage({
  writer,
  allUsers,
  before,
  now,
  refresh,
  onMore,
  last,
}: {
  writer: string | null;
  allUsers: boolean;
  before: string | null;
  now: Date;
  refresh: number;
  onMore: (cursor: string) => void;
  /** Only the last page offers the next one. */
  last: boolean;
}) {
  const t = useTranslations("activity");
  const [{ data, error }, refetch] = useQuery({
    query: ActivityFeedQueryDocument,
    variables: { writer, before, allUsers: allUsers || null },
  });
  useEffect(() => {
    if (refresh > 0) refetch({ requestPolicy: "network-only" });
  }, [refresh, refetch]);
  if (error) {
    return (
      <li role="alert" className="p-4 text-sm text-destructive">
        {errorMessage(error)}
      </li>
    );
  }
  if (!data) return null;
  if (data.activity.length === 0 && before === null) {
    return <li className="p-6 text-center text-sm text-muted-foreground">{t("nothingYet")}</li>;
  }
  const end = data.activity.at(-1);
  return (
    <>
      {data.activity.map((ingest) => (
        <IngestRow key={ingest.id} ingest={ingest} now={now} showOwner={allUsers} />
      ))}
      {last && end && data.activity.length === PAGE_SIZE && (
        <li className="p-2 text-center">
          <button type="button" onClick={() => onMore(end.cursor)} className="rounded-md px-3 py-1 text-sm text-muted-foreground hover:bg-accent hover:text-foreground">
            {t("showOlder")}
          </button>
        </li>
      )}
    </>
  );
}

function IngestRow({ ingest, now, showOwner }: { ingest: Ingest; now: Date; showOwner: boolean }) {
  const t = useTranslations();
  const locale = useLocale();
  const [open, setOpen] = useState(false);
  const expandable = Boolean(ingest.log || ingest.error);

  return (
    <li className="text-sm">
      <button
        type="button"
        disabled={!expandable}
        aria-expanded={expandable ? open : undefined}
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-3 px-4 py-3 text-left enabled:hover:bg-accent/50"
      >
        <ChevronRight className={cn("size-4 shrink-0 text-muted-foreground transition-transform", open && "rotate-90", !expandable && "invisible")} aria-hidden />
        <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-muted">
          {ingest.writer ? <NamedIcon name={ingest.writer.icon} className="size-3.5" /> : <CircleDashed className="size-3.5" aria-hidden />}
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex flex-wrap items-center gap-x-2">
            <span className="truncate font-medium">{ingest.writer?.name ?? t("activity.unknownWriter")}</span>
            <Pill status={ingest.status} />
            <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
              {ingest.kind === "RUN" ? <Play className="size-3" aria-hidden /> : <KeyRound className="size-3" aria-hidden />}
              {ingest.kind === "RUN" ? t("activity.run") : t("activity.apiWrites")}
            </span>
            {showOwner && ingest.owner && <span className="text-xs text-muted-foreground">· {ingest.owner}</span>}
          </span>
          <span className="block text-xs text-muted-foreground tabular-nums">
            {timeAgo(ingest.startedAt, now, locale)} · <RunDuration startedAt={ingest.startedAt} finishedAt={ingest.finishedAt} now={now} />
          </span>
        </span>
        <span className="hidden shrink-0 gap-3 text-xs text-muted-foreground tabular-nums sm:flex">
          <Count change="put" value={ingest.put} />
          <Count change="unchanged" value={ingest.unchanged} />
          <Count change="deleted" value={ingest.deleted} />
        </span>
      </button>
      {open && (
        <div className="space-y-2 px-4 pb-4 pl-[4.25rem]">
          <p className="text-xs text-muted-foreground tabular-nums sm:hidden">
            <Count change="put" value={ingest.put} /> · <Count change="unchanged" value={ingest.unchanged} /> ·{" "}
            <Count change="deleted" value={ingest.deleted} />
          </p>
          {ingest.error && <p className="text-destructive">{ingest.error}</p>}
          {ingest.log && (
            <pre className="max-h-80 overflow-auto rounded-lg bg-muted p-3 font-mono text-xs whitespace-pre-wrap">{ingest.log}</pre>
          )}
        </div>
      )}
    </li>
  );
}
