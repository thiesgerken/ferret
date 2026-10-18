import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import { Admin } from "./admin";

type Operation = { operationName: string; variables: Record<string, unknown> };

const USER = {
  __typename: "UserAccount",
  id: "u-grace",
  username: "grace",
  displayName: "Grace",
  isAdmin: false,
  enabled: true,
  createdAt: "2026-01-01T12:00:00Z",
  lastLoginAt: null,
};

const ADMIN_DATA = {
  users: [USER],
  workers: [
    {
      __typename: "Worker",
      id: "w-1",
      name: "ferret-plugins-0",
      plugins: ["git", "paperless"],
      lastSeenAt: "2026-01-02T12:00:00Z",
      online: true,
    },
    {
      __typename: "Worker",
      id: "w-0",
      name: "ferret-plugins-old",
      plugins: ["git"],
      lastSeenAt: "2026-01-02T09:00:00Z",
      online: false,
    },
  ],
  embeddingStatus: [
    {
      __typename: "ModelStatus",
      name: "bge-m3",
      modelId: "BAAI/bge-m3",
      isDefault: true,
      dimension: 1024,
      backlog: [{ __typename: "SourceBacklog", source: "notes", pending: 7, failed: 1 }],
    },
  ],
  teis: [
    tei({ url: "http://tei-bge:80", lastError: "TEI answered 503", lastErrorAt: "2026-01-02T11:00:00Z" }),
    tei({ url: "http://macbook:8080", external: true, queries: false }),
    tei({ url: "http://old-tei:80", model: null, servedModel: "intfloat/e5-small", state: "UNUSED", problem: "No configured model uses intfloat/e5-small." }),
  ],
  storedEmbeddingModels: [
    {
      __typename: "StoredEmbeddingModel",
      id: "bge-m3",
      dimension: 1024,
      configured: true,
      embedded: 12000,
      indexBytes: 52428800,
      rowBytesEstimate: 24672000,
    },
    {
      __typename: "StoredEmbeddingModel",
      id: "e5-small",
      dimension: 384,
      configured: false,
      embedded: 1,
      indexBytes: null,
      rowBytesEstimate: 776,
    },
  ],
};

/** A TEI of bge-m3 that's on for queries and embedding, unless `overrides` say otherwise. */
function tei(overrides: Record<string, unknown>) {
  return {
    __typename: "Tei",
    url: "http://tei:80",
    model: "bge-m3",
    servedModel: "BAAI/bge-m3",
    external: false,
    enabled: true,
    queries: true,
    embedding: true,
    state: "ONLINE",
    problem: null,
    lastError: null,
    lastErrorAt: null,
    busy: false,
    pumpBatchSize: 16,
    queueSize: 0,
    batchSize: 4.5,
    throughputPerMinute: 120,
    failures: 0,
    load: 0.4,
    etaSeconds: 60,
    ...overrides,
  };
}

/** Renders the page against a fake server that records every operation and answers mutations
 * with `respond`. */
async function renderAdmin(
  respond: (operation: Operation) => Response = () => Response.json({}),
  data: typeof ADMIN_DATA = ADMIN_DATA,
) {
  const operations: Operation[] = [];
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body)) as Operation;
    operations.push(operation);
    if (operation.operationName === "Admin") {
      return Response.json({ data });
    }
    return respond(operation);
  });
  const view = await renderReact(
    <Provider value={client}>
      <Admin />
    </Provider>,
  );
  await settle();
  return { view, operations };
}

test("shows users, the TEIs by model with the backlog, and the workers", async () => {
  const { view } = await renderAdmin();
  const text = view.container.textContent ?? "";

  assert.match(text, /grace · Grace/);
  assert.match(text, /bge-m3 · defaultBAAI\/bge-m3 · 1,024 dimensions/);
  assert.match(text, /http:\/\/tei-bge:80Online/);
  assert.match(text, /http:\/\/macbook:8080ExternalOnline/);
  assert.match(text, /TEI answered 503/);
  assert.match(text, /Unused \(intfloat\/e5-small\)http:\/\/old-tei:80UnusedNo configured model uses intfloat\/e5-small\./);
  assert.equal(view.container.querySelector("[role=status]"), null, "queries and embedding are served");
  assert.match(text, /notes71/);
  assert.match(text, /ferret-plugins-0git, paperlessonline/);
  assert.match(text, /ferret-plugins-oldgitoffline/);
  await view.unmount();
});

test("toggling a checkbox updates the user with the rest unchanged", async () => {
  const { view, operations } = await renderAdmin(() =>
    Response.json({ data: { updateUser: { __typename: "UserAccount", id: "u-grace", isAdmin: true, enabled: true } } }),
  );

  await act(async () => {
    view.container.querySelector<HTMLInputElement>("input[aria-label='grace is an admin']")!.click();
  });
  await settle();

  const update = operations.find((operation) => operation.operationName === "UpdateUser");
  assert.deepEqual(update?.variables, {
    id: "u-grace",
    input: { displayName: "Grace", isAdmin: true, enabled: true },
  });
  await view.unmount();
});

