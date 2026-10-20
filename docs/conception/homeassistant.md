# Home Assistant inventory and configured behavior

Status: approved design, superseding the registry-only research proposal of 2026-10-19. This is rationale and scope, not the operator contract. Use the maintained [Home Assistant guide](../plugins/homeassistant.md) for configuration and shipped behavior.

## Purpose and boundaries

A human or agent describes a real-world device, function or location, retrieves the right device and exact current entity IDs with configured placement, then follows the origin link. This is searchable discovery, not authoritative target selection, exhaustive reporting, live state/control, a knowledge graph or LLM fact extraction. Existing documents, connection scheduling, workers, encrypted credentials, Run now and sync sweeps are sufficient. No Home Assistant server tables or custom search UI are needed. [F1][F2]

One chosen connection root has five sibling sources: `devices`, `standalone-entities`, `areas`, `automations`, `scripts`. A device includes eligible attached entity sections; a device-less entity has one standalone document unless it has a dedicated automation/script type. Areas contain a compact placement directory. No room/integration-specific source paths and no duplicate ordinary attached-entity documents. Origin registry IDs give stable identity; registry-less entities use current entity IDs without guessed rename matching.

## Inventory and placement

Read all device/entity/area/floor/label registries, batch extended entity details and additionally enumerate runtime entities. The full registry list includes disabled entities; `list_for_display` does not. Extended details supply aliases and device classes omitted from the partial list. Runtime enumeration supplies registry-less YAML entities, but only allowlisted descriptive names, device class and static units enter inventory. No live values or state-change timestamps enter documents. [H1][H2]

Render searchable user/original names, aliases, scoped label descriptions, identifiers, integration/platform, category/class/units and supplied hardware identity. Keep exact entity IDs close to device/area/floor context for chunk-bounded search. Sort unordered collections; never add run timestamps. Typed metadata preserves useful IDs/relations without dumping raw options/capabilities. [F2][F3]

Entity area overrides device area. Logical child devices inherit a parent's area only when not explicitly assigned; `via_device_id` represents connectivity and never transfers hub placement. Unknown placement stays null in metadata and is omitted from Markdown rather than inferred from names. Known location context remains close to each entity, with assignment/inheritance provenance but without duplicate explicit/effective area lines. Parent hardware is explicitly attributed, not copied as child hardware. Service devices are not necessarily physical hardware and registry devices from separate integrations are not merged by name/MAC. [H3][H4]

Exclude hidden, registry-disabled and diagnostic-category entities consistently from inventory and dedicated-type eligibility. Keep configuration-category entities unless hidden/disabled, and keep switched-off automations because runtime off is not registry-disabled. Keep device/area documents with zero included entities and their known facts; omit empty sections without claiming that the origin has no entities. Registry-less entities have unknown registry flags, not inferred permission or location; generic missing-registry prose adds no searchable fact.

## Faithful configured behavior

Automations and scripts are part of initial delivery, not a later payload-free structural enrichment. Read loaded `automation/config` and `script/config` with dedicated administrator credentials. Those APIs expose loaded raw configuration, including YAML-defined resources and expanded blueprints, without assuming only editor-managed files. Preserve descriptions, triggers/conditions/actions, script fields/sequences, variables/payloads, configured thresholds/targets and control flow. Preserve templates as expressions, never execute them or claim fully resolved dynamic targets. [H5][H6]

Inventory exclusions are not global redaction. Explicit references to filtered entities remain in definitions. These definitions are private indexed content and may reach the configured embedding endpoint. There is no heuristic secret-redaction promise; operators must understand that later secret additions are copied too. Only read commands are issued, but the required administrator token itself is not read-only. [H5][H6][H7]

`search/related` supplies recognized relation sets. Device/area documents name related automations/scripts; behavior documents name related devices/entities/areas. Inverse recognized edges supplement inventory-direction results without claiming complete dependency analysis. Dynamic templates can hide relations. Definitions are not copied into every device. Do not broaden to scene definitions, histories, traces or last-triggered/running state. [H8]

HA intentionally retains recognized references to resources absent from current inventory, for example a saved automation targeting a deleted entity. Preserve such exact IDs as unresolved relationships, not invented named inventory or fatal registry corruption. Required registry/detail reads and registry placement references remain strictly validated.

## Honest nullable dates

