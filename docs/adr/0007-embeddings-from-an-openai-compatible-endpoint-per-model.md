# Embeddings from an OpenAI-compatible endpoint per model

Each embedding model names the endpoint that computes its embeddings, one for chunks and one for
queries if they differ, and ferret sends every one of them the same OpenAI-shaped request. A TEI,
a proxy such as LiteLLM and a cloud API are all just endpoints. Before, ferret discovered which
model each TEI of a pool served, routed queries and backfill between them, let admins switch them
on and off and showed their load: a small LLM gateway inside a search index, which grew with every
new wish (a laptop's GPU for a backfill, load figures) and still couldn't use a cloud API.
Routing, failover and serving now belong to the endpoint, and ferret measures only its own work
(backlog, rate, last error). The price is that ferret can no longer tell which weights computed a
vector: it checks every answer's shape (count, order, dimension, finite values, a norm above 0)
but trusts that a model name always means the same weights, and a cloud endpoint sees every chunk
and query. A probe vector per request to detect swapped weights was rejected as cost without a
reliable answer.
