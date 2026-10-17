import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import { Provider } from "urql";

import { renderReact, settle } from "../../test/react-test-utils";
import { createGraphqlClient } from "@/lib/graphql";
import { DocumentViewer } from "./document-viewer";

type Operation = { operationName: string; variables: Record<string, unknown> };

/** Serves the document, and answers an exclusion like the server does for a connection's thread. */
async function renderViewer(
  contentType: string,
  text: string,
  hit: { start: number; end: number } | null,
  operations: Operation[] = [],
) {
  window.HTMLElement.prototype.scrollIntoView = () => {};
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body)) as Operation;
    operations.push(operation);
    if (operation.operationName === "ExcludeDocument") {
      return Response.json({ data: { excludeDocument: { __typename: "ExcludedDocuments", deleted: 3, remembered: true } } });
    }
    return Response.json({
      data: {
        document: {
          __typename: "Document",
          id: "d",
          source: "notes",
          thread: "lunch",
          contentType,
          title: "A note",
          text,
          url: "obsidian://open?file=note.md",
          authors: ["thies"],
          createdAt: "2026-01-01T12:00:00Z",
          modifiedAt: "2026-01-02T12:00:00Z",
          previousInThread: null,
          nextInThread: { __typename: "ThreadNeighbor", id: "e", title: null },
        },
      },
    });
  });
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

test("offers the next document of the thread, and no previous one at its start", async () => {
  const view = await renderViewer("PLAIN", "text", null);

  const links = [...view.container.querySelectorAll("nav[aria-label=Thread] a")];
  assert.deepEqual(
    links.map((link) => [link.getAttribute("rel"), link.getAttribute("href"), link.textContent]),
    [["next", "/view?id=e", "Next in thread →"]],
  );
  await view.unmount();
});

test("excludes the whole thread after confirmation and says what happened", async (t) => {
  t.mock.method(window, "confirm", () => true);
  const operations: Operation[] = [];
  const view = await renderViewer("PLAIN", "text", null, operations);
  const button = [...view.container.querySelectorAll("button")].find((b) => b.textContent === "Its whole thread")!;

  await act(async () => {
    button.click();
  });
  await settle();

  const exclude = operations.find((operation) => operation.operationName === "ExcludeDocument");
  assert.deepEqual(exclude?.variables, { id: "d", wholeThread: true });
  assert.match(view.container.querySelector("[role=status]")?.textContent ?? "", /3 documents removed/);
  assert.match(view.container.textContent ?? "", /Its connection won.t ingest it again/);
  await view.unmount();
});
