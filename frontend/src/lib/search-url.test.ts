import assert from "node:assert/strict";
import test from "node:test";

import { matchingOn, parseSearch, searchHref, toggle, toggleMatching } from "./search-url";

test("search state round-trips through the URL", () => {
  const state = {
    q: "invoice 2026",
    sources: ["paperless", "mail/personal"],
    authors: ["Allianz"],
    threads: [],
    range: "month" as const,
    model: "qwen3",
    mode: "keyword" as const,
  };
  const href = searchHref(state);
  assert.equal(href, "/?q=invoice+2026&source=paperless&source=mail%2Fpersonal&author=Allianz&range=month&model=qwen3&mode=keyword");
  assert.deepEqual(parseSearch(new URLSearchParams(href.slice(2))), state);
});

test("unknown ranges fall back to any time", () => {
  assert.equal(parseSearch(new URLSearchParams("q=x&range=decade")).range, "any");
});

test("unknown or missing modes are hybrid, which stays out of the URL", () => {
  assert.equal(parseSearch(new URLSearchParams("q=x&mode=fuzzy")).mode, "hybrid");
  assert.equal(parseSearch(new URLSearchParams("q=x")).mode, "hybrid");
  assert.equal(searchHref({ q: "x", sources: [], authors: [], threads: [], range: "any", model: null, mode: "hybrid" }), "/?q=x");
});

test("keyword and semantic matching switch independently, but never both off", () => {
  assert.equal(toggleMatching("hybrid", "keyword"), "semantic");
  assert.equal(toggleMatching("hybrid", "semantic"), "keyword");
  assert.equal(toggleMatching("keyword", "semantic"), "hybrid");
  assert.equal(toggleMatching("semantic", "keyword"), "hybrid");
  assert.equal(toggleMatching("keyword", "keyword"), "keyword", "the only one left stays on");
  assert.ok(matchingOn("hybrid", "semantic") && matchingOn("keyword", "keyword") && !matchingOn("keyword", "semantic"));
});

test("toggle adds and removes", () => {
  assert.deepEqual(toggle(["a"], "b"), ["a", "b"]);
  assert.deepEqual(toggle(["a", "b"], "a"), ["b"]);
});

test("thread identities survive URLs independently of sources and authors", () => {
  const state = {
    q: "hello", sources: ["chats"], authors: ["Alice"],
    threads: [
      { source: "chats/personal", thread: "same/+? #漢字" },
      { source: "chats/work", thread: "same/+? #漢字" },
    ],
    range: "week" as const, model: null, mode: "keyword" as const,
  };
  const href = searchHref(state);
  const params = new URLSearchParams(href.slice(2));
  assert.equal(params.getAll("thread").length, 2);
  assert.deepEqual(parseSearch(params), state);
  assert.throws(() => parseSearch(new URLSearchParams('thread={"source":"chats"}')));
});
