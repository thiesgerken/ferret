"use client";

import { Brain, Server, Users2 } from "lucide-react";
import { useFormatter, useTranslations } from "next-intl";
import { useEffect, useState, type FormEvent } from "react";
import { Section } from "@/components/page-shell";
import { TeiPool } from "@/components/tei-pool";
import { useMutation, useQuery } from "urql";
import { graphql } from "@/gql";
import type { AdminQuery as AdminData } from "@/gql/graphql";
import { errorMessage } from "@/lib/graphql";
import { cn, fieldClass } from "@/lib/utils";

const AdminQuery = graphql(`
  query Admin {
    users {
      id
      username
      displayName
      isAdmin
      enabled
      createdAt
      lastLoginAt
    }
    workers {
      id
      name
      plugins
      lastSeenAt
      online
    }
    embeddingStatus {
      name
      modelId
      isDefault
      dimension
      backlog {
        source
        pending
        failed
      }
    }
    teis {
      url
      model
      servedModel
      state
      enabled
      queries
      embedding
      ...TeiCardFields
    }
  }
`);

const CreateUserMutation = graphql(`
  mutation CreateUser($input: NewUser!) {
    createUser(input: $input) {
      id
    }
  }
`);

const UpdateUserMutation = graphql(`
  mutation UpdateUser($id: ID!, $input: UserUpdate!) {
    updateUser(id: $id, input: $input) {
      id
      isAdmin
      enabled
    }
  }
`);

const SetUserPasswordMutation = graphql(`
  mutation SetUserPassword($id: ID!, $password: String!) {
    setUserPassword(id: $id, password: $password)
  }
`);

type User = AdminData["users"][number];
/** Health and backlog change on their own, so the page keeps them current. */
const POLL_MS = 10_000;

const tableClass = "w-full text-left text-sm";
const headClass = "border-b border-border text-xs text-muted-foreground";
const DATE_TIME = { dateStyle: "medium", timeStyle: "short" } as const;

