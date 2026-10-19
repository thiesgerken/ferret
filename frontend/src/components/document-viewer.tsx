"use client";

import { ExternalLink } from "lucide-react";
import Link from "next/link";
import { useFormatter, useTranslations } from "next-intl";
import { useEffect, useMemo, useState } from "react";
import ReactMarkdown, { type Components, type Options } from "react-markdown";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import { useMutation, useQuery } from "urql";
import { graphql } from "@/gql";
import type { ViewDocumentQuery } from "@/gql/graphql";
import { errorMessage } from "@/lib/graphql";
import { linkableUrl } from "@/lib/safe-url";
import { rehypeMarkHit, toHitRange, type HitRange } from "@/lib/hit-range";
import { viewerHref } from "@/lib/viewer-url";
import { HomeLink } from "@/components/brand-mark";
import { SourceName } from "@/components/source-name";
import { useConnectionRoots } from "@/lib/connection-roots";

const ViewDocumentQueryDocument = graphql(`
  query ViewDocument($id: ID!) {
    document(id: $id) {
      id
      source
      writer {
        kind
        id
        name
        icon
      }
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
  const t = useTranslations();
  const [{ data, error }] = useQuery({ query: ViewDocumentQueryDocument, variables: { id } });
  const roots = useConnectionRoots();
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
    return <p className="p-8 text-center text-sm text-muted-foreground">{t("viewer.gone")}</p>;
  }

  const originUrl = linkableUrl(document.url);
  return (
    <div className="mx-auto max-w-3xl space-y-6 px-4 py-6">
      <HomeLink />
      <header className="space-y-1 border-b border-border pb-4">
        <h1 className="text-xl font-semibold">{document.title || t("viewer.untitled")}</h1>
        {/* A flex line: inline, the source's icon would set its baseline and lift it above the text. */}
        <p className="flex flex-wrap items-center gap-x-1 text-xs text-muted-foreground">
          <SourceName source={document.source} writer={document.writer} roots={roots} />
          {document.modifiedAt && <span>{t("viewer.modified", { date: new Date(document.modifiedAt) })}</span>}
        </p>
        {originUrl && (
          <a href={originUrl} className="inline-flex items-center gap-1 text-sm text-brand hover:underline">
            <ExternalLink className="size-3.5" aria-hidden />
            {t("common.openInOrigin")}
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
  const t = useTranslations("viewer");
  const format = useFormatter();
  switch (document.contentType) {
    case "MARKDOWN":
      return <MarkdownBody text={document.text} range={range} />;
    case "PLAIN":
      return <PlainBody text={document.text} range={range} />;
    case "EMAIL":
      return (
        <div className="space-y-4">
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 rounded-lg bg-muted p-3 text-sm">
            <dt className="text-muted-foreground">{t("from")}</dt>
            <dd>{document.authors.join(", ")}</dd>
            <dt className="text-muted-foreground">{t("subject")}</dt>
            <dd>{document.title}</dd>
            {document.createdAt && <>
              <dt className="text-muted-foreground">{t("date")}</dt>
              <dd>{format.dateTime(new Date(document.createdAt), { dateStyle: "medium", timeStyle: "short" })}</dd>
            </>}
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

function ImageLink({ src, alt }: { src?: unknown; alt?: string }) {
  const t = useTranslations("viewer");
  return (
    <a href={typeof src === "string" ? src : undefined} target="_blank" rel="noreferrer">
      {alt || t("image")}
    </a>
  );
}

const markdownComponents: Components = {
  // Images become links: loading them would tell any remote server that this document was read.
  img: ({ src, alt }) => <ImageLink src={src} alt={alt} />,
  a: ({ href, children }) => (
    <a href={href} target="_blank" rel="noreferrer">
      {children}
    </a>
  ),
};

// A YAML frontmatter (only at the very start, as remark-frontmatter reads it) shows as a code
// block. Without it the closing `---` turns the frontmatter into a setext heading. The block keeps
// the frontmatter's position, so a hit inside it marks it like any other block.
const remarkPlugins = [remarkGfm, remarkFrontmatter];
const remarkRehypeOptions: Options["remarkRehypeOptions"] = {
  handlers: {
    yaml: (_state, node) => ({
      type: "element",
      tagName: "pre",
      properties: {},
      position: node.position,
      children: [
        {
          type: "element",
          tagName: "code",
          properties: { className: ["language-yaml"] },
          children: [{ type: "text", value: node.value }],
        },
      ],
    }),
  },
};

function MarkdownBody({ text, range }: { text: string; range: HitRange | null }) {
  const rehypePlugins = useMemo(() => (range ? [rehypeMarkHit(range)] : []), [range]);
  return (
    <div className="prose">
      <ReactMarkdown
        remarkPlugins={remarkPlugins}
        remarkRehypeOptions={remarkRehypeOptions}
        rehypePlugins={rehypePlugins}
        components={markdownComponents}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}

type ThreadNeighbor = ViewedDocument["previousInThread"];

/** Steps through the document's thread: a chat channel reads as a conversation, mails as a thread. */
function ThreadNavigation({ previous, next }: { previous: ThreadNeighbor; next: ThreadNeighbor }) {
  const t = useTranslations("viewer");
  if (!previous && !next) {
    return null;
  }
  return (
    <nav aria-label={t("thread")} className="flex justify-between gap-4 border-t border-border pt-4 text-sm">
      {previous ? (
        <Link href={viewerHref(previous.id)} rel="prev" className="text-brand hover:underline">
          ← {previous.title || t("previous")}
        </Link>
      ) : (
        <span />
      )}
      {next && (
        <Link href={viewerHref(next.id)} rel="next" className="text-right text-brand hover:underline">
          {next.title || t("next")} →
        </Link>
      )}
    </nav>
  );
}

/** Removes the document, or its whole thread, from the index for good. */
function ExcludeControls({ document, onExcluded }: { document: ViewedDocument; onExcluded: (excluded: Excluded) => void }) {
  const t = useTranslations("viewer");
  const [{ fetching }, exclude] = useMutation(ExcludeDocumentMutation);
  const [error, setError] = useState("");

  async function handleExclude(wholeThread: boolean) {
    if (!window.confirm(wholeThread ? t("confirmThread") : t("confirmDocument"))) {
      return;
    }
    const result = await exclude({ id: document.id, wholeThread });
    if (result.error || !result.data) {
      setError(result.error ? errorMessage(result.error) : t("nothingExcluded"));
      return;
    }
    onExcluded(result.data.excludeDocument);
  }

  const buttonClass = "rounded-md px-2 py-1 text-destructive hover:bg-accent disabled:opacity-50";
  return (
    <div className="flex flex-wrap items-center gap-2 border-t border-border pt-4 text-sm">
      <span className="text-muted-foreground">{t("exclude")}</span>
      <button type="button" disabled={fetching} onClick={() => handleExclude(false)} className={buttonClass}>
        {t("excludeDocument")}
      </button>
      {document.thread && (
        <button type="button" disabled={fetching} onClick={() => handleExclude(true)} className={buttonClass}>
          {t("excludeThread")}
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
  const t = useTranslations("viewer");
  return (
    <div className="mx-auto max-w-3xl space-y-3 px-4 py-8 text-sm">
      <p role="status">
        {t("removed", { count: excluded.deleted })}
      </p>
      <p className="text-muted-foreground">
        {excluded.remembered
          ? t("remembered")
          : t("notRemembered")}
      </p>
      <Link href="/" className="text-brand hover:underline">
        {t("backToSearch")}
      </Link>
    </div>
  );
}
