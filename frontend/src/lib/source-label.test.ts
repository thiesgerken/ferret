import assert from "node:assert/strict";
import test from "node:test";

import { sourceLabel, type Writer } from "./source-label";

const roots = new Map([["c1", "carapace"]]);
const carapace: Writer = { kind: "CONNECTION", id: "c1", name: "Lumi's memory", icon: "brain" };
const seed: Writer = { kind: "API_KEY", id: "k1", name: "demo seed", icon: "key-round" };

test("a connection's source reads as its name and the path below its root", () => {
  assert.deepEqual(sourceLabel("carapace", carapace, roots), { icon: "brain", text: "Lumi's memory" });
  assert.deepEqual(sourceLabel("carapace/files/skills", carapace, roots), {
    icon: "brain",
    text: "Lumi's memory › files › skills",
  });
});

test("an API key's source shows whole after the key's name, an unknown writer as a bare path", () => {
  assert.deepEqual(sourceLabel("demo/notes", seed, roots), { icon: "key-round", text: "demo seed › demo › notes" });
  assert.deepEqual(sourceLabel("demo/notes", null, roots), { icon: null, text: "demo › notes" });
});

test("a parent above a connection's root reads as the connection, and so does any source until roots load", () => {
  const journal: Writer = { kind: "CONNECTION", id: "c2", name: "Journal", icon: "notebook" };
  const withJournal = new Map([["c2", "notes/journal"]]);
  assert.deepEqual(sourceLabel("notes", journal, withJournal), { icon: "notebook", text: "Journal" });
  assert.deepEqual(sourceLabel("carapace/files", carapace, undefined), { icon: "brain", text: "Lumi's memory" });
});
