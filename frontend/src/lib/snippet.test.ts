import assert from "node:assert/strict";
import test from "node:test";

import { plainMarkdown, snippet } from "./snippet";

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

test("Markdown syntax goes, highlights stay on their words", () => {
  const parts = [
    plain("# Auto\n\n## Notes\n\n- The **"),
    hit("Kfz"),
    plain("** policy, see [the letter](https://x.example/a_b) and `code`.\n> quoted *note*"),
  ];

  assert.deepEqual(plainMarkdown(parts), [
    plain("Auto\n\nNotes\n\nThe "),
    hit("Kfz"),
    plain(" policy, see the letter and code.\nquoted note"),
  ]);
});

test("words with underscores and stars in prose survive", () => {
  const parts = [plain("snake_case_name costs 3 * 4 and 2*3, ![alt text](p.png) _done_")];

  assert.deepEqual(plainMarkdown(parts), [plain("snake_case_name costs 3 * 4 and 2*3, alt text done")]);
});

test("a highlight made of syntax alone disappears", () => {
  assert.deepEqual(plainMarkdown([plain("Intro\n"), hit("##"), plain(" Notes")]), [plain("Intro\n"), plain("Notes")]);
});
