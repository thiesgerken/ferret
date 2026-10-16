import { Search } from "lucide-react";
import type { FormEvent } from "react";
import { cn } from "@/lib/utils";

/**
 * A GET form to `/` with `q`, so it also works before JS loads. With JS, `onSearch` takes over and
 * keeps the page.
 */
export function SearchField({
  defaultValue = "",
  onSearch,
  className,
}: {
  defaultValue?: string;
  onSearch: (q: string) => void;
  className?: string;
}) {
  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const q = String(new FormData(event.currentTarget).get("q")).trim();
    if (q) onSearch(q);
  }

  return (
    <form role="search" action="/" onSubmit={handleSubmit} className={cn("w-full max-w-2xl", className)}>
      <label className="flex h-12 items-center gap-3 rounded-full border border-border bg-background px-5 shadow-sm transition-shadow focus-within:shadow-md hover:shadow-md">
        <Search className="size-5 shrink-0 text-muted-foreground" aria-hidden />
        <input
          // A new query from outside (back button, chip) resets what's typed.
          key={defaultValue}
          type="search"
          name="q"
          aria-label="Search"
          defaultValue={defaultValue}
          autoFocus={!defaultValue}
          autoComplete="off"
          className="h-full flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground"
        />
      </label>
    </form>
  );
}
