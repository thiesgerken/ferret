# /// script
# requires-python = ">=3.12,<3.13"
# dependencies = ["embedding-atlas==0.26.0", "psycopg[binary]>=3.2,<4"]
# ///
"""Dev-only snapshot viewer. Run with uv run scripts/atlas-prototype.py --help."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

import numpy as np
import pandas as pd
import psycopg
from psycopg.rows import dict_row
from umap import UMAP


def snapshot(database_url: str, username: str, model: str) -> tuple[pd.DataFrame, int]:
    # Read-only also protects against accidentally introducing a write in this prototype.
    with psycopg.connect(database_url, row_factory=dict_row) as connection:
        connection.execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        connection.execute("SET LOCAL statement_timeout = '60s'")
        user = connection.execute(
            "SELECT id FROM users WHERE username = %s", (username,)
        ).fetchone()
        if user is None:
            raise ValueError(f"Unknown user: {username}")
        registered = connection.execute(
            "SELECT id FROM embedding_models WHERE id = %s", (model,)
        ).fetchone()
        if registered is None:
            raise ValueError(f"Unknown embedding model: {model}")
        total = connection.execute(
            "SELECT count(*) AS count FROM documents WHERE user_id = %s", (user["id"],)
        ).fetchone()["count"]
        rows = connection.execute(
            """
            SELECT d.title, d.source, d.content_type, d.authors,
                   d.created_at, d.modified_at, d.url AS origin_url, d.text,
                   d.id::text AS document_id, vectors.chunks, vectors.embedding
            FROM documents d
            JOIN (
                SELECT c.document_id, count(*) AS chunks,
                       avg(l2_normalize(e.embedding::vector))::text AS embedding
                FROM chunks c
                LEFT JOIN chunk_embeddings e
                    ON e.chunk_id = c.id AND e.model = %s AND e.user_id = c.user_id
                WHERE c.user_id = %s
                GROUP BY c.document_id
                HAVING count(e.chunk_id) = count(*)
            ) vectors ON vectors.document_id = d.id
            WHERE d.user_id = %s
            ORDER BY d.id
            """,
            (model, user["id"], user["id"]),
        ).fetchall()
    if len(rows) < 4:
        raise ValueError("The map needs at least four fully embedded documents for this user/model")
    frame = pd.DataFrame(rows)
    vectors = np.asarray([json.loads(value) for value in frame.pop("embedding")], dtype=np.float32)
    norms = np.linalg.norm(vectors, axis=1)
    if not np.isfinite(vectors).all() or np.any(norms == 0):
        raise ValueError("Document aggregation produced a non-finite or zero vector")
    vectors /= norms[:, None]
    print(f"Projecting {len(frame)}/{total} documents using {model}; {total - len(frame)} omitted "
          "(no chunks or incomplete embeddings).", flush=True)
    # Keep filters on a fixed layout, and never ask Atlas to re-embed private text.
    coordinates = UMAP(
        n_neighbors=min(15, len(frame) - 1), metric="cosine", random_state=42, n_jobs=1
    ).fit_transform(vectors)
    frame["projection_x"] = coordinates[:, 0]
    frame["projection_y"] = coordinates[:, 1]
    frame["model"] = model
    return frame, total


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--user", required=True, help="Ferret username; only this user's documents are read")
    parser.add_argument("--model", required=True, help="One registered embedding model ID")
    parser.add_argument("--port", type=int, default=5055)
    args = parser.parse_args()
    database_url = os.environ.get("FERRET_DATABASE_URL")
    if not database_url:
        parser.error("Set FERRET_DATABASE_URL to the development Postgres connection URL")

    os.umask(0o077)
    directory = Path(__file__).resolve().parent.parent / ".atlas-prototype"
    directory.mkdir(exist_ok=True)
    frame, total = snapshot(database_url, args.user, args.model)
    dataset = directory / "documents.parquet"
    frame.to_parquet(dataset, index=False)
    print(f"Snapshot: {len(frame)}/{total} documents. Restart to refresh. "
          f"Private local data: {directory}", flush=True)
    state = {
        "version": "0.26.0",
        "charts": {
            "1": {"type": "embedding", "title": "Documents", "data": {
                "x": "projection_x", "y": "projection_y", "text": "text", "category": "source",
            }},
            "2": {"type": "instances", "title": "Documents"},
            "3": {"type": "count-plot", "title": "Source", "data": {"field": "source"}},
            "4": {"type": "count-plot", "title": "Content type", "data": {"field": "content_type"}},
            "5": {"type": "count-plot", "title": "Authors", "data": {"field": "authors", "isList": True}},
        },
        "layouts": {"1": {
            "type": "list", "name": f"{args.model}: {len(frame)}/{total} documents (snapshot)",
            "chartIds": ["1", "2", "3", "4", "5"],
        }},
        "layoutOrder": ["1"],
        "currentLayout": "1",
    }
    subprocess.run(
        [str(Path(sys.executable).with_name("embedding-atlas")), str(dataset),
         "--text", "text", "--x", "projection_x", "--y", "projection_y",
         "--disable-projection", "--host", "127.0.0.1", "--port", str(args.port),
         "--no-auto-port", "--duckdb", "wasm", "--state", json.dumps(state)],
        cwd=directory,
        check=True,
    )


if __name__ == "__main__":
    main()
