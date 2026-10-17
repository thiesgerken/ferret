import { Search } from "lucide-react";
import { useTranslations } from "next-intl";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { cn } from "@/lib/utils";

/** Long enough to skip the keystrokes of a word typed in one go. */
export const TYPING_DEBOUNCE_MS = 250;

/**
 * A GET form to `/` with `q`, so it also works before JS loads. With JS, `onSearch` takes over and
 * keeps the page. With `onType`, the query also goes out once typing pauses.
 */
export function SearchField({
  defaultValue = "",
  onSearch,
  onType,
  className,
}: {
  defaultValue?: string;
  onSearch: (q: string) => void;
  onType?: (q: string) => void;
  className?: string;
}) {
  const t = useTranslations("search");
  const [value, setValue] = useState(defaultValue);
  const [shownDefault, setShownDefault] = useState(defaultValue);
  const typing = useRef<ReturnType<typeof setTimeout>>(undefined);

  // A new query from outside (back button, chip) replaces what's typed. One that only echoes
  // what's typed doesn't, so trailing spaces and the cursor survive.
  if (defaultValue !== shownDefault) {
    setShownDefault(defaultValue);
    if (defaultValue !== value.trim()) setValue(defaultValue);
  }

  useEffect(() => () => clearTimeout(typing.current), []);

  function handleChange(next: string) {
    setValue(next);
    if (!onType) return;
    clearTimeout(typing.current);
    const q = next.trim();
    if (q && q !== defaultValue) typing.current = setTimeout(() => onType(q), TYPING_DEBOUNCE_MS);
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    clearTimeout(typing.current);
    const q = value.trim();
    if (q) onSearch(q);
  }

  return (
    <form role="search" action="/" onSubmit={handleSubmit} className={cn("w-full max-w-2xl", className)}>
      <label className="flex h-12 items-center gap-3 rounded-full border border-border bg-background px-5 shadow-sm transition-shadow focus-within:shadow-md hover:shadow-md">
        <Search className="size-5 shrink-0 text-muted-foreground" aria-hidden />
        <input
          type="search"
          name="q"
          aria-label={t("field")}
          value={value}
          onChange={(event) => handleChange(event.target.value)}
          autoFocus={!defaultValue}
          autoComplete="off"
          className="h-full flex-1 bg-transparent text-base outline-none"
        />
      </label>
    </form>
  );
}
