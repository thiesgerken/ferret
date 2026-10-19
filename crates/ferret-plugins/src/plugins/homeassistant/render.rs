use std::{collections::BTreeSet, fmt::Write};

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut};
use serde_json::{Value, json};

use super::{
    PluginError,
    inventory::{Area, Device, Entity, Relations, Snapshot, timestamp},
};

pub fn documents(
    snapshot: &Snapshot,
    source: &str,
    links: &reqwest::Url,
) -> Result<Vec<DocumentPut>, PluginError> {
    let mut documents = Vec::new();
    for device in snapshot.devices.values() {
        let mut text = format!("# {}\n\nDevice registry ID: {}\n", device.name(), device.id);
        if device
            .name
            .as_deref()
            .is_some_and(|name| name != device.name())
        {
            writeln!(
                text,
                "Original device name: {}",
                device.name.as_deref().expect("present name")
            )?;
        }
        fields(
            &mut text,
            &[
                ("Device type", device.entry_type.as_deref()),
                ("Manufacturer", device.manufacturer.as_deref()),
                ("Model", device.model.as_deref()),
                ("Model ID", device.model_id.as_deref()),
                ("Serial number", device.serial_number.as_deref()),
                ("Hardware version", device.hw_version.as_deref()),
                ("Firmware version", device.sw_version.as_deref()),
            ],
        );
        for (kind, value) in &device.connections {
            writeln!(text, "Connection {kind}: {value}")?;
        }
        for (kind, value) in &device.identifiers {
            writeln!(text, "Integration identifier {kind}: {value}")?;
        }
        labels(&mut text, snapshot, "Device", &device.labels);
        writeln!(
            text,
            "Explicit device area: {}",
            device.area_id.as_deref().unwrap_or("unknown")
        )?;
        placement(&mut text, snapshot, snapshot.device_area(&device.id)?);
        if let Some(parent) = device
            .parent_device_id
            .as_ref()
            .map(|id| &snapshot.devices[id])
        {
            writeln!(
                text,
                "\nLogical parent device: {} ({})",
                parent.name(),
                parent.id
            )?;
            parent_hardware(&mut text, parent);
        }
        if let Some(hub) = device
            .via_device_id
            .as_ref()
            .map(|id| &snapshot.devices[id])
        {
            writeln!(
                text,
                "Connected through: {} ({}). Hub placement is not inherited.",
                hub.name(),
                hub.id
            )?;
        }
        for child in snapshot
            .devices
            .values()
            .filter(|d| d.parent_device_id.as_deref() == Some(&device.id))
        {
            writeln!(
                text,
                "Logical child device: {} ({})",
                child.name(),
                child.id
            )?;
        }
        let attached: Vec<_> = snapshot
            .entities
            .values()
            .filter(|e| e.eligible() && e.device_id() == Some(&device.id))
            .collect();
        text.push_str("\n## Included entities\n");
        if attached.is_empty() {
            text.push_str("\nNo entities included under indexing rules.\n");
        }
        for entity in &attached {
            entity_section(&mut text, snapshot, entity)?;
        }
        let related = related_behavior(snapshot, "device", &device.id);
        behavior_links(&mut text, snapshot, &related);
        let effective = snapshot.device_area(&device.id)?;
        documents.push(document(format!("{source}/devices"), &device.id, device.name(), text,
            links.join(&format!("config/devices/device/{}", device.id))?.into(), Some(timestamp(device.created_at)?),
            json!({"kind":"device", "device":device, "placement":placement_meta(snapshot, effective),
                "entities": attached, "related_behavior":related})));
    }
    for area in snapshot.areas.values() {
        let mut text = format!("# {}\n\nArea registry ID: {}\n", area.name, area.area_id);
        area_context(&mut text, snapshot, area);
        text.push_str("\n## Placement directory\n");
        let mut count = 0;
        for device in snapshot.devices.values() {
            if snapshot
                .device_area(&device.id)?
                .is_some_and(|(a, _)| a.area_id == area.area_id)
            {
                writeln!(
                    text,
                    "\n### Device: {}\n\nDevice registry ID: {}",
                    device.name(),
                    device.id
                )?;
                let included: Vec<_> = snapshot
                    .entities
                    .values()
                    .filter(|e| e.eligible() && e.device_id() == Some(&device.id))
                    .collect();
                if included.is_empty() {
                    text.push_str("No entities included under indexing rules.\n");
                }
                count += 1;
            }
        }
        for entity in snapshot.entities.values().filter(|e| e.eligible()) {
            if snapshot
                .entity_area(entity)?
                .is_some_and(|(a, _)| a.area_id == area.area_id)
            {
                entity_section(&mut text, snapshot, entity)?;
                count += 1;
            }
        }
        if count == 0 {
            text.push_str("\nNo devices or entities included under indexing rules.\n");
        }
        let related = related_behavior(snapshot, "area", &area.area_id);
        behavior_links(&mut text, snapshot, &related);
        documents.push(document(format!("{source}/areas"), &area.area_id, &area.name, text,
            links.join(&format!("config/areas/area/{}", area.area_id))?.into(), Some(timestamp(area.created_at)?),
            json!({"kind":"area", "area":area, "floor":area.floor_id.as_ref().map(|id| &snapshot.floors[id]), "related_behavior":related})));
    }
    for entity in snapshot.entities.values().filter(|e| e.eligible()) {
        if !entity.dedicated() && entity.device_id().is_some() {
            continue;
        }
        let mut text = format!("# {}\n", entity.name());
        entity_section(&mut text, snapshot, entity)?;
        let (suffix, kind) = if entity.dedicated() {
            let config = &snapshot.definitions[&entity.entity_id];
            text.push_str("\n## Configured definition\n\nFaithful loaded configuration. Templates are expressions, not evaluated targets.\n\n");
            // JSON object keys use serde_json's ordered map; array order is configured behavior.
            let definition = serde_json::to_string_pretty(config)?;
            let fence = "`".repeat(longest_backtick_run(&definition).max(2) + 1);
            writeln!(text, "{fence}json\n{definition}\n{fence}")?;
            let relations =
                &snapshot.relations[&(entity.domain().to_owned(), entity.entity_id.clone())];
            named_relations(&mut text, snapshot, relations);
            if entity.domain() == "automation" {
                ("automations", "automation")
            } else {
                ("scripts", "script")
            }
        } else {
            ("standalone-entities", "entity")
        };
        let url = if entity.dedicated() {
            links.join(&format!(
                "config/{}/show/{}",
                entity.domain(),
                entity.entity_id
            ))?
        } else if entity.registry.is_some() {
            let mut url = links.join("config/entities")?;
            url.query_pairs_mut().append_pair("domain", entity.domain());
            url
        } else {
            // No entity permalink is exposed for ordinary runtime-only entities.
            links.clone()
        };
        documents.push(document(format!("{source}/{suffix}"), &entity.external_id(), entity.name(), text, url.into(), entity.created_at()?,
            json!({"kind":kind, "entity":entity, "placement":placement_meta(snapshot, snapshot.entity_area(entity)?),
                "recognized_relations":snapshot.relations.get(&(entity.domain().to_owned(), entity.entity_id.clone()))})));
    }
    Ok(documents)
}

