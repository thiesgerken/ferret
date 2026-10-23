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
  url = "obsidian://open?file=note.md",
  dates: { createdAt: string | null; modifiedAt: string | null } = {
    createdAt: "2026-01-01T12:00:00Z",
    modifiedAt: "2026-01-02T12:00:00Z",
  },
  threadTotal = 1500,
) {
  window.HTMLElement.prototype.scrollIntoView = () => {};
  const client = createGraphqlClient(async (_url, init) => {
    const operation = JSON.parse(String(init?.body)) as Operation;
    operations.push(operation);
    if (operation.operationName === "ConnectionRoots") {
      return Response.json({ data: { connections: [{ __typename: "Connection", id: "c1", source: "notes" }] } });
    }
    if (operation.operationName === "ExcludeDocument") {
      return Response.json({ data: { excludeDocument: { __typename: "ExcludedDocuments", deleted: 3, remembered: true } } });
    }
    return Response.json({
      data: {
        document: {
          __typename: "Document",
          id: "d",
          source: "notes/2026",
          writer: { __typename: "Writer", kind: "CONNECTION", id: "c1", name: "Notes", icon: "notebook" },
          thread: "lunch",
          contentType,
          title: "A note",
          text,
          url,
          authors: ["thies"],
          ...dates,
          threadOutline: {
            __typename: "ThreadOutline",
            total: threadTotal,
            entries: [
              { __typename: "ThreadEntry", id: "c", externalId: "start", parent: null, title: "Lunch?", authors: ["anna"], createdAt: null },
              { __typename: "ThreadEntry", id: "d", externalId: "reply", parent: "start", title: "Re: Lunch?", authors: ["thies"], createdAt: null },
              { __typename: "ThreadEntry", id: "e", externalId: "other", parent: "start", title: null, authors: ["ben"], createdAt: null },
            ],
          },
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

test("an origin url with a dangerous scheme isn't a link", async () => {
  const view = await renderViewer("PLAIN", "text", null, [], "javascript:alert(document.cookie)");

  assert.doesNotMatch(view.container.textContent ?? "", /Open in origin/);
  assert.equal(view.container.querySelector("a[href^='javascript:']"), null);
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

test("Markdown shows a frontmatter at the very start as a YAML block, not as a heading", async () => {
  const text = "---\nname: paperless\ndescription: Search the archive\n---\n\n# Paperless\n\nBody text.";
  const start = text.indexOf("description");
  const view = await renderViewer("MARKDOWN", text, { start, end: start + 11 });

  const yaml = view.container.querySelector(".prose > pre code.language-yaml");
  assert.equal(yaml?.textContent, "name: paperless\ndescription: Search the archive");
  assert.equal(view.container.querySelector(".prose > pre")?.id, "hit", "the hit in the frontmatter marks its block");
  assert.equal(view.container.querySelector("hr"), null);
  assert.deepEqual([...view.container.querySelectorAll(".prose h1, .prose h2")].map((h) => h.textContent), ["Paperless"]);
  await view.unmount();
});

test("a rule further down isn't a frontmatter", async () => {
  const view = await renderViewer("MARKDOWN", "Intro\n\n---\nkey: value\n---", null);
  assert.equal(view.container.querySelector("code.language-yaml"), null);
  await view.unmount();
});

test("the header names the document's writer, the full source in the tooltip", async () => {
  const view = await renderViewer("PLAIN", "text", null);
  const source = view.container.querySelector("header span[title]");
  assert.equal(source?.getAttribute("title"), "notes/2026");
  assert.equal(source?.textContent, "Notes › 2026");
  await view.unmount();
});

test("email shows its headers above the body", async () => {
  const view = await renderViewer("EMAIL", "Hello,\nthe invoice.", null);

  assert.match(view.container.querySelector("dl")?.textContent ?? "", /FromthiesSubjectA noteDate/);
  assert.equal(view.container.querySelector("#hit"), null);
  await view.unmount();
});

test("undated email omits unknown origin dates instead of formatting substitute dates", async () => {
  const view = await renderViewer("EMAIL", "Undated cobalt correspondence", null, [], "https://example.test", {
    createdAt: null,
    modifiedAt: null,
  });
  assert.doesNotMatch(view.container.querySelector("header")?.textContent ?? "", /modified|1970|Jan 1|Jan 2/i);
  assert.deepEqual([...view.container.querySelectorAll("dt")].map((element) => element.textContent), ["From", "Subject"]);
  assert.match(view.container.textContent ?? "", /Undated cobalt correspondence/);
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

test("the thread outline marks the viewed document among the others and says when it's a window", async () => {
  const view = await renderViewer("PLAIN", "text", null);
  const outline = view.container.querySelector("nav[aria-label='Thread']")!;

  assert.match(outline.querySelector("[aria-current]")?.textContent ?? "", /thies.*Re: Lunch\?/);
  assert.deepEqual(
    [...outline.querySelectorAll("a")].map((link) => link.getAttribute("href")),
    ["/view?id=c", "/view?id=e"],
  );
  assert.match(outline.textContent ?? "", /3 of 1,500 documents, around this one/);
  await view.unmount();
});

test("the thread outline comes between the header and the body, and not at all for a lone document", async () => {
  const threaded = await renderViewer("PLAIN", "the body", null);
  const outline = threaded.container.querySelector("nav[aria-label='Thread']")!;
  const header = threaded.container.querySelector("header")!;
  const body = threaded.container.querySelector("pre")!;
  assert.ok(header.compareDocumentPosition(outline) & Node.DOCUMENT_POSITION_FOLLOWING);
  assert.ok(outline.compareDocumentPosition(body) & Node.DOCUMENT_POSITION_FOLLOWING);
  await threaded.unmount();

  const dates = { createdAt: null, modifiedAt: null };
  const lone = await renderViewer("PLAIN", "the body", null, [], "obsidian://open?file=note.md", dates, 1);
  assert.equal(lone.container.querySelector("nav[aria-label='Thread']"), null);
  assert.equal(lone.container.querySelector("aside"), null);
  await lone.unmount();
});
