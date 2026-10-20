mod common;

use common::{TestServer, start_server};
use ferret_api::{
    InventoryEntry,
    internal::{
        ClaimedRun, PluginRegistration, RunFinish, RunResult, RunStats, WorkerRegistration,
    },
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

async fn connection(server: &TestServer, source: &str) -> (Uuid, Uuid) {
    let worker = server
        .internal()
        .register(&WorkerRegistration {
            name: "inventory-worker".into(),
            plugins: vec![PluginRegistration {
                name: "inventory-fixture".into(),
                config_schema: json!({"type":"object"}),
            }],
        })
        .await
        .unwrap()
        .worker_id;
    let cookie = server.sign_in().await;
    let result = server.graphql(&cookie,
        "mutation ($input: NewConnection!) { createConnection(input: $input) { id } }",
        json!({"input":{"name":"Inventory fixture", "icon":"folder", "plugin":"inventory-fixture",
            "source":source, "config":"{}", "schedule":"0 * * * *"}})).await;
    assert_eq!(result["errors"], Value::Null, "{result}");
    (
        result["data"]["createConnection"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        worker,
    )
}

async fn claim(server: &TestServer, worker: Uuid) -> ClaimedRun {
    server
        .internal()
        .claim(worker, Duration::ZERO)
        .await
        .unwrap()
        .unwrap()
}

async fn due(pool: &PgPool, connection: Uuid) {
    sqlx::query("update connections set next_run_at = now() where id = $1")
        .bind(connection)
        .execute(pool)
        .await
        .unwrap();
}

fn entry(id: &str, fingerprint: &str) -> InventoryEntry {
    InventoryEntry {
        external_id: id.into(),
        fingerprint: fingerprint.into(),
        indexed: true,
    }
}

async fn finish(server: &TestServer, run: &ClaimedRun, success: bool) {
    server
        .internal()
        .finish(
            run.run_id,
            &RunFinish {
                result: if success {
                    RunResult::Succeeded {
                        state: json!({"inventory_generation":run.run_id, "indexing":"fixture"}),
                        stats: RunStats::default(),
                    }
                } else {
                    RunResult::Failed {
                        error: "injected failure".into(),
                    }
                },
                log_lines: Vec::new(),
            },
        )
        .await
        .unwrap();
}

#[sqlx::test(migrations = false)]
async fn inventory_is_bounded_durable_connection_scoped_and_success_only(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let (connection_id, worker) = connection(&server, "files/one").await;
    let first = claim(&server, worker).await;
    let api = server.api_with_key(&first.token);
    let staged = api
        .inventory_stage(&[entry("file:a.txt", "first")])
        .await
        .unwrap();
    assert_eq!(staged.generation, first.run_id);
    assert!(
        api.inventory_lookup(first.run_id, vec!["file:a.txt".into()])
            .await
            .unwrap()
            .is_empty()
    );
    let overlarge: Vec<_> = (0..513)
        .map(|i| entry(&format!("file:{i}"), "metadata"))
        .collect();
    assert_eq!(
        api.inventory_stage(&overlarge)
            .await
            .unwrap_err()
            .status()
            .unwrap(),
        400
    );
    assert_eq!(
        api.inventory_lookup(
            first.run_id,
            overlarge.iter().map(|e| e.external_id.clone()).collect()
        )
        .await
        .unwrap_err()
        .status()
        .unwrap(),
        400
    );
    assert_eq!(
        server
            .admin_api()
            .await
            .inventory_stage(&[])
            .await
            .unwrap_err()
            .status()
            .unwrap(),
        403
    );
    finish(&server, &first, true).await;
    assert_eq!(
        api.inventory_stage(&[])
            .await
            .unwrap_err()
            .status()
            .unwrap(),
        401
    );

    // A replacement worker reuses durable server state, not pod-local data.
    let replacement = server
        .internal()
        .register(&WorkerRegistration {
            name: "replacement".into(),
            plugins: vec![PluginRegistration {
                name: "inventory-fixture".into(),
                config_schema: json!({"type":"object"}),
            }],
        })
        .await
        .unwrap()
        .worker_id;
    due(&pool, connection_id).await;
    let second = claim(&server, replacement).await;
    assert_eq!(
        second.state["inventory_generation"],
        first.run_id.to_string()
    );
    let api = server.api_with_key(&second.token);
    let found = api
        .inventory_lookup(first.run_id, vec!["file:a.txt".into()])
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].fingerprint, "first");
    api.inventory_stage(&[entry("file:a.txt", "partial")])
        .await
        .unwrap();
    finish(&server, &second, false).await;
    due(&pool, connection_id).await;
    let third = claim(&server, worker).await;
    assert_eq!(third.state, json!({}));
    let api = server.api_with_key(&third.token);
    assert!(
        api.inventory_lookup(first.run_id, vec!["file:a.txt".into()])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        api.inventory_lookup(second.run_id, vec!["file:a.txt".into()])
            .await
            .unwrap()
            .is_empty()
    );
    api.inventory_stage(&[entry("file:a.txt", "reconciled")])
        .await
        .unwrap();
    finish(&server, &third, true).await;
    let remaining: i64 =
        sqlx::query_scalar("select count(*) from plugin_inventory where connection_id = $1")
            .bind(connection_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 1);

    let (other_connection, other_worker) = connection(&server, "files/two").await;
    let other = claim(&server, other_worker).await;
    assert_eq!(other.connection_id, other_connection);
    assert!(
        server
            .api_with_key(&other.token)
            .inventory_lookup(third.run_id, vec!["file:a.txt".into()])
            .await
            .unwrap()
            .is_empty()
    );
    finish(&server, &other, true).await;

    // Removing a remembered exclusion resets connection state to {}, which also invalidates bulk inventory.
    sqlx::query("update connections set state = '{}' where id = $1")
        .bind(connection_id)
        .execute(&pool)
        .await
        .unwrap();
    due(&pool, connection_id).await;
    let reset = claim(&server, worker).await;
    assert_eq!(reset.state, json!({}));
    assert!(
        server
            .api_with_key(&reset.token)
            .inventory_lookup(third.run_id, vec!["file:a.txt".into()])
            .await
            .unwrap()
            .is_empty()
    );
    finish(&server, &reset, true).await;
}

