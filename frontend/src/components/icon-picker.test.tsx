import assert from "node:assert/strict";
import test from "node:test";
import { act, useState } from "react";

import { renderReact, settle } from "../../test/react-test-utils";
import { CURATED_ICONS, searchIcons } from "./icon-grid";
import { IconPicker } from "./icon-picker";
import { NamedIcon } from "./named-icon";
import { isIconName } from "./named-icon-loader";

test("every suggested icon is one lucide has", () => {
  assert.deepEqual(CURATED_ICONS.filter((name) => !isIconName(name)), []);
});

test("searching finds icons by every word, names that start with the first word first", () => {
  assert.deepEqual(searchIcons(""), CURATED_ICONS);
  const pig = searchIcons("piggy");
  assert.equal(pig[0], "piggy-bank");
  assert.ok(searchIcons("folder open").every((name) => name.includes("folder") && name.includes("open")));
  assert.ok(searchIcons("o").length <= 72, "a broad search stays a grid, not all icons");
  assert.deepEqual(searchIcons("no-such-icon-anywhere"), []);
});

test("picking an icon from the grid chooses it and closes the grid", async () => {
  function Form() {
    const [icon, setIcon] = useState("folder");
    return (
      <>
        <IconPicker value={icon} onChange={setIcon} />
        <output>{icon}</output>
      </>
    );
  }
  const view = await renderReact(<Form />);
  const toggle = view.container.querySelector<HTMLButtonElement>("button[aria-expanded]")!;

  await act(async () => toggle.click());
  await settle();
  const search = view.container.querySelector<HTMLInputElement>("input[aria-label='Search icons']")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!.call(search, "piggy");
    search.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => view.container.querySelector<HTMLButtonElement>("[role=option][aria-label=piggy-bank]")!.click());

  assert.equal(view.container.querySelector("output")?.textContent, "piggy-bank");
  assert.equal(view.container.querySelector("[role=listbox]"), null);
  assert.equal(toggle.getAttribute("aria-label"), "Icon: piggy-bank");
  await view.unmount();
});

test("an icon name lucide doesn't know shows a neutral icon", async () => {
  const view = await renderReact(<NamedIcon name="gone-in-a-newer-lucide" className="probe" />);
  await settle();
  assert.match(view.container.querySelector("svg.probe")?.getAttribute("class") ?? "", /lucide-circle-dashed/);
  await view.unmount();
});
