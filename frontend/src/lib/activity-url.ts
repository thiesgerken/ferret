/** The Activity page, narrowed to one connection's or API key's ingests when `writer` is set. */
export function activityHref(writer: string | null): string {
  return writer ? `/activity?writer=${encodeURIComponent(writer)}` : "/activity";
}
