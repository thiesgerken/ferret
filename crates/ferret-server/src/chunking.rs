//! Cuts a document's text into the chunks that search matches against.

use ferret_api::ContentType;
use text_splitter::{ChunkConfig, MarkdownSplitter, TextSplitter};

/// Chunks fill up to the maximum, preferring the biggest semantic unit (heading section,
/// paragraph, line, sentence) that fits once they reach the minimum.
// ponytail: counts characters, not model tokens. ~2000 characters stay far below every
// embedding model's input limit (Qwen3's TEI accepts 4096 tokens). Count tokens with the
// model's tokenizer if chunks need to fill a token budget exactly.
const CHUNK_CHARS: std::ops::Range<usize> = 1000..2000;

#[derive(Debug, PartialEq, Eq)]
pub struct Chunk<'a> {
    pub text: &'a str,
    /// Characters (Unicode scalar values) into the document's text.
    pub start_offset: usize,
    pub end_offset: usize,
}

/// Markdown by heading, then paragraph; plain text and mail by paragraph, then line.
pub fn chunk(content_type: ContentType, text: &str) -> Vec<Chunk<'_>> {
    let config = ChunkConfig::new(CHUNK_CHARS);
    let indices: Vec<_> = match content_type {
        ContentType::Markdown => MarkdownSplitter::new(config)
            .chunk_char_indices(text)
            .collect(),
        ContentType::Plain | ContentType::Email => {
            TextSplitter::new(config).chunk_char_indices(text).collect()
        }
    };
    indices
        .into_iter()
        .map(|index| Chunk {
            text: index.chunk,
            start_offset: index.char_offset,
            end_offset: index.char_offset + index.chunk.chars().count(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_count_characters_and_point_at_the_chunk() {
        let paragraph = "Grüße aus Köln. ".repeat(80);
        let text = format!("{paragraph}\n\n{paragraph}\n\n{paragraph}");

        let chunks = chunk(ContentType::Plain, &text);

        assert!(chunks.len() > 1);
        for chunk in chunks {
            let at_offsets: String = text
                .chars()
                .skip(chunk.start_offset)
                .take(chunk.end_offset - chunk.start_offset)
                .collect();
            assert_eq!(at_offsets, chunk.text);
            assert!(chunk.text.chars().count() <= CHUNK_CHARS.end);
        }
    }

    #[test]
    fn markdown_splits_at_headings() {
        let section = |heading: &str| format!("# {heading}\n\n{}", "Text. ".repeat(200));
        let text = format!("{}\n\n{}", section("One"), section("Two"));

        let chunks = chunk(ContentType::Markdown, &text);

        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.starts_with("# One"));
        assert!(chunks[1].text.starts_with("# Two"));
    }

    #[test]
    fn short_and_empty_texts() {
        assert_eq!(
            chunk(ContentType::Plain, "  short  "),
            [Chunk {
                text: "short",
                start_offset: 2,
                end_offset: 7
            }]
        );
        assert!(chunk(ContentType::Markdown, "").is_empty());
    }
}
