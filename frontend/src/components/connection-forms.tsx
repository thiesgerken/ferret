"use client";

import { useState, type FormEvent, type ReactNode } from "react";
import { useMutation } from "urql";
import { graphql } from "@/gql";
import type { ConnectionsQuery } from "@/gql/graphql";
import { IconPicker } from "@/components/icon-picker";
import { configFields, configFromForm, configInputName, type ConfigField } from "@/lib/config-schema";
import { defaultPluginIcon } from "@/lib/default-icons";
import { errorMessage } from "@/lib/graphql";
import { cn, fieldClass } from "@/lib/utils";

const CreateConnectionMutation = graphql(`
  mutation CreateConnection($input: NewConnection!) {
    createConnection(input: $input) {
      id
    }
  }
`);

export const UpdateConnectionMutation = graphql(`
  mutation UpdateConnection($id: ID!, $input: ConnectionUpdate!) {
    updateConnection(id: $id, input: $input) {
      id
    }
  }
`);

const DeleteConnectionMutation = graphql(`
  mutation DeleteConnection($id: ID!, $confirmSource: String!) {
    deleteConnection(id: $id, confirmSource: $confirmSource)
  }
`);

type Connection = ConnectionsQuery["connections"][number];
type PluginType = ConnectionsQuery["pluginTypes"][number];

export const buttonClass =
  "inline-flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-sm hover:bg-accent disabled:opacity-50 disabled:hover:bg-transparent [&>svg]:size-4";
export const primaryButtonClass =
  "inline-flex items-center gap-2 rounded-lg bg-foreground px-4 py-2 text-sm font-medium text-background hover:bg-foreground/90 disabled:opacity-50";
const secondaryButtonClass = "rounded-lg px-4 py-2 text-sm hover:bg-accent";

const SCHEDULE_PRESETS = [
  { label: "Every 15 minutes", cron: "*/15 * * * *" },
  { label: "Hourly", cron: "0 * * * *" },
  { label: "Nightly", cron: "0 3 * * *" },
];

export function CreateConnectionForm({
  pluginTypes,
  onCreated,
  onCancel,
}: {
  pluginTypes: PluginType[];
  onCreated: () => void;
  onCancel: () => void;
}) {
  const [{ fetching }, create] = useMutation(CreateConnectionMutation);
  const [plugin, setPlugin] = useState(pluginTypes[0].name);
  // Follows the plugin's default until the user picks one.
  const [pickedIcon, setPickedIcon] = useState<string | null>(null);
  const icon = pickedIcon ?? defaultPluginIcon(plugin);
  const [error, setError] = useState("");
  const { fields, schemaError } = parseConfigSchema(plugin, pluginTypes.find(({ name }) => name === plugin)?.configSchema);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
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
    onCreated();
  }

  return (
    <form onSubmit={handleSubmit} className="rounded-2xl border border-border p-5 sm:p-6">
      <div className="mb-5">
        <h2 className="text-base font-semibold">New connection</h2>
        <p className="text-sm text-muted-foreground">A plugin that keeps a source in ferret in sync with where its documents live.</p>
      </div>
      <FormSection title="Plugin and source" description="What fetches the documents, and where in ferret they land.">
        <Field id="connection-plugin" label="Plugin">
          <select id="connection-plugin" value={plugin} onChange={(event) => setPlugin(event.target.value)} className={fieldClass}>
            {pluginTypes.map(({ name }) => (
              <option key={name}>{name}</option>
            ))}
          </select>
        </Field>
        <Field
          id="connection-source"
          label="Source"
          help={withCode("Where this connection's documents live in ferret, like `notes` or `mail/work`. Searches narrow by it, and API keys can be limited to it. The plugin may add sources below it, like `carapace/facts`. Only this connection writes here, so when something disappears at the origin, ferret removes it without touching anything else.")}
        >
          <input id="connection-source" name="source" required placeholder="notes" className={cn(fieldClass, "font-mono")} />
        </Field>
      </FormSection>
      <IdentitySection nameId="connection-name" icon={icon} onIconChange={setPickedIcon} />
      <ScheduleSection id="connection-schedule" defaultValue="0 * * * *" />
      {/* Keyed by plugin: another plugin's fields start from their own defaults. */}
      <ConfigSection key={plugin} plugin={plugin} idPrefix="config" fields={fields} schemaError={schemaError} />
      <FormFooter error={error}>
        <button type="submit" disabled={Boolean(schemaError) || fetching} className={primaryButtonClass}>
          Create connection
        </button>
        <button type="button" onClick={onCancel} className={secondaryButtonClass}>
          Cancel
        </button>
      </FormFooter>
    </form>
  );
}

