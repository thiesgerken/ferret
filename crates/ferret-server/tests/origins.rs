mod common;

use std::time::Duration;

use common::{TestServer, document, start_server};
use ferret_api::internal::{PluginRegistration, WorkerRegistration};
use serde_json::{Value, json};
use sqlx::PgPool;

/// A connection named "Notes" on `notes` and the API of its live run, which writes like its
/// plugin would.
async fn notes_connection(server: &TestServer) -> ferret_api::Client {
    let internal = server.internal();
    let worker = internal
        .register(&WorkerRegistration {
            name: "worker-0".to_owned(),
            plugins: vec![PluginRegistration {
                name: "fake".to_owned(),
                config_schema: json!({"type": "object"}),
            }],
        })
        .await
        .unwrap()
        .worker_id;
    let cookie = server.sign_in().await;
    let created = server
        .graphql(
            &cookie,
            "mutation ($input: NewConnection!) { createConnection(input: $input) { id } }",
            json!({"input": {"name": "Notes", "icon": "notebook", "plugin": "fake", "source": "notes", "config": "{}", "schedule": "0 * * * *"}}),
        )
        .await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    let run = internal
        .claim(worker, Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    server.api_with_key(&run.token)
}

const SEARCH: &str = "query ($q: String!) {
    search(input: {q: $q}) {
        hits { externalId origin { kind name icon } }
        facets { sources { value origin { kind name icon } } }
    }
    explore(input: {}) { facets { sources { value origin { kind name } } } }
}";

#[sqlx::test(migrations = false)]
async fn hits_and_source_facets_come_with_their_connection_or_api_key(pool: PgPool) {
    let server = start_server(pool).await;
    let run = notes_connection(&server).await;
    run.put_document(&document("notes", "note", "An invoice in my notes."))
        .await
        .unwrap();
    let api = server.admin_api().await;
    api.put_document(&document("inbox", "scan", "An invoice in the inbox."))
        .await
        .unwrap();
    let cookie = server.sign_in().await;

    let body = server
        .graphql(&cookie, SEARCH, json!({"q": "invoice"}))
        .await;

    assert_eq!(body["errors"], Value::Null, "{body}");
    let connection = json!({"kind": "CONNECTION", "name": "Notes", "icon": "notebook"});
    let key = json!({"kind": "API_KEY", "name": "test key", "icon": "key-round"});
    let origins: Vec<(String, Value)> = body["data"]["search"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| {
            (
                hit["externalId"].as_str().unwrap().to_owned(),
                hit["origin"].clone(),
            )
        })
        .collect();
    assert!(
        origins.contains(&("note".to_owned(), connection.clone())),
        "{origins:?}"
    );
    assert!(
        origins.contains(&("scan".to_owned(), key.clone())),
        "{origins:?}"
    );
    let sources = &body["data"]["search"]["facets"]["sources"];
    assert!(
        sources
            .as_array()
            .unwrap()
            .contains(&json!({"value": "notes", "origin": connection})),
        "{sources}"
    );
    let explored = &body["data"]["explore"]["facets"]["sources"];
    assert!(
        explored.as_array().unwrap().contains(
            &json!({"value": "inbox", "origin": {"kind": "API_KEY", "name": "test key"}})
        ),
        "{explored}"
    );

    // A revoked key stops working but stays the origin of what it put.
    let keys = server
        .graphql(&cookie, "{ apiKeys { id } }", json!({}))
        .await;
    for key_id in keys["data"]["apiKeys"].as_array().unwrap() {
        let revoked = server
            .graphql(
                &cookie,
                "mutation ($id: ID!) { revokeApiKey(id: $id) }",
                json!({"id": key_id["id"]}),
            )
            .await;
        assert_eq!(revoked["errors"], Value::Null, "{revoked}");
    }
    assert!(
        api.search(&ferret_api::SearchQuery {
            q: "invoice".to_owned(),
            ..Default::default()
        })
        .await
        .is_err()
    );
    let body = server.graphql(&cookie, SEARCH, json!({"q": "inbox"})).await;
    assert_eq!(body["data"]["search"]["hits"][0]["origin"], key, "{body}");
    let keys = server
        .graphql(&cookie, "{ apiKeys { id } }", json!({}))
        .await;
    assert_eq!(
        keys["data"]["apiKeys"],
        json!([]),
        "revoked keys aren't listed"
    );
}

#[sqlx::test(migrations = false)]
async fn connections_and_keys_take_a_name_and_an_icon(pool: PgPool) {
    let server = start_server(pool).await;
    notes_connection(&server).await;
    let cookie = server.sign_in().await;
    let connections = server
        .graphql(&cookie, "{ connections { id name icon } }", json!({}))
        .await;
    let id = connections["data"]["connections"][0]["id"].clone();

    let update = |name: &str, icon: &str| json!({"id": id, "input": {"name": name, "icon": icon, "config": "{}", "schedule": "0 * * * *", "enabled": true}});
    const UPDATE: &str = "mutation ($id: ID!, $input: ConnectionUpdate!) {
        updateConnection(id: $id, input: $input) { name icon }
    }";
    let updated = server
        .graphql(&cookie, UPDATE, update(" Journal ", "book-open"))
        .await;
    assert_eq!(
        updated["data"]["updateConnection"],
        json!({"name": "Journal", "icon": "book-open"}),
        "{updated}"
    );
    for (name, icon, message) in [
        (" ", "book-open", "a connection needs a name"),
        (
            "Journal",
            "Book_Open",
            "\"Book_Open\" is not an icon name like `git-branch`",
        ),
    ] {
        let rejected = server.graphql(&cookie, UPDATE, update(name, icon)).await;
        assert_eq!(rejected["errors"][0]["message"], message, "{rejected}");
    }

    server.create_api_key(&["SEARCH_READ"], None).await;
    let keys = server
        .graphql(&cookie, "{ apiKeys { id } }", json!({}))
        .await;
    const UPDATE_KEY: &str = "mutation ($id: ID!, $name: String!, $icon: String!) {
        updateApiKey(id: $id, name: $name, icon: $icon) { name icon }
    }";
    let key_id = keys["data"]["apiKeys"][0]["id"].clone();
    let renamed = server
        .graphql(
            &cookie,
            UPDATE_KEY,
            json!({"id": key_id, "name": "Scanner", "icon": "scan-line"}),
        )
        .await;
    assert_eq!(
        renamed["data"]["updateApiKey"],
        json!({"name": "Scanner", "icon": "scan-line"}),
        "{renamed}"
    );
    let rejected = server
        .graphql(
            &cookie,
            UPDATE_KEY,
            json!({"id": key_id, "name": "Scanner", "icon": ""}),
        )
        .await;
    assert_eq!(
        rejected["errors"][0]["message"],
        "\"\" is not an icon name like `key-round`"
    );
}
