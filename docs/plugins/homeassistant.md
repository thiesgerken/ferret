# Home Assistant

The built-in `homeassistant` plugin makes inventory and configured behavior searchable: describe a device, function or room, find its exact current entity IDs and configured placement, then inspect Home Assistant. This is discovery, not an authoritative control-target selector, exhaustive reporting engine or live control/state interface.

## Credentials and connection

Create a dedicated **administrator** account and a long-lived access token in its Home Assistant profile. Full loaded automation/script configuration reads require administrator access. The plugin issues only read commands, but the token itself can control and administer Home Assistant. Protect it accordingly. Ferret encrypts the top-level write-only `token` using its existing connection credential machinery.

Automation and script definitions are copied faithfully, including configured variables, thresholds, payloads, action targets and templates. There is no heuristic redaction. If you later add secrets to those definitions, they become private indexed content and may reach the configured embedding endpoint and agents allowed to search this source. Do not connect an origin whose definitions you cannot copy under those conditions.

In Connections select `homeassistant`, a dedicated root such as `homeassistant/home`, and hourly cron `0 * * * *`. Existing **Run now** performs the same full reconciliation.

```json
{
  "url": "https://ha.internal.example.com",
  "public_url": "https://ha.example.com",
  "token": "<dedicated administrator long-lived access token>"
}
```

- `url` is the worker-reachable HTTP(S) **instance root**, without `/api`, credentials, query, fragment or a path prefix. The worker derives WS(S) `/api/websocket`.
- Optional `public_url` is the browser-facing instance root; it uses `url` when omitted.
- The transport uses normal TLS certificate verification, no redirect following and finite connection/authentication/command deadlines. There is no insecure certificate switch or new polling setting.
- The protocol baseline is Home Assistant Core **2026.10.2**. The plugin needs device/entity/area/floor/label registries, batched extended entity reads, runtime entity enumeration, loaded `automation/config` and `script/config`, and `search/related`. It does not scrape files or fetch blueprint URLs.

## Documents and identity

For root `homeassistant/home`:

| Source | Document | Stable external identity |
| --- | --- | --- |
| `homeassistant/home/devices` | Device plus eligible attached entity sections | Device registry ID |
| `homeassistant/home/standalone-entities` | Eligible device-less entity without a dedicated type | `registry:<entity registry id>` or `runtime:<current entity_id>` |
| `homeassistant/home/areas` | Area and compact placement directory | Area registry ID |
| `homeassistant/home/automations` | Automation and faithful loaded definition | Entity registry ID, or current entity ID if registry-less, with the same prefixes |
| `homeassistant/home/scripts` | Script, inputs and faithful loaded definition | Same entity identity rule |

No separate document is made for each ordinary attached entity, and automation/script documents are not duplicated under standalone entities. Moves between rooms/integrations do not create source paths. An entity-ID rename preserves a registered entity's identity; a registry-less rename is a deletion plus creation. Attaching a standalone entity moves its inventory into the device document and sweeps the standalone copy after success. Registry devices are never merged by name or MAC address.

Device links open `/config/devices/device/<device-id>` and area links open `/config/areas/area/<area-id>`. Automation/script links open the loaded-definition viewer `/config/automation/show/<entity-id>` or `/config/script/show/<entity-id>`, including YAML resources. Ordinary registered standalone entities open the supported domain-filtered inventory `/config/entities?domain=<domain>`, not an invented entity permalink. Ordinary registry-less entities use the instance home. These surfaces were exercised in the disposable 2026.10.2 frontend.

## Indexed inventory

Deterministic Markdown keeps exact entity IDs near device and placement context. It includes user/original names, aliases, scoped label names/descriptions, origin registry IDs, integration/platform, domain, descriptive device class/category/static unit, and supplied hardware identity (serial/model/model ID/manufacturer, MAC connections, hardware/firmware versions). Typed metadata retains relationships and allowlisted origin facts, not raw registry options/capability dumps.

Entity area overrides device area. An unassigned child device may inherit its logical parent's area; a connected-through hub's area is **not** inherited. Floor and area aliases remain searchable. Explicit assignments and effective placement retain provenance. Unknown placement is unknown, regardless of names. Parent hardware facts are explicitly attributed to the parent; child devices do not acquire fabricated hardware identity. Device registry entries can represent services rather than physical hardware.

Hidden, registry-disabled and diagnostic-category entities are excluded from device inventories, standalone documents, area directories and dedicated automation/script eligibility. Configuration-category entities remain unless hidden/disabled. Device and area documents remain even if nothing passes these rules, stating that no entities are included under indexing rules, not that the origin has none. A runtime `off` automation is still included. Registry-less YAML entities have no registry flags or assigned placement to infer.

This is an inventory filter, not secrecy/redaction: explicit references to excluded entities remain in faithful definitions and recognized behavior relationships.

## Configured behavior and relationships

Automation definitions include descriptions, triggers, conditions, actions and configured variables/payloads. Scripts include descriptions, fields and action sequences/control flow. Deterministic readable JSON preserves array order and expressions. Templates are never evaluated. Loaded-entity configuration APIs cover YAML resources as well as editor-managed resources; Home Assistant supplies expanded blueprint raw configuration. A missing/null/malformed required definition fails the run, rather than producing a successful empty document.

Home Assistant's recognized relationships provide concise named resources in both directions: related automations/scripts on device/area documents and related devices/entities/areas on behavior documents. They are not exhaustive dependency analysis; dynamic templates can prevent target resolution. Full definitions are not duplicated into each device.

Recognized references may legitimately name deleted or unavailable inventory resources. Their exact IDs remain in definitions and relationship metadata and are marked “unresolved in current inventory” in rendered relationship sections, without invented names or documents. This does not turn a stale saved target into a failed sync. Missing required inventory listings, entity details, definitions, transport responses or inconsistent registry placement references still fail the run.

No current values, on/off state, execution traces/histories, last-triggered/running values, state-change timestamps, arbitrary runtime attributes, camera data or scene definitions are indexed. Scene entities can still appear in ordinary eligible inventory.

## Synchronization and dates

Each run uses one short-lived authenticated WebSocket. It enumerates registries, reads extended details in batches, supplements with allowlisted runtime entity descriptions, fetches required definitions and recognized relations, validates references and renders everything before opening the sync. Only all successful required reads and writes authorize the connection-root sweep. Failed authentication, a disconnect, missing details/definition or detected inconsistent references preserve previous unseen documents. Earlier successful puts can remain if a later write fails, as with other plugins.

Registry-less entities missing from a complete runtime enumeration are swept, even if temporarily unloaded. Registered entities remain based on registry membership. There is no grace period or guessed rename matching. Separate registry reads are not an atomic snapshot: detected inconsistency fails; undetected concurrent changes converge next run.

Creation dates use reliable origin registry dates when available and otherwise stay null. Home Assistant's zero/Unix-epoch registry-migration sentinel means unknown, so it becomes null in document dates and typed metadata rather than a real 1970 date. Real registry dates remain intact. Composite modification dates remain null because registry dates do not cover every represented definition, relation or membership change. No run clock or runtime state-change date is substituted. Every run fully reconciles serialized content, so unchanged reruns remain unchanged and label/floor/entity/definition edits update affected representations without a parent modified-since shortcut.

Search is bounded retrieval, not a complete inventory export. Read the full device/area document and verify the actual origin before controlling hardware. See [search](../search.md), [connections](../connections.md) and the [design rationale](../conception/homeassistant.md).