export function EditConnectionForm({
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
  const [{ fetching }, update] = useMutation(UpdateConnectionMutation);
  const [error, setError] = useState("");
  const { fields, schemaError } = parseConfigSchema(connection.plugin, configSchema);
  const [icon, setIcon] = useState(connection.icon);

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
    <form onSubmit={handleSubmit}>
      <IdentitySection nameId={`name-${connection.id}`} icon={icon} onIconChange={setIcon} defaultName={connection.name} />
      <ScheduleSection id={`schedule-${connection.id}`} defaultValue={connection.schedule} />
      <ConfigSection
        plugin={connection.plugin}
        idPrefix={`edit-${connection.id}`}
        fields={fields}
        schemaError={schemaError}
        stored={JSON.parse(connection.config) as Record<string, unknown>}
      />
      <FormFooter error={error}>
        <button type="submit" disabled={Boolean(schemaError) || fetching} className={primaryButtonClass}>
          Save
        </button>
        <button type="button" onClick={onCancel} className={secondaryButtonClass}>
          Cancel
        </button>
      </FormFooter>
    </form>
  );
}

export function DeleteConnectionForm({
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
    <form onSubmit={handleSubmit} className="space-y-3 rounded-xl border border-destructive/50 bg-destructive/5 p-4 text-sm">
      <p>
        This deletes the connection and every document below <code className="font-mono">{connection.source}</code>.
        Documents the origin no longer has exist only in ferret and are gone for good.
      </p>
      <Field id={`confirm-${connection.id}`} label={<>Type <code className="font-mono">{connection.source}</code> to confirm</>}>
        <input
          id={`confirm-${connection.id}`}
          value={confirmSource}
          onChange={(event) => setConfirmSource(event.target.value)}
          autoComplete="off"
          autoFocus
          className={fieldClass}
        />
      </Field>
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
        <button type="button" onClick={onCancel} className={secondaryButtonClass}>
          Cancel
        </button>
      </div>
    </form>
  );
}

/** A connection's plugin type always exists: `connections.plugin` references `plugin_types`,
 * whose rows are never deleted. */
export function configSchemaOf(pluginTypes: PluginType[], plugin: string): string {
  const pluginType = pluginTypes.find(({ name }) => name === plugin);
  if (!pluginType) {
    throw new Error(`The server listed no plugin type "${plugin}".`);
  }
  return pluginType.configSchema;
}

function parseConfigSchema(plugin: string, schema: string | undefined): { fields: ConfigField[]; schemaError: string } {
  if (schema === undefined) {
    return { fields: [], schemaError: `No worker offers the plugin "${plugin}" anymore.` };
  }
  try {
    return { fields: configFields(schema), schemaError: "" };
  } catch (err) {
    return { fields: [], schemaError: err instanceof Error ? err.message : String(err) };
  }
}

/** One part of a connection form: what it's about on the left, its fields on the right. */
function FormSection({ title, description, children }: { title: string; description: ReactNode; children: ReactNode }) {
  return (
    <section className="grid gap-x-8 gap-y-3 border-t border-border py-5 sm:grid-cols-[11rem_minmax(0,1fr)]">
      <div>
        <h3 className="text-sm font-medium">{title}</h3>
        <p className="mt-1 text-xs text-muted-foreground">{description}</p>
      </div>
      <div className="space-y-4">{children}</div>
    </section>
  );
}

function Field({ id, label, help, children }: { id: string; label: ReactNode; help?: ReactNode; children: ReactNode }) {
  return (
    <div className="space-y-1.5">
      <label htmlFor={id} className="block text-sm font-medium">
        {label}
      </label>
      {children}
      {help && <p className="text-xs whitespace-pre-line text-muted-foreground">{help}</p>}
    </div>
  );
}

function FormFooter({ error, children }: { error: string; children: ReactNode }) {
  return (
    <div className="space-y-3 border-t border-border pt-5">
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <div className="flex gap-2">{children}</div>
    </div>
  );
}

