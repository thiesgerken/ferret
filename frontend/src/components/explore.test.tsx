import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import type { ExploreState } from "@/lib/explore-url";
import { createGraphqlClient } from "@/lib/graphql";
import { Explore } from "./explore";

const STATE: ExploreState = {
  sources: [],
  contentTypes: [],
  authors: [],
  month: null,
  orderBy: "CREATED_AT",
  descending: true,
  page: 1,
};

const EXPLORATION = {
  __typename: "Exploration",
  total: 51,
  documents: [
    {
      __typename: "ExploredDocument",
      id: "d1",
      source: "mail/zoho",
      externalId: "msg-1",
      contentType: "EMAIL",
      title: null,
      authors: ["Ada", "Grace"],
      createdAt: "2026-01-05T12:00:00Z",
      modifiedAt: "2026-01-06T12:00:00Z",
    },
  ],
  facets: {
    __typename: "Facets",
    sources: [{ __typename: "FacetCount", value: "mail/zoho", count: 51 }],
    contentTypes: [{ __typename: "ContentTypeCount", contentType: "EMAIL", count: 51 }],
    authors: [{ __typename: "FacetCount", value: "Ada", count: 30 }],
    months: [{ __typename: "FacetCount", value: "2026-01", count: 51 }],
  },
};

async function renderExplore(state: ExploreState) {
  const variables: unknown[] = [];
  const changes: ExploreState[] = [];
  const client = createGraphqlClient(async (_url, init) => {
    variables.push(JSON.parse(String(init?.body)).variables);
    return Response.json({ data: { explore: EXPLORATION } });
  });
  const view = await renderReact(
    <Provider value={client}>
      <Explore state={state} onChange={(next) => changes.push(next)} />
    </Provider>,
  );
  await settle();
  const button = (text: string) =>
    [...view.container.querySelectorAll("button")].find((element) => element.textContent?.startsWith(text))!;
  return { view, variables, changes, button };
}

test("lists a page of documents that open the viewer, with facet counts", async () => {
  const { view, variables } = await renderExplore({ ...STATE, month: "2026-01" });

  assert.deepEqual(variables[0], {
    input: {
      sources: null,
      contentTypes: null,
      authors: null,
      from: "2026-01-01T00:00:00.000Z",
      to: "2026-02-01T00:00:00.000Z",
      orderBy: "CREATED_AT",
      descending: true,
      offset: 50,
      limit: 50,
    },
  });
  const link = view.container.querySelector("tbody a")!;
  assert.equal(link.getAttribute("href"), "/view?id=d1");
  assert.equal(link.textContent, "msg-1", "the external id stands in for a missing title");
  assert.match(view.container.querySelector("tbody")!.textContent ?? "", /Email.*Ada, Grace/);
  assert.match(view.container.querySelector("aside")!.textContent ?? "", /Ada30/);
  assert.match(view.container.textContent ?? "", /51–51 of 51 documents/);
  await view.unmount();
});

test("a facet narrows the table and starts over at the first page", async () => {
  const { view, changes, button } = await renderExplore(STATE);

  await act(async () => {
    button("mail/zoho").click();
  });
  await act(async () => {
    button("2026-01").click();
  });

  assert.deepEqual(changes, [
    { ...STATE, sources: ["mail/zoho"], page: 0 },
    { ...STATE, month: "2026-01", page: 0 },
  ]);
  await view.unmount();
});

test("a column header sorts by it, and again reverses", async () => {
  const { view, changes, button } = await renderExplore({ ...STATE, page: 0 });

  await act(async () => {
    button("Title").click();
  });
  await act(async () => {
    button("Created").click();
  });

  assert.deepEqual(
    changes.map(({ orderBy, descending }) => [orderBy, descending]),
    [
      ["TITLE", false],
      ["CREATED_AT", false],
    ],
  );
  await view.unmount();
});
