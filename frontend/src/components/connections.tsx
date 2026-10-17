"use client";

import { Plug } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { useMutation, useQuery, type CombinedError } from "urql";
import { graphql } from "@/gql";
import type { ConnectionsQuery as ConnectionsData, RunStatus } from "@/gql/graphql";
import { configFields, configFromForm, configInputName, type ConfigField } from "@/lib/config-schema";
import { errorMessage } from "@/lib/graphql";
import { cn, fieldClass } from "@/lib/utils";

const ConnectionsQuery = graphql(`
  query Connections {
    pluginTypes {
      name
      configSchema
    }
    connections {
      id
      plugin
      source
      config
      schedule
      enabled
      nextRunAt
      runs {
        id
        status
        startedAt
        finishedAt
        stats {
          put
          unchanged
          deleted
        }
        log
        error
      }
      exclusions {
        id
        source
        externalId
        thread
      }
    }
  }
`);

const CreateConnectionMutation = graphql(`
  mutation CreateConnection($input: NewConnection!) {
    createConnection(input: $input) {
      id
    }
  }
`);

const UpdateConnectionMutation = graphql(`
  mutation UpdateConnection($id: ID!, $input: ConnectionUpdate!) {
    updateConnection(id: $id, input: $input) {
      id
    }
  }
`);

const RunConnectionNowMutation = graphql(`
  mutation RunConnectionNow($id: ID!) {
    runConnectionNow(id: $id)
  }
`);

const RemoveExclusionMutation = graphql(`
  mutation RemoveExclusion($id: ID!) {
    removeExclusion(id: $id)
  }
`);

const DeleteConnectionMutation = graphql(`
  mutation DeleteConnection($id: ID!, $confirmSource: String!) {
    deleteConnection(id: $id, confirmSource: $confirmSource)
  }
`);

type Connection = ConnectionsData["connections"][number];
type Run = Connection["runs"][number];

const STATUS_LABELS: Record<RunStatus, string> = {
  RUNNING: "Running",
  SUCCEEDED: "Succeeded",
  FAILED: "Failed",
  LOST: "Lost (the worker stopped responding)",
};

/** While a run is due or running, the page polls so its outcome shows up without a reload. */
const POLL_MS = 2000;

const buttonClass = "rounded-md px-2 py-1 text-sm hover:bg-accent disabled:opacity-50 disabled:hover:bg-transparent";

export function Connections() {
  const [{ data, error }, refetch] = useQuery({ query: ConnectionsQuery });
  const reload = () => refetch({ requestPolicy: "network-only" });

  useEffect(() => {
    const now = new Date();
    const busy = data?.connections.some(
      (connection) =>
        connection.runs[0]?.status === "RUNNING" || (connection.enabled && new Date(connection.nextRunAt) <= now),
    );
    if (!busy) {
      return;
    }
    const timer = setTimeout(() => refetch({ requestPolicy: "network-only" }), POLL_MS);
    return () => clearTimeout(timer);
  }, [data, refetch]);

  if (error) {
    return <p className="text-sm text-destructive">{errorMessage(error)}</p>;
  }
  if (!data) {
    return null;
  }

  return (
    <div className="space-y-8">
      {data.pluginTypes.length === 0 ? (
        <p className="text-sm text-muted-foreground">No plugin worker has registered yet, so there&apos;s nothing to connect.</p>
      ) : (
        <CreateConnectionForm pluginTypes={data.pluginTypes} onCreated={reload} />
      )}
      {data.connections.length === 0 ? (
        <p className="text-sm text-muted-foreground">No connections yet.</p>
      ) : (
        <ul className="space-y-4">
          {data.connections.map((connection) => (
            <ConnectionCard key={connection.id} connection={connection} onChanged={reload} />
          ))}
        </ul>
      )}
    </div>
  );
}

