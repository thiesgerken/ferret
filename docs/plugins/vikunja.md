# Vikunja tasks

Use `vikunja` to search tasks and their complete comment discussions through Vikunja's REST API v2. Checked against **Vikunja 2.7.0**, including a restricted read-only API token. There is no v1 fallback. Give the token's user access only to projects that should enter ferret.

## Set up

In Vikunja, create a token under **Settings > API Tokens** with these permissions:

- `projects`: `read_all`
- `tasks`: `read_all`
- `tasks_comments`: `read_all`

Those are the API scope names; the instance's `/api/v2/routes` endpoint lists its available permissions. No create, update, delete, administrator or attachment-download permission is needed. Token expiration or revocation fails the next run without sweeping indexed documents. See Vikunja's [authentication documentation](https://vikunja.io/docs/api-documentation/) and [v2 reference](https://vikunja.io/docs/api-v2/).

Create a connection with plugin `vikunja`, source `tasks/vikunja`, a name/icon and schedule `*/15 * * * *`:

```json
{
  "url": "http://vikunja:3456",
  "public_url": "https://tasks.example.com",
  "token": "replace-with-vikunja-api-token"
}
```

`url` is the instance root as the worker reaches it, without `/api/v2`. A deployment under a URL path can include that path. HTTP and HTTPS are supported; embedded credentials, query strings and fragments are not. Redirects are not followed: use an address that reaches Vikunja directly rather than an interactive login proxy.

`public_url` is optional. When omitted, links use Vikunja's configured `/api/v2/info.frontend_url`, not the worker's internal address. The token is write-only and encrypted separately from visible connection configuration. Changing, adding or removing the browser-only `public_url` preserves it. Changing the worker-facing `url` requires re-entry, even alongside a browser-link change. See [connections](../connections.md).

## What is indexed

Each task becomes one plain-text document at the connection source. Its stable external ID is the numeric task ID, so a move between projects updates the same document. Links open `{public_url}/tasks/{id}`.

The searchable body contains project name, completion status, task identifier, labels, assignees, priority and any due date, followed by the description and complete comments with author/timestamp attribution. HTML is converted locally to text using the worker's existing HTML decoder. This is a search representation, not a lossless rich-text export.

- Includes completed tasks and tasks in archived projects. Project enumeration explicitly requests archived projects; the global `/tasks` endpoint alone omits that history.
- Ignores saved filters, which Vikunja also returns as virtual projects with negative IDs. They do not own additional tasks.
- `authors` contains the task creator and comment authors, using their Vikunja usernames.
- `meta` contains project ID/name/archive status, task identifier, completion state/date, due/start/end dates, priority, completion fraction, tags, assignees and attachment IDs/names. Unset Vikunja dates become JSON null rather than year-one timestamps.
- Creation uses the task's creation time. Modification is the latest known task, project, label or remaining-comment update time. It is not an ingestion timestamp; removing the newest comment can move it backwards while still updating the indexed text.
- Attachment bytes, OCR, project descriptions as separate documents, and webhook ingestion are not included.

## Synchronization and failure behavior

Every scheduled run enumerates all accessible projects, each project's tasks and every task's comment pages. It resends complete task documents and commits a sweep only after all required reads and writes succeed. Unchanged documents remain no-op puts. This also picks up project/label renames, comment edits/deletions, task moves, task deletions and lost project visibility without assuming task timestamps cover the assembled document.

The plugin follows the response's effective page size, which Vikunja can cap below the requested size. Saved-filter projects are appended outside Vikunja's project pagination counts and are removed before validating the real inventory. It does not rely on `expand=comments`, which truncates discussions.

Changed pagination totals, duplicate IDs, incomplete pages, task moves detected during enumeration, HTTP errors and malformed required data fail the run rather than authorize a sweep. Earlier puts in a failed run are not rolled back. Vikunja's page-based API is not a snapshot: a concurrent change can still shift an inventory without changing its count. A later full run reconciles those changes.

## Verify and troubleshoot

Inspect **Activity** for project count, put/unchanged/deleted counts and errors. Search a distinctive description or late comment, confirm completed/archived tasks are present, and open the origin link. Editing or removing a comment must update the same task document on the next successful run.

- **401/403:** check token expiration, the three read scopes and the user's project access. Do not solve this by granting write/admin permissions.
- **404 or invalid REST v2 response:** check the instance root, Vikunja version and `/api/v2/docs`. v1-only instances are unsupported.
- **Redirect:** the configured worker URL probably points at a login proxy.
- **Changed/incomplete listing:** let origin writes settle and run again. Existing documents are not swept by the failed run.
- **Incorrect origin URL:** fix Vikunja's configured frontend URL or set `public_url`; changing it takes effect on the next full run.

The worker can reach operator-accessible HTTP services, as with the other HTTP plugins. Constrain its network egress where connections are not trusted. Origin response bodies and JSON decoder details are not included in run errors because they may contain secrets.

Source: [schema, REST client, mapping and reconciliation](../../crates/ferret-plugins/src/plugins/vikunja.rs), [behavioral regression tests](../../crates/ferret-server/tests/vikunja_plugin.rs).
