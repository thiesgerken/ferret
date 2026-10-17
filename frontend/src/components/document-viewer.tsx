"use client";

import { ExternalLink } from "lucide-react";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { useMutation, useQuery } from "urql";
import { graphql } from "@/gql";
import type { ViewDocumentQuery } from "@/gql/graphql";
import { errorMessage } from "@/lib/graphql";
import { linkableUrl } from "@/lib/safe-url";
import { rehypeMarkHit, toHitRange, type HitRange } from "@/lib/hit-range";
import { viewerHref } from "@/lib/viewer-url";
import { HomeLink } from "@/components/brand-mark";

const ViewDocumentQueryDocument = graphql(`
  query ViewDocument($id: ID!) {
    document(id: $id) {
      id
      source
      thread
      contentType
      title
      text
      url
      authors
      createdAt
      modifiedAt
      previousInThread {
        id
        title
      }
      nextInThread {
        id
        title
      }
    }
  }
`);

const ExcludeDocumentMutation = graphql(`
  mutation ExcludeDocument($id: ID!, $wholeThread: Boolean!) {
    excludeDocument(id: $id, wholeThread: $wholeThread) {
      deleted
      remembered
    }
  }
`);

type ViewedDocument = NonNullable<ViewDocumentQuery["document"]>;
type Excluded = { deleted: number; remembered: boolean };

/** Character offsets of the hit in the document's text, as the search returns them. */
export type HitOffsets = { start: number; end: number };

export function DocumentViewer({ id, hit }: { id: string; hit: HitOffsets | null }) {
  const [{ data, error }] = useQuery({ query: ViewDocumentQueryDocument, variables: { id } });
  const document = data?.document;
  const [excluded, setExcluded] = useState<Excluded | null>(null);
  const range = useMemo(
    () => (document && hit ? toHitRange(document.text, hit.start, hit.end) : null),
    [document, hit],
  );

  useEffect(() => {
    window.document.getElementById("hit")?.scrollIntoView({ block: "center" });
  }, [range]);

  if (error) {
    return <p className="p-8 text-center text-sm text-destructive">{error.message}</p>;
  }
  if (!data) {
    return null;
  }
  if (excluded) {
    return <ExcludedNotice excluded={excluded} />;
  }
  if (!document) {
    return <p className="p-8 text-center text-sm text-muted-foreground">This document doesn&apos;t exist (anymore).</p>;
  }

  const originUrl = linkableUrl(document.url);
  return (
    <div className="mx-auto max-w-3xl space-y-6 px-4 py-6">
      <HomeLink />
      <header className="space-y-1 border-b border-border pb-4">
        <h1 className="text-xl font-semibold">{document.title || "Untitled"}</h1>
        <p className="text-xs text-muted-foreground">
          {document.source} · modified {new Date(document.modifiedAt).toLocaleString()}
        </p>
        {originUrl && (
          <a href={originUrl} className="inline-flex items-center gap-1 text-sm text-brand hover:underline">
            <ExternalLink className="size-3.5" aria-hidden />
            Open in origin
          </a>
        )}
      </header>
      <DocumentBody document={document} range={range} />
      <ThreadNavigation previous={document.previousInThread} next={document.nextInThread} />
      <ExcludeControls document={document} onExcluded={setExcluded} />
    </div>
  );
}

function DocumentBody({ document, range }: { document: ViewedDocument; range: HitRange | null }) {
  switch (document.contentType) {
    case "MARKDOWN":
      return <MarkdownBody text={document.text} range={range} />;
    case "PLAIN":
      return <PlainBody text={document.text} range={range} />;
    case "EMAIL":
      return (
        <div className="space-y-4">
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 rounded-lg bg-muted p-3 text-sm">
            <dt className="text-muted-foreground">From</dt>
            <dd>{document.authors.join(", ")}</dd>
            <dt className="text-muted-foreground">Subject</dt>
            <dd>{document.title}</dd>
            <dt className="text-muted-foreground">Date</dt>
            <dd>{new Date(document.createdAt).toLocaleString()}</dd>
          </dl>
          <PlainBody text={document.text} range={range} />
        </div>
      );
  }
}

