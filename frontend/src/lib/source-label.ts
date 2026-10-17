/** Where a document comes from, as the API names it: a connection or an API key. */
export type Origin = { kind: "CONNECTION" | "API_KEY"; id: string; name: string; icon: string };

export type SourceLabel = {
  /** The writer's icon, or null when the writer isn't known. */
  icon: string | null;
  /** "Notes › journal › 2026": the writer's name, then the path below a connection's root. */
  text: string;
};

/**
 * How a source reads: under its writer's name instead of as a bare path. A connection's root is
 * its name, so only what's below it shows. An API key has no root, so its sources show whole.
 * `roots` maps connection ids to their source roots.
 */
export function sourceLabel(source: string, origin: Origin | null | undefined, roots: Map<string, string>): SourceLabel {
  const path = (value: string) => value.split("/").join(" › ");
  if (!origin) return { icon: null, text: path(source) };
  const root = origin.kind === "CONNECTION" ? roots.get(origin.id) : undefined;
  const below = root === undefined ? source : source === root ? "" : source.startsWith(`${root}/`) ? source.slice(root.length + 1) : source;
  return { icon: origin.icon, text: below ? `${origin.name} › ${path(below)}` : origin.name };
}
