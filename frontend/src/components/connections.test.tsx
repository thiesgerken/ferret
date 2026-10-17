import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import { Connections } from "./connections";

type Operation = { operationName: string; variables: Record<string, unknown> };

const GIT_SCHEMA = JSON.stringify({
  type: "object",
  required: ["remote"],
  properties: {
    branch: { type: ["string", "null"], default: null, description: "The branch to index." },
    exclude: { type: "array", items: { type: "string" }, default: [], description: "Globs of files to skip." },
    remote: { type: "string", description: "The repository to clone." },
    token: { type: ["string", "null"], writeOnly: true, description: "For a private repository." },
  },
});

const CONNECTION = {
  __typename: "Connection",
  id: "connection-1",
  name: "Notes",
  icon: "notebook",
  plugin: "git",
  source: "notes",
  config: JSON.stringify({ remote: "https://example.test/notes.git" }),
  schedule: "0 * * * *",
  enabled: true,
  nextRunAt: "2999-01-01T00:00:00Z",
  runs: [
    {
      __typename: "PluginRun",
      id: "run-1",
      status: "FAILED",
      startedAt: "2026-10-17T10:00:00Z",
      finishedAt: "2026-10-17T10:00:05Z",
      stats: null,
      log: "cloning https://example.test/notes.git",
      error: "git clone failed: repository not found",
    },
  ],
  exclusions: [
    { __typename: "Exclusion", id: "exclusion-1", source: "notes", externalId: null, thread: "lunch" },
  ],
};

/** Renders the page against a fake server that records every operation and answers mutations with `respond`. */
async function renderConnections(
  respond: (operation: Operation) => Response = () => Response.json({ data: {} }),
  /** Null for none at all. */
  connection: typeof CONNECTION | null = CONNECTION,
) {
  const operations: Operation[] = [];
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body)) as Operation;
    operations.push(operation);
    if (operation.operationName === "Connections") {
      return Response.json({
        data: {
          pluginTypes: [{ __typename: "PluginType", name: "git", configSchema: GIT_SCHEMA }],
          connections: connection ? [connection] : [],
        },
      });
    }
    return respond(operation);
  });
  const view = await renderReact(
    <Provider value={client}>
      <Connections />
    </Provider>,
  );
  await settle();
  const button = (text: string) =>
    [...view.container.querySelectorAll("button")].find((element) => element.textContent === text)!;
  return { view, operations, button };
}

test("shows each connection with its last run, run history and log", async () => {
  const { view } = await renderConnections();
  const text = view.container.textContent ?? "";

  assert.match(text, /NotesFailedgit · notes · 0 \* \* \* \*/, "name, status at a glance, then plugin, source and schedule");
  assert.match(text, /Failed .* · next run /);
  assert.match(text, /git clone failed: repository not found/);
  assert.match(text, /cloning https:\/\/example\.test\/notes\.git/);
  await view.unmount();
});

test("creates a connection with the config from the plugin's schema", async () => {
  const { view, operations, button } = await renderConnections(() =>
    Response.json({ data: { createConnection: { __typename: "Connection", id: "connection-2" } } }),
  );
  assert.equal(view.container.querySelector("form"), null, "the form opens on request");
  await act(async () => button("New connection").click());
  const field = <T extends Element>(selector: string) => view.container.querySelector<T>(selector)!;
  assert.deepEqual(
    [...view.container.querySelectorAll("fieldset [name]")].map((element) => element.getAttribute("name")),
    ["config.remote", "config.branch", "config.exclude", "config.token"],
  );
  field<HTMLInputElement>("#connection-name").value = "ferret's code";
  field<HTMLInputElement>("#connection-source").value = "code/ferret";
  field<HTMLInputElement>("#config-remote").value = "https://example.test/ferret.git";
  field<HTMLTextAreaElement>("#config-exclude").value = "target/**\nnode_modules/**";

  await act(async () => {
    field<HTMLFormElement>("form").requestSubmit();
  });
  await settle();

  const create = operations.find((operation) => operation.operationName === "CreateConnection");
  const input = create?.variables.input as Record<string, string>;
  assert.equal(input.plugin, "git");
  assert.equal(input.name, "ferret's code");
  assert.equal(input.icon, "git-branch", "the plugin's default icon");
  assert.equal(input.source, "code/ferret");
  assert.equal(input.schedule, "0 * * * *");
  assert.deepEqual(JSON.parse(input.config), {
    remote: "https://example.test/ferret.git",
    exclude: ["target/**", "node_modules/**"],
  });
  await view.unmount();
});

