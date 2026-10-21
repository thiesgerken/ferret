mod common;

use common::{TestServer, document, start_server};
use serde_json::{Value, json};
use sqlx::PgPool;

const SEARCH: &str = "query ($input: SearchInput!) {
    search(input: $input) { hits { documentId judged } }
}";
const SET_JUDGMENT: &str = "mutation ($input: JudgmentInput!) { setJudgment(input: $input) }";

/// `documentId → judged` for the hits of a search.
async fn judged(server: &TestServer, cookie: &str, search: Value) -> Vec<(String, bool)> {
    let body = server
        .graphql(cookie, SEARCH, json!({"input": search}))
        .await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    body["data"]["search"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| {
            (
                hit["documentId"].as_str().unwrap().to_owned(),
                hit["judged"].as_bool().unwrap(),
            )
        })
        .collect()
}

async fn set_judgment(
    server: &TestServer,
    cookie: &str,
    search: Value,
    document_id: &str,
    judged: bool,
) -> Value {
    server
        .graphql(
            cookie,
            SET_JUDGMENT,
            json!({"input": {"search": search, "documentId": document_id, "judged": judged}}),
        )
        .await
}

async fn judgments(pool: &PgPool) -> i64 {
    sqlx::query_scalar("select count(*) from relevance_judgments")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = false)]
async fn a_judgment_belongs_to_its_query_and_filters_and_can_be_taken_back(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let invoice = api
        .put_document(&document("paperless", "invoice", "The invoice is paid."))
        .await
        .unwrap()
        .id()
        .unwrap()
        .to_string();
    api.put_document(&document("notes", "note", "An invoice to pay."))
        .await
        .unwrap();
    let cookie = server.sign_in().await;
    let search = json!({"q": "invoice", "sources": ["paperless", "notes"]});

    let set = set_judgment(&server, &cookie, search.clone(), &invoice, true).await;
    // Twice is still one judgment: an explicit set, not a toggle.
    set_judgment(&server, &cookie, search.clone(), &invoice, true).await;

    assert_eq!(set["data"]["setJudgment"], true, "{set}");
    assert_eq!(judgments(&pool).await, 1);
    let mut hits = judged(&server, &cookie, search.clone()).await;
    hits.sort_by_key(|(_, judged)| !judged);
    assert_eq!(hits[0], (invoice.clone(), true));
    assert!(!hits[1].1);
    // The same filters in another order are the same search.
    let reordered = json!({"q": " invoice ", "sources": ["notes", "paperless"]});
    assert!(
        judged(&server, &cookie, reordered)
            .await
            .contains(&(invoice.clone(), true))
    );
    // Other filters or another query are another search.
    for other in [
        json!({"q": "invoice"}),
        json!({"q": "paid invoice", "sources": ["paperless", "notes"]}),
    ] {
        assert!(
            judged(&server, &cookie, other)
                .await
                .iter()
                .all(|(_, judged)| !judged)
        );
    }

    let unset = set_judgment(&server, &cookie, search.clone(), &invoice, false).await;
    assert_eq!(unset["data"]["setJudgment"], false, "{unset}");
    assert_eq!(judgments(&pool).await, 0);
}

#[sqlx::test(migrations = false)]
async fn judgments_disappear_with_their_document(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let id = api
        .put_document(&document("notes", "a", "An invoice."))
        .await
        .unwrap()
        .id()
        .unwrap()
        .to_string();
    let cookie = server.sign_in().await;
    set_judgment(&server, &cookie, json!({"q": "invoice"}), &id, true).await;
    assert_eq!(judgments(&pool).await, 1);

    api.delete_document("notes", "a").await.unwrap();

    assert_eq!(judgments(&pool).await, 0);
}

