"use client";

import { useTranslations } from "next-intl";
import { ModelEndpointCards } from "@/components/endpoint-card";
import type { AdminQuery } from "@/gql/graphql";

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
  return (
    <div className="space-y-3 rounded-xl border border-border p-4 text-sm">
      <div>
        <p className="font-medium">
          {model.id}
          {model.isDefault && <span className="text-xs font-normal text-muted-foreground"> · {t("common.default")}</span>}
        </p>
        <p className="text-xs text-muted-foreground">{t("endpoint.dimensions", { dimension: model.dimension })}</p>
      </div>
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
