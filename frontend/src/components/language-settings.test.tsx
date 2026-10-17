import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";

import { renderReact } from "../../test/react-test-utils";
import { LanguageSettings } from "./language-settings";
import { LocaleProvider } from "./locale-provider";

test("the language switch translates the UI and sticks", async () => {
  const view = await renderReact(
    <LocaleProvider>
      <LanguageSettings />
    </LocaleProvider>,
  );
  const select = view.container.querySelector("select")!;
  // jsdom's navigator speaks English.
  assert.equal(select.value, "system");
  assert.match(view.container.textContent ?? "", /^LanguageSystem \(English\)/);

  await act(async () => {
    select.value = "de";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  assert.match(view.container.textContent ?? "", /^SpracheSystem \(English\)/);
  assert.equal(document.documentElement.lang, "de");
  assert.equal(localStorage.getItem("ferret.locale"), "de");
  await view.unmount();
});
