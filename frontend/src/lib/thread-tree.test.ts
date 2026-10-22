import assert from "node:assert/strict";
import test from "node:test";

import { threadTree, type ThreadNode } from "./thread-tree";

type Entry = { externalId: string; parent: string | null };

const entry = (externalId: string, parent: string | null = null): Entry => ({ externalId, parent });

/** The tree as nested ids, e.g. `["a", [["b", []]]]`. */
function shape(nodes: ThreadNode<Entry>[]): unknown[] {
  return nodes.map((node) => [node.entry.externalId, shape(node.children)]);
}

test("replies hang below what they answer, oldest first", () => {
  const tree = threadTree([entry("a"), entry("b", "a"), entry("c", "a"), entry("d", "b")]);

  assert.deepEqual(shape(tree), [["a", [["b", [["d", []]]], ["c", []]]]]);
});

test("a thread without parents stays a flat list", () => {
  assert.deepEqual(shape(threadTree([entry("a"), entry("b")])), [["a", []], ["b", []]]);
});

test("an entry whose parent is missing starts a tree of its own", () => {
  assert.deepEqual(shape(threadTree([entry("b", "gone"), entry("c", "b")])), [["b", [["c", []]]]]);
});

test("a parent cycle breaks at its oldest entry", () => {
  const tree = threadTree([entry("root"), entry("a", "b"), entry("b", "a")]);

  assert.deepEqual(shape(tree), [["root", []], ["a", [["b", []]]]]);
});
