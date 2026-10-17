import assert from "node:assert/strict";
import test from "node:test";
import { act, useState } from "react";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import type { SearchState } from "@/lib/search-url";
import { TYPING_DEBOUNCE_MS } from "./search-field";

/** jsdom has no IntersectionObserver: this one reports an intersection when the test scrolls. */
const observers = new Set<IntersectionObserverCallback>();
globalThis.IntersectionObserver = class {
  constructor(private callback: IntersectionObserverCallback) {}
  observe() {
    observers.add(this.callback);
  }
  disconnect() {
    observers.delete(this.callback);
  }
} as unknown as typeof IntersectionObserver;

async function scrollToEnd() {
  await act(async () => {
    for (const callback of [...observers]) {
      callback([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver);
    }
  });
  await settle();
}
import { SearchResults } from "./search-results";

/** notes belongs to the git connection "Notes", mail/zoho came in through an API key. */
const writerOf = (source: string) =>
  source.startsWith("notes")
    ? { __typename: "Writer", kind: "CONNECTION", id: "c1", name: "Notes", icon: "notebook" }
    : { __typename: "Writer", kind: "API_KEY", id: "k1", name: "mail import", icon: "mail" };

const hit = (id: string, source: string, authors: string[]) => ({
  __typename: "Hit",
  documentId: id,
  source,
  writer: writerOf(source),
  externalId: `${id}.md`,
  title: null,
  url: id === "a" ? "obsidian://open?file=a.md" : " JavaScript:alert(1)",
  authors,
  createdAt: "2026-01-01T12:00:00Z",
  collapsed: id === "b" ? 2 : 0,
  judged: false,
  keywordScore: id === "c" ? null : id === "a" ? 7.25 : 3.5,
  similarity: id === "b" ? null : 0.834,
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
    answered = Promise.resolve(),
  }: {
    semanticUnavailable?: string | null;
    models?: string[];
    searchError?: string | null;
    /** When the search for the initial query gets its answer. */
    answered?: Promise<void>;
  } = {},
) {
  const variables: unknown[] = [];
  const changes: SearchState[] = [];
  const typed: SearchState[] = [];
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body));
    if (operation.operationName === "ConnectionRoots") {
      return Response.json({ data: { connections: [{ __typename: "Connection", id: "c1", source: "notes" }] } });
    }
    if (operation.operationName === "EmbeddingModelNames") {
      return Response.json({
        data: {
          embeddingModels: models.map((name) => ({ __typename: "EmbeddingModel", name, minSimilarity: 0.42 })),
        },
      });
    }
    variables.push(operation.variables);
    if (String(operation.query).includes("setJudgment")) {
      return Response.json({ data: { setJudgment: operation.variables.input.judged } });
    }
    if (operation.variables.input.q === state.q) await answered;
    if (searchError) {
      return Response.json({ data: null, errors: [{ message: searchError }] }, { status: 400 });
    }
    return Response.json({
      data: {
        search: {
          __typename: "SearchResult",
          hits:
            operation.variables.input.q === "many"
              ? operation.variables.input.offset
                ? [hit("b", "mail/zoho", []), hit("d", "notes", [])]
                : [hit("a", "notes", []), hit("b", "mail/zoho", [])]
              : operation.variables.input.q === "invoice"
              ? [hit("a", "notes", ["thies"]), hit("b", "mail/zoho", ["Allianz", "thies"])]
              : operation.variables.input.q === "nothing"
                ? []
                : [hit("c", "notes", [])],
          semanticUnavailable,
          model: state.model ?? models[0],
          total: 37,
          totalIsLowerBound: false,
          documentsInScope: 526,
          tookMs: 84,
          hasMore: operation.variables.input.q === "many" && !operation.variables.input.offset,
          facets: {
            __typename: "SearchFacets",
            sources: [
              { __typename: "SourceCount", value: "notes", count: 12, writer: writerOf("notes") },
              { __typename: "SourceCount", value: "mail/zoho", count: 3, writer: writerOf("mail/zoho") },
              ...(operation.variables.input.q === "grouped"
                ? [{ __typename: "SourceCount", value: "notes/journal", count: 2, writer: writerOf("notes/journal") }]
                : []),
            ],
            authors: [
              { __typename: "FacetCount", value: "thies", count: 9 },
              { __typename: "FacetCount", value: "Allianz", count: 2 },
            ],
            areLowerBounds: false,
          },
        },
      },
    });
  });
  function Page() {
    const [current, setCurrent] = useState(state);
    return (
      <SearchResults
        state={current}
        onChange={(next) => {
          changes.push(next);
          setCurrent(next);
        }}
        onType={(next) => {
          typed.push(next);
          setCurrent(next);
        }}
      />
    );
  }
  const view = await renderReact(
    <Provider value={client}>
      <Page />
    </Provider>,
  );
  await settle();
  return { view, variables, changes, typed };
}

