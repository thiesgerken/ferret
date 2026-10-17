"use client";

import { useState, type FormEvent } from "react";
import { login } from "@/lib/auth";
import { cn } from "@/lib/utils";
import { BrandMark } from "@/components/brand-mark";

const inputClass = cn(
  "w-full rounded-lg border border-border bg-background px-3 py-2.5 text-base sm:text-sm",
  "outline-none transition-colors",
  "focus:ring-2 focus:ring-ring/30 focus:border-ring",
);

export function LoginForm({ onSignedIn }: { onSignedIn: () => void }) {
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setError("");
    setPending(true);
    try {
      await login(String(form.get("username")), String(form.get("password")));
      onSignedIn();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(false);
    }
  }

  return (
    <main className="flex min-h-dvh items-center justify-center p-4">
      <form onSubmit={handleSubmit} className="w-full max-w-sm space-y-4">
        <h1 className="flex items-baseline justify-center gap-2 font-wordmark text-4xl font-bold text-brand select-none">
          <BrandMark className="h-[0.8em] w-auto" />
          ferret
        </h1>
        <div className="space-y-1.5">
          <label htmlFor="username" className="text-xs font-medium text-muted-foreground">
            Username
          </label>
          <input id="username" name="username" autoComplete="username" required autoFocus className={inputClass} />
        </div>
        <div className="space-y-1.5">
          <label htmlFor="password" className="text-xs font-medium text-muted-foreground">
            Password
          </label>
          <input
            id="password"
            name="password"
            type="password"
            autoComplete="current-password"
            required
            className={inputClass}
          />
        </div>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <button
          type="submit"
          disabled={pending}
          className={cn(
            "w-full rounded-lg px-4 py-2 text-sm font-medium transition-colors",
            "bg-foreground text-background hover:bg-foreground/90",
            "disabled:cursor-not-allowed disabled:opacity-50",
          )}
        >
          {pending ? "Signing in…" : "Sign in"}
        </button>
      </form>
    </main>
  );
}
