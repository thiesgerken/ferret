//! The workspace's root doc: the list of its docs (`meta.pages`), with their titles, when they
//! were created and whether they're in the trash. The trash lives only here, not on the server.

use chrono::{DateTime, Utc};
use yrs::types::ToJson;
use yrs::updates::decoder::Decode;
use yrs::{Any, Doc, Map, Transact, Update};

use crate::plugins::PluginError;

#[derive(Debug, PartialEq)]
pub struct DocMeta {
    pub id: String,
    pub title: String,
    pub created_at: Option<DateTime<Utc>>,
    pub trashed: bool,
}

pub fn docs(update: &[u8]) -> Result<Vec<DocMeta>, PluginError> {
    let doc = Doc::new();
    let meta = doc.get_or_insert_map("meta");
    let mut txn = doc.transact_mut();
    txn.apply_update(Update::decode_v1(update)?)?;
    let Some(Any::Array(pages)) = meta.get(&txn, "pages").map(|pages| pages.to_json(&txn)) else {
        return Err("the workspace's root doc lists no docs".into());
    };
    pages
        .iter()
        .map(|page| {
            let Any::Map(page) = page else {
                return Err("a doc in the workspace's root doc isn't a map".into());
            };
            let Some(Any::String(id)) = page.get("id") else {
                return Err("a doc in the workspace's root doc has no id".into());
            };
            Ok(DocMeta {
                id: id.to_string(),
                title: match page.get("title") {
                    Some(Any::String(title)) => title.trim().to_owned(),
                    _ => String::new(),
                },
                created_at: match page.get("createDate") {
                    Some(Any::Number(millis)) => {
                        millis.as_i64().and_then(DateTime::from_timestamp_millis)
                    }
                    _ => None,
                },
                trashed: page.get("trash") == Some(&Any::Bool(true)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_doc_lists_every_doc_and_marks_the_trashed_ones() {
        let docs = docs(include_bytes!("../../../fixtures/affine/workspace.bin")).unwrap();

        let live: Vec<(&str, &str)> = docs
            .iter()
            .filter(|doc| !doc.trashed)
            .map(|doc| (doc.id.as_str(), doc.title.as_str()))
            .collect();
        assert_eq!(
            live,
            [
                ("DhJn1OxcrD", "Getting Started ferret was here."),
                ("hGhAvlHvXN", "ferret-linked"),
                ("vCfzul9rpm", "ferret-fixture"),
            ]
        );
        assert_eq!(docs.iter().filter(|doc| doc.trashed).count(), 3);
        let fixture = docs.iter().find(|doc| doc.id == "vCfzul9rpm").unwrap();
        assert_eq!(
            fixture.created_at.unwrap().to_rfc3339(),
            "2026-10-17T18:08:21.556+00:00"
        );
    }
}
