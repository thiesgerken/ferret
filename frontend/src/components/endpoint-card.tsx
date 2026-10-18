"use client";

import { useTranslations } from "next-intl";
import { graphql, useFragment, type FragmentType } from "@/gql";
import { cn } from "@/lib/utils";

export const EndpointCardFields = graphql(`
  fragment EndpointCardFields on ModelEndpoint {
    url
    model
    reachable
    lastError
    lastErrorAt
  }
`);

/**
 * An embedding model's endpoint: where it is, what it calls the model, and how this server's last
 * request to it went. `role` says what the model uses it for.
 */
export function EndpointCard({ endpoint: fragment, role }: { endpoint: FragmentType<typeof EndpointCardFields>; role: string }) {
  const t = useTranslations("endpoint");
  const endpoint = useFragment(EndpointCardFields, fragment);
  const [label, tone] =
    endpoint.reachable == null
      ? [t("unknown"), "bg-muted text-muted-foreground"]
      : endpoint.reachable
        ? [t("reachable"), "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"]
        : [t("unreachable"), "bg-destructive/15 text-destructive"];
  return (
    <div className="space-y-2 rounded-lg bg-muted/50 p-3 text-sm">
      <div className="flex items-start justify-between gap-2">
        <p className="min-w-0 truncate font-mono text-xs leading-5" title={endpoint.url}>
          {endpoint.url}
        </p>
        <span className={cn("shrink-0 rounded-full px-2 py-0.5 text-xs font-medium", tone)}>{label}</span>
      </div>
      <p className="text-xs text-muted-foreground">
        {role} · <span className="font-mono">{endpoint.model}</span>
      </p>
      {endpoint.lastError && (
        <p className="text-xs break-words text-destructive">
          {endpoint.lastErrorAt
            ? t("lastError", { date: new Date(endpoint.lastErrorAt), error: endpoint.lastError })
            : t("lastErrorUndated", { error: endpoint.lastError })}
        </p>
      )}
    </div>
  );
}

type Endpoint = FragmentType<typeof EndpointCardFields> & { url: string; model: string };

/** A model's endpoints: one card when the same endpoint embeds chunks and queries, else two. */
export function ModelEndpointCards({ embeddingApi, queryApi }: { embeddingApi: Endpoint; queryApi: Endpoint }) {
  const t = useTranslations("endpoint");
  const shared = embeddingApi.url === queryApi.url && embeddingApi.model === queryApi.model;
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {shared ? (
        <EndpointCard endpoint={embeddingApi} role={t("forBoth")} />
      ) : (
        <>
          <EndpointCard endpoint={embeddingApi} role={t("forChunks")} />
          <EndpointCard endpoint={queryApi} role={t("forQueries")} />
        </>
      )}
    </div>
  );
}
