export type TextPart = { text: string; highlight: boolean };

/**
 * Markdown syntax to hide in a snippet, each as four groups: what precedes the syntax, the opening
 * syntax, the text it wraps, the closing syntax. The opening and closing syntax go, the rest stays.
 */
const MARKDOWN_SYNTAX = [
  // Heading markers, blockquotes and list bullets at the start of a line.
  /(^[ \t]{0,3})(#{1,6}[ \t]+|>[ \t]?|[-*+][ \t]+)()()/gm,
  // Links and images: their text (or alt text) stays, the target goes.
  /()(!?\[)([^\]\n]*)(\]\([^)\n]*\))/g,
  /()(\*\*|__)(\S[^\n]*?)(\2)/g,
  /(^|[^\w*])(\*)([^*\s](?:[^*\n]*[^*\s])?)(\*)(?![\w*])/g,
  // Not inside words, so snake_case survives.
  /(^|\W)(_)([^_\s](?:[^_\n]*[^_\s])?)(_)(?!\w)/g,
  /()(`+)([^`\n]+?)(\2)/g,
];

/**
 * Removes Markdown syntax from a chunk's parts, so a snippet reads like the rendered text. Works
 * on the joined text and then splits it back, so highlights stay on the characters they marked.
 */
export function plainMarkdown(parts: TextPart[]): TextPart[] {
  const text = parts.map((part) => part.text).join("");
  const kept = new Array<boolean>(text.length).fill(true);
  const drop = (from: number, to: number) => kept.fill(false, from, to);
  for (const syntax of MARKDOWN_SYNTAX) {
    for (const match of text.matchAll(syntax)) {
      const [, lead, open, wrapped, close] = match;
      const start = match.index! + lead.length;
      drop(start, start + open.length);
      const closeAt = start + open.length + wrapped.length;
      drop(closeAt, closeAt + close.length);
    }
  }

  let offset = 0;
  return parts.flatMap((part) => {
    // By UTF-16 index like the match positions. Syntax is ASCII, so no surrogate pair splits.
    let plain = "";
    for (let index = 0; index < part.text.length; index++) {
      if (kept[offset + index]) plain += part.text[index];
    }
    offset += part.text.length;
    return plain ? [{ text: plain, highlight: part.highlight }] : [];
  });
}

/**
 * Cuts a chunk's parts down to about `maxChars`, starting a little before the first highlight, so
 * a long chunk still shows the match. Cut ends get an ellipsis.
 */
export function snippet(parts: TextPart[], maxChars = 320, lead = 80): TextPart[] {
  const text = parts.map((part) => part.text).join("");
  const firstHighlight = parts.findIndex((part) => part.highlight);
  const highlightAt = parts.slice(0, Math.max(firstHighlight, 0)).reduce((n, part) => n + part.text.length, 0);
  const start = Math.max(0, Math.min(highlightAt - lead, text.length - maxChars));
  const end = Math.min(text.length, start + maxChars);

  const result: TextPart[] = [];
  let offset = 0;
  for (const part of parts) {
    const partStart = offset;
    offset += part.text.length;
    const from = Math.max(start, partStart);
    const to = Math.min(end, offset);
    if (from < to) result.push({ text: part.text.slice(from - partStart, to - partStart), highlight: part.highlight });
  }
  if (start > 0 && result.length > 0) result[0] = { ...result[0], text: `…${result[0].text.trimStart()}` };
  if (end < text.length && result.length > 0) {
    const last = result[result.length - 1];
    result[result.length - 1] = { ...last, text: `${last.text.trimEnd()}…` };
  }
  return result;
}
