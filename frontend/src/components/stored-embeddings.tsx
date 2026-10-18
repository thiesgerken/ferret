"use client";

import { useFormatter, useTranslations } from "next-intl";
import { useState } from "react";
import { useMutation } from "urql";
import { graphql } from "@/gql";
import type { AdminQuery } from "@/gql/graphql";
import { errorMessage } from "@/lib/graphql";
import { cn } from "@/lib/utils";

const DeleteEmbeddingModelMutation = graphql(`
  mutation DeleteEmbeddingModel($id: String!) {
    deleteEmbeddingModel(id: $id)
  }
`);

type StoredModel = AdminQuery["storedEmbeddingModels"][number];

const UNITS = ["byte", "kilobyte", "megabyte", "gigabyte", "terabyte"] as const;

/**
 * What every embedding model that ever embedded takes up, configured or not. The embeddings of a
 * model that left the config stay for when it comes back, until an admin deletes them here.
 */
export function StoredEmbeddings({ models, onChanged }: { models: StoredModel[]; onChanged: () => void }) {
  const t = useTranslations("storedEmbeddings");
  const format = useFormatter();
  const [, remove] = useMutation(DeleteEmbeddingModelMutation);
  const [error, setError] = useState("");

  function bytes(value: number) {
    const exponent = Math.min(Math.floor(Math.log(Math.max(value, 1)) / Math.log(1024)), UNITS.length - 1);
    return format.number(value / 1024 ** exponent, { style: "unit", unit: UNITS[exponent], maximumFractionDigits: 1 });
  }

  async function handleDelete(model: StoredModel) {
    if (!window.confirm(t("confirmDelete", { model: model.id, embedded: model.embedded }))) {
      return;
    }
    const result = await remove({ id: model.id });
    setError(result.error ? errorMessage(result.error) : "");
    onChanged();
  }

  return (
    <div className="space-y-2">
      <h3 className="text-sm font-medium">{t("title")}</h3>
      <div className="overflow-x-auto">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-border text-xs text-muted-foreground">
            <tr>
              <th className="py-2 pr-3 font-medium">{t("model")}</th>
              <th className="py-2 pr-3 text-right font-medium">{t("embedded")}</th>
              <th className="py-2 pr-3 text-right font-medium">{t("index")}</th>
              <th className="py-2 pr-3 text-right font-medium" title={t("rowsHint")}>
                {t("rows")}
              </th>
              <th className="py-2 font-medium">
                <span className="sr-only">{t("delete")}</span>
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {models.map((model) => (
              <tr key={model.id} className={cn(!model.configured && "text-muted-foreground")}>
                <td className="py-2 pr-3">
                  <span className="font-medium">{model.id}</span>
                  <span className="text-xs">
                    {" · "}
                    {t("dimensions", { dimension: model.dimension })}
                    {!model.configured && ` · ${t("notConfigured")}`}
                  </span>
                </td>
                <td className="py-2 pr-3 text-right tabular-nums">{format.number(model.embedded)}</td>
                <td className="py-2 pr-3 text-right tabular-nums">{model.indexBytes == null ? t("none") : bytes(model.indexBytes)}</td>
                <td className="py-2 pr-3 text-right tabular-nums">≈ {bytes(model.rowBytesEstimate)}</td>
                <td className="py-2 text-right">
                  {!model.configured && (
                    <button
                      type="button"
                      onClick={() => handleDelete(model)}
                      aria-label={t("deleteModel", { model: model.id })}
                      className="rounded-md px-2 py-1 text-sm text-destructive hover:bg-accent"
                    >
                      {t("delete")}
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
