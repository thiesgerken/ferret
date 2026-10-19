/** The icon a new connection starts with, by plugin. The user can pick another. */
const PLUGIN_ICONS: Record<string, string> = {
  affine: "notebook-pen",
  git: "git-branch",
  homeassistant: "house",
  carapace: "brain",
  paperless: "file-text",
  imap: "mail",
  karakeep: "bookmark",
  mealie: "cooking-pot",
  moneta: "wallet",
  vikunja: "list-checks",
};

export function defaultPluginIcon(plugin: string): string {
  return PLUGIN_ICONS[plugin] ?? "plug";
}

export const DEFAULT_API_KEY_ICON = "key-round";