async function type(view: { container: HTMLElement }, text: string) {
  const field = view.container.querySelector<HTMLInputElement>("input[name=q]")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!.call(field, text);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function pauseTyping() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, TYPING_DEBOUNCE_MS + 50));
  });
  await settle();
}

/** The filter chips, not the hits' ✓ buttons, which are toggles as well. */
const CHIPS = "nav[aria-label=Filters] button[aria-pressed]";
const label = (button: Element) => button.querySelector("span.truncate")?.textContent;
/** The source and author chips, without the matching toggles ahead of them. */
const facetChips = (root: Element, selector = CHIPS) =>
  [...root.querySelectorAll<HTMLButtonElement>(selector)].filter((b) => !["Keyword", "Semantic"].includes(label(b) ?? ""));

const STATE: SearchState = { q: "invoice", sources: [], authors: [], range: "any", model: null, mode: "hybrid" };

test("shows one hit per document with its highlighted snippet and links", async () => {
  const { view, variables } = await renderResults(STATE);

  assert.deepEqual(variables[0], { input: { q: "invoice", sources: null, authors: null, range: null, model: null, mode: null } });
  const items = view.container.querySelectorAll("article");
  assert.equal(items.length, 2);
  assert.equal(items[0].querySelector("mark")?.textContent, "invoice");
  assert.equal(items[0].querySelector("a")?.getAttribute("href"), "/view?id=a&start=10&end=40");
  assert.equal(items[0].querySelector("a")?.textContent, "a.md", "the external id stands in for a missing title");
  assert.ok(items[0].querySelector("a[aria-label='Open in origin']"));
  assert.equal(items[1].querySelector("a[aria-label='Open in origin']"), null, "a scripting URL gets no link");
  assert.doesNotMatch(items[0].textContent ?? "", /in this thread/);
  assert.match(items[1].textContent ?? "", /2 more in this thread/);
  await view.unmount();
});

test("says how many documents matched out of how many, and how long it took", async () => {
  const all = await renderResults(STATE);
  assert.match(all.view.container.textContent ?? "", /37 matches · 526 documents in all sources · 84 ms/);
  await all.view.unmount();

  const { view } = await renderResults({ ...STATE, sources: ["carapace", "demo"] });
  assert.match(view.container.textContent ?? "", /37 matches · 526 documents in carapace, demo · 84 ms/);
  await view.unmount();
});

test("each hit shows its relevance as bars, the exact values on hover, and its writer", async () => {
  const { view } = await renderResults(STATE);
  const [a, b] = view.container.querySelectorAll("article");
  const bars = (item: Element) => item.querySelector("[aria-label][tabindex='0']")!;
  const filled = (item: Element) => bars(item).querySelectorAll(".bg-current").length;

  assert.equal(bars(a).getAttribute("aria-label"), "83% similar · all words found (BM25 7.3)");
  assert.equal(filled(a), 4);
  assert.match(bars(a).className, /text-brand/, "every word matched");
  assert.equal(bars(b).getAttribute("aria-label"), "all words found (BM25 3.5)");
  assert.equal(filled(b), 2, "keyword-only: its BM25 against the best one shown");
  assert.equal(a.querySelector("p span[title]")?.textContent, "Notes", "the connection's root is its name");
  assert.equal(b.querySelector("p span[title='mail/zoho']")?.textContent, "mail import › mail › zoho");
  await view.unmount();
});