test("disables a connection, keeping its config and schedule", async () => {
  const { view, operations, button } = await renderConnections(() =>
    Response.json({ data: { updateConnection: { __typename: "Connection", id: "connection-1" } } }),
  );

  await act(async () => {
    button("Disable").click();
  });
  await settle();

  const update = operations.find((operation) => operation.operationName === "UpdateConnection");
  assert.deepEqual(update?.variables, {
    id: "connection-1",
    input: { name: "Notes", icon: "notebook", config: CONNECTION.config, schedule: "0 * * * *", enabled: false },
  });
  await view.unmount();
});

test("edits the config and schedule, starting from the stored ones", async () => {
  const { view, operations, button } = await renderConnections(() =>
    Response.json({ data: { updateConnection: { __typename: "Connection", id: "connection-1" } } }),
  );
  await act(async () => {
    button("Edit").click();
  });
  const field = <T extends Element>(selector: string) => view.container.querySelector<T>(selector)!;
  const remote = field<HTMLInputElement>("#edit-connection-1-remote");
  assert.equal(remote.value, "https://example.test/notes.git");
  const token = field<HTMLInputElement>("#edit-connection-1-token");
  assert.equal(token.type, "password");
  assert.equal(token.value, "");
  assert.equal(token.required, false, "an empty secret keeps the stored one");
  remote.value = "https://example.test/notes-v2.git";
  field<HTMLInputElement>("#schedule-connection-1").value = "0 3 * * *";

  await act(async () => {
    button("Save").closest("form")!.requestSubmit();
  });
  await settle();

  const update = operations.find((operation) => operation.operationName === "UpdateConnection");
  const input = update?.variables.input as Record<string, unknown>;
  assert.deepEqual(JSON.parse(String(input.config)), { remote: "https://example.test/notes-v2.git" });
  assert.equal(input.schedule, "0 3 * * *");
  assert.equal(input.enabled, true);
  assert.equal(input.name, "Notes", "name and icon start from the stored ones");
  assert.equal(input.icon, "notebook");
  await view.unmount();
});

test("an edit keeps a disabled connection disabled", async () => {
  const { view, operations, button } = await renderConnections(
    () => Response.json({ data: { updateConnection: { __typename: "Connection", id: "connection-1" } } }),
    { ...CONNECTION, enabled: false },
  );
  await act(async () => {
    button("Edit").click();
  });
  await act(async () => {
    button("Save").closest("form")!.requestSubmit();
  });
  await settle();

  const update = operations.find((operation) => operation.operationName === "UpdateConnection");
  assert.equal((update?.variables.input as Record<string, unknown>).enabled, false);
  await view.unmount();
});

test("deletes only once the source is typed", async () => {
  const { view, operations, button } = await renderConnections(() => Response.json({ data: { deleteConnection: 3 } }));
  await act(async () => {
    button("Delete").click();
  });
  const confirm = view.container.querySelector<HTMLInputElement>("#confirm-connection-1")!;
  const submit = button("Delete connection and documents");
  assert.equal(submit.disabled, true);

  await act(async () => {
    // React tracks the value through the native setter, so a plain assignment wouldn't fire onChange.
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!.call(confirm, "notes");
    confirm.dispatchEvent(new window.Event("input", { bubbles: true }));
  });
  assert.equal(submit.disabled, false);
  await act(async () => {
    submit.click();
  });
  await settle();

  const remove = operations.find((operation) => operation.operationName === "DeleteConnection");
  assert.deepEqual(remove?.variables, { id: "connection-1", confirmSource: "notes" });
  await view.unmount();
});

test("lists the exclusions and includes one again", async () => {
  const { view, operations, button } = await renderConnections(() =>
    Response.json({ data: { removeExclusion: "exclusion-1" } }),
  );
  assert.match(view.container.textContent ?? "", /Excluded from the index \(1\).*Thread lunch in notes/);

  await act(async () => {
    button("Include again").click();
  });
  await settle();

  const remove = operations.find((operation) => operation.operationName === "RemoveExclusion");
  assert.deepEqual(remove?.variables, { id: "exclusion-1" });
  await view.unmount();
});

test("without connections, an empty state offers to create the first one", async () => {
  const { view } = await renderConnections(undefined, null);
  assert.match(view.container.textContent ?? "", /No connections yet/);
  assert.ok([...view.container.querySelectorAll("button")].some((b) => b.textContent === "New connection"));
  await view.unmount();
});