fn document(
    source: String,
    id: &str,
    title: &str,
    text: String,
    url: String,
    created_at: Option<DateTime<Utc>>,
    meta: Value,
) -> DocumentPut {
    DocumentPut {
        source,
        external_id: id.to_owned(),
        content_type: ContentType::Markdown,
        title: Some(title.to_owned()),
        text,
        url: Some(url),
        thread: None,
        authors: Vec::new(),
        created_at,
        // Registries do not timestamp every definition, membership or recognized relation change.
        modified_at: None,
        meta: meta.as_object().expect("object metadata").clone(),
    }
}

fn fields(text: &mut String, fields: &[(&str, Option<&str>)]) {
    for (name, value) in fields {
        if let Some(value) = value {
            writeln!(text, "{name}: {value}").expect("string writes succeed");
        }
    }
}

fn labels(text: &mut String, snapshot: &Snapshot, scope: &str, ids: &BTreeSet<String>) {
    for id in ids {
        let label = &snapshot.labels[id];
        writeln!(text, "{scope} label: {} ({id})", label.name).expect("string writes succeed");
        if let Some(description) = &label.description {
            writeln!(text, "{scope} label description: {description}")
                .expect("string writes succeed");
        }
    }
}

fn area_context(text: &mut String, snapshot: &Snapshot, area: &Area) {
    writeln!(text, "Area: {} ({})", area.name, area.area_id).expect("string writes succeed");
    for alias in &area.aliases {
        writeln!(text, "Area alias: {alias}").expect("string writes succeed");
    }
    labels(text, snapshot, "Area", &area.labels);
    if let Some(floor) = area.floor_id.as_ref().map(|id| &snapshot.floors[id]) {
        writeln!(text, "Floor: {} ({})", floor.name, floor.floor_id)
            .expect("string writes succeed");
        if let Some(level) = floor.level {
            writeln!(text, "Floor level: {level}").expect("string writes succeed");
        }
        for alias in &floor.aliases {
            writeln!(text, "Floor alias: {alias}").expect("string writes succeed");
        }
    } else {
        text.push_str("Floor: unknown\n");
    }
}

fn placement(text: &mut String, snapshot: &Snapshot, effective: Option<(&Area, &str)>) {
    if let Some((area, provenance)) = effective {
        writeln!(text, "Effective placement from: {provenance}").expect("string writes succeed");
        area_context(text, snapshot, area);
    } else {
        text.push_str("Effective area and floor: unknown\n");
    }
}

