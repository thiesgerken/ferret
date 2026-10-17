"use client";

import { Check, Copy } from "lucide-react";
import { useFormatter, useTranslations } from "next-intl";
import { useState, type FormEvent } from "react";
import { useMutation, useQuery } from "urql";
import { graphql } from "@/gql";
import type { ApiKeyScope, ApiKeysQuery } from "@/gql/graphql";
import { IconPicker } from "@/components/icon-picker";
import { NamedIcon } from "@/components/named-icon";
import { DEFAULT_API_KEY_ICON } from "@/lib/default-icons";
import { errorMessage } from "@/lib/graphql";
import { fieldClass } from "@/lib/utils";

const ApiKeysQueryDocument = graphql(`
  query ApiKeys {
    me {
      isAdmin
    }
    apiKeys {
      id
      name
      icon
      prefix
      scopes
      sources
      lastUsedAt
      expiresAt
    }
  }
`);

const CreateApiKeyMutation = graphql(`
  mutation CreateApiKey($input: NewApiKey!) {
    createApiKey(input: $input) {
      key
    }
  }
`);

const UpdateApiKeyMutation = graphql(`
  mutation UpdateApiKey($id: ID!, $name: String!, $icon: String!) {
    updateApiKey(id: $id, name: $name, icon: $icon) {
      id
      name
      icon
    }
  }
`);

const RevokeApiKeyMutation = graphql(`
  mutation RevokeApiKey($id: ID!) {
    revokeApiKey(id: $id)
  }
`);

type Access = "" | "READ" | "WRITE";

// Write implies read on the server, so each scope gets one choice instead of two checkboxes.
const SCOPES = [
  { scope: "SEARCH", accesses: ["READ"], adminOnly: false },
  { scope: "INGEST", accesses: ["READ", "WRITE"], adminOnly: false },
  { scope: "ADMIN", accesses: ["READ", "WRITE"], adminOnly: true },
] as const;

const SCOPE_LABELS: Record<ApiKeyScope, string> = {
  SEARCH_READ: "search:read",
  INGEST_READ: "ingest:read",
  INGEST_WRITE: "ingest:write",
  ADMIN_READ: "admin:read",
  ADMIN_WRITE: "admin:write",
};

export function ApiKeysSettings() {
  const t = useTranslations("settings.apiKeys");
  const [{ data, error }, refetch] = useQuery({ query: ApiKeysQueryDocument });
  const [, revoke] = useMutation(RevokeApiKeyMutation);
  const [revokeError, setRevokeError] = useState("");
  const reload = () => refetch({ requestPolicy: "network-only" });

  async function handleRevoke(id: string, name: string) {
    if (!window.confirm(t("confirmRevoke", { name }))) {
      return;
    }
    const result = await revoke({ id });
    setRevokeError(result.error ? errorMessage(result.error) : "");
    reload();
  }

  if (error) {
    return <p className="text-sm text-destructive">{errorMessage(error)}</p>;
  }
  if (!data) {
    return null;
  }

  return (
    <div className="space-y-8">
      <CreateApiKeyForm isAdmin={data.me.isAdmin} onCreated={reload} />
      {revokeError && (
        <p role="alert" className="text-sm text-destructive">
          {revokeError}
        </p>
      )}
      {data.apiKeys.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("none")}</p>
      ) : (
        <ul className="divide-y divide-border rounded-xl border border-border">
          {data.apiKeys.map((key) => (
            <ApiKeyRow key={key.id} apiKey={key} onRevoke={() => handleRevoke(key.id, key.name)} />
          ))}
        </ul>
      )}
    </div>
  );
}

type ApiKey = ApiKeysQuery["apiKeys"][number];

function ApiKeyRow({ apiKey: key, onRevoke }: { apiKey: ApiKey; onRevoke: () => void }) {
  const t = useTranslations();
  const format = useFormatter();
  const date = (iso: string) => format.dateTime(new Date(iso), { dateStyle: "medium" });
  const expiry = !key.expiresAt
    ? t("settings.apiKeys.neverExpires")
    : new Date(key.expiresAt) <= new Date()
      ? t("settings.apiKeys.expired", { date: date(key.expiresAt) })
      : t("settings.apiKeys.expires", { date: date(key.expiresAt) });
  const [{ fetching }, update] = useMutation(UpdateApiKeyMutation);
  const [editing, setEditing] = useState(false);
  const [icon, setIcon] = useState(key.icon);
  const [error, setError] = useState("");

  async function handleSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const name = String(new FormData(event.currentTarget).get("name")).trim();
    const result = await update({ id: key.id, name, icon });
    if (result.error) {
      setError(errorMessage(result.error));
      return;
    }
    setError("");
    setEditing(false);
  }

  return (
    <li className="flex items-start gap-4 p-4">
      <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted">
        <NamedIcon name={key.icon} className="size-4" />
      </span>
      <div className="min-w-0 flex-1 space-y-1 text-sm">
        {editing ? (
          <form onSubmit={handleSave} className="flex flex-wrap items-center gap-2">
            <IconPicker value={icon} onChange={setIcon} />
            <input
              name="name"
              aria-label={t("common.name")}
              required
              pattern=".*\S.*"
              defaultValue={key.name}
              className="h-10 min-w-0 flex-1 rounded-lg border border-border bg-background px-3 text-sm"
            />
            <button type="submit" disabled={fetching} className="rounded-md px-2 py-1 hover:bg-accent disabled:opacity-50">
              {t("common.save")}
            </button>
            <button type="button" onClick={() => setEditing(false)} className="rounded-md px-2 py-1 hover:bg-accent">
              {t("common.cancel")}
            </button>
          </form>
        ) : (
          <p className="font-medium">
            {key.name} <span className="font-mono text-xs text-muted-foreground">fk_{key.prefix}…</span>
          </p>
        )}
        {error && (
          <p role="alert" className="text-destructive">
            {error}
          </p>
        )}
        <p className="text-muted-foreground">
          {key.scopes.map((scope) => SCOPE_LABELS[scope]).join(", ")} · {key.sources?.join(", ") ?? t("common.allSources")}
        </p>
        <p className="text-xs text-muted-foreground">
          {t("settings.apiKeys.lastUsed", { date: key.lastUsedAt ? date(key.lastUsedAt) : t("common.never") })} · {expiry}
        </p>
      </div>
      <div className="flex gap-1">
        {!editing && (
          <button
            type="button"
            onClick={() => {
              setIcon(key.icon);
              setEditing(true);
            }}
            aria-label={t("settings.apiKeys.editName", { name: key.name })}
            className="rounded-md px-2 py-1 text-sm hover:bg-accent"
          >
            {t("common.edit")}
          </button>
        )}
        <button type="button" onClick={onRevoke} className="rounded-md px-2 py-1 text-sm text-destructive hover:bg-accent">
          {t("settings.apiKeys.revoke")}
        </button>
      </div>
    </li>
  );
}

