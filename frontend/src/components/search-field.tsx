import { Search } from "lucide-react";

// ponytail: a plain GET form, so `/?q=` is the search URL before any JS is wired up.
export function SearchField() {
  return (
    <form role="search" action="/" className="w-full max-w-2xl">
      <label className="flex h-12 items-center gap-3 rounded-full border border-border bg-background px-5 shadow-sm transition-shadow focus-within:shadow-md hover:shadow-md">
        <Search className="size-5 shrink-0 text-muted-foreground" aria-hidden />
        <input
          type="search"
          name="q"
          aria-label="Search"
          autoFocus
          autoComplete="off"
          className="h-full flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground"
        />
      </label>
    </form>
  );
}
