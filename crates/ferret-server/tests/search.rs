mod common;

use common::{day, document, start_server};
use ferret_api::{Client, Hit, SearchQuery};
use reqwest::{Method, StatusCode, header};
use sqlx::PgPool;

async fn search(api: &Client, q: &str) -> Vec<Hit> {
    search_with(
        api,
        SearchQuery {
            q: q.to_owned(),
            ..Default::default()
        },
    )
    .await
}

async fn search_with(api: &Client, query: SearchQuery) -> Vec<Hit> {
    api.search(&query).await.unwrap().hits
}

fn ids(hits: &[Hit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.external_id.as_str()).collect()
}

fn highlighted(hit: &Hit) -> Vec<&str> {
    hit.chunk
        .parts
        .iter()
        .filter(|part| part.highlight)
        .map(|part| part.text.as_str())
        .collect()
}

#[sqlx::test(migrations = false)]
async fn finds_a_document_by_keyword_with_its_best_chunk_highlighted(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let filler = "Nothing to see here. ".repeat(80);
    let text = format!(
        "{filler}\n\nThe invoice RE-2026-0042 is due.\n\n{filler}\n\nAnother invoice line."
    );
    api.put_document(&document("paperless", "invoice", &text))
        .await
        .unwrap();
    api.put_document(&document("notes", "other", "No match in here."))
        .await
        .unwrap();

    let hits = search(&api, "invoice").await;

    assert_eq!(ids(&hits), ["invoice"], "one hit per document");
    let hit = &hits[0];
    assert_eq!(highlighted(hit), ["invoice"]);
    let chunk: String = hit
        .chunk
        .parts
        .iter()
        .map(|part| part.text.as_str())
        .collect();
    let at_offsets: String = text
        .chars()
        .skip(hit.chunk.start_offset as usize)
        .take((hit.chunk.end_offset - hit.chunk.start_offset) as usize)
        .collect();
    assert_eq!(chunk, at_offsets);
    assert!(hit.score > 0.0);
}

#[sqlx::test(migrations = false)]
async fn every_word_must_match_and_ids_and_numbers_are_found(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document(
        "paperless",
        "a",
        "Invoice RE-2026-0042 for the car",
    ))
    .await
    .unwrap();
    api.put_document(&document("paperless", "b", "Invoice for the bike"))
        .await
        .unwrap();

    assert_eq!(ids(&search(&api, "invoice car").await), ["a"]);
    assert_eq!(ids(&search(&api, "RE-2026-0042").await), ["a"]);
    assert_eq!(ids(&search(&api, "invoice").await).len(), 2);
}

