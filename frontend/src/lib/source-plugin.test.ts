import assert from "node:assert/strict";
import test from "node:test";

import { pluginOf } from "./source-plugin";

const connections = [
  { source: "carapace", plugin: "carapace" },
  { source: "ferret", plugin: "git" },
  { source: "ferret/docs", plugin: "paperless" },
];

test("a source belongs to the connection with the closest root at or above it", () => {
  assert.equal(pluginOf("carapace/files", connections), "carapace");
  assert.equal(pluginOf("ferret", connections), "git");
  assert.equal(pluginOf("ferret/docs/adr", connections), "paperless");
});

test("a source no connection owns came in through an API key", () => {
  assert.equal(pluginOf("demo/notes", connections), null);
  assert.equal(pluginOf("carapacex", connections), null, "a root matches whole path segments only");
});
