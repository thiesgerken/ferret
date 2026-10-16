import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import { ApiKeysSettings } from "./api-keys-settings";

type Operation = { operationName: string; variables: Record<string, unknown> };

const KEY = {
  __typename: "ApiKey",
  id: "key-1",
  name: "carapace search skill",
  prefix: "0a1b2c3d",
  scopes: ["SEARCH_READ"],
  sources: null,
  lastUsedAt: null,
  expiresAt: null,
};

/** Renders the settings against a fake server and records every operation it receives. */
async function renderSettings(isAdmin: boolean, respond: (operation: Operation) => unknown = () => null) {
  const operations: Operation[] = [];
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body)) as Operation;
    operations.push(operation);
    const data =
      operation.operationName === "ApiKeys"
        ? { me: { __typename: "User", isAdmin }, apiKeys: [KEY] }
        : respond(operation);
    return Response.json({ data });
  });
  const view = await renderReact(
    <Provider value={client}>
      <ApiKeysSettings />
    </Provider>,
  );
  await settle();
  return { view, operations };
}

test("lists the keys and offers admin scopes to admins only", async () => {
  const { view } = await renderSettings(false);

  assert.match(view.container.textContent ?? "", /carapace search skill fk_0a1b2c3d…/);
  assert.match(view.container.textContent ?? "", /search:read · all sources/);
  assert.match(view.container.textContent ?? "", /Last used never · never expires/);
  assert.equal(view.container.querySelector("select[name=ADMIN]"), null);
  await view.unmount();

  const { view: adminView } = await renderSettings(true);
  assert.ok(adminView.container.querySelector("select[name=ADMIN]"));
  await adminView.unmount();
});

test("creates a key from the form and shows it once", async () => {
  const { view, operations } = await renderSettings(false, () => ({
    createApiKey: { __typename: "CreatedApiKey", key: "fk_0a1b2c3d.secret" },
  }));
  const field = <T extends Element>(selector: string) => view.container.querySelector<T>(selector)!;
  field<HTMLInputElement>("#key-name").value = "paperless";
  field<HTMLSelectElement>("select[name=INGEST]").value = "WRITE";
  field<HTMLInputElement>("#key-sources").value = "paperless, notes";

  await act(async () => {
    field<HTMLFormElement>("form").requestSubmit();
  });
  await settle();

  const create = operations.find((operation) => operation.operationName === "CreateApiKey");
  assert.deepEqual(create?.variables, {
    input: { name: "paperless", scopes: ["INGEST_WRITE"], sources: ["paperless", "notes"], expiresAt: null },
  });
  assert.equal(field("#created-key").textContent, "fk_0a1b2c3d.secret");
  await view.unmount();
});

test("revokes a key after confirmation", async (t) => {
  t.mock.method(window, "confirm", () => true);
  const { view, operations } = await renderSettings(false, () => ({ revokeApiKey: "key-1" }));
  const revokeButton = [...view.container.querySelectorAll("button")].find((b) => b.textContent === "Revoke")!;

  await act(async () => {
    revokeButton.click();
  });
  await settle();

  const revoke = operations.find((operation) => operation.operationName === "RevokeApiKey");
  assert.deepEqual(revoke?.variables, { id: "key-1" });
  await view.unmount();
});
