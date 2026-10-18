"use client";

import { Check, Loader2, X } from "lucide-react";
import { useTranslations } from "next-intl";
import { useMutation } from "urql";
import { ModelEndpointCards } from "@/components/endpoint-card";
import { graphql } from "@/gql";
import type { AdminQuery, EndpointRole } from "@/gql/graphql";
import { errorMessage } from "@/lib/graphql";

const TestEmbeddingModelMutation = graphql(`
  mutation TestEmbeddingModel($id: String!) {
    testEmbeddingModel(id: $id) {
      role
      durationMs
      dimension
      error
    }
  }
`);

const ROLE_LABELS: Record<EndpointRole, "forBoth" | "forChunks" | "forQueries"> = {
  CHUNKS_AND_QUERIES: "forBoth",
  CHUNKS: "forChunks",
  QUERIES: "forQueries",
};

type Model = AdminQuery["embeddingStatus"][number];

const tableClass = "w-full text-left text-sm";
const headClass = "border-b border-border text-xs text-muted-foreground";

/** Every configured model with its endpoints and what's still waiting to be embedded. */
export function ModelEndpoints({ models }: { models: Model[] }) {
  return (
    <div className="space-y-4">
      {models.map((model) => (
        <ModelStatus key={model.id} model={model} />
      ))}
    </div>
  );
}

function ModelStatus({ model }: { model: Model }) {
  const t = useTranslations();
  const [test, runTest] = useMutation(TestEmbeddingModelMutation);
  return (
    <div className="space-y-3 rounded-xl border border-border p-4 text-sm">
      <div className="flex items-start justify-between gap-2">
        <div>
          <p className="font-medium">
            {model.id}
            {model.isDefault && <span className="text-xs font-normal text-muted-foreground"> · {t("common.default")}</span>}
          </p>
          <p className="text-xs text-muted-foreground">{t("endpoint.dimensions", { dimension: model.dimension })}</p>
        </div>
        <button
          type="button"
          onClick={() => runTest({ id: model.id })}
          disabled={test.fetching}
          aria-label={t("endpoint.testLabel", { model: model.id })}
          className="inline-flex shrink-0 items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-sm hover:bg-accent disabled:opacity-50"
        >
          {test.fetching && <Loader2 className="size-3.5 animate-spin" aria-hidden />}
          {t("endpoint.test")}
        </button>
      </div>
      {!test.fetching && test.error && (
        <p role="alert" className="text-xs text-destructive">
          {errorMessage(test.error)}
        </p>
      )}
      {!test.fetching && test.data && (
        <ul className="space-y-1 text-xs" aria-label={t("endpoint.testResults")}>
          {test.data.testEmbeddingModel.map((result) => (
            <li key={result.role} className={result.error ? "text-destructive" : "text-emerald-700 dark:text-emerald-400"}>
              {result.error ? <X className="mr-1 inline size-3.5" aria-hidden /> : <Check className="mr-1 inline size-3.5" aria-hidden />}
              {result.error
                ? t("endpoint.testFailed", { role: t(`endpoint.${ROLE_LABELS[result.role]}`), ms: result.durationMs, error: result.error })
                : t("endpoint.testPassed", { role: t(`endpoint.${ROLE_LABELS[result.role]}`), ms: result.durationMs, dimension: result.dimension ?? 0 })}
            </li>
          ))}
        </ul>
      )}
      <ModelEndpointCards embeddingApi={model.embeddingApi} queryApi={model.queryApi} />
      {model.backlog.length === 0 ? (
        <p className="text-muted-foreground">{t("endpoint.allEmbedded")}</p>
      ) : (
        <table className={tableClass}>
          <thead className={headClass}>
            <tr>
              <th className="py-1.5 pr-3 font-medium">{t("endpoint.source")}</th>
              <th className="py-1.5 pr-3 text-right font-medium">{t("endpoint.pending")}</th>
              <th className="py-1.5 text-right font-medium">{t("endpoint.refused")}</th>
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
