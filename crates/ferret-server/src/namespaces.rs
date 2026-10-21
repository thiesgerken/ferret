//! Non-destructive identity relocation for a connection changing its source layout.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use ferret_api::{DocumentPut, NamespaceFlatten, NamespaceFlattened};
use sqlx::types::Json as Jsonb;
use uuid::Uuid;

use crate::{
    AppState,
    api_auth::{ApiCaller, CallerKind},
    content_type, documents,
    error::ApiError,
    exclusions::{self, SourceLock},
    sources,
};

pub async fn flatten(
    State(state): State<AppState>,
    caller: ApiCaller,
    body: Result<Json<NamespaceFlatten>, JsonRejection>,
) -> Result<Json<NamespaceFlattened>, ApiError> {
    let CallerKind::PluginRun(run) = caller.kind() else {
        return Err(ApiError::forbidden(
            "namespace relocation requires a live plugin run",
        ));
    };
    let Json(body) = body?;
    if !sources::is_valid(&body.from_source_prefix)
        || !sources::is_valid(&body.to_source)
        || sources::covers(&body.from_source_prefix, &body.to_source)
    {
        return Err(ApiError::bad_request(
            "the source prefix and destination must be valid paths, with the destination outside the prefix",
        ));
    }
    caller.require_source(&body.from_source_prefix)?;
    caller.require_source(&body.to_source)?;
    let [root] = caller
        .sources()
        .expect("plugin run tokens have source roots")
    else {
        unreachable!("a plugin run owns one source root");
    };
    let mut tx = state.pool.begin().await?;
    // Writers take this shared before their source/row locks. Taking it first also avoids
    // deadlocking with PUT's activity update, which locks the run row last.
    exclusions::lock_source(&mut tx, caller.user_id, root, SourceLock::Exclusive).await?;
    let connection = sqlx::query_scalar!(
        "select c.id from plugin_runs r join connections c on c.id = r.connection_id
         where r.id = $1 and r.status = 'running' and c.user_id = $2 and c.source = $3
           and r.heartbeat_at >= statement_timestamp() - interval '120 seconds'",
        run,
        caller.user_id,
        root,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::conflict("the run isn't running anymore"))?;
    let affected = sqlx::query_scalar!(
        "select source as \"source!\" from (
             select source from documents where user_id = $1 and source_covers($2, source)
             union select source from exclusions where connection_id = $3 and source_covers($2, source)
             union select $4::text as source
         ) affected order by source",
        caller.user_id, body.from_source_prefix, connection, body.to_source,
    ).fetch_all(&mut *tx).await?;
    for source in affected {
        if source != *root {
            exclusions::lock_source(&mut tx, caller.user_id, &source, SourceLock::Exclusive)
                .await?;
        }
    }
    let collision = sqlx::query_scalar!(
        r#"with moved as (
               select id, thread,
                      case when source = $2 then external_id
                           else substr(source, length($2) + 2) || '/' || external_id end as external_id
               from documents where user_id = $1 and source_covers($2, source)
           ), protected as (
               select thread,
                      case when external_id is null then null
                           when source = $2 or source = $3 then external_id
                           else substr(source, length($2) + 2) || '/' || external_id end as external_id
               from exclusions where connection_id = $4
                 and (source_covers($2, source) or source = $3)
           )
           select (exists (select 1 from moved group by external_id having count(*) > 1)
               or exists (select 1 from moved m join documents d
                   on d.user_id = $1 and d.source = $3 and d.external_id = m.external_id)
               or exists (select 1 from moved m join protected p
                   on p.external_id = m.external_id or p.thread = m.thread)) as "collision!""#,
        caller.user_id, body.from_source_prefix, body.to_source, connection,
    ).fetch_one(&mut *tx).await?;
    if collision {
        return Err(ApiError::conflict(
            "namespace relocation has colliding document or exclusion identities; nothing was moved",
        ));
    }
    let mut documents_moved = 0;
    let mut after: Option<Uuid> = None;
    loop {
        let page = sqlx::query!(
            r#"select id, source, external_id, content_type, title, text, url, thread, authors,
                      created_at, modified_at,
                      meta as "meta: Jsonb<serde_json::Map<String, serde_json::Value>>"
               from documents where user_id = $1 and source_covers($2, source)
                 and ($3::uuid is null or id > $3)
               order by id limit 32"#,
            caller.user_id,
            body.from_source_prefix,
            after,
        )
        .fetch_all(&mut *tx)
        .await?;
        if page.is_empty() {
            break;
        }
        let mut ids = Vec::with_capacity(page.len());
        let mut external_ids = Vec::with_capacity(page.len());
        let mut hashes = Vec::with_capacity(page.len());
        for row in page {
            let external_id = if row.source == body.from_source_prefix {
                row.external_id
            } else {
                let relative = &row.source[body.from_source_prefix.len() + 1..];
                format!("{relative}/{}", row.external_id)
            };
            let document = DocumentPut {
                source: body.to_source.clone(),
                external_id,
                content_type: content_type::parse(&row.content_type),
                title: row.title,
                text: row.text,
                url: row.url,
                thread: row.thread,
                authors: row.authors,
                created_at: row.created_at,
                modified_at: row.modified_at,
                meta: row.meta.0,
            };
            ids.push(row.id);
            hashes.push(documents::content_hash(&document));
            external_ids.push(document.external_id);
        }
        let hash_refs: Vec<&[u8]> = hashes.iter().map(|hash| hash.as_slice()).collect();
        documents_moved += sqlx::query!(
            "update documents d set source = $1, external_id = moved.external_id,
                 content_hash = moved.content_hash
             from unnest($2::uuid[], $3::text[], $4::bytea[]) moved(id, external_id, content_hash)
             where d.id = moved.id",
            body.to_source,
            &ids,
            &external_ids,
            &hash_refs as &[&[u8]],
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        after = ids.last().copied();
    }
    let exclusions_moved = sqlx::query!(
        "update exclusions set
             external_id = case when external_id is null then null
                 when source = $2 then external_id
                 else substr(source, length($2) + 2) || '/' || external_id end,
             source = $3
         where connection_id = $1 and source_covers($2, source)",
        connection,
        body.from_source_prefix,
        body.to_source,
    )
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|error| error.is_unique_violation())
        {
            ApiError::conflict(
                "namespace relocation has colliding exclusion identities; nothing was moved",
            )
        } else {
            error.into()
        }
    })?
    .rows_affected();
    sqlx::query!(
        "update chunks set source = $3 where user_id = $1 and source_covers($2, source)",
        caller.user_id,
        body.from_source_prefix,
        body.to_source,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "update chunk_embeddings set source = $3 where user_id = $1 and source_covers($2, source)",
        caller.user_id,
        body.from_source_prefix,
        body.to_source,
    )
    .execute(&mut *tx)
    .await?;
    // Rehashing can outlast a lease interval. Heartbeats must remain able to update the run.
    sqlx::query_scalar!(
        "select c.id from plugin_runs r join connections c on c.id = r.connection_id
         where r.id = $1 and r.status = 'running' and c.user_id = $2 and c.source = $3
           and r.heartbeat_at >= statement_timestamp() - interval '120 seconds'
         for update of r",
        run,
        caller.user_id,
        root,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::conflict("the run isn't running anymore; nothing was moved"))?;
    tx.commit().await?;
    Ok(Json(NamespaceFlattened {
        documents: documents_moved,
        exclusions: exclusions_moved,
    }))
}
