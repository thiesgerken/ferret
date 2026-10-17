import assert from "node:assert/strict";
import test from "node:test";

import { configFields, configFromForm } from "./config-schema";

const SCHEMA = JSON.stringify({
  type: "object",
  required: ["remote"],
  properties: {
    branch: { type: ["string", "null"], default: null, description: "The branch." },
    exclude: { type: "array", items: { type: "string" }, default: [] },
    remote: { type: "string", description: "The repository." },
    shallow: { type: "boolean", default: true },
  },
});

test("maps the schema's properties to fields, required ones first", () => {
  const fields = configFields(SCHEMA);

  assert.deepEqual(
    fields.map(({ name, kind, required }) => [name, kind, required]),
    [
      ["remote", "text", true],
      ["branch", "text", false],
      ["exclude", "lines", false],
      ["shallow", "checkbox", false],
    ],
  );
});

test("rejects a property the form can't show", () => {
  const schema = JSON.stringify({ properties: { depth: { type: "integer" } } });

  assert.throws(() => configFields(schema), /"depth"/);
});

test("builds the config from the form and leaves empty fields to the plugin's defaults", () => {
  const form = new FormData();
  form.set("config.remote", " https://example.test/notes.git ");
  form.set("config.branch", "");
  form.set("config.exclude", ".obsidian/**\n\n  drafts/**  \n");

  assert.deepEqual(configFromForm(configFields(SCHEMA), form), {
    remote: "https://example.test/notes.git",
    exclude: [".obsidian/**", "drafts/**"],
    shallow: false,
  });
});
