mod common;

use common::{
    DOCUMENT_PREFIX, MODEL, QUERY_PREFIX, document, embedding_config,
    fake_tei::{FakeTei, MAX_INPUT_LENGTH, Similarity},
    model, start_semantic_server, start_server_with,
};
use ferret_api::{Client, Hit, SearchQuery};
use sqlx::PgPool;

async fn search(api: &Client, query: SearchQuery) -> Vec<Hit> {
    api.search(&query).await.unwrap().hits
}

fn query(q: &str) -> SearchQuery {
    SearchQuery {
        q: q.to_owned(),
        ..Default::default()
    }
}

fn ids(hits: &[Hit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.external_id.as_str()).collect()
}

#[sqlx::test(migrations = false)]
async fn a_query_without_keyword_overlap_finds_a_semantic_match_once_embedded(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document(
        "notes",
        "car",
        "My car broke down on the highway.",
    ))
    .await
    .unwrap();
    api.put_document(&document("notes", "weather", "Lovely weather today."))
        .await
        .unwrap();

    server.wait_for_embeddings(2).await;
    let hits = search(&api, query("vehicle")).await;

    assert_eq!(ids(&hits), ["car"]);
    assert!(hits[0].chunk.parts.iter().all(|part| !part.highlight));
}

#[sqlx::test(migrations = false)]
async fn keyword_and_semantic_matches_are_fused(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    for (id, text) in [
        ("both", "The vehicle needs new tyres."),
        ("semantic", "Our automobile is red."),
        ("unrelated", "Lovely weather today."),
    ] {
        api.put_document(&document("notes", id, text))
            .await
            .unwrap();
    }
    server.wait_for_embeddings(3).await;

    let hits = search(&api, query("vehicle")).await;

    assert_eq!(ids(&hits), ["both", "semantic"]);
    assert!(hits[0].score > hits[1].score);
}

#[sqlx::test(migrations = false)]
async fn the_model_and_its_prefixes_are_used_and_identical_texts_embedded_once(pool: PgPool) {
    let server = start_semantic_server(pool.clone()).await;
    let api = server.admin_api().await;
    for source in ["notes", "paperless"] {
        api.put_document(&document(source, "same", "The same text twice."))
            .await
            .unwrap();
    }
    server.wait_for_embeddings(2).await;
    for _ in 0..2 {
        search(&api, query("text")).await;
    }

    let embedded = server.tei.embedded();
    let count = |text: &str| embedded.iter().filter(|embedded| *embedded == text).count();
    assert_eq!(count(&format!("{DOCUMENT_PREFIX}The same text twice.")), 1);
    assert_eq!(count(&format!("{QUERY_PREFIX}text")), 1);
    let models: Vec<String> = sqlx::query_scalar("select distinct model from chunk_embeddings")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(models, [MODEL]);
}

#[sqlx::test(migrations = false)]
async fn each_model_gets_a_partial_hnsw_index(pool: PgPool) {
    let _server = start_semantic_server(pool.clone()).await;

    let definition: String = sqlx::query_scalar(
        "select indexdef from pg_indexes where indexname = 'chunk_embeddings_hnsw_fake'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(definition.contains("USING hnsw"), "{definition}");
    assert!(definition.contains("halfvec(256)"), "{definition}");
    assert!(
        definition.contains("WHERE (model = 'fake'::text)"),
        "{definition}"
    );
}

/// Enough rows that the planner uses the HNSW index rather than an exact scan.
const NOISE_CHUNKS: usize = 1000;

/// The nearest neighbours all lie outside the filter: without iterative scans, the HNSW index
/// would hand back only those, and the filter would leave fewer than k hits.
#[sqlx::test(migrations = false)]
async fn filtered_semantic_searches_still_return_k_hits(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    // One document of many chunks, closer to the query than any target. The chunks differ in
    // several dimensions: (near-)identical vectors make a degenerate HNSW graph, which real data
    // never has and which iterative scans can't search through.
    let noise: Vec<String> = (0..NOISE_CHUNKS)
        .map(|index| {
            format!(
                "car car car variant{index} kind{} group{} ",
                index % 17,
                index % 31
            )
            .repeat(30)
        })
        .collect();
    let noise = noise.join("\n\n");
    api.put_document(&document("noise", "noise", &noise))
        .await
        .unwrap();
    for index in 0..5 {
        api.put_document(&document(
            "target",
            &format!("target {index}"),
            &format!("automobile repair shop number {index}"),
        ))
        .await
        .unwrap();
    }
    server.wait_for_embeddings(NOISE_CHUNKS as i64 + 5).await;

    let hits = search(
        &api,
        SearchQuery {
            sources: vec!["target".to_owned()],
            k: Some(5),
            ..query("vehicle")
        },
    )
    .await;

    assert_eq!(hits.len(), 5);
    assert!(hits.iter().all(|hit| hit.source == "target"));
}

#[sqlx::test(migrations = false)]
async fn a_model_shorter_than_the_longest_chunk_stops_the_server(pool: PgPool) {
    ferret_server::migrate(&pool).await.unwrap();
    let tei = FakeTei::start(Similarity::None, 1999).await;

    let error = ferret_server::embeddings::prepare(&pool, embedding_config(model(MODEL, &tei)))
        .await
        .err()
        .unwrap();

    assert!(error.to_string().contains("1999 tokens"), "{error}");
}

#[sqlx::test(migrations = false)]
async fn with_a_bulk_url_the_pump_uses_only_that_and_queries_keep_using_url(pool: PgPool) {
    let online = FakeTei::start(Similarity::Words, MAX_INPUT_LENGTH).await;
    let bulk = FakeTei::start(Similarity::Words, MAX_INPUT_LENGTH).await;
    let mut config = model(MODEL, &online);
    config.bulk_url = Some(bulk.url.clone());
    let server = start_server_with(pool, online.clone(), embedding_config(config)).await;
    let api = server.admin_api().await;

    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;
    let hits = search(&api, query("vehicle")).await;

    let document_text = format!("{DOCUMENT_PREFIX}My car broke down.");
    assert_eq!(ids(&hits), ["car"]);
    assert!(bulk.embedded().contains(&document_text));
    assert!(!online.embedded().contains(&document_text));
    assert!(
        online
            .embedded()
            .contains(&format!("{QUERY_PREFIX}vehicle"))
    );
}

#[sqlx::test(migrations = false)]
async fn an_unreachable_bulk_url_is_reported_and_keyword_search_keeps_working(pool: PgPool) {
    let online = FakeTei::start(Similarity::Words, MAX_INPUT_LENGTH).await;
    let mut config = model(MODEL, &online);
    // Nothing listens on port 1.
    config.bulk_url = Some("http://127.0.0.1:1".to_owned());
    let server = start_server_with(pool.clone(), online, embedding_config(config)).await;
    let api = server.admin_api().await;

    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    let mut error = None;
    for _ in 0..100 {
        error =
            sqlx::query_scalar::<_, String>("select error from embedding_errors where model = $1")
                .bind(MODEL)
                .fetch_optional(&pool)
                .await
                .unwrap();
        if error.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert!(error.unwrap().contains("127.0.0.1:1"));
    let embedded: i64 = sqlx::query_scalar("select count(*) from chunk_embeddings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(embedded, 0);
    assert_eq!(ids(&search(&api, query("car")).await), ["car"]);
}
