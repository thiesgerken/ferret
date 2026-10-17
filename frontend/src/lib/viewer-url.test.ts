import assert from "node:assert/strict";
import test from "node:test";

import { viewerHref } from "./viewer-url";

test("the viewer URL carries the document and an optional hit range", () => {
  assert.equal(viewerHref("d1"), "/view?id=d1");
  assert.equal(viewerHref("d1", { start: 10, end: 42 }), "/view?id=d1&start=10&end=42");
});
