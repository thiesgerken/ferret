import assert from "node:assert/strict";
import test from "node:test";
import { act, useState } from "react";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import { Activity } from "./activity";

type Operation = { operationName: string; variables: Record<string, unknown> };

const writer = { __typename: "Writer", kind: "CONNECTION", id: "c1", name: "Notes", icon: "notebook" };

const ingest = (id: string, overrides: Record<string, unknown> = {}) => ({
  __typename: "Ingest",
  id,
  cursor: `cursor-${id}`,
  kind: "RUN",
  writer,
  status: "SUCCEEDED",
  startedAt: "2026-10-17T11:00:00Z",
  finishedAt: "2026-10-17T11:01:20Z",
  put: 12,
  unchanged: 30,
  deleted: 0,
  error: null,
  log: `log of ${id}`,
  owner: null,
  ...overrides,
});

function storage(documents: number, chunks: number, embeddings: number, indexes: number) {
  return { __typename: "WriterStorage", documents, chunks, embeddings, indexes, total: documents + chunks + embeddings + indexes };
}

function overview(isAdmin: boolean) {
  return {
    me: { __typename: "User", isAdmin },
    writerStatistics: {
      __typename: "WriterStatistics",
      documents: 18945,
      chunks: 1000,
      storage: storage(1000, 1000, 1000, 1000),
      rows: [
        { __typename: "WriterStatisticsRow", writer, sources: ["notes", "notes/nested"], documents: 10000, chunks: 600, storage: storage(600, 600, 600, 600), models: [{ __typename: "WriterModelStorage", model: "retired", embedded: 600, rowBytes: 600, indexBytes: 200 }] },
        { __typename: "WriterStatisticsRow", writer: null, sources: ["legacy"], documents: 8945, chunks: 400, storage: storage(400, 400, 400, 400), models: [] },
      ],
    },
    embeddingModels: [
      {
        __typename: "EmbeddingModel",
        id: "qwen3",
        configuredDefault: true,
        embedded: 619,
        chunks: 1000,
        pending: 381,
        failed: 2,
        ratePerMinute: 42.4,
        etaSeconds: 540,
        online: true,
        embedding: true,
      },
    ],
    connections: [
      {
        __typename: "Connection",
        id: "c1",
        name: "Notes",
        icon: "notebook",
        enabled: true,
        nextRunAt: "2026-10-17T13:00:00Z",
        runs: [
          { __typename: "PluginRun", id: "r2", status: "FAILED", startedAt: "2026-10-17T11:00:00Z" },
          { __typename: "PluginRun", id: "r1", status: "SUCCEEDED", startedAt: "2026-10-17T10:00:00Z" },
        ],
      },
    ],
  };
}

const EMBEDDERS = {
  embeddingStatus: [
    {
      __typename: "ModelStatus",
      id: "qwen3",
      embedded: 5000,
      chunks: 20000,
      etaSeconds: 7200,
      embeddingApi: {
        __typename: "ModelEndpoint",
        url: "http://litellm:4000/v1",
        model: "qwen3-embed",
        reachable: false,
        lastError: "unreachable: connection refused",
        lastErrorAt: null,
      },
      queryApi: {
        __typename: "ModelEndpoint",
        url: "http://tei:80/v1",
        model: "Qwen/Qwen3-Embedding-0.6B",
        reachable: true,
        lastError: null,
        lastErrorAt: null,
      },
    },
  ],
};

/** The writer filter in state, as the route keeps it in the URL. */
function FilteredActivity() {
  const [writer, setWriter] = useState<string | null>(null);
  return <Activity writer={writer} onWriterChange={setWriter} />;
}

async function renderActivity(isAdmin = false, empty = false) {
  const operations: Operation[] = [];
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body)) as Operation;
    operations.push(operation);
    switch (operation.operationName) {
      case "ActivityOverview":
        return Response.json({ data: { ...overview(isAdmin), ...(empty ? { writerStatistics: { __typename: "WriterStatistics", documents: 0, chunks: 0, storage: storage(0, 0, 0, 0), rows: [] } } : {}) } });
      case "EmbedderStatus":
        return Response.json({ data: EMBEDDERS });
      default: {
        // A full first page, so an older one is offered; the older page ends the feed.
        const older = operation.variables.before !== null;
        const page = older
          ? [ingest("old")]
          : [ingest("i1", { status: "RUNNING", finishedAt: null, put: 5 }), ...Array.from({ length: 24 }, (_, n) => ingest(`n${n}`))];
        return Response.json({ data: { activity: page } });
      }
    }
  });
  const view = await renderReact(
    <Provider value={client}>
      <FilteredActivity />
    </Provider>,
  );
  await settle();
  await settle();
  return { view, operations, text: () => view.container.textContent ?? "" };
}

