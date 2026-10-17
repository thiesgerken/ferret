import { Cable, Compass, Search, Settings, ShieldCheck, type LucideIcon } from "lucide-react";

export type Page = {
  href: string;
  label: string;
  icon: LucideIcon;
  /** Pages that set ferret up share one layout with a page list; searching and exploring don't. */
  manage: boolean;
  adminOnly: boolean;
};

/** The app's pages, as the app menu and the settings pages' navigation list them. */
export const PAGES: Page[] = [
  { href: "/", label: "Search", icon: Search, manage: false, adminOnly: false },
  { href: "/explore", label: "Explore", icon: Compass, manage: false, adminOnly: false },
  { href: "/connections", label: "Connections", icon: Cable, manage: true, adminOnly: false },
  { href: "/settings", label: "Settings", icon: Settings, manage: true, adminOnly: false },
  { href: "/admin", label: "Admin", icon: ShieldCheck, manage: true, adminOnly: true },
];
