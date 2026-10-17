"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useTranslations } from "next-intl";
import type { ReactNode } from "react";
import { useQuery } from "urql";
import { HomeLink } from "@/components/brand-mark";
import { graphql } from "@/gql";
import { PAGES } from "@/lib/pages";
import { cn } from "@/lib/utils";

const ShellUserQuery = graphql(`
  query ShellUser {
    me {
      isAdmin
    }
  }
`);

/**
 * The layout of the pages that set ferret up (Connections, Settings, Admin): the list of those
 * pages next to the content on wide screens, above it on narrow ones, and a title with its icon.
 * The columns match the search results', so moving between them doesn't shift the page.
 */
export function PageShell({ href, children }: { href: string; children: ReactNode }) {
  const t = useTranslations("pages");
  const [{ data }] = useQuery({ query: ShellUserQuery });
  // Null outside Next's router (tests).
  const pathname = usePathname();
  const pages = PAGES.filter((page) => page.manage && (!page.adminOnly || data?.me.isAdmin));
  const page = PAGES.find((candidate) => candidate.href === href);
  if (!page) throw new Error(`No page ${href}`);
  const Icon = page.icon;

  return (
    <div className="min-h-dvh">
      <header className="border-b border-border">
        <div className={cn(columns, "py-3 pr-16 lg:pr-4")}>
          <HomeLink />
        </div>
      </header>
      <div className={cn(columns, "pt-4 pb-16 lg:pt-8")}>
        <nav
          aria-label={t("nav")}
          className="-mx-4 flex gap-1 overflow-x-auto px-4 [scrollbar-width:none] lg:sticky lg:top-8 lg:mx-0 lg:flex-col lg:self-start lg:px-0"
        >
          {pages.map(({ href, key, icon: PageIcon }) => (
            <Link
              key={href}
              href={href}
              aria-current={pathname === href ? "page" : undefined}
              className={cn(
                "flex shrink-0 items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-colors",
                pathname === href
                  ? "bg-accent font-medium text-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              <PageIcon className="size-4" aria-hidden />
              {t(`${key}.title`)}
            </Link>
          ))}
        </nav>
        <main className="mt-6 min-w-0 space-y-8 lg:mt-0">
          <div className="flex items-start gap-4">
            <span className="flex size-11 shrink-0 items-center justify-center rounded-xl bg-brand/10 text-brand">
              <Icon className="size-5" aria-hidden />
            </span>
            <div>
              <h1 className="text-2xl font-semibold tracking-tight">{t(`${page.key}.title`)}</h1>
              <p className="mt-1 text-sm text-muted-foreground">{t(`${page.key}.description`)}</p>
            </div>
          </div>
          {children}
        </main>
      </div>
    </div>
  );
}

const columns = "mx-auto max-w-3xl px-4 lg:grid lg:max-w-6xl lg:grid-cols-[14rem_minmax(0,48rem)] lg:gap-x-12";

/** A card for one part of a page: a title with its icon and what it's for, then its content. */
export function Section({
  title,
  description,
  icon,
  children,
}: {
  title: string;
  description?: ReactNode;
  /** A lucide icon element; the card sizes and colors it. */
  icon: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="rounded-2xl border border-border">
      <div className="flex items-start gap-3 border-b border-border px-5 py-4">
        <span className="mt-0.5 shrink-0 text-muted-foreground [&>svg]:size-4" aria-hidden>
          {icon}
        </span>
        <div>
          <h2 className="text-base font-medium">{title}</h2>
          {description && <p className="mt-0.5 text-sm text-muted-foreground">{description}</p>}
        </div>
      </div>
      <div className="p-5">{children}</div>
    </section>
  );
}
