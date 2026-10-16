"use client";

import { Grip, LogOut } from "lucide-react";
import Link from "next/link";
import { useState } from "react";
import { logout } from "@/lib/auth";

type MenuUser = { username: string; displayName?: string | null; isAdmin: boolean };

const SECTIONS = [
  { href: "/explore", label: "Explore" },
  { href: "/connections", label: "Connections" },
  { href: "/settings", label: "Settings" },
];

const itemClass = "flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm hover:bg-accent";

// ponytail: the native popover handles outside clicks, Escape and focus; no menu library.
export function AppMenu({ user }: { user: MenuUser }) {
  const [error, setError] = useState("");
  const sections = user.isAdmin ? [...SECTIONS, { href: "/admin", label: "Admin" }] : SECTIONS;

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
        popover="auto"
        className="inset-auto top-14 right-4 m-0 w-56 rounded-xl border border-border bg-background p-1.5 text-foreground shadow-lg"
      >
        <p className="truncate px-3 py-2 text-xs text-muted-foreground">
          Signed in as {user.displayName || user.username}
        </p>
        {sections.map(({ href, label }) => (
          <Link key={href} href={href} className={itemClass}>
            {label}
          </Link>
        ))}
        <hr className="my-1 border-border" />
        <button type="button" onClick={signOut} className={itemClass}>
          <LogOut className="size-4" aria-hidden />
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
