"use client";

import Link from "next/link";
import { useFormatter, useTranslations } from "next-intl";
import { useEffect, useMemo, useRef } from "react";
import type { ViewDocumentQuery } from "@/gql/graphql";
import { threadTree, type ThreadNode } from "@/lib/thread-tree";
import { viewerHref } from "@/lib/viewer-url";

type Outline = NonNullable<NonNullable<ViewDocumentQuery["document"]>["threadOutline"]>;
type Entry = Outline["entries"][number];

/** The viewed document's thread as a tree of who answered what, with the document marked. */
export function ThreadOutline({ outline, currentId }: { outline: Outline; currentId: string }) {
  const t = useTranslations("viewer");
  const roots = useMemo(() => threadTree(outline.entries), [outline]);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const current = box.current?.querySelector<HTMLElement>("[aria-current]");
    if (box.current && current) {
      box.current.scrollTop = current.offsetTop - box.current.clientHeight / 2;
    }
  }, [currentId]);

  if (outline.total <= 1) {
    return null;
  }
  return (
    <nav aria-label={t("thread")} className="space-y-2 border-t border-border pt-4 text-sm">
      {outline.entries.length < outline.total && (
        <p className="text-xs text-muted-foreground">
          {t("threadWindow", { shown: outline.entries.length, total: outline.total })}
        </p>
      )}
      <div ref={box} className="relative max-h-80 overflow-y-auto">
        <ul>
          {roots.map((root) => (
            <li key={root.entry.id}>
              <Strand node={root} currentId={currentId} />
            </li>
          ))}
        </ul>
      </div>
    </nav>
  );
}

/**
 * A document and its chain of only replies, flat. Where several replies answer one document, each
 * starts an indented strand of its own, so a back-and-forth doesn't creep to the right.
 */
function Strand({ node, currentId }: { node: ThreadNode<Entry>; currentId: string }) {
  const chain = [node];
  while (chain.at(-1)!.children.length === 1) {
    chain.push(chain.at(-1)!.children[0]);
  }
  const fork = chain.at(-1)!.children;
  return (
    <>
      {chain.map((link) => (
        <EntryLine key={link.entry.id} entry={link.entry} current={link.entry.id === currentId} />
      ))}
      {fork.length > 0 && (
        <ul className="ml-3 space-y-1 py-1">
          {fork.map((child) => (
            <li key={child.entry.id} className="border-l border-border pl-2">
              <Strand node={child} currentId={currentId} />
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function EntryLine({ entry, current }: { entry: Entry; current: boolean }) {
  const t = useTranslations("viewer");
  const format = useFormatter();
  const content = (
    <>
      {/* Capped, so a long address can't squeeze the title to nothing on a phone. */}
      <span className="max-w-2/5 shrink-0 truncate">{entry.authors.join(", ")}</span>
      {entry.createdAt && (
        <span className="shrink-0 text-muted-foreground">
          {format.dateTime(new Date(entry.createdAt), { dateStyle: "short", timeStyle: "short" })}
        </span>
      )}
      <span className="min-w-0 truncate text-muted-foreground">{entry.title || t("untitled")}</span>
    </>
  );
  const line = "flex gap-2 rounded px-1 py-0.5";
  return current ? (
    <span aria-current="page" className={`${line} bg-accent font-medium`}>
      {content}
    </span>
  ) : (
    <Link href={viewerHref(entry.id)} className={`${line} hover:bg-accent`}>
      {content}
    </Link>
  );
}
