"use client";

import { useTranslations } from "next-intl";
import { useState, type FormEvent } from "react";
import { changePassword } from "@/lib/auth";
import { fieldClass } from "@/lib/utils";

export function AccountSettings() {
  const t = useTranslations("settings.account");
  const [error, setError] = useState("");
  const [changed, setChanged] = useState(false);
  const [pending, setPending] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    setChanged(false);
    // Typos in a field nobody sees would lock the user out, the server can't catch them.
    if (form.get("new") !== form.get("repeat")) {
      setError(t("mismatch"));
      return;
    }
    setPending(true);
    try {
      await changePassword(String(form.get("current")), String(form.get("new")));
      setError("");
      setChanged(true);
      formElement.reset();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(false);
    }
  }

  return (
    <form onSubmit={handleSubmit} className="max-w-sm space-y-3">
      <PasswordField id="current" label={t("current")} autoComplete="current-password" />
      <PasswordField id="new" label={t("new")} autoComplete="new-password" />
      <PasswordField id="repeat" label={t("repeat")} autoComplete="new-password" />
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {changed && (
        <p role="status" className="text-sm text-muted-foreground">
          {t("changed")}
        </p>
      )}
      <button
        type="submit"
        disabled={pending}
        className="rounded-lg disabled:opacity-60 bg-foreground px-4 py-2 text-sm font-medium text-background hover:bg-foreground/90"
      >
        {t("submit")}
      </button>
    </form>
  );
}

function PasswordField({ id, label, autoComplete }: { id: string; label: string; autoComplete: string }) {
  return (
    <div className="space-y-1.5">
      <label htmlFor={`password-${id}`} className="text-xs font-medium text-muted-foreground">
        {label}
      </label>
      <input
        id={`password-${id}`}
        name={id}
        type="password"
        required
        autoComplete={autoComplete}
        className={fieldClass}
      />
    </div>
  );
}
