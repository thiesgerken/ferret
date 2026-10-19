import assert from "node:assert/strict";
import test from "node:test";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import SearchPage from "./page";

const client = createGraphqlClient(async () =>
  Response.json({
    data: {
      indexSummary: {
        __typename: "IndexSummary",
        documents: 18945,
        chunks: 52113,
        sources: 3,
      },
    },
  }),
);

test("the home page shows the logo, a search field that submits q to /, and the index's size", async () => {
  const view = await renderReact(
    <Provider value={client}>
      <SearchPage />
    </Provider>,
  );
  await settle();

  assert.equal(view.container.querySelector("h1")?.textContent, "ferret");
  const form = view.container.querySelector<HTMLFormElement>("form[role=search]");
  assert.equal(form?.getAttribute("action"), "/");
  assert.ok(form.querySelector("input[type=search][name=q]"));
  assert.match(view.container.textContent ?? "", /52,113 chunks · 18,945 documents · 3 sources/);

  await view.unmount();
});
