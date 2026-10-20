//! Moneta's per-user rendered expenses, reconciled completely on every run.

use std::{collections::HashSet, fmt::Write, time::Duration};

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "moneta";
const PAGE_SIZE: usize = 100;
const QUERY: &str = r#"
query FerretExpenses($skip: Int!, $limit: Int!) {
  expenses {
    list(skip: $skip, limit: $limit, sortBy: ACCRUAL_START, sortDir: ASC,
      flags: {templates: true, taxRelevant: true, extraordinary: true,
              unchecked: true, incomplete: true, normal: true}) {
      totalCount
      edges { node {
        id title store description comments createdAt updatedAt
        referenceAmount effectiveAmount accrualStart accrualEnd
        isTemplate isTaxRelevant isExtraordinary isUnchecked isIncomplete
        categories { category { id name } weight }
        transactions {
          account { id name } date bookingDate amount fraction
          calculatedAmount effectiveAmount comments statement
        }
        positions { title quantity unit amount comments }
        receipts { uri ocr }
      } }
    }
  }
}
"#;

/// Indexes every expense visible to the key's owner, including templates and income.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Moneta")]
pub struct MonetaConfig {
    /// Instance root as reached by the worker, without `/graphql`. Must reach Moneta directly,
    /// not a login proxy. Requires Expense.createdAt and Expense.updatedAt.
    url: String,
    /// Instance root people open. Defaults to `url`; base paths are preserved.
    #[serde(default)]
    #[schemars(extend("x-ferret-preserve-credentials" = true))]
    public_url: Option<String>,
    /// A Moneta API key with read access. Its owner's normal visibility applies.
    #[schemars(extend("writeOnly" = true))]
    token: String,
}

#[derive(Deserialize, Serialize)]
struct Named {
    id: Uuid,
    name: String,
}

#[derive(Deserialize, Serialize)]
struct Categorization {
    category: Named,
    weight: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Transaction {
    account: Named,
    date: DateTime<Utc>,
    #[serde(deserialize_with = "Option::deserialize")]
    booking_date: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "Option::deserialize")]
    amount: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    fraction: Option<f64>,
    calculated_amount: f64,
    effective_amount: f64,
    comments: String,
    statement: String,
}

#[derive(Deserialize, Serialize)]
struct Position {
    title: String,
    #[serde(deserialize_with = "Option::deserialize")]
    quantity: Option<f64>,
    #[serde(deserialize_with = "Option::deserialize")]
    unit: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    amount: Option<i32>,
    comments: String,
}

