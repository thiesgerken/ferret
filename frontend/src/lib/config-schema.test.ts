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
    token: { type: ["string", "null"], writeOnly: true },
    shallow: { type: "boolean", default: true },
  },
});

test("maps the schema's properties to fields, required ones first", () => {
  const fields = configFields(SCHEMA);

  assert.deepEqual(
    fields.map(({ name, kind, required, secret }) => [name, kind, required, secret]),
    [
      ["remote", "text", true, false],
      ["branch", "text", false, false],
      ["exclude", "lines", false, false],
      ["token", "text", false, true],
      ["shallow", "checkbox", false, false],
    ],
  );
});

test("rejects a property the form can't show", () => {
  const schema = JSON.stringify({ properties: { depth: { type: "object" } } });

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

test("renders and submits the files plugin's defaulted byte limit as an integer", () => {
  const schema = JSON.stringify({ properties: {
    max_file_size: { type: "integer", minimum: 0, default: 1048576 },
  } });
  const fields = configFields(schema);
  assert.equal(fields[0].kind, "integer");
  assert.equal(fields[0].default, 1048576);
  assert.equal(fields[0].minimum, 0);
  const form = new FormData();
  assert.deepEqual(configFromForm(fields, form), {});
  for (const limit of ["0", "1048576", "2097152"]) {
    form.set("config.max_file_size", limit);
    assert.deepEqual(configFromForm(fields, form), { max_file_size: Number(limit) });
  }
  for (const invalid of ["1.5", "NaN", "9007199254740992"]) {
    form.set("config.max_file_size", invalid);
    assert.throws(() => configFromForm(fields, form), /safe integer/);
  }
});
