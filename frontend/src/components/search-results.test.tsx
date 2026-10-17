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
  judged: false,
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

async function renderResults(
  state: SearchState,
  {
    semanticUnavailable = null,
    models = ["alpha"],
    searchError = null,
  }: { semanticUnavailable?: string | null; models?: string[]; searchError?: string | null } = {},
) {
  const variables: unknown[] = [];
  const changes: SearchState[] = [];
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body));
    if (operation.operationName === "EmbeddingModelNames") {
      return Response.json({
        data: { embeddingModels: models.map((name) => ({ __typename: "EmbeddingModel", name })) },
      });
    }
    variables.push(operation.variables);
    if (String(operation.query).includes("setJudgment")) {
      return Response.json({ data: { setJudgment: operation.variables.input.judged } });
    }
    if (searchError) {
      return Response.json({ data: null, errors: [{ message: searchError }] }, { status: 400 });
    }
    return Response.json({
      data: {
        search: {
          __typename: "SearchResult",
          hits: [hit("a", "notes", ["thies"]), hit("b", "mail/zoho", ["Allianz", "thies"])],
          semanticUnavailable,
          model: state.model ?? models[0],
        },
      },
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

/** The filter chips, not the hits' ✓ buttons, which are toggles as well. */
const CHIPS = "main > div:first-child button[aria-pressed]";

const STATE: SearchState = { q: "invoice", sources: [], authors: [], range: "any", model: null };

test("shows one hit per document with its highlighted snippet and links", async () => {
  const { view, variables } = await renderResults(STATE);

  assert.deepEqual(variables[0], { input: { q: "invoice", sources: null, authors: null, range: null, model: null } });
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
    [...view.container.querySelectorAll<HTMLButtonElement>(CHIPS)].find((b) => b.textContent === label)!;

  assert.deepEqual(
    [...view.container.querySelectorAll(CHIPS)].map((b) => b.textContent),
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
  assert.equal(input.range, "WEEK");
  const pressed = [...view.container.querySelectorAll("button[aria-pressed=true]")].map((b) => b.textContent);
  assert.deepEqual(pressed, ["paperless", "Allianz"]);
  await view.unmount();
});

test("says when the hits are keyword matches only", async () => {
  const available = await renderResults(STATE);
  assert.equal(available.view.container.querySelector('[role="status"]'), null);
  await available.view.unmount();

  const { view } = await renderResults(STATE, { semanticUnavailable: "the embedding model qwen3 is unavailable" });
  assert.equal(
    view.container.querySelector('[role="status"]')?.textContent,
    "Keyword matches only: the embedding model qwen3 is unavailable.",
  );
  assert.equal(view.container.querySelectorAll("article").length, 2);
  await view.unmount();
});

test("offers a model choice, naming the default's model, only with several models", async () => {
  const single = await renderResults(STATE);
  assert.equal(single.view.container.querySelector('select[aria-label="Embedding model"]'), null);
  await single.view.unmount();

  const { view, changes } = await renderResults(STATE, { models: ["alpha", "beta"] });
  const select = view.container.querySelector<HTMLSelectElement>('select[aria-label="Embedding model"]')!;
  assert.deepEqual(
    [...select.options].map((option) => option.textContent),
    ["Default model (alpha)", "alpha", "beta"],
  );
  await act(async () => {
    select.value = "beta";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  assert.deepEqual(changes, [{ ...STATE, model: "beta" }]);
  await view.unmount();

  const chosen = await renderResults({ ...STATE, model: "beta" }, { models: ["alpha", "beta"] });
  assert.equal((chosen.variables[0] as { input: { model: string } }).input.model, "beta");
  await chosen.view.unmount();
});

test("a search for a model that's gone keeps the select to switch back", async () => {
  const { view, changes } = await renderResults(
    { ...STATE, model: "gone" },
    { models: ["alpha", "beta"], searchError: 'no embedding model "gone" is configured' },
  );

  assert.match(view.container.querySelector('[role="alert"]')?.textContent ?? "", /"gone" is configured/);
  const select = view.container.querySelector<HTMLSelectElement>('select[aria-label="Embedding model"]')!;
  await act(async () => {
    select.value = "";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  assert.deepEqual(changes, [STATE]);
  await view.unmount();
});

test("a ✓ judges the hit for this search, a second click takes it back", async () => {
  const { view, variables } = await renderResults({ ...STATE, sources: ["notes"] });
  const check = () => view.container.querySelector<HTMLButtonElement>('article button[aria-label="Right hit for this search"]')!;

  assert.equal(check().getAttribute("aria-pressed"), "false");
  await act(async () => check().click());
  await settle();
  assert.equal(check().getAttribute("aria-pressed"), "true");
  await act(async () => check().click());
  await settle();

  assert.equal(check().getAttribute("aria-pressed"), "false");
  const search = { q: "invoice", sources: ["notes"], authors: null, range: null, model: null };
  assert.deepEqual(variables.slice(1), [
    { input: { search, documentId: "a", judged: true } },
    { input: { search, documentId: "a", judged: false } },
  ]);
  await view.unmount();
});
