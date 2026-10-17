import assert from "node:assert/strict";
import test from "node:test";

import { browserLocale, MESSAGES } from "./i18n";

/** Every message's dotted key. */
function keys(messages: object, prefix = ""): string[] {
  return Object.entries(messages).flatMap(([key, value]) =>
    typeof value === "string" ? [`${prefix}${key}`] : keys(value, `${prefix}${key}.`),
  );
}

test("both languages have the same messages", () => {
  const en = keys(MESSAGES.en);
  const de = keys(MESSAGES.de);
  assert.deepEqual(
    en.filter((key) => !de.includes(key)),
    [],
    "missing in de.json",
  );
  assert.deepEqual(
    de.filter((key) => !en.includes(key)),
    [],
    "missing in en.json",
  );
});

test("the browser's first language ferret speaks wins", () => {
  assert.equal(browserLocale(["fr-FR", "de-AT", "en-US"]), "de");
  assert.equal(browserLocale(["en-GB", "de"]), "en");
  assert.equal(browserLocale(["fr"]), "en");
  assert.equal(browserLocale([]), "en");
});
