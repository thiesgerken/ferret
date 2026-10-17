import { Activity, Cable, Compass, Search, Settings, ShieldCheck, type LucideIcon } from "lucide-react";

export type Page = {
  href: string;
  /** Its messages: `pages.{key}.title` and `.description`. */
  key: "search" | "explore" | "activity" | "connections" | "settings" | "admin";
  icon: LucideIcon;
  /** Pages that set ferret up or watch it share one layout with a page list; searching and
   * exploring don't. */
  manage: boolean;
  adminOnly: boolean;
};

/** The app's pages, as the app menu and the settings pages' navigation list them. */
export const PAGES: Page[] = [
  { href: "/", key: "search", icon: Search, manage: false, adminOnly: false },
  { href: "/explore", key: "explore", icon: Compass, manage: false, adminOnly: false },
  { href: "/activity", key: "activity", icon: Activity, manage: true, adminOnly: false },
  { href: "/connections", key: "connections", icon: Cable, manage: true, adminOnly: false },
  { href: "/settings", key: "settings", icon: Settings, manage: true, adminOnly: false },
  { href: "/admin", key: "admin", icon: ShieldCheck, manage: true, adminOnly: true },
];