#[sqlx::test(migrations = false)]
async fn only_ones_own_documents_can_be_judged(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let id = api
        .put_document(&document("notes", "a", "An invoice."))
        .await
        .unwrap()
        .id()
        .unwrap();
    sqlx::query(
        "insert into users (id, username, password_hash, created_at, password_changed_at)
         values (gen_random_uuid(), 'other', 'x', now(), now())",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("update documents set user_id = (select id from users where username = 'other') where id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let cookie = server.sign_in().await;

    for document_id in [id.to_string(), "not-a-uuid".to_owned()] {
        let body = set_judgment(
            &server,
            &cookie,
            json!({"q": "invoice"}),
            &document_id,
            true,
        )
        .await;
        assert_eq!(body["errors"][0]["message"], "no such document", "{body}");
    }
    assert_eq!(judgments(&pool).await, 0);
}

/// Another user's judgment of the same search and document neither shows nor goes away.
#[sqlx::test(migrations = false)]
async fn judgments_are_per_user(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let id = api
        .put_document(&document("notes", "a", "An invoice."))
        .await
        .unwrap()
        .id()
        .unwrap();
    // No user management yet, so the other user and their judgment come straight from SQL.
    sqlx::query(
        r#"with bob as (
               insert into users (id, username, password_hash, created_at, password_changed_at)
               values (uuidv7(), 'bob', 'unused', now(), now())
               returning id
           )
           insert into relevance_judgments (id, user_id, query, filters, document_id, created_at)
           select uuidv7(), bob.id, 'invoice',
                  '{"sources": [], "authors": [], "content_types": [], "from": null, "to": null, "range": null}',
                  $1, now()
           from bob"#,
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let cookie = server.sign_in().await;
    let search = json!({"q": "invoice"});

    assert_eq!(
        judged(&server, &cookie, search.clone()).await,
        [(id.to_string(), false)]
    );
    set_judgment(&server, &cookie, search.clone(), &id.to_string(), false).await;
    assert_eq!(judgments(&pool).await, 1, "bob's judgment stays");
    set_judgment(&server, &cookie, search.clone(), &id.to_string(), true).await;
    assert_eq!(judgments(&pool).await, 2);
    assert_eq!(
        judged(&server, &cookie, search).await,
        [(id.to_string(), true)]
    );
}

#[sqlx::test(migrations = false)]
async fn whitespace_doesnt_split_a_search_and_unanswerable_searches_are_refused(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let id = api
        .put_document(&document("notes", "a", "A paid invoice."))
        .await
        .unwrap()
        .id()
        .unwrap()
        .to_string();
    let cookie = server.sign_in().await;

    set_judgment(&server, &cookie, json!({"q": "paid  invoice"}), &id, true).await;
    let refused = set_judgment(&server, &cookie, json!({"q": "-invoice"}), &id, true).await;

    assert_eq!(
        judged(&server, &cookie, json!({"q": " paid invoice"})).await,
        [(id.clone(), true)]
    );
    assert_eq!(
        refused["errors"][0]["message"],
        "the query needs at least one word that isn't excluded"
    );
    assert_eq!(judgments(&pool).await, 1);
}

/// "Past week" resolves to a different `from` every moment. The judgment stores the preset, so
/// the same search later still finds it and a second ✓ doesn't add a row.
#[sqlx::test(migrations = false)]
async fn relative_ranges_are_judged_as_presets(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let mut recent = document("notes", "a", "A recent invoice.");
    recent.created_at = Some(chrono::Utc::now());
    let id = api
        .put_document(&recent)
        .await
        .unwrap()
        .id()
        .unwrap()
        .to_string();
    let cookie = server.sign_in().await;
    let past_week = json!({"q": "invoice", "range": "WEEK"});

    set_judgment(&server, &cookie, past_week.clone(), &id, true).await;
    set_judgment(&server, &cookie, past_week.clone(), &id, true).await;
    let both = set_judgment(
        &server,
        &cookie,
        json!({"q": "invoice", "range": "WEEK", "from": "2026-01-01T00:00:00Z"}),
        &id,
        true,
    )
    .await;

    let filters: Value = sqlx::query_scalar("select filters from relevance_judgments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(filters["range"], "week");
    assert_eq!(filters["from"], Value::Null, "no moment-dependent from");
    assert_eq!(judgments(&pool).await, 1);
    assert_eq!(
        judged(&server, &cookie, past_week).await,
        [(id.clone(), true)]
    );
    assert_eq!(both["errors"][0]["message"], "set either from or range");
}
