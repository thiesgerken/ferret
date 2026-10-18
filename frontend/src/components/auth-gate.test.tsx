import assert from "node:assert/strict";
import test from "node:test";
import { gql, Provider, useQuery } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import { AuthGate } from "./auth-gate";

type Responder = (operationName: string) => Response;

function renderGate(respond: Responder, page = <p id="page">page</p>) {
  const client = createGraphqlClient(async (_url, init) => respond(JSON.parse(String(init?.body)).operationName));
  return renderReact(
    <Provider value={client}>
      <AuthGate>{page}</AuthGate>
    </Provider>,
  );
}

const unauthorized = () => new Response("unauthorized", { status: 401 });

function signedIn(isAdmin: boolean): Responder {
  return () => Response.json({ data: { me: { __typename: "User", username: "thies", displayName: null, isAdmin } } });
}

test("shows the login form instead of the page when the server answers 401", async () => {
  const view = await renderGate(unauthorized);
  await settle();

  assert.ok(view.container.querySelector("input#username"));
  assert.equal(view.container.querySelector("#page"), null);
  await view.unmount();
});

test("shows the page and the app menu when signed in", async () => {
  const view = await renderGate(signedIn(false));
  await settle();

  assert.ok(view.container.querySelector("#page"));
  const menu = view.container.querySelector("#app-menu");
  assert.match(menu?.textContent ?? "", /Signed in as thies/);
  assert.equal(menu?.querySelector("a[href='/admin']"), null);
  await view.unmount();
});

test("offers Admin in the app menu to admins only", async () => {
  const view = await renderGate(signedIn(true));
  await settle();

  assert.ok(view.container.querySelector("#app-menu a[href='/admin']"));
  await view.unmount();
});

const ProbeQuery = gql`
  query Probe {
    me {
      username
    }
  }
`;

function ProbePage() {
  useQuery({ query: ProbeQuery });
  return <p id="page">page</p>;
}

test("switches to the login form when a later query finds the session gone", async () => {
  const me = signedIn(false);
  const view = await renderGate((name) => (name === "Probe" ? unauthorized() : me(name)), <ProbePage />);
  await settle();

  assert.ok(view.container.querySelector("input#username"));
  assert.equal(view.container.querySelector("#page"), null);
  await view.unmount();
});
