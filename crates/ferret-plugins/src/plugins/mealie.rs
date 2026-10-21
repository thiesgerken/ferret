//! Group-wide recipes and their attached notes/comments, fully reconciled every run.

use std::{fmt::Write, time::Duration};

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use uuid::Uuid;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "mealie";
const PAGE_SIZE: usize = 100;

/// Copies every recipe accessible in the token user's group, across households.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Mealie")]
pub struct MealieConfig {
    /// Instance root reachable by the worker, without `/api`. Base paths are preserved.
    url: String,
    /// Instance root people open. Defaults to `url`.
    #[serde(default)]
    #[schemars(extend("x-ferret-preserve-credentials" = true))]
    public_url: Option<String>,
    /// Copies all accessible recipes in the token user's group, across households.
    /// The plugin only reads, but this Mealie API token has its owner's capabilities.
    #[schemars(extend("writeOnly" = true))]
    token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct User {
    group_id: Uuid,
    group_slug: String,
}

#[derive(Deserialize)]
struct Page {
    page: usize,
    per_page: usize,
    total: usize,
    total_pages: usize,
    items: Vec<Summary>,
    #[serde(deserialize_with = "Option::deserialize")]
    next: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    previous: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    id: Uuid,
    group_id: Uuid,
}

#[derive(Deserialize, Serialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reference {
    id: Uuid,
    group_id: Uuid,
    slug: String,
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ingredient {
    title: Option<String>,
    display: String,
    referenced_recipe: Option<Reference>,
}

#[derive(Deserialize)]
struct Step {
    title: Option<String>,
    summary: Option<String>,
    text: String,
}

#[derive(Deserialize)]
struct Note {
    title: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentUser {
    id: Uuid,
    username: Option<String>,
    full_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Comment {
    id: Uuid,
    recipe_id: Uuid,
    text: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    user_id: Uuid,
    user: CommentUser,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Nutrition {
    calories: Option<String>,
    carbohydrate_content: Option<String>,
    cholesterol_content: Option<String>,
    fat_content: Option<String>,
    fiber_content: Option<String>,
    protein_content: Option<String>,
    saturated_fat_content: Option<String>,
    sodium_content: Option<String>,
    sugar_content: Option<String>,
    trans_fat_content: Option<String>,
    unsaturated_fat_content: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recipe {
    id: Uuid,
    group_id: Uuid,
    household_id: Uuid,
    user_id: Uuid,
    name: Option<String>,
    slug: String,
    description: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    recipe_servings: f64,
    recipe_yield_quantity: f64,
    recipe_yield: Option<String>,
    total_time: Option<String>,
    prep_time: Option<String>,
    cook_time: Option<String>,
    perform_time: Option<String>,
    total_time_seconds: Option<u32>,
    prep_time_seconds: Option<u32>,
    perform_time_seconds: Option<u32>,
    #[serde(rename = "orgURL")]
    original_url: Option<String>,
    recipe_ingredient: Vec<Ingredient>,
    recipe_instructions: Option<Vec<Step>>,
    notes: Option<Vec<Note>>,
    comments: Option<Vec<Comment>>,
    tags: Option<Vec<Named>>,
    recipe_category: Option<Vec<Named>>,
    tools: Vec<Named>,
    nutrition: Option<Nutrition>,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    // Decoder errors can include secret values supplied under an invalid field.
    let config: MealieConfig = serde_json::from_value(run.config.clone())
        .map_err(|_| "Mealie configuration is invalid")?;
    let base = instance_url(&config.url)?;
    let links = instance_url(config.public_url.as_deref().unwrap_or(&config.url))?;
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let user: User = get(&http, &config.token, &format!("{base}/api/users/self")).await?;
    if user.group_slug.is_empty() {
        return Err("Mealie returned an invalid user group".into());
    }
    let source = format!("{}/recipes", run.source);
    let sync = run.api.start_sync(&source, None).await?.sync_id;
    let mut stats = RunStats::default();
    let mut total = None;
    let mut previous_id = None;
    let mut seen = 0;
    let mut number = 1;
    loop {
        // Never send credentials to an origin-supplied pagination URL.
        let page: Page = get(
            &http,
            &config.token,
            &format!(
                "{base}/api/recipes?orderBy=id&orderDirection=asc&page={number}&perPage={PAGE_SIZE}"
            ),
        )
        .await?;
        let pages = page.total.div_ceil(PAGE_SIZE);
        if page.page != number
            || page.per_page != PAGE_SIZE
            || page.total_pages != pages
            || *total.get_or_insert(page.total) != page.total
            || page.items.len() != PAGE_SIZE.min(page.total.saturating_sub(seen))
            || page.next.is_some() != (number < pages)
            || page.previous.is_some() != (number > 1)
        {
            return Err(incomplete_listing());
        }
        for item in page.items {
            if item.group_id != user.group_id
                || previous_id.is_some_and(|previous| item.id <= previous)
            {
                return Err(incomplete_listing());
            }
            previous_id = Some(item.id);
            let recipe: Recipe = get(
                &http,
                &config.token,
                &format!("{base}/api/recipes/{}", item.id),
            )
            .await?;
            if recipe.id != item.id || recipe.group_id != user.group_id || recipe.slug.is_empty() {
                return Err("Mealie returned an inconsistent recipe detail".into());
            }
            let document = to_document(recipe, &links, &user.group_slug, &source)?;
            match run.api.put_document(&document).await? {
                PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
                PutResponse::Unchanged { .. } => stats.unchanged += 1,
                PutResponse::Excluded => {}
            }
            seen += 1;
        }
        if seen == page.total {
            break;
        }
        number += 1;
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

async fn get<T: DeserializeOwned>(
    http: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<T, PluginError> {
    let response = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "Mealie request failed")?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Mealie returned {status}; check the direct instance URL, token and visibility"
        )
        .into());
    }
    // Neither response bodies nor decoder diagnostics are safe to expose.
    response
        .json()
        .await
        .map_err(|_| "Mealie returned invalid or incomplete JSON".into())
}

fn incomplete_listing() -> PluginError {
    "Mealie changed or returned an incomplete recipe listing; the next run retries".into()
}

fn instance_url(value: &str) -> Result<String, PluginError> {
    let url =
        reqwest::Url::parse(value).map_err(|_| "Mealie needs an absolute HTTP(S) instance URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Mealie needs an HTTP(S) instance URL without credentials, query or fragment".into(),
        );
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn recipe_url(base: &str, group: &str, slug: &str) -> String {
    let mut url = reqwest::Url::parse(base).expect("validated instance URL");
    url.path_segments_mut()
        .expect("HTTP URL supports path segments")
        .push("g")
        .push(group)
        .push("r")
        .push(slug)
        // Mealie's static frontend serves recipe routes only with their trailing slash.
        .push("");
    url.into()
}

fn paragraph(text: &mut String, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        writeln!(text, "{value}\n").unwrap();
    }
}

fn names(text: &mut String, label: &str, values: &mut [Named]) {
    values.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    if !values.is_empty() {
        writeln!(text, "\n## {label}").unwrap();
        for value in values {
            writeln!(text, "- {}", value.name).unwrap();
        }
    }
}

fn to_document(
    mut recipe: Recipe,
    links: &str,
    group: &str,
    source: &str,
) -> Result<DocumentPut, PluginError> {
    let mut text = String::new();
    paragraph(&mut text, recipe.description.as_deref());
    for (label, value) in [
        ("Servings", Some(recipe.recipe_servings)),
        ("Yield quantity", Some(recipe.recipe_yield_quantity)),
    ] {
        if let Some(value) = value.filter(|value| *value != 0.0) {
            writeln!(text, "{label}: {value}").unwrap();
        }
    }
    for (label, value) in [
        ("Yield", recipe.recipe_yield.as_deref()),
        ("Total time", recipe.total_time.as_deref()),
        ("Prep time", recipe.prep_time.as_deref()),
        ("Cook time", recipe.cook_time.as_deref()),
        ("Perform time", recipe.perform_time.as_deref()),
    ] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            writeln!(text, "{label}: {value}").unwrap();
        }
    }
    for (label, value) in [
        ("Total time", recipe.total_time_seconds),
        ("Prep time", recipe.prep_time_seconds),
        ("Perform time", recipe.perform_time_seconds),
    ] {
        if let Some(value) = value {
            writeln!(text, "{label}: {value} seconds").unwrap();
        }
    }
    text.push_str("\n## Ingredients\n");
    for ingredient in &recipe.recipe_ingredient {
        if let Some(title) = ingredient
            .title
            .as_deref()
            .filter(|title| !title.is_empty())
        {
            writeln!(text, "\n### {title}").unwrap();
        }
        writeln!(text, "- {}", ingredient.display).unwrap();
        if let Some(reference) = &ingredient.referenced_recipe {
            if reference.group_id != recipe.group_id || reference.slug.is_empty() {
                return Err("Mealie returned an inconsistent recipe reference".into());
            }
            writeln!(
                text,
                "  Recipe reference: [{}]({}) ({})",
                reference.name.as_deref().unwrap_or(&reference.slug),
                recipe_url(links, group, &reference.slug),
                reference.id
            )
            .unwrap();
        }
    }
    text.push_str("\n## Instructions\n");
    for (index, step) in recipe.recipe_instructions.iter().flatten().enumerate() {
        if let Some(title) = step.title.as_deref().filter(|title| !title.is_empty()) {
            writeln!(text, "\n### {title}").unwrap();
        }
        paragraph(&mut text, step.summary.as_deref());
        writeln!(text, "{}. {}\n", index + 1, step.text).unwrap();
    }
    if let Some(notes) = &recipe.notes {
        for note in notes {
            writeln!(text, "\n## Recipe note: {}\n\n{}", note.title, note.text).unwrap();
        }
    }
    let mut modified_at = recipe.updated_at;
    if let Some(comments) = &mut recipe.comments {
        comments.sort_unstable_by_key(|comment| (comment.created_at, comment.id));
        let mut ids = std::collections::HashSet::new();
        for comment in comments {
            if comment.recipe_id != recipe.id
                || comment.user_id != comment.user.id
                || !ids.insert(comment.id)
            {
                return Err("Mealie returned an inconsistent recipe comment".into());
            }
            modified_at = modified_at.max(comment.updated_at);
            writeln!(
                text,
                "\n## Comment\n\nCreated: {}\nUpdated: {}",
                comment.created_at.to_rfc3339(),
                comment.updated_at.to_rfc3339()
            )
            .unwrap();
            if let Some(author) = comment
                .user
                .full_name
                .as_deref()
                .filter(|name| !name.is_empty())
                .or(comment.user.username.as_deref())
            {
                writeln!(text, "Author: {author}").unwrap();
            }
            writeln!(text, "\n{}", comment.text).unwrap();
        }
    }
    names(
        &mut text,
        "Categories",
        recipe.recipe_category.as_deref_mut().unwrap_or_default(),
    );
    names(
        &mut text,
        "Tags",
        recipe.tags.as_deref_mut().unwrap_or_default(),
    );
    names(&mut text, "Tools", &mut recipe.tools);
    if let Some(nutrition) = &recipe.nutrition {
        text.push_str("\n## Nutrition\n");
        for (label, value) in [
            ("Calories", &nutrition.calories),
            ("Carbohydrate", &nutrition.carbohydrate_content),
            ("Cholesterol", &nutrition.cholesterol_content),
            ("Fat", &nutrition.fat_content),
            ("Fiber", &nutrition.fiber_content),
            ("Protein", &nutrition.protein_content),
            ("Saturated fat", &nutrition.saturated_fat_content),
            ("Sodium", &nutrition.sodium_content),
            ("Sugar", &nutrition.sugar_content),
            ("Trans fat", &nutrition.trans_fat_content),
            ("Unsaturated fat", &nutrition.unsaturated_fat_content),
        ] {
            if let Some(value) = value.as_deref().filter(|value| !value.is_empty()) {
                writeln!(text, "{label}: {value}").unwrap();
            }
        }
    }
    let serde_json::Value::Object(meta) = json!({
        "recipe_id": recipe.id, "group_id": recipe.group_id,
        "household_id": recipe.household_id, "user_id": recipe.user_id,
        "slug": recipe.slug, "original_url": recipe.original_url,
        "tags": recipe.tags, "categories": recipe.recipe_category, "tools": recipe.tools,
        "servings": recipe.recipe_servings, "yield_quantity": recipe.recipe_yield_quantity,
        "yield": recipe.recipe_yield, "nutrition": recipe.nutrition,
        "total_time_seconds": recipe.total_time_seconds, "prep_time_seconds": recipe.prep_time_seconds,
        "perform_time_seconds": recipe.perform_time_seconds,
    }) else {
        unreachable!("metadata is an object")
    };
    Ok(DocumentPut {
        source: source.to_owned(),
        external_id: recipe.id.to_string(),
        content_type: ContentType::Markdown,
        title: recipe.name,
        text,
        url: Some(recipe_url(links, group, &recipe.slug)),
        thread: None,
        authors: Vec::new(),
        created_at: Some(recipe.created_at),
        modified_at: Some(modified_at),
        meta,
    })
}