#[sqlx::test(migrations = false)]
async fn a_lost_partial_generation_cannot_suppress_reconciliation(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let (connection_id, worker) = connection(&server, "files/lost").await;
    let successful = claim(&server, worker).await;
    server
        .api_with_key(&successful.token)
        .inventory_stage(&[entry("file:a", "successful")])
        .await
        .unwrap();
    finish(&server, &successful, true).await;
    due(&pool, connection_id).await;
    let lost = claim(&server, worker).await;
    server
        .api_with_key(&lost.token)
        .inventory_stage(&[entry("file:a", "partial")])
        .await
        .unwrap();
    sqlx::query("update plugin_runs set heartbeat_at = now() - interval '3 minutes' where id = $1")
        .bind(lost.run_id)
        .execute(&pool)
        .await
        .unwrap();
    due(&pool, connection_id).await;
    let retry = claim(&server, worker).await;
    assert_eq!(retry.state, json!({}));
    let api = server.api_with_key(&retry.token);
    for generation in [successful.run_id, lost.run_id] {
        assert!(
            api.inventory_lookup(generation, vec!["file:a".into()])
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(
        server
            .api_with_key(&lost.token)
            .inventory_stage(&[])
            .await
            .unwrap_err()
            .status()
            .unwrap(),
        401
    );
    finish(&server, &retry, true).await;
}

#[sqlx::test(migrations = false)]
async fn representative_inventory_scale_keeps_claims_and_requests_bounded(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let (connection_id, worker) = connection(&server, "files/scale").await;
    let run = claim(&server, worker).await;
    let api = server.api_with_key(&run.token);
    let count = 326_144;
    for start in (0..count).step_by(512) {
        let entries: Vec<_> = (start..start + 512)
            .map(|i| {
                entry(
                    &format!("file:directory-{}/file-{i}.txt", i / 100),
                    "metadata",
                )
            })
            .collect();
        assert_eq!(
            api.inventory_stage(&entries).await.unwrap().generation,
            run.run_id
        );
    }
    finish(&server, &run, true).await;
    let stored: i64 =
        sqlx::query_scalar("select count(*) from plugin_inventory where connection_id = $1")
            .bind(connection_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, count as i64);
    due(&pool, connection_id).await;
    let next = claim(&server, worker).await;
    assert!(serde_json::to_vec(&next.state).unwrap().len() < 128);
    let found = server
        .api_with_key(&next.token)
        .inventory_lookup(
            run.run_id,
            vec!["file:directory-3261/file-326143.txt".into()],
        )
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    finish(&server, &next, false).await;
}
