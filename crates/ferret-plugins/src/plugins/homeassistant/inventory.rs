use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{PluginError, protocol::Session};

#[derive(Deserialize, Serialize)]
pub struct Device {
    pub id: String,
    pub name: Option<String>,
    pub name_by_user: Option<String>,
    pub area_id: Option<String>,
    pub parent_device_id: Option<String>,
    pub via_device_id: Option<String>,
    pub entry_type: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub model_id: Option<String>,
    pub serial_number: Option<String>,
    pub hw_version: Option<String>,
    pub sw_version: Option<String>,
    #[serde(default)]
    pub connections: BTreeSet<(String, String)>,
    pub identifiers: BTreeSet<(String, String)>,
    pub labels: BTreeSet<String>,
    #[serde(deserialize_with = "registry_date")]
    pub created_at: Option<DateTime<Utc>>,
}

impl Device {
    pub fn name(&self) -> &str {
        self.name_by_user
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or(&self.id)
    }
}

#[derive(Deserialize, Serialize)]
pub struct Area {
    pub area_id: String,
    pub name: String,
    pub aliases: BTreeSet<String>,
    pub floor_id: Option<String>,
    pub labels: BTreeSet<String>,
    #[serde(deserialize_with = "registry_date")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Serialize)]
pub struct Floor {
    pub floor_id: String,
    pub name: String,
    pub aliases: BTreeSet<String>,
    pub level: Option<i32>,
}

#[derive(Deserialize, Serialize)]
pub struct Label {
    pub label_id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct RegistryEntity {
    pub id: String,
    pub entity_id: String,
    pub name: Option<String>,
    pub original_name: Option<String>,
    pub platform: String,
    pub device_id: Option<String>,
    pub area_id: Option<String>,
    pub labels: BTreeSet<String>,
    pub aliases: Vec<Option<String>>,
    pub device_class: Option<String>,
    pub original_device_class: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub disabled_by: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub hidden_by: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub entity_category: Option<String>,
    #[serde(deserialize_with = "registry_date")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct ListedEntity {
    id: String,
    entity_id: String,
}

#[derive(Deserialize)]
pub struct RuntimeEntity {
    pub entity_id: String,
    pub attributes: DescriptiveAttributes,
}

// Only descriptive attributes cross the runtime inventory boundary, never arbitrary state data.
#[derive(Default, Deserialize, Serialize)]
pub struct DescriptiveAttributes {
    pub friendly_name: Option<String>,
    pub device_class: Option<String>,
    pub unit_of_measurement: Option<String>,
}

#[derive(Serialize)]
pub struct Entity {
    pub entity_id: String,
    pub registry: Option<RegistryEntity>,
    pub descriptive: DescriptiveAttributes,
}

impl Entity {
    pub fn eligible(&self) -> bool {
        self.registry.as_ref().is_none_or(|r| {
            r.hidden_by.is_none()
                && r.disabled_by.is_none()
                && r.entity_category.as_deref() != Some("diagnostic")
        })
    }
    pub fn domain(&self) -> &str {
        self.entity_id
            .split_once('.')
            .expect("validated entity ID")
            .0
    }
    pub fn dedicated(&self) -> bool {
        matches!(self.domain(), "automation" | "script")
    }
    pub fn device_id(&self) -> Option<&str> {
        self.registry.as_ref().and_then(|r| r.device_id.as_deref())
    }
    pub fn name(&self) -> &str {
        self.registry
            .as_ref()
            .and_then(|r| r.name.as_deref().or(r.original_name.as_deref()))
            .or(self.descriptive.friendly_name.as_deref())
            .unwrap_or(&self.entity_id)
    }
    pub fn external_id(&self) -> String {
        self.registry.as_ref().map_or_else(
            || format!("runtime:{}", self.entity_id),
            |r| format!("registry:{}", r.id),
        )
    }
    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        self.registry.as_ref().and_then(|r| r.created_at)
    }
}

pub type Relations = BTreeMap<String, BTreeSet<String>>;

pub struct Snapshot {
    pub devices: BTreeMap<String, Device>,
    pub areas: BTreeMap<String, Area>,
    pub floors: BTreeMap<String, Floor>,
    pub labels: BTreeMap<String, Label>,
    pub entities: BTreeMap<String, Entity>,
    pub definitions: BTreeMap<String, Value>,
    pub relations: BTreeMap<(String, String), Relations>,
}

fn registry_date<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error> {
    timestamp(f64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

pub fn timestamp(seconds: f64) -> Result<Option<DateTime<Utc>>, PluginError> {
    if !seconds.is_finite()
        || seconds < i64::MIN as f64 / 1000.0
        || seconds > i64::MAX as f64 / 1000.0
    {
        return Err("Home Assistant returned an invalid registry timestamp".into());
    }
    // Older registry migrations use zero when the origin has no historical date.
    if seconds == 0.0 {
        return Ok(None);
    }
    DateTime::from_timestamp_millis((seconds * 1000.0).round() as i64)
        .map(Some)
        .ok_or_else(|| "Home Assistant returned an out-of-range registry timestamp".into())
}

fn index<T>(items: Vec<T>, key: impl Fn(&T) -> &str) -> Result<BTreeMap<String, T>, PluginError> {
    let mut result = BTreeMap::new();
    for item in items {
        let id = key(&item).to_owned();
        if id.is_empty() || result.insert(id, item).is_some() {
            return Err("Home Assistant returned empty or duplicate inventory identities".into());
        }
    }
    Ok(result)
}

impl Snapshot {
    pub async fn load(session: &mut Session) -> Result<Self, PluginError> {
        let devices = index(
            session
                .command("config/device_registry/list", json!({}))
                .await?,
            |d: &Device| &d.id,
        )?;
        let areas = index(
            session
                .command("config/area_registry/list", json!({}))
                .await?,
            |a: &Area| &a.area_id,
        )?;
        let floors = index(
            session
                .command("config/floor_registry/list", json!({}))
                .await?,
            |f: &Floor| &f.floor_id,
        )?;
        let labels = index(
            session
                .command("config/label_registry/list", json!({}))
                .await?,
            |l: &Label| &l.label_id,
        )?;
        let listed: Vec<ListedEntity> = session
            .command("config/entity_registry/list", json!({}))
            .await?;
        let listed = index(listed, |e| &e.entity_id)?;
        let mut registry_ids = BTreeSet::new();
        let ids: Vec<_> = listed.keys().collect();
        let mut entities = BTreeMap::new();
        for batch in ids.chunks(100) {
            let mut details: BTreeMap<String, Option<RegistryEntity>> = session
                .command(
                    "config/entity_registry/get_entries",
                    json!({"entity_ids": batch}),
                )
                .await?;
            if details.len() != batch.len() {
                return Err("Home Assistant returned incomplete entity details".into());
            }
            for id in batch {
                let mut registry = details
                    .remove(*id)
                    .flatten()
                    .ok_or("Home Assistant omitted required entity details")?;
                if registry.entity_id != **id
                    || registry.id != listed[*id].id
                    || !registry_ids.insert(registry.id.clone())
                {
                    return Err("Home Assistant entity registry changed during enumeration".into());
                }
                registry.aliases.sort();
                registry.aliases.dedup();
                entities.insert(
                    (*id).clone(),
                    Entity {
                        entity_id: (*id).clone(),
                        registry: Some(registry),
                        descriptive: DescriptiveAttributes::default(),
                    },
                );
            }
        }
        let runtime: Vec<RuntimeEntity> = session.command("get_states", json!({})).await?;
        for (id, runtime) in index(runtime, |e| &e.entity_id)? {
            if let Some(entity) = entities.get_mut(&id) {
                entity.descriptive = runtime.attributes;
            } else {
                entities.insert(
                    id.clone(),
                    Entity {
                        entity_id: id,
                        registry: None,
                        descriptive: runtime.attributes,
                    },
                );
            }
        }
        let mut snapshot = Self {
            devices,
            areas,
            floors,
            labels,
            entities,
            definitions: BTreeMap::new(),
            relations: BTreeMap::new(),
        };
        snapshot.validate()?;
        for entity in snapshot
            .entities
            .values()
            .filter(|e| e.eligible() && e.dedicated())
        {
            let mut result: Value = session
                .command(
                    &format!("{}/config", entity.domain()),
                    json!({"entity_id": entity.entity_id}),
                )
                .await?;
            let config = result
                .get_mut("config")
                .filter(|v| v.is_object())
                .map(Value::take)
                .ok_or("Home Assistant omitted a required automation/script definition")?;
            let required = if entity.domain() == "script" {
                "sequence"
            } else {
                "actions"
            };
            // Loaded raw configurations preserve their input spelling (singular keys are valid).
            let actions = config.get(required).or_else(|| {
                if required == "actions" {
                    config.get("action")
                } else {
                    None
                }
            });
            let sequence_shape =
                |value: &Value| value.is_null() || value.is_array() || value.is_object();
            if !actions.is_some_and(sequence_shape)
                || (entity.domain() == "automation"
                    && !config
                        .get("triggers")
                        .or_else(|| config.get("trigger"))
                        .is_some_and(sequence_shape))
            {
                return Err(
                    "Home Assistant returned a malformed required automation/script definition"
                        .into(),
                );
            }
            snapshot
                .definitions
                .insert(entity.entity_id.clone(), config);
        }
        let queries: Vec<_> = snapshot
            .devices
            .keys()
            .map(|id| ("device".to_owned(), id.clone()))
            .chain(
                snapshot
                    .areas
                    .keys()
                    .map(|id| ("area".to_owned(), id.clone())),
            )
            .chain(
                snapshot
                    .entities
                    .values()
                    .filter(|e| e.eligible() && e.dedicated())
                    .map(|e| (e.domain().to_owned(), e.entity_id.clone())),
            )
            .collect();
        for (kind, id) in queries {
            let relations: Relations = session
                .command(
                    "search/related",
                    json!({"item_type": kind, "item_id": id, "include_disabled_entities": true}),
                )
                .await?;
            snapshot.relations.insert((kind, id), relations);
        }
        Ok(snapshot)
    }

    fn validate_labels(&self, labels: &BTreeSet<String>) -> Result<(), PluginError> {
        if labels.iter().any(|id| !self.labels.contains_key(id)) {
            return Err("Home Assistant returned an unresolved label reference".into());
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), PluginError> {
        for device in self.devices.values() {
            self.validate_labels(&device.labels)?;
            self.device_area(&device.id)?;
            for id in [&device.parent_device_id, &device.via_device_id]
                .into_iter()
                .flatten()
            {
                if !self.devices.contains_key(id) {
                    return Err("Home Assistant returned an unresolved device reference".into());
                }
            }
        }
        for area in self.areas.values() {
            self.validate_labels(&area.labels)?;
            if area
                .floor_id
                .as_ref()
                .is_some_and(|id| !self.floors.contains_key(id))
            {
                return Err("Home Assistant returned an unresolved floor reference".into());
            }
        }
        for entity in self.entities.values() {
            if !entity
                .entity_id
                .split_once('.')
                .is_some_and(|(d, id)| !d.is_empty() && !id.is_empty())
            {
                return Err("Home Assistant returned an invalid entity ID".into());
            }
            if let Some(registry) = &entity.registry {
                self.validate_labels(&registry.labels)?;
                if registry
                    .device_id
                    .as_ref()
                    .is_some_and(|id| !self.devices.contains_key(id))
                {
                    return Err(
                        "Home Assistant returned an unresolved entity device reference".into(),
                    );
                }
            }
            self.entity_area(entity)?;
        }
        Ok(())
    }

    pub fn device_area<'a>(&'a self, id: &str) -> Result<Option<(&'a Area, &'a str)>, PluginError> {
        let mut seen = BTreeSet::new();
        let mut device = self
            .devices
            .get(id)
            .ok_or("Home Assistant returned an unresolved device")?;
        loop {
            if !seen.insert(&device.id) {
                return Err("Home Assistant returned a parent-device cycle".into());
            }
            if let Some(area) = &device.area_id {
                return Ok(Some((
                    self.areas
                        .get(area)
                        .ok_or("Home Assistant returned an unresolved area")?,
                    &device.id,
                )));
            }
            match &device.parent_device_id {
                Some(parent) => {
                    device = self
                        .devices
                        .get(parent)
                        .ok_or("Home Assistant returned an unresolved parent device")?
                }
                None => return Ok(None),
            }
        }
    }

    pub fn entity_area<'a>(
        &'a self,
        entity: &'a Entity,
    ) -> Result<Option<(&'a Area, &'a str)>, PluginError> {
        if let Some(registry) = &entity.registry {
            if let Some(id) = &registry.area_id {
                return Ok(Some((
                    self.areas
                        .get(id)
                        .ok_or("Home Assistant returned an unresolved entity area")?,
                    "entity override",
                )));
            }
            if let Some(id) = &registry.device_id {
                return self.device_area(id);
            }
        }
        Ok(None)
    }
}