test("the server's refusal shows next to the users", async () => {
  const { view } = await renderAdmin(() =>
    Response.json(
      { errors: [{ message: "this is the last enabled admin: make someone else an admin first" }] },
      { status: 400 },
    ),
  );

  await act(async () => {
    view.container.querySelector<HTMLInputElement>("input[aria-label='grace can sign in']")!.click();
  });
  await settle();

  assert.equal(
    view.container.querySelector("[role=alert]")?.textContent,
    "this is the last enabled admin: make someone else an admin first",
  );
  await view.unmount();
});

test("creates a user and sets a password", async () => {
  const { view, operations } = await renderAdmin((operation) =>
    Response.json({
      data:
        operation.operationName === "CreateUser"
          ? { createUser: { __typename: "UserAccount", id: "u-ada" } }
          : { setUserPassword: "u-grace" },
    }),
  );
  const field = <T extends Element>(selector: string) => view.container.querySelector<T>(selector)!;
  field<HTMLInputElement>("input[name=username]").value = "ada";
  field<HTMLInputElement>("input[name=password]").value = "a long enough password";
  field<HTMLInputElement>("input[name=isAdmin]").checked = true;

  await act(async () => {
    field<HTMLInputElement>("input[name=username]").form!.requestSubmit();
  });
  await settle();
  await act(async () => {
    [...view.container.querySelectorAll("button")].find((button) => button.textContent === "Set password")!.click();
  });
  field<HTMLInputElement>("#new-password").value = "another long password";
  await act(async () => {
    field<HTMLInputElement>("#new-password").form!.requestSubmit();
  });
  await settle();

  const create = operations.find((operation) => operation.operationName === "CreateUser");
  assert.deepEqual(create?.variables, {
    input: { username: "ada", displayName: null, password: "a long enough password", isAdmin: true },
  });
  const password = operations.find((operation) => operation.operationName === "SetUserPassword");
  assert.deepEqual(password?.variables, { id: "u-grace", password: "another long password" });
  assert.equal(view.container.querySelector("#new-password"), null, "the form closes once it's set");
  await view.unmount();
});

test("a TEI's switch changes only that switch", async () => {
  const { view, operations } = await renderAdmin(() =>
    Response.json({ data: { updateTei: { __typename: "Tei", url: "http://macbook:8080", enabled: true, queries: true, embedding: true } } }),
  );

  await act(async () => {
    view.container.querySelector<HTMLButtonElement>("[role=switch][aria-label='Queries: http://macbook:8080']")!.click();
  });
  await settle();

  const update = operations.find((operation) => operation.operationName === "UpdateTei");
  assert.deepEqual(update?.variables, { url: "http://macbook:8080", input: { queries: true } });
  assert.ok(operations.filter((operation) => operation.operationName === "Admin").length > 1, "reloaded");
  await view.unmount();
});

test("a model without a TEI for queries or embedding says what that means", async () => {
  const { view } = await renderAdmin(undefined, {
    ...ADMIN_DATA,
    teis: [tei({ url: "http://tei-bge:80", queries: false }), tei({ url: "http://macbook:8080", state: "OFFLINE", embedding: true })].map(
      (offline, index) => (index === 0 ? { ...offline, embedding: false } : offline),
    ),
  });

  const warnings = [...view.container.querySelectorAll("[role=status]")].map((warning) => warning.textContent);
  assert.deepEqual(warnings, [
    "No enabled TEI answers this model's queries: searches with it find keyword matches only.",
    "No enabled TEI embeds this model's chunks: its backlog doesn't move.",
  ]);
  await view.unmount();
});


test("lists the stored embeddings, and only models that aren't configured can be deleted", async () => {
  const { view } = await renderAdmin();
  const rows = [...view.container.querySelectorAll("tr")].map((row) => row.textContent);

  assert.ok(rows.includes("bge-m3 · 1,024 dimensions12,00050 MB≈ 23.5 MB"), rows.join("\n"));
  assert.ok(rows.includes("e5-small · 384 dimensions · not configured1none≈ 776 byteDelete"), rows.join("\n"));
  assert.equal(view.container.querySelectorAll("button[aria-label^='Delete the embeddings of']").length, 1);
  await view.unmount();
});

test("deleting a model's embeddings asks first, naming the model and its count", async (t) => {
  const asked: string[] = [];
  let confirmed = false;
  t.mock.method(window, "confirm", (message: string) => {
    asked.push(message);
    return confirmed;
  });
  const { view, operations } = await renderAdmin(() => Response.json({ data: { deleteEmbeddingModel: "e5-small" } }));
  const button = view.container.querySelector<HTMLButtonElement>("button[aria-label='Delete the embeddings of e5-small']")!;

  await act(async () => button.click());
  await settle();
  assert.equal(operations.filter((operation) => operation.operationName === "DeleteEmbeddingModel").length, 0, "cancelled");

  confirmed = true;
  await act(async () => button.click());
  await settle();

  assert.match(asked[0], /^Delete everything stored for e5-small\? That's 1 embedding,/);
  const remove = operations.filter((operation) => operation.operationName === "DeleteEmbeddingModel");
  assert.deepEqual(
    remove.map((operation) => operation.variables),
    [{ id: "e5-small" }],
  );
  await view.unmount();
});
