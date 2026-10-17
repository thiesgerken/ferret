mod common;

use common::{
    DOCUMENT_PREFIX, MODEL, QUERY_PREFIX, document, embedding_config, eventually,
    fake_tei::{FakeTei, MAX_INPUT_LENGTH, UNEMBEDDABLE},
    model, start_semantic_server, start_server_with,
};
use ferret_api::{Client, Hit, SearchMode, SearchQuery};
use reqwest::StatusCode;
use sqlx::PgPool;
use std::time::{Duration, Instant};
use uuid::Uuid;

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

    // The test model's `min_similarity` is 0, so the unrelated document trails.
    assert_eq!(ids(&hits), ["car", "weather"]);
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

    assert_eq!(ids(&hits), ["both", "semantic", "unrelated"]);
    assert!(hits[0].keyword_score.is_some() && hits[0].similarity.is_some());
    assert!(hits[1].keyword_score.is_none());
    assert!(hits[1].similarity > hits[2].similarity);
}

#[sqlx::test(migrations = false)]
async fn the_model_and_its_prefixes_are_used_and_identical_texts_embedded_once(pool: PgPool) {
    let server = start_semantic_server(pool.clone()).await;
    let api = server.admin_api().await;
    let same = |source| document(source, "same", "The same text twice.");
    // Within one batch, then in a later one.
    for source in ["notes", "paperless"] {
        api.put_document(&same(source)).await.unwrap();
    }
    server.wait_for_embeddings(2).await;
    api.put_document(&same("mail")).await.unwrap();
    server.wait_for_embeddings(3).await;
    for _ in 0..2 {
        search(&api, query("text")).await;
    }

    let embedded = server.tei.embedded();
    let count = |text: &str| embedded.iter().filter(|embedded| *embedded == text).count();
    assert_eq!(
        count(&format!("{DOCUMENT_PREFIX}same\n\nThe same text twice.")),
        1
    );
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
    // Takes every chunk, but not with a title in front.
    let tei = FakeTei::start(2201).await;

    let error = ferret_server::embeddings::prepare(&pool, embedding_config(model(MODEL, &tei)))
        .await
        .err()
        .unwrap();

    assert!(error.to_string().contains("2201 tokens"), "{error}");
}