export function Admin() {
  const t = useTranslations("admin");
  const [{ data, error }, refetch] = useQuery({ query: AdminQuery });
  const reload = () => refetch({ requestPolicy: "network-only" });

  useEffect(() => {
    // A tab in the background would count every chunk every 10 s for nothing.
    const timer = setInterval(() => {
      if (!document.hidden) refetch({ requestPolicy: "network-only" });
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [refetch]);

  if (error) {
    return <p className="text-sm text-destructive">{errorMessage(error)}</p>;
  }
  if (!data) {
    return null;
  }

  return (
    <div className="space-y-8">
      <Section title={t("users")} icon={<Users2 />} description={t("usersHint")}>
        <div className="space-y-5">
          <Users users={data.users} onChanged={reload} />
          <NewUserForm onCreated={reload} />
        </div>
      </Section>
      <Section
        title={t("embedding")}
        icon={<Brain />}
        description={t("embeddingHint")}
      >
        <TeiPool models={data.embeddingStatus} teis={data.teis} onChanged={reload} />
      </Section>
      <Section title={t("workers")} icon={<Server />} description={t("workersHint")}>
        <Workers workers={data.workers} />
      </Section>
    </div>
  );
}

function Users({ users, onChanged }: { users: User[]; onChanged: () => void }) {
  const t = useTranslations();
  const format = useFormatter();
  const [, update] = useMutation(UpdateUserMutation);
  const [error, setError] = useState("");
  const [passwordFor, setPasswordFor] = useState<string | null>(null);

  async function change(user: User, changes: { isAdmin?: boolean; enabled?: boolean }) {
    const result = await update({
      id: user.id,
      input: {
        displayName: user.displayName ?? null,
        isAdmin: changes.isAdmin ?? user.isAdmin,
        enabled: changes.enabled ?? user.enabled,
      },
    });
    setError(result.error ? errorMessage(result.error) : "");
    onChanged();
  }

  return (
    <div className="space-y-2">
      <div className="overflow-x-auto">
        <table className={tableClass}>
          <thead className={headClass}>
            <tr>
              <th className="py-2 pr-3 font-medium">{t("admin.user")}</th>
              <th className="py-2 pr-3 font-medium">{t("admin.lastSignIn")}</th>
              <th className="py-2 pr-3 font-medium">{t("admin.admin")}</th>
              <th className="py-2 pr-3 font-medium">{t("admin.enabled")}</th>
              <th className="py-2 font-medium">
                <span className="sr-only">{t("admin.password")}</span>
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {users.map((user) => (
              <tr key={user.id} className={cn(!user.enabled && "text-muted-foreground")}>
                <td className="py-2 pr-3">
                  <span className="font-medium">{user.username}</span>
                  {user.displayName && <span className="text-muted-foreground"> · {user.displayName}</span>}
                </td>
                <td className="py-2 pr-3">{user.lastLoginAt ? format.dateTime(new Date(user.lastLoginAt), DATE_TIME) : t("common.never")}</td>
                <td className="py-2 pr-3">
                  <input
                    type="checkbox"
                    aria-label={t("admin.isAdmin", { user: user.username })}
                    checked={user.isAdmin}
                    onChange={(event) => change(user, { isAdmin: event.target.checked })}
                  />
                </td>
                <td className="py-2 pr-3">
                  <input
                    type="checkbox"
                    aria-label={t("admin.canSignIn", { user: user.username })}
                    checked={user.enabled}
                    onChange={(event) => change(user, { enabled: event.target.checked })}
                  />
                </td>
                <td className="py-2 text-right">
                  <button
                    type="button"
                    onClick={() => setPasswordFor(passwordFor === user.id ? null : user.id)}
                    className="rounded-md px-2 py-1 hover:bg-accent"
                  >
                    {t("admin.setPassword")}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {passwordFor && (
        <SetPasswordForm
          key={passwordFor}
          user={users.find((user) => user.id === passwordFor)!}
          onDone={() => setPasswordFor(null)}
        />
      )}
    </div>
  );
}

function SetPasswordForm({ user, onDone }: { user: User; onDone: () => void }) {
  const t = useTranslations("admin");
  const [, setPassword] = useMutation(SetUserPasswordMutation);
  const [error, setError] = useState("");

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const password = String(new FormData(event.currentTarget).get("password"));
    const result = await setPassword({ id: user.id, password });
    if (result.error) {
      setError(errorMessage(result.error));
      return;
    }
    onDone();
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-3 rounded-xl border border-border p-4">
      <label htmlFor="new-password" className="block text-xs font-medium text-muted-foreground">
        {t("newPasswordFor", { user: user.username })}
      </label>
      <div className="flex gap-2">
        <input
          id="new-password"
          name="password"
          type="password"
          autoComplete="new-password"
          required
          minLength={12}
          className={fieldClass}
        />
        <button type="submit" className="shrink-0 rounded-lg bg-foreground px-4 py-2 text-sm font-medium text-background">
          {t("set")}
        </button>
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
    </form>
  );
}

function NewUserForm({ onCreated }: { onCreated: () => void }) {
  const t = useTranslations("admin");
  const [, create] = useMutation(CreateUserMutation);
  const [error, setError] = useState("");

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    const result = await create({
      input: {
        username: String(form.get("username")),
        displayName: String(form.get("displayName")) || null,
        password: String(form.get("password")),
        isAdmin: form.get("isAdmin") === "on",
      },
    });
    if (result.error) {
      setError(errorMessage(result.error));
      return;
    }
    setError("");
    formElement.reset();
    onCreated();
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4 rounded-xl border border-border p-4">
      <h3 className="text-sm font-medium">{t("newUser")}</h3>
      <div className="grid gap-3 sm:grid-cols-3">
        <label className="space-y-1.5 text-xs font-medium text-muted-foreground">
          <span>{t("username")}</span>
          <input name="username" required autoComplete="off" className={fieldClass} />
        </label>
        <label className="space-y-1.5 text-xs font-medium text-muted-foreground">
          <span>{t("displayName")}</span>
          <input name="displayName" className={fieldClass} />
        </label>
        <label className="space-y-1.5 text-xs font-medium text-muted-foreground">
          <span>{t("newPassword")}</span>
          <input name="password" type="password" autoComplete="new-password" required minLength={12} className={fieldClass} />
        </label>
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input name="isAdmin" type="checkbox" />
        {t("admin")}
      </label>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <button
        type="submit"
        className="rounded-lg bg-foreground px-4 py-2 text-sm font-medium text-background hover:bg-foreground/90"
      >
        {t("createUser")}
      </button>
    </form>
  );
}

function Workers({ workers }: { workers: AdminData["workers"] }) {
  const t = useTranslations();
  const format = useFormatter();
  if (workers.length === 0) {
    return <p className="text-sm text-muted-foreground">{t("admin.noWorkers")}</p>;
  }
  return (
    <table className={tableClass}>
      <thead className={headClass}>
        <tr>
          <th className="py-2 pr-3 font-medium">{t("admin.worker")}</th>
          <th className="py-2 pr-3 font-medium">{t("admin.plugins")}</th>
          <th className="py-2 pr-3 font-medium">{t("admin.status")}</th>
          <th className="py-2 font-medium">{t("admin.lastSeen")}</th>
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {workers.map((worker) => (
          <tr key={worker.id} className={cn(!worker.online && "text-muted-foreground")}>
            <td className="py-2 pr-3 font-mono text-xs">{worker.name}</td>
            <td className="py-2 pr-3">{worker.plugins.join(", ")}</td>
            <td className="py-2 pr-3">{worker.online ? t("common.online") : t("common.offline")}</td>
            <td className="py-2">{format.dateTime(new Date(worker.lastSeenAt), DATE_TIME)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