The original required-date API could not represent registry-less entities honestly. Creation and modification origin dates must independently allow null through ingest/read/search, storage, chunks/embeddings, MCP, GraphQL and frontend presentation. Unknown dates are not the epoch, ingestion time, seen time or runtime last-changed time. Explicit creation-date bounds exclude unknowns; undated documents still participate in unfiltered keyword/semantic/hybrid search. [F2][F3][F4]

Use reliable registry creation dates where available. Composite modification time remains unknown where no origin timestamp covers represented configuration, membership and relations. Full deterministic puts still detect changes, including known/unknown transitions and unchanged null-date reruns. Update denormalized chunk/embedding dates without discarding unchanged embedding inputs. Thread listing/navigation puts undated documents last, ordered by id; Explore sorts unknown dates last and omits unknown month facets. [F2][F4]

Home Assistant registry migrations use zero/Unix epoch when no historical date exists. Normalize that origin sentinel to null for device/area/entity creation dates, including typed metadata, instead of exposing a fabricated date or matching an epoch-era creation filter.

## Synchronization and failure gate

One short-lived authenticated WebSocket per scheduled run, with finite deadlines, numbered result matching, success/error validation and ping/close handling. Derive WS(S) `/api/websocket` from a validated HTTP(S) instance root. `public_url` is optional and defaults to `url`; top-level `token` is write-only. Hourly cron and Run now use normal connection scheduling. No permanent subscriber, polling knob or generic retry framework. [F1][H1]

Fetch every required registry/detail/runtime/definition/relation response and validate identities/references before starting writes. Successful writes plus complete enumeration authorize sweep. Authentication failure, disconnect, malformed/missing definitions or detected inconsistent references never authorize destructive reconciliation. Earlier successful puts may remain if a later write fails. Separate registry reads are not atomic; detected races fail, undetected races converge next run. [F1]

Missing registry-less runtime entities are swept after successful enumeration, without a retention/grace heuristic. Registered entities rely on registry membership even if runtime-unloaded. Identity survives registry entity-ID renames and room moves; standalone attachment moves content into its device document and sweeps the previous standalone copy.

## Verification contract

Use real Ferret/ParadeDB plus a disposable **real** Home Assistant 2026.10.2 instance for origin/worker/search/read proof, without touching personal services. Verify browser device/area/entity paths instead of inventing permalinks. Device/area detail paths and `/config/{automation|script}/show/<entity-id>` are real origin surfaces. Ordinary registered standalone entities use the domain-filtered entity inventory; ordinary registry-less entities use instance home. Exercise nullable-date UI behavior on the actual surface, not only screenshots. Protocol fixtures provide deterministic black-box regressions, not a substitute for real-origin smoke.

Acceptance covers nullable dates independently, date filters, unchanged puts and denormalized transitions; document/type boundaries, exact entity-ID/hardware search, placement inheritance/overrides, child versus hub, registry-less YAML, filtering/config-category/off-automation rules, preserved excluded references, bidirectional named relations, faithful templates, stable rename identity, attachment/deletion sweeps, no-op reruns and failed-read preservation.

## Sources

- [F1: Worker plugin and sync contract](../plugins/README.md)
- [F2: Indexing invariants](../indexing.md)
- [F3: Search and bounded retrieval](../search.md)
- [F4: Complete ingestion contract](../ingestion.md)
- [H1: Official WebSocket API](https://contributors.home-assistant.io/core/internals/websocket-api/)
- [H2: Registry list and extended detail handlers](https://github.com/home-assistant/core/blob/2026.10.2/homeassistant/components/config/entity_registry.py)
- [H3: Device registry and logical child devices](https://contributors.home-assistant.io/core/integrations/device-registry/)
- [H4: Device serialization and parent/via distinction](https://github.com/home-assistant/core/blob/2026.10.2/homeassistant/helpers/device_registry.py)
- [H5: Loaded automation configuration API](https://github.com/home-assistant/core/blob/2026.10.2/homeassistant/components/automation/__init__.py#L1252-L1273)
- [H6: Loaded script configuration API](https://github.com/home-assistant/core/blob/2026.10.2/homeassistant/components/script/__init__.py#L784-L807)
- [H7: Authentication and long-lived tokens](https://contributors.home-assistant.io/core/internals/auth-api/)
- [H8: Recognized related-resource resolution](https://github.com/home-assistant/core/blob/2026.10.2/homeassistant/components/search/__init__.py)
