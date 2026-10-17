"use client";

import { ExternalLink } from "lucide-react";
import Link from "next/link";
import { useEffect, useMemo } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { useQuery } from "urql";
import { graphql } from "@/gql";
import type { ViewDocumentQuery } from "@/gql/graphql";
import { rehypeMarkHit, toHitRange, type HitRange } from "@/lib/hit-range";

const ViewDocumentQueryDocument = graphql(`
  query ViewDocument($id: ID!) {
    document(id: $id) {
      id
      source
      contentType
      title
      text
      url
      authors
      createdAt
      modifiedAt
    }
  }
`);

type ViewedDocument = NonNullable<ViewDocumentQuery["document"]>;

/** Character offsets of the hit in the document's text, as the search returns them. */
export type HitOffsets = { start: number; end: number };

export function DocumentViewer({ id, hit }: { id: string; hit: HitOffsets | null }) {
  const [{ data, error }] = useQuery({ query: ViewDocumentQueryDocument, variables: { id } });
  const document = data?.document;
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
  if (!document) {
    return <p className="p-8 text-center text-sm text-muted-foreground">This document doesn&apos;t exist (anymore).</p>;
  }

  return (
    <div className="mx-auto max-w-3xl space-y-6 px-4 py-6">
      <Link href="/" className="text-2xl font-semibold tracking-tight text-brand select-none">
        ferret
      </Link>
      <header className="space-y-1 border-b border-border pb-4">
        <h1 className="text-xl font-semibold">{document.title || "Untitled"}</h1>
        <p className="text-xs text-muted-foreground">
          {document.source} · modified {new Date(document.modifiedAt).toLocaleString()}
        </p>
        {document.url && (
          <a href={document.url} className="inline-flex items-center gap-1 text-sm text-brand hover:underline">
            <ExternalLink className="size-3.5" aria-hidden />
            Open in origin
          </a>
        )}
      </header>
      <DocumentBody document={document} range={range} />
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
