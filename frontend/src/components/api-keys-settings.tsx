"use client";

import { Check, Copy, KeyRound } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useMutation, useQuery } from "urql";
import { graphql } from "@/gql";
import type { ApiKeyScope } from "@/gql/graphql";
import { errorMessage } from "@/lib/graphql";
import { fieldClass } from "@/lib/utils";

const ApiKeysQuery = graphql(`
  query ApiKeys {
    me {
      isAdmin
    }
    apiKeys {
      id
      name
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

const RevokeApiKeyMutation = graphql(`
  mutation RevokeApiKey($id: ID!) {
    revokeApiKey(id: $id)
  }
`);

type Access = "" | "READ" | "WRITE";

// Write implies read on the server, so each scope gets one choice instead of two checkboxes.
const SCOPES = [
  { scope: "SEARCH", label: "Search", accesses: ["READ"], adminOnly: false },
  { scope: "INGEST", label: "Ingest", accesses: ["READ", "WRITE"], adminOnly: false },
  { scope: "ADMIN", label: "Admin", accesses: ["READ", "WRITE"], adminOnly: true },
] as const;

const SCOPE_LABELS: Record<ApiKeyScope, string> = {
  SEARCH_READ: "search:read",
  INGEST_READ: "ingest:read",
  INGEST_WRITE: "ingest:write",
  ADMIN_READ: "admin:read",
  ADMIN_WRITE: "admin:write",
};

export function ApiKeysSettings() {
  const [{ data, error }, refetch] = useQuery({ query: ApiKeysQuery });
  const [, revoke] = useMutation(RevokeApiKeyMutation);
  const [revokeError, setRevokeError] = useState("");
  const reload = () => refetch({ requestPolicy: "network-only" });

  async function handleRevoke(id: string, name: string) {
    if (!window.confirm(`Revoke "${name}"? Anything using this key stops working.`)) {
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
        <p className="text-sm text-muted-foreground">No API keys yet.</p>
      ) : (
        <ul className="divide-y divide-border rounded-xl border border-border">
          {data.apiKeys.map((key) => (
            <li key={key.id} className="flex items-start gap-4 p-4">
              <KeyRound className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden />
              <div className="min-w-0 flex-1 space-y-1 text-sm">
                <p className="font-medium">
                  {key.name} <span className="font-mono text-xs text-muted-foreground">fk_{key.prefix}…</span>
                </p>
                <p className="text-muted-foreground">
                  {key.scopes.map((scope) => SCOPE_LABELS[scope]).join(", ")} · {key.sources?.join(", ") ?? "all sources"}
                </p>
                <p className="text-xs text-muted-foreground">
                  Last used {key.lastUsedAt ? formatDate(key.lastUsedAt) : "never"} ·{" "}
                  {expiryText(key.expiresAt)}
                </p>
              </div>
              <button
                type="button"
                onClick={() => handleRevoke(key.id, key.name)}
                className="rounded-md px-2 py-1 text-sm text-destructive hover:bg-accent"
              >
                Revoke
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function CreateApiKeyForm({ isAdmin, onCreated }: { isAdmin: boolean; onCreated: () => void }) {
  const [, create] = useMutation(CreateApiKeyMutation);
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
        name: String(form.get("name")),
        scopes,
        sources: sources.length > 0 ? sources : null,
        // The end of the chosen day, in the browser's time zone.
        expiresAt: expires ? new Date(`${expires}T23:59:59`).toISOString() : null,
      },
    });
    if (result.error || !result.data) {
      setError(result.error ? errorMessage(result.error) : "The key wasn't created.");
      return;
    }
    setError("");
    setCopied(false);
    setCreatedKey(result.data.createApiKey.key);
    formElement.reset();
    onCreated();
  }

  async function copy() {
    await navigator.clipboard.writeText(createdKey);
    setCopied(true);
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4 rounded-xl border border-border p-4">
      <h3 className="text-sm font-medium">New API key</h3>
      <div className="space-y-1.5">
        <label htmlFor="key-name" className="text-xs font-medium text-muted-foreground">
          Name
        </label>
        <input id="key-name" name="name" required placeholder="carapace search skill" className={fieldClass} />
      </div>
      <fieldset className="grid gap-3 sm:grid-cols-3">
        <legend className="mb-1.5 text-xs font-medium text-muted-foreground">Scopes</legend>
        {SCOPES.filter(({ adminOnly }) => isAdmin || !adminOnly).map(({ scope, label, accesses }) => (
          <label key={scope} className="space-y-1.5 text-sm">
            <span>{label}</span>
            <select name={scope} defaultValue="" className={fieldClass}>
              <option value="">No access</option>
              {accesses.map((access) => (
                <option key={access} value={access}>
                  {access === "READ" ? "Read" : "Read and write"}
                </option>
              ))}
            </select>
          </label>
        ))}
      </fieldset>
      <div className="space-y-1.5">
        <label htmlFor="key-sources" className="text-xs font-medium text-muted-foreground">
          Sources (optional, restricts every scope to these and below; not with admin scopes)
        </label>
        <input id="key-sources" name="sources" placeholder="notes, paperless" className={fieldClass} />
      </div>
      <div className="space-y-1.5">
        <label htmlFor="key-expires" className="text-xs font-medium text-muted-foreground">
          Expires (optional)
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
        Create key
      </button>
      {createdKey && (
        <div className="space-y-2 rounded-lg bg-muted p-3 text-sm">
          <p>Copy the key now. It won&apos;t be shown again.</p>
          <div className="flex items-center gap-2">
            <code id="created-key" className="min-w-0 flex-1 break-all font-mono text-xs">
              {createdKey}
            </code>
            <button
              type="button"
              onClick={copy}
              aria-label="Copy key"
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

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, { dateStyle: "medium" });
}

function expiryText(expiresAt: string | null | undefined): string {
  if (!expiresAt) {
    return "never expires";
  }
  return new Date(expiresAt) <= new Date() ? `expired ${formatDate(expiresAt)}` : `expires ${formatDate(expiresAt)}`;
}
