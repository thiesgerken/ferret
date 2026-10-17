import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Text inputs, selects and textareas. */
export const fieldClass = cn(
  "w-full rounded-lg border border-border bg-background px-3 py-2 text-base sm:text-sm",
  "outline-none transition-colors focus:border-ring focus:ring-2 focus:ring-ring/30",
);
