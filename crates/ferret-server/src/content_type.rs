//! `documents.content_type` and `chunks.content_type` as stored.

use ferret_api::ContentType;

pub fn as_str(content_type: ContentType) -> &'static str {
    match content_type {
        ContentType::Markdown => "markdown",
        ContentType::Plain => "plain",
        ContentType::Email => "email",
    }
}

pub fn parse(content_type: &str) -> ContentType {
    match content_type {
        "markdown" => ContentType::Markdown,
        "plain" => ContentType::Plain,
        "email" => ContentType::Email,
        // The tables' check constraints admit no other value.
        _ => panic!("unknown content type {content_type:?} in the database"),
    }
}
