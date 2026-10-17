// Schemes that run code or smuggle content when a browser follows the link. A denylist, because
// origins legitimately use custom schemes (obsidian:, mid:, zotero:). The server rejects them at
// ingest with the same rule (crates/ferret-server/src/documents.rs); this guards what's already
// stored or slips through.
const DANGEROUS_SCHEMES = ["javascript", "data", "vbscript"];

/**
 * `url` if it may be rendered as a link, `null` otherwise. Reads the scheme the way browsers do:
 * leading control characters and spaces don't count, tabs and line breaks inside are dropped,
 * case doesn't matter.
 */
export function linkableUrl(url: string | null | undefined): string | null {
  if (!url) return null;
  const normalized = url.replace(/^[\u0000- ]+/, "").replace(/[\t\n\r]/g, "");
  const colon = normalized.indexOf(":");
  const scheme = colon === -1 ? "" : normalized.slice(0, colon).toLowerCase();
  return DANGEROUS_SCHEMES.includes(scheme) ? null : url;
}
