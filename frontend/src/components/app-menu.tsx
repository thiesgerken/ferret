"use client";

import { LogOut } from "lucide-react";
import Link from "next/link";
import { useTranslations } from "next-intl";
import { useRef, useState } from "react";
import { VersionBadge } from "@/components/version-badge";
import { graphql, useFragment, type FragmentType } from "@/gql";
import { logout } from "@/lib/auth";
import { PAGES } from "@/lib/pages";

const AppMenuUserFragment = graphql(`
  fragment AppMenuUser on User {
    username
    displayName
    isAdmin
  }
`);

const itemClass = "flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm hover:bg-accent";
const menuGroups = [
  { key: "discover", manage: false },
  { key: "manage", manage: true },
] as const;

// ponytail: the native popover handles outside clicks, Escape and focus; no menu library.
export function AppMenu(props: { user: FragmentType<typeof AppMenuUserFragment>; serverVersion: string }) {
  const t = useTranslations();
  const user = useFragment(AppMenuUserFragment, props.user);
  const accountName = user.displayName?.trim() || user.username;
  const menuRef = useRef<HTMLElement>(null);
  const [error, setError] = useState("");
  const sections = PAGES.filter((page) => user.isAdmin || !page.adminOnly);

  async function signOut() {
    setError("");
    try {
      await logout();
      // A full page load, not a client-side push: it must drop the signed-out user's cached data.
      // eslint-disable-next-line @next/next/no-location-assign-relative-destination
      window.location.assign("/");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <>
      <button
        type="button"
        popoverTarget="app-menu"
        aria-label={t("app.menu")}
        title={accountName}
        className="flex size-9 items-center justify-center rounded-full border border-border bg-background text-xs font-semibold text-muted-foreground hover:bg-accent hover:text-accent-foreground"
      >
        {accountName.slice(0, 1).toLocaleUpperCase()}
      </button>
      <nav
        id="app-menu"
        ref={menuRef}
        popover="auto"
        className="inset-auto top-14 right-4 m-0 max-h-[calc(100dvh-4.5rem)] w-64 overflow-y-auto rounded-xl border border-border bg-background p-1.5 text-foreground shadow-lg"
      >
        {menuGroups.map(({ key, manage }) => (
          <section key={key} aria-labelledby={`app-menu-${key}`} className="border-t border-border py-1 first:border-t-0">
            <h2 id={`app-menu-${key}`} className="px-3 pt-2 pb-1 text-xs font-medium text-muted-foreground">
              {t(`app.groups.${key}`)}
            </h2>
            {sections.filter((page) => page.manage === manage).map(({ href, key, icon: Icon }) => (
              // The menu outlives client-side navigation, and light dismiss only reacts to clicks outside it.
              <Link key={href} href={href} onClick={() => menuRef.current?.hidePopover()} className={itemClass}>
                <Icon className="size-4 text-muted-foreground" aria-hidden />
                {t(`pages.${key}.title`)}
              </Link>
            ))}
          </section>
        ))}
        <hr className="my-1 border-border" />
        <div className="flex items-center gap-2 px-3">
          <a
            href="https://github.com/thiesgerken/ferret"
            target="_blank"
            rel="noreferrer"
            onClick={() => menuRef.current?.hidePopover()}
            className="flex min-w-0 flex-1 items-center gap-2 rounded-md py-2 text-sm hover:bg-accent"
          >
            <svg viewBox="0 0 24 24" fill="currentColor" className="size-4 shrink-0 text-muted-foreground" aria-hidden>
              <path d="M12 1.5C6.201 1.5 1.5 6.201 1.5 12c0 4.64 3.01 8.577 7.184 9.966.525.097.716-.228.716-.507 0-.25-.009-.913-.015-1.792-2.922.635-3.538-1.409-3.538-1.409-.478-1.213-1.168-1.536-1.168-1.536-.955-.652.072-.639.072-.639 1.056.074 1.611 1.084 1.611 1.084.939 1.608 2.463 1.144 3.063.875.095-.68.368-1.144.668-1.407-2.333-.265-4.785-1.166-4.785-5.192 0-1.147.41-2.085 1.082-2.82-.108-.266-.469-1.336.102-2.786 0 0 .882-.282 2.89 1.077A10.048 10.048 0 0 1 12 6.59c.892.004 1.79.121 2.629.355 2.006-1.359 2.887-1.077 2.887-1.077.573 1.45.212 2.52.104 2.786.674.735 1.08 1.673 1.08 2.82 0 4.036-2.456 4.924-4.797 5.184.378.325.714.965.714 1.946 0 1.406-.013 2.54-.013 2.886 0 .282.189.609.723.506A10.503 10.503 0 0 0 22.5 12c0-5.799-4.701-10.5-10.5-10.5Z" />
            </svg>
            <span className="truncate">thiesgerken/ferret</span>
          </a>
          <div className="min-w-0 max-w-20 text-xs text-muted-foreground">
            <VersionBadge serverVersion={props.serverVersion} />
          </div>
        </div>
        <hr className="my-1 border-border" />
        <p className="truncate px-3 py-2 text-xs text-muted-foreground">
          {t("app.signedInAs", { name: accountName })}
        </p>
        <button type="button" onClick={signOut} className={itemClass}>
          <LogOut className="size-4 text-muted-foreground" aria-hidden />
          {t("app.signOut")}
        </button>
        {error && (
          <p role="alert" className="px-3 py-2 text-xs text-destructive">
            {error}
          </p>
        )}
      </nav>
    </>
  );
}
