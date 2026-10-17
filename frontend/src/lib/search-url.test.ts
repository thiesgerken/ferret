import assert from "node:assert/strict";
import test from "node:test";

import { parseSearch, searchHref, toggle } from "./search-url";

test("search state round-trips through the URL", () => {
  const state = { q: "invoice 2026", sources: ["paperless", "mail/zoho"], authors: ["Allianz"], range: "month" as const };
  const href = searchHref(state);
  assert.equal(href, "/?q=invoice+2026&source=paperless&source=mail%2Fzoho&author=Allianz&range=month");
  assert.deepEqual(parseSearch(new URLSearchParams(href.slice(2))), state);
});

test("unknown ranges fall back to any time", () => {
  assert.equal(parseSearch(new URLSearchParams("q=x&range=decade")).range, "any");
});

test("toggle adds and removes", () => {
  assert.deepEqual(toggle(["a"], "b"), ["a", "b"]);
  assert.deepEqual(toggle(["a", "b"], "a"), ["b"]);
});
