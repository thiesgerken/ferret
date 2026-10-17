"use client";

import { TriangleAlert } from "lucide-react";
import { useState } from "react";
import { useMutation } from "urql";
import { TeiCard } from "@/components/tei-card";
import { graphql } from "@/gql";
import type { AdminQuery, TeiUpdate } from "@/gql/graphql";
import { errorMessage } from "@/lib/graphql";
import { cn } from "@/lib/utils";

const UpdateTeiMutation = graphql(`
  mutation UpdateTei($url: String!, $input: TeiUpdate!) {
    updateTei(url: $url, input: $input) {
      url
      enabled
      queries
      embedding
    }
  }
`);

type Model = AdminQuery["embeddingStatus"][number];
type Tei = AdminQuery["teis"][number];

const tableClass = "w-full text-left text-sm";
const headClass = "border-b border-border text-xs text-muted-foreground";

/**
 * The TEI pool by the model each TEI serves, with what's still waiting to be embedded, and the
 * TEIs no configured model uses. Switches take effect within 10 seconds on every server.
 */
export function TeiPool({ models, teis, onChanged }: { models: Model[]; teis: Tei[]; onChanged: () => void }) {
  const others = teis.filter((tei) => !models.some((model) => model.name === tei.model));
  // Unused ones by the model they serve, then those that don't answer.
  const groups = new Map<string, Tei[]>();
  for (const tei of others) {
    const title = tei.servedModel ? `Unused (${tei.servedModel})` : "Offline";
    groups.set(title, [...(groups.get(title) ?? []), tei]);
  }
  return (
    <div className="space-y-4">
      {models.map((model) => (
        <ModelPool key={model.name} model={model} teis={teis.filter((tei) => tei.model === model.name)} onChanged={onChanged} />
      ))}
      {[...groups].map(([title, teis]) => (
        <div key={title} className="space-y-3 rounded-xl border border-dashed border-border p-4 text-sm">
          <p className="font-medium">{title}</p>
          <TeiGrid teis={teis} onChanged={onChanged} />
        </div>
      ))}
    </div>
  );
}

function ModelPool({ model, teis, onChanged }: { model: Model; teis: Tei[]; onChanged: () => void }) {
  const serving = teis.filter((tei) => tei.enabled && tei.state === "ONLINE");
  const warnings = [
    !serving.some((tei) => tei.queries) &&
      "No enabled TEI answers this model's queries: searches with it find keyword matches only.",
    !serving.some((tei) => tei.embedding) && "No enabled TEI embeds this model's chunks: its backlog doesn't move.",
  ].filter((warning): warning is string => Boolean(warning));
  return (
    <div className="space-y-3 rounded-xl border border-border p-4 text-sm">
      <div>
        <p className="font-medium">
          {model.name}
          {model.isDefault && <span className="text-xs font-normal text-muted-foreground"> · default</span>}
        </p>
        <p className="text-xs text-muted-foreground">
          <span className="font-mono">{model.modelId}</span>
          {" · "}
          {model.dimension} dimensions
        </p>
      </div>
      {warnings.map((warning) => (
        <p key={warning} role="status" className="flex items-start gap-1.5 text-xs text-warning-foreground dark:text-warning">
          <TriangleAlert className="mt-px size-3.5 shrink-0" aria-hidden />
          {warning}
        </p>
      ))}
      {teis.length === 0 ? (
        <p className="text-muted-foreground">No TEI serves this model.</p>
      ) : (
        <TeiGrid teis={teis} onChanged={onChanged} />
      )}
      {model.backlog.length === 0 ? (
        <p className="text-muted-foreground">Everything is embedded.</p>
      ) : (
        <table className={tableClass}>
          <thead className={headClass}>
            <tr>
              <th className="py-1.5 pr-3 font-medium">Source</th>
              <th className="py-1.5 pr-3 text-right font-medium">Pending</th>
              <th className="py-1.5 text-right font-medium">Refused by TEI</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {model.backlog.map((row) => (
              <tr key={row.source}>
                <td className="py-1.5 pr-3">{row.source}</td>
                <td className="py-1.5 pr-3 text-right tabular-nums">{row.pending}</td>
                <td className="py-1.5 text-right tabular-nums">{row.failed}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function TeiGrid({ teis, onChanged }: { teis: Tei[]; onChanged: () => void }) {
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {teis.map((tei) => (
        <TeiCard key={tei.url} tei={tei} controls={<TeiSwitches tei={tei} onChanged={onChanged} />} />
      ))}
    </div>
  );
}

function TeiSwitches({ tei, onChanged }: { tei: Tei; onChanged: () => void }) {
  const [{ fetching }, update] = useMutation(UpdateTeiMutation);
  const [error, setError] = useState("");

  async function change(input: TeiUpdate) {
    const result = await update({ url: tei.url, input });
    setError(result.error ? errorMessage(result.error) : "");
    onChanged();
  }

  return (
    <div className="space-y-1.5">
      <div className="flex flex-wrap gap-x-4 gap-y-1.5">
        <Switch label="On" description={tei.url} checked={tei.enabled} disabled={fetching} onChange={(enabled) => change({ enabled })} />
        <Switch
          label="Queries"
          description={tei.url}
          checked={tei.queries}
          disabled={fetching || !tei.enabled}
          onChange={(queries) => change({ queries })}
        />
        <Switch
          label="Embedding"
          description={tei.url}
          checked={tei.embedding}
          disabled={fetching || !tei.enabled}
          onChange={(embedding) => change({ embedding })}
        />
      </div>
      {error && (
        <p role="alert" className="text-xs text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}

function Switch({
  label,
  description,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  /** Which TEI it's for, so screen readers can tell the switches of several TEIs apart. */
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={`${label}: ${description}`}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className="inline-flex items-center gap-2 text-xs disabled:opacity-50"
    >
      <span className={cn("relative h-4 w-7 rounded-full transition-colors", checked ? "bg-brand" : "bg-border")}>
        <span
          className={cn("absolute top-0.5 left-0.5 size-3 rounded-full bg-background shadow transition-transform", checked && "translate-x-3")}
        />
      </span>
      {label}
    </button>
  );
}
