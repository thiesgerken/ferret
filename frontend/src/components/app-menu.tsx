"use client";

import { Grip, LogOut } from "lucide-react";
import Link from "next/link";
import { useRef, useState } from "react";
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

// ponytail: the native popover handles outside clicks, Escape and focus; no menu library.
export function AppMenu(props: { user: FragmentType<typeof AppMenuUserFragment> }) {
  const user = useFragment(AppMenuUserFragment, props.user);
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
        aria-label="App menu"
        className="rounded-full p-2 text-muted-foreground hover:bg-accent hover:text-accent-foreground"
      >
        <Grip className="size-5" aria-hidden />
      </button>
      <nav
        id="app-menu"
        ref={menuRef}
        popover="auto"
        className="inset-auto top-14 right-4 m-0 w-56 rounded-xl border border-border bg-background p-1.5 text-foreground shadow-lg"
      >
        <p className="truncate px-3 py-2 text-xs text-muted-foreground">
          Signed in as {user.displayName || user.username}
        </p>
        {sections.map(({ href, label, icon: Icon }) => (
          // The menu outlives client-side navigation, and light dismiss only reacts to clicks outside it.
          <Link key={href} href={href} onClick={() => menuRef.current?.hidePopover()} className={itemClass}>
            <Icon className="size-4 text-muted-foreground" aria-hidden />
            {label}
          </Link>
        ))}
        <hr className="my-1 border-border" />
        <button type="button" onClick={signOut} className={itemClass}>
          <LogOut className="size-4 text-muted-foreground" aria-hidden />
          Sign out
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