test("the source line leaves out what a hit doesn't have", async () => {
  const { view } = await renderResults({ ...STATE, q: "no authors" });
  const line = view.container.querySelector("article p")!.textContent;
  assert.equal(line, "Notes · Jan 1, 2026");
  await view.unmount();
});

test("offers the matches' sources and authors with their counts as filters", async () => {
  const { view, changes } = await renderResults(STATE);
  const chip = (name: string) =>
    [...view.container.querySelectorAll<HTMLButtonElement>(CHIPS)].find((b) => label(b) === name)!;

  assert.deepEqual(
    facetChips(view.container).map((b) => `${label(b)} ${b.querySelector(".tabular-nums")?.textContent}`),
    ["Notes 12", "mail import › mail › zoho 3", "thies 9", "Allianz 2"],
    "sources, then authors, as the facets order them",
  );
  await act(async () => chip("mail import › mail › zoho").click());
  await act(async () => chip("thies").click());

  assert.deepEqual(changes, [
    { ...STATE, sources: ["mail/zoho"] },
    { ...STATE, sources: ["mail/zoho"], authors: ["thies"] },
  ]);
  await view.unmount();
});

test("selected filters go into the query and stay as pressed chips", async () => {
  const { view, variables } = await renderResults({ ...STATE, sources: ["paperless"], authors: ["Allianz"], range: "week" });

  const input = (variables[0] as { input: Record<string, unknown> }).input;
  assert.deepEqual(input.sources, ["paperless"]);
  assert.deepEqual(input.authors, ["Allianz"]);
  assert.equal(input.range, "WEEK");
  const pressed = facetChips(view.container, "button[aria-pressed=true]").map(label);
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
  const search = { q: "invoice", sources: ["notes"], authors: null, range: null, model: null, mode: null };
  assert.deepEqual(variables.slice(1), [
    { input: { search, documentId: "a", judged: true } },
    { input: { search, documentId: "a", judged: false } },
  ]);
  await view.unmount();
});

test("typing searches once it pauses, keeping what's typed", async () => {
  const { view, typed, changes } = await renderResults(STATE);

  await type(view, "invoi");
  await type(view, "invoices ");
  assert.deepEqual(typed, [], "nothing while typing");
  await pauseTyping();

  assert.deepEqual(typed, [{ ...STATE, q: "invoices" }]);
  assert.deepEqual(changes, [], "typing replaces the URL, it doesn't push");
  assert.equal(view.container.querySelectorAll("article").length, 1);
  assert.equal(view.container.querySelector<HTMLInputElement>("input[name=q]")!.value, "invoices ");
  await view.unmount();
});

test("submitting searches right away, without a second search once typing pauses", async () => {
  const { view, typed, changes } = await renderResults(STATE);

  await type(view, "invoices");
  await act(async () => view.container.querySelector("form")!.requestSubmit());
  await pauseTyping();

  assert.deepEqual(changes, [{ ...STATE, q: "invoices" }]);
  assert.deepEqual(typed, []);
  await view.unmount();
});

test("a late answer to an earlier query doesn't replace the newer one's hits", async () => {
  let answer!: () => void;
  const { view } = await renderResults(STATE, { answered: new Promise((resolve) => (answer = resolve)) });

  await type(view, "invoices");
  await pauseTyping();
  answer();
  await settle();

  const items = view.container.querySelectorAll("article");
  assert.equal(items.length, 1);
  assert.equal(items[0].querySelector("a")?.textContent, "c.md");
  await view.unmount();
});

test("a chip clicked while typing wins over the typed query", async () => {
  const { view, typed, changes } = await renderResults(STATE);
  const chip = [...view.container.querySelectorAll<HTMLButtonElement>(CHIPS)].find((b) => label(b) === "Notes")!;

  await type(view, "invoices");
  await act(async () => chip.click());
  await pauseTyping();

  assert.deepEqual(changes, [{ ...STATE, sources: ["notes"] }]);
  assert.deepEqual(typed, [], "the typed query belonged to the search before the chip");
  await view.unmount();
});

