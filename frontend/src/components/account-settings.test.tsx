import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";

import { renderReact, settle } from "../../test/react-test-utils";
import { AccountSettings } from "./account-settings";

async function submit(container: HTMLElement, current: string, next: string, repeat = next) {
  const field = (id: string) => container.querySelector<HTMLInputElement>(`#password-${id}`)!;
  field("current").value = current;
  field("new").value = next;
  field("repeat").value = repeat;
  await act(async () => {
    container.querySelector("form")!.requestSubmit();
  });
  await settle();
}

test("posts the current and the new password and confirms", async (t) => {
  const fetchMock = t.mock.method(globalThis, "fetch", async () => new Response(null, { status: 204 }));
  const view = await renderReact(<AccountSettings />);

  await submit(view.container, "old password", "a brand new password");

  const [url, init] = fetchMock.mock.calls[0].arguments as [string, RequestInit];
  assert.equal(url, "/auth/password");
  assert.deepEqual(JSON.parse(String(init.body)), {
    current_password: "old password",
    new_password: "a brand new password",
  });
  assert.match(view.container.querySelector("[role=status]")?.textContent ?? "", /Password changed/);
  await view.unmount();
});

test("shows the server's refusal", async (t) => {
  t.mock.method(globalThis, "fetch", async () =>
    Response.json({ error: "the current password is wrong" }, { status: 403 }),
  );
  const view = await renderReact(<AccountSettings />);

  await submit(view.container, "wrong", "a brand new password");

  assert.equal(view.container.querySelector("[role=alert]")?.textContent, "the current password is wrong");
  await view.unmount();
});

test("doesn't send new passwords that don't match", async (t) => {
  const fetchMock = t.mock.method(globalThis, "fetch", async () => new Response(null, { status: 204 }));
  const view = await renderReact(<AccountSettings />);

  await submit(view.container, "old password", "a brand new password", "a brand new pasword");

  assert.equal(fetchMock.mock.callCount(), 0);
  assert.equal(view.container.querySelector("[role=alert]")?.textContent, "The new passwords don't match.");
  await view.unmount();
});
