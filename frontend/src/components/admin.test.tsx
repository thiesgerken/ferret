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
      id: "bge-m3",
      isDefault: true,
      dimension: 1024,
      embeddingApi: endpoint({ url: "http://litellm:4000/v1", model: "bge", reachable: false, lastError: "unreachable: connection refused", lastErrorAt: "2026-01-02T11:00:00Z" }),
      queryApi: endpoint({ url: "http://tei-bge:80/v1", model: "BAAI/bge-m3", reachable: true }),
      backlog: [{ __typename: "SourceBacklog", source: "notes", pending: 7, failed: 1 }],
    },
    {
      __typename: "ModelStatus",
      id: "qwen3",
      isDefault: false,
      dimension: 1024,
      embeddingApi: endpoint({ url: "http://tei:80/v1", model: "Qwen/Qwen3-Embedding-0.6B", reachable: null }),
      queryApi: endpoint({ url: "http://tei:80/v1", model: "Qwen/Qwen3-Embedding-0.6B", reachable: null }),
      backlog: [],
    },
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

function endpoint(fields: { url: string; model: string; reachable: boolean | null; lastError?: string; lastErrorAt?: string }) {
  return { __typename: "ModelEndpoint", lastError: null, lastErrorAt: null, ...fields };
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

test("shows users, each model's endpoints with the backlog, and the workers", async () => {
  const { view } = await renderAdmin();
  const text = view.container.textContent ?? "";

  assert.match(text, /grace · Grace/);
  assert.match(text, /bge-m3 · default1,024 dimensions/);
  assert.match(text, /http:\/\/litellm:4000\/v1UnreachableChunks · bge/);
  assert.match(text, /unreachable: connection refused/);
  assert.match(text, /http:\/\/tei-bge:80\/v1ReachableQueries · BAAI\/bge-m3/);
  assert.match(text, /notes71/);
  assert.match(text, /qwen31,024 dimensionshttp:\/\/tei:80\/v1Not asked yetChunks and queries · Qwen\/Qwen3-Embedding-0\.6BEverything is embedded\./);
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
