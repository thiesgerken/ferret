"use client";

import { Search } from "lucide-react";
import { useState } from "react";
import { ICON_NAMES } from "@/components/named-icon-loader";
import { NamedIcon } from "@/components/named-icon";
import { cn } from "@/lib/utils";

/** What a writer of personal data usually is, shown before anything is searched. */
export const CURATED_ICONS = [
  "folder", "folder-open", "notebook", "notebook-pen", "file-text", "files", "archive", "inbox",
  "mail", "message-circle", "messages-square", "at-sign", "book-open", "bookmark", "newspaper", "rss",
  "globe", "code", "git-branch", "terminal", "brain", "bot", "sparkles", "lightbulb",
  "calendar", "clock", "wallet", "receipt", "piggy-bank", "banknote", "credit-card", "house",
  "building-2", "car", "plane", "map-pin", "heart-pulse", "stethoscope", "pill", "dumbbell",
  "utensils", "shopping-cart", "camera", "image", "music", "film", "gamepad-2", "graduation-cap",
  "briefcase", "users", "user", "baby", "dog", "cat", "leaf", "sun",
  "key-round", "plug", "database", "server", "cloud", "shield", "lock",
];

/** Enough to fill the grid; a narrower search finds the rest. Every shown icon is its own request. */
const MAX_RESULTS = 72;

/** The icons whose name contains every word of `query`, those starting with it first. */
export function searchIcons(query: string): string[] {
  const words = query.toLowerCase().trim().split(/[\s-]+/).filter(Boolean);
  if (words.length === 0) return CURATED_ICONS;
  const matches = ICON_NAMES.filter((name) => words.every((word) => name.includes(word)));
  const first = words[0];
  return [...matches.filter((name) => name.startsWith(first)), ...matches.filter((name) => !name.startsWith(first))].slice(
    0,
    MAX_RESULTS,
  );
}

/**
 * The picker's open panel: a search over every lucide icon and the matches as a grid. Its own
 * module, so the list of all icon names loads only when a picker opens.
 */
export default function IconGrid({ value, onChoose }: { value: string; onChoose: (name: string) => void }) {
  const [query, setQuery] = useState("");
  const icons = searchIcons(query);
  return (
    <>
      <label className="flex items-center gap-2 rounded-lg border border-border px-2.5">
        <Search className="size-4 shrink-0 text-muted-foreground" aria-hidden />
        <input
          type="search"
          aria-label="Search icons"
          placeholder="Search all icons"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          autoFocus
          className="h-9 min-w-0 flex-1 bg-transparent text-sm outline-none"
        />
      </label>
      <p className="mt-2 mb-1.5 text-xs text-muted-foreground">
        {query.trim() ? `${icons.length === MAX_RESULTS ? `First ${MAX_RESULTS}` : icons.length} matches` : "Suggestions"}
      </p>
      <div role="listbox" aria-label="Icons" className="grid max-h-64 grid-cols-8 gap-1 overflow-y-auto">
        {icons.map((name) => (
          <button
            key={name}
            type="button"
            role="option"
            aria-selected={name === value}
            aria-label={name}
            title={name}
            onClick={() => onChoose(name)}
            className={cn(
              "flex aspect-square items-center justify-center rounded-md hover:bg-accent",
              name === value && "bg-brand/15 text-brand ring-1 ring-brand/40",
            )}
          >
            <NamedIcon name={name} className="size-4.5" />
          </button>
        ))}
        {icons.length === 0 && <p className="col-span-8 py-6 text-center text-sm text-muted-foreground">No icon matches.</p>}
      </div>
    </>
  );
}
