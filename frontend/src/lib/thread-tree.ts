/** A document of a thread with the documents that answer it, oldest first. */
export type ThreadNode<T> = { entry: T; children: ThreadNode<T>[] };

/**
 * Arranges a thread's entries (oldest first) into trees by their parents. An entry whose parent
 * isn't among them starts a tree of its own: the parent may be excluded, deleted, never indexed or
 * outside the outline's window. So does the oldest entry of a parent cycle (broken mail headers),
 * so every entry shows exactly once.
 */
export function threadTree<T extends { externalId: string; parent?: string | null }>(entries: T[]): ThreadNode<T>[] {
  const nodes = new Map(entries.map((entry) => [entry.externalId, { entry, children: [] as ThreadNode<T>[] }]));
  const roots: ThreadNode<T>[] = [];
  for (const node of nodes.values()) {
    const parent = node.entry.parent ? nodes.get(node.entry.parent) : undefined;
    (parent?.children ?? roots).push(node);
  }

  const reached = new Set<ThreadNode<T>>();
  const reach = (node: ThreadNode<T>) => {
    reached.add(node);
    node.children.forEach(reach);
  };
  roots.forEach(reach);
  for (const node of nodes.values()) {
    if (!reached.has(node)) {
      const parent = nodes.get(node.entry.parent!)!;
      parent.children.splice(parent.children.indexOf(node), 1);
      roots.push(node);
      reach(node);
    }
  }
  return roots;
}