test("shows the index's size and each model's progress", async () => {
  const { view, text } = await renderActivity();

  assert.match(text(), /Documents18,945/);
  assert.match(text(), /Writers2/);
  assert.match(text(), /Chunks1,000/);
  assert.match(text(), /619 of 1,000 chunks embedded \(61%\)/);
  assert.match(text(), /381 pending, done in about 9 min/);
  assert.equal(view.container.querySelector("[role=progressbar]")?.getAttribute("aria-valuenow"), "61");
  assert.doesNotMatch(text(), /All users/, "the global numbers are for admins");
  await view.unmount();
});

test("admins see the whole index and the model's endpoints", async () => {
  const { view, operations, text } = await renderActivity(true);

  assert.ok(operations.some((operation) => operation.operationName === "EmbedderStatus"));
  assert.match(text(), /All users: 5,000 of 20,000 chunks embedded, done in about 2 h/);
  assert.match(text(), /Queries onlineEmbedding online/, "the model's health");
  assert.match(text(), /http:\/\/litellm:4000\/v1UnreachableChunks · qwen3-embedLast error: unreachable: connection refused/);
  assert.match(text(), /http:\/\/tei:80\/v1ReachableQueries · Qwen\/Qwen3-Embedding-0\.6B/);
  await view.unmount();
});

test("the feed lists ingests, a run opens to its log, and older pages follow by cursor", async () => {
  const { view, operations, text } = await renderActivity();
  const rows = () => [...view.container.querySelectorAll("ol > li button[aria-expanded]")];

  assert.equal(rows().length, 25);
  assert.match(text(), /NotesRunningRun/);
  await act(async () => (rows()[1] as HTMLButtonElement).click());
  assert.match(text(), /log of n0/);

  const older = [...view.container.querySelectorAll("button")].find((button) => button.textContent === "Show older")!;
  await act(async () => older.click());
  await settle();
  const feed = operations.filter((operation) => operation.operationName === "ActivityFeed");
  assert.equal(feed.at(-1)?.variables.before, "cursor-n23");
  assert.equal(rows().length, 26);
  await view.unmount();
});

test("a connection's history narrows the feed to it", async () => {
  const { view, operations } = await renderActivity();
  const history = view.container.querySelector<HTMLButtonElement>("button[aria-pressed]")!;

  assert.equal(history.querySelectorAll("[title]").length, 2, "one dot per run");
  await act(async () => history.click());
  await settle();

  const feed = operations.filter((operation) => operation.operationName === "ActivityFeed");
  assert.equal(feed.at(-1)?.variables.writer, "c1");
  await view.unmount();
});

test("writer details expose current paths, percentages and all stored models without narrowing with the feed", async () => {
  const { view } = await renderActivity();
  const table = view.container.querySelector("table")!;
  const count = table.querySelector<HTMLElement>("span[tabindex='0']")!;
  count.focus();
  assert.equal(document.activeElement, count);
  assert.equal(count.getAttribute("aria-label"), "10,000: 52.8% of your documents");
  assert.match(table.textContent ?? "", /Unknown writer/);
  assert.match(table.textContent ?? "", /notes\/nested/);
  assert.match(table.textContent ?? "", /60% of your estimated storage/);
  assert.match(table.textContent ?? "", /retired600 embeddings/);
  const summaries = table.querySelectorAll<HTMLElement>("summary");
  summaries[1].focus();
  assert.equal(document.activeElement, summaries[1], "native details summaries are keyboard focusable");
  summaries[1].click();
  assert.ok(summaries[1].parentElement?.hasAttribute("open"));
  const before = table.textContent;
  await act(async () => view.container.querySelector<HTMLButtonElement>("button[aria-pressed]")!.click());
  await settle();
  assert.equal(table.textContent, before, "feed filters leave the whole-index denominators alone");
  await view.unmount();
});

test("an empty index keeps zero counts and storage valid", async () => {
  const { view, text } = await renderActivity(false, true);
  assert.match(text(), /Writers0/);
  assert.match(text(), /Your total00≈ 0 byte/);
  assert.doesNotMatch(text(), /NaN|undefined|Infinity/);
  await view.unmount();
});
