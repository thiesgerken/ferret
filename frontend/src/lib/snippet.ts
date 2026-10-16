export type TextPart = { text: string; highlight: boolean };

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
