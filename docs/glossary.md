# ferret

ferret is a personal search index: it makes everything that describes a user (notes, documents,
mail, chats, agent memory) searchable by keyword and meaning, and points back to where each piece
lives.

## Content

**Origin**:
The external system a piece of content comes from and lives in, e.g. a Paperless instance, a mail
account, a notes folder.
_Avoid_: backend, provider, upstream

**Document**:
The unit of content ferret indexes and returns. What a document is (a file, a mail, a fact, a chat
segment) is decided by whoever ingests it.
_Avoid_: item, record, entry, file

**Chunk**:
A piece of a document's text, cut by ferret, that is matched against queries.
_Avoid_: passage, fragment, window

**Thread**:
An ordered sequence of documents that belong together, such as the mails of a conversation or the
segments of a chat.
_Avoid_: conversation, chain, channel

**Segment**:
A stretch of a chat conversation, bounded by pauses, that a chat plugin turns into one document.
_Avoid_: window, session

## Ingestion

**Source**:
A path-shaped namespace that documents belong to, e.g. `mail/zoho` or `carapace/facts`. A source
includes every source below it.
_Avoid_: collection, index, origin (an origin is the external system)

**Plugin**:
Code that knows how to turn one kind of origin into documents, e.g. `imap` or `paperless`.
_Avoid_: connector, integration, ingester (for built-in ones)

**Connection**:
A plugin configured by one user for one origin, with its schedule and the source root it owns.
_Avoid_: plugin instance, integration, feed, source

**Ingest**:
One stretch of writing into the index: a run, or a burst. What the Activity page lists.
_Avoid_: job, import

**Burst**:
An API key's writes with gaps under 10 minutes, counted as one ingest. Grouped by time, not by
sync: a key that never syncs has bursts, and a sync with a longer pause spans two.
_Avoid_: sync, session

**Writer**:
A connection or an API key: what puts documents into sources. Shown with its name and icon.
_Avoid_: origin (the external system), ingester

**Run**:
One execution of a connection.
_Avoid_: job, ingestion (for a single execution)

**Sync**:
A bracket around ingestion in which everything still present at the origin is reported, so that
whatever wasn't reported can be removed when the sync is committed.
_Avoid_: full import, refresh

**Sweep**:
The removal, at the commit of a sync, of documents that weren't reported during it.
_Avoid_: cleanup, garbage collection

**Exclusion**:
A document or thread a user removed from the index for good, so its connection never ingests it
again.
_Avoid_: blocklist entry, denylist, hidden document

## Retrieval

**Hit**:
A document returned by a search, shown with its best-matching chunk.
_Avoid_: result, match

**Judgment**:
A user's statement that a document is a right hit for a search. A search may have several.
_Avoid_: label, rating, vote

**Eval set**:
All judgments of a user, to measure how well an embedding model retrieves on their data (the
evaluation follows after v1, #21).
_Avoid_: test set, benchmark

**Embedding model**:
A model that turns text into vectors for semantic search. Several can be live at once.
_Avoid_: encoder, model (unqualified)

**Default model**:
The embedding model a user's searches use unless a search asks for another one.
_Avoid_: active model, primary model

**Model registry**:
Every embedding model that ever embedded, configured or not, with its dimension (`embedding_models`).
Admin lists each model's stored embeddings, and deletes those of models that aren't configured.

**Configured default**:
The model the config names as `embedding.default`, used by every user who chose no default model.

**TEI**:
A text-embeddings-inference server that computes embeddings for one embedding model at a time.
_Avoid_: embedder, embedding server

**TEI pool**:
The TEIs ferret embeds with. Each works for whichever configured embedding model it serves,
which the server discovers. An external TEI is one outside the deployment, e.g. a laptop's GPU.
_Avoid_: bulk TEI, GPU TEI (as a role)
