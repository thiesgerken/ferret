import assert from "node:assert/strict";
import test from "node:test";

import { linkableUrl } from "./safe-url";

test("refuses scripting schemes the way browsers read them", () => {
  for (const url of [
    "javascript:alert(1)",
    "JavaScript:alert(1)",
    "  \u0001javascript:alert(1)",
    "java\tscript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "VBScript:msgbox",
  ]) {
    assert.equal(linkableUrl(url), null, url);
  }
});

test("keeps web links and custom schemes", () => {
  for (const url of ["https://example.com", "obsidian://open?vault=main&file=a", "mid:1234@example.com"]) {
    assert.equal(linkableUrl(url), url);
  }
  assert.equal(linkableUrl(null), null);
  assert.equal(linkableUrl(""), null);
});
