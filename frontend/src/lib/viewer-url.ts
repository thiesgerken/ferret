/** The viewer's URL for a document, optionally scrolled to a hit's range (server offsets, in
 * characters). */
export function viewerHref(id: string, range?: { start: number; end: number }): string {
  const params = new URLSearchParams({ id });
  if (range) {
    params.set("start", String(range.start));
    params.set("end", String(range.end));
  }
  return `/view?${params}`;
}
