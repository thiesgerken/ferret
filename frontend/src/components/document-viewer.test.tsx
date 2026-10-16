import assert from "node:assert/strict";
import test from "node:test";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import { DocumentViewer } from "./document-viewer";

async function renderViewer(contentType: string, text: string, hit: { start: number; end: number } | null) {
  window.HTMLElement.prototype.scrollIntoView = () => {};
  const client = createGraphqlClient(async () =>
    Response.json({
      data: {
        document: {
          __typename: "Document",
          id: "d",
          source: "notes",
          contentType,
          title: "A note",
          text,
          url: "obsidian://open?file=note.md",
          authors: ["thies"],
          createdAt: "2026-01-01T12:00:00Z",
          modifiedAt: "2026-01-02T12:00:00Z",
        },
      },
    }),
  );
  const view = await renderReact(
    <Provider value={client}>
      <DocumentViewer id="d" hit={hit} />
    </Provider>,
  );
  await settle();
  return view;
}

test("plain text marks the hit by character offsets", async () => {
  const view = await renderViewer("PLAIN", "🦦 first line\nthe invoice is due", { start: 17, end: 24 });

  assert.equal(view.container.querySelector("#hit")?.textContent, "invoice");
  assert.equal(view.container.querySelector("a[href^='obsidian:']")?.textContent, "Open in origin");
  await view.unmount();
});

test("Markdown marks the blocks of the hit and never loads images", async () => {
  const text = "# Title\n\nIntro paragraph.\n\nThe paragraph with the hit.\n\n![tracker](https://example.com/p.png)";
  const start = text.indexOf("The paragraph");
  const view = await renderViewer("MARKDOWN", text, { start, end: start + 10 });

  assert.equal(view.container.querySelector("#hit")?.textContent, "The paragraph with the hit.");
  assert.equal(view.container.querySelectorAll(".hit").length, 1);
  assert.equal(view.container.querySelector("img"), null);
  assert.equal(view.container.querySelector("a[href='https://example.com/p.png']")?.textContent, "tracker");
  await view.unmount();
});

test("email shows its headers above the body", async () => {
  const view = await renderViewer("EMAIL", "Hello,\nthe invoice.", null);

  assert.match(view.container.querySelector("dl")?.textContent ?? "", /FromthiesSubjectA noteDate/);
  assert.equal(view.container.querySelector("#hit"), null);
  await view.unmount();
});
