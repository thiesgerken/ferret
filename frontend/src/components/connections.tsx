"use client";

import { Cable, Plus } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { useMutation, useQuery, type CombinedError } from "urql";
import { graphql } from "@/gql";
import type { ConnectionsQuery as ConnectionsData, RunStatus } from "@/gql/graphql";
import { configFields, configFromForm, configInputName, type ConfigField } from "@/lib/config-schema";
import { IconPicker } from "@/components/icon-picker";
import { NamedIcon } from "@/components/named-icon";
import { defaultPluginIcon } from "@/lib/default-icons";
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
      name
      icon
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
const primaryButtonClass =
  "inline-flex items-center gap-2 rounded-lg bg-foreground px-4 py-2 text-sm font-medium text-background hover:bg-foreground/90 disabled:opacity-50";

export function Connections() {
  const [{ data, error }, refetch] = useQuery({ query: ConnectionsQuery });
  const [creating, setCreating] = useState(false);
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
    <div className="space-y-6">
      {data.pluginTypes.length === 0 ? (
        <p className="rounded-xl bg-muted px-4 py-3 text-sm text-muted-foreground">
          No plugin worker has registered yet, so there&apos;s nothing to connect.
        </p>
      ) : creating ? (
        <CreateConnectionForm
          pluginTypes={data.pluginTypes}
          onCreated={() => {
            setCreating(false);
            reload();
          }}
          onCancel={() => setCreating(false)}
        />
      ) : (
        data.connections.length > 0 && (
          <button type="button" onClick={() => setCreating(true)} className={primaryButtonClass}>
            <Plus className="size-4" aria-hidden />
            New connection
          </button>
        )
      )}
      {data.connections.length === 0 ? (
        !creating && (
          <div className="flex flex-col items-center gap-3 rounded-2xl border border-dashed border-border px-6 py-14 text-center">
            <span className="flex size-12 items-center justify-center rounded-full bg-muted text-muted-foreground">
              <Cable className="size-6" aria-hidden />
            </span>
            <p className="font-medium">No connections yet</p>
            <p className="max-w-sm text-sm text-muted-foreground">
              A connection keeps a source in sync with where its documents live, like a git repository or Paperless.
            </p>
            {data.pluginTypes.length > 0 && (
              <button type="button" onClick={() => setCreating(true)} className={primaryButtonClass}>
                <Plus className="size-4" aria-hidden />
                New connection
              </button>
            )}
          </div>
        )
      ) : (
        <ul className="space-y-4">
          {data.connections.map((connection) => (
            <ConnectionCard
              key={connection.id}
              connection={connection}
              configSchema={configSchemaOf(data.pluginTypes, connection.plugin)}
              onChanged={reload}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function CreateConnectionForm({
  pluginTypes,
  onCreated,
  onCancel,
}: {
  pluginTypes: ConnectionsData["pluginTypes"];
  onCreated: () => void;
  onCancel: () => void;
}) {
  const [{ fetching }, create] = useMutation(CreateConnectionMutation);
  const [plugin, setPlugin] = useState(pluginTypes[0].name);
  // Follows the plugin's default until the user picks one.
  const [pickedIcon, setPickedIcon] = useState<string | null>(null);
  const icon = pickedIcon ?? defaultPluginIcon(plugin);
  const [error, setError] = useState("");
  const { fields, schemaError } = parseConfigSchema(
    plugin,
    pluginTypes.find(({ name }) => name === plugin)?.configSchema,
  );

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    const result = await create({
      input: {
        plugin,
        name: String(form.get("name")).trim(),
        icon,
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
    setPickedIcon(null);
    onCreated();
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4 rounded-2xl border border-border p-5">
      <h2 className="text-base font-medium">New connection</h2>
      <div className="flex items-end gap-3">
        <div className="space-y-1.5">
          <span className="block text-xs font-medium text-muted-foreground">Icon</span>
          <IconPicker value={icon} onChange={setPickedIcon} />
        </div>
        <div className="min-w-0 flex-1 space-y-1.5">
          <label htmlFor="connection-name" className="text-xs font-medium text-muted-foreground">
            Name
          </label>
          <input id="connection-name" name="name" required pattern=".*\S.*" placeholder="My notes" className={fieldClass} />
        </div>
      </div>
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
      {/* Keyed by plugin: another plugin's fields start from their own defaults. */}
      <ConfigFieldset key={plugin} idPrefix="config" fields={fields} schemaError={schemaError} />
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <button type="submit" disabled={Boolean(schemaError) || fetching} className={primaryButtonClass}>
          Create connection
        </button>
        <button type="button" onClick={onCancel} className="rounded-lg px-4 py-2 text-sm hover:bg-accent">
          Cancel
        </button>
      </div>
    </form>
  );
}

/** A connection's plugin type always exists: `connections.plugin` references `plugin_types`,
 * whose rows are never deleted. */
function configSchemaOf(pluginTypes: ConnectionsData["pluginTypes"], plugin: string): string {
  const pluginType = pluginTypes.find(({ name }) => name === plugin);
  if (!pluginType) {
    throw new Error(`The server listed no plugin type "${plugin}".`);
  }
  return pluginType.configSchema;
}

function parseConfigSchema(
  plugin: string,
  schema: string | undefined,
): { fields: ConfigField[]; schemaError: string } {
  if (schema === undefined) {
    return { fields: [], schemaError: `No worker offers the plugin "${plugin}" anymore.` };
  }
  try {
    return { fields: configFields(schema), schemaError: "" };
  } catch (err) {
    return { fields: [], schemaError: err instanceof Error ? err.message : String(err) };
  }
}

/**
 * The config's inputs, filled from `stored` (a connection's config, which never holds its secrets)
 * or else left to the plugin's defaults.
 */
function ConfigFieldset({
  idPrefix,
  fields,
  schemaError,
  stored,
}: {
  idPrefix: string;
  fields: ConfigField[];
  schemaError: string;
  stored?: Record<string, unknown>;
}) {
  if (schemaError) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {schemaError}
      </p>
    );
  }
  return (
    <fieldset className="space-y-4">
      <legend className="mb-1.5 text-xs font-medium text-muted-foreground">Configuration</legend>
      {fields.map((field) => (
        <ConfigInput
          key={field.name}
          id={`${idPrefix}-${field.name}`}
          field={field}
          value={stored?.[field.name]}
          keepsStoredSecret={stored !== undefined && field.secret}
        />
      ))}
    </fieldset>
  );
}

function ConfigInput({
  id,
  field,
  value,
  keepsStoredSecret,
}: {
  id: string;
  field: ConfigField;
  value: unknown;
  /** An empty secret field keeps the stored secret, so it's never required. */
  keepsStoredSecret: boolean;
}) {
  const name = configInputName(field);
  const label = field.name.replaceAll("_", " ");
  if (field.kind === "checkbox") {
    return (
      <label className="flex items-start gap-2 text-sm">
        <input
          id={id}
          name={name}
          type="checkbox"
          defaultChecked={(value ?? field.default) === true}
          className="mt-1"
        />
        <span>
          {label}
          <span className="block text-xs text-muted-foreground">{field.description}</span>
        </span>
      </label>
    );
  }
  const placeholder = keepsStoredSecret
    ? "Leave empty to keep the stored one, unless the configuration changes"
    : Array.isArray(field.default)
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
          defaultValue={Array.isArray(value) ? value.join("\n") : undefined}
          placeholder={placeholder}
          className={cn(fieldClass, "font-mono")}
        />
      ) : (
        <input
          id={id}
          name={name}
          type={field.secret ? "password" : "text"}
          // Not the browser's saved ferret password.
          autoComplete={field.secret ? "new-password" : undefined}
          required={field.required && !keepsStoredSecret}
          defaultValue={typeof value === "string" ? value : undefined}
          placeholder={placeholder}
          className={fieldClass}
        />
      )}
    </div>
  );
}

function ConnectionCard({
  connection,
  configSchema,
  onChanged,
}: {
  connection: Connection;
  configSchema: string;
  onChanged: () => void;
}) {
  const [, update] = useMutation(UpdateConnectionMutation);
  const [, runNow] = useMutation(RunConnectionNowMutation);
  const [, removeExclusion] = useMutation(RemoveExclusionMutation);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState(false);
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
        input: {
          name: connection.name,
          icon: connection.icon,
          config: connection.config,
          schedule: connection.schedule,
          enabled,
        },
      }),
    );

  return (
    <li className={cn("space-y-3 rounded-2xl border border-border p-5", !connection.enabled && "bg-muted/40")}>
      <div className="flex flex-wrap items-start gap-x-4 gap-y-3">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-brand/10 text-brand">
          <NamedIcon name={connection.icon} className="size-5" />
        </span>
        <div className="min-w-0 flex-1 space-y-1 text-sm">
          <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="text-base font-medium">{connection.name}</span>
            <StatusPill run={lastRun} enabled={connection.enabled} />
          </p>
          <p className="text-muted-foreground">
            {connection.plugin} · {connection.source} · <code className="font-mono text-xs">{connection.schedule}</code>
          </p>
          <p className="text-xs text-muted-foreground">
            {lastRun ? <RunSummary run={lastRun} /> : "Never ran"}
            {connection.enabled && ` · next run ${formatDateTime(connection.nextRunAt)}`}
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
            onClick={() => setEditing(true)}
            aria-label={`Edit ${connection.source}`}
            className={buttonClass}
          >
            Edit
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
      {editing && (
        <EditConnectionForm
          connection={connection}
          configSchema={configSchema}
          onCancel={() => setEditing(false)}
          onSaved={() => {
            setEditing(false);
            onChanged();
          }}
        />
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

const PILLS: Record<RunStatus, { label: string; className: string }> = {
  RUNNING: { label: "Running", className: "bg-brand/15 text-brand" },
  SUCCEEDED: { label: "OK", className: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400" },
  FAILED: { label: "Failed", className: "bg-destructive/15 text-destructive" },
  LOST: { label: "Lost", className: "bg-warning/20 text-warning-foreground dark:text-warning" },
};

/** The connection's state at a glance: disabled, or how its last run went. */
function StatusPill({ run, enabled }: { run: Run | undefined; enabled: boolean }) {
  const { label, className } = !enabled
    ? { label: "Disabled", className: "bg-muted text-muted-foreground" }
    : run
      ? PILLS[run.status]
      : { label: "New", className: "bg-muted text-muted-foreground" };
  return <span className={cn("rounded-full px-2 py-0.5 text-xs font-medium", className)}>{label}</span>;
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

function EditConnectionForm({
  connection,
  configSchema,
  onCancel,
  onSaved,
}: {
  connection: Connection;
  configSchema: string;
  onCancel: () => void;
  onSaved: () => void;
}) {
  const [, update] = useMutation(UpdateConnectionMutation);
  const [error, setError] = useState("");
  const { fields, schemaError } = parseConfigSchema(connection.plugin, configSchema);
  const [icon, setIcon] = useState(connection.icon);
  const scheduleId = `schedule-${connection.id}`;
  const nameId = `name-${connection.id}`;

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const result = await update({
      id: connection.id,
      input: {
        name: String(form.get("name")).trim(),
        icon,
        config: JSON.stringify(configFromForm(fields, form)),
        schedule: String(form.get("schedule")),
        enabled: connection.enabled,
      },
    });
    if (result.error) {
      setError(errorMessage(result.error));
      return;
    }
    onSaved();
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4 rounded-lg border border-border p-3 text-sm">
      <div className="flex items-end gap-3">
        <div className="space-y-1.5">
          <span className="block text-xs font-medium text-muted-foreground">Icon</span>
          <IconPicker value={icon} onChange={setIcon} />
        </div>
        <div className="min-w-0 flex-1 space-y-1.5">
          <label htmlFor={nameId} className="text-xs font-medium text-muted-foreground">
            Name
          </label>
          <input id={nameId} name="name" required pattern=".*\S.*" defaultValue={connection.name} className={fieldClass} />
        </div>
      </div>
      <div className="space-y-1.5">
        <label htmlFor={scheduleId} className="text-xs font-medium text-muted-foreground">
          Schedule (cron)
        </label>
        <input
          id={scheduleId}
          name="schedule"
          required
          defaultValue={connection.schedule}
          className={cn(fieldClass, "font-mono")}
        />
      </div>
      {fields.some((field) => field.secret) && (
        <p className="text-xs text-muted-foreground">
          Changing the configuration (not the schedule) needs every stored secret entered again, so
          an edit can&apos;t send them somewhere new.
        </p>
      )}
      <ConfigFieldset
        idPrefix={`edit-${connection.id}`}
        fields={fields}
        schemaError={schemaError}
        stored={JSON.parse(connection.config) as Record<string, unknown>}
      />
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <button
          type="submit"
          disabled={Boolean(schemaError)}
          className="rounded-lg bg-foreground px-4 py-2 font-medium text-background hover:bg-foreground/90 disabled:opacity-50"
        >
          Save
        </button>
        <button type="button" onClick={onCancel} className={buttonClass}>
          Cancel
        </button>
      </div>
    </form>
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