test("hit-shaped placeholders stand in until the first answer", async () => {
  let answer!: () => void;
  const { view } = await renderResults(STATE, { answered: new Promise((resolve) => (answer = resolve)) });

  assert.ok(view.container.querySelector("ol[aria-busy=true]"));
  answer();
  await settle();
  assert.equal(view.container.querySelector("ol[aria-busy=true]"), null);
  assert.equal(view.container.querySelectorAll("article").length, 2);
  await view.unmount();
});

test("no matches says so, and offers to clear the filters when there are any", async () => {
  const plain = await renderResults({ ...STATE, q: "nothing" });
  assert.match(plain.view.container.textContent ?? "", /No documents match “nothing”/);
  assert.equal(plain.view.container.querySelector("main button"), null);
  await plain.view.unmount();

  const { view, changes } = await renderResults({ ...STATE, q: "nothing", sources: ["notes"], range: "week" });
  const clear = [...view.container.querySelectorAll("main button")].find((b) => b.textContent === "Clear filters")!;
  await act(async () => (clear as HTMLButtonElement).click());
  assert.deepEqual(changes, [{ ...STATE, q: "nothing" }]);
  await view.unmount();
});

test("the next page loads when the list's end scrolls into view, without repeating a hit", async () => {
  const { view, variables } = await renderResults({ ...STATE, q: "many" });
  const ids = () => [...view.container.querySelectorAll("article h3 a, article a[href^='/view']")].map((a) => a.textContent);

  assert.deepEqual(ids(), ["a.md", "b.md"]);
  assert.doesNotMatch(view.container.textContent ?? "", /That's all/);
  await scrollToEnd();

  assert.deepEqual(ids(), ["a.md", "b.md", "d.md"], "b came again on the second page");
  const searches = variables.filter((v) => (v as { input: { q?: string } }).input.q === "many");
  assert.equal((searches.at(-1) as { input: { offset: number } }).input.offset, 2);
  assert.match(view.container.textContent ?? "", /That's all/);
  await view.unmount();
});

test("the column groups a writer's sources under its name, the chips keep full labels", async () => {
  const { view } = await renderResults({ ...STATE, q: "grouped" });
  const nav = view.container.querySelector("nav[aria-label=Filters]")!;
  const heading = [...nav.querySelectorAll("p")].map((p) => p.textContent);
  const buttons = facetChips(nav).slice(0, 3);
  const [full, short] = [0, 1].map((i) => buttons.map((b) => b.querySelectorAll("[data-label] > span")[i]?.textContent ?? null));

  assert.deepEqual(heading, ["Notes"], "only the writer with several sources gets a heading");
  assert.deepEqual(full, ["Notes", "Notes › journal", "mail import › mail › zoho"]);
  assert.deepEqual(short, ["All", "journal", null]);
  await view.unmount();
});

test("keyword and semantic matching switch off one at a time, never both", async () => {
  const { view, variables, changes } = await renderResults(STATE, { models: ["alpha", "beta"] });
  const matching = (name: string) =>
    [...view.container.querySelectorAll<HTMLButtonElement>(CHIPS)].find((button) => label(button) === name)!;
  const text = () => view.container.textContent ?? "";
  assert.equal(matching("Keyword").getAttribute("aria-pressed"), "true");
  assert.equal(matching("Semantic").getAttribute("aria-pressed"), "true");

  await act(async () => matching("Semantic").click());
  await settle();
  assert.deepEqual(changes.at(-1), { ...STATE, mode: "keyword" });
  assert.equal((variables.at(-1) as { input: { mode: string } }).input.mode, "KEYWORD");
  assert.match(text(), /37 keyword matches/);
  assert.equal(matching("Keyword").disabled, true, "the only one left stays on");
  assert.equal(view.container.querySelector('select[aria-label="Embedding model"]'), null, "keyword matching uses no model");

  await act(async () => matching("Semantic").click());
  await act(async () => matching("Keyword").click());
  await settle();
  assert.deepEqual(changes.at(-1), { ...STATE, mode: "semantic" });
  assert.match(text(), /37 semantic matches/);
  await view.unmount();
});
