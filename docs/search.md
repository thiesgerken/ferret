# Searching Ferret

Use search to discover what exists and where it lives, then open the origin to work with it. Search only sees text already ingested for your user; it does not fan out live to origins.

## UI workflow

Enter a query on the home page. **Keyword** and **Semantic** are independent toggles; both on means hybrid, only Keyword means keyword search, only Semantic means meaning search. At least one stays on. Start with keyword mode for names, ids and exact phrases, or when a model is unavailable.

Source and author facets narrow results. A source includes descendants; several selected sources/authors are alternatives within that filter, while different filter kinds combine. Time filters use document **creation** time, not modification or ingestion time. The UI also offers Explore for browsing without a query. A copied search URL preserves query/filter choices. The API additionally accepts content types and explicit timestamps.

Unknown origin creation and modification dates stay null in REST, GraphQL and MCP. Undated documents participate in keyword, semantic and hybrid search without date filters. An explicit creation-date bound excludes unknown creation dates; it never filters on ingestion/seen time. The UI omits missing dates; Explore leaves their date cells empty, sorts unknown dates last in either direction, and does not invent a month facet for them. Thread listings and viewer neighbors place unknown creation dates after dated documents, ordered by document id.

Click the hit title to open the local viewer at its best passage. **Open in origin**, the external-link icon, opens its `url` when the UI considers that URL safe. A stored unsafe scheme is not turned into a clickable origin. With no URL, the local viewer is the origin. The viewer renders Markdown/plain/email text, metadata and previous/next thread navigation; it is not an original-file/PDF archive. Snippets are previews, not the full document.

## Query syntax

| Input | Meaning |
| --- | --- |
| `torque wrench` | keyword half requires every word to match a chunk's indexed text/title forms |
| `"torque wrench"` | phrase must occur in the candidate chunk text, ignoring case and whitespace differences |
| `torque -invoice` | exclude documents containing the excluded word in the implemented text/title matching |
| `"travel plan" -cancelled` | combine positive phrase and excluded token |

At least one positive word is required; an all-exclusion or empty query is invalid. An unclosed quote runs to the end. There is no boolean `OR`/`AND`, regex, field-colon or wildcard language in the parser. Filters are separate parameters, not `source:mail` syntax. Do not assume stemming makes a quoted phrase loose: the phrase check is more literal than the BM25 word matching.

For meaning search, the model receives positive words joined without operators. Phrase and exclusion filters still constrain candidates. Semantic-only does not require every positive keyword to occur literally. Exact phrase filtering checks chunk text, so a phrase spanning a chunk boundary or appearing only in a title can fail the phrase constraint.

## Ranking and scores

The keyword half ranks up to 1000 chunk candidates using BM25 over German/English stemmed and trigram title/text fields. The semantic half requests up to 100 HNSW nearest chunk vectors for the selected model, then drops candidates below its cosine-similarity threshold. HNSW is approximate and model coverage may still be incomplete.

Hybrid uses reciprocal rank fusion with constant 60: each chunk contributes `1 / (60 + rank)` from each half where it appears. It does **not** add raw BM25 to cosine similarity. The best fused chunk supplies the snippet; reported keyword/similarity values are the document's best values among retrieved candidates, not necessarily that snippet's values. Similarity bars are a presentation aid, not a calibrated probability that a document is correct.

Results contain one hit per document. By default, hits in the same `(source, thread)` collapse to the highest-ranked one; `collapsed` counts additional matching documents, not all messages in the thread. Documents without threads remain separate. The API can set `collapse=false`; the local viewer can navigate the full readable thread.

## Counts, pages and failure behavior

`total` is after matching/cutoff/collapse, not just the current page. `documents_in_scope` is an
informational count of readable documents in the selected sources before other filters, cached
for 60 seconds per user and effective source scope on each server replica. API-key source
restrictions narrow that scope before cache lookup; root order and duplicates do not create
different snapshots. It can lag ingestion/deletion and temporarily differ from fresh matching
totals. Hits, matching totals and facets are not cached by this metadata cache.

`total_is_lower_bound` warns when bounded candidates may hide additional matches; HNSW
approximation means even a false flag is not an exhaustive-corpus guarantee. The semantic check
looks only far enough into the eligible scope to establish whether more than the retrieved
nearest candidates exist. Facets apply all filters except their own so alternative choices remain
visible. Author facets include the twenty most frequent values plus selected values.

REST pages use `k` (default 10, range 1–100) and `offset`; subsequent requests should retain query/filter/model/mode/collapse choices. Each request has a consistent database snapshot for its hits, matching totals and facets; the informational scope count may be older. Successive pages are not a persistent snapshot across ingestion.

Hybrid returns keyword-only results with `semantic_unavailable` if query embedding is unavailable. Semantic-only returns service unavailable; keyword makes no query-embedding request. An unknown model is invalid, even for keyword requests. A successful semantic query may still return few results while the model's document backlog is pending. Inspect [embedding progress](embeddings.md) rather than treating every empty result as an ingestion failure.

Try the known sample in the [quickstart](quickstart.md). For machine requests use [REST](api/rest.md), [GraphQL](api/graphql.md), or [MCP](api/mcp.md).

Implementation: [query parser](../crates/ferret-server/src/search_query.rs), [retrieval and fusion](../crates/ferret-server/src/search.rs), [hit UI](../frontend/src/components/search-hit.tsx), [viewer](../frontend/src/components/document-viewer.tsx), [safe URL handling](../frontend/src/lib/safe-url.ts).
