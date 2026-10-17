"use client";

import { ChevronDown } from "lucide-react";
import { lazy, Suspense, useId, useState, type KeyboardEvent } from "react";
import { NamedIcon } from "@/components/named-icon";

// Lazy, so the list of every icon name loads only once a picker opens.
const IconGrid = lazy(() => import("./icon-grid"));

/** A button showing the chosen icon that opens a searchable grid of lucide icons. */
export function IconPicker({
  value,
  onChange,
  label = "Icon",
}: {
  value: string;
  onChange: (name: string) => void;
  label?: string;
}) {
  const [open, setOpen] = useState(false);
  const panel = useId();

  function choose(name: string) {
    onChange(name);
    setOpen(false);
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.stopPropagation();
      setOpen(false);
    }
  }

  return (
    <div
      className="relative inline-block"
      onKeyDown={handleKeyDown}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
      }}
    >
      <button
        type="button"
        aria-label={`${label}: ${value}`}
        aria-expanded={open}
        aria-controls={panel}
        onClick={() => setOpen(!open)}
        className="flex h-10 items-center gap-2 rounded-lg border border-border bg-background px-3 hover:bg-accent"
      >
        <NamedIcon name={value} className="size-5" />
        <ChevronDown className="size-3.5 text-muted-foreground" aria-hidden />
      </button>
      {open && (
        <div
          id={panel}
          className="absolute top-full left-0 z-20 mt-2 w-80 rounded-xl border border-border bg-background p-3 shadow-lg"
        >
          <Suspense fallback={<p className="py-10 text-center text-sm text-muted-foreground">Loading icons…</p>}>
            <IconGrid value={value} onChoose={choose} />
          </Suspense>
        </div>
      )}
    </div>
  );
}
