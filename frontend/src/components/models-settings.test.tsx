import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import { ModelsSettings } from "./models-settings";

type Operation = { operationName: string; variables: Record<string, unknown> };

const model = (name: string, configuredDefault: boolean, embedded: number, chunks = 1200) => ({
  __typename: "EmbeddingModel",
  name,
  configuredDefault,
  embedded,
  chunks,
});

test("lists the models' coverage and sets the default", async () => {
  const operations: Operation[] = [];
  let chosen: string | null = null;
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body)) as Operation;
    operations.push(operation);
    if (operation.operationName === "SetDefaultEmbeddingModel") {
      chosen = operation.variables.model as string | null;
      return Response.json({ data: { setDefaultEmbeddingModel: chosen } });
    }
    return Response.json({
      data: {
        me: { __typename: "User", defaultEmbeddingModel: chosen },
        embeddingModels: [model("qwen3", true, 1200), model("e5", false, 300)],
      },
    });
  });
  const view = await renderReact(
    <Provider value={client}>
      <ModelsSettings />
    </Provider>,
  );
  await settle();
  const radios = () => [...view.container.querySelectorAll<HTMLInputElement>("input[type=radio]")];

  assert.deepEqual(
    [...view.container.querySelectorAll("label")].map((label) => label.textContent),
    [
      "Configured default (qwen3)",
      "qwen31,200 of 1,200 chunks embedded (100%)",
      "e5300 of 1,200 chunks embedded (25%)",
    ],
  );
  assert.deepEqual(radios().map((radio) => radio.checked), [true, false, false]);
  await act(async () => radios()[2].click());
  await settle();

  assert.deepEqual(operations.find((op) => op.operationName === "SetDefaultEmbeddingModel")?.variables, { model: "e5" });
  assert.deepEqual(radios().map((radio) => radio.checked), [false, false, true]);
  await view.unmount();
});

test("coverage without chunks", async () => {
  const client = createGraphqlClient(async () =>
    Response.json({
      data: { me: { __typename: "User", defaultEmbeddingModel: null }, embeddingModels: [model("qwen3", true, 0, 0)] },
    }),
  );
  const view = await renderReact(
    <Provider value={client}>
      <ModelsSettings />
    </Provider>,
  );
  await settle();
  assert.match(view.container.textContent ?? "", /qwen3no chunks yet/);
  await view.unmount();
});
