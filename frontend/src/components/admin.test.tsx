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
      isDefault: true,
      online: true,
      bulkOnline: false,
      lastError: "TEI answered 503",
      lastErrorAt: "2026-01-02T11:00:00Z",
      backlog: [{ __typename: "SourceBacklog", source: "notes", pending: 7, failed: 1 }],
    },
  ],
};

/** Renders the page against a fake server that records every operation and answers mutations
 * with `respond`. */
async function renderAdmin(respond: (operation: Operation) => Response = () => Response.json({})) {
  const operations: Operation[] = [];
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body)) as Operation;
    operations.push(operation);
    if (operation.operationName === "Admin") {
      return Response.json({ data: ADMIN_DATA });
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

test("shows users, embedding health with its backlog, and the workers", async () => {
  const { view } = await renderAdmin();
  const text = view.container.textContent ?? "";

  assert.match(text, /grace · Grace/);
  assert.match(text, /Queries: online · Bulk: offline/);
  assert.match(text, /TEI answered 503/);
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
