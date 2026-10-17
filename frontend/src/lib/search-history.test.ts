import assert from "node:assert/strict";
import test from "node:test";

import { submitSearch, typeSearch } from "./search-history";
import type { SearchState } from "./search-url";

const search = (q: string): SearchState => ({ q, sources: [], authors: [], range: "any", model: null });
const url = () => window.location.search;

async function back() {
  const popped = new Promise((resolve) => window.addEventListener("popstate", resolve, { once: true }));
  window.history.back();
  await popped;
}

test("Back after typing returns to the query typing started from", async () => {
  submitSearch(search("start"));
  typeSearch(search("ty"));
  typeSearch(search("typed"));
  await back();
  assert.equal(url(), "?q=start");
});

test("typing gets one history entry after the submitted query, so Back returns to it", () => {
  submitSearch(search("foo"));
  const start = window.history.length;

  typeSearch(search("ba"));
  typeSearch(search("bar"));
  assert.equal(url(), "?q=bar");
  assert.equal(window.history.length, start + 1, "one entry for all of the typing");

  submitSearch(search("bar"));
  assert.equal(window.history.length, start + 1, "Enter on the typed query keeps its entry");
  typeSearch(search("baz"));
  assert.equal(window.history.length, start + 2, "typing after Enter starts a new entry");

  submitSearch(search("baz, with a chip"));
  typeSearch(search("qux"));
  assert.equal(window.history.length, start + 4, "a chip submits, so typing after it pushes again");
});