#[sqlx::test(migrations = false)]
async fn with_a_bulk_url_the_pump_uses_only_that_and_queries_keep_using_url(pool: PgPool) {
    let online = FakeTei::start(MAX_INPUT_LENGTH).await;
    let bulk = FakeTei::start(MAX_INPUT_LENGTH).await;
    let mut config = model(MODEL, &online);
    config.bulk_url = Some(bulk.url.clone());
    let server = start_server_with(pool, online.clone(), embedding_config(config)).await;
    let api = server.admin_api().await;

    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;
    let hits = search(&api, query("vehicle")).await;

    let document_text = format!("{DOCUMENT_PREFIX}car\n\nMy car broke down.");
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
    let online = FakeTei::start(MAX_INPUT_LENGTH).await;
    let mut config = model(MODEL, &online);
    // Nothing listens on port 1.
    config.bulk_url = Some("http://127.0.0.1:1".to_owned());
    let server = start_server_with(pool.clone(), online, embedding_config(config)).await;
    let api = server.admin_api().await;

    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    let error: String = eventually("a recorded embedding error", async || {
        sqlx::query_scalar("select error from embedding_errors where model = $1")
            .bind(MODEL)
            .fetch_optional(&pool)
            .await
            .unwrap()
    })
    .await;

    assert!(error.contains("127.0.0.1:1"));
    let embedded: i64 = sqlx::query_scalar("select count(*) from chunk_embeddings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(embedded, 0);
    assert_eq!(ids(&search(&api, query("car")).await), ["car"]);
}

#[sqlx::test(migrations = false)]
async fn without_the_embedding_model_search_answers_by_keyword_and_says_so(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;

    server.tei.break_down();
    let response = api.search(&query("car")).await.unwrap();
    let cookie = server.sign_in().await;
    let graphql = server
        .graphql(
            &cookie,
            "{ search(input: {q: \"vehicle\"}) { hits { externalId } semanticUnavailable } }",
            serde_json::json!({}),
        )
        .await;

    assert_eq!(ids(&response.hits), ["car"]);
    assert_eq!(
        response.semantic_unavailable.as_deref(),
        Some("the embedding model fake is unavailable")
    );
    assert_eq!(
        graphql["data"]["search"],
        serde_json::json!({"hits": [], "semanticUnavailable": "the embedding model fake is unavailable"})
    );
}

fn in_mode(q: &str, mode: SearchMode) -> SearchQuery {
    SearchQuery { mode, ..query(q) }
}

#[sqlx::test(migrations = false)]
async fn each_half_runs_alone_in_its_mode(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    api.put_document(&document("notes", "weather", "Lovely weather today."))
        .await
        .unwrap();
    server.wait_for_embeddings(2).await;

    let keyword = api
        .search(&in_mode("car", SearchMode::Keyword))
        .await
        .unwrap();
    let semantic = api
        .search(&in_mode("car", SearchMode::Semantic))
        .await
        .unwrap();

    assert_eq!(ids(&keyword.hits), ["car"]);
    assert_eq!(keyword.total, 1);
    assert!(keyword.hits.iter().all(|hit| hit.similarity.is_none()));
    // The test model's `min_similarity` is 0, so every embedded document matches semantically.
    assert_eq!(ids(&semantic.hits), ["car", "weather"]);
    assert_eq!(semantic.total, 2);
    assert!(!semantic.total_is_lower_bound);
    assert!(semantic.hits.iter().all(|hit| hit.keyword_score.is_none()));
    assert_eq!(
        semantic
            .facets
            .sources
            .iter()
            .map(|count| count.count)
            .sum::<u32>(),
        2
    );
}

#[sqlx::test(migrations = false)]
async fn a_keyword_search_does_not_embed_the_query(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;
    let embedded_before = server.tei.embedded().len();

    server.tei.break_down();
    let response = api
        .search(&in_mode("car", SearchMode::Keyword))
        .await
        .unwrap();

    assert_eq!(ids(&response.hits), ["car"]);
    assert_eq!(response.semantic_unavailable, None);
    assert_eq!(server.tei.embedded().len(), embedded_before);
}

#[sqlx::test(migrations = false)]
async fn a_semantic_search_without_the_embedding_model_fails(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;

    server.tei.break_down();
    let error = api
        .search(&in_mode("car", SearchMode::Semantic))
        .await
        .unwrap_err();
    let cookie = server.sign_in().await;
    let graphql = server
        .graphql(
            &cookie,
            "{ search(input: {q: \"car\", mode: SEMANTIC}) { total } }",
            serde_json::json!({}),
        )
        .await;

    assert_eq!(error.status(), Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(
        graphql["errors"][0]["message"],
        "the embedding model fake is unavailable"
    );
    assert_eq!(graphql["errors"][0]["extensions"]["code"], "UNAVAILABLE");
}

#[sqlx::test(migrations = false)]
async fn a_chunk_the_model_rejects_is_recorded_and_the_pump_moves_on(pool: PgPool) {
    let server = start_semantic_server(pool.clone()).await;
    let api = server.admin_api().await;
    for (id, text) in [
        ("rejected", format!("An {UNEMBEDDABLE} car.")),
        ("car", "My car broke down.".to_owned()),
    ] {
        api.put_document(&document("notes", id, &text))
            .await
            .unwrap();
    }
    server.wait_for_embeddings(1).await;
    api.put_document(&document("notes", "later", "A vehicle came later."))
        .await
        .unwrap();
    server.wait_for_embeddings(2).await;

    let failures: Vec<(String, String)> = sqlx::query_as(
        "select d.external_id, f.error from embedding_failures f
         join chunks c on c.id = f.chunk_id join documents d on d.id = c.document_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, "rejected");
    assert!(failures[0].1.contains("422"), "{}", failures[0].1);
}

#[sqlx::test(migrations = false)]
async fn a_stuck_embedding_model_degrades_search_to_keywords_quickly(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;

    server.tei.hang();
    let started = Instant::now();
    let response = api.search(&query("car")).await.unwrap();

    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(ids(&response.hits), ["car"]);
    assert!(response.semantic_unavailable.is_some());
}

#[sqlx::test(migrations = false)]
async fn the_title_is_embedded_with_every_chunk_and_a_new_title_re_embeds(pool: PgPool) {
    let server = start_semantic_server(pool.clone()).await;
    let api = server.admin_api().await;
    let mut note = document("notes", "note", "Broke down on the highway.");
    note.title = Some("My car".to_owned());
    api.put_document(&note).await.unwrap();
    server.wait_for_embeddings(1).await;
    let chunk: Uuid = sqlx::query_scalar("select chunk_id from chunk_embeddings")
        .fetch_one(&pool)
        .await
        .unwrap();

    note.title = Some("My bike".to_owned());
    api.put_document(&note).await.unwrap();
    server.wait_for_embeddings(1).await;

    let embedded = server.tei.embedded();
    for title in ["My car", "My bike"] {
        let input = format!("{DOCUMENT_PREFIX}{title}\n\nBroke down on the highway.");
        assert!(embedded.contains(&input), "{embedded:?}");
    }
    let rechunked: Uuid = sqlx::query_scalar("select chunk_id from chunk_embeddings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rechunked, chunk, "a new title keeps the chunk");
}

#[sqlx::test(migrations = false)]
async fn a_chunk_rejected_for_its_title_gets_another_chance_with_a_new_one(pool: PgPool) {
    let server = start_semantic_server(pool.clone()).await;
    let api = server.admin_api().await;
    let mut note = document("notes", "note", "Broke down on the highway.");
    note.title = Some(format!("An {UNEMBEDDABLE} title"));
    api.put_document(&note).await.unwrap();
    let failures = async || -> i64 {
        sqlx::query_scalar("select count(*) from embedding_failures")
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    eventually("a recorded failure", async || {
        (failures().await == 1).then_some(())
    })
    .await;

    note.title = Some("My car".to_owned());
    api.put_document(&note).await.unwrap();
    server.wait_for_embeddings(1).await;

    assert_eq!(failures().await, 0);
}

#[sqlx::test(migrations = false)]
async fn long_titles_are_capped_in_the_embedded_input(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    let mut note = document("notes", "note", "Broke down on the highway.");
    let title = "abcd ".repeat(100);
    note.title = Some(title.clone());
    api.put_document(&note).await.unwrap();
    server.wait_for_embeddings(1).await;

    let capped = format!(
        "{DOCUMENT_PREFIX}{}\n\nBroke down on the highway.",
        &title[..200]
    );
    assert!(server.tei.embedded().contains(&capped));
}

#[sqlx::test(migrations = false)]
async fn semantic_candidates_below_the_models_min_similarity_are_dropped(pool: PgPool) {
    let tei = FakeTei::start(MAX_INPUT_LENGTH).await;
    let mut strict = model(MODEL, &tei);
    strict.min_similarity = 0.3;
    let server = start_server_with(pool, tei, embedding_config(strict)).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    api.put_document(&document("notes", "weather", "Lovely weather today."))
        .await
        .unwrap();
    server.wait_for_embeddings(2).await;

    let response = api.search(&query("vehicle")).await.unwrap();

    assert_eq!(ids(&response.hits), ["car"]);
    assert!(
        response.hits[0]
            .similarity
            .is_some_and(|similarity| similarity >= 0.3)
    );
    assert_eq!(response.total, 1);
    assert!(
        !response.total_is_lower_bound,
        "a neighbour fell below the cut-off"
    );
}

#[sqlx::test(migrations = false)]
async fn the_total_is_a_lower_bound_only_while_more_chunks_could_pass_the_cut_off(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    // The test model's `min_similarity` is 0, so every chunk passes.
    for index in 0..3 {
        api.put_document(&document("notes", &format!("car-{index}"), "My car."))
            .await
            .unwrap();
    }
    server.wait_for_embeddings(3).await;

    let response = api.search(&query("vehicle")).await.unwrap();
    assert_eq!(response.total, 3);
    assert!(
        !response.total_is_lower_bound,
        "all of the scope's chunks came back"
    );

    // One more than the vector half's candidates.
    for index in 3..101 {
        api.put_document(&document("notes", &format!("car-{index}"), "My car."))
            .await
            .unwrap();
    }
    server.wait_for_embeddings(101).await;

    let response = api.search(&query("vehicle")).await.unwrap();
    assert!(response.total_is_lower_bound, "{}", response.total);
}
