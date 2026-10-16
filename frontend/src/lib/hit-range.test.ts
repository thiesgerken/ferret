import assert from "node:assert/strict";
import test from "node:test";

import { toHitRange } from "./hit-range";

test("character offsets equal string indices for BMP text", () => {
  assert.deepEqual(toHitRange("hello world", 6, 11), { start: 6, end: 11 });
});

test("characters outside the BMP shift the string indices", () => {
  const text = "🦦 otter 🦦 ferret";
  const range = toHitRange(text, 10, 16);
  assert.equal(text.slice(range.start, range.end), "ferret");
});

test("a range up to the end of the text ends at its length", () => {
  assert.deepEqual(toHitRange("ab🦦", 1, 3), { start: 1, end: 4 });
});
