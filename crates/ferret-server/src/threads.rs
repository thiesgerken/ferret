//! Source-local thread filters and selected facet labels, shared by search and Explore.

use ferret_api::{ThreadCount, ThreadIdentity};
use sqlx::{PgConnection, Postgres, QueryBuilder, types::Json as Jsonb};
use uuid::Uuid;

use crate::sources;

pub fn validate(threads: &[ThreadIdentity]) -> Result<(), String> {
    for thread in threads {
        if !sources::is_valid(&thread.source) {
            return Err(format!(
                "{:?} is not a source path like `notes` or `mail/personal`",
                thread.source
            ));
        }
        if thread.thread.contains('\0') {
            return Err("thread must not contain U+0000".to_owned());
        }
    }
    Ok(())
}

/// A predicate over `documents d`. Pairing both fields avoids cross-source thread matches.
pub fn push_filter(sql: &mut QueryBuilder<Postgres>, threads: &[ThreadIdentity]) {
    sql.push("(d.source, d.thread) in (");
    sql.push_values(threads, |mut row, thread| {
        row.push_bind(&thread.source).push_bind(&thread.thread);
    });
    sql.push(")");
}

/// Keep zero-count selections removable and named after reloads, without exposing titles outside
/// the credential's readable roots. Other filters deliberately don't hide a selected label.
pub async fn complete_counts(
    connection: &mut PgConnection,
    user_id: Uuid,
    allowed: Option<&[String]>,
    selected: &[ThreadIdentity],
    counts: &mut Vec<ThreadCount>,
) -> Result<(), sqlx::Error> {
    let missing: Vec<_> = selected
        .iter()
        .filter(|thread| {
            !counts
                .iter()
                .any(|count| count.source == thread.source && count.thread == thread.thread)
        })
        .collect();
    if !missing.is_empty() {
        let labels = sqlx::query!(
            r#"select picked.source as "source!", picked.thread as "thread!",
                      (select d.title from documents d
                       where d.user_id = $1 and d.source = picked.source and d.thread = picked.thread
                         and d.title is not null and d.title != ''
                       order by d.created_at desc nulls last, d.id desc limit 1) as title
               from jsonb_to_recordset($2) picked(source text, thread text)
               where $3::text[] is null or exists (
                   select 1 from unnest($3::text[]) root where source_covers(root, picked.source))"#,
            user_id, Jsonb(&missing) as _, allowed,
        ).fetch_all(connection).await?;
        for label in labels {
            counts.push(ThreadCount {
                source: label.source,
                thread: label.thread,
                title: label.title,
                count: 0,
            });
        }
    }
    counts.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.thread.cmp(&b.thread))
    });
    Ok(())
}
