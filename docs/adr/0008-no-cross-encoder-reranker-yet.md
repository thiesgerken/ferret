# No cross-encoder reranker yet

Search ranks by reciprocal rank fusion of BM25 and vector neighbours and stops there. A
cross-encoder that reorders the fused top N is the usual next step for retrieval systems. Measured
on a real personal corpus it bought little: about +0.03 nDCG@10 when reranking the top 20, less or
even a loss when reranking deeper, because the reranker lifts plausible-looking but irrelevant
passages from far down. It costs a lot in return: one more model endpoint (TEI's `/rerank` isn't
OpenAI-shaped, see [ADR 0007](0007-embeddings-from-an-openai-compatible-endpoint-per-model.md)),
and seconds per search on CPU hardware. The larger loss sits before ranking: about 9% of the judged
documents aren't among the top 100 candidates at all, and a reranker only reorders what retrieval
found. The keyword half is empty for about 4 in 10 natural queries because it requires every word
([#212](https://github.com/thiesgerken/ferret/issues/212)). Recall comes first. A reranker gets
measured again, on the same eval set, once that is fixed.

## Measurement

Corpus: about 40,000 documents of one user (mail, chat, finance transactions, scanned paperwork,
files, agent sessions, bookmarks, recipes), ferret 0.36.0, embedding model `qwen3-embedding-0.6b`.

Eval set: 300 queries with 2,634 judgments, written by three LLM agents, each covering one part of
the corpus.

- 190 **known-item** queries. An agent read a random document, then wrote the query a person would
  type months later to find it again. Paraphrased, written before searching, never rewritten after
  seeing results.
- 110 **topical** queries. An information need with several answers. The agent pooled the top 30
  of hybrid, keyword and semantic search and judged each document by reading it.

The judgments are stored as the user's judgments (the eval set of #57).

Runs: REST search with `k=100` and `collapse=false`, so that thread collapse can't hide a judged
document behind a sibling. The reranker was `BAAI/bge-reranker-v2-m3` on title plus best chunk,
`max_length` 512, reordering the hybrid top N and leaving the rest as it was. Recall is capped at
min(relevant, k), because topical queries over recurring expenses judge more than 100 documents.

| system | R@10 | MRR@10 | nDCG@10 | R@100 |
| --- | --- | --- | --- | --- |
| hybrid (RRF) | 0.767 | 0.786 | 0.718 | 0.907 |
| semantic only | 0.729 | 0.755 | 0.672 | 0.879 |
| keyword only | 0.354 | 0.490 | 0.375 | 0.359 |
| rerank top 20 | 0.785 | 0.823 | 0.744 | 0.907 |
| rerank top 50 | 0.767 | 0.822 | 0.729 | 0.907 |
| rerank top 100 | 0.758 | 0.818 | 0.721 | 0.907 |

On topical queries alone, reranking the top 100 lowers nDCG@10 from 0.661 to 0.640. Reranking 100
chunks took about 1.5 s per query on an Apple-silicon GPU and about 85 s on CPU (TEI in Docker).

Caveats:

- Agents wrote the queries, and they tend to be tidier than typed ones.
- The topical pools came from the current search modes, which can favour the current ranking.
- With 300 queries, a difference of 0.03 is near the noise.