#[derive(Deserialize, Serialize)]
struct Receipt {
    uri: String,
    #[serde(deserialize_with = "Option::deserialize")]
    ocr: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Expense {
    id: Uuid,
    title: String,
    store: String,
    description: String,
    comments: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    #[serde(deserialize_with = "Option::deserialize")]
    reference_amount: Option<i32>,
    effective_amount: f64,
    accrual_start: DateTime<Utc>,
    accrual_end: DateTime<Utc>,
    is_template: bool,
    is_tax_relevant: bool,
    is_extraordinary: bool,
    is_unchecked: bool,
    is_incomplete: bool,
    categories: Vec<Categorization>,
    transactions: Vec<Transaction>,
    positions: Vec<Position>,
    receipts: Vec<Receipt>,
}

#[derive(Deserialize)]
struct Response {
    data: Data,
}
#[derive(Deserialize)]
struct Data {
    expenses: Expenses,
}
#[derive(Deserialize)]
struct Expenses {
    list: Page,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    total_count: usize,
    edges: Vec<Edge>,
}
#[derive(Deserialize)]
struct Edge {
    node: Expense,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    // Config errors must not echo a token supplied under a malformed field.
    let config: MonetaConfig = serde_json::from_value(run.config.clone())
        .map_err(|_| "Moneta configuration is invalid")?;
    let base = instance_url(&config.url)?;
    let links = instance_url(config.public_url.as_deref().unwrap_or(&config.url))?;
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let source = format!("{}/expenses", run.source);
    let sync = run.api.start_sync(&source).await?.sync_id;
    let mut stats = RunStats::default();
    let mut ids = HashSet::new();
    let mut total = None;
    let mut previous = None;
    loop {
        let response = http
            .post(format!("{base}/graphql"))
            .bearer_auth(&config.token)
            .json(&json!({"query": QUERY, "variables": {"skip": ids.len(), "limit": PAGE_SIZE}}))
            .send()
            .await
            .map_err(|_| "Moneta GraphQL request failed")?;
        let status = response.status();
        if status.is_redirection() {
            return Err(
                "Moneta redirected /graphql; use a direct instance URL, not a login proxy".into(),
            );
        }
        if !status.is_success() {
            return Err(format!(
                "Moneta /graphql returned {status}; check the instance URL and read key"
            )
            .into());
        }
        // Origins can echo credentials or financial data in any error, including decoder errors.
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|_| "Moneta returned invalid GraphQL JSON")?;
        if value.get("errors").is_some_and(
            |errors| !matches!(errors, serde_json::Value::Array(errors) if errors.is_empty()),
        ) {
            return Err(
                "Moneta GraphQL errors; check read access and required Expense timestamp fields"
                    .into(),
            );
        }
        let page = serde_json::from_value::<Response>(value)
            .map_err(|_| "Moneta returned an invalid expense page")?
            .data
            .expenses
            .list;
        if *total.get_or_insert(page.total_count) != page.total_count
            || page.edges.len() != PAGE_SIZE.min(page.total_count.saturating_sub(ids.len()))
        {
            return Err(incomplete_listing());
        }
        for edge in page.edges {
            let expense = edge.node;
            let order = (expense.accrual_start, expense.id);
            if previous.is_some_and(|previous| order <= previous) || !ids.insert(expense.id) {
                return Err(incomplete_listing());
            }
            previous = Some(order);
            let document = to_document(expense, &links, &source)?;
            match run.api.put_document(&document).await? {
                PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
                PutResponse::Unchanged { .. } => stats.unchanged += 1,
                PutResponse::Excluded => {}
            }
        }
        if ids.len() == page.total_count {
            break;
        }
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    run.log.line(format!(
        "{} put, {} unchanged, {} deleted",
        stats.put, stats.unchanged, stats.deleted
    ));
    Ok(Outcome {
        state: json!({}),
        stats,
    })
}

fn incomplete_listing() -> PluginError {
    "Moneta changed or returned an incomplete expense listing; the next run retries".into()
}

fn instance_url(value: &str) -> Result<String, PluginError> {
    let url =
        reqwest::Url::parse(value).map_err(|_| "Moneta needs an absolute HTTP(S) instance URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Moneta needs an HTTP(S) instance URL without credentials, query or fragment".into(),
        );
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

// Display retains fraction-derived cents instead of rounding them to integer cents first.
fn eur(cents: f64) -> String {
    format!("{} EUR", cents / 100.0)
}

fn to_document(expense: Expense, links: &str, source: &str) -> Result<DocumentPut, PluginError> {
    let created_at = expense
        .transactions
        .iter()
        .map(|transaction| transaction.date)
        .min()
        .ok_or("Moneta expense has no transaction value date")?;
    let mut text = String::new();
    if expense.is_template {
        text.push_str("Template, not an actual payment\n");
    }
    writeln!(
        text,
        "{}\nStore: {}\n{}\nComments: {}",
        expense.title, expense.store, expense.description, expense.comments
    )
    .unwrap();
    writeln!(
        text,
        "User's effective wealth change: {} (negative = spend, positive = income)",
        eur(expense.effective_amount)
    )
    .unwrap();
    if let Some(amount) = expense.reference_amount {
        writeln!(text, "Reference amount: {}", eur(f64::from(amount))).unwrap();
    }
    writeln!(
        text,
        "Accrual period: {} to {}\nMoneta record created: {}\nMoneta record updated: {}",
        expense.accrual_start.to_rfc3339(),
        expense.accrual_end.to_rfc3339(),
        expense.created_at.to_rfc3339(),
        expense.updated_at.to_rfc3339()
    )
    .unwrap();
    writeln!(
        text,
        "Flags: template={}, tax relevant={}, extraordinary={}, unchecked={}, incomplete={}",
        expense.is_template,
        expense.is_tax_relevant,
        expense.is_extraordinary,
        expense.is_unchecked,
        expense.is_incomplete
    )
    .unwrap();
    text.push_str("\nCategories:\n");
    for category in &expense.categories {
        writeln!(
            text,
            "{} (weight {})",
            category.category.name, category.weight
        )
        .unwrap();
    }
    text.push_str("\nTransactions:\n");
    for transaction in &expense.transactions {
        writeln!(
            text,
            "Account: {}\nValue date (Wertstellung): {}",
            transaction.account.name,
            transaction.date.to_rfc3339()
        )
        .unwrap();
        if let Some(date) = transaction.booking_date {
            writeln!(text, "Booking date: {}", date.to_rfc3339()).unwrap();
        }
        writeln!(text, "Account movement: {}\nUser's effective wealth change: {}\nComments: {}\nBank statement: {}", eur(transaction.calculated_amount), eur(transaction.effective_amount), transaction.comments, transaction.statement).unwrap();
        if let Some(fraction) = transaction.fraction {
            writeln!(text, "Fraction of reference amount: {fraction}").unwrap();
        }
    }
    text.push_str("\nLine items:\n");
    for position in &expense.positions {
        writeln!(text, "{}\nComments: {}", position.title, position.comments).unwrap();
        if let Some(quantity) = position.quantity {
            writeln!(text, "Quantity: {quantity}").unwrap();
        }
        if let Some(unit) = &position.unit {
            writeln!(text, "Unit: {unit}").unwrap();
        }
        if let Some(amount) = position.amount {
            writeln!(text, "Amount: {}", eur(f64::from(amount))).unwrap();
        }
    }
    text.push_str("\nReceipt references (not downloaded):\n");
    for receipt in &expense.receipts {
        writeln!(text, "{}", receipt.uri).unwrap();
        if let Some(ocr) = &receipt.ocr {
            writeln!(text, "{ocr}").unwrap();
        }
    }
    let serde_json::Value::Object(meta) = json!({
        "currency": "EUR", "amount_unit": "cents", "is_template": expense.is_template,
        "expense": expense,
    }) else {
        unreachable!("metadata is an object")
    };
    Ok(DocumentPut {
        source: source.to_owned(),
        external_id: expense.id.to_string(),
        content_type: ContentType::Plain,
        title: Some(expense.title),
        text,
        url: Some(format!("{links}/expenses?expense={}", expense.id)),
        thread: None,
        authors: Vec::new(),
        created_at: Some(created_at),
        modified_at: Some(expense.updated_at),
        meta,
    })
}
