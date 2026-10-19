# AFFiNE workspaces

Use `affine` for documents synced to a self-hosted AFFiNE workspace. Invite a dedicated AFFiNE account with read access to that workspace and use its email/password. The plugin signs in to AFFiNE directly; it does not use an API token, SSO session or browser cookie supplied by you.

## Set up

Copy the workspace id from `/workspace/<id>/…` in the AFFiNE UI. Create plugin `affine`, source `notes/affine`, a name/icon and schedule `*/15 * * * *`:

```json
{
  "url": "http://affine:3010",
  "public_url": "https://affine.example.com",
  "workspace_id": "replace-with-workspace-id",
  "email": "ferret@example.com",
  "password": "replace-with-dedicated-user-password"
}
```

All fields except `public_url` are required. `url` must reach the instance directly from the worker, without an interactive authentication proxy. `public_url` defaults to `url` and controls origin links. The account must have its password sign-in available.

## Mapping and reconciliation

One live workspace doc becomes one Markdown document at the connection source, identified by AFFiNE doc id. Title prefers the rendered page title, then workspace metadata. Origin links use `/workspace/<workspace-id>/<doc-id>`. No threads or authors are assigned. Creation uses workspace metadata when present and otherwise stays null; modification independently uses AFFiNE's merged update timestamp. The update time is not a substitute creation date.

Every run reads the workspace root's document list, excludes trash, marks live ids seen, fetches changed content and commits a sweep. A ten-minute overlap catches merge lag around the saved update cursor. Docs restored from trash are fetched again. Changing instance/workspace addresses refetches all docs. Removing/trashing a doc removes the indexed copy on successful reconciliation.

The renderer converts supported AFFiNE blocks to Markdown and rewrites internal doc references with workspace titles/links. Unknown block types retain their available text and are logged. This is a search-text representation, not a lossless page/whiteboard export. Repeated/cyclic block structures are rendered once. Binary assets are not a substitute for searchable page text.

## Verify and troubleshoot

Inspect Activity for workspace/live/trash/changed counts and errors. Search a known title under `notes/affine`, then check its origin opens the correct page. A document that exists only on an unsynced client has no server content and is skipped with a log message. Wait for AFFiNE to sync before retrying.

Redirected sign-in usually means `url` points at a login proxy. A workspace-not-readable error means the account cannot read the root document or the id is wrong. Self-hosted AFFiNE protocol changes can require plugin changes; do not assume arbitrary future AFFiNE versions or AFFiNE Cloud have the same API contract.

See [connections](../connections.md). Source: [schema, auth and sync](../../crates/ferret-plugins/src/plugins/affine/mod.rs), [workspace decoding](../../crates/ferret-plugins/src/plugins/affine/workspace.rs), [Markdown rendering](../../crates/ferret-plugins/src/plugins/affine/markdown.rs).