function CreateApiKeyForm({ isAdmin, onCreated }: { isAdmin: boolean; onCreated: () => void }) {
  const t = useTranslations();
  const [, create] = useMutation(CreateApiKeyMutation);
  const [icon, setIcon] = useState(DEFAULT_API_KEY_ICON);
  const [error, setError] = useState("");
  const [createdKey, setCreatedKey] = useState("");
  const [copied, setCopied] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    const scopes = SCOPES.flatMap(({ scope }) => {
      const access = String(form.get(scope) ?? "") as Access;
      return access ? [`${scope}_${access}` as ApiKeyScope] : [];
    });
    const sources = String(form.get("sources")).split(/[\s,]+/).filter(Boolean);
    const expires = String(form.get("expires"));
    const result = await create({
      input: {
        name: String(form.get("name")).trim(),
        icon,
        scopes,
        sources: sources.length > 0 ? sources : null,
        // The end of the chosen day, in the browser's time zone.
        expiresAt: expires ? new Date(`${expires}T23:59:59`).toISOString() : null,
      },
    });
    if (result.error || !result.data) {
      setError(result.error ? errorMessage(result.error) : t("settings.apiKeys.notCreated"));
      return;
    }
    setError("");
    setCopied(false);
    setCreatedKey(result.data.createApiKey.key);
    formElement.reset();
    setIcon(DEFAULT_API_KEY_ICON);
    onCreated();
  }

  async function copy() {
    await navigator.clipboard.writeText(createdKey);
    setCopied(true);
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4 rounded-xl border border-border p-4">
      <h3 className="text-sm font-medium">{t("settings.apiKeys.new")}</h3>
      <div className="flex items-end gap-3">
        <div className="space-y-1.5">
          <span className="block text-xs font-medium text-muted-foreground">{t("common.icon")}</span>
          <IconPicker value={icon} onChange={setIcon} />
        </div>
        <div className="min-w-0 flex-1 space-y-1.5">
          <label htmlFor="key-name" className="text-xs font-medium text-muted-foreground">
            {t("common.name")}
          </label>
          <input
            id="key-name"
            name="name"
            required
            pattern=".*\S.*"
            placeholder={t("settings.apiKeys.namePlaceholder")}
            className={fieldClass}
          />
        </div>
      </div>
      <fieldset className="grid gap-3 sm:grid-cols-3">
        <legend className="mb-1.5 text-xs font-medium text-muted-foreground">{t("settings.apiKeys.scopes")}</legend>
        {SCOPES.filter(({ adminOnly }) => isAdmin || !adminOnly).map(({ scope, accesses }) => (
          <label key={scope} className="space-y-1.5 text-sm">
            <span>{t(`settings.apiKeys.scope.${scope}`)}</span>
            <select name={scope} defaultValue="" className={fieldClass}>
              <option value="">{t("settings.apiKeys.noAccess")}</option>
              {accesses.map((access) => (
                <option key={access} value={access}>
                  {access === "READ" ? t("settings.apiKeys.read") : t("settings.apiKeys.readWrite")}
                </option>
              ))}
            </select>
          </label>
        ))}
      </fieldset>
      <div className="space-y-1.5">
        <label htmlFor="key-sources" className="text-xs font-medium text-muted-foreground">
          {t("settings.apiKeys.sources")}
        </label>
        <input id="key-sources" name="sources" placeholder="notes, paperless" className={fieldClass} />
      </div>
      <div className="space-y-1.5">
        <label htmlFor="key-expires" className="text-xs font-medium text-muted-foreground">
          {t("settings.apiKeys.expiresLabel")}
        </label>
        <input id="key-expires" name="expires" type="date" className={fieldClass} />
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <button
        type="submit"
        className="rounded-lg bg-foreground px-4 py-2 text-sm font-medium text-background hover:bg-foreground/90"
      >
        {t("settings.apiKeys.create")}
      </button>
      {createdKey && (
        <div className="space-y-2 rounded-lg bg-muted p-3 text-sm">
          <p>{t("settings.apiKeys.copyNow")}</p>
          <div className="flex items-center gap-2">
            <code id="created-key" className="min-w-0 flex-1 break-all font-mono text-xs">
              {createdKey}
            </code>
            <button
              type="button"
              onClick={copy}
              aria-label={t("settings.apiKeys.copy")}
              className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-accent-foreground"
            >
              {copied ? <Check className="size-4" aria-hidden /> : <Copy className="size-4" aria-hidden />}
            </button>
          </div>
        </div>
      )}
    </form>
  );
}
