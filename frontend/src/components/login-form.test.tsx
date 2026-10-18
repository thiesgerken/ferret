import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";

import { renderReact, settle } from "../../test/react-test-utils";
import { LoginForm } from "./login-form";

async function submit(container: HTMLElement, username: string, password: string) {
  container.querySelector<HTMLInputElement>("#username")!.value = username;
  container.querySelector<HTMLInputElement>("#password")!.value = password;
  await act(async () => {
    container.querySelector("form")!.requestSubmit();
  });
  await settle();
}

test("posts the credentials and reports success", async (t) => {
  const fetchMock = t.mock.method(globalThis, "fetch", async () => new Response(null, { status: 204 }));
  let signedIn = false;
  const view = await renderReact(<LoginForm onSignedIn={() => (signedIn = true)} />);

  await submit(view.container, "alex", "hunter2");

  const [url, init] = fetchMock.mock.calls[0].arguments as [string, RequestInit];
  assert.equal(url, "/auth/login");
  assert.deepEqual(JSON.parse(String(init.body)), { username: "alex", password: "hunter2" });
  assert.equal(signedIn, true);
  await view.unmount();
});

test("shows the server's error and stays on the form when sign-in is rejected", async (t) => {
  t.mock.method(globalThis, "fetch", async () =>
    Response.json({ error: "invalid username or password" }, { status: 401 }),
  );
  let signedIn = false;
  const view = await renderReact(<LoginForm onSignedIn={() => (signedIn = true)} />);

  await submit(view.container, "alex", "wrong");

  assert.equal(view.container.querySelector("[role=alert]")?.textContent, "invalid username or password");
  assert.equal(signedIn, false);
  await view.unmount();
});
