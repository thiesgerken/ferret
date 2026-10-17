/** The plugin whose connection owns `source`: the connection with the longest source root at or
 * above it. Null for sources no connection owns, which an API key fills. */
export function pluginOf(source: string, connections: { source: string; plugin: string }[]): string | null {
  let owner: { source: string; plugin: string } | null = null;
  for (const connection of connections) {
    const owns = source === connection.source || source.startsWith(`${connection.source}/`);
    if (owns && (!owner || connection.source.length > owner.source.length)) owner = connection;
  }
  return owner?.plugin ?? null;
}