function CreateConnectionForm({
  pluginTypes,
  onCreated,
}: {
  pluginTypes: ConnectionsData["pluginTypes"];
  onCreated: () => void;
}) {
  const [{ fetching }, create] = useMutation(CreateConnectionMutation);
  const [plugin, setPlugin] = useState(pluginTypes[0].name);
  const [error, setError] = useState("");
  const schema = pluginTypes.find(({ name }) => name === plugin)?.configSchema;
  let fields: ConfigField[] = [];
  let schemaError = schema === undefined ? `No worker offers the plugin "${plugin}" anymore.` : "";
  try {
    fields = schema === undefined ? [] : configFields(schema);
  } catch (err) {
    schemaError = err instanceof Error ? err.message : String(err);
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    const result = await create({
      input: {
        plugin,
        source: String(form.get("source")),
        config: JSON.stringify(configFromForm(fields, form)),
        schedule: String(form.get("schedule")),
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
      <h3 className="text-sm font-medium">New connection</h3>
      <div className="grid gap-4 sm:grid-cols-3">
        <div className="space-y-1.5">
          <label htmlFor="connection-plugin" className="text-xs font-medium text-muted-foreground">
            Plugin
          </label>
          <select
            id="connection-plugin"
            value={plugin}
            onChange={(event) => setPlugin(event.target.value)}
            className={fieldClass}
          >
            {pluginTypes.map(({ name }) => (
              <option key={name}>{name}</option>
            ))}
          </select>
        </div>
        <div className="space-y-1.5">
          <label htmlFor="connection-source" className="text-xs font-medium text-muted-foreground">
            Source
          </label>
          <input id="connection-source" name="source" required placeholder="notes" className={fieldClass} />
        </div>
        <div className="space-y-1.5">
          <label htmlFor="connection-schedule" className="text-xs font-medium text-muted-foreground">
            Schedule (cron)
          </label>
          <input
            id="connection-schedule"
            name="schedule"
            required
            defaultValue="0 * * * *"
            className={cn(fieldClass, "font-mono")}
          />
        </div>
      </div>
      <p className="text-xs text-muted-foreground">
        The connection owns its source and everything below it: no other connection or API key can write there.
      </p>
      {schemaError ? (
        <p role="alert" className="text-sm text-destructive">
          {schemaError}
        </p>
      ) : (
        // Keyed by plugin: another plugin's fields start from their own defaults.
        <fieldset key={plugin} className="space-y-4">
          <legend className="mb-1.5 text-xs font-medium text-muted-foreground">Configuration</legend>
          {fields.map((field) => (
            <ConfigInput key={field.name} field={field} />
          ))}
        </fieldset>
      )}
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <button
        type="submit"
        disabled={Boolean(schemaError) || fetching}
        className="rounded-lg bg-foreground px-4 py-2 text-sm font-medium text-background hover:bg-foreground/90 disabled:opacity-50"
      >
        Create connection
      </button>
    </form>
  );
}

function ConfigInput({ field }: { field: ConfigField }) {
  const id = `config-${field.name}`;
  const name = configInputName(field);
  const label = field.name.replaceAll("_", " ");
  if (field.kind === "checkbox") {
    return (
      <label className="flex items-start gap-2 text-sm">
        <input id={id} name={name} type="checkbox" defaultChecked={field.default === true} className="mt-1" />
        <span>
          {label}
          <span className="block text-xs text-muted-foreground">{field.description}</span>
        </span>
      </label>
    );
  }
  const placeholder = Array.isArray(field.default)
    ? field.default.join("\n")
    : typeof field.default === "string"
      ? field.default
      : undefined;
  return (
    <div className="space-y-1.5">
      <label htmlFor={id} className="text-sm">
        {label}
        {field.required ? "" : " (optional)"}
      </label>
      <p className="text-xs whitespace-pre-line text-muted-foreground">
        {field.description}
        {field.kind === "lines" ? " One per line." : ""}
      </p>
      {field.kind === "lines" ? (
        <textarea
          id={id}
          name={name}
          rows={2}
          required={field.required}
          placeholder={placeholder}
          className={cn(fieldClass, "font-mono")}
        />
      ) : (
        <input id={id} name={name} required={field.required} placeholder={placeholder} className={fieldClass} />
      )}
    </div>
  );
}

function ConnectionCard({ connection, onChanged }: { connection: Connection; onChanged: () => void }) {
  const [, update] = useMutation(UpdateConnectionMutation);
  const [, runNow] = useMutation(RunConnectionNowMutation);
  const [, removeExclusion] = useMutation(RemoveExclusionMutation);
  const [error, setError] = useState("");
  const [deleting, setDeleting] = useState(false);
  const lastRun = connection.runs[0];

  async function mutate(mutation: () => Promise<{ error?: CombinedError }>) {
    const result = await mutation();
    setError(result.error ? errorMessage(result.error) : "");
    onChanged();
  }

  const setEnabled = (enabled: boolean) =>
    mutate(() =>
      update({
        id: connection.id,
        input: { config: connection.config, schedule: connection.schedule, enabled },
      }),
    );

  return (
    <li className="space-y-3 rounded-xl border border-border p-4">
      <div className="flex flex-wrap items-start gap-x-4 gap-y-2">
        <Plug className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden />
        <div className="min-w-0 flex-1 space-y-1 text-sm">
          <p className="font-medium">
            {connection.source}{" "}
            <span className="font-normal text-muted-foreground">
              {connection.plugin} · <code className="font-mono text-xs">{connection.schedule}</code>
            </span>
          </p>
          <p className="text-muted-foreground">
            {lastRun ? <RunSummary run={lastRun} /> : "Never ran"} ·{" "}
            {connection.enabled ? `next run ${formatDateTime(connection.nextRunAt)}` : "disabled"}
          </p>
        </div>
        <div className="flex gap-1">
          <button
            type="button"
            disabled={!connection.enabled || lastRun?.status === "RUNNING"}
            onClick={() => mutate(() => runNow({ id: connection.id }))}
            aria-label={`Run ${connection.source} now`}
            className={buttonClass}
          >
            Run now
          </button>
          <button
            type="button"
            onClick={() => setEnabled(!connection.enabled)}
            aria-label={`${connection.enabled ? "Disable" : "Enable"} ${connection.source}`}
            className={buttonClass}
          >
            {connection.enabled ? "Disable" : "Enable"}
          </button>
          <button
            type="button"
            onClick={() => setDeleting(true)}
            aria-label={`Delete ${connection.source}`}
            className={cn(buttonClass, "text-destructive")}
          >
            Delete
          </button>
        </div>
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {deleting && (
        <DeleteConnectionForm connection={connection} onCancel={() => setDeleting(false)} onDeleted={onChanged} />
      )}
      <details className="text-sm">
        <summary className="cursor-pointer text-muted-foreground">Configuration and runs</summary>
        <pre className="mt-2 overflow-x-auto rounded-lg bg-muted p-3 font-mono text-xs">
          {JSON.stringify(JSON.parse(connection.config), null, 2)}
        </pre>
        <ul className="mt-2 space-y-1">
          {connection.runs.map((run) => (
            <li key={run.id}>
              <details>
                <summary className="cursor-pointer">
                  <RunSummary run={run} />
                </summary>
                {run.error && <p className="mt-1 text-destructive">{run.error}</p>}
                <pre className="mt-1 max-h-80 overflow-auto rounded-lg bg-muted p-3 font-mono text-xs whitespace-pre-wrap">
                  {run.log || "No log."}
                </pre>
              </details>
            </li>
          ))}
        </ul>
      </details>
      {connection.exclusions.length > 0 && (
        <details className="text-sm">
          <summary className="cursor-pointer text-muted-foreground">
            Excluded from the index ({connection.exclusions.length})
          </summary>
          <p className="mt-2 text-xs text-muted-foreground">
            The connection drops these whenever it finds them. Removing one makes the next run start
            over from scratch and ingest it again.
          </p>
          <ul className="mt-2 divide-y divide-border">
            {connection.exclusions.map((exclusion) => (
              <li key={exclusion.id} className="flex items-center gap-2 py-1.5">
                <span className="min-w-0 flex-1 truncate">
                  {exclusion.externalId ?? `Thread ${exclusion.thread}`}{" "}
                  <span className="text-xs text-muted-foreground">in {exclusion.source}</span>
                </span>
                <button
                  type="button"
                  onClick={() => mutate(() => removeExclusion({ id: exclusion.id }))}
                  aria-label={`Include ${exclusion.externalId ?? `thread ${exclusion.thread}`} again`}
                  className={buttonClass}
                >
                  Include again
                </button>
              </li>
            ))}
          </ul>
        </details>
      )}
    </li>
  );
}

function RunSummary({ run }: { run: Run }) {
  const failed = run.status === "FAILED" || run.status === "LOST";
  return (
    <>
      <span className={cn(failed && "text-destructive")}>{STATUS_LABELS[run.status]}</span>{" "}
      {formatDateTime(run.startedAt)}
      {run.stats &&
        ` · ${run.stats.put} put, ${run.stats.unchanged} unchanged, ${run.stats.deleted} deleted`}
    </>
  );
}

function DeleteConnectionForm({
  connection,
  onCancel,
  onDeleted,
}: {
  connection: Connection;
  onCancel: () => void;
  onDeleted: () => void;
}) {
  const [, remove] = useMutation(DeleteConnectionMutation);
  const [confirmSource, setConfirmSource] = useState("");
  const [error, setError] = useState("");

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const result = await remove({ id: connection.id, confirmSource });
    if (result.error) {
      setError(errorMessage(result.error));
      return;
    }
    onDeleted();
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-3 rounded-lg border border-destructive/50 p-3 text-sm">
      <p>
        This deletes the connection and every document below <code className="font-mono">{connection.source}</code>.
        Documents the origin no longer has exist only in ferret and are gone for good.
      </p>
      <label htmlFor={`confirm-${connection.id}`} className="block text-xs font-medium text-muted-foreground">
        Type <code className="font-mono">{connection.source}</code> to confirm
      </label>
      <input
        id={`confirm-${connection.id}`}
        value={confirmSource}
        onChange={(event) => setConfirmSource(event.target.value)}
        autoComplete="off"
        autoFocus
        className={fieldClass}
      />
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <button
          type="submit"
          disabled={confirmSource !== connection.source}
          className="rounded-lg bg-destructive px-4 py-2 font-medium text-white disabled:opacity-50"
        >
          Delete connection and documents
        </button>
        <button type="button" onClick={onCancel} className={buttonClass}>
          Cancel
        </button>
      </div>
    </form>
  );
}

function formatDateTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}
