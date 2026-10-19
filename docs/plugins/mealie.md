# Mealie recipes

Use `mealie` to search recipes and their attached notes and comments. The token user's **entire group**, across households, is copied into Ferret. There is no household, owner, public/private, tag or cookbook filter. Choose the Ferret connection owner and source access accordingly.

## Set up

In Mealie's user profile, create an API token. No administrator credentials are needed. Mealie tokens carry their owner's capabilities: although this plugin issues only GET requests, the token itself is **not a scoped read-only credential**.

Create a Ferret connection with plugin `mealie`, source root `food/mealie`, and schedule `0 * * * *` (hourly):

```json
{
  "url": "http://mealie:9000",
  "public_url": "https://recipes.example.com",
  "token": "replace-with-mealie-user-token"
}
```

`url` is the instance root reachable by the worker, without `/api`. `public_url` defaults to `url` and controls browser links only. Both preserve instance base paths and must be absolute HTTP(S) URLs without userinfo, query or fragment. Use a direct instance address, not an interactive login proxy: redirects are not followed. Requests have finite connection and response timeouts.

The required token is marked `writeOnly` in the configuration schema. Ferret encrypts it separately and does not return it through visible configuration. This is a Ferret storage/UI property, not a restriction on Mealie token permissions.

## Indexed content and identity

One recipe becomes one Markdown document at `<root>/recipes`, with its UUID as `external_id`. Slug and title changes keep document identity. Hits open `{public_url}/g/{groupSlug}/r/{slug}/` in Mealie, not the original recipe website or an individual comment. Path segments are URL-encoded; the trailing slash is required by Mealie's static frontend. No creator attribution is invented from owner IDs.

The searchable body contains supplied description, ordered ingredient display text and section titles, ordered instruction titles/summaries/text, attached recipe notes, comments labelled with available author and timestamps, categories, tags, tools, servings/yield, times and nutrition. Authored Markdown is preserved. Comments are sorted by creation timestamp and UUID; categories, tags and tools by name. Referenced recipes stay references: the plugin does not recursively copy them or fetch their contents.

Metadata includes recipe/group/household/user IDs, slug, original website URL, categories/tags/tools, servings/yield, nutrition and structured duration seconds. Arbitrary extras, settings and raw responses are not copied. Binary/image assets, external recipe websites, meal plans, shopping lists, cooking timeline events and standalone cookbooks are outside the plugin's scope. Notes and comments do not become separate documents.

`created_at` is the recipe's required `createdAt`. `modified_at` is the latest recipe `updatedAt` or attached comment `updatedAt`. Missing/invalid required origin timestamps fail the run; ingestion time is never substituted. Note/tag/comment edits are picked up by complete puts even if the recipe's own `updatedAt` does not change. Mealie 3.29's structured `totalTimeSeconds`, `prepTimeSeconds` and `performTimeSeconds` are included alongside supplied text durations.

## Full reconciliation and failures

Every run discovers the token user's group with `/api/users/self`, lists every bounded `/api/recipes` page in explicit `id ASC` order, and fetches each complete recipe by UUID. It starts a sync over **only `<root>/recipes`** and commits only after every origin read and document put succeeds. Sibling sources are not swept. Unchanged puts are no-ops; excluded documents stay excluded. State is `{}`; there is no incremental cursor or persistent cache.

A successful complete run removes deleted or inaccessible recipes. Duplicate/out-of-order IDs, inconsistent counts/pages, truncated inventories, cross-group or mismatched detail IDs, malformed required data, HTTP/authentication errors and failed detail reads (including a concurrent-deletion 404) fail without authorizing a sweep. Pagination links are checked for presence but never followed with credentials.

This uses standard Ferret eventual consistency: a failed run retains previously indexed unseen documents, including after token revocation. Earlier successful puts are not rolled back. Offset pagination is not an atomic snapshot: concurrent changes that do not expose an inconsistency may shift the inventory, and the next full run reconciles it. There is no second enumeration pass or snapshot protocol. The full origin-read cost is intentional for small recipe collections.

## Troubleshooting and privacy

- **401/403:** check token expiration/revocation and the user's group access. Revoking the token does not erase Ferret's existing copy; remove indexed content explicitly when required.
- **Invalid/incomplete JSON:** check that the configured root reaches Mealie directly and returns the required recipe fields and timestamps.
- **Changed/incomplete listing:** let concurrent origin edits settle and retry. Failed runs do not remove unseen documents.
- **Wrong browser address:** set `public_url`; the next full run updates existing links.

Errors/logs omit origin response bodies, JSON decoder diagnostics and credentials. Restrict worker egress for untrusted connections. Indexed recipe and comment text follows normal Ferret search, chunking and configured embedding behavior.

Source: [plugin](../../crates/ferret-plugins/src/plugins/mealie.rs), [real-server behavioral tests](../../crates/ferret-server/tests/mealie_plugin.rs). Checked against an isolated real Mealie **v3.29.0** container: registered-worker ingestion and keyword search for ingredients/instructions/notes/comments, resolving origin links, unchanged no-op, note/comment edits, UUID-preserving rename with updated deep link, and deletion/read-back. The stable image digest was `sha256:78eb28b8776d0bad40c5e605c72b3f4f5976793e491558c7dcbdfc04c835d4bc`. This is not a claim of compatibility with all historical releases.
