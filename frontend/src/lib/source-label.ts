import type { WriterKind } from "@/gql/graphql";

/** A document's writer as the queries select it. Codegen exports only the operations' types. */
export type Writer = { kind: WriterKind; id: string; name: string; icon: string };

export type SourceLabel = {
  /** The writer's icon, or null when the writer isn't known. */
  icon: string | null;
  /** "Notes › journal › 2026": the writer's name, then the path below a connection's root. */
  text: string;
};

/**
 * How a source reads: under its writer's name instead of as a bare path. A connection's root is
 * its name, so only what's below it shows. An API key has no root, so its sources show whole.
 * `roots` maps connection ids to their source roots; until they're known, a connection's sources
 * read as its name alone rather than as its name over the full path.
 */
export function sourceLabel(
  source: string,
  writer: Writer | null | undefined,
  roots: Map<string, string> | undefined,
): SourceLabel {
  const path = (value: string) => value.split("/").join(" › ");
  if (!writer) return { icon: null, text: path(source) };
  if (writer.kind === "API_KEY") return { icon: writer.icon, text: `${writer.name} › ${path(source)}` };

  const root = roots?.get(writer.id);
  // A source at or above the root (a selected parent covering this one connection) is the
  // connection as a whole.
  if (root === undefined || !source.startsWith(`${root}/`)) return { icon: writer.icon, text: writer.name };
  return { icon: writer.icon, text: `${writer.name} › ${path(source.slice(root.length + 1))}` };
}
