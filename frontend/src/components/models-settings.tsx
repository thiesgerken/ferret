"use client";

import { useTranslations } from "next-intl";
import { useState } from "react";
import { useMutation, useQuery } from "urql";
import { graphql } from "@/gql";
import { errorMessage } from "@/lib/graphql";

const ModelsQuery = graphql(`
  query EmbeddingModels {
    me {
      defaultEmbeddingModel
    }
    embeddingModels {
      name
      configuredDefault
      embedded
      chunks
    }
  }
`);

const SetDefaultModelMutation = graphql(`
  mutation SetDefaultEmbeddingModel($model: String) {
    setDefaultEmbeddingModel(model: $model)
  }
`);

/** The configured embedding models with how much of the user's data each covers, and which one
 * the user's searches use. */
export function ModelsSettings() {
  const t = useTranslations("settings.models");
  const [{ data, error }, refetch] = useQuery({ query: ModelsQuery });
  const [, setDefault] = useMutation(SetDefaultModelMutation);
  const [setError, setSetError] = useState("");

  async function choose(model: string | null) {
    const result = await setDefault({ model });
    setSetError(result.error ? errorMessage(result.error) : "");
    refetch({ requestPolicy: "network-only" });
  }

  if (error) {
    return <p className="text-sm text-destructive">{errorMessage(error)}</p>;
  }
  if (!data) {
    return null;
  }
  const chosen = data.me.defaultEmbeddingModel;
  const configured = data.embeddingModels.find((model) => model.configuredDefault)!.name;

  return (
    <fieldset className="space-y-2">
      <legend className="sr-only">{t("legend")}</legend>
      {setError && (
        <p role="alert" className="text-sm text-destructive">
          {setError}
        </p>
      )}
      <ul className="divide-y divide-border rounded-xl border border-border">
        <li className="p-4 text-sm">
          <label className="flex items-center gap-3">
            <input type="radio" name="model" checked={chosen === null} onChange={() => choose(null)} />
            <span>{t("configuredDefault", { model: configured })}</span>
          </label>
        </li>
        {data.embeddingModels.map((model) => (
          <li key={model.name} className="p-4 text-sm">
            <label className="flex items-center gap-3">
              <input type="radio" name="model" checked={chosen === model.name} onChange={() => choose(model.name)} />
              <span className="flex-1 font-medium">{model.name}</span>
              <span className="text-muted-foreground"><Coverage embedded={model.embedded} chunks={model.chunks} /></span>
            </label>
          </li>
        ))}
      </ul>
    </fieldset>
  );
}

function Coverage({ embedded, chunks }: { embedded: number; chunks: number }) {
  const t = useTranslations("settings.models");
  if (chunks === 0) return t("noChunks");
  // Floored, so a model that's nearly done doesn't read as done.
  return t("coverage", { embedded, chunks, share: Math.floor((embedded / chunks) * 100) / 100 });
}
