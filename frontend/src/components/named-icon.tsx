"use client";

import { CircleDashed } from "lucide-react";
import { DynamicIcon, iconNames, type IconName } from "lucide-react/dynamic";

const KNOWN = new Set<string>(iconNames);

/**
 * A lucide icon stored by name (a connection's or API key's icon). Each icon loads on its own,
 * so the bundle doesn't carry all of them. A name lucide doesn't know, e.g. one a newer version
 * dropped, shows a neutral icon instead of nothing.
 */
export function NamedIcon({ name, className }: { name: string; className?: string }) {
  const neutral = () => <CircleDashed className={className} aria-hidden />;
  if (!KNOWN.has(name)) return neutral();
  return <DynamicIcon name={name as IconName} className={className} fallback={neutral} aria-hidden />;
}

export function isIconName(name: string): boolean {
  return KNOWN.has(name);
}

/** Every lucide icon name, for the picker's search. */
export const ICON_NAMES: readonly string[] = iconNames;
