"use client";

import { useEffect, useState, type FormEvent } from "react";
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
      isDefault
      online
      bulkOnline
      lastError
      lastErrorAt
      backlog {
        source
        pending
        failed
      }
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
type Model = AdminData["embeddingStatus"][number];

/** Health and backlog change on their own, so the page keeps them current. */
const POLL_MS = 10_000;

const sectionHeading = "text-base font-medium";
const tableClass = "w-full text-left text-sm";
const headClass = "border-b border-border text-xs text-muted-foreground";

export function Admin() {
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
    <div className="space-y-10">
      <section className="space-y-4">
        <h2 className={sectionHeading}>Users</h2>
        <Users users={data.users} onChanged={reload} />
        <NewUserForm onCreated={reload} />
      </section>
      <section className="space-y-4">
        <h2 className={sectionHeading}>Embedding</h2>
        {data.embeddingStatus.map((model) => (
          <ModelCard key={model.name} model={model} />
        ))}
      </section>
      <section className="space-y-4">
        <h2 className={sectionHeading}>Plugin workers</h2>
        <Workers workers={data.workers} />
      </section>
    </div>
  );
}

function Users({ users, onChanged }: { users: User[]; onChanged: () => void }) {
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
              <th className="py-2 pr-3 font-medium">User</th>
              <th className="py-2 pr-3 font-medium">Last sign-in</th>
              <th className="py-2 pr-3 font-medium">Admin</th>
              <th className="py-2 pr-3 font-medium">Enabled</th>
              <th className="py-2 font-medium">
                <span className="sr-only">Password</span>
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
                <td className="py-2 pr-3">{user.lastLoginAt ? formatDateTime(user.lastLoginAt) : "never"}</td>
                <td className="py-2 pr-3">
                  <input
                    type="checkbox"
                    aria-label={`${user.username} is an admin`}
                    checked={user.isAdmin}
                    onChange={(event) => change(user, { isAdmin: event.target.checked })}
                  />
                </td>
                <td className="py-2 pr-3">
                  <input
                    type="checkbox"
                    aria-label={`${user.username} can sign in`}
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
                    Set password
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
        New password for {user.username} (signs them out everywhere)
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
          Set
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
      <h3 className="text-sm font-medium">New user</h3>
      <div className="grid gap-3 sm:grid-cols-3">
        <label className="space-y-1.5 text-xs font-medium text-muted-foreground">
          <span>Username</span>
          <input name="username" required autoComplete="off" className={fieldClass} />
        </label>
        <label className="space-y-1.5 text-xs font-medium text-muted-foreground">
          <span>Display name (optional)</span>
          <input name="displayName" className={fieldClass} />
        </label>
        <label className="space-y-1.5 text-xs font-medium text-muted-foreground">
          <span>Password (12+ characters)</span>
          <input name="password" type="password" autoComplete="new-password" required minLength={12} className={fieldClass} />
        </label>
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input name="isAdmin" type="checkbox" />
        Admin
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
        Create user
      </button>
    </form>
  );
}

function ModelCard({ model }: { model: Model }) {
  return (
    <div className="space-y-3 rounded-xl border border-border p-4 text-sm">
      <p className="font-medium">
        {model.name}
        {model.isDefault && <span className="text-xs font-normal text-muted-foreground"> · default</span>}
      </p>
      <p>
        <Health label="Queries" online={model.online} />
        {model.bulkOnline !== null && model.bulkOnline !== undefined && (
          <>
            {" · "}
            <Health label="Bulk" online={model.bulkOnline} />
          </>
        )}
      </p>
      {model.lastError && (
        <p className="text-destructive">
          Last error{model.lastErrorAt && ` (${formatDateTime(model.lastErrorAt)})`}: {model.lastError}
        </p>
      )}
      {model.backlog.length === 0 ? (
        <p className="text-muted-foreground">Everything is embedded.</p>
      ) : (
        <table className={tableClass}>
          <thead className={headClass}>
            <tr>
              <th className="py-1.5 pr-3 font-medium">Source</th>
              <th className="py-1.5 pr-3 text-right font-medium">Pending</th>
              <th className="py-1.5 text-right font-medium">Refused by TEI</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {model.backlog.map((row) => (
              <tr key={row.source}>
                <td className="py-1.5 pr-3">{row.source}</td>
                <td className="py-1.5 pr-3 text-right tabular-nums">{row.pending}</td>
                <td className="py-1.5 text-right tabular-nums">{row.failed}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function Health({ label, online }: { label: string; online: boolean }) {
  return (
    <span className={online ? "text-foreground" : "text-destructive"}>
      {label}: {online ? "online" : "offline"}
    </span>
  );
}

function Workers({ workers }: { workers: AdminData["workers"] }) {
  if (workers.length === 0) {
    return <p className="text-sm text-muted-foreground">No worker asked for a run in the last day.</p>;
  }
  return (
    <table className={tableClass}>
      <thead className={headClass}>
        <tr>
          <th className="py-2 pr-3 font-medium">Worker</th>
          <th className="py-2 pr-3 font-medium">Plugins</th>
          <th className="py-2 pr-3 font-medium">Status</th>
          <th className="py-2 font-medium">Last seen</th>
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {workers.map((worker) => (
          <tr key={worker.id} className={cn(!worker.online && "text-muted-foreground")}>
            <td className="py-2 pr-3 font-mono text-xs">{worker.name}</td>
            <td className="py-2 pr-3">{worker.plugins.join(", ")}</td>
            <td className="py-2 pr-3">{worker.online ? "online" : "offline"}</td>
            <td className="py-2">{formatDateTime(worker.lastSeenAt)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function formatDateTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}
