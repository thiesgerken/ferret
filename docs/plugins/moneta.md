# Moneta expenses

Use `moneta` to search every expense visible to a Moneta API key's owner.

## Set up

In Moneta's settings, create an API key with **Nur lesen** (read) access. No write or administrator access is needed. The key sees exactly its owner's rendered expenses, including shared-account and synchronized-account rendering.

Create a Ferret connection with plugin `moneta`, source root `finance/moneta`, and schedule `0 * * * *` (hourly):

```json
{
  "url": "http://moneta:8000",
  "public_url": "https://money.example.com",
  "token": "replace-with-moneta-read-key"
}
```

`url` is the worker-reachable instance root, without `/graphql`. `public_url` is optional and defaults to `url`. HTTP(S) base paths are preserved: a root ending in `/household` reads `/household/graphql` and links to `/household/expenses?expense=<uuid>`. Credentials embedded in URLs, query strings and fragments are rejected. Redirects are not followed; bypass interactive login proxies. Requests have a finite timeout.

`token` is write-only. Ferret encrypts it separately from visible configuration and supplies it only to a claimed worker run. Re-enter it when changing non-secret configuration. See [connections](../connections.md) for setup, **Run now**, and exclusions.

## Search representation

One expense becomes one deterministic plain-text document in `<root>/expenses`, with its UUID as stable `external_id`. A hit opens the selected expense's editor in Moneta. No author attribution is invented.

All visible expenses are included without account, date or flag filters: purchases, income, transfers, incomplete/unchecked/tax-relevant/extraordinary expenses and templates. Templates begin with **Template, not an actual payment** and carry `meta.is_template = true`. They are searchable context, not financial events to add into a report.

The body includes:

- Title, store, description and expense comments.
- Category names and weights.
- Every rendered transaction's account name, value date (Wertstellung), booking date when present, calculated account movement, effective wealth change, fraction, comments and bank statement.
- Accrual period, Moneta record creation and edit timestamps, and all flags.
- Line-item titles, quantities, units, amounts and comments.
- Receipt references and OCR text already stored in Moneta. References are text only: Ferret never downloads receipts, follows receipt URLs, runs OCR or ingests attachments.

### Money and dates

Moneta uses **cents**, with negative amounts for spending and positive amounts for income. `calculatedAmount` is movement on the account after resolving a fraction; `effectiveAmount` is the current reader's wealth change, accounting for ownership shares and visibility. A transaction on somebody else's non-mirrored account can have nonzero movement and zero effective change. Transfers can have offsetting movements. The body labels these separately and displays EUR without first truncating fractional cents.

Metadata preserves the typed rendered response under `meta.expense` (Moneta's camelCase field names), including IDs/names, signed amounts, fractions, flags, all transaction/booking dates, accrual endpoints, line items, receipts and record timestamps. `meta.currency` is `EUR`, and `meta.amount_unit` is `cents`, including the fraction-derived floating-point `calculatedAmount` and `effectiveAmount`. This is a search representation, not financial reporting or aggregation.

The document's `created_at` is its earliest transaction **value date**, not the Moneta record's creation date, booking date or accrual start. `modified_at` is the current expense's **updatedAt**, not an audit snapshot or ingestion time. Related account/category edits can change searchable content without changing that timestamp; full puts on every run still pick them up.

## Full reconciliation and failures

Every run lists all expense pages with explicit `ACCRUAL_START ASC` ordering and Moneta's UUID tiebreaker, sends complete puts, and commits a sync over **only `<root>/expenses`** after every read and write succeeds. Sibling sources under the root are not swept. Unchanged second runs produce no-op puts. Removed expenses and lost visibility disappear after the next successful run; exclusions remain excluded.

Changed counts, duplicate/out-of-order IDs, incomplete pages, missing/invalid required data, HTTP/authentication errors, malformed JSON and GraphQL errors (including HTTP 200 authentication errors) fail without authorizing a sweep. Earlier puts in a failed run are not rolled back. Offset pagination is not a snapshot: concurrent edits that do not expose an inconsistency may shift the inventory; the next hourly full run reconciles it. There is no incremental cursor, snapshot protocol or extra enumeration pass.

## Verify and troubleshoot

Search for a distinctive expense comment, bank statement, category, line item and receipt OCR phrase. Open the origin link and check the selected expense. Run unchanged, edit an expense, rename an account/category without editing the expense, then delete an expense or remove its visibility and use **Run now** to observe reconciliation. Check **Activity** for put/unchanged/deleted counts.

- **401/403 or GraphQL errors:** check the read key, expiration/revocation and its owner's visibility. Do not grant write access to fix indexing.
- **Invalid expense page:** check that `/graphql` reaches Moneta directly and returns all required expense fields.
- **Redirect:** use an internal/direct worker URL rather than an interactive login proxy.
- **Changed/incomplete listing:** let origin edits settle and retry. Failed runs retain documents that were not seen.
- **Wrong origin address:** set `public_url`; the next full run updates existing links.

Origin response bodies, JSON decoder details, credentials and Authorization headers are not logged. As with other HTTP plugins, constrain worker egress where connections are untrusted. Indexed financial text follows normal Ferret chunking, search and configured embedding behavior. The plugin does not inspect embedding gateways or enforce a Moneta-specific deployment/privacy policy.

Source: [plugin](../../crates/ferret-plugins/src/plugins/moneta.rs), [real-server behavioral tests](../../crates/ferret-server/tests/moneta_plugin.rs).
