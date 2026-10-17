/** A form field for one property of a plugin's config schema. */
export type ConfigField = {
  name: string;
  kind: "text" | "lines" | "checkbox";
  required: boolean;
  /** `writeOnly` in the schema: the server stores it encrypted and never returns it. */
  secret: boolean;
  description: string;
  default: unknown;
};

type Property = {
  type?: string | string[];
  items?: { type?: string };
  description?: string;
  default?: unknown;
  writeOnly?: boolean;
};

/**
 * The form fields of a plugin's config schema, required ones first. Only the shapes plugins use are
 * supported: strings, lists of strings and booleans, each optionally nullable. Anything else
 * throws, so a plugin with a new shape fails visibly instead of getting a field that can't hold
 * its value.
 */
export function configFields(schemaJson: string): ConfigField[] {
  const schema = JSON.parse(schemaJson) as { properties?: Record<string, Property>; required?: string[] };
  const required = new Set(schema.required ?? []);
  return Object.entries(schema.properties ?? {})
    .map(([name, property]) => ({
      name,
      kind: fieldKind(name, property),
      required: required.has(name),
      secret: property.writeOnly === true,
      description: property.description ?? "",
      default: property.default,
    }))
    .sort((a, b) => Number(b.required) - Number(a.required));
}

function fieldKind(name: string, property: Property): ConfigField["kind"] {
  const types = [property.type].flat().filter((type) => type !== "null");
  if (types.length === 1) {
    if (types[0] === "string") {
      return "text";
    }
    if (types[0] === "boolean") {
      return "checkbox";
    }
    if (types[0] === "array" && property.items?.type === "string") {
      return "lines";
    }
  }
  throw new Error(`The config field "${name}" has a type this form can't show.`);
}

/** The form input's name for a config field, apart from the connection's own fields. */
export function configInputName(field: ConfigField): string {
  return `config.${field.name}`;
}

/** The config from a submitted form. Empty fields are left out, so the plugin's defaults apply. */
export function configFromForm(fields: ConfigField[], form: FormData): Record<string, unknown> {
  const config: Record<string, unknown> = {};
  for (const field of fields) {
    const value = String(form.get(configInputName(field)) ?? "").trim();
    if (field.kind === "checkbox") {
      config[field.name] = value === "on";
    } else if (field.kind === "lines") {
      const lines = value
        .split("\n")
        .map((line) => line.trim())
        .filter(Boolean);
      if (lines.length > 0) {
        config[field.name] = lines;
      }
    } else if (value) {
      config[field.name] = value;
    }
  }
  return config;
}