function PlainBody({ text, range }: { text: string; range: HitRange | null }) {
  return (
    <pre className="font-sans text-sm leading-relaxed whitespace-pre-wrap">
      {range ? (
        <>
          {text.slice(0, range.start)}
          <mark id="hit" className="hit">
            {text.slice(range.start, range.end)}
          </mark>
          {text.slice(range.end)}
        </>
      ) : (
        text
      )}
    </pre>
  );
}

const markdownComponents: Components = {
  // Images become links: loading them would tell any remote server that this document was read.
  img: ({ src, alt }) => (
    <a href={typeof src === "string" ? src : undefined} target="_blank" rel="noreferrer">
      {alt || "image"}
    </a>
  ),
  a: ({ href, children }) => (
    <a href={href} target="_blank" rel="noreferrer">
      {children}
    </a>
  ),
};

function MarkdownBody({ text, range }: { text: string; range: HitRange | null }) {
  const rehypePlugins = useMemo(() => (range ? [rehypeMarkHit(range)] : []), [range]);
  return (
    <div className="prose">
      <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={rehypePlugins} components={markdownComponents}>
        {text}
      </ReactMarkdown>
    </div>
  );
}

type ThreadNeighbor = ViewedDocument["previousInThread"];

/** Steps through the document's thread: a chat channel reads as a conversation, mails as a thread. */
function ThreadNavigation({ previous, next }: { previous: ThreadNeighbor; next: ThreadNeighbor }) {
  if (!previous && !next) {
    return null;
  }
  return (
    <nav aria-label="Thread" className="flex justify-between gap-4 border-t border-border pt-4 text-sm">
      {previous ? (
        <Link href={viewerHref(previous.id)} rel="prev" className="text-brand hover:underline">
          ← {previous.title || "Previous in thread"}
        </Link>
      ) : (
        <span />
      )}
      {next && (
        <Link href={viewerHref(next.id)} rel="next" className="text-right text-brand hover:underline">
          {next.title || "Next in thread"} →
        </Link>
      )}
    </nav>
  );
}

/** Removes the document, or its whole thread, from the index for good. */
function ExcludeControls({ document, onExcluded }: { document: ViewedDocument; onExcluded: (excluded: Excluded) => void }) {
  const [{ fetching }, exclude] = useMutation(ExcludeDocumentMutation);
  const [error, setError] = useState("");

  async function handleExclude(wholeThread: boolean) {
    const what = wholeThread ? "this document and the rest of its thread" : "this document";
    if (!window.confirm(`Delete ${what} from ferret, and don't ingest it again?`)) {
      return;
    }
    const result = await exclude({ id: document.id, wholeThread });
    if (result.error || !result.data) {
      setError(result.error ? errorMessage(result.error) : "Nothing was excluded.");
      return;
    }
    onExcluded(result.data.excludeDocument);
  }

  const buttonClass = "rounded-md px-2 py-1 text-destructive hover:bg-accent disabled:opacity-50";
  return (
    <div className="flex flex-wrap items-center gap-2 border-t border-border pt-4 text-sm">
      <span className="text-muted-foreground">Exclude from index:</span>
      <button type="button" disabled={fetching} onClick={() => handleExclude(false)} className={buttonClass}>
        This document
      </button>
      {document.thread && (
        <button type="button" disabled={fetching} onClick={() => handleExclude(true)} className={buttonClass}>
          Its whole thread
        </button>
      )}
      {error && (
        <p role="alert" className="w-full text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}

function ExcludedNotice({ excluded }: { excluded: Excluded }) {
  return (
    <div className="mx-auto max-w-3xl space-y-3 px-4 py-8 text-sm">
      <p role="status">
        {excluded.deleted === 1 ? "1 document" : `${excluded.deleted} documents`} removed from the index.
      </p>
      <p className="text-muted-foreground">
        {excluded.remembered
          ? "Its connection won't ingest it again. Undo that on the Connections page."
          : "No connection owns its source, so nothing remembers this: whatever wrote it there can write it again."}
      </p>
      <Link href="/" className="text-brand hover:underline">
        Back to search
      </Link>
    </div>
  );
}
