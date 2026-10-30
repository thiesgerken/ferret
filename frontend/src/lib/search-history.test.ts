import assert from "node:assert/strict";
import test from "node:test";

import { submitSearch, typeSearch } from "./search-history";
import type { SearchState } from "./search-url";

const search = (q: string): SearchState => ({ q, sources: [], authors: [], range: "any", model: null, mode: "hybrid", typoTolerance: true });
const url = () => window.location.search;

/** Next's patched history: it adds its `__NA` marker to every entry's data and takes a call whose
 * data already carries one for its own, which it doesn't pass on to the router. */
function nextRouter() {
  const seen: string[] = [];
  const { pushState, replaceState } = window.history;
  const patch = (original: History["pushState"]) =>
    function (data: Record<string, unknown> | null, unused: string, url?: string | URL | null) {
      if (!data?.__NA) seen.push(String(url));
      original.call(window.history, { ...data, __NA: true }, unused, url);
    };
  window.history.pushState = patch(pushState);
  window.history.replaceState = patch(replaceState);
  return {
    seen,
    restore() {
      window.history.pushState = pushState;
      window.history.replaceState = replaceState;
    },
  };
}

test("every pause in typing reaches Next's router", () => {
  const router = nextRouter();
  try {
    submitSearch(search("start"));
    typeSearch(search("ty"));
    typeSearch(search("typed"));
    submitSearch(search("typed"));
  } finally {
    router.restore();
  }
  assert.deepEqual(router.seen, ["/?q=start", "/?q=ty", "/?q=typed", "/?q=typed"]);
});

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
