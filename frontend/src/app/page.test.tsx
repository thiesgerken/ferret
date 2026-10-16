import assert from "node:assert/strict";
import test from "node:test";

import { renderReact } from "../../test/react-test-utils";
import SearchPage from "./page";

test("search page shows the logo and a search field that submits q to /", async () => {
  const view = await renderReact(<SearchPage />);

  assert.equal(view.container.querySelector("h1")?.textContent, "ferret");
  const form = view.container.querySelector<HTMLFormElement>("form[role=search]");
  assert.equal(form?.getAttribute("action"), "/");
  assert.ok(form.querySelector("input[type=search][name=q]"));

  await view.unmount();
});
