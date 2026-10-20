# /// script
# requires-python = ">=3.12,<3.13"
# dependencies = ["embedding-atlas==0.26.0", "psycopg[binary]>=3.2,<4"]
# ///
"""Export a private Ferret snapshot and view it locally with Apple Embedding Atlas."""

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import time

import numpy as np
import pandas as pd
import psycopg
from psycopg.conninfo import make_conninfo
from psycopg.rows import dict_row
from umap import UMAP


def read_documents(database_url: str, username: str, model: str) -> tuple[pd.DataFrame, int]:
    with psycopg.connect(database_url, row_factory=dict_row, connect_timeout=10) as connection:
        # Enforce read-only even when the supplied credentials can write.
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
    return pd.DataFrame(rows), total


@contextmanager
def database_tunnel(kubectl: list[str], deployment: str):
    # Let kubectl allocate a free local port rather than racing another dev service.
    process = subprocess.Popen(
        [*kubectl, "port-forward", "--address", "127.0.0.1", f"deployment/{deployment}", ":5432"],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    try:
        deadline = time.monotonic() + 30
        output = b""
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([process.stdout], [], [], remaining)[0]:
                raise TimeoutError("Database port-forward did not become ready within 30 seconds")
            chunk = os.read(process.stdout.fileno(), 4096)
            if not chunk:
                raise RuntimeError(f"Database port-forward exited: {output.decode(errors='replace')}")
            output += chunk
            match = re.search(rb"Forwarding from 127\.0\.0\.1:(\d+)", output)
            if match:
                yield int(match[1])
                return
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        process.stdout.close()


def read_kubernetes_documents(args) -> tuple[pd.DataFrame, int]:
    kubectl = ["kubectl", "--context", args.context, "--namespace", args.namespace]
    with database_tunnel(kubectl, args.deployment) as port:
        # Capture credentials only in memory, never in argv, files or terminal output.
        result = subprocess.run(
            [*kubectl, "exec", f"deployment/{args.deployment}", "--", "sh", "-c",
             'printf "%s\\0%s\\0%s" "$POSTGRES_USER" "$POSTGRES_DB" "$POSTGRES_PASSWORD"'],
            check=True, capture_output=True, timeout=30,
        )
        credentials = result.stdout.decode().split("\0")
        if len(credentials) != 3 or not all(credentials):
            raise ValueError("Postgres pod must define POSTGRES_USER, POSTGRES_DB and POSTGRES_PASSWORD")
        user, database, password = credentials
        connection = make_conninfo(
            host="127.0.0.1", port=port, user=user, dbname=database, password=password,
        )
        return read_documents(connection, args.user, args.model)


def atlas_state(username: str, model: str, projected: int, total: int) -> dict:
    return {
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
            "type": "list", "name": f"{username} / {model}: {projected}/{total} documents (snapshot)",
            "chartIds": ["1", "2", "3", "4", "5"],
        }},
        "layoutOrder": ["1"],
        "currentLayout": "1",
    }


def write_snapshot(frame: pd.DataFrame, total: int, username: str, model: str, output: Path) -> None:
    vectors = np.asarray([json.loads(value) for value in frame.pop("embedding")], dtype=np.float32)
    norms = np.linalg.norm(vectors, axis=1)
    if not np.isfinite(vectors).all() or np.any(norms == 0):
        raise ValueError("Document aggregation produced a non-finite or zero vector")
    vectors /= norms[:, None]
    print(f"Projecting {len(frame)}/{total} documents locally; {total - len(frame)} omitted "
          "(no chunks or incomplete embeddings). Database connection closed.", flush=True)
    coordinates = UMAP(
        n_neighbors=min(15, len(frame) - 1), metric="cosine", random_state=42, n_jobs=1,
    ).fit_transform(vectors)
    if not np.isfinite(coordinates).all():
        raise ValueError("UMAP produced non-finite coordinates")
    frame["projection_x"] = coordinates[:, 0]
    frame["projection_y"] = coordinates[:, 1]
    frame["model"] = model
    frame.to_parquet(output / "documents.parquet", index=False)
    state = atlas_state(username, model, len(frame), total)
    (output / "state.json").write_text(json.dumps(state, indent=2))
    summary = {
        "user": username, "model": model,
        "completed_at": datetime.now(timezone.utc).isoformat(),
        "documents_total": total, "documents_projected": len(frame),
        "documents_omitted": total - len(frame),
        "sources": frame.groupby("source").size().to_dict(),
    }
    (output / "snapshot.json").write_text(json.dumps(summary, indent=2))
    print(f"Saved private snapshot to {output}. Use the serve command to open it.", flush=True)


def serve(snapshot: Path, port: int) -> None:
    dataset = snapshot.resolve() / "documents.parquet"
    state = snapshot.resolve() / "state.json"
    for path in (dataset, state):
        if not path.is_file():
            raise FileNotFoundError(path)
    # A saved map needs neither new embeddings nor another projection.
    subprocess.run(
        [str(Path(sys.executable).with_name("embedding-atlas")), str(dataset),
         "--text", "text", "--x", "projection_x", "--y", "projection_y",
         "--disable-projection", "--host", "127.0.0.1", "--port", str(port),
         "--no-auto-port", "--duckdb", "server", "--state", str(state)],
        check=True,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name, help_text in (
        ("export", "Read Postgres via FERRET_DATABASE_URL, then project locally"),
        ("export-kubernetes", "Read the Helm Postgres through a temporary kubectl tunnel"),
    ):
        command = commands.add_parser(name, help=help_text)
        command.add_argument("--user", required=True, help="Ferret username to export")
        command.add_argument("--model", required=True, help="One registered embedding model ID")
        command.add_argument("--output", required=True, type=Path, help="New private snapshot directory")
        if name == "export-kubernetes":
            command.add_argument("--context", required=True)
            command.add_argument("--namespace", required=True)
            command.add_argument("--deployment", required=True, help="Postgres deployment name")
    viewer = commands.add_parser("serve", help="Open an existing snapshot without database access")
    viewer.add_argument("snapshot", type=Path)
    viewer.add_argument("--port", type=int, default=5055)
    args = parser.parse_args()
    os.umask(0o077)
    if args.command == "serve":
        serve(args.snapshot, args.port)
        return
    if args.command == "export" and not os.environ.get("FERRET_DATABASE_URL"):
        parser.error("Set FERRET_DATABASE_URL to the Postgres connection URL")
    # A new directory prevents a failed export from corrupting an existing snapshot.
    args.output.mkdir(parents=True, mode=0o700, exist_ok=False)
    if args.command == "export-kubernetes":
        frame, total = read_kubernetes_documents(args)
    else:
        frame, total = read_documents(os.environ["FERRET_DATABASE_URL"], args.user, args.model)
    write_snapshot(frame, total, args.user, args.model, args.output)


if __name__ == "__main__":
    main()
