import assert from "node:assert/strict";
import test from "node:test";

import { snippet } from "./snippet";

const plain = (text: string) => ({ text, highlight: false });
const hit = (text: string) => ({ text, highlight: true });

test("short chunks stay whole", () => {
  const parts = [plain("Pay the "), hit("invoice"), plain(" today.")];
  assert.deepEqual(snippet(parts), parts);
});

test("long chunks are cut around the first highlight", () => {
  const parts = [plain("a".repeat(500) + " "), hit("invoice"), plain(" " + "b".repeat(500))];
  const cut = snippet(parts, 100, 20);
  assert.equal(cut.map((part) => part.text).join("").length, 102);
  assert.ok(cut[0].text.startsWith("…"));
  assert.ok(cut[cut.length - 1].text.endsWith("…"));
  assert.deepEqual(cut[1], hit("invoice"));
});