#[sqlx::test(migrations = false)]
async fn exact_phrases_and_excluded_words(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "shoes", "Running shoes are great."))
        .await
        .unwrap();
    api.put_document(&document("notes", "reversed", "Shoes for running, great."))
        .await
        .unwrap();

    assert_eq!(ids(&search(&api, r#""running shoes""#).await), ["shoes"]);
    assert_eq!(ids(&search(&api, "shoes -great").await), Vec::<&str>::new());
    assert_eq!(
        ids(&search(&api, "running -reversed -for").await),
        ["shoes"]
    );
}

#[sqlx::test(migrations = false)]
async fn german_compounds_and_stems_in_both_languages(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document(
        "paperless",
        "kfz",
        "Ihr Kfz-Versicherungsbeitrag für 2026 beträgt 412 Euro.",
    ))
    .await
    .unwrap();
    api.put_document(&document(
        "paperless",
        "rechnung",
        "Die Rechnung ist bezahlt.",
    ))
    .await
    .unwrap();
    api.put_document(&document("notes", "runner", "She runs every morning."))
        .await
        .unwrap();

    let compound = search(&api, "Versicherung").await;
    assert_eq!(ids(&compound), ["kfz"]);
    assert_eq!(highlighted(&compound[0]), ["Versicherungsbeitrag"]);
    assert_eq!(ids(&search(&api, "Rechnungen").await), ["rechnung"]);
    assert_eq!(ids(&search(&api, "running").await), ["runner"]);
}

#[sqlx::test(migrations = false)]
async fn sources_filter_by_path_prefix_and_may_repeat(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    for source in ["notes", "notes/work", "notes-old", "mail/zoho"] {
        api.put_document(&document(source, source, "shared word"))
            .await
            .unwrap();
    }
    let in_sources = |sources: &[&str]| SearchQuery {
        q: "shared".to_owned(),
        sources: sources.iter().map(|source| source.to_string()).collect(),
        ..Default::default()
    };

    let mut notes = ids(&search_with(&api, in_sources(&["notes"])).await)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    notes.sort();
    let mut notes_and_mail = ids(&search_with(&api, in_sources(&["notes/work", "mail"])).await)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    notes_and_mail.sort();

    assert_eq!(notes, ["notes", "notes/work"]);
    assert_eq!(notes_and_mail, ["mail/zoho", "notes/work"]);
}

#[sqlx::test(migrations = false)]
async fn from_and_to_filter_on_created_at(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    for created in [1, 10, 20] {
        let mut note = document("notes", &format!("day {created}"), "dated note");
        note.created_at = day(created);
        note.modified_at = day(28);
        api.put_document(&note).await.unwrap();
    }

    let hits = search_with(
        &api,
        SearchQuery {
            q: "dated".to_owned(),
            from: Some(day(10)),
            to: Some(day(20)),
            ..Default::default()
        },
    )
    .await;

    assert_eq!(ids(&hits), ["day 10"]);
}

#[sqlx::test(migrations = false)]
async fn a_key_only_finds_its_own_users_documents_in_its_sources(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let admin = server.admin_api().await;
    for source in ["paperless", "notes"] {
        admin
            .put_document(&document(source, source, "shared word"))
            .await
            .unwrap();
    }
    let foreign = admin
        .put_document(&document("paperless", "foreign", "shared word"))
        .await
        .unwrap()
        .id;
    sqlx::query(
        "insert into users (id, username, password_hash, created_at, password_changed_at)
         values (gen_random_uuid(), 'other', 'x', now(), now())",
    )
    .execute(&pool)
    .await
    .unwrap();
    for move_to_other_user in [
        "update documents set user_id = (select id from users where username = 'other')
         where id = $1",
        "update chunks set user_id = (select id from users where username = 'other')
         where document_id = $1",
    ] {
        sqlx::query(move_to_other_user)
            .bind(foreign)
            .execute(&pool)
            .await
            .unwrap();
    }
    let paperless = server.api_with_key(
        &server
            .create_api_key(&["SEARCH_READ"], Some(&["paperless"]))
            .await,
    );

    assert_eq!(ids(&search(&paperless, "shared").await), ["paperless"]);
    let asking_for_notes = SearchQuery {
        q: "shared".to_owned(),
        sources: vec!["notes".to_owned()],
        ..Default::default()
    };
    assert!(search_with(&paperless, asking_for_notes).await.is_empty());
}

#[sqlx::test(migrations = false)]
async fn invalid_searches_are_rejected(pool: PgPool) {
    let server = start_server(pool).await;
    let key = server
        .create_api_key(&["SEARCH_READ", "INGEST_READ"], None)
        .await;
    let ingest_only = server.api_with_key(&server.create_api_key(&["INGEST_WRITE"], None).await);

    for query in [
        "q=-only+-excluded",
        "q=word&k=0",
        "q=word&k=101",
        "q=word&source=/notes",
        "k=5",
        "q=word&from=yesterday",
    ] {
        let response = server
            .request(Method::GET, &format!("/api/search?{query}"))
            .header(header::AUTHORIZATION, format!("Bearer {key}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "{query}");
        assert!(
            common::json_body(response).await["error"].is_string(),
            "{query}"
        );
    }
    let forbidden = ingest_only
        .search(&SearchQuery {
            q: "word".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(forbidden.status(), Some(StatusCode::FORBIDDEN));
}
