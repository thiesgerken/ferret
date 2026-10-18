mod common;

use common::{TestServer, document, start_server};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use sqlx::PgPool;

async fn connect(server: &TestServer, key: &str) -> RunningService<RoleClient, ()> {
    let config =
        StreamableHttpClientTransportConfig::with_uri(format!("{}/mcp", server.public_url()))
            .auth_header(key);
    ().serve(StreamableHttpClientTransport::from_config(config))
        .await
        .expect("the client connects")
}

async fn call(
    client: &RunningService<RoleClient, ()>,
    tool: &str,
    arguments: Value,
) -> CallToolResult {
    let Value::Object(arguments) = arguments else {
        panic!("arguments are an object");
    };
    client
        .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments))
        .await
        .expect("the tool answers")
}

fn structured(result: CallToolResult) -> Value {
    assert_ne!(result.is_error, Some(true), "{result:?}");
    result.structured_content.expect("a structured result")
}

/// The client puts notes and mail, the agent's key may only read notes.
async fn notes_and_mail(server: &TestServer) -> String {
    let api = server.admin_api().await;
    for (source, id, text) in [
        ("notes", "car", "My car broke down on the motorway."),
        ("notes", "garage", "The garage repairs the car on Monday."),
        (
            "mail/personal",
            "invoice",
            "Your car insurance invoice is attached.",
        ),
    ] {
        api.put_document(&document(source, id, text)).await.unwrap();
    }
    server
        .create_api_key(&["SEARCH_READ"], Some(&["notes"]))
        .await
}

#[sqlx::test(migrations = false)]
async fn an_agent_searches_reads_and_lists_within_its_keys_sources(pool: PgPool) {
    let server = start_server(pool).await;
    let key = notes_and_mail(&server).await;
    let client = connect(&server, &key).await;

    let mut tools: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    tools.sort();
    let first = structured(call(&client, "search", json!({"query": "car", "limit": 1})).await);
    let next = first["next_cursor"].as_str().unwrap().to_owned();
    let second = structured(
        call(
            &client,
            "search",
            json!({"query": "car", "limit": 1, "cursor": next, "mode": "keyword"}),
        )
        .await,
    );
    let id = first["hits"][0]["document_id"].as_str().unwrap();
    let read = structured(call(&client, "get_document", json!({"id": id})).await);
    let sources = structured(call(&client, "list_sources", json!({})).await);

    assert_eq!(tools, ["get_document", "list_sources", "search"]);
    assert_eq!(
        first["total"], 2,
        "the mail is outside the key's sources: {first}"
    );
    assert_eq!(first["hits"][0]["source"], "notes");
    assert!(
        first["hits"][0]["chunk"].as_str().unwrap().contains("car"),
        "{first}"
    );
    assert_eq!(second["hits"].as_array().unwrap().len(), 1);
    assert_ne!(
        second["hits"][0]["document_id"],
        first["hits"][0]["document_id"]
    );
    assert_eq!(second["next_cursor"], Value::Null);
    assert_eq!(read["id"], id);
    assert!(read["text"].as_str().unwrap().contains("car"), "{read}");
    assert_eq!(
        sources,
        json!({"sources": [{"source": "notes", "documents": 2, "writer": "test key"}]})
    );
}

#[sqlx::test(migrations = false)]
async fn a_document_outside_the_keys_sources_and_a_bad_query_are_tool_errors(pool: PgPool) {
    let server = start_server(pool).await;
    let key = notes_and_mail(&server).await;
    let mail = server
        .admin_api()
        .await
        .search(&ferret_api::SearchQuery {
            q: "invoice".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap()
        .hits[0]
        .document_id;
    let client = connect(&server, &key).await;

    let outside = call(&client, "get_document", json!({"id": mail})).await;
    let excluded_only = call(&client, "search", json!({"query": "-car"})).await;
    let too_many = call(&client, "search", json!({"query": "car", "limit": 101})).await;

    assert_eq!(outside.is_error, Some(true));
    assert_eq!(excluded_only.is_error, Some(true));
    assert!(format!("{excluded_only:?}").contains("at least one word"));
    assert_eq!(too_many.is_error, Some(true));
    assert!(format!("{too_many:?}").contains("limit must be between 1 and 100"));
}

#[sqlx::test(migrations = false)]
async fn mcp_needs_a_key_with_search_read(pool: PgPool) {
    let server = start_server(pool).await;
    let ingest_only = server.create_api_key(&["INGEST_WRITE"], None).await;
    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "test", "version": "1"}},
    });
    let post = |key: Option<&str>| {
        let request = server
            .request(reqwest::Method::POST, "/mcp")
            .header("accept", "application/json, text/event-stream")
            .json(&initialize);
        match key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    };

    let anonymous = post(None).send().await.unwrap();
    let without_scope = post(Some(&ingest_only)).send().await.unwrap();

    assert_eq!(anonymous.status(), 401);
    assert_eq!(without_scope.status(), 403);
}
