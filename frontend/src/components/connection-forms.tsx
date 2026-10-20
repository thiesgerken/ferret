"use client";

import { useTranslations } from "next-intl";
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
  { label: "quarterHourly", cron: "*/15 * * * *" },
  { label: "hourly", cron: "0 * * * *" },
  { label: "nightly", cron: "0 3 * * *" },
] as const;

export function CreateConnectionForm({
  pluginTypes,
  onCreated,
  onCancel,
}: {
  pluginTypes: PluginType[];
  onCreated: () => void;
  onCancel: () => void;
}) {
  const t = useTranslations();
  const [{ fetching }, create] = useMutation(CreateConnectionMutation);
  const [plugin, setPlugin] = useState(pluginTypes[0].name);
  // Follows the plugin's default until the user picks one.
  const [pickedIcon, setPickedIcon] = useState<string | null>(null);
  const icon = pickedIcon ?? defaultPluginIcon(plugin);
  const [error, setError] = useState("");
  const configSchema = pluginTypes.find(({ name }) => name === plugin)?.configSchema;
  // The plugin list can outlive the worker that offered a plugin.
  const { fields, schemaError } =
    configSchema === undefined
      ? { fields: [], schemaError: t("connections.form.pluginGone", { plugin }) }
      : parseConfigSchema(configSchema);

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
        <h2 className="text-base font-semibold">{t("connections.form.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("connections.form.description")}</p>
      </div>
      <FormSection title={t("connections.form.pluginAndSource")} description={t("connections.form.pluginAndSourceHint")}>
        <Field id="connection-plugin" label={t("connections.form.plugin")}>
          <select id="connection-plugin" value={plugin} onChange={(event) => setPlugin(event.target.value)} className={fieldClass}>
            {pluginTypes.map(({ name }) => (
              <option key={name}>{name}</option>
            ))}
          </select>
        </Field>
        <Field
          id="connection-source"
          label={t("connections.form.source")}
          help={withCode(t("connections.form.sourceHelp"))}
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
          {t("connections.form.create")}
        </button>
        <button type="button" onClick={onCancel} className={secondaryButtonClass}>
          {t("common.cancel")}
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
  const t = useTranslations("common");
  const [{ fetching }, update] = useMutation(UpdateConnectionMutation);
  const [error, setError] = useState("");
  const { fields, schemaError } = parseConfigSchema(configSchema);
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
          {t("save")}
        </button>
        <button type="button" onClick={onCancel} className={secondaryButtonClass}>
          {t("cancel")}
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
  const t = useTranslations();
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
      <p>{t.rich("connections.form.deleteWarning", { source: connection.source, code })}</p>
      <Field id={`confirm-${connection.id}`} label={t.rich("connections.form.confirm", { source: connection.source, code })}>
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
          {t("connections.form.deleteAll")}
        </button>
        <button type="button" onClick={onCancel} className={secondaryButtonClass}>
          {t("common.cancel")}
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

function parseConfigSchema(schema: string): { fields: ConfigField[]; schemaError: string } {
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
  const t = useTranslations();
  return (
    <FormSection title={t("connections.form.identity")} description={t("connections.form.identityHint")}>
      <div className="flex items-end gap-3">
        <div className="space-y-1.5">
          <span className="block text-sm font-medium">{t("common.icon")}</span>
          <IconPicker value={icon} onChange={onIconChange} />
        </div>
        <div className="min-w-0 flex-1">
          <Field id={nameId} label={t("common.name")}>
            <input
              id={nameId}
              name="name"
              required
              pattern=".*\S.*"
              placeholder={t("connections.form.namePlaceholder")}
              defaultValue={defaultName}
              className={fieldClass}
            />
          </Field>
        </div>
      </div>
    </FormSection>
  );
}

function ScheduleSection({ id, defaultValue }: { id: string; defaultValue: string }) {
  const t = useTranslations("connections.form");
  const [schedule, setSchedule] = useState(defaultValue);
  return (
    <FormSection title={t("schedule")} description={t("scheduleHint")}>
      <Field id={id} label={t("cron")} help={withCode(t("cronHelp"))}>
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
            {t(`presets.${preset.label}`)}
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
  const t = useTranslations("connections.form");
  const editsSecrets = stored !== undefined && fields.some((field) => field.secret);
  return (
    <FormSection
      title={t("configuration")}
      description={
        <>
          {t.rich("configurationHint", {
            plugin: () => <span className="font-medium text-foreground">{plugin}</span>,
          })}
          {editsSecrets && ` ${t("secretsHint")}`}
        </>
      }
    >
      {schemaError ? (
        <p role="alert" className="text-sm text-destructive">
          {schemaError}
        </p>
      ) : fields.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("noConfiguration")}</p>
      ) : (
        <fieldset className="space-y-4">
          <legend className="sr-only">{t("configuration")}</legend>
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
  /** The server preserves an omitted secret unless a protected setting changes. */
  keepsStoredSecret: boolean;
}) {
  const t = useTranslations();
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
    ? t("connections.form.keepSecret")
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
          {!field.required && <span className="font-normal text-muted-foreground"> {t("common.optional")}</span>}
        </>
      }
      help={withCode(`${field.description}${field.kind === "lines" ? ` ${t("connections.form.onePerLine")}` : ""}`)}
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

function code(chunks: ReactNode) {
  return <code className="font-mono">{chunks}</code>;
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
