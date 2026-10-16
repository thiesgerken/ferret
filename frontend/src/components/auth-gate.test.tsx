import assert from "node:assert/strict";
import test from "node:test";
import { Client, fetchExchange, Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { AuthGate } from "./auth-gate";

function renderGate(graphqlResponse: Response) {
  const client = new Client({
    url: "https://ferret.example.test/graphql",
    exchanges: [fetchExchange],
    fetch: async () => graphqlResponse,
  });
  return renderReact(
    <Provider value={client}>
      <AuthGate>
        <p id="page">page</p>
      </AuthGate>
    </Provider>,
  );
}

function meResponse(isAdmin: boolean): Response {
  return Response.json({ data: { me: { username: "thies", displayName: null, isAdmin } } });
}

test("shows the login form instead of the page when the server answers 401", async () => {
  const view = await renderGate(new Response("unauthorized", { status: 401 }));
  await settle();

  assert.ok(view.container.querySelector("input#username"));
  assert.equal(view.container.querySelector("#page"), null);
  await view.unmount();
});

test("shows the page and the app menu when signed in", async () => {
  const view = await renderGate(meResponse(false));
  await settle();

  assert.ok(view.container.querySelector("#page"));
  const menu = view.container.querySelector("#app-menu");
  assert.match(menu?.textContent ?? "", /Signed in as thies/);
  assert.equal(menu?.querySelector("a[href='/admin']"), null);
  await view.unmount();
});

test("offers Admin in the app menu to admins only", async () => {
  const view = await renderGate(meResponse(true));
  await settle();

  assert.ok(view.container.querySelector("#app-menu a[href='/admin']"));
  await view.unmount();
});
