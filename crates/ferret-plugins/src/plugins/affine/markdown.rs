//! An AFFiNE doc's Yjs state as Markdown. A doc is a map of blocks (`blocks`), each with a
//! `sys:flavour`, its `sys:children` and its `prop:*` properties, rooted in one `affine:page`.
//! Rich text is a Y.Text whose formatting lives in the attributes of its deltas.

use std::collections::{BTreeSet, HashMap};

use chrono::DateTime;
use yrs::types::ToJson;
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{Any, Array, Doc, GetString, Map, MapRef, Out, ReadTxn, Text, Transact, Update};

use crate::plugins::PluginError;

/// A doc as ferret indexes it.
#[derive(Debug, PartialEq)]
pub struct Page {
    /// The page's own title, empty for an untitled page.
    pub title: String,
    pub markdown: String,
    /// Flavours this converter doesn't know, for the run log. Their text, if any, is kept.
    pub unknown: BTreeSet<String>,
}

/// Where linked docs point: their titles, and the address a doc id is appended to.
pub struct Links<'a> {
    pub titles: &'a HashMap<String, String>,
    /// `{public_url}/workspace/{workspace}`.
    pub base: &'a str,
}

impl Links<'_> {
    pub fn url(&self, doc_id: &str) -> String {
        format!("{}/{doc_id}", self.base)
    }

    fn markdown(&self, doc_id: &str) -> String {
        let title = self
            .titles
            .get(doc_id)
            .map(String::as_str)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or("Untitled");
        format!("[{}]({})", title.trim(), self.url(doc_id))
    }
}

pub fn to_markdown(update: &[u8], links: &Links) -> Result<Page, PluginError> {
    let doc = Doc::new();
    let blocks = doc.get_or_insert_map("blocks");
    {
        let mut txn = doc.transact_mut();
        txn.apply_update(Update::decode_v1(update)?)?;
    }
    let txn = doc.transact();
    let mut renderer = Renderer {
        txn: &txn,
        blocks: &blocks,
        links,
        unknown: BTreeSet::new(),
    };
    let page = blocks
        .iter(&txn)
        .filter_map(|(_, block)| match block {
            Out::YMap(block) => Some(block),
            _ => None,
        })
        .find(|block| renderer.flavour(block) == "affine:page")
        .ok_or("the doc has no page block")?;
    let title = renderer.plain(&page, "prop:title");
    let mut chunks = Vec::new();
    let children = renderer.children(&page);
    // The canvas comes last: in page mode it isn't shown at all.
    let (surfaces, flow): (Vec<_>, Vec<_>) = children.iter().partition(|id| {
        renderer
            .block(id)
            .is_some_and(|block| renderer.flavour(&block) == "affine:surface")
    });
    renderer.render(&flow, 0, &mut chunks);
    renderer.render(&surfaces, 0, &mut chunks);
    Ok(Page {
        title,
        markdown: join(&chunks),
        unknown: renderer.unknown,
    })
}

/// A rendered block. Consecutive list items stay together, everything else is a paragraph.
struct Chunk {
    text: String,
    list_item: bool,
}

fn join(chunks: &[Chunk]) -> String {
    let mut markdown = String::new();
    for (index, chunk) in chunks.iter().enumerate() {
        if index > 0 {
            let tight = chunk.list_item && chunks[index - 1].list_item;
            markdown.push_str(if tight { "\n" } else { "\n\n" });
        }
        markdown.push_str(&chunk.text);
    }
    markdown
}

struct Renderer<'a, T: ReadTxn> {
    txn: &'a T,
    blocks: &'a MapRef,
    links: &'a Links<'a>,
    unknown: BTreeSet<String>,
}