function IdentitySection({
  nameId,
  icon,
  onIconChange,
  defaultName,
}: {
  nameId: string;
  icon: string;
  onIconChange: (icon: string) => void;
  defaultName?: string;
}) {
  return (
    <FormSection title="Name and icon" description="How the connection shows up in search filters and on Activity.">
      <div className="flex items-end gap-3">
        <div className="space-y-1.5">
          <span className="block text-sm font-medium">Icon</span>
          <IconPicker value={icon} onChange={onIconChange} />
        </div>
        <div className="min-w-0 flex-1">
          <Field id={nameId} label="Name">
            <input id={nameId} name="name" required pattern=".*\S.*" placeholder="My notes" defaultValue={defaultName} className={fieldClass} />
          </Field>
        </div>
      </div>
    </FormSection>
  );
}

function ScheduleSection({ id, defaultValue }: { id: string; defaultValue: string }) {
  const [schedule, setSchedule] = useState(defaultValue);
  return (
    <FormSection title="Schedule" description="When it syncs, as a cron expression in UTC.">
      <Field id={id} label="Cron" help={withCode("Minute, hour, day of month, month and weekday: `0 * * * *` runs at the top of every hour.")}>
        <input id={id} name="schedule" required value={schedule} onChange={(event) => setSchedule(event.target.value)} className={cn(fieldClass, "font-mono")} />
      </Field>
      <div className="flex flex-wrap gap-1.5">
        {SCHEDULE_PRESETS.map((preset) => (
          <button
            key={preset.cron}
            type="button"
            aria-pressed={schedule === preset.cron}
            onClick={() => setSchedule(preset.cron)}
            className="rounded-full border border-border px-2.5 py-0.5 text-xs hover:bg-accent aria-pressed:border-brand aria-pressed:bg-brand/10 aria-pressed:text-brand"
          >
            {preset.label}
          </button>
        ))}
      </div>
    </FormSection>
  );
}

/**
 * The plugin's config, filled from `stored` (a connection's config, which never holds its secrets)
 * or else left to the plugin's defaults.
 */
function ConfigSection({
  plugin,
  idPrefix,
  fields,
  schemaError,
  stored,
}: {
  plugin: string;
  idPrefix: string;
  fields: ConfigField[];
  schemaError: string;
  stored?: Record<string, unknown>;
}) {
  const editsSecrets = stored !== undefined && fields.some((field) => field.secret);
  return (
    <FormSection
      title="Configuration"
      description={
        <>
          What the <span className="font-medium text-foreground">{plugin}</span> plugin needs to reach the origin.
          {editsSecrets &&
            " Changing it (not the schedule) needs every stored secret entered again, so an edit can't send them somewhere new."}
        </>
      }
    >
      {schemaError ? (
        <p role="alert" className="text-sm text-destructive">
          {schemaError}
        </p>
      ) : fields.length === 0 ? (
        <p className="text-sm text-muted-foreground">This plugin needs no configuration.</p>
      ) : (
        <fieldset className="space-y-4">
          <legend className="sr-only">Configuration</legend>
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
      )}
    </FormSection>
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
  const words = field.name.replaceAll("_", " ");
  const label = words[0].toUpperCase() + words.slice(1);
  if (field.kind === "checkbox") {
    return (
      <label className="flex items-start gap-2.5 text-sm">
        <input id={id} name={name} type="checkbox" defaultChecked={(value ?? field.default) === true} className="mt-0.5 size-4" />
        <span>
          <span className="font-medium">{label}</span>
          <span className="block text-xs text-muted-foreground">{withCode(field.description)}</span>
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
    <Field
      id={id}
      label={
        <>
          {label}
          {!field.required && <span className="font-normal text-muted-foreground"> (optional)</span>}
        </>
      }
      help={withCode(`${field.description}${field.kind === "lines" ? " One per line." : ""}`)}
    >
      {field.kind === "lines" ? (
        <textarea
          id={id}
          name={name}
          rows={3}
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
    </Field>
  );
}

/** Plugins write their descriptions with Markdown's `code` spans, for paths and examples. */
function withCode(text: string): ReactNode {
  return text.split(/`([^`]+)`/).map((part, index) =>
    index % 2 === 1 ? (
      <code key={index} className="rounded bg-muted px-1 font-mono">
        {part}
      </code>
    ) : (
      part
    ),
  );
}
