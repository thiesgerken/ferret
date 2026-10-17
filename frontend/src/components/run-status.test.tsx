import assert from "node:assert/strict";
import test from "node:test";

import { renderReact } from "../../test/react-test-utils";
import { RunDuration } from "./run-status";

test("a run reads as running by its status, not by a missing end", async () => {
  const now = new Date("2026-10-17T12:00:00Z");
  const run = { startedAt: "2026-10-17T11:58:00Z", finishedAt: "2026-10-17T11:59:00Z" };
  const view = await renderReact(
    <>
      <RunDuration run={{ ...run, status: "SUCCEEDED" }} now={now} />|
      {/* An API key's burst: its last write so far is `finishedAt`, and more may follow. */}
      <RunDuration run={{ ...run, status: "RUNNING" }} now={now} />|
      <RunDuration run={{ ...run, status: "RUNNING", finishedAt: null }} now={now} />
    </>,
  );
  assert.equal(view.container.textContent, "took 1 min|for 1 min|for 2 min");
  await view.unmount();
});
