import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import type { SearchState } from "@/lib/search-url";
import { SearchResults } from "./search-results";

const hit = (id: string, source: string, authors: string[]) => ({
  __typename: "Hit",
  documentId: id,
  source,
  externalId: `${id}.md`,
  title: null,
  url: id === "a" ? "obsidian://open?file=a.md" : " JavaScript:alert(1)",
  authors,
  createdAt: "2026-01-01T12:00:00Z",
  chunk: {
    __typename: "HitChunk",
    startOffset: 10,
    endOffset: 40,
    parts: [
      { __typename: "TextPart", text: "Pay the ", highlight: false },
      { __typename: "TextPart", text: "invoice", highlight: true },
    ],
  },
});

async function renderResults(state: SearchState) {
  const variables: unknown[] = [];
  const changes: SearchState[] = [];
  const client = createGraphqlClient(async (_url, init) => {
    variables.push(JSON.parse(String(init?.body)).variables);
    return Response.json({
      data: { search: [hit("a", "notes", ["thies"]), hit("b", "mail/zoho", ["Allianz", "thies"])] },
    });
  });
  const view = await renderReact(
    <Provider value={client}>
      <SearchResults state={state} onChange={(next) => changes.push(next)} />
    </Provider>,
  );
  await settle();
  return { view, variables, changes };
}

const STATE: SearchState = { q: "invoice", sources: [], authors: [], range: "any" };

test("shows one hit per document with its highlighted snippet and links", async () => {
  const { view, variables } = await renderResults(STATE);

  assert.deepEqual(variables[0], { input: { q: "invoice", sources: null, authors: null, from: null } });
  const items = view.container.querySelectorAll("article");
  assert.equal(items.length, 2);
  assert.equal(items[0].querySelector("mark")?.textContent, "invoice");
  assert.equal(items[0].querySelector("a")?.getAttribute("href"), "/view?id=a&start=10&end=40");
  assert.equal(items[0].querySelector("a")?.textContent, "a.md", "the external id stands in for a missing title");
  assert.match(items[0].textContent ?? "", /Open in origin/);
  assert.doesNotMatch(items[1].textContent ?? "", /Open in origin/);
  await view.unmount();
});

test("offers the hits' sources and authors as chips that filter", async () => {
  const { view, changes } = await renderResults(STATE);
  const chip = (label: string) =>
    [...view.container.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find((b) => b.textContent === label)!;

  assert.deepEqual(
    [...view.container.querySelectorAll("button[aria-pressed]")].map((b) => b.textContent),
    ["notes", "mail/zoho", "thies", "Allianz"],
    "sources, then authors by frequency",
  );
  await act(async () => chip("mail/zoho").click());
  await act(async () => chip("thies").click());

  assert.deepEqual(changes, [
    { ...STATE, sources: ["mail/zoho"] },
    { ...STATE, authors: ["thies"] },
  ]);
  await view.unmount();
});

test("selected filters go into the query and stay as pressed chips", async () => {
  const { view, variables } = await renderResults({ ...STATE, sources: ["paperless"], authors: ["Allianz"], range: "week" });

  const input = (variables[0] as { input: Record<string, unknown> }).input;
  assert.deepEqual(input.sources, ["paperless"]);
  assert.deepEqual(input.authors, ["Allianz"]);
  assert.equal(typeof input.from, "string");
  const pressed = [...view.container.querySelectorAll("button[aria-pressed=true]")].map((b) => b.textContent);
  assert.deepEqual(pressed, ["paperless", "Allianz"]);
  await view.unmount();
});
