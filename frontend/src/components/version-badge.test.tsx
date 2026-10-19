import assert from "node:assert/strict";
import test from "node:test";
import { version as frontendVersion } from "../../package.json";
import { renderReact } from "../../test/react-test-utils";
import { VersionBadge } from "./version-badge";

test("matching frontend and server versions do not show a mismatch warning", async () => {
  const view = await renderReact(<VersionBadge serverVersion={frontendVersion} />);

  assert.equal(view.container.textContent, `v${frontendVersion}`);
  assert.equal(view.container.querySelector(".sr-only"), null);
  await view.unmount();
});

test("a different server version is displayed and announces the mismatch", async () => {
  const serverVersion = "99.98.97";
  const view = await renderReact(<VersionBadge serverVersion={serverVersion} />);

  assert.ok(view.container.textContent?.startsWith(`v${serverVersion}`));
  assert.ok(view.container.querySelector(".sr-only"));
  const tooltip = view.container.querySelector("[title]")?.getAttribute("title");
  assert.ok(tooltip?.includes(frontendVersion));
  assert.ok(tooltip?.includes(serverVersion));
  await view.unmount();
});