impl<T: ReadTxn> Renderer<'_, T> {
    fn block(&self, id: &str) -> Option<MapRef> {
        match self.blocks.get(self.txn, id) {
            Some(Out::YMap(block)) => Some(block),
            _ => None,
        }
    }

    fn flavour(&self, block: &MapRef) -> String {
        self.string(block, "sys:flavour").unwrap_or_default()
    }

    fn children(&self, block: &MapRef) -> Vec<String> {
        match block.get(self.txn, "sys:children") {
            Some(Out::YArray(children)) => children
                .iter(self.txn)
                .filter_map(|child| match child {
                    Out::Any(Any::String(id)) => Some(id.to_string()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn string(&self, block: &MapRef, key: &str) -> Option<String> {
        match block.get(self.txn, key) {
            Some(Out::Any(Any::String(value))) => Some(value.to_string()),
            _ => None,
        }
    }

    /// A text property without its formatting.
    fn plain(&self, block: &MapRef, key: &str) -> String {
        match block.get(self.txn, key) {
            Some(Out::YText(text)) => text.get_string(self.txn),
            Some(Out::Any(Any::String(text))) => text.to_string(),
            _ => String::new(),
        }
    }

    /// A text property as inline Markdown.
    fn inline(&self, block: &MapRef, key: &str) -> String {
        let Some(Out::YText(text)) = block.get(self.txn, key) else {
            return self.plain(block, key);
        };
        let mut markdown = String::new();
        for delta in text.diff(self.txn, YChange::identity) {
            let Out::Any(Any::String(insert)) = delta.insert else {
                continue;
            };
            let attribute = |name: &str| {
                delta
                    .attributes
                    .as_ref()
                    .and_then(|attributes| attributes.get(name))
            };
            // A reference or an inline equation stands on a placeholder character.
            if let Some(Any::Map(reference)) = attribute("reference")
                && let Some(Any::String(doc_id)) = reference.get("pageId")
            {
                markdown.push_str(&self.links.markdown(doc_id));
                continue;
            }
            if let Some(Any::String(latex)) = attribute("latex") {
                markdown.push_str(&format!("${latex}$"));
                continue;
            }
            let mut piece = insert.to_string();
            if attribute("code") == Some(&Any::Bool(true)) {
                piece = format!("`{piece}`");
            }
            for (name, marker) in [("bold", "**"), ("italic", "_"), ("strike", "~~")] {
                if attribute(name) == Some(&Any::Bool(true)) {
                    piece = format!("{marker}{piece}{marker}");
                }
            }
            if let Some(Any::String(url)) = attribute("link") {
                piece = format!("[{piece}]({url})");
            }
            markdown.push_str(&piece);
        }
        markdown
    }

    fn render(&mut self, ids: &[&String], depth: usize, chunks: &mut Vec<Chunk>) {
        let mut number = 0;
        for id in ids {
            let Some(block) = self.block(id) else {
                continue;
            };
            let flavour = self.flavour(&block);
            let numbered = flavour == "affine:list"
                && self.string(&block, "prop:type").as_deref() == Some("numbered");
            number = if numbered { number + 1 } else { 0 };
            let (text, list_item) = self.text(&block, &flavour, number);
            if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
                chunks.push(Chunk {
                    text: indent(&text, depth),
                    list_item,
                });
            }
            // A database's children are its rows, which it renders itself.
            if flavour == "affine:database" {
                continue;
            }
            let children = self.children(&block);
            let children: Vec<&String> = children.iter().collect();
            // Containers hold their children at their own level, other blocks nest them.
            let nested = if is_container(&flavour) {
                depth
            } else {
                depth + 1
            };
            self.render(&children, nested, chunks);
        }
    }

    /// A block's own Markdown, without its children, and whether it's a list item.
    fn text(&mut self, block: &MapRef, flavour: &str, number: usize) -> (Option<String>, bool) {
        let kind = self.string(block, "prop:type").unwrap_or_default();
        let text = match flavour {
            "affine:paragraph" => {
                let text = self.inline(block, "prop:text");
                match kind.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        let level = kind[1..].parse().expect("h1 to h6");
                        format!("{} {text}", "#".repeat(level))
                    }
                    "quote" => text
                        .lines()
                        .map(|line| format!("> {line}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => text,
                }
            }
            "affine:list" => {
                let text = self.inline(block, "prop:text");
                let marker = match kind.as_str() {
                    "numbered" => format!("{number}."),
                    "todo"
                        if block.get(self.txn, "prop:checked")
                            == Some(Out::Any(Any::Bool(true))) =>
                    {
                        "- [x]".to_owned()
                    }
                    "todo" => "- [ ]".to_owned(),
                    _ => "-".to_owned(),
                };
                return (
                    Some(format!("{marker} {}", text.replace('\n', "\n  "))),
                    true,
                );
            }
            "affine:code" => {
                let language = self.string(block, "prop:language").unwrap_or_default();
                format!("```{language}\n{}\n```", self.plain(block, "prop:text"))
            }
            "affine:divider" => "---".to_owned(),
            "affine:latex" => format!("$$\n{}\n$$", self.plain(block, "prop:latex")),
            "affine:table" => self.table(block),
            "affine:database" => self.database(block),
            "affine:embed-linked-doc" | "affine:embed-synced-doc" => {
                match self.string(block, "prop:pageId") {
                    Some(doc_id) => self.links.markdown(&doc_id),
                    None => String::new(),
                }
            }
            "affine:image" => self.plain(block, "prop:caption"),
            "affine:attachment" => self.plain(block, "prop:name"),
            "affine:bookmark" => self.link(block),
            flavour if flavour.starts_with("affine:embed-") => self.link(block),
            // Containers render through their children, a frame only groups blocks on the canvas.
            flavour if is_container(flavour) || flavour == "affine:frame" => return (None, false),
            flavour => {
                self.unknown.insert(flavour.to_owned());
                self.inline(block, "prop:text")
            }
        };
        (Some(text), false)
    }

    fn link(&self, block: &MapRef) -> String {
        let Some(url) = self.string(block, "prop:url").filter(|url| !url.is_empty()) else {
            return String::new();
        };
        match self
            .string(block, "prop:title")
            .filter(|title| !title.is_empty())
        {
            Some(title) => format!("[{title}]({url})"),
            None => format!("<{url}>"),
        }
    }

    /// A table block keeps its rows, columns and cells as flattened properties:
    /// `prop:rows.{row}.order`, `prop:columns.{column}.order` and `prop:cells.{row}:{column}.text`.
    fn table(&self, block: &MapRef) -> String {
        let mut rows = Vec::new();
        let mut columns = Vec::new();
        for (key, value) in block.iter(self.txn) {
            let Out::Any(Any::String(order)) = value else {
                continue;
            };
            if let Some(row) = key
                .strip_prefix("prop:rows.")
                .and_then(|key| key.strip_suffix(".order"))
            {
                rows.push((order.to_string(), row.to_owned()));
            } else if let Some(column) = key
                .strip_prefix("prop:columns.")
                .and_then(|key| key.strip_suffix(".order"))
            {
                columns.push((order.to_string(), column.to_owned()));
            }
        }
        // Orders are fractional indexes, which sort as strings.
        rows.sort();
        columns.sort();
        let cells: Vec<Vec<String>> = rows
            .iter()
            .map(|(_, row)| {
                columns
                    .iter()
                    .map(|(_, column)| {
                        self.plain(block, &format!("prop:cells.{row}:{column}.text"))
                    })
                    .collect()
            })
            .collect();
        markdown_table(&cells)
    }

    /// A database: its title, then a table of its rows (child blocks, titled by their text) and
    /// their cells (`prop:cells.{row}.{column}.value`) under the columns' names.
    fn database(&self, block: &MapRef) -> String {
        let title = self.plain(block, "prop:title");
        let columns = match block
            .get(self.txn, "prop:columns")
            .map(|columns| columns.to_json(self.txn))
        {
            Some(Any::Array(columns)) => columns.to_vec(),
            _ => Vec::new(),
        };
        let cells = match block
            .get(self.txn, "prop:cells")
            .map(|cells| cells.to_json(self.txn))
        {
            Some(Any::Map(cells)) => cells,
            _ => Default::default(),
        };
        let mut table = vec![
            std::iter::once("Title".to_owned())
                .chain(
                    columns
                        .iter()
                        .map(|column| field(column, "name").unwrap_or_default()),
                )
                .collect::<Vec<_>>(),
        ];
        for row in self.children(block) {
            let Some(row_block) = self.block(&row) else {
                continue;
            };
            let values: Vec<String> = columns
                .iter()
                .map(|column| {
                    let value = match (&cells.get(&row), field(column, "id")) {
                        (Some(Any::Map(row)), Some(id)) => match row.get(&id) {
                            Some(Any::Map(cell)) => cell.get("value").cloned(),
                            _ => None,
                        },
                        _ => None,
                    };
                    value
                        .map(|value| cell_text(column, &value))
                        .unwrap_or_default()
                })
                .collect();
            let row_title = self.inline(&row_block, "prop:text");
            if row_title.trim().is_empty() && values.iter().all(String::is_empty) {
                continue;
            }
            table.push(std::iter::once(row_title).chain(values).collect());
        }
        let title = if title.trim().is_empty() {
            None
        } else {
            Some(format!("**{}**", title.trim()))
        };
        let table = (table.len() > 1).then(|| markdown_table(&table));
        [title, table]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Blocks that only hold others: their children render at their level.
fn is_container(flavour: &str) -> bool {
    matches!(
        flavour,
        "affine:note" | "affine:surface" | "affine:edgeless-text" | "affine:callout"
    )
}

fn indent(text: &str, depth: usize) -> String {
    let prefix = "    ".repeat(depth);
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn field(object: &Any, name: &str) -> Option<String> {
    match object {
        Any::Map(map) => match map.get(name) {
            Some(Any::String(value)) => Some(value.to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// A database cell as text. Selects store option ids, which the column names.
fn cell_text(column: &Any, value: &Any) -> String {
    let options: HashMap<String, String> = match column {
        Any::Map(column) => match column.get("data") {
            Some(Any::Map(data)) => match data.get("options") {
                Some(Any::Array(options)) => options
                    .iter()
                    .filter_map(|option| Some((field(option, "id")?, field(option, "value")?)))
                    .collect(),
                _ => HashMap::new(),
            },
            _ => HashMap::new(),
        },
        _ => HashMap::new(),
    };
    let kind = field(column, "type").unwrap_or_default();
    match value {
        Any::String(id) if !options.is_empty() => {
            options.get(id.as_ref()).cloned().unwrap_or_default()
        }
        Any::Array(ids) => ids
            .iter()
            .filter_map(|id| match id {
                Any::String(id) => options
                    .get(id.as_ref())
                    .cloned()
                    .or_else(|| Some(id.to_string())),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", "),
        Any::Number(millis) if kind == "date" => millis
            .as_i64()
            .and_then(DateTime::from_timestamp_millis)
            .map(|date| date.date_naive().to_string())
            .unwrap_or_default(),
        Any::Bool(true) => "yes".to_owned(),
        Any::Bool(false) | Any::Null | Any::Undefined => String::new(),
        Any::String(text) => text.to_string(),
        other => {
            let mut text = String::new();
            other.to_json(&mut text);
            text
        }
    }
}

/// A Markdown table with the first row as its header.
fn markdown_table(rows: &[Vec<String>]) -> String {
    let Some(header) = rows.first() else {
        return String::new();
    };
    let line = |cells: &[String]| {
        let cells: Vec<String> = cells
            .iter()
            .map(|cell| cell.replace('|', "\\|").replace('\n', " "))
            .collect();
        format!("| {} |", cells.join(" | "))
    };
    let mut lines = vec![
        line(header),
        format!("|{}|", vec!["---"; header.len()].join("|")),
    ];
    lines.extend(rows[1..].iter().map(|row| line(row)));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use yrs::{ArrayPrelim, In, MapPrelim, StateVector, TextPrelim};

    use super::*;

    /// A doc of blocks, each given as its id and properties (its flavour first). `top` are the
    /// ids under the page's note.
    fn doc(top: &[&str], blocks: Vec<(&str, Vec<(&str, In)>)>) -> Vec<u8> {
        let doc = Doc::new();
        let map = doc.get_or_insert_map("blocks");
        let mut txn = doc.transact_mut();
        let ids: Vec<Any> = top.iter().map(|id| Any::from(*id)).collect();
        let block = |flavour: &str, props: Vec<(&str, In)>| {
            let mut block: Vec<(String, In)> =
                vec![("sys:flavour".to_owned(), In::Any(flavour.into()))];
            block.extend(
                props
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value)),
            );
            MapPrelim::from_iter(block)
        };
        map.insert(
            &mut txn,
            "page",
            block(
                "affine:page",
                vec![
                    ("prop:title", In::Text(TextPrelim::new("Chores").into())),
                    (
                        "sys:children",
                        In::Array(ArrayPrelim::from(vec![Any::from("note")])),
                    ),
                ],
            ),
        );
        map.insert(
            &mut txn,
            "note",
            block(
                "affine:note",
                vec![("sys:children", In::Array(ArrayPrelim::from(ids)))],
            ),
        );
        for (id, mut props) in blocks {
            let flavour = match props.remove(0) {
                ("sys:flavour", In::Any(Any::String(flavour))) => flavour,
                _ => panic!("each block starts with its flavour"),
            };
            map.insert(&mut txn, id, block(&flavour, props));
        }
        txn.encode_state_as_update_v1(&StateVector::default())
    }

    fn any(json: Value) -> In {
        In::Any(serde_json::from_value(json).unwrap())
    }

    use serde_json::{Value, json};

    fn links(titles: &HashMap<String, String>) -> Links<'_> {
        Links {
            titles,
            base: "https://affine.example/workspace/w1",
        }
    }

    #[test]
    fn a_page_reads_as_markdown() {
        let titles = HashMap::from([("hGhAvlHvXN".to_owned(), "ferret-linked".to_owned())]);

        let page = to_markdown(
            include_bytes!("../../../fixtures/affine/heat-pump.bin"),
            &links(&titles),
        )
        .unwrap();

        assert_eq!(page.title, "ferret-fixture");
        assert_eq!(
            page.markdown,
            include_str!("../../../fixtures/affine/heat-pump.md").trim_end()
        );
        assert!(page.unknown.is_empty(), "{:?}", page.unknown);
    }

    #[test]
    fn databases_embeds_and_equations_read_as_text() {
        let flavour = |name: &str| ("sys:flavour", In::Any(name.into()));
        let update = doc(
            &["db", "embed", "eq", "odd"],
            vec![
                (
                    "db",
                    vec![
                        flavour("affine:database"),
                        ("prop:title", In::Text(TextPrelim::new("Plants").into())),
                        (
                            "sys:children",
                            In::Array(ArrayPrelim::from(vec![Any::from("r1"), Any::from("r2")])),
                        ),
                        (
                            "prop:columns",
                            any(json!([
                                {"id": "s", "name": "Status", "type": "select", "data": {"options": [
                                    {"id": "o1", "value": "Todo"}, {"id": "o2", "value": "Done"}]}},
                                {"id": "d", "name": "Watered", "type": "date", "data": {}},
                            ])),
                        ),
                        (
                            "prop:cells",
                            any(json!({"r1": {
                                "s": {"columnId": "s", "value": "o2"},
                                "d": {"columnId": "d", "value": 1792195200000_i64},
                            }})),
                        ),
                    ],
                ),
                (
                    "r1",
                    vec![
                        flavour("affine:paragraph"),
                        ("prop:text", In::Text(TextPrelim::new("Monstera").into())),
                    ],
                ),
                (
                    "r2",
                    vec![
                        flavour("affine:paragraph"),
                        ("prop:text", In::Text(TextPrelim::new("").into())),
                    ],
                ),
                (
                    "embed",
                    vec![
                        flavour("affine:embed-linked-doc"),
                        ("prop:pageId", In::Any("shop".into())),
                    ],
                ),
                (
                    "eq",
                    vec![
                        flavour("affine:latex"),
                        ("prop:latex", In::Any("e^{i\\pi} = -1".into())),
                    ],
                ),
                (
                    "odd",
                    vec![
                        flavour("affine:transcription"),
                        ("prop:text", In::Text(TextPrelim::new("Kept").into())),
                    ],
                ),
            ],
        );
        let titles = HashMap::from([("shop".to_owned(), "Shopping".to_owned())]);

        let page = to_markdown(&update, &links(&titles)).unwrap();

        assert_eq!(page.title, "Chores");
        assert_eq!(
            page.markdown,
            "**Plants**\n\n\
             | Title | Status | Watered |\n|---|---|---|\n| Monstera | Done | 2026-10-17 |\n\n\
             [Shopping](https://affine.example/workspace/w1/shop)\n\n\
             $$\ne^{i\\pi} = -1\n$$\n\n\
             Kept"
        );
        assert_eq!(
            page.unknown,
            BTreeSet::from(["affine:transcription".to_owned()])
        );
    }

    #[test]
    fn references_and_inline_equations_stand_on_placeholders() {
        let update = doc(
            &["p"],
            vec![(
                "p",
                vec![
                    ("sys:flavour", In::Any("affine:paragraph".into())),
                    (
                        "prop:text",
                        In::Text(TextPrelim::new("See , where ").into()),
                    ),
                ],
            )],
        );
        // Formatting goes on the text once it's in a doc.
        let formatted = Doc::new();
        let blocks = formatted.get_or_insert_map("blocks");
        let mut txn = formatted.transact_mut();
        txn.apply_update(Update::decode_v1(&update).unwrap())
            .unwrap();
        let Some(Out::YMap(paragraph)) = blocks.get(&txn, "p") else {
            panic!("no paragraph")
        };
        let Some(Out::YText(text)) = paragraph.get(&txn, "prop:text") else {
            panic!("no text")
        };
        let attribute = |name: &str, value: Value| {
            HashMap::from([(
                Arc::<str>::from(name),
                serde_json::from_value(value).unwrap(),
            )])
        };
        text.insert_with_attributes(&mut txn, 12, " ", attribute("latex", json!("x^2")));
        text.insert_with_attributes(
            &mut txn,
            4,
            " ",
            attribute("reference", json!({"type": "LinkedPage", "pageId": "shop"})),
        );
        let update = txn.encode_state_as_update_v1(&StateVector::default());
        drop(txn);
        let titles = HashMap::from([("shop".to_owned(), "Shopping".to_owned())]);

        let page = to_markdown(&update, &links(&titles)).unwrap();

        assert_eq!(
            page.markdown,
            "See [Shopping](https://affine.example/workspace/w1/shop), where $x^2$"
        );
    }
}