fn placement_meta(snapshot: &Snapshot, effective: Option<(&Area, &str)>) -> Value {
    effective.map_or(Value::Null, |(area, provenance)| {
        json!({"area":area, "provenance":provenance,
        "floor":area.floor_id.as_ref().map(|id| &snapshot.floors[id]),
        "area_labels":area.labels.iter().map(|id| &snapshot.labels[id]).collect::<Vec<_>>()})
    })
}

fn parent_hardware(text: &mut String, parent: &Device) {
    fields(
        text,
        &[
            ("Parent manufacturer", parent.manufacturer.as_deref()),
            ("Parent model", parent.model.as_deref()),
            ("Parent model ID", parent.model_id.as_deref()),
            ("Parent serial number", parent.serial_number.as_deref()),
            ("Parent hardware version", parent.hw_version.as_deref()),
            ("Parent firmware version", parent.sw_version.as_deref()),
        ],
    );
    for (kind, value) in &parent.connections {
        writeln!(text, "Parent connection {kind}: {value}").expect("string writes succeed");
    }
}

fn entity_section(
    text: &mut String,
    snapshot: &Snapshot,
    entity: &Entity,
) -> Result<(), PluginError> {
    writeln!(
        text,
        "\n### Entity: {}\n\nExact entity ID: {}\nDomain: {}",
        entity.name(),
        entity.entity_id,
        entity.domain()
    )?;
    if let Some(device) = entity.device_id().map(|id| &snapshot.devices[id]) {
        writeln!(text, "Device: {} ({})", device.name(), device.id)?;
    }
    if let Some(registry) = &entity.registry {
        writeln!(
            text,
            "Entity registry ID: {}\nIntegration/platform: {}",
            registry.id, registry.platform
        )?;
        fields(
            text,
            &[
                (
                    "Original entity name",
                    registry
                        .original_name
                        .as_deref()
                        .filter(|name| *name != entity.name()),
                ),
                (
                    "Device class",
                    registry
                        .device_class
                        .as_deref()
                        .or(registry.original_device_class.as_deref()),
                ),
                ("Entity category", registry.entity_category.as_deref()),
            ],
        );
        for alias in registry.aliases.iter().flatten() {
            writeln!(text, "Entity alias: {alias}")?;
        }
        labels(text, snapshot, "Entity", &registry.labels);
        writeln!(
            text,
            "Explicit entity area: {}",
            registry.area_id.as_deref().unwrap_or("not assigned")
        )?;
    } else {
        text.push_str("Registry entry: none. Registry flags and assigned placement are unknown.\n");
    }
    fields(
        text,
        &[
            (
                "Static unit",
                entity.descriptive.unit_of_measurement.as_deref(),
            ),
            (
                "Descriptive device class",
                entity.descriptive.device_class.as_deref(),
            ),
        ],
    );
    placement(text, snapshot, snapshot.entity_area(entity)?);
    Ok(())
}

fn related_behavior(snapshot: &Snapshot, kind: &str, id: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Some(relations) = snapshot.relations.get(&(kind.to_owned(), id.to_owned())) {
        for key in ["automation", "script"] {
            ids.extend(relations.get(key).into_iter().flatten().cloned());
        }
    }
    // Keep recognized reverse links even when HA's inventory query does not walk that edge.
    for ((relation_kind, entity), relations) in &snapshot.relations {
        if matches!(relation_kind.as_str(), "automation" | "script")
            && relations
                .get(kind)
                .is_some_and(|values| values.contains(id))
        {
            ids.insert(entity.clone());
        }
    }
    ids.retain(|id| snapshot.entities[id].eligible());
    ids
}

fn behavior_links(text: &mut String, snapshot: &Snapshot, ids: &BTreeSet<String>) {
    text.push_str("\n## Related behavior recognized by Home Assistant\n\nNot exhaustive: dynamic templates may hide targets.\n");
    if ids.is_empty() {
        text.push_str("\nNo included automations or scripts recognized.\n");
    }
    for id in ids {
        writeln!(text, "- {} ({id})", snapshot.entities[id].name()).expect("string writes succeed");
    }
}

fn named_relations(text: &mut String, snapshot: &Snapshot, relations: &Relations) {
    text.push_str("\n## Related resources recognized by Home Assistant\n\nNot exhaustive: dynamic templates may hide targets. Inventory exclusions do not redact configured references.\n");
    for (kind, ids) in relations {
        for id in ids {
            let name = match kind.as_str() {
                "device" => Some(snapshot.devices[id].name()),
                "area" => Some(snapshot.areas[id].name.as_str()),
                "floor" => Some(snapshot.floors[id].name.as_str()),
                "label" => Some(snapshot.labels[id].name.as_str()),
                "entity" | "automation" | "script" => Some(snapshot.entities[id].name()),
                _ => None,
            };
            if let Some(name) = name {
                writeln!(text, "- {kind}: {name} ({id})").expect("string writes succeed");
            }
        }
    }
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0)
}
