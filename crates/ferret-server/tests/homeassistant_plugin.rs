mod common;

use common::{TestServer, start_server};
use ferret_api::{Document, SearchQuery};
use ferret_plugins::{
    plugins::{self, Log, PluginOptions, Run},
    worker::Worker,
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

const TOKEN: &str = "homeassistant-test-secret-never-log";

struct Origin {
    devices: Vec<Value>,
    areas: Vec<Value>,
    floors: Vec<Value>,
    labels: Vec<Value>,
    entities: Vec<Value>,
    runtime: Vec<Value>,
    definitions: BTreeMap<String, Value>,
    relations: BTreeMap<(String, String), Value>,
    failure: Option<&'static str>,
    command_error: Option<(&'static str, String)>,
}
type SharedOrigin = Arc<Mutex<Origin>>;

fn device(
    id: &str,
    name: &str,
    area: Option<&str>,
    parent: Option<&str>,
    hub: Option<&str>,
) -> Value {
    json!({"id":id,"name":name,"name_by_user":null,"area_id":area,"parent_device_id":parent,"via_device_id":hub,
        "identifiers":[["fixture",id]],"connections":[],"labels":[],"created_at":1735689600.0})
}
fn entity(id: &str, entity_id: &str, device: Option<&str>, area: Option<&str>) -> Value {
    json!({"id":id,"entity_id":entity_id,"device_id":device,"area_id":area,"name":id,"original_name":null,
        "platform":"fixture","labels":[],"aliases":[],"device_class":null,"original_device_class":null,
        "hidden_by":null,"disabled_by":null,"entity_category":null,"created_at":1735689600.0})
}
fn runtime(id: &str, name: &str) -> Value {
    json!({"entity_id":id,"state":"off","last_changed":"2026-01-01T00:00:00Z","attributes":{
        "friendly_name":name,"unit_of_measurement":"widgets","secret_runtime":"Neverindexlivesecret","battery_level":12}})
}
fn origin() -> SharedOrigin {
    let mut parent = device("parent", "Readingstation", Some("study"), None, Some("hub"));
    parent["manufacturer"] = json!("Example Works");
    parent["model"] = json!("Cobaltmodel42");
    parent["model_id"] = json!("COBALT42");
    parent["serial_number"] = json!("COBALTSERIAL731");
    parent["connections"] = json!([["mac", "02:00:00:12:34:56"]]);
    parent["labels"] = json!(["reading"]);
    let mut entities = vec![
        entity("lamp-registry", "light.reading_lamp", Some("parent"), None),
        entity(
            "override-registry",
            "sensor.room_override",
            Some("parent"),
            Some("hall"),
        ),
        entity(
            "child-registry",
            "switch.child_channel",
            Some("child"),
            None,
        ),
        entity(
            "helper-registry",
            "input_boolean.helper",
            None,
            Some("study"),
        ),
        entity(
            "hidden-registry",
            "switch.hidden_fixture",
            Some("parent"),
            None,
        ),
        entity(
            "disabled-registry",
            "switch.disabled_fixture",
            Some("parent"),
            None,
        ),
        entity(
            "diagnostic-registry",
            "sensor.diagnostic_fixture",
            Some("empty"),
            None,
        ),
        entity(
            "config-registry",
            "switch.configuration_fixture",
            Some("parent"),
            None,
        ),
        entity(
            "automation-registry",
            "automation.evening_reading",
            None,
            None,
        ),
        entity("script-registry", "script.reading_setup", None, None),
    ];
    entities[0]["aliases"] = json!(["Booklampalias", null]);
    entities[4]["hidden_by"] = json!("user");
    entities[5]["disabled_by"] = json!("user");
    entities[6]["entity_category"] = json!("diagnostic");
    entities[7]["entity_category"] = json!("config");
    let runtime = vec![
        runtime("sensor.yaml_inventory", "YAMLinventorysentinel"),
        runtime("automation.evening_reading", "Eveningbehavior"),
        runtime("script.reading_setup", "Readingscript"),
    ];
    let definitions = BTreeMap::from([
        (
            "automation.evening_reading".into(),
            json!({"alias":"Eveningbehavior","description":"Eveningdescription","triggers":[{"trigger":"numeric_state","entity_id":"sensor.room_override","above":17}],"conditions":[],"actions":[{"action":"switch.turn_on","target":{"entity_id":["switch.hidden_fixture","switch.disabled_fixture"]}},{"action":"script.reading_setup"}],"variables":{"target":"{{ 'light.' ~ room }}","payload":"Faithfulpayloadsentinel"}}),
        ),
        (
            "script.reading_setup".into(),
            json!({"alias":"Readingscript","fields":{"greeting":{"description":"Scriptinputsentinel"}},"sequence":[{"action":"light.turn_on","target":{"entity_id":"light.reading_lamp"}},{"choose":[{"conditions":"{{ greeting is defined }}","sequence":[{"delay":1}]}]}]}),
        ),
    ]);
    let relations = BTreeMap::from([
        (
            ("device".into(), "parent".into()),
            json!({"automation":["automation.evening_reading"],"script":["script.reading_setup"]}),
        ),
        (
            ("area".into(), "study".into()),
            json!({"automation":["automation.evening_reading"],"script":["script.reading_setup"]}),
        ),
        (
            ("automation".into(), "automation.evening_reading".into()),
            json!({"device":["parent"],"entity":["light.reading_lamp","switch.hidden_fixture"],"area":["study"],"script":["script.reading_setup"]}),
        ),
        (
            ("script".into(), "script.reading_setup".into()),
            json!({"device":["parent"],"entity":["light.reading_lamp"],"area":["study"]}),
        ),
    ]);
    Arc::new(Mutex::new(Origin {
        devices: vec![
            parent,
            device("child", "Logicalchannel", None, Some("parent"), None),
            device("hub", "Remotehub", Some("hall"), None, None),
            device("empty", "Filteredonlydevice", None, None, None),
        ],
        areas: vec![
            json!({"area_id":"study","name":"Studyroom","aliases":["Booknookalias"],"floor_id":"upstairs","labels":["reading"],"created_at":1735689600.0}),
            json!({"area_id":"hall","name":"Hallroom","aliases":[],"floor_id":null,"labels":[],"created_at":1735689600.0}),
        ],
        floors: vec![
            json!({"floor_id":"upstairs","name":"Upperfloor","aliases":["Atticalias"],"level":1}),
        ],
        labels: vec![
            json!({"label_id":"reading","name":"Readinglabel","description":"Quietreadingdescription"}),
        ],
        entities,
        runtime,
        definitions,
        relations,
        failure: None,
        command_error: None,
    }))
}
async fn serve(origin: SharedOrigin) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let origin = origin.clone();
            tokio::spawn(async move {
                let mut ws = accept_async(stream).await.unwrap();
                ws.send(Message::Text(
                    json!({"type":"auth_required","ha_version":"2026.10.2"})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
                let auth: Value =
                    serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap())
                        .unwrap();
                assert_eq!(auth["access_token"], TOKEN);
                if origin.lock().failure == Some("auth") {
                    let _ = ws
                        .send(Message::Text(
                            json!({"type":"auth_invalid","message":TOKEN})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
                ws.send(Message::Text(json!({"type":"auth_ok"}).to_string().into()))
                    .await
                    .unwrap();
                while let Some(Ok(message)) = ws.next().await {
                    if message.is_close() {
                        return;
                    }
                    let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                    let kind = request["type"].as_str().unwrap();
                    let command_error = {
                        let origin = origin.lock();
                        origin
                            .command_error
                            .as_ref()
                            .filter(|(command, _)| *command == kind)
                            .map(|(_, code)| code.clone())
                            .or_else(|| {
                                let behavior = match kind {
                                    "automation/config" | "script/config" => {
                                        request["entity_id"].as_str()
                                    }
                                    "search/related"
                                        if matches!(
                                            request["item_type"].as_str(),
                                            Some("automation" | "script")
                                        ) =>
                                    {
                                        request["item_id"].as_str()
                                    }
                                    _ => None,
                                };
                                behavior
                                    .filter(|id| !origin.definitions.contains_key(*id))
                                    .map(|_| "not_found".to_owned())
                            })
                    };
                    if let Some(code) = command_error {
                        if ws.send(Message::Text(json!({"id":request["id"],"type":"result","success":false,
                            "error":{"code":code,"message":format!("{TOKEN}: private origin failure")}}).to_string().into())).await.is_err() { return; }
                        continue;
                    }
                    let result = {
                        let origin = origin.lock();
                        if origin.failure == Some(kind) {
                            None
                        } else {
                            Some(match kind {
                                "config/device_registry/list" => json!(origin.devices),
                                "config/area_registry/list" => json!(origin.areas),
                                "config/floor_registry/list" => json!(origin.floors),
                                "config/label_registry/list" => json!(origin.labels),
                                "config/entity_registry/list" => json!(origin.entities),
                                "config/entity_registry/get_entries" => request["entity_ids"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|id| {
                                        (
                                            id.as_str().unwrap().to_owned(),
                                            origin
                                                .entities
                                                .iter()
                                                .find(|e| e["entity_id"] == *id)
                                                .cloned()
                                                .unwrap_or(Value::Null),
                                        )
                                    })
                                    .collect::<serde_json::Map<_, _>>()
                                    .into(),
                                "get_states" => json!(origin.runtime),
                                "automation/config" | "script/config" => {
                                    json!({"config":origin.definitions.get(request["entity_id"].as_str().unwrap())})
                                }
                                "search/related" => origin
                                    .relations
                                    .get(&(
                                        request["item_type"].as_str().unwrap().into(),
                                        request["item_id"].as_str().unwrap().into(),
                                    ))
                                    .cloned()
                                    .unwrap_or(json!({})),
                                _ => panic!("unexpected command {kind}"),
                            })
                        }
                    };
                    let Some(result) = result else {
                        return;
                    };
                    if ws.send(Message::Text(json!({"id":request["id"],"type":"result","success":true,"result":result}).to_string().into())).await.is_err() { return; }
                }
            });
        }
    });
    url
}
async fn run(server: &TestServer, url: &str) -> Run {
    Run {
        api: server.admin_api().await,
        source: "homeassistant/home".into(),
        config: json!({"url":url,"public_url":"https://ha.example.test","token":TOKEN}),
        state: json!({}),
        log: Log::default(),
        options: PluginOptions::default(),
    }
}
async fn documents(server: &TestServer, pool: &PgPool) -> Vec<Document> {
    let api = server.admin_api().await;
    let ids: Vec<uuid::Uuid> =
        sqlx::query_scalar("select id from documents order by source, external_id")
            .fetch_all(pool)
            .await
            .unwrap();
    let mut result = Vec::new();
    for id in ids {
        result.push(api.document(id).await.unwrap());
    }
    result
}
async fn finds(server: &TestServer, q: &str) -> Vec<ferret_api::Hit> {
    server
        .admin_api()
        .await
        .search(&SearchQuery {
            q: q.into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .hits
}

#[sqlx::test(migrations = false)]
async fn inventory_behavior_relationships_and_reconciliation_are_searchable(pool: PgPool) {
    let origin = origin();
    let url = serve(origin.clone()).await;
    let server = start_server(pool.clone()).await;
    let run = run(&server, &url).await;
    let first = plugins::run("homeassistant", &run).await.unwrap();
    assert_eq!(first.stats.put, 10);
    let docs = documents(&server, &pool).await;
    let parent = docs.iter().find(|d| d.external_id == "parent").unwrap();
    assert!(parent.text.contains("Exact entity ID: light.reading_lamp"));
    assert!(
        parent.text.contains("Booklampalias") && parent.text.contains("Quietreadingdescription")
    );
    assert!(parent.text.contains("switch.configuration_fixture"));
    assert!(
        !parent.text.contains("switch.hidden_fixture")
            && !parent.text.contains("switch.disabled_fixture")
    );
    assert!(
        parent.text.contains("Related behavior")
            && parent.text.contains("automation.evening_reading")
    );
    let child = docs.iter().find(|d| d.external_id == "child").unwrap();
    assert!(
        child.text.contains("Studyroom")
            && child.text.contains("Parent serial number: COBALTSERIAL731")
    );
    assert!(!child.text.contains("Hallroom"));
    let area = docs.iter().find(|d| d.external_id == "study").unwrap();
    assert!(!area.text.contains("sensor.room_override"));
    assert!(
        docs.iter()
            .find(|d| d.external_id == "empty")
            .unwrap()
            .text
            .contains("No entities included under indexing rules")
    );
    let yaml = docs
        .iter()
        .find(|d| d.external_id == "runtime:sensor.yaml_inventory")
        .unwrap();
    assert!(yaml.created_at.is_none() && yaml.modified_at.is_none());
    assert_eq!(yaml.url.as_deref(), Some("https://ha.example.test/"));
    for doc in &docs {
        assert!(!doc.text.contains("Neverindexlivesecret") && !doc.text.contains("last_changed"));
        assert!(doc.thread.is_none());
    }
    let automation = docs
        .iter()
        .find(|d| d.source.ends_with("/automations"))
        .unwrap();
    assert_eq!(
        automation.url.as_deref(),
        Some("https://ha.example.test/config/automation/show/automation.evening_reading")
    );
    for text in [
        "Faithfulpayloadsentinel",
        "{{ 'light.' ~ room }}",
        "switch.hidden_fixture",
        "switch.disabled_fixture",
        "17",
        "Studyroom",
        "Readingstation",
    ] {
        assert!(automation.text.contains(text), "{text}");
    }
    assert!(
        docs.iter()
            .find(|d| d.source.ends_with("/scripts"))
            .unwrap()
            .text
            .contains("Scriptinputsentinel")
    );
    for q in [
        "COBALTSERIAL731",
        "Cobaltmodel42",
        "light.reading_lamp",
        "Booknookalias",
        "Atticalias",
    ] {
        assert!(!finds(&server, q).await.is_empty(), "{q}");
    }
    assert_eq!(
        plugins::run("homeassistant", &run)
            .await
            .unwrap()
            .stats
            .unchanged,
        10
    );
    let helper_id = docs
        .iter()
        .find(|d| d.external_id == "registry:helper-registry")
        .unwrap()
        .id;
    {
        let mut o = origin.lock();
        o.entities[3]["entity_id"] = json!("input_boolean.renamed_helper");
        o.labels[0]["description"] = json!("Editedlabeldescription");
        o.floors[0]["name"] = json!("Editedfloorname");
        o.definitions.get_mut("script.reading_setup").unwrap()["description"] =
            json!("Editedscriptdefinition");
    }
    plugins::run("homeassistant", &run).await.unwrap();
    let renamed = run.api.document(helper_id).await.unwrap();
    assert!(renamed.text.contains("input_boolean.renamed_helper"));
    assert_eq!(
        renamed.url.as_deref(),
        Some("https://ha.example.test/config/entities?domain=input_boolean")
    );
    assert!(
        run.api
            .document(parent.id)
            .await
            .unwrap()
            .text
            .contains("Editedfloorname")
    );
    {
        let mut o = origin.lock();
        o.entities[3]["device_id"] = json!("parent");
        o.runtime
            .retain(|e| e["entity_id"] != "sensor.yaml_inventory");
    }
    let changed = plugins::run("homeassistant", &run).await.unwrap();
    assert_eq!(changed.stats.deleted, 2);
    assert!(finds(&server, "YAMLinventorysentinel").await.is_empty());
    assert!(
        run.api
            .document(parent.id)
            .await
            .unwrap()
            .text
            .contains("input_boolean.renamed_helper")
    );
}

#[sqlx::test(migrations = false)]
async fn failed_required_reads_never_sweep_previous_inventory(pool: PgPool) {
    let origin = origin();
    let url = serve(origin.clone()).await;
    let server = start_server(pool.clone()).await;
    let run = run(&server, &url).await;
    plugins::run("homeassistant", &run).await.unwrap();
    for failure in [
        "auth",
        "config/entity_registry/get_entries",
        "get_states",
        "automation/config",
        "script/config",
        "search/related",
    ] {
        origin.lock().failure = Some(failure);
        let error = plugins::run("homeassistant", &run)
            .await
            .err()
            .expect("required read must fail")
            .to_string();
        assert!(!error.contains(TOKEN));
        assert_eq!(documents(&server, &pool).await.len(), 10);
    }
    origin.lock().failure = None;
    origin.lock().definitions.remove("script.reading_setup");
    assert!(plugins::run("homeassistant", &run).await.is_err());
    assert_eq!(documents(&server, &pool).await.len(), 10);
    origin.lock().entities[0]["area_id"] = json!("missing-area");
    assert!(plugins::run("homeassistant", &run).await.is_err());
    assert_eq!(documents(&server, &pool).await.len(), 10);
}

#[sqlx::test(migrations = false)]
async fn worker_connection_encrypts_admin_token_and_indexes_all_source_types(pool: PgPool) {
    let url = serve(origin()).await;
    let server = start_server(pool.clone()).await;
    let worker = Worker {
        internal: server.internal(),
        public_url: server.public_url(),
        name: "ha-test-worker".into(),
        options: PluginOptions::default(),
    };
    let id = worker.register().await;
    let cookie = server.sign_in().await;
    let created=server.graphql(&cookie,"mutation ($input: NewConnection!) { createConnection(input:$input) { id config } }",json!({"input":{"name":"Home Assistant","icon":"house","plugin":"homeassistant","source":"homeassistant/home","config":json!({"url":url,"token":TOKEN}).to_string(),"schedule":"0 * * * *"}})).await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    assert!(!created.to_string().contains(TOKEN));
    let stored: (Value, Vec<u8>) = sqlx::query_as("select config,credentials from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(stored.0.get("token").is_none() && !stored.1.is_empty());
    assert!(
        !stored
            .1
            .windows(TOKEN.len())
            .any(|window| window == TOKEN.as_bytes())
    );
    assert!(worker.work_once(id, Duration::ZERO).await.unwrap());
    let sources: Vec<String> =
        sqlx::query_scalar("select distinct source from documents order by source")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        sources,
        [
            "areas",
            "automations",
            "devices",
            "scripts",
            "standalone-entities"
        ]
        .map(|suffix| format!("homeassistant/home/{suffix}"))
    );
}

#[sqlx::test(migrations = false)]
async fn stale_recognized_targets_remain_searchable_without_invented_inventory(pool: PgPool) {
    let origin = origin();
    {
        let mut o = origin.lock();
        o.definitions.get_mut("automation.evening_reading").unwrap()["actions"] = json!([{"action":"light.turn_on","target":{"entity_id":"light.deleted_fixture","device_id":"deleted-device","area_id":"deleted-area","floor_id":"deleted-floor","label_id":"deleted-label"}}]);
        o.relations.insert(("automation".into(), "automation.evening_reading".into()),
            json!({"entity":["light.deleted_fixture"],"device":["deleted-device"],"area":["deleted-area"],"floor":["deleted-floor"],"label":["deleted-label"],"automation":["automation.deleted_fixture"],"script":["script.deleted_fixture"]}));
        for kind in ["device", "area"] {
            let id = if kind == "device" { "parent" } else { "study" };
            o.relations.insert((kind.into(), id.into()),
                json!({"automation":["automation.deleted_fixture"],"script":["script.deleted_fixture"]}));
        }
    }
    let url = serve(origin).await;
    let server = start_server(pool.clone()).await;
    let run = run(&server, &url).await;
    assert_eq!(
        plugins::run("homeassistant", &run).await.unwrap().stats.put,
        10
    );
    let docs = documents(&server, &pool).await;
    let automation = docs
        .iter()
        .find(|d| d.source.ends_with("/automations"))
        .unwrap();
    for id in [
        "light.deleted_fixture",
        "deleted-device",
        "deleted-area",
        "deleted-floor",
        "deleted-label",
        "automation.deleted_fixture",
        "script.deleted_fixture",
    ] {
        assert!(
            automation
                .text
                .contains(&format!("unresolved in current inventory ({id})")),
            "{id}"
        );
    }
    assert_eq!(
        automation.meta["recognized_relations"]["entity"],
        json!(["light.deleted_fixture"])
    );
    for id in ["parent", "study"] {
        let inventory = docs.iter().find(|d| d.external_id == id).unwrap();
        assert!(
            inventory
                .text
                .contains("unresolved in current inventory (automation.deleted_fixture)")
        );
        assert!(
            inventory
                .text
                .contains("unresolved in current inventory (script.deleted_fixture)")
        );
    }
    assert!(!finds(&server, "light.deleted_fixture").await.is_empty());
    assert_eq!(docs.len(), 10);
    assert_eq!(
        plugins::run("homeassistant", &run)
            .await
            .unwrap()
            .stats
            .unchanged,
        10
    );
}

#[sqlx::test(migrations = false)]
async fn migrated_unknown_registry_dates_are_null_and_do_not_match_epoch_filters(pool: PgPool) {
    let origin = origin();
    {
        let mut o = origin.lock();
        o.devices[0]["created_at"] = json!(0.0);
        o.areas[0]["created_at"] = json!(0.0);
        o.entities[3]["created_at"] = json!(0.0);
    }
    let url = serve(origin).await;
    let server = start_server(pool.clone()).await;
    let run = run(&server, &url).await;
    plugins::run("homeassistant", &run).await.unwrap();
    let docs = documents(&server, &pool).await;
    for external_id in ["parent", "study", "registry:helper-registry"] {
        let doc = docs.iter().find(|d| d.external_id == external_id).unwrap();
        assert!(
            doc.created_at.is_none(),
            "{external_id}: {:?}",
            doc.created_at
        );
        assert!(doc.modified_at.is_none());
        let metadata_date = match external_id {
            "parent" => &doc.meta["device"]["created_at"],
            "study" => &doc.meta["area"]["created_at"],
            _ => &doc.meta["entity"]["registry"]["created_at"],
        };
        assert_eq!(metadata_date, &Value::Null);
        let hits = run
            .api
            .search(&SearchQuery {
                q: doc.title.clone().unwrap(),
                sources: vec![doc.source.clone()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            hits.hits
                .iter()
                .any(|hit| hit.document_id == doc.id && hit.created_at.is_none())
        );
        let filtered = run
            .api
            .search(&SearchQuery {
                q: doc.title.clone().unwrap(),
                sources: vec![doc.source.clone()],
                from: Some("1969-01-01T00:00:00Z".parse().unwrap()),
                to: Some("1971-01-01T00:00:00Z".parse().unwrap()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(filtered.hits.is_empty(), "{external_id}");
    }
    assert!(
        docs.iter()
            .find(|d| d.external_id == "child")
            .unwrap()
            .created_at
            .is_some()
    );
    assert_eq!(
        plugins::run("homeassistant", &run)
            .await
            .unwrap()
            .stats
            .unchanged,
        10
    );
}

#[sqlx::test(migrations = false)]
async fn null_empty_sequences_are_ingested_faithfully_and_reruns_are_unchanged(pool: PgPool) {
    let origin = origin();
    let url = serve(origin.clone()).await;
    let server = start_server(pool.clone()).await;
    let run = run(&server, &url).await;
    for (trigger_key, action_key) in [("triggers", "actions"), ("trigger", "action")] {
        for (triggers, actions) in [
            (Value::Null, json!([{"delay": 1}])),
            (json!([]), Value::Null),
            (Value::Null, Value::Null),
        ] {
            let automation = json!({(trigger_key): triggers, (action_key): actions});
            let script = json!({"sequence": null});
            {
                let mut o = origin.lock();
                o.definitions
                    .insert("automation.evening_reading".into(), automation.clone());
                o.definitions
                    .insert("script.reading_setup".into(), script.clone());
            }
            plugins::run("homeassistant", &run).await.unwrap();
            let docs = documents(&server, &pool).await;
            for (suffix, definition) in [("/automations", &automation), ("/scripts", &script)] {
                let document = docs.iter().find(|d| d.source.ends_with(suffix)).unwrap();
                let rendered = document
                    .text
                    .split("```json\n")
                    .nth(1)
                    .unwrap()
                    .split("\n```")
                    .next()
                    .unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(rendered).unwrap(),
                    *definition
                );
                assert!(
                    finds(&server, document.title.as_deref().unwrap())
                        .await
                        .iter()
                        .any(|hit| hit.document_id == document.id)
                );
            }
            assert_eq!(
                plugins::run("homeassistant", &run)
                    .await
                    .unwrap()
                    .stats
                    .unchanged,
                10
            );
        }
    }
}

#[sqlx::test(migrations = false)]
async fn invalid_required_definition_shapes_still_fail_without_sweeping(pool: PgPool) {
    let origin = origin();
    let original_definitions = origin.lock().definitions.clone();
    let url = serve(origin.clone()).await;
    let server = start_server(pool.clone()).await;
    let run = run(&server, &url).await;
    plugins::run("homeassistant", &run).await.unwrap();
    for (entity_id, invalid) in [
        ("automation.evening_reading", Value::Null),
        ("automation.evening_reading", json!([])),
        (
            "automation.evening_reading",
            json!("invalid top-level definition"),
        ),
        ("automation.evening_reading", json!({"triggers": null})),
        ("automation.evening_reading", json!({"actions": null})),
        ("automation.evening_reading", json!({"trigger": null})),
        ("automation.evening_reading", json!({"action": null})),
        (
            "automation.evening_reading",
            json!({"triggers": false, "actions": null}),
        ),
        (
            "automation.evening_reading",
            json!({"triggers": null, "actions": 1}),
        ),
        (
            "automation.evening_reading",
            json!({"trigger": "invalid", "action": null}),
        ),
        (
            "automation.evening_reading",
            json!({"trigger": null, "action": false}),
        ),
        ("script.reading_setup", Value::Null),
        ("script.reading_setup", json!({})),
        ("script.reading_setup", json!({"sequence": "invalid"})),
    ] {
        {
            let mut o = origin.lock();
            o.definitions = original_definitions.clone();
            o.definitions.insert(entity_id.into(), invalid.clone());
        }
        assert!(
            plugins::run("homeassistant", &run).await.is_err(),
            "{entity_id}: {invalid}"
        );
        assert_eq!(documents(&server, &pool).await.len(), 10);
    }
}

#[sqlx::test(migrations = false)]
async fn restored_behavior_placeholders_do_not_require_loaded_definitions(pool: PgPool) {
    let origin = origin();
    {
        let mut o = origin.lock();
        for (registry_id, entity_id) in [
            ("restored-automation", "automation.restored_fixture"),
            ("restored-script", "script.restored_fixture"),
            ("absent-runtime", "automation.unloaded_fixture"),
            ("ordinary-restored", "sensor.restored_fixture"),
        ] {
            let device_id = (registry_id == "ordinary-restored").then_some("parent");
            o.entities
                .push(entity(registry_id, entity_id, device_id, None));
            if registry_id != "absent-runtime" {
                let mut state = runtime(entity_id, registry_id);
                state["state"] = json!("unavailable");
                state["attributes"]["restored"] = json!(true);
                o.runtime.push(state);
            }
        }
        // An unavailable state alone is not evidence that a real loaded object is absent.
        o.runtime
            .iter_mut()
            .find(|state| state["entity_id"] == "script.reading_setup")
            .unwrap()["state"] = json!("unavailable");
        for (id, definition) in [
            (
                "automation.yaml_loaded",
                json!({"triggers": null, "actions": null}),
            ),
            ("script.yaml_loaded", json!({"sequence": null})),
        ] {
            o.runtime.push(runtime(id, id));
            o.definitions.insert(id.into(), definition);
        }
        o.relations.insert(("device".into(), "parent".into()),
            json!({"automation":["automation.evening_reading","automation.restored_fixture"],"script":["script.restored_fixture"]}));
        o.relations
            .get_mut(&("automation".into(), "automation.evening_reading".into()))
            .unwrap()["entity"] = json!(["script.restored_fixture"]);
    }
    let url = serve(origin.clone()).await;
    let server = start_server(pool.clone()).await;
    let run = run(&server, &url).await;
    assert_eq!(
        plugins::run("homeassistant", &run).await.unwrap().stats.put,
        12
    );
    let docs = documents(&server, &pool).await;
    let behavior_ids: Vec<_> = docs
        .iter()
        .filter(|doc| doc.source.ends_with("/automations") || doc.source.ends_with("/scripts"))
        .map(|doc| doc.external_id.as_str())
        .collect();
    assert_eq!(
        behavior_ids,
        [
            "registry:automation-registry",
            "runtime:automation.yaml_loaded",
            "registry:script-registry",
            "runtime:script.yaml_loaded"
        ]
    );
    for document in docs
        .iter()
        .filter(|doc| behavior_ids.contains(&doc.external_id.as_str()))
    {
        assert!(
            finds(&server, document.title.as_deref().unwrap())
                .await
                .iter()
                .any(|hit| hit.document_id == document.id)
        );
    }
    let parent = docs.iter().find(|doc| doc.external_id == "parent").unwrap();
    assert!(
        parent
            .text
            .contains("Exact entity ID: sensor.restored_fixture")
    );
    assert!(parent.text.contains(
        "restored-automation (automation.restored_fixture); configured definition not loaded"
    ));
    let automation = docs
        .iter()
        .find(|doc| doc.external_id == "registry:automation-registry")
        .unwrap();
    assert!(
        automation
            .text
            .contains("restored-script (script.restored_fixture)")
    );
    assert!(
        automation
            .text
            .contains("Configured definition not loaded.")
    );
    assert_eq!(
        plugins::run("homeassistant", &run)
            .await
            .unwrap()
            .stats
            .unchanged,
        12
    );
    {
        let mut o = origin.lock();
        for id in ["automation.evening_reading", "script.reading_setup"] {
            o.definitions.remove(id);
            let state = o
                .runtime
                .iter_mut()
                .find(|state| state["entity_id"] == id)
                .unwrap();
            state["state"] = json!("unavailable");
            state["attributes"]["restored"] = json!(true);
        }
    }
    assert_eq!(
        plugins::run("homeassistant", &run)
            .await
            .unwrap()
            .stats
            .deleted,
        2
    );
    let current = documents(&server, &pool).await;
    assert!(!current.iter().any(|doc| matches!(
        doc.external_id.as_str(),
        "registry:automation-registry" | "registry:script-registry"
    )));
    assert!(
        current
            .iter()
            .any(|doc| doc.external_id == "runtime:automation.yaml_loaded")
    );
    assert_eq!(
        plugins::run("homeassistant", &run)
            .await
            .unwrap()
            .stats
            .unchanged,
        10
    );
}

#[sqlx::test(migrations = false)]
async fn loaded_definition_errors_preserve_inventory_and_report_only_safe_context(pool: PgPool) {
    let origin = origin();
    let url = serve(origin.clone()).await;
    let server = start_server(pool.clone()).await;
    let run = run(&server, &url).await;
    plugins::run("homeassistant", &run).await.unwrap();
    for (command, entity_id) in [
        ("automation/config", "automation.evening_reading"),
        ("script/config", "script.reading_setup"),
    ] {
        for (origin_code, safe_code) in [
            ("unauthorized", "unauthorized"),
            ("not_found", "not_found"),
            ("unknown_command", "unknown_command"),
            (TOKEN, "unrecognized_error"),
        ] {
            origin.lock().command_error = Some((command, origin_code.to_owned()));
            let error = plugins::run("homeassistant", &run)
                .await
                .err()
                .expect("loaded definition error must fail")
                .to_string();
            assert_eq!(
                error,
                format!("Home Assistant command {command} for {entity_id} failed ({safe_code})")
            );
            assert!(!error.contains(TOKEN) && !error.contains("private origin failure"));
            assert_eq!(documents(&server, &pool).await.len(), 10);
        }
    }
}
