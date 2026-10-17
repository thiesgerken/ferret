/** A hit's range in a document's text, in JavaScript string indices (UTF-16 code units). */
export type HitRange = { start: number; end: number };

/**
 * Converts the server's offsets, which count characters (Unicode code points), into string
 * indices. They differ as soon as the text holds characters outside the BMP, such as emoji.
 */
export function toHitRange(text: string, startChar: number, endChar: number): HitRange {
  let index = 0;
  let chars = 0;
  let start = text.length;
  for (const char of text) {
    if (chars === startChar) start = index;
    if (chars === endChar) return { start, end: index };
    index += char.length;
    chars += 1;
  }
  return { start: Math.min(start, index), end: index };
}

type HastNode = {
  type: string;
  children?: HastNode[];
  properties?: Record<string, unknown>;
  position?: { start: { offset?: number }; end: { offset?: number } };
};

/**
 * A rehype plugin that marks the top-level blocks of rendered Markdown that overlap `range`: each
 * gets the `hit` class, the first one also `id="hit"` to scroll to. Block granularity, because
 * rendered Markdown has no 1:1 mapping back to characters of the source.
 */
export function rehypeMarkHit(range: HitRange) {
  return () => (tree: HastNode) => {
    let first = true;
    for (const node of tree.children ?? []) {
      const start = node.position?.start.offset;
      const end = node.position?.end.offset;
      if (node.type !== "element" || start === undefined || end === undefined) continue;
      if (start < range.end && end > range.start) {
        node.properties = { ...node.properties, className: ["hit"], ...(first ? { id: "hit" } : {}) };
        first = false;
      }
    }
  };
}
