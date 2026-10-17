"use client";

import { CircleDashed } from "lucide-react";
import { DynamicIcon, iconNames, type IconName } from "lucide-react/dynamic";

const KNOWN = new Set<string>(iconNames);

/**
 * Renders a lucide icon by name. This module holds lucide's map of every icon's import (about
 * 65 KB), so only `named-icon` loads it, lazily. A name lucide doesn't know, e.g. one a newer
 * version dropped, shows a neutral icon instead of nothing.
 */
export default function NamedIconLoader({ name, className }: { name: string; className?: string }) {
  const neutral = () => <CircleDashed className={className} aria-hidden />;
  if (!KNOWN.has(name)) return neutral();
  return <DynamicIcon name={name as IconName} className={className} fallback={neutral} aria-hidden />;
}

export function isIconName(name: string): boolean {
  return KNOWN.has(name);
}

/** Every lucide icon name, for the picker's search. */
export const ICON_NAMES: readonly string[] = iconNames;
